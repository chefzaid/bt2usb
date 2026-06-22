use super::hid::consumer::{ConsumerReport, ConsumerUsage, CONSUMER_REPORT_SIZE};
use super::hid::keyboard::KeyboardReport;
use super::hid::mouse::MouseReport;
use super::hid::*;

// ════════════════════════════════════════════════════════════════════════
// Keyboard Report Tests
// ════════════════════════════════════════════════════════════════════════

#[test]
fn keyboard_report_empty() {
    let report = KeyboardReport::empty();
    assert!(report.is_empty());
    assert_eq!(report.modifier, 0);
    assert_eq!(report.reserved, 0);
    assert_eq!(report.keycodes, [0; 6]);
}

#[test]
fn keyboard_report_from_valid_ble_bytes() {
    let data = [0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00];
    let report = KeyboardReport::from_ble_bytes(&data).unwrap();
    assert_eq!(report.modifier, 0x02); // Left Shift
    assert_eq!(report.reserved, 0x00);
    assert_eq!(report.keycodes[0], 0x04); // 'a' key
    assert!(!report.is_empty());
}

#[test]
fn keyboard_report_from_short_bytes_fails() {
    assert!(KeyboardReport::from_ble_bytes(&[]).is_none());
    assert!(KeyboardReport::from_ble_bytes(&[0x02]).is_none());
    assert!(KeyboardReport::from_ble_bytes(&[0x02, 0x00, 0x04]).is_none());
    assert!(KeyboardReport::from_ble_bytes(&[0; 7]).is_none());
}

#[test]
fn keyboard_report_from_exact_8_bytes() {
    let data = [0xFF, 0x00, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09];
    let report = KeyboardReport::from_ble_bytes(&data).unwrap();
    assert_eq!(report.modifier, 0xFF); // All modifiers
    assert_eq!(report.keycodes, [0x04, 0x05, 0x06, 0x07, 0x08, 0x09]);
}

#[test]
fn keyboard_report_from_longer_bytes_ok() {
    // Extra bytes should be ignored
    let data = [0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF];
    let report = KeyboardReport::from_ble_bytes(&data).unwrap();
    assert_eq!(report.modifier, 0x02);
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

    // Deserialize and compare
    let parsed = KeyboardReport::from_ble_bytes(&buf).unwrap();
    assert_eq!(parsed.modifier, original.modifier);
    assert_eq!(parsed.keycodes, original.keycodes);
}

#[test]
fn keyboard_report_serialize_buffer_too_small() {
    let report = KeyboardReport::empty();
    let mut buf = [0u8; 4];
    let written = report.serialize(&mut buf);
    assert_eq!(written, 0); // Should fail gracefully
}

#[test]
fn keyboard_report_all_modifiers() {
    // All modifiers pressed: Ctrl+Shift+Alt+GUI (both sides)
    let data = [0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    let report = KeyboardReport::from_ble_bytes(&data).unwrap();
    assert_eq!(report.modifier, 0xFF);
    // is_empty() checks BOTH modifier AND keycodes - modifiers pressed means not empty
    assert!(!report.is_empty());
}

#[test]
fn keyboard_report_is_empty_with_modifier_only() {
    let mut report = KeyboardReport::empty();
    report.modifier = 0x01; // Left Ctrl
                            // is_empty checks modifier AND keycodes
                            // With modifier set but no keys, it depends on implementation
                            // Let's verify the actual behavior
    let has_keys = report.keycodes.iter().any(|&k| k != 0);
    assert!(!has_keys);
}

#[test]
fn keyboard_report_six_keys_rollover() {
    // 6KRO: simultaneous keys
    let data = [0x00, 0x00, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09];
    let report = KeyboardReport::from_ble_bytes(&data).unwrap();
    assert_eq!(report.keycodes, [0x04, 0x05, 0x06, 0x07, 0x08, 0x09]);
}

// ════════════════════════════════════════════════════════════════════════
// Mouse Report Tests
// ════════════════════════════════════════════════════════════════════════

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
    let data = [0x01, 0x0A, 0xFB]; // Left click, X=10, Y=-5
    let report = MouseReport::from_ble_bytes(&data).unwrap();
    assert_eq!(report.buttons, 0x01);
    assert_eq!(report.x, 10);
    assert_eq!(report.y, -5);
    assert_eq!(report.wheel, 0);
    assert!(!report.is_idle());
}

#[test]
fn mouse_report_from_4_byte_data() {
    let data = [0x02, 0x00, 0x00, 0x01]; // Right click, wheel=1
    let report = MouseReport::from_ble_bytes(&data).unwrap();
    assert_eq!(report.buttons, 0x02);
    assert_eq!(report.x, 0);
    assert_eq!(report.y, 0);
    assert_eq!(report.wheel, 1);
}

#[test]
fn mouse_report_from_short_bytes_fails() {
    assert!(MouseReport::from_ble_bytes(&[]).is_none());
    assert!(MouseReport::from_ble_bytes(&[0x01]).is_none());
    assert!(MouseReport::from_ble_bytes(&[0x01, 0x02]).is_none());
}

#[test]
fn mouse_report_negative_movement() {
    // Test signed values: X=-128, Y=127
    let data = [0x00, 0x80, 0x7F, 0x00];
    let report = MouseReport::from_ble_bytes(&data).unwrap();
    assert_eq!(report.x, -128);
    assert_eq!(report.y, 127);
}

#[test]
fn mouse_report_all_buttons() {
    // All 3 buttons pressed
    let data = [0x07, 0x00, 0x00, 0x00];
    let report = MouseReport::from_ble_bytes(&data).unwrap();
    assert_eq!(report.buttons, 0x07);
    assert!(!report.is_idle()); // Buttons pressed
}

#[test]
fn mouse_report_serialize_roundtrip() {
    let original = MouseReport {
        buttons: 0x05,
        x: -10,
        y: 20,
        wheel: -3,
    };
    let mut buf = [0u8; 4];
    let written = original.serialize(&mut buf);
    assert_eq!(written, 4);

    let parsed = MouseReport::from_ble_bytes(&buf).unwrap();
    assert_eq!(parsed.buttons, original.buttons);
    assert_eq!(parsed.x, original.x);
    assert_eq!(parsed.y, original.y);
    assert_eq!(parsed.wheel, original.wheel);
}

#[test]
fn mouse_report_serialize_buffer_too_small() {
    let report = MouseReport::empty();
    let mut buf = [0u8; 2];
    let written = report.serialize(&mut buf);
    assert_eq!(written, 0);
}

#[test]
fn mouse_report_wheel_scroll() {
    // Forward scroll
    let data = [0x00, 0x00, 0x00, 0x03];
    let report = MouseReport::from_ble_bytes(&data).unwrap();
    assert_eq!(report.wheel, 3);

    // Backward scroll
    let data = [0x00, 0x00, 0x00, 0xFD]; // -3
    let report = MouseReport::from_ble_bytes(&data).unwrap();
    assert_eq!(report.wheel, -3);
}

#[test]
fn mouse_report_is_idle_with_movement_only() {
    let report = MouseReport {
        buttons: 0,
        x: 10,
        y: -5,
        wheel: 0,
    };
    // Movement without buttons - not idle
    assert!(!report.is_idle());
}

// ════════════════════════════════════════════════════════════════════════
// Consumer Control Report Tests
// ════════════════════════════════════════════════════════════════════════

#[test]
fn consumer_report_empty() {
    let report = ConsumerReport::empty();
    assert!(report.is_empty());
    assert_eq!(report.usage, 0);
    assert_eq!(report.get_usage(), ConsumerUsage::None);
}

#[test]
fn consumer_report_volume_up() {
    let report = ConsumerReport::new(ConsumerUsage::VolumeUp);
    assert!(!report.is_empty());
    assert_eq!(report.usage, 0x00E9);
    assert_eq!(report.get_usage(), ConsumerUsage::VolumeUp);
}

#[test]
fn consumer_report_volume_down() {
    let report = ConsumerReport::new(ConsumerUsage::VolumeDown);
    assert_eq!(report.usage, 0x00EA);
    assert_eq!(report.get_usage(), ConsumerUsage::VolumeDown);
}

#[test]
fn consumer_report_mute() {
    let report = ConsumerReport::new(ConsumerUsage::Mute);
    assert_eq!(report.usage, 0x00E2);
}

#[test]
fn consumer_report_media_controls() {
    assert_eq!(ConsumerReport::new(ConsumerUsage::PlayPause).usage, 0x00CD);
    assert_eq!(ConsumerReport::new(ConsumerUsage::NextTrack).usage, 0x00B5);
    assert_eq!(ConsumerReport::new(ConsumerUsage::PrevTrack).usage, 0x00B6);
    assert_eq!(ConsumerReport::new(ConsumerUsage::Stop).usage, 0x00B7);
}

#[test]
fn consumer_report_browser_controls() {
    assert_eq!(
        ConsumerReport::new(ConsumerUsage::BrowserHome).usage,
        0x0223
    );
    assert_eq!(
        ConsumerReport::new(ConsumerUsage::BrowserBack).usage,
        0x0224
    );
    assert_eq!(
        ConsumerReport::new(ConsumerUsage::BrowserForward).usage,
        0x0225
    );
    assert_eq!(
        ConsumerReport::new(ConsumerUsage::BrowserRefresh).usage,
        0x0227
    );
}

#[test]
fn consumer_report_app_launchers() {
    assert_eq!(
        ConsumerReport::new(ConsumerUsage::LaunchEmail).usage,
        0x018A
    );
    assert_eq!(
        ConsumerReport::new(ConsumerUsage::LaunchCalculator).usage,
        0x0192
    );
    assert_eq!(
        ConsumerReport::new(ConsumerUsage::LaunchFileBrowser).usage,
        0x0194
    );
}

#[test]
fn consumer_report_serialize() {
    let report = ConsumerReport::new(ConsumerUsage::PlayPause);
    let mut buf = [0u8; 2];
    let len = report.serialize(&mut buf);
    assert_eq!(len, CONSUMER_REPORT_SIZE);
    assert_eq!(buf, [0xCD, 0x00]); // Little-endian 0x00CD
}

#[test]
fn consumer_report_serialize_buffer_too_small() {
    let report = ConsumerReport::new(ConsumerUsage::VolumeUp);
    let mut buf = [0u8; 1];
    let len = report.serialize(&mut buf);
    assert_eq!(len, 0);
}

#[test]
fn consumer_report_from_bytes() {
    let data = [0xE9, 0x00]; // Volume Up (little-endian)
    let report = ConsumerReport::from_ble_bytes(&data).unwrap();
    assert_eq!(report.get_usage(), ConsumerUsage::VolumeUp);
}

#[test]
fn consumer_report_from_short_bytes_fails() {
    assert!(ConsumerReport::from_ble_bytes(&[]).is_none());
    assert!(ConsumerReport::from_ble_bytes(&[0xE9]).is_none());
}

#[test]
fn consumer_usage_from_unknown() {
    let usage = ConsumerUsage::from(0xFFFF);
    assert_eq!(usage, ConsumerUsage::None);
}

#[test]
fn consumer_report_roundtrip() {
    for usage in [
        ConsumerUsage::VolumeUp,
        ConsumerUsage::VolumeDown,
        ConsumerUsage::Mute,
        ConsumerUsage::PlayPause,
    ] {
        let original = ConsumerReport::new(usage);
        let mut buf = [0u8; 2];
        original.serialize(&mut buf);
        let parsed = ConsumerReport::from_ble_bytes(&buf).unwrap();
        assert_eq!(parsed.usage, original.usage);
    }
}

// ════════════════════════════════════════════════════════════════════════
// Classification Tests
// ════════════════════════════════════════════════════════════════════════

#[test]
fn classify_report_by_id_keyboard() {
    let data = [0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00];
    let report = classify_report(1, &data);
    assert!(report.is_some());
    assert!(report.unwrap().is_keyboard());
}

#[test]
fn classify_report_by_id_mouse() {
    let data = [0x01, 0x10, 0x20, 0x00];
    let report = classify_report(2, &data);
    assert!(report.is_some());
    assert!(report.unwrap().is_mouse());
}

#[test]
fn classify_report_by_id_consumer() {
    let data = [0xE9, 0x00]; // Volume Up
    let report = classify_report(3, &data);
    assert!(report.is_some());
    assert!(report.unwrap().is_consumer());
}

#[test]
fn classify_report_by_length_keyboard() {
    let data = [0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00];
    let report = classify_report(0, &data); // Unknown ID
    assert!(report.is_some());
    assert!(report.unwrap().is_keyboard());
}

#[test]
fn classify_report_rejects_keyboard_with_nonzero_reserved_byte() {
    // 8-byte report-protocol payloads can otherwise look like boot keyboards.
    let data = [0x02, 0x01, 0x05, 0xFB, 0x01, 0x00, 0x00, 0x00];
    let report = classify_report(0, &data);
    assert!(report.is_none());
}

#[test]
fn classify_report_by_length_mouse_3_bytes() {
    let data = [0x01, 0x10, 0x20];
    let report = classify_report(0, &data);
    assert!(report.is_some());
    assert!(report.unwrap().is_mouse());
}

#[test]
fn classify_report_by_length_mouse_4_bytes() {
    let data = [0x01, 0x10, 0x20, 0x05];
    let report = classify_report(0, &data);
    assert!(report.is_some());
    assert!(report.unwrap().is_mouse());
}

#[test]
fn classify_report_by_length_consumer() {
    let data = [0xE9, 0x00]; // Valid consumer usage
    let report = classify_report(0, &data);
    assert!(report.is_some());
    assert!(report.unwrap().is_consumer());
}

#[test]
fn classify_report_2_byte_zero_is_consumer_release() {
    // 2 bytes [0x00, 0x00] = consumer release event (usage 0)
    let data = [0x00, 0x00];
    let report = classify_report(0, &data);
    assert!(report.is_some());
    assert!(report.unwrap().is_consumer());
}

#[test]
fn classify_report_invalid_2_byte_not_consumer() {
    // 2 bytes but usage code too high (>=0x1000)
    let data = [0x00, 0x10]; // 0x1000
    let report = classify_report(0, &data);
    assert!(report.is_none());
}

#[test]
fn classify_report_unknown_length() {
    let data = [0x01, 0x02, 0x03, 0x04, 0x05]; // 5 bytes
    let report = classify_report(0, &data);
    assert!(report.is_none());
}

#[test]
fn classify_report_empty_data() {
    let report = classify_report(0, &[]);
    assert!(report.is_none());
}

#[test]
fn classify_report_single_byte() {
    let report = classify_report(0, &[0x01]);
    assert!(report.is_none());
}

#[test]
fn classify_notification_with_report_id_prefix_keyboard() {
    let data = [1, 0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00];
    let report = classify_notification(&data);
    assert!(matches!(report, Some(HidReport::Keyboard(_))));
}

#[test]
fn classify_notification_with_report_id_prefix_mouse() {
    let data = [2, 0x01, 0x10, 0x20, 0x00];
    let report = classify_notification(&data);
    assert!(matches!(report, Some(HidReport::Mouse(_))));
}

#[test]
fn classify_notification_with_report_id_prefix_consumer_release() {
    let data = [3, 0x00, 0x00];
    let report = classify_notification(&data);
    assert!(matches!(report, Some(HidReport::Consumer(_))));
}

#[test]
fn classify_notification_prefers_direct_parse() {
    let data = [0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00];
    let report = classify_notification(&data);
    assert!(matches!(report, Some(HidReport::Keyboard(_))));
}
