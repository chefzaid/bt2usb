//! Pure planner for boot-time auto-reconnect with rotating-address resolution.
//!
//! A bonded peer that uses a **Resolvable Private Address (RPA)** advertises
//! under a random address that rotates over time, so the address we stored at
//! pairing time goes stale. Connecting to that stale address (via a whitelist)
//! then never matches and the device fails to auto-reconnect.
//!
//! The fix is to scan first and identify each stored peer in the live scan
//! results by resolving the advertised RPA against the peer's IRK, then connect
//! to its *current* address. The cryptographic resolution itself lives in the
//! SoftDevice (`nrf_softdevice::ble::IdentityKey::is_match`); this module is the
//! hardware-free "functional core" that only *sequences* the decisions — which
//! stored peer maps to which scan result, deduping and capping to the available
//! slots — so it can be unit-tested on the host.

use crate::ble::coordinator::MAX_CONNECTIONS;
use heapless::Vec;

/// One resolved auto-reconnect target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReconnectTarget {
    /// Index into the caller's recency-ordered list of stored peers.
    pub peer: usize,
    /// Index into the scan results to connect to (the peer's *live* address),
    /// or `None` when the peer wasn't seen in the scan — in which case the
    /// caller falls back to the stored address (still correct for static
    /// addresses, and harmless for a device that is simply not advertising).
    pub scanned: Option<usize>,
}

/// Decide which stored peers to auto-reconnect after boot, and at which address.
///
/// `peer_count` stored devices are considered in the caller's order (most
/// recently used first). `matches(peer, scanned)` reports whether scan result
/// `scanned` is `peer` — by resolving a rotating RPA against the peer's IRK, or
/// by a plain address match for a stable address.
///
/// Returns up to [`MAX_CONNECTIONS`] targets (one per connection slot), never
/// assigning the same scan result to two different peers.
pub fn resolve_reconnect_targets<F>(
    peer_count: usize,
    scanned_count: usize,
    matches: F,
) -> Vec<ReconnectTarget, MAX_CONNECTIONS>
where
    F: Fn(usize, usize) -> bool,
{
    let mut targets: Vec<ReconnectTarget, MAX_CONNECTIONS> = Vec::new();
    // Bitset of scan results already claimed by an earlier peer. Scans never
    // exceed `BLE_MAX_DISCOVERED` (8) entries, so a u32 is ample.
    let mut claimed: u32 = 0;

    for peer in 0..peer_count {
        if targets.is_full() {
            break;
        }

        let mut scanned = None;
        for s in 0..scanned_count.min(u32::BITS as usize) {
            let bit = 1u32 << s;
            if claimed & bit == 0 && matches(peer, s) {
                claimed |= bit;
                scanned = Some(s);
                break;
            }
        }

        let _ = targets.push(ReconnectTarget { peer, scanned });
    }

    targets
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a matcher from an explicit `(peer, scanned)` truth table.
    fn matcher(pairs: &[(usize, usize)]) -> impl Fn(usize, usize) -> bool + '_ {
        move |p, s| pairs.contains(&(p, s))
    }

    #[test]
    fn no_peers_yields_no_targets() {
        let targets = resolve_reconnect_targets(0, 3, |_, _| true);
        assert!(targets.is_empty());
    }

    #[test]
    fn unmatched_peers_fall_back_to_stored_address() {
        // Nothing in the scan matches: each peer still gets a target with no
        // live address, so the caller connects to the stored one.
        let targets = resolve_reconnect_targets(2, 4, |_, _| false);
        assert_eq!(targets.len(), 2);
        assert_eq!(
            targets[0],
            ReconnectTarget {
                peer: 0,
                scanned: None
            }
        );
        assert_eq!(
            targets[1],
            ReconnectTarget {
                peer: 1,
                scanned: None
            }
        );
    }

    #[test]
    fn resolved_rpa_uses_live_scan_address() {
        // Peer 0 resolves to scan result 2 (its rotated RPA).
        let targets = resolve_reconnect_targets(1, 3, matcher(&[(0, 2)]));
        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0],
            ReconnectTarget {
                peer: 0,
                scanned: Some(2)
            }
        );
    }

    #[test]
    fn a_scan_result_is_not_claimed_by_two_peers() {
        // Both peers would match scan results 0 and 1; each must take a
        // distinct one (peer 0 → 0, peer 1 → 1).
        let targets = resolve_reconnect_targets(2, 2, matcher(&[(0, 0), (0, 1), (1, 0), (1, 1)]));
        assert_eq!(
            targets[0],
            ReconnectTarget {
                peer: 0,
                scanned: Some(0)
            }
        );
        assert_eq!(
            targets[1],
            ReconnectTarget {
                peer: 1,
                scanned: Some(1)
            }
        );
    }

    #[test]
    fn targets_are_capped_to_connection_slots() {
        // More stored peers than slots: only MAX_CONNECTIONS come back.
        let targets = resolve_reconnect_targets(MAX_CONNECTIONS + 2, 0, |_, _| false);
        assert_eq!(targets.len(), MAX_CONNECTIONS);
    }

    #[test]
    fn mixes_resolved_and_fallback_targets() {
        // Peer 0 isn't in the scan (fallback); peer 1 resolves to scan 0.
        let targets = resolve_reconnect_targets(2, 2, matcher(&[(1, 0)]));
        assert_eq!(
            targets[0],
            ReconnectTarget {
                peer: 0,
                scanned: None
            }
        );
        assert_eq!(
            targets[1],
            ReconnectTarget {
                peer: 1,
                scanned: Some(0)
            }
        );
    }
}
