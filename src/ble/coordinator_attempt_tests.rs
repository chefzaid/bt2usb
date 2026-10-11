//! Host tests for attempt numbers and retry takeovers: a selection that takes
//! over a background reconnect, reports from an attempt the coordinator has
//! since replaced, a failed takeover handing the slot back to the retry, and
//! a reserved slot freed when its attempt ends without a link.

use super::*;

/// A stand-in for identity resolution: 0x1N and 0x2N are two addresses of
/// the same bonded peer N.
fn same_bonded_peer(a: &Addr, b: &Addr) -> bool {
    a == b || (a & 0x0F == b & 0x0F && a & 0xF0 != 0 && b & 0xF0 != 0)
}

/// Put slot 0 in a background retry for `kb`, at power-up (`false`) or
/// after its link was lost (`true`), and return the retry's attempt number.
fn retrying(m: &mut ConnManager<Addr>, kb: &DeviceInfo<Addr>, after_link_loss: bool) -> u32 {
    if !after_link_loss {
        return m.reserve_retry(0, kb);
    }
    let attempt = m.reserve_slot(0, kb);
    on_slot_connected(m, 0, attempt, kb);
    on_slot_link_lost(m, 0, attempt, kb);
    attempt
}

#[test]
fn selecting_a_device_takes_over_its_background_retry() {
    for after_link_loss in [false, true] {
        let mut m = mgr();
        let devices = [dev(7, "kb")];
        let retry = retrying(&mut m, &devices[0], after_link_loss);
        let acts = plan_connect(&mut m, &devices, 0, PartialEq::eq);
        // The same slot connects again, now allowed to pair, under a new
        // attempt number; no second slot.
        let attempt = m.slot_attempt(0);
        assert_ne!(attempt, retry);
        assert_eq!(
            acts.as_slice(),
            &[Action::ConnectSlot {
                slot: 0,
                device: dev(7, "kb"),
                attempt,
            }]
        );
        assert_eq!(m.occupied_count(), 1);
        assert_eq!(m.find_empty_slot(), Some(1));
        // The slot is now the user's connection: selecting again waits for it.
        assert!(plan_connect(&mut m, &devices, 0, PartialEq::eq).is_empty());
    }
}

#[test]
fn events_from_a_replaced_attempt_are_ignored() {
    // The coordinator handles the selection before the retry's queued
    // reports, which must neither end nor complete the user's connection.
    let mut m = mgr();
    let kb = dev(7, "kb");
    let retry = retrying(&mut m, &kb, true);
    plan_connect(&mut m, core::slice::from_ref(&kb), 0, PartialEq::eq);
    let attempt = m.slot_attempt(0);
    assert!(on_slot_connected(&mut m, 0, retry, &kb).is_empty());
    assert!(on_slot_error(&mut m, 0, retry, ErrorTag::HidNotFound, false).is_empty());
    assert!(on_slot_link_lost(&mut m, 0, retry, &kb).is_empty());
    assert!(on_slot_disconnected(&mut m, 0, retry).is_empty());
    assert_eq!(m.slot_attempt(0), attempt);
    assert_eq!(m.active_count(), 0);
    // Still the user's connection, not a retry: selecting again waits for it.
    assert!(plan_connect(&mut m, core::slice::from_ref(&kb), 0, PartialEq::eq).is_empty());
    // The worker confirms the link under the new number.
    let acts = on_slot_connected(&mut m, 0, attempt, &kb);
    assert_eq!(acts[0], Action::PersistDevice(kb.clone()));
    assert_eq!(m.active_count(), 1);
}

#[test]
fn a_free_slot_ignores_every_event() {
    let mut m = mgr();
    let kb = dev(7, "kb");
    // A free slot's number is 0, which no attempt is given.
    assert_eq!(m.slot_attempt(0), 0);
    assert!(on_slot_connected(&mut m, 0, 0, &kb).is_empty());
    let attempt = m.reserve_slot(0, &kb);
    m.disconnect_slot(0);
    assert!(on_slot_connected(&mut m, 0, attempt, &kb).is_empty());
    assert!(on_slot_link_lost(&mut m, 0, attempt, &kb).is_empty());
    assert!(on_slot_error(&mut m, 0, attempt, ErrorTag::ConnectFailed, true).is_empty());
    assert!(on_slot_disconnected(&mut m, 0, attempt).is_empty());
    assert!(on_slot_connected(&mut m, MAX_CONNECTIONS, attempt, &kb).is_empty());
    assert_eq!(m.occupied_count(), 0);
}

#[test]
fn a_failed_takeover_goes_back_to_the_retry() {
    let kb = dev(7, "kb");
    let mut m = mgr();
    retrying(&mut m, &kb, false);
    plan_connect(&mut m, core::slice::from_ref(&kb), 0, PartialEq::eq);
    let attempt = m.slot_attempt(0);
    let acts = on_slot_error(&mut m, 0, attempt, ErrorTag::ConnectFailed, true);
    // A pairing the failed connection made was never saved; the retry's own
    // keys are saved, so discarding leaves them.
    assert_eq!(
        acts.as_slice(),
        &[
            Action::DiscardUnsavedBond(7),
            Action::Emit(UiEvent::Error(ErrorTag::ConnectFailed)),
            Action::Emit(UiEvent::Disconnected),
        ]
    );
    // Held for the device under the same number while the worker retries.
    assert!(m.is_slot_occupied(0));
    assert_eq!(m.slot_attempt(0), attempt);
    assert_eq!(m.find_empty_slot(), Some(1));
    // The retry still completes the connection when the device comes back.
    let mut connected = mgr();
    connected.slots = m.slots.clone();
    on_slot_connected(&mut connected, 0, attempt, &kb);
    assert_eq!(connected.active_count(), 1);
    // And it is a retry again: a new selection takes it over once more.
    let acts = plan_connect(&mut m, core::slice::from_ref(&kb), 0, PartialEq::eq);
    assert!(matches!(
        acts.as_slice(),
        [Action::ConnectSlot { slot: 0, attempt: next, .. }] if *next != attempt
    ));
}

#[test]
fn a_reserved_slot_is_freed_when_its_attempt_ends() {
    // No link is up yet for the user's connection, a background retry, or a
    // selection that took a retry over; an error the worker will not retry,
    // or a disconnect, frees the slot.
    let kb = dev(7, "kb");
    for case in 0..3 {
        let reserve = |m: &mut ConnManager<Addr>| match case {
            0 => m.reserve_slot(0, &kb),
            1 => m.reserve_retry(0, &kb),
            _ => {
                retrying(m, &kb, false);
                plan_connect(m, core::slice::from_ref(&kb), 0, PartialEq::eq);
                m.slot_attempt(0)
            }
        };
        let mut m = mgr();
        let attempt = reserve(&mut m);
        let acts = on_slot_error(&mut m, 0, attempt, ErrorTag::HidNotFound, false);
        // Any pairing the attempt made goes with it: it was never saved.
        assert_eq!(
            acts.as_slice(),
            &[
                Action::DiscardUnsavedBond(7),
                Action::Emit(UiEvent::Error(ErrorTag::HidNotFound)),
                Action::Emit(UiEvent::Disconnected),
            ]
        );
        assert!(!m.is_slot_occupied(0));
        assert_eq!(m.slot_attempt(0), 0);
        // The next selection reserves the slot afresh.
        let acts = plan_connect(&mut m, core::slice::from_ref(&kb), 0, PartialEq::eq);
        assert!(matches!(
            acts.as_slice(),
            [Action::ConnectSlot { slot: 0, attempt: next, .. }] if *next != attempt
        ));

        let mut m = mgr();
        let attempt = reserve(&mut m);
        let acts = on_slot_disconnected(&mut m, 0, attempt);
        assert_eq!(
            acts.as_slice(),
            &[
                Action::DiscardUnsavedBond(7),
                Action::Emit(UiEvent::Disconnected),
            ]
        );
        assert!(!m.is_slot_occupied(0));
        assert_eq!(m.find_empty_slot(), Some(0));
    }
}

#[test]
fn attempt_numbers_skip_zero_when_they_wrap() {
    let mut m = mgr();
    m.last_attempt = u32::MAX;
    assert_eq!(m.reserve_slot(0, &dev(1, "kb")), 1);
    assert_eq!(m.reserve_retry(1, &dev(2, "mouse")), 2);
}

#[test]
fn connecting_keeps_the_reservations_attempt() {
    let mut m = mgr();
    let attempt = m.reserve_slot(0, &dev(1, "kb"));
    assert_eq!(m.connect_slot(0, &dev(1, "kb")), attempt);
    // A slot connected without a reservation is numbered too.
    let other = m.connect_slot(1, &dev(2, "mouse"));
    assert_ne!(other, 0);
    assert_ne!(other, attempt);
}

#[test]
fn a_bonded_peer_at_a_new_address_uses_the_slot_that_holds_it() {
    let mut m = mgr();
    m.reserve_retry(0, &dev(0x13, "kb"));
    let devices = [dev(0x23, "kb")];
    let acts = plan_connect(&mut m, &devices, 0, same_bonded_peer);
    assert_eq!(
        acts.as_slice(),
        &[Action::ConnectSlot {
            slot: 0,
            device: dev(0x23, "kb"),
            attempt: m.slot_attempt(0),
        }]
    );
    assert_eq!(m.slot_address(0), Some(&0x23));
    assert_eq!(m.occupied_count(), 1);

    // Connected under its old address, it is acknowledged, not connected twice.
    let mut m = mgr();
    m.connect_slot(1, &dev(0x13, "kb"));
    let acts = plan_connect(&mut m, &devices, 0, same_bonded_peer);
    assert_eq!(
        acts.as_slice(),
        &[Action::Emit(UiEvent::Connected(dev(0x13, "kb").name))]
    );
    assert_eq!(m.occupied_count(), 1);
}
