//! What each screen shows: its text lines and where they sit on the OLED.
//!
//! [`lines`] turns a [`UiState`] into at most [`MAX_LINES`] lines of text,
//! each with the baseline it is drawn at, as data. The display task draws
//! them in the 6×10 font and does nothing else, so the screen layout is
//! host-tested here, and the Renode OLED model reads the same lines back from
//! the pixels the firmware sends (docs/testing.md#renode-simulation).

use core::fmt::Write;

use heapless::{String, Vec};

use crate::diagnostics::{short_commit, ResetCause, ResetReasons, FIRMWARE_VERSION, SOURCE_COMMIT};
use crate::ui::input_logic::device_list_window;
use crate::ui::ui_logic::{Screen, UiState};

/// The most lines a screen uses: a list's title, four rows, and its footer.
pub const MAX_LINES: usize = 6;

/// Device-list rows that fit between a list's title and its footer.
pub const LIST_ROWS: usize = 4;

// The Connected screen has a name row for each link above its two hints.
const _: () = assert!(
    crate::config::BLE_MAX_CONNECTIONS <= 2,
    "the Connected screen names at most two devices"
);

/// Characters of the 6-pixel-wide font that fit across the 128-pixel panel;
/// the panel cuts off the rest of a longer line. Tests check fixed text
/// against it.
#[cfg(test)]
pub const SCREEN_COLUMNS: usize = 128 / 6;

/// One line of text: `y` is the alphabetic baseline in pixels from the top,
/// so the font's 10-pixel cell starts 7 pixels above it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub y: i32,
    pub text: String<36>,
}

/// The lines the display draws for `state`, top to bottom.
pub fn lines(state: &UiState) -> Vec<Line, MAX_LINES> {
    let mut out = Vec::new();
    match state.screen {
        Screen::Home => {
            let status = if state.reconnecting {
                "bt2usb / Reconnecting"
            } else {
                "bt2usb / Idle"
            };
            push(&mut out, 10, status);
            push(&mut out, 30, "SELECT: scan");
            push(&mut out, 48, "UP: saved devices");
        }
        Screen::Scanning => {
            push(&mut out, 10, "Scanning");
            push(
                &mut out,
                30,
                match state.scan_dots % 4 {
                    0 => "",
                    1 => ".",
                    2 => "..",
                    _ => "...",
                },
            );
        }
        Screen::Connecting => push(&mut out, 20, "Connecting..."),
        Screen::Managing => push(&mut out, 20, "Please wait..."),
        Screen::DeviceList => list(&mut out, &state.devices, state.selected, false),
        Screen::SavedDevices => list(&mut out, &state.paired_names, state.selected, true),
        Screen::ConfirmForget(index) => {
            let name = state
                .paired_names
                .get(index)
                .map(|name| name.as_str())
                .unwrap_or("Device unavailable");
            confirm(&mut out, "Forget device?", name, "Forget", state.selected);
        }
        Screen::ConfirmReset => {
            confirm(
                &mut out,
                "Reset all pairings?",
                "Disconnect all",
                "Reset",
                state.selected,
            );
        }
        Screen::Connected => {
            push(&mut out, 10, "Connected");
            // Each connected device's name on its own row, in slot order.
            for (row, name) in state.connected_names.iter().enumerate() {
                push(&mut out, 23 + row as i32 * 10, name);
            }
            push(&mut out, 46, "SEL:add DOWN:disc");
            push(&mut out, 58, "UP:saved devices");
        }
        Screen::Error => {
            push(&mut out, 10, "ERROR");
            push(&mut out, 26, &state.message);
            push(&mut out, 44, "SEL:retry DOWN:back");
            push(&mut out, 58, "UP:saved devices");
        }
        Screen::Notice => {
            push(&mut out, 10, "Complete");
            push(&mut out, 28, &state.message);
            push(&mut out, 48, "SELECT: back");
        }
        Screen::Paired => {
            push(&mut out, 10, "Paired");
            push(&mut out, 26, &state.message);
            push(&mut out, 44, "SELECT: back");
            push(&mut out, 58, "UP:saved devices");
        }
        Screen::BridgeInfo => bridge_info(&mut out, state.reset_reasons),
        Screen::NoReply => {
            push(&mut out, 10, "No reply");
            push(&mut out, 26, &state.message);
            push(&mut out, 44, "SELECT: back");
            push(&mut out, 58, "UP:saved devices");
        }
    }
    out
}

/// The saved list's entries after the device names, in order.
const SAVED_LIST_END: [&str; 2] = ["Factory reset", "Bridge info"];

/// A list screen: the title, the window of rows around the selection (the
/// saved list ends with [`SAVED_LIST_END`]), and the saved list's footer.
fn list(out: &mut Vec<Line, MAX_LINES>, names: &[String<32>], selected: usize, saved: bool) {
    push(
        out,
        10,
        if saved {
            "Saved devices"
        } else {
            "Select device"
        },
    );
    let end: &[&str] = if saved { &SAVED_LIST_END } else { &[] };
    let count = names.len() + end.len();
    for (row, index) in device_list_window(count, selected, LIST_ROWS).enumerate() {
        let label = match names.get(index) {
            Some(name) => name.as_str(),
            None => end.get(index - names.len()).copied().unwrap_or_default(),
        };
        push_row(out, 23 + row as i32 * 10, index == selected, label);
    }
    if saved {
        push(out, 63, "UP at first: back");
    }
}

/// The firmware's identity and why it last restarted: the first cause, and
/// how many more `POWER.RESETREAS` recorded.
fn bridge_info(out: &mut Vec<Line, MAX_LINES>, reasons: ResetReasons) {
    push(out, 10, "Bridge info");
    let mut line = String::<32>::new();
    let _ = write!(line, "Version {}", FIRMWARE_VERSION);
    push(out, 23, &line);
    line.clear();
    let _ = write!(line, "Commit {}", short_commit(SOURCE_COMMIT));
    push(out, 33, &line);
    line.clear();
    let mut causes = reasons.causes();
    let first = causes.next().map_or("", ResetCause::short_name);
    let _ = write!(line, "Reset: {}", first);
    let more = causes.count();
    if more > 0 {
        let _ = write!(line, " +{}", more);
    }
    push(out, 43, &line);
    push(out, 58, "SELECT: back");
}

/// A confirmation: Cancel (the default, row 0) above the action (row 1).
fn confirm(
    out: &mut Vec<Line, MAX_LINES>,
    title: &str,
    subject: &str,
    action: &str,
    selected: usize,
) {
    push(out, 10, title);
    push(out, 24, subject);
    push_row(out, 40, selected == 0, "Cancel");
    push_row(out, 54, selected == 1, action);
}

/// A selectable row: "> " marks the selection, two spaces keep the others
/// aligned with it.
fn push_row(out: &mut Vec<Line, MAX_LINES>, y: i32, selected: bool, label: &str) {
    let mut text = String::<36>::new();
    let _ = text.push_str(if selected { "> " } else { "  " });
    let _ = text.push_str(label);
    let _ = out.push(Line { y, text });
}

fn push(out: &mut Vec<Line, MAX_LINES>, y: i32, value: &str) {
    let mut text = String::new();
    // Every label is a literal or a String<32>, so it always fits.
    let _ = text.push_str(value);
    let _ = out.push(Line { y, text });
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
