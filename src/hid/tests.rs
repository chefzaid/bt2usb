//! Unit tests for HID report parsing and serialization.
//!
//! These tests run on the host (not embedded) and verify the pure
//! logic of report classification, parsing, and serialization.

use super::keyboard::KeyboardReport;
use super::mouse::MouseReport;
use super::{classify_report, HidReport};

// ═══════════════════════════════════════════════════════════════════════════
// Keyboard Report Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn keyboard_report_empty() {
    let report = KeyboardReport::empty();
    assert!(report.is_empty());
    assert_eq!(report.modifier, 0);
    assert_eq!(report.keycodes, [0; 6]);
}

#[test]
fn keyboard_report_from_valid_ble_bytes() {
    // Modifier: Left Shift (0x02), Reserved: 0, Keys: 'A' (0x04)
    let data = [0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00];
    let report = KeyboardReport::from_ble_bytes(&data).unwrap();

    assert_eq!(report.modifier, 0x02);
    assert_eq!(report.reserved, 0x00);
    assert_eq!(report.keycodes[0], 0x04);
    assert!(!report.is_empty());
}

#[test]
fn keyboard_report_from_short_bytes_fails() {
    let data = [0x02, 0x00, 0x04]; // Only 3 bytes - too short
    assert!(KeyboardReport::from_ble_bytes(&data).is_none());
}

#[test]
fn keyboard_report_serialize_roundtrip() {
    let original = KeyboardReport {
        modifier: 0x05,
        reserved: 0x00,
        keycodes: [0x04, 0x05, 0x06, 0x00, 0x00, 0x00],
    };

    let mut buf = [0u8; 8];
    let written = original.serialize(&mut buf);

    assert_eq!(written, 8);
    assert_eq!(buf, [0x05, 0x00, 0x04, 0x05, 0x06, 0x00, 0x00, 0x00]);

    // Roundtrip: parse the serialized data back
    let parsed = KeyboardReport::from_ble_bytes(&buf).unwrap();
    assert_eq!(parsed.modifier, original.modifier);
    assert_eq!(parsed.keycodes, original.keycodes);
}

#[test]
fn keyboard_report_serialize_buffer_too_small() {
    let report = KeyboardReport::empty();
    let mut small_buf = [0u8; 4];
    let written = report.serialize(&mut small_buf);
    assert_eq!(written, 0); // Should fail gracefully
}

// ═══════════════════════════════════════════════════════════════════════════
// Mouse Report Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mouse_report_empty() {
    let report = MouseReport::empty();
    assert!(report.is_idle());
    assert_eq!(report.buttons, 0);
    assert_eq!(report.x, 0);
    assert_eq!(report.y, 0);
    assert_eq!(report.wheel, 0);
}

#[test]
fn mouse_report_from_3_byte_data() {
    // Left button pressed, X=10, Y=-5
    let data = [0x01, 0x0A, 0xFB]; // 0xFB = -5 as i8
    let report = MouseReport::from_ble_bytes(&data).unwrap();

    assert_eq!(report.buttons, 0x01);
    assert_eq!(report.x, 10);
    assert_eq!(report.y, -5);
    assert_eq!(report.wheel, 0); // Not provided, defaults to 0
}

#[test]
fn mouse_report_from_4_byte_data() {
    // Right button, X=0, Y=0, Wheel scroll up
    let data = [0x02, 0x00, 0x00, 0x01];
    let report = MouseReport::from_ble_bytes(&data).unwrap();

    assert_eq!(report.buttons, 0x02);
    assert_eq!(report.x, 0);
    assert_eq!(report.y, 0);
    assert_eq!(report.wheel, 1);
}

#[test]
fn mouse_report_from_short_bytes_fails() {
    let data = [0x01, 0x0A]; // Only 2 bytes - too short
    assert!(MouseReport::from_ble_bytes(&data).is_none());
}

#[test]
fn mouse_report_serialize_roundtrip() {
    let original = MouseReport {
        buttons: 0x05,
        x: -100,
        y: 50,
        wheel: -2,
    };

    let mut buf = [0u8; 4];
    let written = original.serialize(&mut buf);

    assert_eq!(written, 4);
    assert_eq!(buf[0], 0x05);
    assert_eq!(buf[1] as i8, -100);
    assert_eq!(buf[2] as i8, 50);
    assert_eq!(buf[3] as i8, -2);
}

#[test]
fn mouse_report_is_not_idle_when_moving() {
    let report = MouseReport {
        buttons: 0,
        x: 1,
        y: 0,
        wheel: 0,
    };
    assert!(!report.is_idle());
}

// ═══════════════════════════════════════════════════════════════════════════
// Report Classification Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn classify_report_by_id_keyboard() {
    let data = [0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00];
    let report = classify_report(1, &data);
    assert!(matches!(report, Some(HidReport::Keyboard(_))));
}

#[test]
fn classify_report_by_id_mouse() {
    let data = [0x01, 0x10, 0x20, 0x00];
    let report = classify_report(2, &data);
    assert!(matches!(report, Some(HidReport::Mouse(_))));
}

#[test]
fn classify_report_by_length_keyboard() {
    // Unknown report ID (0), 8 bytes → should infer keyboard
    let data = [0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00];
    let report = classify_report(0, &data);
    assert!(matches!(report, Some(HidReport::Keyboard(_))));
}

#[test]
fn classify_report_by_length_mouse() {
    // Unknown report ID (0), 4 bytes → should infer mouse
    let data = [0x01, 0x10, 0x20, 0x05];
    let report = classify_report(0, &data);
    assert!(matches!(report, Some(HidReport::Mouse(_))));
}

#[test]
fn classify_report_unknown_length() {
    // 5 bytes - doesn't match keyboard (8) or mouse (3-4)
    let data = [0x01, 0x02, 0x03, 0x04, 0x05];
    let report = classify_report(0, &data);
    assert!(report.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// HidReport Enum Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn hid_report_serialize_keyboard() {
    let kb = KeyboardReport {
        modifier: 0x01,
        reserved: 0x00,
        keycodes: [0x04, 0x00, 0x00, 0x00, 0x00, 0x00],
    };
    let report = HidReport::Keyboard(kb);

    let mut buf = [0u8; 8];
    let size = report.serialize(&mut buf);

    assert_eq!(size, 8);
    assert_eq!(buf[0], 0x01);
    assert_eq!(buf[2], 0x04);
}

#[test]
fn hid_report_serialize_mouse() {
    let mouse = MouseReport {
        buttons: 0x02,
        x: 10,
        y: -10,
        wheel: 0,
    };
    let report = HidReport::Mouse(mouse);

    let mut buf = [0u8; 8];
    let size = report.serialize(&mut buf);

    assert_eq!(size, 4);
    assert_eq!(buf[0], 0x02);
    assert_eq!(buf[1], 10);
    assert_eq!(buf[2] as i8, -10);
}
