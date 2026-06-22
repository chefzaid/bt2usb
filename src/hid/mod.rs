//! HID report types and the BLE→USB classification/translation layer.
//!
//! This module is `no_std` and free of hardware dependencies, so it is shared
//! verbatim between the firmware and the host test suite (`cargo test`) — there
//! is no separate host reimplementation. `defmt::Format` is derived only when
//! the `defmt` feature is on (firmware builds).

pub mod consumer;
pub mod keyboard;
pub mod mouse;
pub mod report_protocol;

use report_protocol::{HidDescriptor, ReportKind};

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum HidReport {
    Keyboard(keyboard::KeyboardReport),
    Mouse(mouse::MouseReport),
    Consumer(consumer::ConsumerReport),
}

impl HidReport {
    /// Serialize into the USB HID report wire format, returning the byte count.
    pub fn serialize(&self, buf: &mut [u8]) -> usize {
        match self {
            HidReport::Keyboard(k) => k.serialize(buf),
            HidReport::Mouse(m) => m.serialize(buf),
            HidReport::Consumer(c) => c.serialize(buf),
        }
    }

    #[cfg(test)]
    pub fn is_keyboard(&self) -> bool {
        matches!(self, HidReport::Keyboard(_))
    }

    #[cfg(test)]
    pub fn is_mouse(&self) -> bool {
        matches!(self, HidReport::Mouse(_))
    }

    #[cfg(test)]
    pub fn is_consumer(&self) -> bool {
        matches!(self, HidReport::Consumer(_))
    }
}

pub fn classify_report(report_id: u8, data: &[u8]) -> Option<HidReport> {
    match report_id {
        1 => keyboard::KeyboardReport::from_ble_bytes(data).map(HidReport::Keyboard),
        2 => mouse::MouseReport::from_ble_bytes(data).map(HidReport::Mouse),
        3 => consumer::ConsumerReport::from_ble_bytes(data).map(HidReport::Consumer),
        _ => infer_from_length(data),
    }
}

pub fn classify_notification(data: &[u8]) -> Option<HidReport> {
    classify_report_id_prefix(data).or_else(|| classify_report(0, data))
}

pub fn classify_notification_with_hint(
    data: &[u8],
    descriptor: Option<&HidDescriptor>,
) -> Option<HidReport> {
    if let Some(desc) = descriptor {
        if desc.has_report_ids() && data.len() > 1 {
            let report_id = data[0];
            if let Some(kind) = desc.report_kind_for_id(report_id) {
                // Descriptor recognises this report ID — parse with type info.
                if let Some(report) = parse_by_kind(kind, &data[1..]) {
                    return Some(report);
                }
            }
            // Descriptor has report IDs but this ID is unrecognised.
            // Still strip the first byte and try standard ID-based classification
            // (handles devices that send report types not listed in the descriptor).
            // Do NOT fall through to the length-based heuristic because the raw
            // payload includes a report-ID prefix that would be misinterpreted.
            return classify_report(report_id, &data[1..]);
        }

        // Descriptor present but no report IDs — boot-protocol device.
        if let Some(report) = classify_report(0, data) {
            return Some(report);
        }
    }

    // No descriptor available — heuristic fallback.
    classify_notification(data)
}

fn parse_by_kind(kind: ReportKind, data: &[u8]) -> Option<HidReport> {
    match kind {
        ReportKind::Keyboard => {
            keyboard::KeyboardReport::from_ble_bytes(data).map(HidReport::Keyboard)
        }
        ReportKind::Mouse => mouse::MouseReport::from_ble_bytes(data).map(HidReport::Mouse),
        ReportKind::Consumer => {
            consumer::ConsumerReport::from_ble_bytes(data).map(HidReport::Consumer)
        }
    }
}

fn classify_report_id_prefix(data: &[u8]) -> Option<HidReport> {
    if data.len() <= 1 {
        return None;
    }

    let payload = &data[1..];
    match data[0] {
        1 if payload.len() >= keyboard::KEYBOARD_REPORT_SIZE => {
            keyboard::KeyboardReport::from_ble_bytes(payload).map(HidReport::Keyboard)
        }
        2 if payload.len() == mouse::MOUSE_REPORT_SIZE => {
            mouse::MouseReport::from_ble_bytes(payload).map(HidReport::Mouse)
        }
        3 if payload.len() == consumer::CONSUMER_REPORT_SIZE => {
            consumer::ConsumerReport::from_ble_bytes(payload).map(HidReport::Consumer)
        }
        _ => None,
    }
}

fn infer_from_length(data: &[u8]) -> Option<HidReport> {
    match data.len() {
        8 => keyboard::KeyboardReport::from_ble_bytes(data).map(HidReport::Keyboard),
        3..=4 => mouse::MouseReport::from_ble_bytes(data).map(HidReport::Mouse),
        2 => {
            let usage = u16::from_le_bytes([data[0], data[1]]);
            // Allow usage == 0 so consumer release events (key-up) are forwarded.
            if usage < 0x1000 {
                consumer::ConsumerReport::from_ble_bytes(data).map(HidReport::Consumer)
            } else {
                None
            }
        }
        _ => {
            #[cfg(feature = "defmt")]
            defmt::warn!("Unknown HID report length: {}", data.len());
            None
        }
    }
}
