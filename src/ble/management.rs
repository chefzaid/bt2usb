//! Management transaction primitives shared by the live BLE tasks and tests,
//! and what the user is told once a connected device is saved.

use crate::ble::coordinator::{ConnManager, ErrorTag, MAX_CONNECTIONS};
use crate::ble::messages::{Event, PairingNotice};

/// A command-specific barrier prevents a queued Connected/LinkLost event from
/// recreating a forgotten bond while connection workers are cancelling. Workers
/// acknowledge the unique token only after releasing their input source and
/// dropping their retry target. An ordinary Disconnected event is not an ack.
pub struct Quiescence {
    targets: [bool; MAX_CONNECTIONS],
    pending: [bool; MAX_CONNECTIONS],
    token: u32,
}

impl Quiescence {
    pub const fn new(targets: [bool; MAX_CONNECTIONS], token: u32) -> Self {
        Self {
            targets,
            pending: targets,
            token,
        }
    }

    pub fn suppresses(&self, slot: usize) -> bool {
        self.targets.get(slot).copied().unwrap_or(false)
    }

    pub fn acknowledge(&mut self, slot: usize, token: u32) -> bool {
        match self.pending.get_mut(slot) {
            Some(pending) if token == self.token && *pending => {
                *pending = false;
                true
            }
            _ => false,
        }
    }

    pub fn complete(&self) -> bool {
        !self.pending.iter().any(|&pending| pending)
    }
}

/// The slots a Forget must quiesce before the store changes: those whose
/// connected or reserved address belongs to the forgotten peer, as `is_peer`
/// decides (the firmware also resolves the peer's private addresses there).
/// A factory reset quiesces every slot instead.
pub fn forget_targets<A: Clone + PartialEq>(
    manager: &ConnManager<A>,
    is_peer: impl Fn(&A) -> bool,
) -> [bool; MAX_CONNECTIONS] {
    core::array::from_fn(|slot| manager.slot_address(slot).is_some_and(&is_peer))
}

/// What adding a connected device did to the saved devices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stored<A> {
    /// Already saved as given.
    Unchanged,
    /// Saved as a new device, or over a saved one's address, name, or missing
    /// keys.
    Saved,
    /// Saved over the different keys held for the same identity.
    ReplacedBond,
    /// Saved after removing the oldest device to make room. Holds that
    /// device's identity address when it had keys, which the bonder must drop
    /// as well.
    RemovedOldest(Option<A>),
}

/// What the user is told once the store holding a connected device was
/// written. `stored` is what adding the device did, `None` when the store
/// refused it (reported already as `BondRefused`), and `saved` whether the
/// write reached flash. A pairing's effect on another device is told only
/// once it is saved; a new or changed device that did not reach flash is lost
/// at power-off.
pub fn persist_report<A, B>(stored: Option<&Stored<A>>, saved: bool) -> Option<Event<B>> {
    match (saved, stored) {
        (true, Some(Stored::ReplacedBond)) => Some(Event::Notice(PairingNotice::ReplacedBond)),
        (true, Some(Stored::RemovedOldest(_))) => Some(Event::Notice(PairingNotice::RemovedOldest)),
        (true, _) => None,
        (false, Some(Stored::Saved | Stored::ReplacedBond | Stored::RemovedOldest(_))) => {
            Some(Event::Error(ErrorTag::NotPersisted))
        }
        (false, _) => Some(Event::Error(ErrorTag::StorageFailed)),
    }
}

/// Publish a new in-memory store only after persistence succeeds. The live
/// flash adapter and host fault-injection tests exercise this same boundary.
pub async fn commit<T, E>(
    current: &mut T,
    mut candidate: T,
    persist: impl AsyncFnOnce(&mut T) -> Result<(), E>,
) -> Result<(), E> {
    persist(&mut candidate).await?;
    *current = candidate;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn immediate<F: core::future::Future>(future: F) -> F::Output {
        let mut future = core::pin::pin!(future);
        let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
        match future.as_mut().poll(&mut cx) {
            core::task::Poll::Ready(value) => value,
            core::task::Poll::Pending => panic!("fake persistence must finish immediately"),
        }
    }

    #[test]
    fn failed_deletion_preserves_current_store_and_bonds() {
        let mut current = [Some(10), Some(20)];
        let candidate = [None, Some(20)];
        assert_eq!(
            immediate(commit(&mut current, candidate, async |_| Err("flash"))),
            Err("flash")
        );
        assert_eq!(current, [Some(10), Some(20)]);
    }

    #[test]
    fn cancelled_persistence_never_publishes_candidate() {
        let mut current = [Some(10), Some(20)];
        {
            let future = commit(&mut current, [None, None], async |_| {
                core::future::pending::<()>().await;
                Ok::<_, ()>(())
            });
            let mut future = core::pin::pin!(future);
            let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
            assert!(core::future::Future::poll(future.as_mut(), &mut cx).is_pending());
        }
        assert_eq!(current, [Some(10), Some(20)]);
    }

    #[test]
    fn successful_deletion_commits_only_persisted_state() {
        let mut current = [Some(10), Some(20)];
        let mut written = None;
        let result = immediate(commit(&mut current, [None, Some(20)], async |candidate| {
            written = Some(*candidate);
            Ok::<_, ()>(())
        }));
        assert_eq!(result, Ok(()));
        assert_eq!(written, Some(current));
        assert_eq!(current, [None, Some(20)]);
    }

    #[test]
    fn reconnect_events_are_suppressed_until_matching_cancellation_ack() {
        let mut barrier = Quiescence::new([true, false], 41);
        assert!(barrier.suppresses(0)); // Connected and LinkLost both discarded.
        assert!(!barrier.suppresses(1)); // Unaffected mouse can stay connected.
        assert!(!barrier.acknowledge(0, 40)); // Old management ack is not enough.
        assert!(!barrier.acknowledge(1, 41));
        assert!(!barrier.complete());
        assert!(barrier.acknowledge(0, 41));
        assert!(barrier.complete());
        assert!(barrier.suppresses(0));
    }

    #[test]
    fn forget_targets_connected_and_reconnecting_slots_of_the_peer_only() {
        use crate::ble::coordinator::DeviceInfo;
        let device = |address: u8| DeviceInfo {
            address,
            name: heapless::String::new(),
            rssi: -50,
        };
        let mut manager = ConnManager::new();
        assert_eq!(forget_targets(&manager, |_: &u8| true), [false, false]);
        manager.connect_slot(0, &device(7));
        manager.reserve_retry(1, &device(9)); // link lost, retrying in the background
        assert_eq!(forget_targets(&manager, |a| *a == 9), [false, true]);
        assert_eq!(forget_targets(&manager, |a| *a == 7), [true, false]);
        assert_eq!(forget_targets(&manager, |a| *a == 8), [false, false]);
    }

    #[test]
    fn a_saved_pairing_tells_only_what_it_did_to_another_device() {
        let told = |stored: Option<Stored<u8>>| persist_report::<u8, u8>(stored.as_ref(), true);
        assert_eq!(
            told(Some(Stored::ReplacedBond)),
            Some(Event::Notice(PairingNotice::ReplacedBond))
        );
        for evicted in [None, Some(3)] {
            assert_eq!(
                told(Some(Stored::RemovedOldest(evicted))),
                Some(Event::Notice(PairingNotice::RemovedOldest))
            );
        }
        // Refused devices were reported as `BondRefused` already.
        for stored in [None, Some(Stored::Unchanged), Some(Stored::Saved)] {
            assert_eq!(told(stored), None);
        }
    }

    #[test]
    fn a_failed_write_says_whether_the_connected_device_is_lost_at_power_off() {
        let failed = |stored: Option<Stored<u8>>| persist_report::<u8, u8>(stored.as_ref(), false);
        for stored in [
            Stored::Saved,
            Stored::ReplacedBond,
            Stored::RemovedOldest(Some(3)),
        ] {
            assert_eq!(
                failed(Some(stored)),
                Some(Event::Error(ErrorTag::NotPersisted))
            );
        }
        // Flash already held this device, or the store refused it: the write
        // failed for the rest of the store.
        for stored in [None, Some(Stored::Unchanged)] {
            assert_eq!(failed(stored), Some(Event::Error(ErrorTag::StorageFailed)));
        }
    }

    #[test]
    fn reset_waits_for_both_sources_and_ignores_invalid_slots() {
        let mut barrier = Quiescence::new([true, true], 2);
        assert!(!barrier.acknowledge(9, 2));
        assert!(barrier.acknowledge(1, 2));
        assert!(!barrier.complete());
        assert!(barrier.acknowledge(0, 2));
        assert!(barrier.complete());
    }
}
