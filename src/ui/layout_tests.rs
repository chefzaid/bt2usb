//! Host tests for the screen layout: each screen's lines, list scrolling and
//! selection marks, and the panel's size and font limits.

use super::*;

fn name(value: &str) -> String<32> {
    let mut name = String::new();
    let _ = name.push_str(value);
    name
}

fn state(screen: Screen) -> UiState {
    let mut state = UiState::new();
    state.screen = screen;
    state
}

fn shown(state: &UiState) -> std::vec::Vec<(i32, std::string::String)> {
    lines(state)
        .iter()
        .map(|line| (line.y, line.text.as_str().into()))
        .collect()
}

fn texts(state: &UiState) -> std::vec::Vec<std::string::String> {
    shown(state).into_iter().map(|(_, text)| text).collect()
}

/// One state per screen, with the longest fixed content each can show.
fn every_screen() -> std::vec::Vec<UiState> {
    let mut states = std::vec::Vec::new();
    for screen in [
        Screen::Home,
        Screen::Scanning,
        Screen::DeviceList,
        Screen::Connecting,
        Screen::Connected,
        Screen::Error,
        Screen::SavedDevices,
        Screen::ConfirmForget(0),
        Screen::ConfirmForget(9),
        Screen::ConfirmReset,
        Screen::Managing,
        Screen::Notice,
        Screen::NoReply,
    ] {
        for selected in 0..2 {
            let mut state = state(screen);
            state.selected = selected;
            states.push(state);
        }
    }
    states
}

#[test]
fn home_shows_the_title_and_both_button_hints() {
    assert_eq!(
        shown(&state(Screen::Home)),
        [
            (10, "bt2usb / Idle".into()),
            (30, "SELECT: scan".into()),
            (48, "UP: saved devices".into()),
        ]
    );
}

#[test]
fn scanning_animates_up_to_three_dots_then_starts_over() {
    let mut scanning = state(Screen::Scanning);
    for (dots, expected) in [
        (0, ""),
        (1, "."),
        (2, ".."),
        (3, "..."),
        (4, ""),
        (255, "..."),
    ] {
        scanning.scan_dots = dots;
        assert_eq!(texts(&scanning), ["Scanning", expected], "{dots} dots");
    }
}

#[test]
fn waiting_screens_show_one_line() {
    assert_eq!(
        shown(&state(Screen::Connecting)),
        [(20, "Connecting...".into())]
    );
    assert_eq!(
        shown(&state(Screen::Managing)),
        [(20, "Please wait...".into())]
    );
}

#[test]
fn device_list_marks_the_selection_and_scrolls_to_keep_it_visible() {
    let mut list = state(Screen::DeviceList);
    for index in 0..6 {
        let _ = list.devices.push(name(&format!("Device {index}")));
    }
    list.selected = 1;
    assert_eq!(
        shown(&list),
        [
            (10, "Select device".into()),
            (23, "  Device 0".into()),
            (33, "> Device 1".into()),
            (43, "  Device 2".into()),
            (53, "  Device 3".into()),
        ]
    );
    list.selected = 5;
    assert_eq!(
        texts(&list),
        [
            "Select device",
            "  Device 2",
            "  Device 3",
            "  Device 4",
            "> Device 5"
        ]
    );
}

#[test]
fn an_empty_device_list_shows_only_its_title() {
    assert_eq!(texts(&state(Screen::DeviceList)), ["Select device"]);
}

#[test]
fn saved_list_ends_with_factory_reset_and_says_how_to_go_back() {
    let mut saved = state(Screen::SavedDevices);
    let _ = saved.paired_names.push(name("Mouse"));
    let _ = saved.paired_names.push(name("Keyboard"));
    saved.selected = 2;
    assert_eq!(
        shown(&saved),
        [
            (10, "Saved devices".into()),
            (23, "  Mouse".into()),
            (33, "  Keyboard".into()),
            (43, "> Factory reset".into()),
            (63, "UP at first: back".into()),
        ]
    );
    saved.paired_names.clear();
    saved.selected = 0;
    assert_eq!(
        texts(&saved),
        ["Saved devices", "> Factory reset", "UP at first: back"]
    );
}

#[test]
fn forget_confirmation_names_the_device_and_defaults_to_cancel() {
    let mut confirm = state(Screen::ConfirmForget(1));
    let _ = confirm.paired_names.push(name("Mouse"));
    let _ = confirm.paired_names.push(name("Keyboard"));
    assert_eq!(
        shown(&confirm),
        [
            (10, "Forget device?".into()),
            (24, "Keyboard".into()),
            (40, "> Cancel".into()),
            (54, "  Forget".into()),
        ]
    );
    confirm.selected = 1;
    assert_eq!(texts(&confirm)[2..], ["  Cancel", "> Forget"]);
    // The list was refreshed under the dialog and the index is gone.
    confirm.screen = Screen::ConfirmForget(2);
    assert_eq!(texts(&confirm)[1], "Device unavailable");
}

#[test]
fn reset_confirmation_warns_that_every_link_drops() {
    let mut confirm = state(Screen::ConfirmReset);
    assert_eq!(
        texts(&confirm),
        [
            "Reset all pairings?",
            "Disconnect all",
            "> Cancel",
            "  Reset"
        ]
    );
    confirm.selected = 1;
    assert_eq!(texts(&confirm)[2..], ["  Cancel", "> Reset"]);
}

#[test]
fn status_screens_show_the_name_or_message_under_the_title() {
    let mut connected = state(Screen::Connected);
    connected.connected_name = name("2 devices");
    assert_eq!(
        shown(&connected),
        [
            (10, "Connected".into()),
            (24, "2 devices".into()),
            (40, "SEL:add DOWN:disc".into()),
            (54, "UP:saved devices".into()),
        ]
    );
    let mut error = state(Screen::Error);
    error.error("Connect failed");
    assert_eq!(
        texts(&error),
        [
            "ERROR",
            "Connect failed",
            "SEL:retry DOWN:back",
            "UP:saved devices"
        ]
    );
    let mut notice = state(Screen::Home);
    notice.notice("Device forgotten");
    assert_eq!(
        texts(&notice),
        ["Complete", "Device forgotten", "SELECT: back"]
    );
    let mut no_reply = state(Screen::NoReply);
    let _ = no_reply.message.push_str("Reset result unknown");
    assert_eq!(
        texts(&no_reply),
        [
            "No reply",
            "Reset result unknown",
            "SELECT: back",
            "UP:saved devices"
        ]
    );
}

#[test]
fn every_fixed_label_fits_across_the_panel() {
    for state in every_screen() {
        for line in lines(&state) {
            assert!(
                line.text.chars().count() <= SCREEN_COLUMNS,
                "{:?}: {:?} is wider than the panel",
                state.screen,
                line.text
            );
        }
    }
    let mut saved = state(Screen::SavedDevices);
    saved.selected = 0;
    assert!(texts(&saved)[1].len() <= SCREEN_COLUMNS); // "> Factory reset"
}

#[test]
fn lines_stay_on_the_panel_and_never_overlap() {
    const FONT_HEIGHT: i32 = 10;
    const BASELINE: i32 = 7;
    for state in every_screen() {
        let lines = lines(&state);
        for line in &lines {
            assert!(
                line.y - BASELINE >= 0,
                "{:?}: line above the panel",
                state.screen
            );
            assert!(line.y <= 63, "{:?}: baseline below the panel", state.screen);
        }
        for pair in lines.windows(2) {
            assert!(
                pair[1].y - pair[0].y >= FONT_HEIGHT,
                "{:?}: {:?} overlaps {:?}",
                state.screen,
                pair[0].text,
                pair[1].text
            );
        }
    }
}

#[test]
fn a_name_wider_than_the_panel_is_kept_whole_for_the_panel_to_cut() {
    let long = "A keyboard with a 32-byte name!!";
    assert_eq!(long.len(), 32);
    let mut list = state(Screen::DeviceList);
    let _ = list.devices.push(name(long));
    let row = &texts(&list)[1];
    assert_eq!(row, &format!("> {long}"));
    // The panel shows the first 21 characters: "> " and 19 of the name.
    assert_eq!(&row[..SCREEN_COLUMNS], "> A keyboard with a 3");
}
