//! Pure UI state-machine logic (functional core for the main UI loop).
//!
//! Holds the `Screen`/`ButtonEvent` types and the **button transition reducer**:
//! given the current screen and a button press, it returns the next screen plus
//! the side effects to perform (which BLE command to send, what to redraw) as
//! data. The `main.rs` loop is the imperative shell that applies the outcome
//! (channel send + OLED draw). Being I/O-free, this is host-unit-tested
//! (the orchestration layer of the docs/testing.md).

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

/// One management operation at a time, with stale replies rejected by identity.
#[derive(Default)]
pub struct ManagementRequests {
    next_id: u32,
    pending: Option<(u32, UiCommand)>,
}

impl ManagementRequests {
    pub fn begin(&mut self, command: UiCommand) -> Option<u32> {
        if self.pending.is_some() {
            return None;
        }
        self.next_id = self.next_id.wrapping_add(1);
        self.pending = Some((self.next_id, command));
        Some(self.next_id)
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn complete(&mut self, id: u32) -> Option<UiCommand> {
        if self.pending.is_some_and(|(pending_id, _)| pending_id == id) {
            self.pending.take().map(|(_, command)| command)
        } else {
            None
        }
    }
}

/// Which view the shell should redraw after applying an outcome. The shell owns
/// the data (device list, connected name) needed to actually render.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Redraw {
    None,
    Scanning,
    DeviceList,
    Home,
    Current,
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
        redraw: Redraw::None,
    };

    match (screen, btn) {
        (Screen::Home | Screen::Connected | Screen::Error | Screen::Notice, ButtonEvent::Up) => {
            out.screen = Screen::Managing;
            out.selected = 0;
            out.command = Some(UiCommand::ListPaired);
            out.redraw = Redraw::Current;
        }
        (Screen::Error, ButtonEvent::Down) | (Screen::Notice, ButtonEvent::Select) => {
            out.screen = Screen::Home;
            out.command = Some(UiCommand::Dismiss);
            out.redraw = Redraw::Current;
        }
        (Screen::SavedDevices, ButtonEvent::Up) => {
            if selected == 0 {
                out.screen = Screen::Home;
                out.command = Some(UiCommand::Dismiss);
            } else {
                out.selected -= 1;
            }
            out.redraw = Redraw::Current;
        }
        (Screen::SavedDevices, ButtonEvent::Down) => {
            out.selected = selected.saturating_add(1).min(device_count);
            out.redraw = Redraw::Current;
        }
        (Screen::SavedDevices, ButtonEvent::Select) => {
            out.screen = if selected < device_count {
                Screen::ConfirmForget(selected)
            } else {
                Screen::ConfirmReset
            };
            out.selected = 0; // A repeated SELECT always cancels, never deletes.
            out.redraw = Redraw::Current;
        }
        (Screen::ConfirmForget(_) | Screen::ConfirmReset, ButtonEvent::Up) => {
            out.selected = 0;
            out.redraw = Redraw::Current;
        }
        (Screen::ConfirmForget(_) | Screen::ConfirmReset, ButtonEvent::Down) => {
            out.selected = 1;
            out.redraw = Redraw::Current;
        }
        (Screen::ConfirmForget(_) | Screen::ConfirmReset, ButtonEvent::Select) if selected == 0 => {
            // The index isn't needed for cancellation; return to the saved list.
            out.screen = Screen::SavedDevices;
            out.selected = 0;
            out.redraw = Redraw::Current;
        }
        (Screen::ConfirmForget(index), ButtonEvent::Select) => {
            out.screen = Screen::Managing;
            out.command = Some(UiCommand::Forget(index));
            out.redraw = Redraw::Current;
        }
        (Screen::ConfirmReset, ButtonEvent::Select) => {
            out.screen = Screen::Managing;
            out.command = Some(UiCommand::FactoryReset);
            out.redraw = Redraw::Current;
        }
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
            let next = if selected < device_count.saturating_sub(1) {
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
        (Screen::DeviceList, ButtonEvent::Select) if device_count > 0 => {
            out.screen = Screen::Connecting;
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
    pub devices: heapless::Vec<heapless::String<32>, 8>,
    pub paired_names: heapless::Vec<heapless::String<32>, 4>,
    pub connected_name: heapless::String<32>,
    pub message: heapless::String<32>,
    pub scan_dots: u8,
    interactive_scan: bool,
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
            interactive_scan: false,
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
        if outcome.command == Some(UiCommand::StartScan) {
            self.interactive_scan = true;
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
        // Boot reconnection should reach Connected; a manual scan should retain
        // its picker even if an existing source reconnects in the background.
        if !self.interactive_scan && matches!(self.screen, Screen::Scanning | Screen::DeviceList) {
            self.screen = Screen::Home;
        }
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

    pub fn scan_started(&mut self) {
        // A boot scan can finish after the user opens management. Keep dialogs.
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
        let first = requests.begin(UiCommand::ListPaired).unwrap();
        assert!(requests.is_pending());
        assert_eq!(requests.begin(UiCommand::FactoryReset), None);
        assert_eq!(requests.complete(first.wrapping_add(1)), None);
        assert!(requests.is_pending());
        assert_eq!(requests.complete(first), Some(UiCommand::ListPaired));
        assert!(!requests.is_pending());
        // A late duplicate of the finished reply cannot complete a newer request.
        let second = requests.begin(UiCommand::Forget(0)).unwrap();
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
        let wrapped = requests.begin(UiCommand::ListPaired).unwrap();
        assert_eq!(requests.complete(u32::MAX), None);
        assert_eq!(requests.complete(wrapped), Some(UiCommand::ListPaired));
        assert_ne!(requests.begin(UiCommand::ListPaired), Some(wrapped));
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
}
