//! Host tests for the UI controller: management request tracking, the
//! commands button presses send, and how coordinator events and the
//! housekeeping tick change the view.

use super::*;
use crate::ble::coordinator::DeviceInfo;
use crate::ui::layout::SCREEN_COLUMNS;

type Addr = u8;

fn name(value: &str) -> heapless::String<32> {
    heapless::String::try_from(value).unwrap()
}

fn device(address: Addr, value: &str) -> DeviceInfo<Addr> {
    DeviceInfo {
        address,
        name: name(value),
        rssi: -50,
    }
}

/// A controller showing the saved devices `devices`, newest first.
fn listing(devices: &[(Addr, &str)]) -> UiController<Addr> {
    let mut ui = UiController::new();
    let Some(Command::ListPaired { id }) = ui.button(ButtonEvent::Up, 0) else {
        panic!("UP on Home must list the saved devices");
    };
    ui.event(Event::PairedDevices {
        id,
        devices: devices
            .iter()
            .map(|&(address, value)| device(address, value))
            .collect(),
    });
    assert_eq!(ui.state.screen, Screen::SavedDevices);
    ui
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
fn scan_results_are_listed_and_connect_sends_the_highlighted_index() {
    let mut ui = UiController::<Addr>::new();
    assert_eq!(ui.button(ButtonEvent::Select, 0), Some(Command::StartScan));
    ui.event(Event::ScanStarted);
    ui.event(Event::DeviceFound(device(1, "Keyboard")));
    ui.event(Event::DeviceFound(device(2, "Mouse")));
    ui.event(Event::ScanComplete);
    assert_eq!(ui.state.screen, Screen::DeviceList);
    assert_eq!(ui.state.devices.len(), 2);
    assert_eq!(ui.button(ButtonEvent::Down, 0), None);
    assert_eq!(ui.button(ButtonEvent::Select, 0), Some(Command::Connect(1)));
    assert_eq!(ui.state.screen, Screen::Connecting);
    ui.event(Event::Connected(name("Mouse")));
    assert_eq!(ui.state.screen, Screen::Connected);
    assert_eq!(ui.button(ButtonEvent::Down, 0), Some(Command::Disconnect));
}

#[test]
fn devices_found_outside_a_scan_are_not_listed() {
    let mut ui = UiController::<Addr>::new();
    ui.event(Event::DeviceFound(device(1, "Keyboard")));
    assert!(ui.state.devices.is_empty());
    assert_eq!(ui.state.screen, Screen::Home);
}

#[test]
fn buttons_wait_for_the_list_reply_and_a_stale_reply_is_ignored() {
    let mut ui = UiController::<Addr>::new();
    let Some(Command::ListPaired { id }) = ui.button(ButtonEvent::Up, 0) else {
        panic!("UP on Home must list the saved devices");
    };
    assert_eq!(ui.state.screen, Screen::Managing);
    for button in [ButtonEvent::Up, ButtonEvent::Down, ButtonEvent::Select] {
        assert_eq!(ui.button(button, 0), None);
        assert_eq!(ui.state.screen, Screen::Managing);
    }
    ui.event(Event::PairedDevices {
        id: id.wrapping_add(1),
        devices: [device(7, "Keyboard")].into_iter().collect(),
    });
    assert_eq!(ui.state.screen, Screen::Managing);
    assert!(ui.state.paired_names.is_empty());
    ui.event(Event::PairedDevices {
        id,
        devices: [device(7, "Keyboard"), device(9, "Mouse")]
            .into_iter()
            .collect(),
    });
    assert_eq!(ui.state.screen, Screen::SavedDevices);
    assert_eq!(ui.state.paired_names, [name("Keyboard"), name("Mouse")]);
    // The reply ended the request, so the buttons act again.
    assert_eq!(ui.button(ButtonEvent::Down, 0), None);
    assert_eq!(ui.state.selected, 1);
}

#[test]
fn list_reply_does_not_hide_an_error() {
    let mut ui = UiController::<Addr>::new();
    let Some(Command::ListPaired { id }) = ui.button(ButtonEvent::Up, 0) else {
        panic!("UP on Home must list the saved devices");
    };
    ui.event(Event::Error(ErrorTag::StorageFailed));
    ui.event(Event::PairedDevices {
        id,
        devices: [device(7, "Keyboard")].into_iter().collect(),
    });
    assert_eq!(ui.state.screen, Screen::Error);
    assert_eq!(ui.state.paired_names, [name("Keyboard")]);
}

#[test]
fn forget_names_the_listed_address_and_reports_the_stored_result() {
    let mut ui = listing(&[(7, "Keyboard"), (9, "Mouse")]);
    assert_eq!(ui.button(ButtonEvent::Down, 0), None);
    assert_eq!(ui.button(ButtonEvent::Select, 0), None);
    assert_eq!(ui.state.screen, Screen::ConfirmForget(1));
    assert_eq!(ui.button(ButtonEvent::Down, 0), None);
    let Some(Command::Forget { id, address }) = ui.button(ButtonEvent::Select, 0) else {
        panic!("confirming must forget the device");
    };
    assert_eq!(address, 9);
    assert_eq!(ui.state.screen, Screen::Managing);
    // The coordinator reports the remaining link before the result.
    ui.event(Event::Connected(name("Keyboard")));
    assert_eq!(ui.state.screen, Screen::Managing);
    ui.event(Event::ManagementResult { id, result: Ok(()) });
    assert_eq!(ui.state.screen, Screen::Notice);
    assert_eq!(ui.state.message, "Device forgotten");
    assert!(ui.state.paired_names.is_empty());
    assert_eq!(ui.button(ButtonEvent::Select, 0), None);
    assert_eq!(ui.state.screen, Screen::Connected);
}

#[test]
fn factory_reset_reports_its_result_and_returns_home_without_links() {
    let mut ui = listing(&[]);
    assert_eq!(ui.button(ButtonEvent::Select, 0), None);
    assert_eq!(ui.state.screen, Screen::ConfirmReset);
    assert_eq!(ui.button(ButtonEvent::Down, 0), None);
    let Some(Command::FactoryReset { id }) = ui.button(ButtonEvent::Select, 0) else {
        panic!("confirming must reset the pairings");
    };
    ui.event(Event::Disconnected);
    ui.event(Event::ManagementResult { id, result: Ok(()) });
    assert_eq!(ui.state.message, "Pairings reset");
    assert_eq!(ui.button(ButtonEvent::Select, 0), None);
    assert_eq!(ui.state.screen, Screen::Home);
}

#[test]
fn failed_management_shows_the_coordinator_error() {
    let mut ui = listing(&[(7, "Keyboard")]);
    ui.button(ButtonEvent::Select, 0);
    ui.button(ButtonEvent::Down, 0);
    let Some(Command::Forget { id, .. }) = ui.button(ButtonEvent::Select, 0) else {
        panic!("confirming must forget the device");
    };
    ui.event(Event::ManagementResult {
        id: id.wrapping_add(1),
        result: Ok(()),
    });
    assert_eq!(ui.state.screen, Screen::Managing);
    ui.event(Event::ManagementResult {
        id,
        result: Err(ErrorTag::StorageFailed),
    });
    assert_eq!(ui.state.screen, Screen::Error);
    assert_eq!(ui.state.message, "Storage failed");
}

#[test]
fn forget_of_an_entry_missing_from_the_snapshot_sends_nothing() {
    let mut ui = UiController::<Addr>::new();
    let _ = ui.state.paired_names.push(name("Keyboard"));
    ui.state.screen = Screen::ConfirmForget(0);
    ui.state.selected = 1;
    assert_eq!(ui.button(ButtonEvent::Select, 0), None);
    assert_eq!(ui.state.screen, Screen::Error);
    assert_eq!(ui.state.message, "Device changed; retry");
    // No request is left waiting for a reply.
    assert!(matches!(
        ui.button(ButtonEvent::Up, 0),
        Some(Command::ListPaired { .. })
    ));
}

#[test]
fn a_command_that_could_not_be_queued_reports_busy_and_frees_the_request() {
    let mut ui = UiController::<Addr>::new();
    let command = ui.button(ButtonEvent::Up, 0).unwrap();
    ui.command_not_sent(&command);
    assert_eq!(ui.state.screen, Screen::Error);
    assert_eq!(ui.state.message, "Busy; try again");
    let Some(Command::ListPaired { id }) = ui.button(ButtonEvent::Up, 0) else {
        panic!("a new list request must start");
    };
    assert_ne!(Some(id), command.request_id());
    let mut ui = UiController::<Addr>::new();
    ui.command_not_sent(&Command::StartScan);
    assert_eq!(ui.state.message, "Busy; try again");
}

#[test]
fn tick_animates_only_a_visible_scan() {
    let mut ui = UiController::<Addr>::new();
    ui.button(ButtonEvent::Select, 0);
    assert!(!ui.tick(1_000, true));
    assert_eq!(ui.state.scan_dots, 1);
    assert!(!ui.tick(2_000, false));
    assert_eq!(ui.state.scan_dots, 1);
    ui.event(Event::ScanComplete);
    ui.tick(3_000, true);
    assert_eq!(ui.state.scan_dots, 1);
}

#[test]
fn tick_abandons_an_unanswered_request_at_its_deadline() {
    let mut ui = UiController::<Addr>::new();
    let deadline = UI_MANAGEMENT_TIMEOUT_SECS * 1000 + 5;
    let Some(Command::ListPaired { id }) = ui.button(ButtonEvent::Up, 5) else {
        panic!("UP on Home must list the saved devices");
    };
    assert!(!ui.tick(deadline - 1, true));
    assert_eq!(ui.state.screen, Screen::Managing);
    assert!(ui.tick(deadline, true));
    assert_eq!(ui.state.screen, Screen::NoReply);
    assert_eq!(ui.state.message, "List not loaded");
    assert!(!ui.tick(deadline + 1_000, true));
    // The late reply changes nothing.
    ui.event(Event::PairedDevices {
        id,
        devices: [device(7, "Keyboard")].into_iter().collect(),
    });
    assert_eq!(ui.state.screen, Screen::NoReply);
    assert!(ui.state.paired_names.is_empty());
}

#[test]
fn every_error_has_a_distinct_message_that_fits_the_screen() {
    let tags = [
        ErrorTag::ScanFailed,
        ErrorTag::ConnectFailed,
        ErrorTag::HidNotFound,
        ErrorTag::NotifyFailed,
        ErrorTag::StorageFailed,
        ErrorTag::ManagementFailed,
        ErrorTag::ReportMapReadFailed,
        ErrorTag::ReportMapTooLarge,
        ErrorTag::ReportMapInvalid,
    ];
    for (index, &tag) in tags.iter().enumerate() {
        let message = error_message(tag);
        assert!(!message.is_empty());
        assert!(message.len() <= SCREEN_COLUMNS, "{message}");
        for &other in &tags[index + 1..] {
            assert_ne!(message, error_message(other));
        }
        let mut ui = UiController::<Addr>::new();
        ui.event(Event::Error(tag));
        assert_eq!(ui.state.screen, Screen::Error);
        assert_eq!(ui.state.message, message);
    }
}
