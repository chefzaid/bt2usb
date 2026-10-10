//! What each screen shows: its text lines and where they sit on the OLED.
//!
//! [`lines`] turns a [`UiState`] into at most [`MAX_LINES`] lines of text,
//! each with the baseline it is drawn at, as data. The display task draws
//! them in the 6×10 font and does nothing else, so the screen layout is
//! host-tested here, and the Renode OLED model reads the same lines back from
//! the pixels the firmware sends (docs/testing.md#renode-simulation).

use heapless::{String, Vec};

use crate::ui::input_logic::device_list_window;
use crate::ui::ui_logic::{Screen, UiState};

/// The most lines a screen uses: a list's title, four rows, and its footer.
pub const MAX_LINES: usize = 6;

/// Device-list rows that fit between a list's title and its footer.
pub const LIST_ROWS: usize = 4;

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
            push(&mut out, 10, "bt2usb / Idle");
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
            push(&mut out, 24, &state.connected_name);
            push(&mut out, 40, "SEL:add DOWN:disc");
            push(&mut out, 54, "UP:saved devices");
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
        Screen::NoReply => {
            push(&mut out, 10, "No reply");
            push(&mut out, 26, &state.message);
            push(&mut out, 44, "SELECT: back");
            push(&mut out, 58, "UP:saved devices");
        }
    }
    out
}

/// A list screen: the title, the window of rows around the selection (the
/// saved list ends with "Factory reset"), and the saved list's footer.
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
    let count = names.len() + usize::from(saved);
    for (row, index) in device_list_window(count, selected, LIST_ROWS).enumerate() {
        let name = names
            .get(index)
            .map(|name| name.as_str())
            .unwrap_or("Factory reset");
        push_row(out, 23 + row as i32 * 10, index == selected, name);
    }
    if saved {
        push(out, 63, "UP at first: back");
    }
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
