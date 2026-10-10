//! Host tests for the UI reducer, management requests, and `UiState`.

use super::*;

#[test]
fn home_select_starts_scan() {
    let out = on_button(Screen::Home, ButtonEvent::Select, 0, 0);
    assert_eq!(out.screen, Screen::Scanning);
    assert_eq!(out.command, Some(UiCommand::StartScan));
    assert!(out.reset_devices);
    assert_eq!(out.redraw, Redraw::Scanning);
}

#[test]
fn error_select_starts_scan() {
    let out = on_button(Screen::Error, ButtonEvent::Select, 3, 5);
    assert_eq!(out.screen, Screen::Scanning);
    assert_eq!(out.selected, 0);
    assert_eq!(out.command, Some(UiCommand::StartScan));
}

#[test]
fn device_list_up_moves_selection_and_redraws() {
    let out = on_button(Screen::DeviceList, ButtonEvent::Up, 2, 4);
    assert_eq!(out.selected, 1);
    assert_eq!(out.redraw, Redraw::DeviceList);
    assert_eq!(out.command, None);
}

#[test]
fn device_list_up_at_top_clamps() {
    let out = on_button(Screen::DeviceList, ButtonEvent::Up, 0, 4);
    assert_eq!(out.selected, 0);
    // Original redraws unconditionally on Up.
    assert_eq!(out.redraw, Redraw::DeviceList);
}

#[test]
fn device_list_down_advances_within_bounds() {
    let out = on_button(Screen::DeviceList, ButtonEvent::Down, 1, 4);
    assert_eq!(out.selected, 2);
    assert_eq!(out.redraw, Redraw::DeviceList);
}

#[test]
fn device_list_down_at_end_is_noop() {
    let out = on_button(Screen::DeviceList, ButtonEvent::Down, 3, 4);
    assert_eq!(out.selected, 3);
    assert_eq!(
        out.redraw,
        Redraw::None,
        "no redraw when selection unchanged"
    );
    assert_eq!(out.command, None);
}

#[test]
fn device_list_select_connects_highlighted() {
    let out = on_button(Screen::DeviceList, ButtonEvent::Select, 2, 4);
    assert_eq!(out.screen, Screen::Connecting);
    assert_eq!(out.command, Some(UiCommand::Connect(2)));
    assert!(!out.reset_devices, "keep device list for the connect");
}

#[test]
fn empty_list_cannot_dispatch_connection() {
    let out = on_button(Screen::DeviceList, ButtonEvent::Select, 0, 0);
    assert_eq!(out.command, None);
    assert_eq!(out.screen, Screen::DeviceList);
}

#[test]
fn stale_selection_is_clamped_before_navigation_and_connect() {
    let out = on_button(Screen::DeviceList, ButtonEvent::Down, usize::MAX, 8);
    assert_eq!(out.selected, 7);
    let out = on_button(Screen::DeviceList, ButtonEvent::Select, usize::MAX, 2);
    assert_eq!(out.command, Some(UiCommand::Connect(1)));
}

#[test]
fn connected_select_rescans() {
    let out = on_button(Screen::Connected, ButtonEvent::Select, 0, 0);
    assert_eq!(out.screen, Screen::Scanning);
    assert_eq!(out.command, Some(UiCommand::StartScan));
    assert!(out.reset_devices);
}

#[test]
fn connected_down_disconnects_home() {
    let out = on_button(Screen::Connected, ButtonEvent::Down, 0, 0);
    assert_eq!(out.screen, Screen::Home);
    assert_eq!(out.command, Some(UiCommand::Disconnect));
    assert_eq!(out.redraw, Redraw::Home);
}

#[test]
fn ignored_combinations_are_noops() {
    // e.g. Up on Home, Down on Home, Select already handled elsewhere.
    let out = on_button(Screen::Scanning, ButtonEvent::Up, 0, 0);
    assert_eq!(out.screen, Screen::Scanning);
    assert_eq!(out.command, None);
    assert_eq!(out.redraw, Redraw::None);

    let out = on_button(Screen::Home, ButtonEvent::Down, 0, 0);
    assert_eq!(out.command, None);
}

#[test]
fn scan_complete_picks_list_or_error() {
    assert_eq!(on_scan_complete(0), Screen::Error);
    assert_eq!(on_scan_complete(3), Screen::DeviceList);
}

#[test]
fn management_requires_explicit_confirmation_and_defaults_to_cancel() {
    let out = on_button(Screen::Home, ButtonEvent::Up, 0, 0);
    assert_eq!(out.command, Some(UiCommand::ListPaired));
    let out = on_button(Screen::SavedDevices, ButtonEvent::Select, 1, 2);
    assert_eq!(out.screen, Screen::ConfirmForget(1));
    assert_eq!(out.selected, 0);
    let cancelled = on_button(out.screen, ButtonEvent::Select, out.selected, 2);
    assert_eq!(cancelled.screen, Screen::SavedDevices);
    assert_eq!(cancelled.command, None);
    let armed = on_button(out.screen, ButtonEvent::Down, 0, 2);
    let confirmed = on_button(armed.screen, ButtonEvent::Select, armed.selected, 2);
    assert_eq!(confirmed.command, Some(UiCommand::Forget(1)));
    assert_eq!(confirmed.screen, Screen::Managing);
    assert_eq!(
        on_button(confirmed.screen, ButtonEvent::Select, 0, 2).command,
        None
    );
}

#[test]
fn management_requests_are_exclusive_and_reject_stale_replies() {
    let mut requests = ManagementRequests::default();
    assert!(!requests.is_pending());
    let first = requests.begin(UiCommand::ListPaired, u64::MAX).unwrap();
    assert!(requests.is_pending());
    assert_eq!(requests.begin(UiCommand::FactoryReset, u64::MAX), None);
    assert_eq!(requests.complete(first.wrapping_add(1)), None);
    assert!(requests.is_pending());
    assert_eq!(requests.complete(first), Some(UiCommand::ListPaired));
    assert!(!requests.is_pending());
    // A late duplicate of the finished reply cannot complete a newer request.
    let second = requests.begin(UiCommand::Forget(0), u64::MAX).unwrap();
    assert_ne!(second, first);
    assert_eq!(requests.complete(first), None);
    assert_eq!(requests.complete(second), Some(UiCommand::Forget(0)));
}

#[test]
fn management_request_ids_stay_unique_across_wraparound() {
    let mut requests = ManagementRequests {
        next_id: u32::MAX,
        pending: None,
    };
    let wrapped = requests.begin(UiCommand::ListPaired, u64::MAX).unwrap();
    assert_eq!(requests.complete(u32::MAX), None);
    assert_eq!(requests.complete(wrapped), Some(UiCommand::ListPaired));
    assert_ne!(
        requests.begin(UiCommand::ListPaired, u64::MAX),
        Some(wrapped)
    );
}

#[test]
fn empty_store_still_offers_deliberate_factory_reset() {
    let confirm = on_button(Screen::SavedDevices, ButtonEvent::Select, 0, 0);
    assert_eq!(confirm.screen, Screen::ConfirmReset);
    assert_eq!(confirm.selected, 0);
    assert_eq!(
        on_button(confirm.screen, ButtonEvent::Select, 0, 0).command,
        None
    );
    assert_eq!(
        on_button(confirm.screen, ButtonEvent::Select, 1, 0).command,
        Some(UiCommand::FactoryReset)
    );
}

#[test]
fn error_survives_followup_status_and_acknowledges_to_current_link() {
    let mut state = UiState::new();
    state.error("Connect failed");
    state.connection_status(Some(heapless::String::try_from("Keyboard").unwrap()));
    assert_eq!(state.screen, Screen::Error);
    assert_eq!(state.message, "Connect failed");
    assert_eq!(state.button(ButtonEvent::Down), Some(UiCommand::Dismiss));
    assert_eq!(state.screen, Screen::Connected);
    state.error("Flash write failed");
    state.connection_status(None);
    assert_eq!(state.screen, Screen::Error);
    state.button(ButtonEvent::Down);
    assert_eq!(state.screen, Screen::Home);
}

#[test]
fn background_status_and_scan_do_not_dismiss_confirmation() {
    let mut state = UiState::new();
    state.screen = Screen::ConfirmReset;
    state.selected = 1;
    state.connection_status(None);
    state.scan_started();
    state.scan_complete();
    assert_eq!(state.screen, Screen::ConfirmReset);
    assert_eq!(state.selected, 1);
}

#[test]
fn unanswered_request_expires_at_its_deadline_and_late_reply_is_ignored() {
    let mut requests = ManagementRequests::default();
    let id = requests.begin(UiCommand::Forget(1), 30_000).unwrap();
    assert_eq!(requests.expire(29_999), None);
    assert!(requests.is_pending());
    assert_eq!(requests.expire(30_000), Some(UiCommand::Forget(1)));
    assert!(!requests.is_pending());
    assert_eq!(requests.expire(u64::MAX), None);
    // The coordinator's late reply to the abandoned request changes nothing,
    // before or after a new request starts.
    assert_eq!(requests.complete(id), None);
    let next = requests.begin(UiCommand::ListPaired, 70_000).unwrap();
    assert_ne!(next, id);
    assert_eq!(requests.complete(id), None);
    assert!(requests.is_pending());
    assert_eq!(requests.complete(next), Some(UiCommand::ListPaired));
}

#[test]
fn answered_request_does_not_expire() {
    let mut requests = ManagementRequests::default();
    let id = requests.begin(UiCommand::FactoryReset, 1_000).unwrap();
    assert_eq!(requests.complete(id), Some(UiCommand::FactoryReset));
    assert_eq!(requests.expire(2_000), None);
}

#[test]
fn timed_out_management_claims_no_outcome_and_reopens_the_list() {
    for (command, message) in [
        (UiCommand::Forget(0), "Forget result unknown"),
        (UiCommand::FactoryReset, "Reset result unknown"),
        (UiCommand::ListPaired, "List not loaded"),
    ] {
        let mut state = UiState::new();
        let _ = state
            .paired_names
            .push(heapless::String::try_from("Keyboard").unwrap());
        state.screen = Screen::Managing;
        state.management_timed_out(command);
        assert_eq!(state.screen, Screen::NoReply);
        assert_eq!(state.message, message);
        // The old snapshot may no longer match flash; it is read again.
        assert!(state.paired_names.is_empty());
        // A link update does not hide the notice.
        state.connection_status(Some(heapless::String::try_from("Mouse").unwrap()));
        assert_eq!(state.screen, Screen::NoReply);
        assert_eq!(state.button(ButtonEvent::Up), Some(UiCommand::ListPaired));
        assert_eq!(state.screen, Screen::Managing);
    }
}

#[test]
fn no_reply_notice_is_acknowledged_with_select() {
    let mut state = UiState::new();
    state.management_timed_out(UiCommand::FactoryReset);
    assert_eq!(state.button(ButtonEvent::Down), None);
    assert_eq!(state.screen, Screen::NoReply);
    assert_eq!(state.button(ButtonEvent::Select), Some(UiCommand::Dismiss));
    assert_eq!(state.screen, Screen::Home);
    assert!(state.message.is_empty());
}

#[test]
fn timeout_keeps_an_error_already_showing() {
    let mut state = UiState::new();
    state.error("Storage failed");
    state.management_timed_out(UiCommand::Forget(0));
    assert_eq!(state.screen, Screen::Error);
    assert_eq!(state.message, "Storage failed");
}

fn name(value: &str) -> heapless::String<32> {
    heapless::String::try_from(value).unwrap()
}

#[test]
fn saved_devices_navigation_reaches_every_entry_and_backs_out() {
    // Two saved devices plus the final Factory reset entry.
    let back = on_button(Screen::SavedDevices, ButtonEvent::Up, 0, 2);
    assert_eq!(back.screen, Screen::Home);
    assert_eq!(back.command, Some(UiCommand::Dismiss));
    let up = on_button(Screen::SavedDevices, ButtonEvent::Up, 2, 2);
    assert_eq!(
        (up.screen, up.selected, up.command),
        (Screen::SavedDevices, 1, None)
    );
    let down = on_button(Screen::SavedDevices, ButtonEvent::Down, 1, 2);
    assert_eq!(down.selected, 2);
    let clamped = on_button(Screen::SavedDevices, ButtonEvent::Down, 2, 2);
    assert_eq!(clamped.selected, 2);
    let forget = on_button(Screen::SavedDevices, ButtonEvent::Select, 1, 2);
    assert_eq!(
        (forget.screen, forget.selected),
        (Screen::ConfirmForget(1), 0)
    );
    let reset = on_button(Screen::SavedDevices, ButtonEvent::Select, 2, 2);
    assert_eq!((reset.screen, reset.selected), (Screen::ConfirmReset, 0));
}

#[test]
fn ui_state_counts_saved_devices_on_management_screens() {
    let mut state = UiState::new();
    state.screen = Screen::SavedDevices;
    let _ = state.paired_names.push(name("Keyboard"));
    // With one saved device, index 1 is Factory reset.
    state.button(ButtonEvent::Down);
    assert_eq!(state.selected, 1);
    state.button(ButtonEvent::Select);
    assert_eq!(state.screen, Screen::ConfirmReset);
}

#[test]
fn completed_management_shows_a_notice_and_drops_the_saved_list() {
    for (command, message) in [
        (UiCommand::Forget(0), "Device forgotten"),
        (UiCommand::FactoryReset, "Pairings reset"),
    ] {
        let mut state = UiState::new();
        let _ = state.paired_names.push(name("Keyboard"));
        state.screen = Screen::Managing;
        state.management_completed(command);
        assert_eq!(state.screen, Screen::Notice);
        assert_eq!(state.message, message);
        assert!(state.paired_names.is_empty());
    }
    let mut state = UiState::new();
    state.error("Storage failed");
    state.management_completed(UiCommand::FactoryReset);
    assert_eq!(state.screen, Screen::Error);
    assert_eq!(state.message, "Storage failed");
}

#[test]
fn scan_lifecycle_lists_results_or_reports_none() {
    let mut state = UiState::new();
    state.selected = 3;
    state.scan_started();
    assert_eq!((state.screen, state.selected), (Screen::Scanning, 0));
    let _ = state.devices.push(name("Keyboard"));
    state.selected = 5;
    state.scan_complete();
    assert_eq!(state.screen, Screen::DeviceList);
    assert_eq!(state.selected, 0);
    // A completion with no scan in progress changes nothing.
    state.scan_complete();
    assert_eq!(state.screen, Screen::DeviceList);

    let mut empty = UiState::new();
    empty.scan_started();
    empty.scan_complete();
    assert_eq!(empty.screen, Screen::Error);
    assert_eq!(empty.message, "No devices found");
}

#[test]
fn starting_a_scan_from_the_buttons_clears_the_old_results() {
    let mut state = UiState::new();
    let _ = state.devices.push(name("Old"));
    state.scan_dots = 3;
    assert_eq!(
        state.button(ButtonEvent::Select),
        Some(UiCommand::StartScan)
    );
    assert!(state.devices.is_empty());
    assert_eq!(state.scan_dots, 0);
}

#[test]
fn a_new_link_returns_home_screens_to_connected_and_clears_the_list() {
    let mut state = UiState::new();
    let _ = state.devices.push(name("Keyboard"));
    state.selected = 1;
    state.connection_status(Some(name("Keyboard")));
    assert_eq!(state.screen, Screen::Connected);
    assert!(state.devices.is_empty());
    assert_eq!(state.selected, 0);
}

#[test]
fn long_messages_are_cut_to_the_message_capacity() {
    let mut state = UiState::new();
    state.notice("This notice is much longer than thirty-two bytes");
    assert_eq!(state.screen, Screen::Notice);
    assert_eq!(state.message.len(), 32);
    assert!("This notice is much longer than thirty-two bytes".starts_with(state.message.as_str()));
}
