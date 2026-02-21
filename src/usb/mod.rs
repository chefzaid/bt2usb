//! USB Device subsystem - presents a composite HID device to the host.
//!
//! The nRF52840's built-in USB 2.0 Full-Speed controller is driven by
//! `embassy-usb`.  We create a **composite device** with two HID
//! interfaces:
//!
//! - Interface 0: Keyboard (boot protocol)
//! - Interface 1: Mouse    (boot protocol)
//!
//! The USB task reads HID reports from the BLE→USB channel and writes
//! them to the correct HID endpoint.

pub mod hid_device;
