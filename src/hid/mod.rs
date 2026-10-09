//! HID report types and the BLE→USB classification/translation layer.
//!
//! This module is `no_std` and free of hardware dependencies, so it is shared
//! verbatim between the firmware and the host test suite (`cargo test`) — there
//! is no separate host reimplementation. `defmt::Format` is derived only when
//! the `defmt` feature is on (firmware builds).

pub mod aggregate;
pub mod coalesce;
pub mod consumer;
pub mod delivery;
pub mod host_leds;
pub mod keyboard;
pub mod mouse;
pub mod report_protocol;
pub mod wake;

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

/// How a report's kind was established, which decides how strictly its
/// payload is checked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum KindSource {
    /// The peer declares it: a Report Reference or the Report Map names the
    /// report's kind.
    Declared,
    /// The bridge infers it from a conventional report ID (1, 2, 3) or from
    /// the payload length, with nothing from the peer to confirm it.
    Inferred,
}

pub fn classify_report(report_id: u8, data: &[u8]) -> Option<HidReport> {
    match report_id {
        1 => parse_by_kind(ReportKind::Keyboard, data, KindSource::Inferred),
        2 => parse_by_kind(ReportKind::Mouse, data, KindSource::Inferred),
        3 => parse_by_kind(ReportKind::Consumer, data, KindSource::Inferred),
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
        if desc.has_report_ids() {
            let (&report_id, payload) = data.split_first()?;
            // Descriptor IDs are device-defined, not conventional 1/2/3 IDs.
            // Unknown IDs or malformed known reports must not become another
            // endpoint's input through a length-based fallback.
            return parse_by_kind(
                desc.report_kind_for_id(report_id)?,
                payload,
                KindSource::Declared,
            );
        }

        // No prefix is declared. A map whose only input is a keyboard declares
        // that this is the keyboard report, as it does for the LED output
        // report (`subscribe_all` in `ble/hid_client.rs`).
        if desc.has_keyboard && !desc.has_mouse && !desc.has_consumer {
            return parse_by_kind(ReportKind::Keyboard, data, KindSource::Declared);
        }

        // Otherwise only route to a kind actually advertised; having no report
        // IDs alone does not imply the boot report layout.
        let report = classify_report(0, data)?;
        return match report {
            HidReport::Keyboard(_) if desc.has_keyboard => Some(report),
            HidReport::Mouse(_) if desc.has_mouse => Some(report),
            HidReport::Consumer(_) if desc.has_consumer => Some(report),
            _ => None,
        };
    }

    // No descriptor available — heuristic fallback.
    classify_notification(data)
}

/// Classify a notification whose report kind is already known from its HID
/// Report Reference descriptor.
///
/// Unlike [`classify_notification_with_hint`], this is used when each report
/// characteristic is subscribed individually: the GATT value carries no
/// report-ID prefix, so the kind comes from the characteristic's descriptor
/// rather than from the payload.
pub fn classify_known(kind: ReportKind, data: &[u8]) -> Option<HidReport> {
    parse_by_kind(kind, data, KindSource::Declared)
}

/// Classify a HID-over-GATT Report characteristic value. GATT values never
/// carry the Report ID prefix: the Report Reference descriptor supplies it.
/// Numbered maps require a recognized characteristic kind. Unnumbered maps
/// constrain the fallback to advertised kinds, still without stripping byte 0.
pub fn classify_gatt_notification(
    data: &[u8],
    kind: Option<ReportKind>,
    descriptor: Option<&HidDescriptor>,
) -> Option<HidReport> {
    match kind {
        Some(kind) => classify_known(kind, data),
        None => match descriptor {
            Some(desc) if !desc.has_report_ids() => {
                classify_notification_with_hint(data, Some(desc))
            }
            Some(_) => None,
            None => classify_report(0, data),
        },
    }
}

fn parse_by_kind(kind: ReportKind, data: &[u8], source: KindSource) -> Option<HidReport> {
    // These decoders support only fixed byte-aligned layouts. Silently taking
    // the first bytes of NKRO/16-bit-axis reports generates unintended input.
    match (kind, data.len()) {
        (ReportKind::Keyboard, keyboard::KEYBOARD_REPORT_SIZE) => match source {
            // The peer named this its keyboard report, so the OEM-reserved
            // byte is ignored as HID 1.11 requires.
            KindSource::Declared => keyboard::KeyboardReport::from_identified_bytes(data),
            KindSource::Inferred => keyboard::KeyboardReport::from_ble_bytes(data),
        }
        .map(HidReport::Keyboard),
        (ReportKind::Mouse, 3..=mouse::MOUSE_REPORT_SIZE) => {
            mouse::MouseReport::from_ble_bytes(data).map(HidReport::Mouse)
        }
        (ReportKind::Consumer, consumer::CONSUMER_REPORT_SIZE) => {
            consumer::ConsumerReport::from_ble_bytes(data).map(HidReport::Consumer)
        }
        _ => None,
    }
}

fn classify_report_id_prefix(data: &[u8]) -> Option<HidReport> {
    if data.len() <= 1 {
        return None;
    }

    let payload = &data[1..];
    match data[0] {
        1 if payload.len() == keyboard::KEYBOARD_REPORT_SIZE => {
            keyboard::KeyboardReport::from_ble_bytes(payload).map(HidReport::Keyboard)
        }
        2 if (3..=mouse::MOUSE_REPORT_SIZE).contains(&payload.len()) => {
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
        3..=5 => mouse::MouseReport::from_ble_bytes(data).map(HidReport::Mouse),
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
