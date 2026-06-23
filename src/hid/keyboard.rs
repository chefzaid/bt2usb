//! USB HID keyboard report (boot protocol compatible).
//!
//! Layout (8 bytes):
//! ```text
//! Byte 0: Modifier keys (bitfield)
//!         Bit 0 = Left Ctrl,  Bit 1 = Left Shift,
//!         Bit 2 = Left Alt,   Bit 3 = Left GUI,
//!         Bit 4 = Right Ctrl, Bit 5 = Right Shift,
//!         Bit 6 = Right Alt,  Bit 7 = Right GUI
//! Byte 1: Reserved (0x00)
//! Byte 2-7: Up to 6 simultaneous key codes (USB HID usage codes)
//! ```

/// Keyboard report size in bytes.
pub const KEYBOARD_REPORT_SIZE: usize = 8;

/// Standard USB HID boot-protocol keyboard report.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct KeyboardReport {
    /// Modifier key bitfield.
    pub modifier: u8,
    /// Reserved byte (always 0x00 per HID spec).
    pub reserved: u8,
    /// Up to 6 simultaneously pressed key codes.
    pub keycodes: [u8; 6],
}

impl KeyboardReport {
    /// Create an empty (all-keys-released) report.
    #[cfg(test)]
    pub const fn empty() -> Self {
        Self {
            modifier: 0,
            reserved: 0,
            keycodes: [0; 6],
        }
    }

    /// Parse from raw BLE HID notification bytes.
    ///
    /// BLE HID boot-protocol keyboard reports are identical in layout
    /// to USB boot-protocol reports, so this is a direct copy.
    pub fn from_ble_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < KEYBOARD_REPORT_SIZE {
            return None;
        }
        if data[1] != 0 {
            return None;
        }
        Some(Self {
            modifier: data[0],
            reserved: data[1],
            keycodes: [data[2], data[3], data[4], data[5], data[6], data[7]],
        })
    }

    /// Serialise into a byte slice for USB HID transmission.
    /// Returns the number of bytes written (always 8).
    pub fn serialize(&self, buf: &mut [u8]) -> usize {
        if buf.len() < KEYBOARD_REPORT_SIZE {
            return 0;
        }
        buf[0] = self.modifier;
        buf[1] = self.reserved;
        buf[2..8].copy_from_slice(&self.keycodes);
        KEYBOARD_REPORT_SIZE
    }

    /// Returns `true` if no keys are pressed (release event).
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.modifier == 0 && self.keycodes.iter().all(|&k| k == 0)
    }
}

/// Host keyboard LED state — the 1-byte HID LED **output** report the host
/// sends (USB) and which we forward to the BLE keyboard (USB→BLE pass-through).
///
/// Bit layout per HID Usage Page 0x08 (LEDs); the USB and BLE boot-protocol
/// keyboard output reports share this byte, so the pass-through is the identity
/// on the byte (we only mask off the undefined upper bits).
///
/// ```text
/// bit 0 = Num Lock,  bit 1 = Caps Lock, bit 2 = Scroll Lock,
/// bit 3 = Compose,   bit 4 = Kana
/// ```
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct KeyboardLeds(u8);

impl KeyboardLeds {
    /// Mask of defined LED bits (Num/Caps/Scroll/Compose/Kana).
    const DEFINED: u8 = 0x1F;

    /// Build from a raw HID LED report byte, masking undefined bits.
    pub const fn from_byte(byte: u8) -> Self {
        Self(byte & Self::DEFINED)
    }

    /// The wire byte to forward to the BLE keyboard's output report.
    pub const fn byte(self) -> u8 {
        self.0
    }

    /// `true` when Num Lock is on.
    pub const fn num_lock(self) -> bool {
        self.0 & 0x01 != 0
    }

    /// `true` when Caps Lock is on.
    pub const fn caps_lock(self) -> bool {
        self.0 & 0x02 != 0
    }

    /// `true` when Scroll Lock is on.
    pub const fn scroll_lock(self) -> bool {
        self.0 & 0x04 != 0
    }
}

// USB HID report descriptor for a boot-protocol keyboard

/// USB HID Report Descriptor for a standard keyboard.
///
/// This descriptor tells the USB host that we are a keyboard with:
///   - 8 modifier key bits (input)
///   - 1 reserved byte
///   - 5 LED indicators (output)
///   - 6 key code bytes (input)
pub const KEYBOARD_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x01, // Usage Page (Generic Desktop)
    0x09, 0x06, // Usage (Keyboard)
    0xA1, 0x01, // Collection (Application)
    //
    //   - Modifier keys (8 bits) -
    0x05, 0x07, //   Usage Page (Keyboard/Keypad)
    0x19, 0xE0, //   Usage Minimum (Left Control)
    0x29, 0xE7, //   Usage Maximum (Right GUI)
    0x15, 0x00, //   Logical Minimum (0)
    0x25, 0x01, //   Logical Maximum (1)
    0x75, 0x01, //   Report Size (1)
    0x95, 0x08, //   Report Count (8)
    0x81, 0x02, //   Input (Data, Variable, Absolute)
    //
    //   - Reserved byte -
    0x95, 0x01, //   Report Count (1)
    0x75, 0x08, //   Report Size (8)
    0x81, 0x01, //   Input (Constant) - padding
    //
    //   - LED output (5 bits + 3 padding) -
    0x05, 0x08, //   Usage Page (LEDs)
    0x19, 0x01, //   Usage Minimum (Num Lock)
    0x29, 0x05, //   Usage Maximum (Kana)
    0x95, 0x05, //   Report Count (5)
    0x75, 0x01, //   Report Size (1)
    0x91, 0x02, //   Output (Data, Variable, Absolute)
    0x95, 0x01, //   Report Count (1)
    0x75, 0x03, //   Report Size (3)
    0x91, 0x01, //   Output (Constant) - padding
    //
    //   - Key codes (6 bytes) -
    0x05, 0x07, //   Usage Page (Keyboard/Keypad)
    0x19, 0x00, //   Usage Minimum (0)
    0x29, 0xFF, //   Usage Maximum (255)
    0x15, 0x00, //   Logical Minimum (0)
    0x26, 0xFF, 0x00, // Logical Maximum (255)
    0x95, 0x06, //   Report Count (6)
    0x75, 0x08, //   Report Size (8)
    0x81, 0x00, //   Input (Data, Array)
    //
    0xC0, // End Collection
];

#[cfg(test)]
mod led_tests {
    use super::KeyboardLeds;

    #[test]
    fn decodes_individual_leds() {
        assert!(KeyboardLeds::from_byte(0x01).num_lock());
        assert!(KeyboardLeds::from_byte(0x02).caps_lock());
        assert!(KeyboardLeds::from_byte(0x04).scroll_lock());

        let none = KeyboardLeds::from_byte(0x00);
        assert!(!none.num_lock() && !none.caps_lock() && !none.scroll_lock());
    }

    #[test]
    fn caps_and_num_together() {
        let leds = KeyboardLeds::from_byte(0x03);
        assert!(leds.num_lock() && leds.caps_lock());
        assert!(!leds.scroll_lock());
    }

    #[test]
    fn masks_undefined_upper_bits_and_round_trips() {
        // Bits 5-7 are undefined and must be stripped so the byte we forward
        // to the BLE keyboard is well-formed.
        assert_eq!(KeyboardLeds::from_byte(0xE5).byte(), 0x05);
        assert_eq!(KeyboardLeds::from_byte(0x07).byte(), 0x07);
    }

    #[test]
    fn default_is_all_off() {
        assert_eq!(KeyboardLeds::default().byte(), 0);
    }
}
