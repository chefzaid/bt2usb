//! Host-test entry point for bt2usb.
//!
//! The firmware (`main.rs`) and this library compile the same hardware-free
//! files; there is no separate host reimplementation. The inline `ble`, `ui`,
//! and `storage` modules below declare only the pure files of those
//! directories, so paths such as `crate::ble::coordinator` resolve the same way
//! here and in the firmware. The SoftDevice-, USB-, and peripheral-coupled
//! modules are not compiled here.

#![cfg_attr(not(test), no_std)]
// The shared pure modules stay free of `unsafe` (docs/code-quality.md).
#![forbid(unsafe_code)]

// The firmware's constants. The pure modules read capacities such as the link
// count from here, as they do in the firmware.
pub mod config;

// The build identity and reset-reason decoding the firmware logs at boot, and
// the event counters with the policy for logging them.
pub mod diagnostics;

// Where the MPU stack guard goes and the register values that make it.
pub mod stack_logic;

// The HID module is entirely hardware-free, so it is shared verbatim with the
// firmware (`defmt::Format` is feature-gated inside it).
pub mod hid;

// The pure BLE modules, the same files the firmware's `ble` module declares.
pub mod ble {
    pub mod adv_parser;
    pub mod bond_table;
    pub mod conn_params;
    pub mod coordinator;
    pub mod long_read;
    pub mod management;
    pub mod messages;
    pub mod pnp_id;
    pub mod reconnect;
    pub mod scan_list;
}

// The pure parts of the paired-device store: the device list, its record
// codec, and the flash-item framing (host-tested independently of the embedded
// `storage` shell, which is SoftDevice-coupled and not compiled here).
#[cfg(test)]
mod storage {
    pub mod codec;
    pub mod devices;
    pub mod framing;
    pub mod record;
}

pub mod ui {
    pub mod controller;
    pub mod display_logic;
    pub mod input_logic;
    pub mod layout;
    pub mod ui_logic;

    pub use ui_logic::{ButtonEvent, Screen};
}

pub mod power_logic;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "lib_logic_tests.rs"]
mod logic_tests;

#[cfg(test)]
#[path = "hid_descriptor_tests.rs"]
mod hid_descriptor_tests;

#[cfg(test)]
#[path = "hid_keyboard_report_tests.rs"]
mod hid_keyboard_report_tests;

#[cfg(test)]
#[path = "hid_classify_tests.rs"]
mod hid_classify_tests;
