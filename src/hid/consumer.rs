//! Consumer Control HID support - media keys, volume, etc.
//!
//! Consumer Control is a separate HID usage page (0x0C) that handles:
//! - Volume Up/Down/Mute
//! - Play/Pause/Stop/Next/Previous
//! - Browser controls (Back, Forward, Home)
//! - Power controls (Sleep, Wake)
//!
//! This is transmitted as a separate USB HID report alongside
//! keyboard and mouse reports.

/// Consumer control report size (2 bytes for usage ID).
pub const CONSUMER_REPORT_SIZE: usize = 2;

/// Common consumer control usage codes (Usage Page 0x0C).
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u16)]
pub enum ConsumerUsage {
    /// No action.
    None = 0x0000,
    /// Play/Pause toggle.
    PlayPause = 0x00CD,
    /// Next track.
    NextTrack = 0x00B5,
    /// Previous track.
    PrevTrack = 0x00B6,
    /// Stop.
    Stop = 0x00B7,
    /// Volume up.
    VolumeUp = 0x00E9,
    /// Volume down.
    VolumeDown = 0x00EA,
    /// Mute toggle.
    Mute = 0x00E2,
    /// Browser home.
    BrowserHome = 0x0223,
    /// Browser back.
    BrowserBack = 0x0224,
    /// Browser forward.
    BrowserForward = 0x0225,
    /// Browser refresh.
    BrowserRefresh = 0x0227,
    /// Launch email client.
    LaunchEmail = 0x018A,
    /// Launch calculator.
    LaunchCalculator = 0x0192,
    /// Launch file browser.
    LaunchFileBrowser = 0x0194,
    /// Sleep.
    Sleep = 0x0032,
}

#[cfg(test)]
impl From<u16> for ConsumerUsage {
    fn from(code: u16) -> Self {
        match code {
            0x00CD => ConsumerUsage::PlayPause,
            0x00B5 => ConsumerUsage::NextTrack,
            0x00B6 => ConsumerUsage::PrevTrack,
            0x00B7 => ConsumerUsage::Stop,
            0x00E9 => ConsumerUsage::VolumeUp,
            0x00EA => ConsumerUsage::VolumeDown,
            0x00E2 => ConsumerUsage::Mute,
            0x0223 => ConsumerUsage::BrowserHome,
            0x0224 => ConsumerUsage::BrowserBack,
            0x0225 => ConsumerUsage::BrowserForward,
            0x0227 => ConsumerUsage::BrowserRefresh,
            0x018A => ConsumerUsage::LaunchEmail,
            0x0192 => ConsumerUsage::LaunchCalculator,
            0x0194 => ConsumerUsage::LaunchFileBrowser,
            0x0032 => ConsumerUsage::Sleep,
            _ => ConsumerUsage::None,
        }
    }
}

/// Consumer Control HID report.
///
/// Simple 2-byte report containing a single usage code.
/// Multiple simultaneous keys are not supported in this implementation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ConsumerReport {
    /// Active consumer control usage (little-endian u16).
    pub usage: u16,
}

impl ConsumerReport {
    /// Create an empty (no keys pressed) report.
    #[cfg(test)]
    pub const fn empty() -> Self {
        Self { usage: 0 }
    }

    /// Create a report with a single usage.
    #[cfg(test)]
    pub const fn new(usage: ConsumerUsage) -> Self {
        Self {
            usage: usage as u16,
        }
    }

    /// Parse from raw BLE bytes.
    pub fn from_ble_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 2 {
            return None;
        }
        let usage = u16::from_le_bytes([data[0], data[1]]);
        Some(Self { usage })
    }

    /// Serialize to USB HID report bytes.
    pub fn serialize(&self, buf: &mut [u8]) -> usize {
        if buf.len() < CONSUMER_REPORT_SIZE {
            return 0;
        }
        let bytes = self.usage.to_le_bytes();
        buf[0] = bytes[0];
        buf[1] = bytes[1];
        CONSUMER_REPORT_SIZE
    }

    /// Check if any key is pressed.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.usage == 0
    }

    /// Get the usage as an enum.
    #[cfg(test)]
    pub fn get_usage(&self) -> ConsumerUsage {
        ConsumerUsage::from(self.usage)
    }
}

/// USB HID Report Descriptor for Consumer Control.
///
/// This is a minimal descriptor for a single 16-bit usage.
pub const CONSUMER_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x0C, // Usage Page (Consumer)
    0x09, 0x01, // Usage (Consumer Control)
    0xA1, 0x01, // Collection (Application)
    0x15, 0x00, //   Logical Minimum (0)
    0x26, 0xFF, 0x03, //   Logical Maximum (1023)
    0x19, 0x00, //   Usage Minimum (0)
    0x2A, 0xFF, 0x03, //   Usage Maximum (1023)
    0x75, 0x10, //   Report Size (16)
    0x95, 0x01, //   Report Count (1)
    0x81, 0x00, //   Input (Data, Array, Absolute)
    0xC0, // End Collection
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumer_report_empty() {
        let report = ConsumerReport::empty();
        assert!(report.is_empty());
        assert_eq!(report.get_usage(), ConsumerUsage::None);
    }

    #[test]
    fn consumer_report_volume_up() {
        let report = ConsumerReport::new(ConsumerUsage::VolumeUp);
        assert!(!report.is_empty());
        assert_eq!(report.usage, 0x00E9);
    }

    #[test]
    fn consumer_report_serialize() {
        let report = ConsumerReport::new(ConsumerUsage::PlayPause);
        let mut buf = [0u8; 2];
        let len = report.serialize(&mut buf);
        assert_eq!(len, 2);
        assert_eq!(buf, [0xCD, 0x00]); // Little-endian 0x00CD
    }

    #[test]
    fn consumer_report_from_bytes() {
        let data = [0xE9, 0x00]; // Volume Up
        let report = ConsumerReport::from_ble_bytes(&data).unwrap();
        assert_eq!(report.get_usage(), ConsumerUsage::VolumeUp);
    }
}
