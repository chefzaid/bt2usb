//! Pure UI state-machine logic (functional core for the main UI loop).
//!
//! Holds the `Screen`/`ButtonEvent` types and the **button transition reducer**:
//! given the current screen and a button press, it returns the next screen plus
//! the side effects to perform (which BLE command to send, what to redraw) as
//! data. The `main.rs` loop is the imperative shell that applies the outcome
//! (channel send + OLED draw). Being I/O-free, this is host-unit-tested
//! (Layer 2 — see `TESTING.md`).

/// Screens (views) the UI can be in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Screen {
    /// Idle / home - shows connection status.
    Home,
    /// Scanning for BLE devices - shows spinner/progress.
    Scanning,
    /// Device list - user picks one to connect.
    DeviceList,
    /// Connected - shows active device info.
    Connected,
    /// Error - shows a transient message.
    Error,
}

/// Physical button events (after debouncing).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ButtonEvent {
    Up,
    Down,
    Select,
}

/// A BLE command the UI wants sent as a result of a button press.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiCommand {
    StartScan,
    Connect(usize),
    Disconnect,
}

/// Which view the shell should redraw after applying an outcome. The shell owns
/// the data (device list, connected name) needed to actually render.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Redraw {
    None,
    Scanning,
    DeviceList,
    Home,
}

/// The result of handling a button press: the new UI state plus the side
/// effects to perform.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ButtonOutcome {
    /// Screen to switch to.
    pub screen: Screen,
    /// New selection index.
    pub selected: usize,
    /// Whether the shell should clear the cached device list + count.
    pub reset_devices: bool,
    /// BLE command to send, if any.
    pub command: Option<UiCommand>,
    /// What to redraw.
    pub redraw: Redraw,
}

/// Decide the next UI state + side effects for a button press.
///
/// Pure: `selected`/`device_count` are the current values from the shell; the
/// returned `ButtonOutcome` tells the shell what to apply.
pub fn on_button(
    screen: Screen,
    btn: ButtonEvent,
    selected: usize,
    device_count: usize,
) -> ButtonOutcome {
    let mut out = ButtonOutcome {
        screen,
        selected,
        reset_devices: false,
        command: None,
        redraw: Redraw::None,
    };

    match (screen, btn) {
        // Start a scan from Home or after an error.
        (Screen::Home, ButtonEvent::Select) | (Screen::Error, ButtonEvent::Select) => {
            out.screen = Screen::Scanning;
            out.selected = 0;
            out.reset_devices = true;
            out.command = Some(UiCommand::StartScan);
            out.redraw = Redraw::Scanning;
        }

        // Navigate the device list.
        (Screen::DeviceList, ButtonEvent::Up) => {
            out.selected = selected.saturating_sub(1);
            out.redraw = Redraw::DeviceList;
        }
        (Screen::DeviceList, ButtonEvent::Down) => {
            let next = if selected + 1 < device_count {
                selected + 1
            } else {
                selected
            };
            if next != selected {
                out.selected = next;
                out.redraw = Redraw::DeviceList;
            }
        }

        // Connect to the highlighted device.
        (Screen::DeviceList, ButtonEvent::Select) => {
            out.screen = Screen::Scanning;
            out.command = Some(UiCommand::Connect(selected));
            out.redraw = Redraw::Scanning;
        }

        // From Connected: SELECT rescans (to add another device)...
        (Screen::Connected, ButtonEvent::Select) => {
            out.screen = Screen::Scanning;
            out.selected = 0;
            out.reset_devices = true;
            out.command = Some(UiCommand::StartScan);
            out.redraw = Redraw::Scanning;
        }
        // ...DOWN disconnects and returns home.
        (Screen::Connected, ButtonEvent::Down) => {
            out.screen = Screen::Home;
            out.command = Some(UiCommand::Disconnect);
            out.redraw = Redraw::Home;
        }

        _ => {}
    }

    out
}

/// Decide the screen to show when a scan completes, given how many devices were
/// found.
pub fn on_scan_complete(device_count: usize) -> Screen {
    if device_count > 0 {
        Screen::DeviceList
    } else {
        Screen::Error
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(out.screen, Screen::Scanning);
        assert_eq!(out.command, Some(UiCommand::Connect(2)));
        assert!(!out.reset_devices, "keep device list for the connect");
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
        let out = on_button(Screen::Home, ButtonEvent::Up, 0, 0);
        assert_eq!(out.screen, Screen::Home);
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
}
