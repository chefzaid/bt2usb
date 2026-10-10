//! The UI loop's decisions as a host-tested core: which BLE command a button
//! press sends, how the coordinator's events change the view, and when a
//! management request is abandoned.
//!
//! [`UiController`] owns the [`UiState`] view model, the pending management
//! request, and the addresses of the saved devices the UI last listed. The
//! firmware's `main` loop and the Renode simulation (`sim.rs`) both run it and
//! keep only their I/O: the command and event channels, power policy, logging,
//! and publishing the view to the display task. Generic over the address type
//! like the coordinator, so tests and the simulation substitute a stand-in for
//! the SoftDevice `Address`.

use heapless::Vec;

use crate::ble::coordinator::ErrorTag;
use crate::ble::messages::{Command, Event};
use crate::config::{MAX_PAIRED_DEVICES, UI_MANAGEMENT_TIMEOUT_SECS};
use crate::ui::input_logic::next_scan_dots;
use crate::ui::ui_logic::{ButtonEvent, Screen, UiCommand, UiState};

/// One management operation at a time, with stale replies rejected by identity
/// and a deadline after which the UI stops waiting for the reply.
#[derive(Default)]
struct ManagementRequests {
    next_id: u32,
    pending: Option<PendingRequest>,
}

#[derive(Clone, Copy)]
struct PendingRequest {
    id: u32,
    command: UiCommand,
    deadline_ms: u64,
}

impl ManagementRequests {
    /// Start `command` and return its request ID, or `None` while another
    /// request is pending. The UI waits for the reply until `deadline_ms`.
    pub fn begin(&mut self, command: UiCommand, deadline_ms: u64) -> Option<u32> {
        if self.pending.is_some() {
            return None;
        }
        self.next_id = self.next_id.wrapping_add(1);
        self.pending = Some(PendingRequest {
            id: self.next_id,
            command,
            deadline_ms,
        });
        Some(self.next_id)
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Accept the reply to the pending request, identified by its ID. Any
    /// other ID, including that of a request that already timed out, is
    /// ignored.
    pub fn complete(&mut self, id: u32) -> Option<UiCommand> {
        if self.pending.is_some_and(|pending| pending.id == id) {
            self.pending.take().map(|pending| pending.command)
        } else {
            None
        }
    }

    /// Stop waiting once the deadline has passed, and return the abandoned
    /// command. A reply that still arrives carries the abandoned ID, so
    /// [`complete`](Self::complete) ignores it.
    pub fn expire(&mut self, now_ms: u64) -> Option<UiCommand> {
        match self.pending {
            Some(pending) if now_ms >= pending.deadline_ms => {
                self.pending = None;
                Some(pending.command)
            }
            _ => None,
        }
    }
}

/// The UI loop's state: the view model, the management request awaiting its
/// reply, and the saved devices' addresses in the order the UI lists them.
pub struct UiController<A> {
    pub state: UiState,
    management: ManagementRequests,
    /// Address of each entry in `state.paired_names`, from the last list
    /// reply. A Forget names the device by this address, never by its index.
    paired: Vec<A, MAX_PAIRED_DEVICES>,
}

impl<A: Clone> Default for UiController<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Clone> UiController<A> {
    pub fn new() -> Self {
        Self {
            state: UiState::new(),
            management: ManagementRequests::default(),
            paired: Vec::new(),
        }
    }

    /// Apply a button press at `now_ms` and return the command to send the
    /// BLE coordinator, if any. Presses are ignored while a management request
    /// waits for its reply. A Forget whose entry is no longer in the listed
    /// snapshot sends nothing and shows an error instead.
    pub fn button(&mut self, button: ButtonEvent, now_ms: u64) -> Option<Command<A>> {
        if self.management.is_pending() {
            return None;
        }
        let action = self.state.button(button)?;
        let id = if action.is_management() {
            self.management
                .begin(action, now_ms + UI_MANAGEMENT_TIMEOUT_SECS * 1000)
        } else {
            None
        };
        let command = match action {
            UiCommand::StartScan => Some(Command::StartScan),
            UiCommand::Connect(index) => Some(Command::Connect(index)),
            UiCommand::Disconnect => Some(Command::Disconnect),
            UiCommand::ListPaired => id.map(|id| Command::ListPaired { id }),
            UiCommand::Forget(index) => {
                self.paired
                    .get(index)
                    .zip(id)
                    .map(|(address, id)| Command::Forget {
                        id,
                        address: address.clone(),
                    })
            }
            UiCommand::FactoryReset => id.map(|id| Command::FactoryReset { id }),
            UiCommand::Dismiss => return None,
        };
        if command.is_none() {
            if let Some(id) = id {
                self.management.complete(id);
            }
            self.state.error("Device changed; retry");
        }
        command
    }

    /// The command could not be queued for the coordinator. Never waiting on
    /// a full channel keeps the UI and BLE tasks from deadlocking, so report
    /// it and stop waiting for a reply that will not come.
    pub fn command_not_sent(&mut self, command: &Command<A>) {
        self.state.error("Busy; try again");
        if let Some(id) = command.request_id() {
            self.management.complete(id);
        }
    }

    /// Apply an event from the BLE coordinator. A list or management reply
    /// that does not match the pending request is ignored.
    pub fn event(&mut self, event: Event<A>) {
        match event {
            Event::ScanStarted => self.state.scan_started(),
            Event::DeviceFound(device) => {
                if self.state.screen == Screen::Scanning {
                    let _ = self.state.devices.push(device.name);
                }
            }
            Event::ScanComplete => self.state.scan_complete(),
            Event::Connected(name) => self.state.connection_status(Some(name)),
            Event::Disconnected => self.state.connection_status(None),
            Event::Error(tag) => self.state.error(error_message(tag)),
            Event::PairedDevices { id, devices } => {
                if self.management.complete(id) == Some(UiCommand::ListPaired) {
                    self.paired.clear();
                    self.state.paired_names.clear();
                    for device in devices {
                        let _ = self.paired.push(device.address);
                        let _ = self.state.paired_names.push(device.name);
                    }
                    if self.state.screen != Screen::Error {
                        self.state.screen = Screen::SavedDevices;
                        self.state.selected = 0;
                    }
                }
            }
            Event::ManagementResult { id, result } => {
                if let Some(action) = self.management.complete(id) {
                    match result {
                        Ok(()) => {
                            self.paired.clear();
                            self.state.management_completed(action);
                        }
                        Err(tag) => self.state.error(error_message(tag)),
                    }
                }
            }
        }
    }

    /// Once-a-second housekeeping at `now_ms`: advance the scan animation
    /// while the screen is on, and stop waiting for a management reply past
    /// its deadline, so a coordinator that never answers cannot lock the
    /// buttons. Returns whether a request was abandoned.
    pub fn tick(&mut self, now_ms: u64, display_on: bool) -> bool {
        if self.state.screen == Screen::Scanning && display_on {
            self.state.scan_dots = next_scan_dots(self.state.scan_dots);
        }
        match self.management.expire(now_ms) {
            Some(action) => {
                self.paired.clear();
                self.state.management_timed_out(action);
                true
            }
            None => false,
        }
    }
}

/// The text the UI shows for a coordinator error.
pub fn error_message(tag: ErrorTag) -> &'static str {
    match tag {
        ErrorTag::ScanFailed => "Scan failed",
        ErrorTag::ConnectFailed => "Connect failed",
        ErrorTag::HidNotFound => "No HID service",
        ErrorTag::NotifyFailed => "Notify failed",
        ErrorTag::StorageFailed => "Storage failed",
        ErrorTag::BondRefused => "Pairing not saved",
        ErrorTag::ManagementFailed => "Action failed; retry",
        ErrorTag::ReportMapReadFailed => "HID map read failed",
        ErrorTag::ReportMapTooLarge => "HID map too large",
        ErrorTag::ReportMapInvalid => "Unsupported HID map",
    }
}

#[cfg(test)]
#[path = "controller_tests.rs"]
mod tests;
