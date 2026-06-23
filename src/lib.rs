//! Host-test entry point for bt2usb.
//!
//! The embedded firmware (`main.rs`, `#![no_std]`/`#![no_main]`) and this library
//! share the *same* pure-logic modules — there is no separate host
//! reimplementation. This crate root simply exposes the hardware-free modules so
//! they can be unit-tested on the host with `cargo test` / `mask test`.
//!
//! The SoftDevice-coupled BLE modules (`multi_conn`, `hid_client`, `scanner`) and
//! `storage`/`usb` are *not* included here; only their pure cores are
//! (`ble::adv_parser`, `ble::coordinator`).

#![cfg_attr(not(test), no_std)]

// The HID module is entirely hardware-free, so it is shared verbatim with the
// firmware (`defmt::Format` is feature-gated inside it).
pub mod hid;

#[path = "ble/adv_parser.rs"]
mod ble_adv_parser_impl;

#[path = "ble/coordinator.rs"]
mod ble_coordinator_impl;

#[path = "ble/reconnect.rs"]
mod ble_reconnect_impl;

// Pure flash-record framing (host-tested independently of the embedded
// `storage` shell, which is SoftDevice-coupled and not compiled here).
#[cfg(test)]
#[path = "storage/framing.rs"]
mod storage_framing_impl;

#[path = "power_logic.rs"]
mod power_logic_impl;
#[path = "ui/input_logic.rs"]
mod ui_input_logic_impl;
#[path = "ui/ui_logic.rs"]
mod ui_ui_logic_impl;

pub mod ble {
    pub mod adv_parser {
        pub use crate::ble_adv_parser_impl::{contains_hid_service_uuid, extract_device_name};
    }
    /// Pure BLE coordination core (connection-slot state machine + reducers).
    pub mod coordinator {
        pub use crate::ble_coordinator_impl::*;
    }
    /// Pure boot-time auto-reconnect planner (RPA resolution sequencing).
    pub mod reconnect {
        pub use crate::ble_reconnect_impl::*;
    }
}

pub mod ui {
    pub use crate::ui_ui_logic_impl::{ButtonEvent, Screen};

    pub mod input_logic {
        pub use crate::ui_input_logic_impl::next_scan_dots;
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
