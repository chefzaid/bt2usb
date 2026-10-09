//! Bluetooth Low Energy subsystem.
//!
//! This module drives the Nordic SoftDevice S140 in **Central** role:
//!
//! 1. **Scanner** - discovers nearby BLE peripherals advertising the
//!    HID-over-GATT Profile (HOGP).
//! 2. **HID Client** - performs GATT service/characteristic discovery
//!    on a connected peripheral and subscribes to HID Report notifications.
//! 3. **Connection coordinator** - [`multi_conn::ble_task`] owns the
//!    connection-slot state machine ([`coordinator`]), loads the paired-device
//!    store, runs scans and boot-time reconnects, and reports status changes
//!    to the UI task.
//! 4. **Connection workers** - two [`multi_conn::connection_slot_task`]s, one
//!    per link (typically a keyboard and a mouse), each connecting, pairing or
//!    encrypting, running the HID client, and reconnecting after a drop.
//!
//! Communication with other tasks is done via Embassy channels defined
//! in the crate root.

// The pure coordination core and the advertisement parser are SoftDevice-free,
// so they compile for every target (host tests, the embedded firmware, and the
// Renode `sim` build). The live BLE tasks below need the Nordic SoftDevice and
// are only compiled into the real firmware (`embedded` feature).
pub mod adv_parser;
pub mod coordinator;
pub mod long_read;
pub mod management;
pub mod reconnect;

#[cfg(feature = "embedded")]
pub mod hid_client;
#[cfg(feature = "embedded")]
pub mod multi_conn;
#[cfg(feature = "embedded")]
pub mod scanner;

#[cfg(feature = "embedded")]
mod softdevice_types {
    use super::coordinator;
    use defmt::Format;
    use heapless::String;
    use nrf_softdevice::ble::Address;

    /// Information about a discovered BLE peripheral.
    ///
    /// This is the embedded instantiation of the address-generic
    /// [`coordinator::DeviceInfo`], so the same value type flows through both the
    /// pure coordination core (host-tested) and the live BLE tasks.
    pub type DiscoveredDevice = coordinator::DeviceInfo<Address>;

    /// Commands that the UI task can send to the BLE task.
    #[derive(Clone, Format)]
    pub enum BleCommand {
        /// Start scanning for peripherals.
        StartScan,
        /// Connect to the peripheral at the given index in the discovered list.
        Connect(usize),
        /// Disconnect the currently connected peripheral.
        Disconnect,
        /// Read the current saved-device list for explicit management.
        ListPaired { id: u32 },
        /// Forget this stable identity (never a mutable list index).
        Forget { id: u32, address: Address },
        /// Deliberately remove all peers, including recovery of unreadable flash.
        FactoryReset { id: u32 },
    }

    /// Events the BLE task publishes for the UI / main loop.
    #[derive(Clone, Format)]
    pub enum BleEvent {
        /// Scan started.
        ScanStarted,
        /// A new peripheral was found during scanning.
        DeviceFound(DiscoveredDevice),
        /// Scan completed (no more results forthcoming).
        ScanComplete,
        /// Successfully connected & HID service ready.
        Connected(String<32>),
        /// Connection lost or intentionally closed.
        Disconnected,
        /// An error occurred (human-readable tag).
        Error(super::BleErrorTag),
        PairedDevices {
            id: u32,
            devices: heapless::Vec<DiscoveredDevice, { crate::config::MAX_PAIRED_DEVICES }>,
        },
        /// Correlated completion; success means the change reached persistent storage.
        ManagementResult {
            id: u32,
            result: Result<(), super::BleErrorTag>,
        },
    }
}

#[cfg(feature = "embedded")]
pub use softdevice_types::{BleCommand, BleEvent, DiscoveredDevice};

/// Serialises SoftDevice GAP scan and connect procedures.
///
/// The SoftDevice allows only one locally initiated scan *or* connection
/// establishment at a time: a second `sd_ble_gap_connect` / `scan_start` while
/// one is pending fails with `NRF_ERROR_INVALID_STATE`. Both connection slots
/// and the scanner run concurrently, so each holds this lock for the duration
/// of its procedure (connection *establishment* only, not the life of a link).
#[cfg(feature = "embedded")]
pub static GAP_PROCEDURE: embassy_sync::mutex::Mutex<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    (),
> = embassy_sync::mutex::Mutex::new(());

/// Lightweight error tag for UI display (no dynamic alloc).
///
/// Re-exported from the pure coordination core so the same tag type is shared
/// between host-tested logic and the embedded tasks. (The `sim` build refers to
/// `coordinator::ErrorTag` directly, so the alias is only needed for the live
/// firmware.)
#[cfg(feature = "embedded")]
pub use coordinator::ErrorTag as BleErrorTag;
