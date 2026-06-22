//! Host tests: HID report-descriptor parsing (`report_protocol`) and the
//! descriptor-guided notification classifier (`classify_notification_with_hint`).

use super::hid::report_protocol::{DesktopUsage, HidDescriptor, ReportKind, UsagePage};
use super::hid::{classify_notification_with_hint, HidReport};

// ── Usage-page / desktop-usage decoding ──────────────────────────────────────

#[test]
fn usage_page_decoding() {
    assert_eq!(UsagePage::from(0x01), UsagePage::GenericDesktop);
    assert_eq!(UsagePage::from(0x07), UsagePage::Keyboard);
    assert_eq!(UsagePage::from(0x0C), UsagePage::Consumer);
    assert_eq!(UsagePage::from(0x1234), UsagePage::Unknown(0x1234));
}

#[test]
fn desktop_usage_decoding() {
    assert_eq!(DesktopUsage::from(0x02), DesktopUsage::Mouse);
    assert_eq!(DesktopUsage::from(0x30), DesktopUsage::X);
    assert_eq!(DesktopUsage::from(0x99), DesktopUsage::Unknown(0x99));
}

// ── Descriptor parsing ───────────────────────────────────────────────────────

#[test]
fn parse_detects_keyboard_with_report_id() {
    // Report ID (1), Usage Page (Keyboard), Input.
    let desc = HidDescriptor::parse(&[0x85, 0x01, 0x05, 0x07, 0x81, 0x02]).unwrap();
    assert!(desc.has_keyboard);
    assert!(!desc.has_mouse);
    assert_eq!(desc.keyboard_report_id, Some(1));
    assert!(desc.has_report_ids());
    assert_eq!(desc.report_kind_for_id(1), Some(ReportKind::Keyboard));
    assert_eq!(desc.report_kind_for_id(9), None);
}

#[test]
fn parse_detects_mouse() {
    // Usage Page (Generic Desktop), Usage (Mouse), Input.
    let desc = HidDescriptor::parse(&[0x05, 0x01, 0x09, 0x02, 0x81, 0x02]).unwrap();
    assert!(desc.has_mouse);
    assert!(!desc.has_keyboard);
}

#[test]
fn parse_detects_consumer() {
    // Usage Page (Consumer), Input.
    let desc = HidDescriptor::parse(&[0x05, 0x0C, 0x81, 0x02]).unwrap();
    assert!(desc.has_consumer);
}

#[test]
fn parse_returns_none_for_unrecognized() {
    // Usage Page (LEDs) only — no input report of a known kind.
    assert!(HidDescriptor::parse(&[0x05, 0x08]).is_none());
    assert!(HidDescriptor::parse(&[]).is_none());
}

#[test]
fn parse_stops_on_truncated_item() {
    // 2-byte size item that runs past the end must not panic.
    assert!(HidDescriptor::parse(&[0x06, 0x01]).is_none());
}

// ── Descriptor-guided classification ────────────────────────────────────────

fn kbd_desc(report_id: Option<u8>) -> HidDescriptor {
    HidDescriptor {
        has_keyboard: true,
        has_mouse: false,
        has_consumer: false,
        keyboard_report_id: report_id,
        mouse_report_id: None,
        consumer_report_id: None,
    }
}

#[test]
fn hint_routes_by_report_id() {
    let desc = kbd_desc(Some(1));
    let data = [1, 0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00];
    assert!(matches!(
        classify_notification_with_hint(&data, Some(&desc)),
        Some(HidReport::Keyboard(_))
    ));
}

#[test]
fn hint_falls_back_on_unknown_report_id() {
    // Descriptor has report IDs but this ID isn't listed → strip prefix and
    // classify by the standard ID path.
    let desc = kbd_desc(Some(1));
    let data = [9, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00];
    assert!(matches!(
        classify_notification_with_hint(&data, Some(&desc)),
        Some(HidReport::Keyboard(_))
    ));
}

#[test]
fn hint_boot_protocol_when_no_report_ids() {
    let desc = kbd_desc(None); // descriptor present, no report IDs
    let data = [0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00];
    assert!(matches!(
        classify_notification_with_hint(&data, Some(&desc)),
        Some(HidReport::Keyboard(_))
    ));
}

#[test]
fn hint_heuristic_when_no_descriptor() {
    let data = [0x01, 0x10, 0x20, 0x00]; // 4-byte mouse
    assert!(matches!(
        classify_notification_with_hint(&data, None),
        Some(HidReport::Mouse(_))
    ));
}
