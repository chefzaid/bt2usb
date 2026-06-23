//! USB HID mouse report.
//!
//! Boot-mouse compatible: a boot host reads only bytes 0–2 (buttons, X, Y) and
//! ignores the rest, so the extra wheel/pan/buttons are spec-safe to append.
//!
//! Layout (5 bytes):
//! ```text
//! Byte 0: Button bitfield
//!         Bit 0 = Left, Bit 1 = Right, Bit 2 = Middle,
//!         Bit 3 = Back (4), Bit 4 = Forward (5)
//! Byte 1: X displacement (signed, -127..127)
//! Byte 2: Y displacement (signed, -127..127)
//! Byte 3: Vertical scroll wheel  (signed, -127..127)
//! Byte 4: Horizontal scroll / AC Pan (signed, -127..127)
//! ```

/// Mouse report size in bytes.
pub const MOUSE_REPORT_SIZE: usize = 5;

/// USB HID mouse report: 5 buttons, relative X/Y, vertical wheel, horizontal pan.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct MouseReport {
    /// Button bitfield (bit 0 = left, 1 = right, 2 = middle, 3 = back, 4 = forward).
    pub buttons: u8,
    /// Relative X movement (signed).
    pub x: i8,
    /// Relative Y movement (signed).
    pub y: i8,
    /// Vertical scroll wheel delta (signed).
    pub wheel: i8,
    /// Horizontal scroll / AC Pan delta (signed).
    pub pan: i8,
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
            pan: 0,
        }
    }

    /// Parse from raw BLE HID notification bytes.
    ///
    /// Accepts boot-style 3-byte (no wheel) and 4-byte (with wheel) reports as
    /// well as extended 5-byte reports that add a horizontal-scroll (pan) byte.
    /// Absent trailing fields default to 0; all button bits are preserved so
    /// 4- and 5-button mice work.
    pub fn from_ble_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 3 {
            return None;
        }
        Some(Self {
            buttons: data[0],
            x: data[1] as i8,
            y: data[2] as i8,
            wheel: if data.len() >= 4 { data[3] as i8 } else { 0 },
            pan: if data.len() >= 5 { data[4] as i8 } else { 0 },
        })
    }

    /// Serialise into a byte slice for USB HID transmission.
    /// Returns the number of bytes written (always 5).
    pub fn serialize(&self, buf: &mut [u8]) -> usize {
        if buf.len() < MOUSE_REPORT_SIZE {
            return 0;
        }
        buf[0] = self.buttons;
        buf[1] = self.x as u8;
        buf[2] = self.y as u8;
        buf[3] = self.wheel as u8;
        buf[4] = self.pan as u8;
        MOUSE_REPORT_SIZE
    }

    /// Combine an older pending report with a `newer` one into a single report,
    /// accumulating relative movement (saturating at the `i8` range) and taking
    /// the newer button state.
    ///
    /// Used when reports pile up behind a busy USB sink: mouse motion is
    /// relative, so coalescing by summing deltas preserves total travel instead
    /// of discarding it (see [`crate::hid::coalesce`]).
    pub fn merged_with(&self, newer: &Self) -> Self {
        Self {
            buttons: newer.buttons,
            x: self.x.saturating_add(newer.x),
            y: self.y.saturating_add(newer.y),
            wheel: self.wheel.saturating_add(newer.wheel),
            pan: self.pan.saturating_add(newer.pan),
        }
    }

    /// Returns `true` when no buttons are pressed and there is no movement.
    #[cfg(test)]
    pub fn is_idle(&self) -> bool {
        self.buttons == 0 && self.x == 0 && self.y == 0 && self.wheel == 0 && self.pan == 0
    }
}

// USB HID report descriptor for a 5-button mouse with wheel + horizontal pan.

/// USB HID Report Descriptor for a 5-button mouse with vertical wheel and
/// horizontal scroll (AC Pan). Bytes 0–2 keep the boot-mouse layout so the
/// device still works under a boot/BIOS host.
pub const MOUSE_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x01, // Usage Page (Generic Desktop)
    0x09, 0x02, // Usage (Mouse)
    0xA1, 0x01, // Collection (Application)
    0x09, 0x01, //   Usage (Pointer)
    0xA1, 0x00, //   Collection (Physical)
    //
    //   - Buttons (5 bits + 3 padding) -
    0x05, 0x09, //     Usage Page (Buttons)
    0x19, 0x01, //     Usage Minimum (Button 1)
    0x29, 0x05, //     Usage Maximum (Button 5)
    0x15, 0x00, //     Logical Minimum (0)
    0x25, 0x01, //     Logical Maximum (1)
    0x95, 0x05, //     Report Count (5)
    0x75, 0x01, //     Report Size (1)
    0x81, 0x02, //     Input (Data, Variable, Absolute)
    0x95, 0x01, //     Report Count (1)
    0x75, 0x03, //     Report Size (3)
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
    //   - Vertical scroll wheel -
    0x09, 0x38, //     Usage (Wheel)
    0x15, 0x81, //     Logical Minimum (-127)
    0x25, 0x7F, //     Logical Maximum (127)
    0x75, 0x08, //     Report Size (8)
    0x95, 0x01, //     Report Count (1)
    0x81, 0x06, //     Input (Data, Variable, Relative)
    //
    //   - Horizontal scroll (AC Pan) -
    0x05, 0x0C, //     Usage Page (Consumer)
    0x0A, 0x38, 0x02, // Usage (AC Pan)
    0x15, 0x81, //     Logical Minimum (-127)
    0x25, 0x7F, //     Logical Maximum (127)
    0x75, 0x08, //     Report Size (8)
    0x95, 0x01, //     Report Count (1)
    0x81, 0x06, //     Input (Data, Variable, Relative)
    //
    0xC0, //   End Collection (Physical)
    0xC0, // End Collection (Application)
];
