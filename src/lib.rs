//! Host-test entry point for bt2usb.
//!
//! The embedded firmware (`main.rs`, `#![no_std]`/`#![no_main]`) and this library
//! share the *same* pure-logic modules — there is no separate host
//! reimplementation. This crate root simply exposes the hardware-free modules so
//! they can be unit-tested on the host with `cargo test` / `mask test`.
//!
//! The library compiles `hid`, `ble::{adv_parser, conn_params, coordinator,
//! reconnect, long_read, management}`, `ui::{ui_logic, input_logic, display_logic}` and
//! `power_logic`, plus `storage::{framing, record}` under `cfg(test)` only, and
//! `config`, whose capacities the pure modules size their buffers from.
//! The SoftDevice-coupled modules (`ble::{multi_conn, hid_client, scanner}`,
//! `storage` and `storage::codec`, `usb`, `power`, `sd_setup`, `stack`,
//! `ui::{display, buttons}`) are *not* included here.

#![cfg_attr(not(test), no_std)]
// The shared pure modules stay free of `unsafe` (docs/code-quality.md).
#![forbid(unsafe_code)]

// The firmware's constants. The pure modules read capacities such as the link
// count from here, as they do in the firmware.
pub mod config;

// The HID module is entirely hardware-free, so it is shared verbatim with the
// firmware (`defmt::Format` is feature-gated inside it).
pub mod hid;

#[path = "ble/adv_parser.rs"]
mod ble_adv_parser_impl;

#[path = "ble/conn_params.rs"]
mod ble_conn_params_impl;

#[path = "ble/coordinator.rs"]
mod ble_coordinator_impl;

#[path = "ble/reconnect.rs"]
mod ble_reconnect_impl;

#[path = "ble/long_read.rs"]
mod ble_long_read_impl;
#[path = "ble/management.rs"]
mod ble_management_impl;

// Pure flash-record framing (host-tested independently of the embedded
// `storage` shell, which is SoftDevice-coupled and not compiled here).
#[cfg(test)]
#[path = "storage/framing.rs"]
mod storage_framing_impl;

#[cfg(test)]
#[path = "storage/record.rs"]
mod storage_record_impl;

#[path = "power_logic.rs"]
mod power_logic_impl;
#[path = "ui/display_logic.rs"]
mod ui_display_logic_impl;
#[path = "ui/input_logic.rs"]
mod ui_input_logic_impl;
#[path = "ui/ui_logic.rs"]
mod ui_ui_logic_impl;

pub mod ble {
    pub mod long_read {
        pub use crate::ble_long_read_impl::*;
    }
    pub mod management {
        pub use crate::ble_management_impl::*;
    }
    pub mod adv_parser {
        pub use crate::ble_adv_parser_impl::{
            advertised_name, contains_hid_service_uuid, extract_device_name,
        };
    }
    /// Pure bounds for a peripheral's connection parameter request.
    pub mod conn_params {
        pub use crate::ble_conn_params_impl::*;
    }
    /// Pure BLE coordination core (connection-slot state machine + reducers).
    pub mod coordinator {
        pub use crate::ble_coordinator_impl::*;
    }
    /// Pure background-reconnect coordination (shared scan, sightings, duty).
    pub mod reconnect {
        pub use crate::ble_reconnect_impl::*;
    }
}

pub mod ui {
    pub mod display_logic {
        pub use crate::ui_display_logic_impl::*;
    }
    pub use crate::ui_ui_logic_impl::{ButtonEvent, Screen};

    pub mod input_logic {
        pub use crate::ui_input_logic_impl::{device_list_window, next_scan_dots};
    }

    /// Pure UI state-machine logic (screen transitions).
    pub mod ui_logic {
        pub use crate::ui_ui_logic_impl::*;
    }
}

pub mod power_logic {
    pub use crate::power_logic_impl::{next_power_state, screen_should_be_on, PowerState};
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "lib_logic_tests.rs"]
mod logic_tests;

#[cfg(test)]
#[path = "hid_descriptor_tests.rs"]
mod hid_descriptor_tests;
