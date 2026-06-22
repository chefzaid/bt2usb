//! Bluetooth Low Energy subsystem.
//!
//! This module drives the Nordic SoftDevice S140 in **Central** role:
//!
//! 1. **Scanner** - discovers nearby BLE peripherals advertising the
//!    HID-over-GATT Profile (HOGP).
//! 2. **HID Client** - performs GATT service/characteristic discovery
//!    on a connected peripheral and subscribes to HID Report notifications.
//! 3. **Connection Manager** - maintains the active connection, handles
//!    connect/disconnect flow, and reports status changes to the UI task.
//!
//! Communication with other tasks is done via Embassy channels defined
//! in the crate root.

pub mod adv_parser;
pub mod coordinator;
pub mod hid_client;
pub mod multi_conn;
pub mod scanner;

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
    Error(BleErrorTag),
}

/// Lightweight error tag for UI display (no dynamic alloc).
///
/// Re-exported from the pure coordination core so the same tag type is shared
/// between host-tested logic and the embedded tasks.
pub use coordinator::ErrorTag as BleErrorTag;
