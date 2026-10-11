//! Host tests for the diagnostics: the build identity the build script
//! supplies, the decoding of `POWER.RESETREAS`, and the event counters with
//! the policy for logging them.

use super::*;

fn causes(bits: u32) -> std::vec::Vec<ResetCause> {
    ResetReasons::from_register(bits).causes().collect()
}

#[test]
fn the_build_identity_names_a_commit_profile_and_log_filter() {
    assert_eq!(FIRMWARE_VERSION, env!("CARGO_PKG_VERSION"));
    let commit = SOURCE_COMMIT
        .strip_suffix("-dirty")
        .unwrap_or(SOURCE_COMMIT);
    let full = commit.len() == 40
        && commit
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    assert!(
        full || SOURCE_COMMIT == "unknown",
        "unexpected source commit {SOURCE_COMMIT:?}"
    );
    assert!(["debug", "release"].contains(&BUILD_PROFILE));
    assert!(!LOG_FILTER.is_empty());
}

#[test]
fn no_cause_bit_is_a_power_on_or_brown_out() {
    assert_eq!(causes(0), [ResetCause::PowerOnOrBrownout]);
    assert_eq!(ResetReasons::from_register(0).undefined_bits(), 0);
}

#[test]
fn each_defined_bit_decodes_to_its_cause() {
    let expected = [
        (0, ResetCause::ResetPin),
        (1, ResetCause::Watchdog),
        (2, ResetCause::SoftReset),
        (3, ResetCause::Lockup),
        (16, ResetCause::WakeFromOffGpio),
        (17, ResetCause::WakeFromOffLpcomp),
        (18, ResetCause::WakeFromOffDebug),
        (19, ResetCause::WakeFromOffNfc),
        (20, ResetCause::WakeFromOffVbus),
    ];
    for (bit, cause) in expected {
        assert_eq!(causes(1 << bit), [cause], "bit {bit}");
    }
}

#[test]
fn the_defined_mask_holds_exactly_the_cause_bits() {
    let bits = ResetCause::BITS.iter().fold(0, |mask, (bit, _)| mask | bit);
    assert_eq!(ResetReasons::DEFINED, bits);
}

#[test]
fn accumulated_causes_are_listed_in_bit_order() {
    // A probe's soft reset after a pin reset that was never cleared.
    assert_eq!(causes(0b101), [ResetCause::ResetPin, ResetCause::SoftReset]);
    assert_eq!(ResetReasons::from_register(0b101).bits(), 0b101);
}

#[test]
fn undefined_bits_are_kept_for_the_report() {
    let reasons = ResetReasons::from_register((1 << 8) | (1 << 2));
    assert_eq!(
        reasons.causes().collect::<std::vec::Vec<_>>(),
        [ResetCause::SoftReset]
    );
    assert_eq!(reasons.undefined_bits(), 1 << 8);
    // Only undefined bits: no defined cause, so it reads as a power-on reset.
    let odd = ResetReasons::from_register(1 << 31);
    assert_eq!(
        odd.causes().collect::<std::vec::Vec<_>>(),
        [ResetCause::PowerOnOrBrownout]
    );
    assert_eq!(odd.undefined_bits(), 1 << 31);
}

#[test]
fn every_cause_has_a_distinct_name() {
    let all = [
        ResetCause::PowerOnOrBrownout,
        ResetCause::ResetPin,
        ResetCause::Watchdog,
        ResetCause::SoftReset,
        ResetCause::Lockup,
        ResetCause::WakeFromOffGpio,
        ResetCause::WakeFromOffLpcomp,
        ResetCause::WakeFromOffDebug,
        ResetCause::WakeFromOffNfc,
        ResetCause::WakeFromOffVbus,
    ];
    for (i, a) in all.iter().enumerate() {
        assert!(!a.name().is_empty());
        for b in &all[i + 1..] {
            assert_ne!(a.name(), b.name());
        }
    }
}

/// A snapshot with `count` for each listed counter and zero for the rest.
fn counts(set: &[(Counter, u32)]) -> Snapshot {
    let counters = Counters::new();
    for &(counter, count) in set {
        counters.set(counter, count);
    }
    counters.snapshot()
}

#[test]
fn each_counter_has_its_own_slot_in_log_order() {
    for (index, counter) in Counter::ALL.into_iter().enumerate() {
        assert_eq!(counter as usize, index, "{counter:?}");
        let counters = Counters::new();
        counters.bump(counter);
        counters.bump(counter);
        let mut expected = [0; Counter::ALL.len()];
        expected[index] = 2;
        assert_eq!(counters.snapshot(), Snapshot(expected), "{counter:?}");
    }
}

#[test]
fn every_counter_has_a_distinct_name() {
    for (i, a) in Counter::ALL.iter().enumerate() {
        assert!(!a.name().is_empty());
        for b in &Counter::ALL[i + 1..] {
            assert_ne!(a.name(), b.name());
        }
    }
}

#[test]
fn a_count_stops_at_its_limit_instead_of_wrapping() {
    let counters = Counters::new();
    counters.set(Counter::UsbWriteFailures, u32::MAX - 1);
    counters.bump(Counter::UsbWriteFailures);
    counters.bump(Counter::UsbWriteFailures);
    counters.bump(Counter::LinksLost);
    assert_eq!(
        counters.snapshot(),
        counts(&[
            (Counter::UsbWriteFailures, u32::MAX),
            (Counter::LinksLost, 1)
        ])
    );
}

#[test]
fn counters_and_reports_start_at_zero() {
    assert_eq!(Counters::default().snapshot(), Snapshot::default());
    assert_eq!(COUNTERS.snapshot().0.len(), Counter::ALL.len());
    assert_eq!(CounterReport::default().poll(0, Snapshot::default()), None);
}

#[test]
fn counts_that_never_change_are_never_logged() {
    let mut report = CounterReport::new();
    for second in 0..600 {
        assert_eq!(report.poll(second * 1000, Snapshot::default()), None);
    }
}

#[test]
fn a_change_is_logged_at_once_then_at_most_once_per_interval() {
    let interval = DIAGNOSTICS_REPORT_INTERVAL_SECS * 1000;
    let mut report = CounterReport::new();
    let first = counts(&[(Counter::LinksLost, 1)]);
    assert_eq!(report.poll(5_000, first), Some(first));
    // Unchanged: nothing more, however long it stays so.
    assert_eq!(report.poll(5_000 + 3 * interval, first), None);

    // A change after a quiet spell is logged at once, and the next ones wait
    // out the interval; the line then carries the latest counts.
    let second = counts(&[(Counter::LinksLost, 2)]);
    let start = 5_000 + 4 * interval;
    assert_eq!(report.poll(start, second), Some(second));
    let third = counts(&[(Counter::LinksLost, 2), (Counter::ReconnectAttempts, 1)]);
    assert_eq!(report.poll(start + 1_000, third), None);
    let fourth = counts(&[(Counter::LinksLost, 2), (Counter::ReconnectAttempts, 3)]);
    assert_eq!(report.poll(start + interval - 1, fourth), None);
    assert_eq!(report.poll(start + interval, fourth), Some(fourth));
    assert_eq!(report.poll(start + 2 * interval, fourth), None);
}

#[test]
fn the_report_clock_cannot_overflow() {
    let mut report = CounterReport::new();
    let changed = counts(&[(Counter::FlashWriteRetries, 1)]);
    assert_eq!(report.poll(u64::MAX - 1, changed), Some(changed));
    // The next report time saturates instead of wrapping to the past, so the
    // next change still waits.
    let later = counts(&[(Counter::FlashWriteRetries, 2)]);
    assert_eq!(report.poll(u64::MAX - 1, later), None);
}
