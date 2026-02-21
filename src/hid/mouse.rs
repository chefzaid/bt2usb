//! USB HID mouse report (boot protocol compatible).
//!
//! Layout (4 bytes):
//! ```text
//! Byte 0: Button bitfield
//!         Bit 0 = Left, Bit 1 = Right, Bit 2 = Middle
//! Byte 1: X displacement (signed, -127..127)
//! Byte 2: Y displacement (signed, -127..127)
//! Byte 3: Scroll wheel  (signed, -127..127)
//! ```

/// Mouse report size in bytes.
pub const MOUSE_REPORT_SIZE: usize = 4;

/// Standard USB HID boot-protocol mouse report.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct MouseReport {
    /// Button bitfield (bit 0 = left, bit 1 = right, bit 2 = middle).
    pub buttons: u8,
    /// Relative X movement (signed).
    pub x: i8,
    /// Relative Y movement (signed).
    pub y: i8,
    /// Scroll wheel delta (signed).
    pub wheel: i8,
}

impl MouseReport {
    /// Create an idle (no movement, no buttons) report.
    #[cfg(test)]
    pub const fn empty() -> Self {
        Self {
            buttons: 0,
            x: 0,
            y: 0,
            wheel: 0,
        }
    }

    /// Parse from raw BLE HID notification bytes.
    ///
    /// Accepts 3-byte (no wheel) or 4-byte (with wheel) reports.
    pub fn from_ble_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 3 {
            return None;
        }
        Some(Self {
            buttons: data[0],
            x: data[1] as i8,
            y: data[2] as i8,
            wheel: if data.len() >= 4 { data[3] as i8 } else { 0 },
        })
    }

    /// Serialise into a byte slice for USB HID transmission.
    /// Returns the number of bytes written (always 4).
    pub fn serialize(&self, buf: &mut [u8]) -> usize {
        if buf.len() < MOUSE_REPORT_SIZE {
            return 0;
        }
        buf[0] = self.buttons;
        buf[1] = self.x as u8;
        buf[2] = self.y as u8;
        buf[3] = self.wheel as u8;
        MOUSE_REPORT_SIZE
    }

    /// Returns `true` when no buttons are pressed and there is no movement.
    #[cfg(test)]
    pub fn is_idle(&self) -> bool {
        self.buttons == 0 && self.x == 0 && self.y == 0 && self.wheel == 0
    }
}

// USB HID report descriptor for a boot-protocol mouse

/// USB HID Report Descriptor for a standard 3-button mouse with scroll wheel.
pub const MOUSE_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x01, // Usage Page (Generic Desktop)
    0x09, 0x02, // Usage (Mouse)
    0xA1, 0x01, // Collection (Application)
    0x09, 0x01, //   Usage (Pointer)
    0xA1, 0x00, //   Collection (Physical)
    //
    //   - Buttons (3 bits + 5 padding) -
    0x05, 0x09, //     Usage Page (Buttons)
    0x19, 0x01, //     Usage Minimum (Button 1)
    0x29, 0x03, //     Usage Maximum (Button 3)
    0x15, 0x00, //     Logical Minimum (0)
    0x25, 0x01, //     Logical Maximum (1)
    0x95, 0x03, //     Report Count (3)
    0x75, 0x01, //     Report Size (1)
    0x81, 0x02, //     Input (Data, Variable, Absolute)
    0x95, 0x01, //     Report Count (1)
    0x75, 0x05, //     Report Size (5)
    0x81, 0x01, //     Input (Constant) - padding
    //
    //   - X, Y displacement -
    0x05, 0x01, //     Usage Page (Generic Desktop)
    0x09, 0x30, //     Usage (X)
    0x09, 0x31, //     Usage (Y)
    0x15, 0x81, //     Logical Minimum (-127)
    0x25, 0x7F, //     Logical Maximum (127)
    0x75, 0x08, //     Report Size (8)
    0x95, 0x02, //     Report Count (2)
    0x81, 0x06, //     Input (Data, Variable, Relative)
    //
    //   - Scroll wheel -
    0x09, 0x38, //     Usage (Wheel)
    0x15, 0x81, //     Logical Minimum (-127)
    0x25, 0x7F, //     Logical Maximum (127)
    0x75, 0x08, //     Report Size (8)
    0x95, 0x01, //     Report Count (1)
    0x81, 0x06, //     Input (Data, Variable, Relative)
    //
    0xC0, //   End Collection (Physical)
    0xC0, // End Collection (Application)
];
