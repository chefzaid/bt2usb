//! HID Report Protocol parser.
//!
//! Classifies input report IDs using HID usage pages and application
//! collections. This is metadata for routing, not a field-layout decoder:
//! NKRO keyboards, packed fields and high-resolution axes still require a
//! descriptor-driven translator before their payloads can be forwarded.
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
//! - At most one report ID is retained for each supported kind
//! - Collection and global-state nesting are bounded to 16 levels
//! - Delimiter alternatives are unsupported and rejected

use heapless::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ReportKind {
    Keyboard,
    Mouse,
    Consumer,
}

/// Direction of a HID report, from a Report Reference descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ReportType {
    Input,
    Output,
    Feature,
    Other(u8),
}

impl From<u8> for ReportType {
    fn from(value: u8) -> Self {
        match value {
            1 => ReportType::Input,
            2 => ReportType::Output,
            3 => ReportType::Feature,
            other => ReportType::Other(other),
        }
    }
}

/// Parsed HID **Report Reference** descriptor (UUID `0x2908`).
///
/// Each HID Report characteristic (`0x2A4D`) carries one of these, identifying
/// which report ID and direction its notifications belong to. On a multi-report
/// device this is how we tell a keyboard report characteristic from a consumer
/// one — their notification payloads carry no report-ID prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ReportReference {
    pub report_id: u8,
    pub report_type: ReportType,
}

impl ReportReference {
    /// Parse the 2-byte descriptor value (`[report_id, report_type]`).
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() != 2 {
            return None;
        }
        Some(Self {
            report_id: data[0],
            report_type: ReportType::from(data[1]),
        })
    }

    /// `true` for input reports (the only ones we subscribe to for notifications).
    pub fn is_input(&self) -> bool {
        self.report_type == ReportType::Input
    }
}

/// Usage page codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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
        // A report containing fields for multiple kinds cannot be translated
        // by any one of the fixed-layout decoders. Never choose one by order.
        let mut found = None;
        for (id, kind) in [
            (self.keyboard_report_id, ReportKind::Keyboard),
            (self.mouse_report_id, ReportKind::Mouse),
            (self.consumer_report_id, ReportKind::Consumer),
        ] {
            if id == Some(report_id) {
                if found.is_some() {
                    return None;
                }
                found = Some(kind);
            }
        }
        found
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

        // Only routing metadata is retained. In particular, do not multiply
        // untrusted Report Size/Count values: no field offsets are used here.
        let mut globals = GlobalState::default();
        let mut global_stack: Vec<GlobalState, 16> = Vec::new();
        let mut collections: Vec<Application, 16> = Vec::new();
        let mut application = Application::default();
        let mut usage = None;

        let mut i = 0;
        while i < data.len() {
            let prefix = data[i];
            // Long items have a length and tag byte before their payload. The
            // payload must never be interpreted as a sequence of short items.
            if prefix == 0xFE {
                let length = *data.get(i + 1)? as usize;
                let end = i.checked_add(3)?.checked_add(length)?;
                data.get(i..end)?;
                i = end;
                continue;
            }
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
                return None;
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
                            if size == 0 {
                                return None;
                            }
                            // Constant padding does not identify an input.
                            if value & 0x01 == 0 {
                                let kind = if application.present {
                                    application.kind
                                } else {
                                    field_kind(globals.usage_page, usage)
                                };
                                if let Some(kind) = kind {
                                    desc.note_input(kind, globals.report_id);
                                }
                            }
                        }
                        // Collection
                        0x0A => {
                            if size != 1 {
                                return None;
                            }
                            collections.push(application).ok()?;
                            if value == 0x01 {
                                application = Application {
                                    present: true,
                                    kind: usage.and_then(application_kind),
                                };
                            }
                        }
                        // End Collection
                        0x0C => {
                            if size != 0 {
                                return None;
                            }
                            application = collections.pop()?;
                        }
                        _ => {}
                    }
                    // Per HID spec, Local items (Usage, Usage Min/Max, etc.) only
                    // apply to the next Main item. Reset after each Main item to
                    // prevent stale usage values from affecting subsequent items.
                    usage = None;
                }
                // Global items
                1 => {
                    match tag {
                        // Usage Page
                        0x00 => globals.usage_page = u16::try_from(value).ok()?,
                        // Report ID
                        0x08 => {
                            if size != 1 || value == 0 {
                                return None;
                            }
                            globals.report_id = value as u8;
                        }
                        // Push/Pop restore the global metadata, including ID.
                        0x0A if size == 0 => global_stack.push(globals).ok()?,
                        0x0B if size == 0 => globals = global_stack.pop()?,
                        0x0A | 0x0B => return None,
                        _ => {}
                    }
                }
                // Local items: Usage (tag 0x00).
                2 if tag == 0x00 && usage.is_none() => {
                    // A 32-bit Usage carries its own page in the upper half.
                    // Keep the first Usage for a Collection's identity.
                    let page = if size == 4 {
                        (value >> 16) as u16
                    } else {
                        globals.usage_page
                    };
                    usage = Some((page, value as u16));
                }
                // Alternative usage sets need a field-layout decoder.
                2 if tag == 0x0A => return None,
                _ => {}
            }

            i += 1 + size;
        }

        if !collections.is_empty() || !global_stack.is_empty() {
            return None;
        }

        if desc.has_keyboard || desc.has_mouse || desc.has_consumer {
            Some(desc)
        } else {
            #[cfg(feature = "defmt")]
            defmt::debug!("HID descriptor: no recognized usages found");
            None
        }
    }

    fn note_input(&mut self, kind: ReportKind, report_id: u8) {
        let (present, id) = match kind {
            ReportKind::Keyboard => (&mut self.has_keyboard, &mut self.keyboard_report_id),
            ReportKind::Mouse => (&mut self.has_mouse, &mut self.mouse_report_id),
            ReportKind::Consumer => (&mut self.has_consumer, &mut self.consumer_report_id),
        };
        *present = true;
        if report_id != 0 && id.is_none() {
            *id = Some(report_id);
        }
    }
}

#[derive(Clone, Copy, Default)]
struct GlobalState {
    usage_page: u16,
    report_id: u8,
}

#[derive(Clone, Copy, Default)]
struct Application {
    present: bool,
    kind: Option<ReportKind>,
}

fn application_kind((page, usage): (u16, u16)) -> Option<ReportKind> {
    match (UsagePage::from(page), usage) {
        (UsagePage::GenericDesktop, usage) => match DesktopUsage::from(usage) {
            DesktopUsage::Mouse => Some(ReportKind::Mouse),
            DesktopUsage::Keyboard => Some(ReportKind::Keyboard),
            _ => None,
        },
        (UsagePage::Consumer, 0x01) => Some(ReportKind::Consumer),
        _ => None,
    }
}

fn field_kind(page: u16, usage: Option<(u16, u16)>) -> Option<ReportKind> {
    let (page, usage) = usage.unwrap_or((page, 0));
    match UsagePage::from(page) {
        UsagePage::Keyboard => Some(ReportKind::Keyboard),
        UsagePage::Consumer => Some(ReportKind::Consumer),
        UsagePage::GenericDesktop
            if matches!(
                DesktopUsage::from(usage),
                DesktopUsage::Pointer | DesktopUsage::Mouse
            ) =>
        {
            Some(ReportKind::Mouse)
        }
        _ => None,
    }
}
