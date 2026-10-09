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
fn hint_rejects_unknown_report_id() {
    // A device-defined ID absent from the descriptor must not be guessed from
    // its payload length: an unrelated/vendor report could look like a key.
    let desc = kbd_desc(Some(1));
    let data = [9, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00];
    assert!(classify_notification_with_hint(&data, Some(&desc)).is_none());
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

#[test]
fn parses_actual_usb_descriptors_without_cross_classifying_pan() {
    use super::hid::consumer::CONSUMER_REPORT_DESCRIPTOR;
    use super::hid::keyboard::KEYBOARD_REPORT_DESCRIPTOR;
    use super::hid::mouse::MOUSE_REPORT_DESCRIPTOR;

    for (bytes, expected) in [
        (KEYBOARD_REPORT_DESCRIPTOR, (true, false, false)),
        (MOUSE_REPORT_DESCRIPTOR, (false, true, false)),
        (CONSUMER_REPORT_DESCRIPTOR, (false, false, true)),
    ] {
        let desc = HidDescriptor::parse(bytes).unwrap();
        assert_eq!(
            (desc.has_keyboard, desc.has_mouse, desc.has_consumer),
            expected
        );
        // Every incomplete prefix must fail closed, even after a recognized
        // Input item, rather than returning partial routing metadata.
        for end in 0..bytes.len() {
            assert!(HidDescriptor::parse(&bytes[..end]).is_none());
        }
    }
}

#[test]
fn mouse_collection_keeps_id_through_nested_axes_and_pan() {
    let desc = HidDescriptor::parse(&[
        0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, // mouse application
        0x85, 0x07, // ID 7
        0x09, 0x01, 0xA1, 0x00, // pointer physical collection
        0x09, 0x30, 0x09, 0x31, 0x81, 0x06, // X/Y input
        0x05, 0x0C, 0x0A, 0x38, 0x02, 0x81, 0x06, // consumer AC Pan input
        0xC0, 0xC0,
    ])
    .unwrap();
    assert_eq!(desc.report_kind_for_id(7), Some(ReportKind::Mouse));
    assert!(!desc.has_consumer);
}

#[test]
fn global_push_pop_restores_usage_page_and_report_id() {
    let desc = HidDescriptor::parse(&[
        0x05, 0x07, 0x85, 0x01, // Keyboard, ID 1
        0xA4, // Push
        0x05, 0x0C, 0x85, 0x02, 0x81, 0x02, // Consumer, ID 2, Input
        0xB4, // Pop
        0x81, 0x02, // Restored Keyboard, ID 1, Input
    ])
    .unwrap();
    assert_eq!(desc.report_kind_for_id(1), Some(ReportKind::Keyboard));
    assert_eq!(desc.report_kind_for_id(2), Some(ReportKind::Consumer));
}

#[test]
fn long_item_payload_is_not_parsed_as_short_items() {
    assert!(HidDescriptor::parse(&[
        0xFE, 0x04, 0x00, // Long item: 4-byte payload, tag 0
        0x05, 0x07, 0x81, 0x02, // Looks like Keyboard Input, but is opaque
    ])
    .is_none());
    let desc = HidDescriptor::parse(&[
        0xFE, 0x01, 0x00, 0xC0, // opaque End Collection byte
        0x05, 0x07, 0x81, 0x02,
    ])
    .unwrap();
    assert!(desc.has_keyboard);
}

#[test]
fn malformed_descriptors_never_return_partial_metadata() {
    for suffix in [
        &[0x06, 0x01][..], // truncated short item
        &[0xFE][..],
        &[0xFE, 0x00][..],             // missing long tag
        &[0xFE, 0x02, 0x00, 0x05][..], // truncated long payload
        &[0xC0][..],                   // collection stack underflow
        &[0xA1, 0x00][..],             // unclosed collection
        &[0xB4][..],                   // global stack underflow
        &[0xA4][..],                   // unclosed global Push
        &[0x85, 0x00][..],             // reserved report ID 0
        &[0x86, 0x00, 0x01][..],       // two-byte report ID
        &[0xA9, 0x01][..],             // unsupported usage delimiter
    ] {
        let mut bytes = std::vec![0x05, 0x07, 0x81, 0x02];
        bytes.extend_from_slice(suffix);
        assert!(HidDescriptor::parse(&bytes).is_none(), "{suffix:?}");
    }
}

#[test]
fn parser_nesting_is_bounded() {
    assert!(HidDescriptor::parse(&[0xA4; 17]).is_none());
    let collections = [0xA1, 0x00].repeat(17);
    assert!(HidDescriptor::parse(&collections).is_none());
}

#[test]
fn untrusted_report_dimensions_cannot_overflow_routing_parser() {
    let desc = HidDescriptor::parse(&[
        0x05, 0x07, // Keyboard
        0x77, 0xFF, 0xFF, 0xFF, 0xFF, // Report Size = u32::MAX
        0x97, 0xFF, 0xFF, 0xFF, 0xFF, // Report Count = u32::MAX
        0x81, 0x02, // Input
    ])
    .unwrap();
    assert!(desc.has_keyboard);
}

#[test]
fn constant_padding_does_not_advertise_input_kind() {
    assert!(HidDescriptor::parse(&[0x05, 0x07, 0x81, 0x01]).is_none());
}

#[test]
fn unsupported_application_does_not_become_a_keyboard() {
    assert!(HidDescriptor::parse(&[
        0x05, 0x01, 0x09, 0x04, 0xA1, 0x01, // Joystick Application
        0x05, 0x07, 0x81, 0x02, // Keyboard-page input fields
        0xC0,
    ])
    .is_none());
}

#[test]
fn extended_usage_carries_its_own_application_page() {
    let desc = HidDescriptor::parse(&[
        0x05, 0x07, // Global Keyboard page
        0x0B, 0x02, 0x00, 0x01, 0x00, // 32-bit Generic Desktop/Mouse Usage
        0xA1, 0x01, 0x81, 0x02, 0xC0,
    ])
    .unwrap();
    assert!(desc.has_mouse);
    assert!(!desc.has_keyboard);
}

#[test]
fn mixed_kind_report_id_is_not_routable() {
    let desc = HidDescriptor::parse(&[
        0x85, 0x01, // ID 1
        0x05, 0x07, 0x81, 0x02, // Keyboard input
        0x05, 0x0C, 0x81, 0x02, // Consumer input in same report
    ])
    .unwrap();
    assert!(desc.has_keyboard && desc.has_consumer);
    assert_eq!(desc.report_kind_for_id(1), None);
}

#[test]
fn malformed_known_id_never_falls_back_to_another_kind() {
    let desc = kbd_desc(Some(3));
    assert!(classify_notification_with_hint(&[3, 0xE9, 0], Some(&desc)).is_none());
    assert!(classify_notification_with_hint(&[3], Some(&desc)).is_none());
    assert!(classify_notification_with_hint(&[], Some(&desc)).is_none());
}

#[test]
fn unnumbered_descriptor_rejects_unadvertised_kind() {
    assert!(classify_notification_with_hint(&[1, 2, 3], Some(&kbd_desc(None))).is_none());
}

#[test]
fn known_kind_rejects_unsupported_extended_payloads() {
    assert!(classify_known(ReportKind::Keyboard, &[0; 16]).is_none());
    assert!(classify_known(ReportKind::Mouse, &[0; 8]).is_none());
    assert!(classify_known(ReportKind::Consumer, &[0; 3]).is_none());
}

#[test]
fn all_consumer_routes_enforce_usb_descriptor_usage_range() {
    use super::hid::{classify_notification, classify_report};
    for usage in [0x1000_u16, 0xFFFF] {
        let bytes = usage.to_le_bytes();
        assert!(classify_known(ReportKind::Consumer, &bytes).is_none());
        assert!(classify_report(3, &bytes).is_none());
        // Check prefixed data directly with an explicit consumer descriptor:
        // the heuristic-only API cannot disambiguate arbitrary 3-byte inputs.
        let desc = HidDescriptor {
            has_keyboard: false,
            has_mouse: false,
            has_consumer: true,
            keyboard_report_id: None,
            mouse_report_id: None,
            consumer_report_id: Some(3),
        };
        assert!(classify_notification_with_hint(&[3, bytes[0], bytes[1]], Some(&desc)).is_none());
        assert!(classify_notification(&bytes).is_none());
    }
}

#[test]
fn report_reference_rejects_trailing_bytes() {
    assert!(ReportReference::parse(&[1, 1, 0]).is_none());
}

#[test]
fn boot_mouse_serialization_has_three_buttons_and_no_scroll() {
    use super::hid::mouse::MouseReport;
    let report = MouseReport {
        buttons: 0xFF,
        x: -5,
        y: 7,
        wheel: 9,
        pan: -2,
    };
    let mut buf = [0xAA; 5];
    assert_eq!(report.serialize_boot(&mut buf), 3);
    assert_eq!(buf, [7, 251, 7, 0xAA, 0xAA]);
    assert_eq!(report.serialize(&mut buf), 5);
    assert_eq!(buf, [0x1F, 251, 7, 9, 254]);
    let mut short = [0xAA; 2];
    assert_eq!(report.serialize_boot(&mut short), 0);
    assert_eq!(short, [0xAA; 2]);

    // Report mode declares the full i8 range; the boot format uses -127..127.
    let extreme = MouseReport {
        x: i8::MIN,
        y: i8::MIN,
        ..MouseReport::default()
    };
    extreme.serialize(&mut buf);
    assert_eq!(&buf[1..3], &[128, 128]);
    extreme.serialize_boot(&mut buf);
    assert_eq!(&buf[1..3], &[129, 129]);
}

#[test]
fn gatt_values_preserve_modifiers_and_buttons_that_resemble_ids() {
    use super::hid::classify_gatt_notification;
    for modifier in 1..=3 {
        let data = [modifier, 0, 4, 0, 0, 0, 0, 0];
        let Some(HidReport::Keyboard(report)) = classify_gatt_notification(&data, None, None)
        else {
            panic!("GATT keyboard must keep byte 0");
        };
        assert_eq!(report.modifier, modifier);
    }
    for buttons in 1..=3 {
        // Former heuristic interpreted button 2 as report ID 2 and shifted all
        // axes; a three-byte button-3 report became a consumer usage.
        for data in [&[buttons, 5, 7][..], &[buttons, 5, 7, 1][..]] {
            let Some(HidReport::Mouse(report)) = classify_gatt_notification(data, None, None)
            else {
                panic!("GATT mouse must keep byte 0");
            };
            assert_eq!((report.buttons, report.x, report.y), (buttons, 5, 7));
        }
    }
}

#[test]
fn gatt_report_map_requires_known_characteristic_kind() {
    use super::hid::classify_gatt_notification;
    let data = [2, 0, 4, 0, 0, 0, 0, 0];
    let desc = kbd_desc(Some(1));
    assert!(classify_gatt_notification(&data, None, Some(&desc)).is_none());
    let Some(HidReport::Keyboard(report)) =
        classify_gatt_notification(&data, Some(ReportKind::Keyboard), Some(&desc))
    else {
        panic!("known GATT characteristic kind must route without prefix");
    };
    assert_eq!(report.modifier, 2);
    assert_eq!(report.keycodes[0], 4);
    assert!(matches!(
        classify_gatt_notification(&data, None, Some(&kbd_desc(None))),
        Some(HidReport::Keyboard(_))
    ));
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
