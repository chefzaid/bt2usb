//! Bounds for the connection parameters a peripheral may ask for.
//!
//! The bridge opens each link with a short connection interval, no peripheral
//! latency, and a 4-second supervision timeout. A peripheral can later ask for
//! other values. Granting a request unchanged would let a peripheral slow every
//! report with a long interval, or stretch the supervision timeout to 32
//! seconds, during which a key held when the link silently fails stays held on
//! the host. [`bound_request`] answers a request with the nearest values inside
//! [`ConnParamLimits`], while always meeting the Bluetooth Core rule that the
//! supervision timeout exceeds `(1 + latency) * interval * 2`. A peripheral
//! that will take nothing as fast as the bridge prefers gets an interval it
//! asked for, up to a cap, because some peripherals disconnect otherwise.
//!
//! This module is hardware-free; the SoftDevice event handler converts to and
//! from `ble_gap_conn_params_t`.

/// Connection parameters in the units the Bluetooth Core uses: intervals in
/// 1.25 ms units, latency in connection events, supervision timeout in 10 ms
/// units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ConnParams {
    pub min_interval: u16,
    pub max_interval: u16,
    pub latency: u16,
    pub supervision_timeout: u16,
}

/// The ranges the bridge grants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnParamLimits {
    /// Shortest connection interval (1.25 ms units).
    pub min_interval: u16,
    /// Longest connection interval (1.25 ms units) granted to a peripheral
    /// whose requested range reaches down to it.
    pub max_interval: u16,
    /// Longest connection interval (1.25 ms units) granted, as a single value,
    /// to a peripheral whose whole requested range is slower than
    /// `max_interval`.
    pub slow_request_max_interval: u16,
    /// Most connection events a peripheral may skip.
    pub max_latency: u16,
    /// Shortest supervision timeout (10 ms units).
    pub min_supervision_timeout: u16,
    /// Longest supervision timeout (10 ms units).
    pub max_supervision_timeout: u16,
}

/// Answer a peripheral's connection parameter request.
///
/// - The interval range is the overlap of the requested range and the
///   allowed one. A request entirely slower than the allowed range gets its
///   own shortest interval, capped at `slow_request_max_interval`, as a single
///   value; one entirely faster gets the shortest allowed interval.
/// - Latency is capped at `max_latency`, and lowered further if the longest
///   allowed supervision timeout could not otherwise satisfy the Core rule.
/// - The supervision timeout is the requested one, kept inside the allowed
///   range and raised if needed to satisfy the Core rule for the granted
///   latency and longest interval.
///
/// A request with its interval bounds reversed is read as the range between
/// them.
pub fn bound_request(request: ConnParams, limits: &ConnParamLimits) -> ConnParams {
    let (req_min, req_max) = requested_range(&request);

    let low = req_min.max(limits.min_interval);
    let high = req_max.min(limits.max_interval);
    let (min_interval, max_interval) = if low <= high {
        (low, high)
    } else if req_min > limits.max_interval {
        // Nothing the peripheral asked for is as fast as the bridge prefers.
        // Give it the fastest interval it did ask for, so a peripheral that
        // checks its interval does not disconnect, but never slower than the
        // cap.
        let interval = req_min.min(limits.slow_request_max_interval.max(limits.max_interval));
        (interval, interval)
    } else {
        (limits.min_interval, limits.min_interval)
    };

    let latency = request.latency.min(limits.max_latency).min(max_latency_for(
        limits.max_supervision_timeout,
        max_interval,
    ));

    let required = min_supervision_timeout(latency, max_interval);
    let supervision_timeout = request
        .supervision_timeout
        .max(limits.min_supervision_timeout)
        .max(required)
        .min(limits.max_supervision_timeout);

    ConnParams {
        min_interval,
        max_interval,
        latency,
        supervision_timeout,
    }
}

/// Whether the interval range `granted` lies inside the range `request` asked
/// for, reading reversed bounds as a range. A peripheral given an interval
/// outside its range may disconnect.
pub fn interval_within_request(request: ConnParams, granted: ConnParams) -> bool {
    let (low, high) = requested_range(&request);
    low <= granted.min_interval && granted.max_interval <= high
}

/// The interval range `request` asks for as `(shortest, longest)`, reading
/// reversed bounds as the range between them.
fn requested_range(request: &ConnParams) -> (u16, u16) {
    (
        request.min_interval.min(request.max_interval),
        request.min_interval.max(request.max_interval),
    )
}

/// The shortest supervision timeout (10 ms units) the Core rule allows:
/// `timeout * 10 ms > (1 + latency) * interval * 1.25 ms * 2`, that is
/// `timeout > (1 + latency) * interval / 4`.
pub fn min_supervision_timeout(latency: u16, max_interval: u16) -> u16 {
    let floor = (u32::from(latency) + 1) * u32::from(max_interval) / 4 + 1;
    u16::try_from(floor).unwrap_or(u16::MAX)
}

/// The largest latency for which `timeout` (10 ms units) still satisfies the
/// Core rule at `max_interval`.
fn max_latency_for(timeout: u16, max_interval: u16) -> u16 {
    if max_interval == 0 {
        return u16::MAX;
    }
    if timeout == 0 {
        // No latency satisfies the rule; the caller raises the timeout.
        return 0;
    }
    // With n = latency + 1, `min_supervision_timeout` <= timeout reads
    // floor(n * interval / 4) + 1 <= timeout, that is n * interval < 4 * timeout,
    // so the largest n is exactly (4 * timeout - 1) / interval.
    let events = (4 * u32::from(timeout) - 1) / u32::from(max_interval);
    u16::try_from(events.saturating_sub(1)).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bridge's configured limits: 7.5 to 15 ms, or up to 30 ms for a
    /// peripheral that asks only for slower intervals, latency up to 20,
    /// supervision timeout 1 to 4 s.
    const LIMITS: ConnParamLimits = ConnParamLimits {
        min_interval: 6,
        max_interval: 12,
        slow_request_max_interval: 24,
        max_latency: 20,
        min_supervision_timeout: 100,
        max_supervision_timeout: 400,
    };

    fn request(min: u16, max: u16, latency: u16, timeout: u16) -> ConnParams {
        ConnParams {
            min_interval: min,
            max_interval: max,
            latency,
            supervision_timeout: timeout,
        }
    }

    /// The Bluetooth Core rule, in microseconds:
    /// `timeout > (1 + latency) * max interval * 2`.
    fn meets_core_rule(p: ConnParams) -> bool {
        u32::from(p.supervision_timeout) * 10_000
            > (u32::from(p.latency) + 1) * u32::from(p.max_interval) * 1_250 * 2
    }

    /// Every answer must satisfy the Core rule and stay inside the limits.
    fn assert_valid(answer: ConnParams) {
        assert!(answer.min_interval <= answer.max_interval);
        assert!(answer.min_interval >= LIMITS.min_interval);
        assert!(answer.max_interval <= LIMITS.slow_request_max_interval);
        assert!(answer.latency <= LIMITS.max_latency);
        assert!(answer.supervision_timeout >= LIMITS.min_supervision_timeout);
        assert!(answer.supervision_timeout <= LIMITS.max_supervision_timeout);
        assert!(meets_core_rule(answer), "{answer:?} breaks the Core rule");
    }

    #[test]
    fn a_request_inside_the_limits_is_granted_unchanged() {
        let asked = request(6, 12, 4, 300);
        assert_eq!(bound_request(asked, &LIMITS), asked);
    }

    #[test]
    fn a_peripheral_that_wants_a_slower_interval_gets_its_shortest() {
        // 20 to 40 ms: nothing as fast as 15 ms. A peripheral that checks
        // its granted interval against this range would disconnect at 15 ms.
        let asked = request(16, 32, 0, 400);
        let answer = bound_request(asked, &LIMITS);
        assert_eq!((answer.min_interval, answer.max_interval), (16, 16));
        assert!(interval_within_request(asked, answer));
        assert_valid(answer);
    }

    #[test]
    fn a_very_long_interval_is_pulled_back_to_the_cap() {
        // 50 to 100 ms: slower than the bridge grants anyone.
        let asked = request(40, 80, 0, 400);
        let answer = bound_request(asked, &LIMITS);
        assert_eq!((answer.min_interval, answer.max_interval), (24, 24));
        assert!(!interval_within_request(asked, answer));
        assert_valid(answer);
        // A request whose fastest interval is the cap itself gets it, inside
        // its range.
        let asked = request(24, 40, 0, 400);
        let answer = bound_request(asked, &LIMITS);
        assert_eq!((answer.min_interval, answer.max_interval), (24, 24));
        assert!(interval_within_request(asked, answer));
    }

    #[test]
    fn interval_within_request_reads_reversed_bounds_as_a_range() {
        let granted = request(8, 10, 0, 400);
        assert!(interval_within_request(request(12, 6, 0, 400), granted));
        assert!(interval_within_request(request(8, 10, 0, 400), granted));
        assert!(!interval_within_request(request(9, 12, 0, 400), granted));
        assert!(!interval_within_request(request(6, 9, 0, 400), granted));
    }

    #[test]
    fn an_overlapping_interval_range_is_narrowed_to_the_overlap() {
        // A keyboard asking for 7.5 to 30 ms with latency 6 to save power.
        let asked = request(6, 24, 6, 430);
        let answer = bound_request(asked, &LIMITS);
        assert_eq!((answer.min_interval, answer.max_interval), (6, 12));
        assert!(interval_within_request(asked, answer));
        assert_eq!(answer.latency, 6);
        assert_valid(answer);
    }

    #[test]
    fn a_32_second_supervision_timeout_is_capped() {
        let answer = bound_request(request(6, 12, 0, 3200), &LIMITS);
        assert_eq!(answer.supervision_timeout, 400);
        assert_valid(answer);
    }

    #[test]
    fn a_very_short_supervision_timeout_is_raised_to_the_minimum() {
        let answer = bound_request(request(6, 12, 0, 10), &LIMITS);
        assert_eq!(answer.supervision_timeout, 100);
        assert_valid(answer);
    }

    #[test]
    fn latency_is_capped() {
        let answer = bound_request(request(6, 12, 499, 3200), &LIMITS);
        assert_eq!(answer.latency, 20);
        assert_valid(answer);
    }

    #[test]
    fn reversed_interval_bounds_are_read_as_a_range() {
        let answer = bound_request(request(12, 6, 0, 400), &LIMITS);
        assert_eq!((answer.min_interval, answer.max_interval), (6, 12));
        assert_valid(answer);
    }

    #[test]
    fn a_request_faster_than_allowed_gets_the_shortest_interval() {
        // Below 7.5 ms, which the SoftDevice cannot grant: 7.5 ms, outside the
        // requested range.
        let asked = request(4, 5, 0, 400);
        let answer = bound_request(asked, &LIMITS);
        assert_eq!((answer.min_interval, answer.max_interval), (6, 6));
        assert!(!interval_within_request(asked, answer));
        assert_valid(answer);
        // The same rule with a raised 15 ms floor.
        let limits = ConnParamLimits {
            min_interval: 12,
            ..LIMITS
        };
        let answer = bound_request(request(6, 8, 0, 400), &limits);
        assert_eq!((answer.min_interval, answer.max_interval), (12, 12));
    }

    #[test]
    fn latency_is_lowered_when_the_timeout_cap_cannot_cover_it() {
        // With a 1 s cap and a 15 ms interval, latency 20 would need more
        // than 630 ms; latency is fine. Shrink the cap to force a reduction.
        let limits = ConnParamLimits {
            max_supervision_timeout: 100,
            min_supervision_timeout: 10,
            max_latency: 499,
            ..LIMITS
        };
        let answer = bound_request(request(6, 12, 499, 3200), &limits);
        assert!(meets_core_rule(answer));
        assert_eq!(answer.supervision_timeout, 100);
        // The largest latency that still fits: (n) * 15 ms * 2 < 1 s → n <= 33.
        assert_eq!(answer.latency, 32);
    }

    #[test]
    fn the_timeout_is_raised_to_satisfy_the_core_rule() {
        // Latency 20 at 15 ms needs more than 630 ms; a 1 s floor covers it,
        // but a floor of 100 ms would not.
        let limits = ConnParamLimits {
            min_supervision_timeout: 10,
            ..LIMITS
        };
        let answer = bound_request(request(12, 12, 20, 10), &limits);
        assert_eq!(answer.supervision_timeout, min_supervision_timeout(20, 12));
        assert!(meets_core_rule(answer));
    }

    #[test]
    fn max_latency_for_is_the_largest_latency_the_timeout_covers() {
        for interval in [1, 2, 3, 5, 6, 7, 12, 13, 24, 100, 3200, u16::MAX] {
            assert_eq!(max_latency_for(0, interval), 0);
            for timeout in 1..=3300 {
                let latency = max_latency_for(timeout, interval);
                let fits = |latency| min_supervision_timeout(latency, interval) <= timeout;
                // Latency 0 is the floor even when the timeout cannot cover it;
                // the caller then raises the timeout.
                assert!(latency == 0 || fits(latency), "{timeout} {interval}");
                assert!(!fits(latency + 1), "{timeout} {interval}");
            }
        }
    }

    #[test]
    fn every_request_gets_a_valid_answer() {
        // Sample the Core's legal ranges (interval 6..=3200, latency 0..=499,
        // timeout 10..=3200) at every boundary the policy has, plus values a
        // peer could send outside them.
        const INTERVALS: [u16; 15] = [
            0,
            1,
            5,
            6,
            7,
            12,
            13,
            16,
            24,
            25,
            100,
            800,
            3200,
            3201,
            u16::MAX,
        ];
        const LATENCIES: [u16; 8] = [0, 1, 20, 21, 100, 499, 500, u16::MAX];
        const TIMEOUTS: [u16; 10] = [0, 9, 10, 99, 100, 101, 400, 401, 3200, u16::MAX];
        for min in INTERVALS {
            for max in INTERVALS {
                let (low, high) = requested_range(&request(min, max, 0, 0));
                for latency in LATENCIES {
                    for timeout in TIMEOUTS {
                        let asked = request(min, max, latency, timeout);
                        let answer = bound_request(asked, &LIMITS);
                        assert_valid(answer);
                        // A peripheral that accepts 15 ms is never slowed down.
                        if low <= LIMITS.max_interval {
                            assert!(answer.max_interval <= LIMITS.max_interval, "{asked:?}");
                        }
                        // Any request that reaches into the grantable range
                        // gets an interval it asked for.
                        if low <= LIMITS.slow_request_max_interval && high >= LIMITS.min_interval {
                            assert!(interval_within_request(asked, answer), "{asked:?}");
                        }
                    }
                }
            }
        }
    }
}
