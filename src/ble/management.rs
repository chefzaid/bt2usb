//! Management transaction primitives shared by the live BLE tasks and tests.

use crate::ble::coordinator::{ConnManager, MAX_CONNECTIONS};

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
    fn reset_waits_for_both_sources_and_ignores_invalid_slots() {
        let mut barrier = Quiescence::new([true, true], 2);
        assert!(!barrier.acknowledge(9, 2));
        assert!(barrier.acknowledge(1, 2));
        assert!(!barrier.complete());
        assert!(barrier.acknowledge(0, 2));
        assert!(barrier.complete());
    }
}
