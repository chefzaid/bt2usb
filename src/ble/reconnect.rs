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
//! A slot waiting between attempts is woken when another slot's scan hands it
//! a sighting; [`ReconnectTable::wake_pending`] says when that wake is due and
//! when it lapses, and the firmware's scanner keeps one signal per slot equal
//! to it.
//!
//! Resolving a rotating private address against a peer's identity key needs
//! the SoftDevice, so the caller passes the match as a closure; this module only
//! records registrations, sightings, wakes and timing, which keeps it
//! host-testable.

use crate::ble::coordinator::MAX_CONNECTIONS;

/// A saved device a connection slot is reconnecting to in the background.
///
/// `A` is the over-the-air address type and `K` the identity key a bond
/// stores, which resolves the device's rotating private addresses.
#[derive(Clone, Copy, Debug)]
pub struct SavedPeer<A, K> {
    /// The address stored when the device was saved, or its last live address.
    pub address: A,
    /// The bonded peer's identity key; `None` for a record saved without a
    /// bond.
    pub identity: Option<K>,
}

impl<A: Copy + PartialEq, K> SavedPeer<A, K> {
    /// Whether an advertisement from `seen` comes from this device: its
    /// identity key resolves `seen`, or `seen` is the stored address.
    /// `resolves` checks a key against an address; on the firmware it calls
    /// into the SoftDevice.
    pub fn matches(&self, seen: A, resolves: impl Fn(&K, A) -> bool) -> bool {
        self.identity
            .as_ref()
            .is_some_and(|key| resolves(key, seen))
            || self.address == seen
    }
}

impl<A: PartialEq, K: PartialEq> PartialEq for SavedPeer<A, K> {
    /// The same device: a bonded peer by its identity key, whatever private
    /// address it last used, and any other record by its stored address. A slot
    /// registers its device again before every attempt, so this decides
    /// whether the attempt keeps the failure holdoff and the fast-scan window.
    fn eq(&self, other: &Self) -> bool {
        match (&self.identity, &other.identity) {
            (Some(a), Some(b)) => a == b,
            (None, None) => self.address == other.address,
            _ => false,
        }
    }
}

/// What [`ReconnectTable::record_sighting`] did with an advertisement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recorded {
    /// The scanning slot saw its own device and connects to it next.
    Own,
    /// The scanning slot saw another slot's device and handed the sighting
    /// over; that slot is now due a wake ([`ReconnectTable::wake_pending`]).
    HandedOver,
    /// The owning slot is no longer registered; nothing was recorded, and the
    /// scan should go on.
    NotRegistered,
}

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
struct Sighting<A> {
    address: A,
    seen_ms: u64,
    /// Recorded by another slot's scan, so the owning slot is due a wake.
    handed_over: bool,
}

#[derive(Clone, Copy, Debug)]
struct Entry<T, A> {
    target: T,
    since_ms: u64,
    sighting: Option<Sighting<A>>,
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
    sighting_ttl_ms: u64,
}

impl<T: Copy + PartialEq, A: Copy> ReconnectTable<T, A> {
    /// An empty table whose fast duty cycle lasts `fast_window_ms` after a
    /// target is registered, which hides a target from the other slots' scans
    /// for `failed_holdoff_ms` after an attempt to connect to it fails, and
    /// whose sightings stay usable for `sighting_ttl_ms`.
    pub const fn new(fast_window_ms: u64, failed_holdoff_ms: u64, sighting_ttl_ms: u64) -> Self {
        Self {
            entries: [None; MAX_CONNECTIONS],
            fast_window_ms,
            failed_holdoff_ms,
            sighting_ttl_ms,
        }
    }

    /// Record that `slot` is reconnecting to `target`.
    ///
    /// Registering the same target again, as each retry does, keeps the time
    /// the outage started, any pending sighting and the wake due for it, and
    /// any holdoff after a failed attempt; a different target starts a new fast
    /// window with none of them.
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
    /// or gave up. Its pending sighting, and any wake due for it, is dropped.
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
    /// for it. Any pending sighting, and the wake due for it, is dropped.
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

    /// Record that `scanning_slot`'s scan saw `owner`'s device advertising at
    /// `address`, replacing any earlier sighting for `owner`. When `owner` is
    /// another slot, the sighting is handed over and `owner` is due a wake.
    pub fn record_sighting(
        &mut self,
        scanning_slot: usize,
        owner: usize,
        address: A,
        now_ms: u64,
    ) -> Recorded {
        let Some(Some(entry)) = self.entries.get_mut(owner) else {
            return Recorded::NotRegistered;
        };
        let handed_over = owner != scanning_slot;
        entry.sighting = Some(Sighting {
            address,
            seen_ms: now_ms,
            handed_over,
        });
        if handed_over {
            Recorded::HandedOver
        } else {
            Recorded::Own
        }
    }

    /// Take `slot`'s sighting if it is at most `sighting_ttl_ms` old. A
    /// sighting is used at
    /// most once; a stale one is discarded. Either way, any wake due for it
    /// lapses.
    pub fn take_sighting(&mut self, slot: usize, now_ms: u64) -> Option<A> {
        let entry = self.entries.get_mut(slot)?.as_mut()?;
        let sighting = entry.sighting.take()?;
        (now_ms.saturating_sub(sighting.seen_ms) <= self.sighting_ttl_ms)
            .then_some(sighting.address)
    }

    /// Whether `slot`, if it is pausing between attempts, should be woken:
    /// another slot's scan handed it a sighting it has not taken yet. The wake
    /// lapses when the slot takes the sighting, when its attempt fails, when it
    /// is cleared, and when it registers a different device.
    pub fn wake_pending(&self, slot: usize) -> bool {
        self.entries
            .get(slot)
            .and_then(Option::as_ref)
            .and_then(|entry| entry.sighting.as_ref())
            .is_some_and(|sighting| sighting.handed_over)
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
#[path = "reconnect_tests.rs"]
mod tests;
