//! Host tests for the pure connection coordinator: the slot manager, the
//! scan, connect, and disconnect plans, and the slot event reducers. The
//! attempt-number and retry-takeover tests are in
//! `coordinator_attempt_tests.rs`.

use super::*;

#[path = "coordinator_attempt_tests.rs"]
mod attempt;

// Trivial stand-in for the embedded `Address` type.
type Addr = u8;

fn dev(address: Addr, name: &str) -> DeviceInfo<Addr> {
    let mut n: String<32> = String::new();
    let _ = n.push_str(name);
    DeviceInfo {
        address,
        name: n,
        rssi: -50,
    }
}

fn mgr() -> ConnManager<Addr> {
    ConnManager::new()
}

/// The `Connected` event naming these devices.
fn shows(names: &[&str]) -> UiEvent {
    UiEvent::Connected(names.iter().map(|&name| dev(0, name).name).collect())
}

// ── ConnManager state machine ──────────────────────────────────────────

#[test]
fn new_manager_is_empty() {
    let m = mgr();
    assert_eq!(m.active_count(), 0);
    assert_eq!(m.occupied_count(), 0);
    assert_eq!(m.find_empty_slot(), Some(0));
    assert!(!m.is_slot_occupied(0));
}

#[test]
fn reserve_then_connect_then_disconnect() {
    let mut m = mgr();
    let kb = dev(1, "Keyboard");

    m.reserve_slot(0, &kb);
    assert!(m.is_slot_occupied(0));
    assert_eq!(m.occupied_count(), 1);
    assert_eq!(m.active_count(), 0, "reserved != active");
    assert_eq!(m.slot_address(0), Some(&1));

    m.connect_slot(0, &kb);
    assert_eq!(m.active_count(), 1);
    assert_eq!(m.get_connected_names().len(), 1);

    m.disconnect_slot(0);
    assert_eq!(m.occupied_count(), 0);
    assert_eq!(m.active_count(), 0);
    assert_eq!(m.slot_address(0), None);
}

#[test]
fn out_of_range_slot_ops_are_ignored() {
    let mut m = mgr();
    // Indices >= MAX_CONNECTIONS must be no-ops, not panics.
    m.reserve_slot(MAX_CONNECTIONS, &dev(1, "x"));
    m.connect_slot(99, &dev(2, "y"));
    m.disconnect_slot(MAX_CONNECTIONS);
    assert_eq!(m.occupied_count(), 0);
    assert!(!m.is_slot_occupied(MAX_CONNECTIONS));
    assert!(!m.is_slot_occupied(99));
}

#[test]
fn default_matches_new() {
    let m: ConnManager<Addr> = ConnManager::default();
    assert_eq!(m.occupied_count(), 0);
    assert_eq!(m.find_empty_slot(), Some(0));
}

#[test]
fn reserve_uses_second_slot_when_first_busy() {
    let mut m = mgr();
    m.connect_slot(0, &dev(1, "kb"));
    let acts = plan_connect(&mut m, &[dev(2, "mouse")], 0, PartialEq::eq);
    match &acts[0] {
        Action::ConnectSlot { slot, .. } => assert_eq!(*slot, 1),
        other => panic!("expected ConnectSlot to slot 1, got {other:?}"),
    }
}

#[test]
fn find_empty_slot_fills_then_returns_none() {
    let mut m = mgr();
    m.reserve_slot(0, &dev(1, "a"));
    assert_eq!(m.find_empty_slot(), Some(1));
    m.reserve_slot(1, &dev(2, "b"));
    assert_eq!(m.find_empty_slot(), None);
}

#[test]
fn link_state_names_each_connected_device_in_slot_order() {
    let mut m = mgr();
    assert_eq!(link_state(&m), UiEvent::Disconnected);

    m.connect_slot(1, &dev(2, "Mouse"));
    assert_eq!(link_state(&m), shows(&["Mouse"]));

    m.reserve_slot(0, &dev(1, "Keyboard"));
    assert_eq!(link_state(&m), shows(&["Mouse"]));
    m.connect_slot(0, &dev(1, "Keyboard"));
    assert_eq!(link_state(&m), shows(&["Keyboard", "Mouse"]));

    m.disconnect_slot(1);
    assert_eq!(link_state(&m), shows(&["Keyboard"]));
}

#[test]
fn link_state_reports_background_reconnects_until_every_one_ends() {
    let (kb, mouse) = (dev(1, "Keyboard"), dev(2, "Mouse"));
    let mut m = mgr();
    // Power-up: both saved devices are being reconnected.
    let kb_retry = m.reserve_retry(0, &kb);
    m.reserve_retry(1, &mouse);
    assert_eq!(link_state(&m), UiEvent::Reconnecting);
    // A link that is up wins over a retry in the other slot.
    m.connect_slot(1, &mouse);
    assert_eq!(link_state(&m), shows(&["Mouse"]));
    // Its link drops, so both slots retry again.
    let mouse_retry = m.slot_attempt(1);
    let acts = on_slot_link_lost(&mut m, 1, mouse_retry, &mouse);
    assert_eq!(acts.as_slice(), &[Action::Emit(UiEvent::Reconnecting)]);
    // A retry that stops (here the keys are gone) leaves the other one.
    let acts = on_slot_error(&mut m, 0, kb_retry, ErrorTag::KeysMissing, false);
    assert_eq!(acts.last(), Some(&Action::Emit(UiEvent::Reconnecting)));
    // A command that frees the last retrying slot (Disconnect, or a scan with
    // both slots busy) ends it: its worker reports the slot free.
    let acts = on_slot_disconnected(&mut m, 1, mouse_retry);
    assert_eq!(acts.last(), Some(&Action::Emit(UiEvent::Disconnected)));
    // A user connection in progress is not a background reconnect.
    m.reserve_slot(0, &kb);
    assert_eq!(link_state(&m), UiEvent::Disconnected);
}

// ── Reducers ────────────────────────────────────────────────────────────

#[test]
fn plan_start_scan_noop_when_not_full() {
    let mut m = mgr();
    m.connect_slot(0, &dev(1, "kb")); // one slot busy, one free
    assert!(plan_start_scan(&m).is_empty());
}

#[test]
fn plan_start_scan_disconnects_all_when_full() {
    let mut m = mgr();
    m.connect_slot(0, &dev(1, "kb"));
    m.connect_slot(1, &dev(2, "mouse"));
    let acts = plan_start_scan(&m);
    assert_eq!(acts.len(), 2);
    assert_eq!(acts[0], Action::DisconnectSlot(0));
    assert_eq!(acts[1], Action::DisconnectSlot(1));
}

#[test]
fn plan_connect_out_of_range_errors() {
    let mut m = mgr();
    let acts = plan_connect(&mut m, &[], 0, PartialEq::eq);
    assert_eq!(
        acts[0],
        Action::Emit(UiEvent::Error(ErrorTag::ConnectFailed))
    );
}

#[test]
fn plan_connect_success_reserves_and_emits_connect() {
    let mut m = mgr();
    let devices = [dev(1, "kb"), dev(2, "mouse")];
    let acts = plan_connect(&mut m, &devices, 1, PartialEq::eq);
    assert_eq!(acts.len(), 1);
    match &acts[0] {
        Action::ConnectSlot {
            slot,
            device,
            attempt,
        } => {
            assert_eq!(*slot, 0);
            assert_eq!(device.address, 2);
            assert_eq!(*attempt, m.slot_attempt(0));
        }
        other => panic!("expected ConnectSlot, got {other:?}"),
    }
    // Slot 0 is now reserved (occupied but not active).
    assert!(m.is_slot_occupied(0));
    assert_eq!(m.active_count(), 0);
}

#[test]
fn plan_connect_already_connected_acknowledges_without_duplicate_connect() {
    let mut m = mgr();
    m.connect_slot(0, &dev(7, "kb"));
    let devices = [dev(7, "kb")];
    let acts = plan_connect(&mut m, &devices, 0, PartialEq::eq);
    assert_eq!(acts.as_slice(), &[Action::Emit(shows(&["kb"]))]);
    assert_eq!(m.active_count(), 1);
    assert_eq!(m.occupied_count(), 1);
}

#[test]
fn plan_connect_already_connecting_waits_for_real_result() {
    let mut m = mgr();
    let devices = [dev(7, "kb")];
    m.reserve_slot(0, &devices[0]);
    let acts = plan_connect(&mut m, &devices, 0, PartialEq::eq);
    assert!(acts.is_empty(), "no duplicate worker or premature success");
    assert_eq!(m.active_count(), 0);
    assert_eq!(m.occupied_count(), 1);
}

#[test]
fn plan_connect_no_free_slot_errors() {
    let mut m = mgr();
    m.connect_slot(0, &dev(1, "a"));
    m.connect_slot(1, &dev(2, "b"));
    let devices = [dev(3, "c")];
    let acts = plan_connect(&mut m, &devices, 0, PartialEq::eq);
    assert_eq!(
        acts[0],
        Action::Emit(UiEvent::Error(ErrorTag::ConnectFailed))
    );
}

#[test]
fn plan_disconnect_targets_occupied_slots() {
    let mut m = mgr();
    m.connect_slot(1, &dev(2, "mouse")); // only slot 1 busy
    let acts = plan_disconnect(&m);
    assert_eq!(acts.len(), 1);
    assert_eq!(acts[0], Action::DisconnectSlot(1));
}

#[test]
fn on_slot_connected_persists_and_emits_summary() {
    let mut m = mgr();
    let kb = dev(1, "Keyboard");
    let attempt = m.reserve_slot(0, &kb);
    let acts = on_slot_connected(&mut m, 0, attempt, &kb);
    assert_eq!(acts.len(), 2);
    assert!(matches!(acts[0], Action::PersistDevice(_)));
    assert_eq!(acts[1], Action::Emit(shows(&["Keyboard"])));
    assert_eq!(m.active_count(), 1);
}

#[test]
fn on_slot_disconnected_last_link_emits_disconnected() {
    let mut m = mgr();
    let attempt = m.connect_slot(0, &dev(1, "kb"));
    let acts = on_slot_disconnected(&mut m, 0, attempt);
    // The connected device was saved when it connected: nothing to discard.
    assert_eq!(acts.as_slice(), &[Action::Emit(UiEvent::Disconnected)]);
}

#[test]
fn on_slot_disconnected_with_other_link_emits_summary() {
    let mut m = mgr();
    let attempt = m.connect_slot(0, &dev(1, "kb"));
    m.connect_slot(1, &dev(2, "Mouse"));
    let acts = on_slot_disconnected(&mut m, 0, attempt);
    // Slot 0 gone, slot 1 ("Mouse") remains.
    assert_eq!(acts[0], Action::Emit(shows(&["Mouse"])));
}

#[test]
fn on_slot_error_emits_error_then_status() {
    let mut m = mgr();
    let attempt = m.connect_slot(0, &dev(1, "kb"));
    let acts = on_slot_error(&mut m, 0, attempt, ErrorTag::NotifyFailed, false);
    assert_eq!(acts.len(), 2);
    assert_eq!(
        acts[0],
        Action::Emit(UiEvent::Error(ErrorTag::NotifyFailed))
    );
    assert_eq!(acts[1], Action::Emit(UiEvent::Disconnected));
}

#[test]
fn a_reconnect_given_up_for_its_keys_frees_the_slot_and_tells_the_user() {
    for tag in [ErrorTag::KeysMissing, ErrorTag::KeysRejected] {
        let mut m = mgr();
        let kb = dev(1, "kb");
        let attempt = m.reserve_retry(0, &kb);
        let acts = on_slot_error(&mut m, 0, attempt, tag, false);
        assert_eq!(
            acts.as_slice(),
            &[
                Action::DiscardUnsavedBond(1),
                Action::Emit(UiEvent::Error(tag)),
                Action::Emit(UiEvent::Disconnected),
            ]
        );
        assert!(!m.is_slot_occupied(0));
        // The slot can take another device, and the user's selection of this
        // one connects it afresh.
        assert_eq!(m.find_empty_slot(), Some(0));
    }
}

#[test]
fn on_slot_error_with_surviving_link_reports_summary() {
    let mut m = mgr();
    let attempt = m.connect_slot(0, &dev(1, "kb"));
    m.connect_slot(1, &dev(2, "mouse"));
    let acts = on_slot_error(&mut m, 0, attempt, ErrorTag::ConnectFailed, false);
    assert_eq!(
        acts[0],
        Action::Emit(UiEvent::Error(ErrorTag::ConnectFailed))
    );
    assert!(matches!(acts[1], Action::Emit(UiEvent::Connected(_))));
    assert_eq!(m.active_count(), 1);
}

#[test]
fn on_slot_link_lost_keeps_slot_reserved_for_reconnect() {
    let mut m = mgr();
    let kb = dev(1, "kb");
    let attempt = m.connect_slot(0, &kb);
    let acts = on_slot_link_lost(&mut m, 0, attempt, &kb);
    // No link is up, and Home says the keyboard is being reconnected.
    assert_eq!(acts.as_slice(), &[Action::Emit(UiEvent::Reconnecting)]);
    // Not active any more, but still held for the same device under the same
    // number, so a connect request for another device can't take the slot
    // out from under the reconnect (selecting the same device turns the
    // retry into a user connection).
    assert_eq!(m.active_count(), 0);
    assert!(m.is_slot_occupied(0));
    assert_eq!(m.slot_address(0), Some(&1));
    assert_eq!(m.slot_attempt(0), attempt);
    assert_eq!(m.find_empty_slot(), Some(1));
}

#[test]
fn on_slot_link_lost_with_other_link_reports_summary() {
    let mut m = mgr();
    let kb = dev(1, "kb");
    let attempt = m.connect_slot(0, &kb);
    m.connect_slot(1, &dev(2, "Mouse"));
    let acts = on_slot_link_lost(&mut m, 0, attempt, &kb);
    assert_eq!(acts[0], Action::Emit(shows(&["Mouse"])));
    assert_eq!(m.occupied_count(), 2);
}

#[test]
fn link_lost_then_reconnected_is_active_again() {
    let mut m = mgr();
    let kb = dev(1, "kb");
    let attempt = m.connect_slot(0, &kb);
    on_slot_link_lost(&mut m, 0, attempt, &kb);
    let acts = on_slot_connected(&mut m, 0, attempt, &kb);
    assert_eq!(m.active_count(), 1);
    assert!(matches!(acts[1], Action::Emit(UiEvent::Connected(_))));
}

#[test]
fn disconnect_while_reconnecting_targets_the_slot() {
    // A user "disconnect" must reach a slot that's silently reconnecting, so
    // the worker stops retrying.
    let mut m = mgr();
    let kb = dev(1, "kb");
    let attempt = m.connect_slot(0, &kb);
    on_slot_link_lost(&mut m, 0, attempt, &kb);
    let acts = plan_disconnect(&m);
    assert_eq!(acts.as_slice(), &[Action::DisconnectSlot(0)]);
}

#[test]
fn a_link_that_drops_during_setup_failed_to_connect() {
    for tag in ErrorTag::ALL {
        // A discovery failure on a link still up keeps its own error...
        assert_eq!(tag.for_failed_setup(true), tag);
        // ...while one whose link dropped is retried like a failed connect,
        // unless the keys were found wanting: a peer may drop the link once
        // it refuses them.
        let dropped = if tag.needs_new_pairing() {
            tag
        } else {
            ErrorTag::ConnectFailed
        };
        assert_eq!(tag.for_failed_setup(false), dropped, "{tag:?}");
    }
}

#[test]
fn only_a_verdict_on_the_keys_needs_a_new_pairing() {
    let needing: std::vec::Vec<ErrorTag> = ErrorTag::ALL
        .into_iter()
        .filter(|tag| tag.needs_new_pairing())
        .collect();
    assert_eq!(needing, [ErrorTag::KeysRejected, ErrorTag::KeysMissing]);
}
