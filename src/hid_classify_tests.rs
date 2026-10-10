//! Host tests: report classification without a Report Map, by conventional
//! report ID (1, 2, 3) or by payload length (`classify_report`), and with a
//! report-ID prefix on the GATT value (`classify_notification`).

use super::hid::*;

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
    let data = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07]; // 7 bytes - no known report
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
