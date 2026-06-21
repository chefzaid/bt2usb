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
//! The keyboard/mouse report *layouts* are boot-protocol compatible (fixed
//! 8-byte keyboard, 4-byte mouse), but the interfaces are exposed as standard
//! HID interfaces (Report Protocol, not the USB HID Boot subclass). They
//! therefore work under any OS HID driver but are not guaranteed to work in a
//! pre-OS/BIOS environment, which needs the Boot subclass.
//!
//! The USB task reads HID reports from the BLE→USB channel and writes
//! them to the correct HID endpoint.

pub mod hid_device;
