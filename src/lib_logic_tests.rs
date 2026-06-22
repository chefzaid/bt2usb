//! Host tests: HidReport enum + advertisement/UI/power pure logic.

use super::hid::consumer::{ConsumerReport, ConsumerUsage};
use super::hid::keyboard::KeyboardReport;
use super::hid::mouse::MouseReport;
use super::hid::HidReport;

#[test]
fn hid_report_serialize_keyboard() {
    let report = HidReport::Keyboard(KeyboardReport {
        modifier: 0x02,
        reserved: 0,
        keycodes: [0x04, 0, 0, 0, 0, 0],
    });
    let mut buf = [0u8; 8];
    let len = report.serialize(&mut buf);
    assert_eq!(len, 8);
    assert_eq!(buf[0], 0x02);
    assert_eq!(buf[2], 0x04);
}

#[test]
fn hid_report_serialize_mouse() {
    let report = HidReport::Mouse(MouseReport {
        buttons: 0x01,
        x: 10,
        y: -20,
        wheel: 0,
    });
    let mut buf = [0u8; 4];
    let len = report.serialize(&mut buf);
    assert_eq!(len, 4);
    assert_eq!(buf[0], 0x01);
}

#[test]
fn hid_report_serialize_consumer() {
    let report = HidReport::Consumer(ConsumerReport::new(ConsumerUsage::Mute));
    let mut buf = [0u8; 2];
    let len = report.serialize(&mut buf);
    assert_eq!(len, 2);
}

#[test]
fn hid_report_type_checks() {
    let kb = HidReport::Keyboard(KeyboardReport::empty());
    assert!(kb.is_keyboard());
    assert!(!kb.is_mouse());
    assert!(!kb.is_consumer());

    let mouse = HidReport::Mouse(MouseReport::empty());
    assert!(!mouse.is_keyboard());
    assert!(mouse.is_mouse());
    assert!(!mouse.is_consumer());

    let consumer = HidReport::Consumer(ConsumerReport::empty());
    assert!(!consumer.is_keyboard());
    assert!(!consumer.is_mouse());
    assert!(consumer.is_consumer());
}

#[test]
fn hid_report_equality() {
    let a = HidReport::Keyboard(KeyboardReport::empty());
    let b = HidReport::Keyboard(KeyboardReport::empty());
    assert_eq!(a, b);

    let c = HidReport::Mouse(MouseReport::empty());
    assert_ne!(a, c);
}

#[test]
fn ble_adv_parser_detects_hid_uuid() {
    let ad_data = [0x03, 0x03, 0x12, 0x18];
    assert!(crate::ble::adv_parser::contains_hid_service_uuid(&ad_data));
}

#[test]
fn ble_adv_parser_extracts_name_or_unknown() {
    let named = [0x05, 0x09, b'M', b'o', b'u', b's'];
    assert_eq!(
        crate::ble::adv_parser::extract_device_name(&named).as_str(),
        "Mous"
    );

    let unnamed = [0x02, 0x01, 0x06];
    assert_eq!(
        crate::ble::adv_parser::extract_device_name(&unnamed).as_str(),
        "Unknown"
    );
}

#[test]
fn ui_input_logic_scan_dots_cycle() {
    use crate::ui::input_logic::next_scan_dots;
    assert_eq!(next_scan_dots(0), 1);
    assert_eq!(next_scan_dots(1), 2);
    assert_eq!(next_scan_dots(2), 3);
    assert_eq!(next_scan_dots(3), 0); // wraps
}

#[test]
fn ble_adv_parser_rejects_non_hid_uuid() {
    let ad_data = [0x03, 0x03, 0x0F, 0x18];
    assert!(!crate::ble::adv_parser::contains_hid_service_uuid(&ad_data));
}

#[test]
fn ble_adv_parser_handles_malformed_lengths() {
    let ad_zero_len = [0x00];
    assert!(!crate::ble::adv_parser::contains_hid_service_uuid(
        &ad_zero_len
    ));

    let ad_too_short = [0x05, 0x03, 0x12];
    assert!(!crate::ble::adv_parser::contains_hid_service_uuid(
        &ad_too_short
    ));
}

#[test]
fn ble_adv_parser_name_is_truncated_to_capacity() {
    let mut ad_data = [0u8; 40];
    ad_data[0] = 35;
    ad_data[1] = 0x09;
    for i in 2..37 {
        ad_data[i] = b'X';
    }
    let name = crate::ble::adv_parser::extract_device_name(&ad_data);
    assert_eq!(name.len(), 32);
}

#[test]
fn screen_power_policy_auto_off_enabled_after_timeout() {
    assert!(crate::power_logic::screen_should_be_on(
        true, true, 119, 120
    ));
    assert!(!crate::power_logic::screen_should_be_on(
        true, true, 120, 120
    ));
    assert!(!crate::power_logic::screen_should_be_on(
        true, true, 240, 120
    ));
}

#[test]
fn screen_power_policy_auto_off_disabled_stays_on() {
    assert!(crate::power_logic::screen_should_be_on(
        true, false, 120, 120
    ));
    assert!(crate::power_logic::screen_should_be_on(
        true, false, 3600, 120
    ));
}

#[test]
fn screen_power_policy_respects_base_display_state() {
    assert!(!crate::power_logic::screen_should_be_on(
        false, true, 0, 120
    ));
    assert!(!crate::power_logic::screen_should_be_on(
        false, false, 999, 120
    ));
}
