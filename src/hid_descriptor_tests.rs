//! Host tests: HID report-descriptor parsing (`report_protocol`) and the
//! descriptor-guided notification classifier (`classify_notification_with_hint`).

use super::hid::report_protocol::{
    DesktopUsage, HidDescriptor, ReportKind, ReportReference, ReportType, UsagePage,
};
use super::hid::{classify_known, classify_notification_with_hint, HidReport};

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

// ── Report Reference descriptor parsing ──────────────────────────────────────

#[test]
fn report_reference_parses_id_and_type() {
    let r = ReportReference::parse(&[3, 1]).unwrap();
    assert_eq!(r.report_id, 3);
    assert_eq!(r.report_type, ReportType::Input);
    assert!(r.is_input());

    assert_eq!(
        ReportReference::parse(&[1, 2]).unwrap().report_type,
        ReportType::Output
    );
    assert_eq!(
        ReportReference::parse(&[1, 3]).unwrap().report_type,
        ReportType::Feature
    );
    assert_eq!(
        ReportReference::parse(&[1, 9]).unwrap().report_type,
        ReportType::Other(9)
    );
}

#[test]
fn report_reference_rejects_short_descriptor() {
    assert!(ReportReference::parse(&[1]).is_none());
    assert!(ReportReference::parse(&[]).is_none());
}

#[test]
fn output_and_feature_reports_are_not_input() {
    assert!(!ReportReference::parse(&[1, 2]).unwrap().is_input()); // Output
    assert!(!ReportReference::parse(&[1, 3]).unwrap().is_input()); // Feature
}

// ── Known-kind classification (per-characteristic, no report-ID prefix) ───────

#[test]
fn classify_known_routes_each_kind() {
    // Multi-report device: per-characteristic notifications carry no ID prefix.
    let keyboard = [0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00];
    assert!(matches!(
        classify_known(ReportKind::Keyboard, &keyboard),
        Some(HidReport::Keyboard(_))
    ));

    let mouse = [0x01, 0x05, 0xFB, 0x00];
    assert!(matches!(
        classify_known(ReportKind::Mouse, &mouse),
        Some(HidReport::Mouse(_))
    ));

    let consumer = [0xE9, 0x00]; // Volume Up
    assert!(matches!(
        classify_known(ReportKind::Consumer, &consumer),
        Some(HidReport::Consumer(_))
    ));
}
