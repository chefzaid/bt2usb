//! HID Report Protocol parser.
//!
//! Parses HID Report Descriptors to understand the structure of
//! non-boot-protocol HID reports. This enables support for:
//! - Devices that don't support Boot Protocol
//! - Extended reports (NKRO keyboards, high-resolution mice)
//! - Consumer control reports embedded in keyboard descriptors
//!
//! ## HID Report Descriptor Structure
//!
//! A Report Descriptor is a sequence of items that describe the
//! format of HID reports. Key items:
//! - Usage Page: Category of usages (keyboard, mouse, consumer, etc.)
//! - Usage: Specific function within a page
//! - Report ID: Identifies which report follows (if multiple)
//! - Report Size: Bits per field
//! - Report Count: Number of fields
//! - Input/Output/Feature: Direction of the report
//!
//! ## Limitations
//!
//! This implementation handles common cases but not the full HID spec:
//! - Nested collections are flattened
//! - Push/Pop state is not supported
//! - Delimiter tags are ignored

use defmt::{debug, Format};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Format)]
pub enum ReportKind {
    Keyboard,
    Mouse,
    Consumer,
}

/// Usage page codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Format)]
pub enum UsagePage {
    /// Generic Desktop (mouse, keyboard, joystick).
    GenericDesktop,
    /// Keyboard/Keypad.
    Keyboard,
    /// LEDs.
    Led,
    /// Button.
    Button,
    /// Consumer Control.
    Consumer,
    /// Unknown/unsupported.
    Unknown(u16),
}

impl From<u16> for UsagePage {
    fn from(code: u16) -> Self {
        match code {
            0x01 => UsagePage::GenericDesktop,
            0x07 => UsagePage::Keyboard,
            0x08 => UsagePage::Led,
            0x09 => UsagePage::Button,
            0x0C => UsagePage::Consumer,
            other => UsagePage::Unknown(other),
        }
    }
}

/// Generic Desktop usage codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Format)]
pub enum DesktopUsage {
    Pointer,
    Mouse,
    Keyboard,
    X,
    Y,
    Wheel,
    Unknown(u16),
}

impl From<u16> for DesktopUsage {
    fn from(code: u16) -> Self {
        match code {
            0x01 => DesktopUsage::Pointer,
            0x02 => DesktopUsage::Mouse,
            0x06 => DesktopUsage::Keyboard,
            0x30 => DesktopUsage::X,
            0x31 => DesktopUsage::Y,
            0x38 => DesktopUsage::Wheel,
            other => DesktopUsage::Unknown(other),
        }
    }
}

/// Parsed HID descriptor.
#[derive(Clone, Copy, Debug)]
pub struct HidDescriptor {
    /// Does this device have a keyboard report?
    pub has_keyboard: bool,
    /// Does this device have a mouse report?
    pub has_mouse: bool,
    /// Does this device have consumer control?
    pub has_consumer: bool,
    /// Report ID for keyboard input, when present.
    pub keyboard_report_id: Option<u8>,
    /// Report ID for mouse input, when present.
    pub mouse_report_id: Option<u8>,
    /// Report ID for consumer input, when present.
    pub consumer_report_id: Option<u8>,
}

impl HidDescriptor {
    pub fn has_report_ids(&self) -> bool {
        self.keyboard_report_id.is_some()
            || self.mouse_report_id.is_some()
            || self.consumer_report_id.is_some()
    }

    pub fn report_kind_for_id(&self, report_id: u8) -> Option<ReportKind> {
        if self.keyboard_report_id == Some(report_id) {
            return Some(ReportKind::Keyboard);
        }
        if self.mouse_report_id == Some(report_id) {
            return Some(ReportKind::Mouse);
        }
        if self.consumer_report_id == Some(report_id) {
            return Some(ReportKind::Consumer);
        }
        None
    }
}

impl HidDescriptor {
    /// Parse a HID Report Descriptor.
    pub fn parse(data: &[u8]) -> Option<Self> {
        let mut desc = HidDescriptor {
            has_keyboard: false,
            has_mouse: false,
            has_consumer: false,
            keyboard_report_id: None,
            mouse_report_id: None,
            consumer_report_id: None,
        };

        // Parser state.
        let mut usage_page: UsagePage = UsagePage::Unknown(0);
        let mut usage: u16 = 0;
        let mut report_id: u8 = 0;
        let mut report_size: u16 = 0;
        let mut report_count: u16 = 0;
        let mut _bit_offset: u16 = 0;

        let mut i = 0;
        while i < data.len() {
            let prefix = data[i];
            let tag = (prefix >> 4) & 0x0F;
            let item_type = (prefix >> 2) & 0x03;
            let size = match prefix & 0x03 {
                0 => 0,
                1 => 1,
                2 => 2,
                3 => 4,
                _ => 0,
            };

            if i + 1 + size > data.len() {
                break;
            }

            let value: u32 = match size {
                0 => 0,
                1 => data[i + 1] as u32,
                2 => u16::from_le_bytes([data[i + 1], data[i + 2]]) as u32,
                4 => u32::from_le_bytes([data[i + 1], data[i + 2], data[i + 3], data[i + 4]]),
                _ => 0,
            };

            match item_type {
                // Main items
                0 => {
                    match tag {
                        // Input
                        0x08 => {
                            let _is_array = (value & 0x02) == 0;
                            let total_bits = report_size * report_count;

                            // Detect usage types.
                            match usage_page {
                                UsagePage::Keyboard => {
                                    desc.has_keyboard = true;
                                    if report_id != 0 && desc.keyboard_report_id.is_none() {
                                        desc.keyboard_report_id = Some(report_id);
                                    }
                                }
                                UsagePage::GenericDesktop => {
                                    if matches!(
                                        DesktopUsage::from(usage),
                                        DesktopUsage::Mouse | DesktopUsage::Pointer
                                    ) {
                                        desc.has_mouse = true;
                                        if report_id != 0 && desc.mouse_report_id.is_none() {
                                            desc.mouse_report_id = Some(report_id);
                                        }
                                    }
                                }
                                UsagePage::Consumer => {
                                    desc.has_consumer = true;
                                    if report_id != 0 && desc.consumer_report_id.is_none() {
                                        desc.consumer_report_id = Some(report_id);
                                    }
                                }
                                _ => {}
                            }

                            _bit_offset += total_bits;
                        }
                        // Collection
                        0x0A => {
                            // Start of collection - track depth if needed.
                        }
                        // End Collection
                        0x0C => {
                            // End of collection.
                        }
                        _ => {}
                    }
                }
                // Global items
                1 => {
                    match tag {
                        // Usage Page
                        0x00 => usage_page = UsagePage::from(value as u16),
                        // Report ID
                        0x08 => {
                            report_id = value as u8;
                            _bit_offset = 0;
                        }
                        // Report Size
                        0x07 => report_size = value as u16,
                        // Report Count
                        0x09 => report_count = value as u16,
                        _ => {}
                    }
                }
                // Local items
                2 => {
                    if tag == 0x00 {
                        usage = value as u16;
                    }
                }
                _ => {}
            }

            i += 1 + size;
        }

        if desc.has_keyboard || desc.has_mouse || desc.has_consumer {
            Some(desc)
        } else {
            debug!("HID descriptor: no recognized usages found");
            None
        }
    }
}
