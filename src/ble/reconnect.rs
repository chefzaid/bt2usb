//! Pure coordination of the connection slots' background reconnects.
//!
//! A slot that is reconnecting in the background registers the saved device it
//! is waiting for. Whichever slot holds the radio scans for every registered
//! device at once, so a device that is asleep cannot keep the radio from
//! finding the other slot's device. A sighting of another slot's device is
//! handed to that slot, which then connects to the live address without a scan
//! of its own. A device whose connection attempt just failed is left out of the
//! other slots' scans for a while, so a device that advertises but will not
//! connect cannot keep cutting those scans short.
//!
//! Resolving a rotating private address against a peer's identity key needs
//! the SoftDevice, so the caller passes the match as a closure; this module only
//! records registrations, sightings and timing, which keeps it host-testable.

use crate::ble::coordinator::MAX_CONNECTIONS;

/// How long a sighting stays usable, in milliseconds. The device was
/// advertising when it was seen, so a connection started shortly afterwards
/// finds it; an older sighting may carry a private address that has rotated.
pub const SIGHTING_TTL_MS: u64 = 2_000;

/// How a reconnect scan should share the radio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ScanDuty {
    /// A device was lost, or the bridge started, a short time ago: scan with a
    /// high duty cycle so it is found as soon as it advertises.
    Fast,
    /// Every registered device has been missing for longer than the fast
    /// window: scan with the low default duty cycle to save power.
    Slow,
}

#[derive(Clone, Copy, Debug)]
struct Entry<T, A> {
    target: T,
    since_ms: u64,
    sighting: Option<(A, u64)>,
    /// Until this time, other slots' scans ignore this target.
    holdoff_until_ms: u64,
}

/// Registered reconnect targets and pending sightings, one entry per slot.
///
/// `T` identifies a saved device (its stored address and, for a bonded peer,
/// its identity key); `A` is the over-the-air address type. Times are
/// milliseconds from any monotonic clock.
#[derive(Debug)]
pub struct ReconnectTable<T, A> {
    entries: [Option<Entry<T, A>>; MAX_CONNECTIONS],
    fast_window_ms: u64,
    failed_holdoff_ms: u64,
}

impl<T: Copy + PartialEq, A: Copy> ReconnectTable<T, A> {
    /// An empty table whose fast duty cycle lasts `fast_window_ms` after a
    /// target is registered, and which hides a target from the other slots'
    /// scans for `failed_holdoff_ms` after an attempt to connect to it fails.
    pub const fn new(fast_window_ms: u64, failed_holdoff_ms: u64) -> Self {
        Self {
            entries: [None; MAX_CONNECTIONS],
            fast_window_ms,
            failed_holdoff_ms,
        }
    }

    /// Record that `slot` is reconnecting to `target`.
    ///
    /// Registering the same target again, as each retry does, keeps the time
    /// the outage started, any pending sighting, and any holdoff after a failed
    /// attempt; a different target starts a new fast window with neither.
    pub fn register(&mut self, slot: usize, target: T, now_ms: u64) {
        let Some(entry) = self.entries.get_mut(slot) else {
            return;
        };
        if entry.as_ref().is_some_and(|e| e.target == target) {
            return;
        }
        *entry = Some(Entry {
            target,
            since_ms: now_ms,
            sighting: None,
            holdoff_until_ms: 0,
        });
    }

    /// `slot` stopped reconnecting: it connected, was given another command,
    /// or gave up. Its pending sighting is dropped with it.
    pub fn clear(&mut self, slot: usize) {
        if let Some(entry) = self.entries.get_mut(slot) {
            *entry = None;
        }
    }

    /// `slot` saw its device, or was handed a sighting, but could not connect
    /// to it. Until `failed_holdoff_ms` from now, the other slots' scans ignore
    /// that device, so one that advertises but will not connect, for example
    /// because it was paired again with another computer, cannot keep ending
    /// their scans at its first advertisement. The slot's own scans still look
    /// for it. Any pending sighting is dropped.
    pub fn attempt_failed(&mut self, slot: usize, now_ms: u64) {
        if let Some(Some(entry)) = self.entries.get_mut(slot) {
            entry.holdoff_until_ms = now_ms.saturating_add(self.failed_holdoff_ms);
            entry.sighting = None;
        }
    }

    /// The targets a scan run by `scanning_slot` looks for: its own, and each
    /// other slot's unless that slot's last attempt failed less than
    /// `failed_holdoff_ms` ago. The caller matches advertisements against a
    /// copy, because resolving a private address calls into the SoftDevice and
    /// must not run while the table is locked.
    pub fn targets(&self, scanning_slot: usize, now_ms: u64) -> [Option<T>; MAX_CONNECTIONS] {
        let mut targets = [None; MAX_CONNECTIONS];
        for (slot, (target, entry)) in targets.iter_mut().zip(&self.entries).enumerate() {
            *target = entry
                .as_ref()
                .filter(|e| slot == scanning_slot || now_ms >= e.holdoff_until_ms)
                .map(|e| e.target);
        }
        targets
    }

    /// Record that `slot`'s device was seen advertising at `address`.
    /// Returns `false`, recording nothing, when `slot` is not registered.
    pub fn record_sighting(&mut self, slot: usize, address: A, now_ms: u64) -> bool {
        match self.entries.get_mut(slot) {
            Some(Some(entry)) => {
                entry.sighting = Some((address, now_ms));
                true
            }
            _ => false,
        }
    }

    /// Take `slot`'s sighting if it is still fresh. A sighting is used at
    /// most once; a stale one is discarded.
    pub fn take_sighting(&mut self, slot: usize, now_ms: u64) -> Option<A> {
        let entry = self.entries.get_mut(slot)?.as_mut()?;
        let (address, seen_ms) = entry.sighting.take()?;
        (now_ms.saturating_sub(seen_ms) <= SIGHTING_TTL_MS).then_some(address)
    }

    /// The duty cycle the next reconnect scan should use: fast while any
    /// registered target is still inside its fast window.
    pub fn duty(&self, now_ms: u64) -> ScanDuty {
        let fast = self
            .entries
            .iter()
            .flatten()
            .any(|entry| now_ms.saturating_sub(entry.since_ms) < self.fast_window_ms);
        if fast {
            ScanDuty::Fast
        } else {
            ScanDuty::Slow
        }
    }
}

/// The slot whose target an advertisement from `address` belongs to, from a
/// copy of [`ReconnectTable::targets`]. When two slots would match, the lower
/// slot wins.
pub fn owner_of<T, A, F>(targets: &[Option<T>], address: A, matches: F) -> Option<usize>
where
    A: Copy,
    F: Fn(&T, A) -> bool,
{
    targets
        .iter()
        .position(|target| target.as_ref().is_some_and(|t| matches(t, address)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: u64 = 30_000;
    const HOLDOFF: u64 = 6_500;

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
        let slot = owner_of(&table.targets(scanning, now_ms), address, same)?;
        assert!(table.record_sighting(slot, address, now_ms));
        Some(slot)
    }

    /// An advertisement seen by slot 0's scan.
    fn see(table: &mut Table, address: u8, now_ms: u64) -> Option<usize> {
        see_from(table, 0, address, now_ms)
    }

    #[test]
    fn a_new_table_has_no_targets_and_scans_slowly() {
        let table = Table::new(WINDOW, HOLDOFF);
        assert_eq!(table.targets(0, 0), [None; MAX_CONNECTIONS]);
        assert_eq!(table.duty(0), ScanDuty::Slow);
    }

    #[test]
    fn a_sighting_goes_to_the_slot_that_owns_the_device() {
        let mut table = Table::new(WINDOW, HOLDOFF);
        table.register(0, 10, 0);
        table.register(1, 20, 0);
        // Slot 0's scan sees slot 1's device: it is recorded for slot 1.
        assert_eq!(see(&mut table, 20, 100), Some(1));
        assert_eq!(table.take_sighting(0, 100), None);
        assert_eq!(table.take_sighting(1, 150), Some(20));
    }

    #[test]
    fn unregistered_devices_are_ignored() {
        let mut table = Table::new(WINDOW, HOLDOFF);
        table.register(0, 10, 0);
        assert_eq!(see(&mut table, 99, 100), None);
        assert_eq!(table.take_sighting(0, 100), None);
    }

    #[test]
    fn a_sighting_for_an_unregistered_slot_is_not_recorded() {
        let mut table = Table::new(WINDOW, HOLDOFF);
        assert!(!table.record_sighting(0, 10, 100));
        assert!(!table.record_sighting(MAX_CONNECTIONS, 10, 100));
        assert_eq!(table.take_sighting(0, 100), None);
    }

    #[test]
    fn a_sighting_is_used_once() {
        let mut table = Table::new(WINDOW, HOLDOFF);
        table.register(0, 10, 0);
        see(&mut table, 10, 100);
        assert_eq!(table.take_sighting(0, 100), Some(10));
        assert_eq!(table.take_sighting(0, 100), None);
    }

    #[test]
    fn a_newer_sighting_replaces_an_older_one() {
        let mut table = Table::new(WINDOW, HOLDOFF);
        table.register(0, 10, 0);
        assert!(table.record_sighting(0, 10, 100));
        assert!(table.record_sighting(0, 11, 200));
        assert_eq!(table.take_sighting(0, 200), Some(11));
    }

    #[test]
    fn a_stale_sighting_is_discarded() {
        let mut table = Table::new(WINDOW, HOLDOFF);
        table.register(0, 10, 0);
        see(&mut table, 10, 100);
        assert_eq!(table.take_sighting(0, 100 + SIGHTING_TTL_MS + 1), None);
        // Discarded, not kept for later.
        assert_eq!(table.take_sighting(0, 100), None);
    }

    #[test]
    fn a_sighting_at_the_ttl_is_still_fresh() {
        let mut table = Table::new(WINDOW, HOLDOFF);
        table.register(0, 10, 0);
        see(&mut table, 10, 100);
        assert_eq!(table.take_sighting(0, 100 + SIGHTING_TTL_MS), Some(10));
    }

    #[test]
    fn clearing_a_slot_drops_its_target_and_sighting() {
        let mut table = Table::new(WINDOW, HOLDOFF);
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
        let mut table = Table::new(WINDOW, HOLDOFF);
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
        let mut table = Table::new(WINDOW, HOLDOFF);
        table.register(0, 10, 0);
        see(&mut table, 10, 100);
        // Still fresh, but it belongs to the previous device.
        table.register(0, 11, 150);
        assert_eq!(table.take_sighting(0, 150), None);
    }

    #[test]
    fn a_different_target_starts_a_new_window() {
        let mut table = Table::new(WINDOW, HOLDOFF);
        table.register(0, 10, 0);
        table.register(0, 11, 40_000);
        assert_eq!(table.duty(40_000 + WINDOW - 1), ScanDuty::Fast);
        assert_eq!(table.duty(40_000 + WINDOW), ScanDuty::Slow);
    }

    #[test]
    fn duty_is_fast_while_any_target_is_inside_its_window() {
        let mut table = Table::new(WINDOW, HOLDOFF);
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
        let mut table = Table::new(WINDOW, HOLDOFF);
        table.register(0, 10, 0);
        table.register(1, 10, 0);
        assert_eq!(see(&mut table, 10, 100), Some(0));
    }

    #[test]
    fn targets_reflect_registrations() {
        let mut table = Table::new(WINDOW, HOLDOFF);
        table.register(1, 20, 0);
        assert_eq!(table.targets(0, 0), [None, Some(20)]);
        assert_eq!(table.targets(1, 0), [None, Some(20)]);
    }

    #[test]
    fn a_failed_attempt_hides_the_device_from_other_slots_scans() {
        let mut table = Table::new(WINDOW, HOLDOFF);
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
        let mut table = Table::new(WINDOW, HOLDOFF);
        table.register(1, 20, 0);
        table.attempt_failed(1, 1_000);
        assert_eq!(table.targets(0, 1_000 + HOLDOFF - 1), [None, None]);
        assert_eq!(table.targets(0, 1_000 + HOLDOFF), [None, Some(20)]);
        assert_eq!(see(&mut table, 20, 1_000 + HOLDOFF), Some(1));
    }

    #[test]
    fn a_failed_attempt_drops_the_pending_sighting() {
        let mut table = Table::new(WINDOW, HOLDOFF);
        table.register(1, 20, 0);
        see(&mut table, 20, 100);
        table.attempt_failed(1, 200);
        assert_eq!(table.take_sighting(1, 200), None);
    }

    #[test]
    fn re_registering_keeps_the_holdoff_and_a_new_target_ends_it() {
        let mut table = Table::new(WINDOW, HOLDOFF);
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
        let mut table = Table::new(WINDOW, HOLDOFF);
        table.register(1, 20, 0);
        table.attempt_failed(1, 1_000);
        table.attempt_failed(1, 5_000);
        assert_eq!(table.targets(0, 1_000 + HOLDOFF), [None, None]);
        assert_eq!(table.targets(0, 5_000 + HOLDOFF), [None, Some(20)]);
    }

    #[test]
    fn a_failure_for_an_unregistered_slot_is_ignored() {
        let mut table = Table::new(WINDOW, HOLDOFF);
        table.attempt_failed(0, 1_000);
        table.attempt_failed(MAX_CONNECTIONS, 1_000);
        // A device registered afterwards is not held off.
        table.register(0, 10, 1_000);
        assert_eq!(table.targets(1, 1_000), [Some(10), None]);
    }

    #[test]
    fn out_of_range_slots_are_ignored() {
        let mut table = Table::new(WINDOW, HOLDOFF);
        table.register(MAX_CONNECTIONS, 10, 0);
        assert_eq!(table.targets(0, 0), [None; MAX_CONNECTIONS]);
        assert!(!table.record_sighting(MAX_CONNECTIONS, 10, 0));
        assert_eq!(table.take_sighting(MAX_CONNECTIONS, 0), None);
        table.clear(MAX_CONNECTIONS);
        assert_eq!(table.targets(MAX_CONNECTIONS, 0), [None; MAX_CONNECTIONS]);
    }

    #[test]
    fn the_clock_going_backwards_does_not_underflow() {
        let mut table = Table::new(WINDOW, HOLDOFF);
        table.register(0, 10, 1_000);
        see(&mut table, 10, 1_000);
        assert_eq!(table.duty(0), ScanDuty::Fast);
        assert_eq!(table.take_sighting(0, 0), Some(10));
    }
}
