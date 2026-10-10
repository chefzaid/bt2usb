//! Host tests: which report is the keyboard's, and the OEM-reserved byte of a
//! keyboard report the Report Map declares (`classify_known`,
//! `classify_gatt_notification`, `HidDescriptor::is_keyboard_report`).

use super::hid::report_protocol::{HidDescriptor, ReportKind, ReportReference};
use super::hid::{classify_known, classify_notification_with_hint, HidReport};
use super::hid_descriptor_tests::kbd_desc;

// ── Which report is the keyboard's ──────────────────────────────────────────

#[test]
fn an_unnumbered_keyboard_only_map_owns_every_report() {
    let desc = kbd_desc(None);
    assert!(desc.is_unnumbered_keyboard_only());
    // Any Report Reference ID, including 0, names the keyboard's report.
    assert!(desc.is_keyboard_report(0));
    assert!(desc.is_keyboard_report(7));
}

#[test]
fn a_numbered_map_owns_only_the_keyboard_id() {
    let desc = kbd_desc(Some(1));
    assert!(!desc.is_unnumbered_keyboard_only());
    assert!(desc.is_keyboard_report(1));
    assert!(!desc.is_keyboard_report(0));
    assert!(!desc.is_keyboard_report(2));
}

#[test]
fn an_unnumbered_map_with_another_input_owns_no_report() {
    for desc in [
        HidDescriptor {
            has_mouse: true,
            ..kbd_desc(None)
        },
        HidDescriptor {
            has_consumer: true,
            ..kbd_desc(None)
        },
        HidDescriptor {
            has_keyboard: false,
            has_mouse: true,
            ..kbd_desc(None)
        },
    ] {
        assert!(!desc.is_unnumbered_keyboard_only());
        assert!(!desc.is_keyboard_report(0));
    }
}

#[test]
fn a_report_id_shared_by_two_kinds_is_not_the_keyboard() {
    let desc = HidDescriptor {
        has_mouse: true,
        mouse_report_id: Some(1),
        ..kbd_desc(Some(1))
    };
    assert!(!desc.is_keyboard_report(1));
}

// ── Reserved keyboard byte ──────────────────────────────────────────────────

/// A keyboard report pressing Left Shift + A whose reserved byte carries OEM
/// data, as some keyboards send it.
const OEM_RESERVED: [u8; 8] = [0x02, 0xA5, 0x04, 0, 0, 0, 0, 0];

fn assert_shift_a(report: Option<HidReport>) {
    let Some(HidReport::Keyboard(report)) = report else {
        panic!("a declared keyboard report must be accepted, got {report:?}");
    };
    assert_eq!(report.modifier, 0x02);
    assert_eq!(report.keycodes, [0x04, 0, 0, 0, 0, 0]);
    // The OEM byte is discarded, so the USB report carries zero there.
    assert_eq!(report.reserved, 0);
    let mut usb = [0xFF; 8];
    assert_eq!(report.serialize(&mut usb), 8);
    assert_eq!(usb, [0x02, 0x00, 0x04, 0, 0, 0, 0, 0]);
}

#[test]
fn a_keyboard_report_reference_ignores_the_reserved_byte() {
    use super::hid::classify_gatt_notification;
    assert_shift_a(classify_known(ReportKind::Keyboard, &OEM_RESERVED));
    assert_shift_a(classify_gatt_notification(
        &OEM_RESERVED,
        Some(ReportKind::Keyboard),
        Some(&kbd_desc(Some(1))),
    ));
}

#[test]
fn a_numbered_report_map_ignores_the_reserved_byte() {
    use super::hid::classify_gatt_notification;
    // A keyboard with media keys: keyboard input as report 1, consumer input
    // as report 2.
    let desc = HidDescriptor::parse(&[
        0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, // keyboard application
        0x85, 0x01, 0x05, 0x07, 0x81, 0x02, // ID 1, keyboard input
        0xC0, //
        0x05, 0x0C, 0x09, 0x01, 0xA1, 0x01, // consumer control application
        0x85, 0x02, 0x81, 0x02, // ID 2, consumer input
        0xC0,
    ])
    .unwrap();
    // As `subscribe_all` does: the characteristic's Report Reference (ID 1,
    // Input) is mapped through the Report Map to a kind, and the GATT value
    // carries no report-ID prefix.
    let reference = ReportReference::parse(&[0x01, 0x01]).unwrap();
    assert!(reference.is_input());
    let kind = desc.report_kind_for_id(reference.report_id);
    assert_eq!(kind, Some(ReportKind::Keyboard));
    assert_shift_a(classify_gatt_notification(&OEM_RESERVED, kind, Some(&desc)));
}

#[test]
fn a_keyboard_only_unnumbered_map_ignores_the_reserved_byte() {
    use super::hid::classify_gatt_notification;
    assert_shift_a(classify_gatt_notification(
        &OEM_RESERVED,
        None,
        Some(&kbd_desc(None)),
    ));
    assert_shift_a(classify_notification_with_hint(
        &OEM_RESERVED,
        Some(&kbd_desc(None)),
    ));
}

#[test]
fn an_unnumbered_map_with_other_kinds_keeps_the_reserved_byte_check() {
    // The map does not say which kind an unnumbered 8-byte report is, so the
    // payload check still decides.
    let desc = HidDescriptor {
        has_mouse: true,
        ..kbd_desc(None)
    };
    assert!(classify_notification_with_hint(&OEM_RESERVED, Some(&desc)).is_none());
    let mut clean = OEM_RESERVED;
    clean[1] = 0;
    assert!(matches!(
        classify_notification_with_hint(&clean, Some(&desc)),
        Some(HidReport::Keyboard(_))
    ));
}

#[test]
fn the_legacy_paths_keep_the_reserved_byte_check() {
    use super::hid::{classify_gatt_notification, classify_notification, classify_report};
    // No Report Map: only the length says this is a keyboard report.
    assert!(classify_gatt_notification(&OEM_RESERVED, None, None).is_none());
    assert!(classify_report(0, &OEM_RESERVED).is_none());
    // A conventional report ID is a guess too.
    assert!(classify_report(1, &OEM_RESERVED).is_none());
    let mut prefixed = [0u8; 9];
    prefixed[0] = 1;
    prefixed[1..].copy_from_slice(&OEM_RESERVED);
    assert!(classify_notification(&prefixed).is_none());
}

#[test]
fn a_declared_keyboard_report_must_still_be_eight_bytes() {
    assert!(classify_known(ReportKind::Keyboard, &OEM_RESERVED[..7]).is_none());
    let mut long = [0u8; 9];
    long[..8].copy_from_slice(&OEM_RESERVED);
    assert!(classify_known(ReportKind::Keyboard, &long).is_none());
}
