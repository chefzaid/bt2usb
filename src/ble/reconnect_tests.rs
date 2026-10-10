//! Host tests for the pure background-reconnect table: registrations,
//! sightings and handovers, wakes, the failure holdoff, scan duty, and the
//! saved-device identity that decides when a retry is the same device.

use super::*;

const WINDOW: u64 = 30_000;
const HOLDOFF: u64 = 6_500;
const TTL: u64 = 2_000;

/// Targets and addresses are plain ids in these tests.
type Table = ReconnectTable<u8, u8>;

/// Targets are plain ids; an address "matches" a target with the same id.
fn same(target: &u8, address: u8) -> bool {
    *target == address
}

/// What slot `scanning`'s scan callback does with one advertisement:
/// match it against a copy of the targets, then record the sighting for
/// the owning slot.
fn see_from(table: &mut Table, scanning: usize, address: u8, now_ms: u64) -> Option<usize> {
    let owner = owner_of(&table.targets(scanning, now_ms), address, same)?;
    assert_ne!(
        table.record_sighting(scanning, owner, address, now_ms),
        Recorded::NotRegistered
    );
    Some(owner)
}

/// An advertisement seen by slot 0's scan.
fn see(table: &mut Table, address: u8, now_ms: u64) -> Option<usize> {
    see_from(table, 0, address, now_ms)
}

#[test]
fn a_new_table_has_no_targets_and_scans_slowly() {
    let table = Table::new(WINDOW, HOLDOFF, TTL);
    assert_eq!(table.targets(0, 0), [None; MAX_CONNECTIONS]);
    assert_eq!(table.duty(0), ScanDuty::Slow);
}

#[test]
fn a_sighting_goes_to_the_slot_that_owns_the_device() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(0, 10, 0);
    table.register(1, 20, 0);
    // Slot 0's scan sees slot 1's device: it is recorded for slot 1.
    assert_eq!(see(&mut table, 20, 100), Some(1));
    assert_eq!(table.take_sighting(0, 100), None);
    assert_eq!(table.take_sighting(1, 150), Some(20));
}

#[test]
fn unregistered_devices_are_ignored() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(0, 10, 0);
    assert_eq!(see(&mut table, 99, 100), None);
    assert_eq!(table.take_sighting(0, 100), None);
}

#[test]
fn a_sighting_for_an_unregistered_slot_is_not_recorded() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    assert_eq!(
        table.record_sighting(0, 0, 10, 100),
        Recorded::NotRegistered
    );
    assert_eq!(
        table.record_sighting(1, 0, 10, 100),
        Recorded::NotRegistered
    );
    assert_eq!(
        table.record_sighting(0, MAX_CONNECTIONS, 10, 100),
        Recorded::NotRegistered
    );
    assert_eq!(table.take_sighting(0, 100), None);
    assert!(!table.wake_pending(0));
}

#[test]
fn a_sighting_is_used_once() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(0, 10, 0);
    see(&mut table, 10, 100);
    assert_eq!(table.take_sighting(0, 100), Some(10));
    assert_eq!(table.take_sighting(0, 100), None);
}

#[test]
fn a_newer_sighting_replaces_an_older_one() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(0, 10, 0);
    assert_eq!(table.record_sighting(0, 0, 10, 100), Recorded::Own);
    assert_eq!(table.record_sighting(0, 0, 11, 200), Recorded::Own);
    assert_eq!(table.take_sighting(0, 200), Some(11));
}

#[test]
fn a_stale_sighting_is_discarded() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(0, 10, 0);
    see(&mut table, 10, 100);
    assert_eq!(table.take_sighting(0, 100 + TTL + 1), None);
    // Discarded, not kept for later.
    assert_eq!(table.take_sighting(0, 100), None);
}

#[test]
fn a_sighting_at_the_ttl_is_still_fresh() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(0, 10, 0);
    see(&mut table, 10, 100);
    assert_eq!(table.take_sighting(0, 100 + TTL), Some(10));
}

#[test]
fn clearing_a_slot_drops_its_target_and_sighting() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(1, 20, 0);
    see(&mut table, 20, 100);
    table.clear(1);
    assert_eq!(table.targets(0, 100), [None; MAX_CONNECTIONS]);
    assert_eq!(table.take_sighting(1, 100), None);
    // A cleared slot no longer claims its device's advertisements.
    assert_eq!(see(&mut table, 20, 200), None);
}

#[test]
fn re_registering_the_same_target_keeps_the_outage_start_and_sighting() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(0, 10, 0);
    see(&mut table, 10, 100);
    table.register(0, 10, 5_000);
    assert_eq!(table.take_sighting(0, 200), Some(10));
    // The fast window still counts from the first registration.
    assert_eq!(table.duty(WINDOW - 1), ScanDuty::Fast);
    assert_eq!(table.duty(WINDOW), ScanDuty::Slow);
}

#[test]
fn a_different_target_drops_the_old_sighting() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(0, 10, 0);
    see(&mut table, 10, 100);
    // Still fresh, but it belongs to the previous device.
    table.register(0, 11, 150);
    assert_eq!(table.take_sighting(0, 150), None);
}

#[test]
fn a_different_target_starts_a_new_window() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(0, 10, 0);
    table.register(0, 11, 40_000);
    assert_eq!(table.duty(40_000 + WINDOW - 1), ScanDuty::Fast);
    assert_eq!(table.duty(40_000 + WINDOW), ScanDuty::Slow);
}

#[test]
fn duty_is_fast_while_any_target_is_inside_its_window() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(0, 10, 0);
    assert_eq!(table.duty(WINDOW + 1), ScanDuty::Slow);
    // A second device lost later brings back the fast duty cycle.
    table.register(1, 20, WINDOW + 1);
    assert_eq!(table.duty(WINDOW + 2), ScanDuty::Fast);
    table.clear(1);
    assert_eq!(table.duty(WINDOW + 2), ScanDuty::Slow);
}

#[test]
fn the_lower_slot_wins_when_both_would_match() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(0, 10, 0);
    table.register(1, 10, 0);
    assert_eq!(see(&mut table, 10, 100), Some(0));
}

#[test]
fn targets_reflect_registrations() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(1, 20, 0);
    assert_eq!(table.targets(0, 0), [None, Some(20)]);
    assert_eq!(table.targets(1, 0), [None, Some(20)]);
}

#[test]
fn a_failed_attempt_hides_the_device_from_other_slots_scans() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(0, 10, 0);
    table.register(1, 20, 0);
    table.attempt_failed(1, 1_000);
    // Slot 0's scan keeps looking for its own device instead of stopping
    // at the advertisement of a device that just failed to connect.
    assert_eq!(table.targets(0, 1_000), [Some(10), None]);
    assert_eq!(see_from(&mut table, 0, 20, 1_500), None);
    assert_eq!(see_from(&mut table, 0, 10, 1_600), Some(0));
    // Slot 1's own scan still looks for it, and for slot 0's device.
    assert_eq!(table.targets(1, 1_500), [Some(10), Some(20)]);
    assert_eq!(see_from(&mut table, 1, 20, 1_700), Some(1));
}

#[test]
fn the_holdoff_ends_after_its_duration() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(1, 20, 0);
    table.attempt_failed(1, 1_000);
    assert_eq!(table.targets(0, 1_000 + HOLDOFF - 1), [None, None]);
    assert_eq!(table.targets(0, 1_000 + HOLDOFF), [None, Some(20)]);
    assert_eq!(see(&mut table, 20, 1_000 + HOLDOFF), Some(1));
}

#[test]
fn a_failed_attempt_drops_the_pending_sighting() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(1, 20, 0);
    see(&mut table, 20, 100);
    table.attempt_failed(1, 200);
    assert_eq!(table.take_sighting(1, 200), None);
}

#[test]
fn re_registering_keeps_the_holdoff_and_a_new_target_ends_it() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(1, 20, 0);
    table.attempt_failed(1, 1_000);
    // The retry after the pause registers the same device again.
    table.register(1, 20, 1_500);
    assert_eq!(table.targets(0, 1_500), [None, None]);
    // A different device has failed nothing yet.
    table.register(1, 21, 2_000);
    assert_eq!(table.targets(0, 2_000), [None, Some(21)]);
}

#[test]
fn a_new_failure_extends_the_holdoff() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(1, 20, 0);
    table.attempt_failed(1, 1_000);
    table.attempt_failed(1, 5_000);
    assert_eq!(table.targets(0, 1_000 + HOLDOFF), [None, None]);
    assert_eq!(table.targets(0, 5_000 + HOLDOFF), [None, Some(20)]);
}

#[test]
fn a_failure_for_an_unregistered_slot_is_ignored() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.attempt_failed(0, 1_000);
    table.attempt_failed(MAX_CONNECTIONS, 1_000);
    // A device registered afterwards is not held off.
    table.register(0, 10, 1_000);
    assert_eq!(table.targets(1, 1_000), [Some(10), None]);
}

#[test]
fn out_of_range_slots_are_ignored() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(MAX_CONNECTIONS, 10, 0);
    assert_eq!(table.targets(0, 0), [None; MAX_CONNECTIONS]);
    assert_eq!(
        table.record_sighting(0, MAX_CONNECTIONS, 10, 0),
        Recorded::NotRegistered
    );
    assert_eq!(table.take_sighting(MAX_CONNECTIONS, 0), None);
    assert!(!table.wake_pending(MAX_CONNECTIONS));
    table.clear(MAX_CONNECTIONS);
    assert_eq!(table.targets(MAX_CONNECTIONS, 0), [None; MAX_CONNECTIONS]);
}

#[test]
fn the_clock_going_backwards_does_not_underflow() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(0, 10, 1_000);
    see(&mut table, 10, 1_000);
    assert_eq!(table.duty(0), ScanDuty::Fast);
    assert_eq!(table.take_sighting(0, 0), Some(10));
}

// ── Wakes ───────────────────────────────────────────────────────────────────

#[test]
fn a_handover_wakes_only_the_owner() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(0, 10, 0);
    table.register(1, 20, 0);
    assert_eq!(table.record_sighting(0, 1, 20, 100), Recorded::HandedOver);
    assert!(table.wake_pending(1));
    assert!(!table.wake_pending(0));
    // A slot that sees its own device connects at once; nobody is woken.
    assert_eq!(table.record_sighting(0, 0, 10, 150), Recorded::Own);
    assert!(!table.wake_pending(0));
    assert!(table.wake_pending(1));
}

#[test]
fn taking_the_sighting_ends_the_wake_fresh_or_stale() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(1, 20, 0);
    table.record_sighting(0, 1, 20, 100);
    assert_eq!(table.take_sighting(1, 200), Some(20));
    assert!(!table.wake_pending(1));
    table.record_sighting(0, 1, 20, 300);
    assert_eq!(table.take_sighting(1, 300 + TTL + 1), None);
    assert!(!table.wake_pending(1));
}

#[test]
fn a_failed_attempt_or_clearing_ends_the_wake() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(1, 20, 0);
    table.record_sighting(0, 1, 20, 100);
    table.attempt_failed(1, 200);
    assert!(!table.wake_pending(1));
    table.record_sighting(0, 1, 20, 300);
    table.clear(1);
    assert!(!table.wake_pending(1));
}

#[test]
fn re_registering_keeps_the_wake_and_a_new_target_ends_it() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(1, 20, 0);
    table.record_sighting(0, 1, 20, 100);
    table.register(1, 20, 150);
    assert!(table.wake_pending(1));
    table.register(1, 21, 200);
    assert!(!table.wake_pending(1));
}

#[test]
fn a_sighting_for_a_slot_cleared_meanwhile_wakes_nobody() {
    let mut table = Table::new(WINDOW, HOLDOFF, TTL);
    table.register(1, 20, 0);
    // Slot 0's scan matched against a copy taken before slot 1 stopped.
    let owner = owner_of(&table.targets(0, 100), 20, same);
    table.clear(1);
    assert_eq!(owner, Some(1));
    assert_eq!(
        table.record_sighting(0, 1, 20, 100),
        Recorded::NotRegistered
    );
    assert!(!table.wake_pending(1));
}

// ── Saved-device identity ───────────────────────────────────────────────────

/// Addresses and identity keys are plain ids; a key resolves the address with
/// the same id plus 100.
type Peer = SavedPeer<u8, u8>;

fn resolves(key: &u8, address: u8) -> bool {
    address == key + 100
}

fn bonded(address: u8, key: u8) -> Peer {
    Peer {
        address,
        identity: Some(key),
    }
}

fn unbonded(address: u8) -> Peer {
    Peer {
        address,
        identity: None,
    }
}

#[test]
fn a_saved_peer_is_the_same_device_by_identity_key_or_by_address() {
    // A bonded peer keeps its identity across private addresses.
    assert_eq!(bonded(1, 7), bonded(2, 7));
    assert_ne!(bonded(1, 7), bonded(1, 8));
    // A record without a bond is known only by its stored address.
    assert_eq!(unbonded(1), unbonded(1));
    assert_ne!(unbonded(1), unbonded(2));
    // A bond added or lost makes it another record, even at one address.
    assert_ne!(bonded(1, 7), unbonded(1));
    assert_ne!(unbonded(1), bonded(1, 7));
}

#[test]
fn a_saved_peer_matches_its_resolved_or_stored_address() {
    assert!(bonded(1, 7).matches(107, resolves));
    assert!(bonded(1, 7).matches(1, resolves));
    assert!(!bonded(1, 7).matches(108, resolves));
    assert!(unbonded(1).matches(1, resolves));
    assert!(!unbonded(1).matches(101, resolves));
}

#[test]
fn a_retry_at_a_new_private_address_keeps_the_holdoff_and_window() {
    let mut table = ReconnectTable::<Peer, u8>::new(WINDOW, HOLDOFF, TTL);
    table.register(1, bonded(20, 7), 0);
    table.attempt_failed(1, 1_000);
    // The slot retries with the live address it last connected to.
    table.register(1, bonded(107, 7), 1_500);
    assert_eq!(table.targets(0, 1_500), [None, None]);
    assert_eq!(table.duty(WINDOW - 1), ScanDuty::Fast);
    assert_eq!(table.duty(WINDOW), ScanDuty::Slow);
    // A peer bonded under another identity is a new device.
    table.register(1, bonded(107, 8), 2_000);
    assert_eq!(table.targets(0, 2_000), [None, Some(bonded(107, 8))]);
    assert_eq!(table.duty(WINDOW), ScanDuty::Fast);
}
