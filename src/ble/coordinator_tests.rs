use super::*;

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
    assert!(m.is_connected_address(&1));

    m.connect_slot(0, &kb);
    assert_eq!(m.active_count(), 1);
    assert_eq!(m.get_connected_names().len(), 1);

    m.disconnect_slot(0);
    assert_eq!(m.occupied_count(), 0);
    assert_eq!(m.active_count(), 0);
    assert!(!m.is_connected_address(&1));
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
    let acts = plan_connect(&mut m, &[dev(2, "mouse")], 0);
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
fn summary_reflects_connection_count() {
    let mut m = mgr();
    assert_eq!(connection_summary(&m).as_str(), "Connected");

    m.connect_slot(0, &dev(1, "Keyboard"));
    assert_eq!(connection_summary(&m).as_str(), "Keyboard");

    m.connect_slot(1, &dev(2, "Mouse"));
    assert_eq!(connection_summary(&m).as_str(), "2 devices");
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
    let acts = plan_connect(&mut m, &[], 0);
    assert_eq!(
        acts[0],
        Action::Emit(UiEvent::Error(ErrorTag::ConnectFailed))
    );
}

#[test]
fn plan_connect_success_reserves_and_emits_connect() {
    let mut m = mgr();
    let devices = [dev(1, "kb"), dev(2, "mouse")];
    let acts = plan_connect(&mut m, &devices, 1);
    assert_eq!(acts.len(), 1);
    match &acts[0] {
        Action::ConnectSlot { slot, device } => {
            assert_eq!(*slot, 0);
            assert_eq!(device.address, 2);
        }
        other => panic!("expected ConnectSlot, got {other:?}"),
    }
    // Slot 0 is now reserved (occupied but not active).
    assert!(m.is_slot_occupied(0));
    assert_eq!(m.active_count(), 0);
}

#[test]
fn plan_connect_already_connected_is_noop() {
    let mut m = mgr();
    m.connect_slot(0, &dev(7, "kb"));
    let devices = [dev(7, "kb")];
    let acts = plan_connect(&mut m, &devices, 0);
    assert!(acts.is_empty(), "no duplicate connect");
}

#[test]
fn plan_connect_no_free_slot_errors() {
    let mut m = mgr();
    m.connect_slot(0, &dev(1, "a"));
    m.connect_slot(1, &dev(2, "b"));
    let devices = [dev(3, "c")];
    let acts = plan_connect(&mut m, &devices, 0);
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
    let acts = on_slot_connected(&mut m, 0, &kb);
    assert_eq!(acts.len(), 2);
    assert!(matches!(acts[0], Action::PersistDevice(_)));
    assert_eq!(
        acts[1],
        Action::Emit(UiEvent::Connected({
            let mut s: String<32> = String::new();
            let _ = s.push_str("Keyboard");
            s
        }))
    );
    assert_eq!(m.active_count(), 1);
}

#[test]
fn on_slot_disconnected_last_link_emits_disconnected() {
    let mut m = mgr();
    m.connect_slot(0, &dev(1, "kb"));
    let acts = on_slot_disconnected(&mut m, 0);
    assert_eq!(acts[0], Action::Emit(UiEvent::Disconnected));
}

#[test]
fn on_slot_disconnected_with_other_link_emits_summary() {
    let mut m = mgr();
    m.connect_slot(0, &dev(1, "kb"));
    m.connect_slot(1, &dev(2, "Mouse"));
    let acts = on_slot_disconnected(&mut m, 0);
    // Slot 0 gone, slot 1 ("Mouse") remains.
    assert_eq!(
        acts[0],
        Action::Emit(UiEvent::Connected({
            let mut s: String<32> = String::new();
            let _ = s.push_str("Mouse");
            s
        }))
    );
}

#[test]
fn on_slot_error_emits_error_then_status() {
    let mut m = mgr();
    m.connect_slot(0, &dev(1, "kb"));
    let acts = on_slot_error(&mut m, 0, ErrorTag::NotifyFailed);
    assert_eq!(acts.len(), 2);
    assert_eq!(
        acts[0],
        Action::Emit(UiEvent::Error(ErrorTag::NotifyFailed))
    );
    assert_eq!(acts[1], Action::Emit(UiEvent::Disconnected));
}

#[test]
fn on_slot_error_with_surviving_link_reports_summary() {
    let mut m = mgr();
    m.connect_slot(0, &dev(1, "kb"));
    m.connect_slot(1, &dev(2, "mouse"));
    let acts = on_slot_error(&mut m, 0, ErrorTag::ConnectFailed);
    assert_eq!(
        acts[0],
        Action::Emit(UiEvent::Error(ErrorTag::ConnectFailed))
    );
    assert!(matches!(acts[1], Action::Emit(UiEvent::Connected(_))));
    assert_eq!(m.active_count(), 1);
}
