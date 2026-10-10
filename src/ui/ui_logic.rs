//! Pure UI state-machine logic (functional core for the main UI loop).
//!
//! Holds the `Screen`/`ButtonEvent` types, the **button transition reducer**,
//! and the [`UiState`] view model. Given the current screen and a button press,
//! the reducer returns the next screen and selection plus the command to send,
//! as data. The UI controller (`ui::controller`) turns that command into a BLE
//! request, and the display task renders the latest `UiState` whole, so the
//! reducer does not say what to redraw. Being I/O-free, this is
//! host-unit-tested (it is in the pure-core layer, see
//! docs/architecture.md#module-layers-and-dependency-rules).

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
    /// Waiting for the selected peer to connect and expose HID.
    Connecting,
    /// Connected - shows active device info.
    Connected,
    /// Error remains visible until a user action acknowledges it.
    Error,
    SavedDevices,
    ConfirmForget(usize),
    ConfirmReset,
    Managing,
    Notice,
    /// A management request got no reply in time; its outcome is unknown.
    NoReply,
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
    ListPaired,
    Forget(usize),
    FactoryReset,
    Dismiss,
}

impl UiCommand {
    /// Whether the command changes or reads the saved devices, which the UI
    /// tracks as one management request at a time.
    pub fn is_management(self) -> bool {
        matches!(
            self,
            UiCommand::ListPaired | UiCommand::Forget(_) | UiCommand::FactoryReset
        )
    }
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
    // Scan/connection events may invalidate the old cursor. Never dispatch a
    // connection for an empty list or an index outside the current snapshot.
    let count = match screen {
        Screen::SavedDevices => device_count.saturating_add(1), // final item = reset
        Screen::ConfirmForget(_) | Screen::ConfirmReset => 2,   // cancel / confirm
        _ => device_count,
    };
    let selected = selected.min(count.saturating_sub(1));
    let mut out = ButtonOutcome {
        screen,
        selected,
        reset_devices: false,
        command: None,
    };

    match (screen, btn) {
        (
            Screen::Home | Screen::Connected | Screen::Error | Screen::Notice | Screen::NoReply,
            ButtonEvent::Up,
        ) => {
            out.screen = Screen::Managing;
            out.selected = 0;
            out.command = Some(UiCommand::ListPaired);
        }
        (Screen::Error, ButtonEvent::Down)
        | (Screen::Notice | Screen::NoReply, ButtonEvent::Select) => {
            out.screen = Screen::Home;
            out.command = Some(UiCommand::Dismiss);
        }
        (Screen::SavedDevices, ButtonEvent::Up) => {
            if selected == 0 {
                out.screen = Screen::Home;
                out.command = Some(UiCommand::Dismiss);
            } else {
                out.selected -= 1;
            }
        }
        (Screen::SavedDevices, ButtonEvent::Down) => {
            out.selected = selected.saturating_add(1).min(device_count);
        }
        (Screen::SavedDevices, ButtonEvent::Select) => {
            out.screen = if selected < device_count {
                Screen::ConfirmForget(selected)
            } else {
                Screen::ConfirmReset
            };
            out.selected = 0; // A repeated SELECT always cancels, never deletes.
        }
        (Screen::ConfirmForget(_) | Screen::ConfirmReset, ButtonEvent::Up) => {
            out.selected = 0;
        }
        (Screen::ConfirmForget(_) | Screen::ConfirmReset, ButtonEvent::Down) => {
            out.selected = 1;
        }
        (Screen::ConfirmForget(_) | Screen::ConfirmReset, ButtonEvent::Select) if selected == 0 => {
            // The index isn't needed for cancellation; return to the saved list.
            out.screen = Screen::SavedDevices;
            out.selected = 0;
        }
        (Screen::ConfirmForget(index), ButtonEvent::Select) => {
            out.screen = Screen::Managing;
            out.command = Some(UiCommand::Forget(index));
        }
        (Screen::ConfirmReset, ButtonEvent::Select) => {
            out.screen = Screen::Managing;
            out.command = Some(UiCommand::FactoryReset);
        }
        // Start a scan from Home or after an error.
        (Screen::Home, ButtonEvent::Select) | (Screen::Error, ButtonEvent::Select) => {
            out.screen = Screen::Scanning;
            out.selected = 0;
            out.reset_devices = true;
            out.command = Some(UiCommand::StartScan);
        }

        // Navigate the device list.
        (Screen::DeviceList, ButtonEvent::Up) => {
            out.selected = selected.saturating_sub(1);
        }
        (Screen::DeviceList, ButtonEvent::Down) if selected < device_count.saturating_sub(1) => {
            out.selected = selected + 1;
        }

        // Connect to the highlighted device.
        (Screen::DeviceList, ButtonEvent::Select) if device_count > 0 => {
            out.screen = Screen::Connecting;
            out.command = Some(UiCommand::Connect(selected));
        }

        // From Connected: SELECT rescans (to add another device)...
        (Screen::Connected, ButtonEvent::Select) => {
            out.screen = Screen::Scanning;
            out.selected = 0;
            out.reset_devices = true;
            out.command = Some(UiCommand::StartScan);
        }
        // ...DOWN disconnects and returns home.
        (Screen::Connected, ButtonEvent::Down) => {
            out.screen = Screen::Home;
            out.command = Some(UiCommand::Disconnect);
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

/// Link updates must not overwrite an actionable error or a management dialog.
pub fn on_connection_status(screen: Screen, connected: bool) -> Screen {
    match screen {
        Screen::Home | Screen::Connected | Screen::Connecting => {
            if connected {
                Screen::Connected
            } else {
                Screen::Home
            }
        }
        other => other,
    }
}

/// Hardware-free view model shared by the UI shell and latest-frame renderer.
#[derive(Clone, PartialEq, Eq)]
pub struct UiState {
    pub screen: Screen,
    pub selected: usize,
    pub devices: heapless::Vec<heapless::String<32>, { crate::config::BLE_MAX_DISCOVERED }>,
    pub paired_names: heapless::Vec<heapless::String<32>, { crate::config::MAX_PAIRED_DEVICES }>,
    pub connected_name: heapless::String<32>,
    pub message: heapless::String<32>,
    pub scan_dots: u8,
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

impl UiState {
    pub fn new() -> Self {
        Self {
            screen: Screen::Home,
            selected: 0,
            devices: heapless::Vec::new(),
            paired_names: heapless::Vec::new(),
            connected_name: heapless::String::new(),
            message: heapless::String::new(),
            scan_dots: 0,
        }
    }

    pub fn button(&mut self, button: ButtonEvent) -> Option<UiCommand> {
        let count = if matches!(
            self.screen,
            Screen::SavedDevices | Screen::ConfirmForget(_) | Screen::ConfirmReset
        ) {
            self.paired_names.len()
        } else {
            self.devices.len()
        };
        let outcome = on_button(self.screen, button, self.selected, count);
        self.screen = outcome.screen;
        self.selected = outcome.selected;
        if outcome.reset_devices {
            self.devices.clear();
            self.scan_dots = 0;
        }
        if outcome.command == Some(UiCommand::Dismiss) {
            self.screen = on_connection_status(Screen::Home, !self.connected_name.is_empty());
            self.message.clear();
        }
        outcome.command
    }

    pub fn error(&mut self, message: &str) {
        self.screen = Screen::Error;
        self.set_message(message);
    }

    pub fn notice(&mut self, message: &str) {
        self.screen = Screen::Notice;
        self.set_message(message);
    }

    fn set_message(&mut self, message: &str) {
        self.message.clear();
        for ch in message.chars() {
            if self.message.push(ch).is_err() {
                break;
            }
        }
    }

    pub fn connection_status(&mut self, name: Option<heapless::String<32>>) {
        self.connected_name = name.unwrap_or_default();
        // Every scan is one the user started, so its picker stays on screen
        // even if a saved device reconnects in the background meanwhile.
        self.screen = on_connection_status(self.screen, !self.connected_name.is_empty());
        if matches!(self.screen, Screen::Home | Screen::Connected) {
            self.devices.clear();
            self.selected = 0;
        }
    }

    pub fn management_completed(&mut self, command: UiCommand) {
        self.paired_names.clear(); // refresh the authoritative snapshot on reopening
        if self.screen != Screen::Error {
            self.notice(if command == UiCommand::FactoryReset {
                "Pairings reset"
            } else {
                "Device forgotten"
            });
        }
    }

    /// The coordinator did not answer a management request in time. Claim no
    /// outcome that was not observed: drop the saved-device list so it is read
    /// again, keep an error that is already showing, and otherwise say the
    /// result is unknown.
    pub fn management_timed_out(&mut self, command: UiCommand) {
        self.paired_names.clear();
        if self.screen != Screen::Error {
            self.screen = Screen::NoReply;
            self.selected = 0;
            self.set_message(match command {
                UiCommand::Forget(_) => "Forget result unknown",
                UiCommand::FactoryReset => "Reset result unknown",
                _ => "List not loaded",
            });
        }
    }

    pub fn scan_started(&mut self) {
        // Only Home or Scanning moves to Scanning; menus and dialogs are kept.
        if matches!(self.screen, Screen::Home | Screen::Scanning) {
            self.screen = Screen::Scanning;
            self.devices.clear();
            self.selected = 0;
            self.scan_dots = 0;
        }
    }

    pub fn scan_complete(&mut self) {
        if self.screen != Screen::Scanning {
            return;
        }
        self.screen = on_scan_complete(self.devices.len());
        self.selected = self.selected.min(self.devices.len().saturating_sub(1));
        if self.screen == Screen::Error {
            self.set_message("No devices found");
        }
    }
}

#[cfg(test)]
#[path = "ui_logic_tests.rs"]
mod tests;
