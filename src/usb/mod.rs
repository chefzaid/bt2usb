//! USB Device subsystem - presents a composite HID device to the host.
//!
//! The nRF52840's built-in USB 2.0 Full-Speed controller is driven by
//! `embassy-usb`.  We create a **composite device** with three HID
//! interfaces:
//!
//! - Interface 0: Keyboard
//! - Interface 1: Mouse
//! - Interface 2: Consumer control
//!
//! The keyboard and mouse interfaces use the USB HID Boot subclass (and
//! boot-compatible report layouts), so they also work in BIOS / pre-OS
//! environments, not only once an OS HID driver has loaded.
//!
//! The USB task reads HID reports from the BLE→USB channel and writes
//! them to the correct HID endpoint.

pub mod hid_device;
