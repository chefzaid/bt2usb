//! Application-wide constants and compile-time configuration.
//!
//! All hardware pin assignments, timing parameters, and protocol
//! constants live here so they can be tuned in one place.

// BLE

/// Duration of a BLE scan window (seconds).
pub const BLE_SCAN_DURATION_SECS: u64 = 8;

/// Maximum number of BLE peripherals we can discover in one scan.
pub const BLE_MAX_DISCOVERED: usize = 8;

/// BLE connection interval range (in 1.25 ms units).
/// 6 = 7.5 ms (lowest latency for HID).
pub const BLE_CONN_INTERVAL_MIN: u16 = 6;
pub const BLE_CONN_INTERVAL_MAX: u16 = 12;

/// BLE slave latency (number of connection events the peripheral can skip).
pub const BLE_SLAVE_LATENCY: u16 = 0;

/// BLE supervision timeout (in 10 ms units). 400 = 4 s.
pub const BLE_SUP_TIMEOUT: u16 = 400;

// USB

/// USB VID/PID - use the "pid.codes" open-source test VID.
/// Replace with your own allocated VID/PID for production.
pub const USB_VID: u16 = 0x1209;
pub const USB_PID: u16 = 0x0001;

/// USB device strings.
pub const USB_MANUFACTURER: &str = "bt2usb";
pub const USB_PRODUCT: &str = "BT-to-USB HID Bridge";
pub const USB_SERIAL_NUMBER: &str = "000001";

/// USB HID polling interval (ms). 1 ms = 1000 Hz for lowest latency.
pub const USB_HID_POLL_MS: u8 = 1;

// GPIO pin assignments (nRF52840-DK defaults)
//
// These are logical names; actual `embassy_nrf::peripherals::*` types are
// selected in `main.rs` via type aliases.  Adjust for your custom PCB.
//
//   Button UP      → P0.11
//   Button DOWN    → P0.12
//   Button SELECT  → P0.24
//   I²C SDA        → P0.26
//   I²C SCL        → P0.27
//   Status LED     → P0.06

/// Button debounce time (ms).
pub const BUTTON_DEBOUNCE_MS: u64 = 50;

/// Enable automatic OLED screen power-off after inactivity.
pub const SCREEN_AUTO_OFF_ENABLED: bool = true;

/// Inactivity timeout before OLED is turned off (seconds).
pub const SCREEN_AUTO_OFF_TIMEOUT_SECS: u64 = 120;

// Paired-device storage

/// Maximum number of paired devices tracked in storage.
pub const MAX_PAIRED_DEVICES: usize = 4;

/// Flash page index where pairing storage starts (4 KB per page on nRF52840).
pub const STORAGE_FLASH_PAGE_START: u32 = 240;

/// Number of flash pages reserved for pairing storage.
pub const STORAGE_FLASH_PAGE_COUNT: u32 = 4;
