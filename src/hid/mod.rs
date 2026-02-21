//! HID report types and BLE→USB translation layer.

pub mod consumer;
pub mod keyboard;
pub mod mouse;
pub mod report_protocol;

#[cfg(test)]
mod tests;

use defmt::Format;
use report_protocol::{HidDescriptor, ReportKind};

#[derive(Clone, Format)]
pub enum HidReport {
    Keyboard(keyboard::KeyboardReport),
    Mouse(mouse::MouseReport),
    Consumer(consumer::ConsumerReport),
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
    classify_report(0, data).or_else(|| {
        if data.len() > 1 {
            classify_report(data[0], &data[1..])
        } else {
            None
        }
    })
}

pub fn classify_notification_with_hint(
    data: &[u8],
    descriptor: Option<&HidDescriptor>,
) -> Option<HidReport> {
    if let Some(desc) = descriptor {
        if desc.has_report_ids() && data.len() > 1 {
            let report_id = data[0];
            if let Some(kind) = desc.report_kind_for_id(report_id) {
                if let Some(report) = parse_by_kind(kind, &data[1..]) {
                    return Some(report);
                }
            }
        }

        // If descriptor has known capabilities but no explicit report ID map,
        // prefer direct parse first to support boot-protocol payloads.
        if let Some(report) = classify_report(0, data) {
            return Some(report);
        }
    }

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

fn infer_from_length(data: &[u8]) -> Option<HidReport> {
    match data.len() {
        8 => keyboard::KeyboardReport::from_ble_bytes(data).map(HidReport::Keyboard),
        3..=4 => mouse::MouseReport::from_ble_bytes(data).map(HidReport::Mouse),
        2 => {
            let usage = u16::from_le_bytes([data[0], data[1]]);
            if usage > 0 && usage < 0x1000 {
                consumer::ConsumerReport::from_ble_bytes(data).map(HidReport::Consumer)
            } else {
                None
            }
        }
        _ => {
            defmt::warn!("Unknown HID report length: {}", data.len());
            None
        }
    }
}
