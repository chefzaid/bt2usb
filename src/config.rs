//! Application-wide constants and compile-time configuration.
//!
//! All hardware pin assignments, timing parameters, and protocol
//! constants live here so they can be tuned in one place.

// BLE

/// Simultaneous BLE links (central connections), typically a keyboard and a
/// mouse. The coordinator's slots, the slot workers and their command
/// channels, the USB report merger's sources, the host-LED receivers, and the
/// SoftDevice's connection and role counts all follow this value. Raising it
/// also needs more SoftDevice RAM, which `memory_sd.x` reserves.
pub const BLE_MAX_CONNECTIONS: usize = 2;

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

/// SoftDevice connection event length (in 1.25 ms units).
/// Must not exceed the connection interval; with two simultaneous links a
/// short event length lets the scheduler interleave both. 6 = 7.5 ms.
pub const BLE_CONN_EVENT_LENGTH: u16 = 6;

/// BLE supervision timeout (in 10 ms units). 400 = 4 s.
pub const BLE_SUP_TIMEOUT: u16 = 400;

/// How long one connection attempt scans for its peer before giving up.
///
/// The SoftDevice runs only one GAP scan/connect procedure at a time, so an
/// attempt holds the radio and a user-requested scan waits for it; this bounds
/// that wait. 6 s covers at least three of the default 1.7 s scan intervals.
pub const BLE_CONNECT_TIMEOUT_SECS: u16 = 6;

/// Pause between silent reconnect attempts to a lost or not-yet-seen paired
/// device, leaving the radio free for a user scan in between.
pub const BLE_RECONNECT_BACKOFF_MS: u64 = 500;

/// How long, after a background reconnect attempt fails, the other slot's
/// reconnect scans ignore that slot's device: the pause before the failed
/// slot's next attempt plus one full reconnect scan. A device that advertises
/// but will not connect, for example one paired again with another computer,
/// would otherwise end every scan of the other slot at its first
/// advertisement, so the other slot's own device would rarely be heard.
pub const BLE_FAILED_RECONNECT_HOLDOFF_MS: u64 =
    BLE_CONNECT_TIMEOUT_SECS as u64 * 1000 + BLE_RECONNECT_BACKOFF_MS;

/// Scan interval and window (in 0.625 ms units) for a reconnect scan inside
/// its fast window, and for every connection attempt: a 50 ms window every
/// 100 ms. A waking keyboard advertises every few tens of milliseconds, so it
/// is usually seen in the first window; the gaps leave the SoftDevice time for
/// flash writes.
pub const BLE_FAST_SCAN_INTERVAL: u32 = 160;
pub const BLE_FAST_SCAN_WINDOW: u32 = 80;

/// How long reconnect scans keep the fast duty cycle after power-up or after
/// a link is lost. After that they fall back to the SoftDevice default (a
/// 312.5 ms window every 1.7 s) until the device returns.
pub const BLE_FAST_RECONNECT_SECS: u64 = 30;

/// Longest connection interval granted, as a single value, to a peripheral
/// that asks only for intervals slower than [`BLE_CONN_INTERVAL_MAX`] (in
/// 1.25 ms units). 24 = 30 ms. Such a peripheral gets its own shortest interval
/// up to this cap rather than 15 ms, because some peripherals disconnect when
/// the interval they get lies outside the range they asked for; Nordic's nRF5
/// SDK `ble_conn_params` module, for one, can be set to do so.
pub const BLE_PEER_MAX_CONN_INTERVAL: u16 = 24;

/// Largest peripheral latency granted when a peripheral asks to change the
/// connection parameters (connection events it may skip). A host LED change
/// reaches a keyboard within (latency + 1) connection intervals, so 20 keeps
/// that under about 315 ms at the 15 ms maximum interval.
pub const BLE_MAX_PERIPHERAL_LATENCY: u16 = 20;

/// Shortest supervision timeout granted to a peripheral's request (in 10 ms
/// units). 100 = 1 s. The longest is [`BLE_SUP_TIMEOUT`].
pub const BLE_MIN_SUP_TIMEOUT: u16 = 100;

// USB

/// USB VID/PID - use the "pid.codes" open-source test VID.
/// Replace with your own allocated VID/PID for production.
pub const USB_VID: u16 = 0x1209;
pub const USB_PID: u16 = 0x0001;

/// USB device strings.
pub const USB_MANUFACTURER: &str = "bt2usb";
pub const USB_PRODUCT: &str = "BT-to-USB HID Bridge";

/// USB HID polling interval (ms). 1 ms = 1000 Hz for lowest latency.
pub const USB_HID_POLL_MS: u8 = 1;

// GPIO pin assignments (nRF52840-DK defaults)
//
// These are documentation only: `main.rs` passes the pins directly
// (`p.P0_11`, `p.P0_26`, ...) when it spawns the button tasks and builds the
// I²C bus.  Adjust them there for your custom PCB.
//
//   Button UP      → P0.11
//   Button DOWN    → P0.12
//   Button SELECT  → P0.24
//   I²C SDA        → P0.26
//   I²C SCL        → P0.27
//
// P0.06 is not used by the bridge firmware.  The Renode simulation build
// (`sim.rs`) uses it as the UARTE0 TX pin, as on the DK's VCOM UART.

/// Button debounce time (ms).
pub const BUTTON_DEBOUNCE_MS: u64 = 50;

/// Enable automatic OLED screen power-off after inactivity.
pub const SCREEN_AUTO_OFF_ENABLED: bool = true;

/// Inactivity timeout before OLED is turned off (seconds).
pub const SCREEN_AUTO_OFF_TIMEOUT_SECS: u64 = 120;

// Paired-device storage

/// Maximum number of paired devices tracked in storage.
pub const MAX_PAIRED_DEVICES: usize = 4;

/// Flash page size on the nRF52840 (4 KB), the unit the flash erases in.
pub const FLASH_PAGE_SIZE: u32 = 4096;

/// Flash page index where pairing storage starts.
pub const STORAGE_FLASH_PAGE_START: u32 = 240;

/// Number of flash pages reserved for pairing storage.
pub const STORAGE_FLASH_PAGE_COUNT: u32 = 4;

/// First byte of pairing storage (`0x000F_0000`).
///
/// `build.rs` compiles this file too and hands this address and
/// [`STORAGE_FLASH_END`] to the linker. `memory_sd.x` fails the link unless its
/// `FLASH` region ends exactly here, so code and constants can never be placed
/// on pages the store erases. Change the pages above and the `FLASH` length in
/// `memory_sd.x` together.
pub const STORAGE_FLASH_START: u32 = STORAGE_FLASH_PAGE_START * FLASH_PAGE_SIZE;

/// First byte after pairing storage (`0x000F_4000`, exclusive).
pub const STORAGE_FLASH_END: u32 = STORAGE_FLASH_START + STORAGE_FLASH_PAGE_COUNT * FLASH_PAGE_SIZE;
