//! Integration tests for bt2usb host-testable logic.

use bt2usb::hid::{classify_notification, HidReport};

#[test]
fn keyboard_notification_roundtrip() {
    // Report ID 1 + 8-byte keyboard payload.
    let notif = [1, 0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00];
    let report = classify_notification(&notif).expect("expected keyboard report");

    let mut out = [0u8; 8];
    let written = report.serialize(&mut out);
    assert_eq!(written, 8);
    assert_eq!(out, [0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn mouse_notification_roundtrip() {
    // Report ID 2 + 4-byte mouse payload.
    let notif = [2, 0x01, 0x05, 0xFB, 0x01];
    let report = classify_notification(&notif).expect("expected mouse report");

    match report {
        HidReport::Mouse(m) => {
            assert_eq!(m.buttons, 0x01);
            assert_eq!(m.x, 5);
            assert_eq!(m.y, -5);
            assert_eq!(m.wheel, 1);
        }
        _ => panic!("expected mouse report variant"),
    }
}

#[test]
fn consumer_notification_roundtrip() {
    // Report ID 3 + 2-byte consumer usage (Volume Increment = 0x00E9).
    let notif = [3, 0xE9, 0x00];
    let report = classify_notification(&notif).expect("expected consumer report");

    let mut out = [0u8; 2];
    let written = report.serialize(&mut out);
    assert_eq!(written, 2);
    assert_eq!(out, [0xE9, 0x00]);
}
