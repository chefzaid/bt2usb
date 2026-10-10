//! Commands the UI sends the BLE coordinator and the events it reports back.
//!
//! Generic over the address type, like [`DeviceInfo`], so the same messages
//! flow through the firmware (`BleCommand` and `BleEvent` are these types over
//! the SoftDevice `Address`), the host tests, and the Renode simulation, which
//! uses a `u32` stand-in.

use heapless::{String, Vec};

use crate::ble::coordinator::{DeviceInfo, ErrorTag, UiEvent};
use crate::config::MAX_PAIRED_DEVICES;

/// A request from the UI to the BLE coordinator.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Command<A> {
    /// Start scanning for peripherals.
    StartScan,
    /// Connect to the peripheral at this index in the last scan's results.
    Connect(usize),
    /// Disconnect every link.
    Disconnect,
    /// Read the current saved-device list for explicit management.
    ListPaired { id: u32 },
    /// Forget this stable identity (never a mutable list index).
    Forget { id: u32, address: A },
    /// Deliberately remove all peers, including recovery of unreadable flash.
    FactoryReset { id: u32 },
}

impl<A> Command<A> {
    /// The management request ID this command carries, if it is one.
    pub fn request_id(&self) -> Option<u32> {
        match self {
            Self::ListPaired { id } | Self::Forget { id, .. } | Self::FactoryReset { id } => {
                Some(*id)
            }
            Self::StartScan | Self::Connect(_) | Self::Disconnect => None,
        }
    }
}

/// An event from the BLE coordinator (and the scanner) for the UI.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Event<A> {
    /// Scan started.
    ScanStarted,
    /// A new peripheral was found during scanning.
    DeviceFound(DeviceInfo<A>),
    /// Scan completed (no more results forthcoming).
    ScanComplete,
    /// At least one link is up; the summary names it or counts them.
    Connected(String<32>),
    /// No link is up.
    Disconnected,
    /// An error the UI shows until the user acknowledges it.
    Error(ErrorTag),
    /// The saved devices, newest first, in reply to `ListPaired { id }`.
    PairedDevices {
        id: u32,
        devices: Vec<DeviceInfo<A>, MAX_PAIRED_DEVICES>,
    },
    /// Correlated completion; success means the change reached persistent storage.
    ManagementResult {
        id: u32,
        result: Result<(), ErrorTag>,
    },
}

impl<A> Event<A> {
    /// Whether this event reports that a link is up (`Some(true)`) or that
    /// none is (`Some(false)`); `None` for every other event.
    pub fn link_up(&self) -> Option<bool> {
        match self {
            Self::Connected(_) => Some(true),
            Self::Disconnected => Some(false),
            _ => None,
        }
    }
}

impl<A> From<UiEvent> for Event<A> {
    fn from(event: UiEvent) -> Self {
        match event {
            UiEvent::Connected(summary) => Self::Connected(summary),
            UiEvent::Disconnected => Self::Disconnected,
            UiEvent::Error(tag) => Self::Error(tag),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(value: &str) -> String<32> {
        let mut name = String::new();
        let _ = name.push_str(value);
        name
    }

    #[test]
    fn only_management_commands_carry_a_request_id() {
        assert_eq!(Command::<u8>::ListPaired { id: 3 }.request_id(), Some(3));
        assert_eq!(
            Command::Forget {
                id: 4,
                address: 9u8
            }
            .request_id(),
            Some(4)
        );
        assert_eq!(Command::<u8>::FactoryReset { id: 5 }.request_id(), Some(5));
        for command in [
            Command::<u8>::StartScan,
            Command::Connect(1),
            Command::Disconnect,
        ] {
            assert_eq!(command.request_id(), None);
        }
    }

    #[test]
    fn link_state_comes_only_from_connected_and_disconnected() {
        assert_eq!(
            Event::<u8>::Connected(name("Keyboard")).link_up(),
            Some(true)
        );
        assert_eq!(Event::<u8>::Disconnected.link_up(), Some(false));
        for event in [
            Event::<u8>::ScanStarted,
            Event::ScanComplete,
            Event::Error(ErrorTag::ConnectFailed),
            Event::ManagementResult {
                id: 1,
                result: Ok(()),
            },
            Event::PairedDevices {
                id: 1,
                devices: Vec::new(),
            },
        ] {
            assert_eq!(event.link_up(), None);
        }
    }

    #[test]
    fn coordinator_ui_events_map_to_the_same_ui_events() {
        assert_eq!(
            Event::<u8>::from(UiEvent::Connected(name("2 devices"))),
            Event::Connected(name("2 devices"))
        );
        assert_eq!(
            Event::<u8>::from(UiEvent::Disconnected),
            Event::Disconnected
        );
        assert_eq!(
            Event::<u8>::from(UiEvent::Error(ErrorTag::HidNotFound)),
            Event::Error(ErrorTag::HidNotFound)
        );
    }
}
