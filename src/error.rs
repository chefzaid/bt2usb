//! Unified error type for bt2usb.
//!
//! We avoid `alloc` - all error variants carry only fixed-size data.
//! Implements `defmt::Format` for efficient on-target logging.

use defmt::Format;

/// Top-level error type used across the application.
#[derive(Debug, Format)]
pub enum Error {
    // BLE
    /// The SoftDevice returned a BLE-level error.
    Ble(BleError),

    /// No BLE adapter / SoftDevice could be initialised.
    BleNotAvailable,

    /// BLE scan completed but found zero peripherals.
    NoDevicesFound,

    /// The selected peripheral does not expose an HID service.
    HidServiceNotFound,

    /// The HID Report characteristic (0x2A4D) was not found.
    HidReportCharNotFound,

    /// Connection to the peripheral was lost unexpectedly.
    Disconnected,

    // USB
    /// USB stack returned an error.
    Usb,

    // Storage
    /// Flash read/write/erase failed.
    Storage,

    // UI / Display
    /// I²C transaction to the display failed.
    Display,

    // Generic
    /// Buffer too small for the requested operation.
    BufferOverflow,

    /// Operation timed out.
    Timeout,
}

/// Subset of BLE errors we propagate (keeps the enum `Copy`-friendly).
#[derive(Debug, Clone, Copy, Format)]
pub enum BleError {
    /// GAP / GATT raw error code from the SoftDevice.
    Raw(u32),
    /// Scan was cancelled or could not start.
    ScanFailed,
    /// Connection attempt failed.
    ConnectFailed,
    /// GATT discovery failed.
    DiscoveryFailed,
    /// Characteristic subscribe/notify failed.
    NotifyFailed,
}

// Convenience conversions

impl From<BleError> for Error {
    fn from(e: BleError) -> Self {
        Error::Ble(e)
    }
}
