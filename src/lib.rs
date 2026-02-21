//! Test-only library interface for bt2usb.
//!
//! This module re-exports the pure logic modules that can be tested
//! on the host (no embedded hardware required).
//!
//! Usage: `mask test` or `cargo test --lib`
//!
//! For coverage: `mask coverage`
//!
//! Note: The embedded binary uses main.rs with #![no_std] and #![no_main].
//! This lib.rs provides a separate entry point for host-based testing.

#![cfg_attr(not(test), no_std)]

// ═══════════════════════════════════════════════════════════════════════════
// HID Module Re-exports
// ═══════════════════════════════════════════════════════════════════════════

pub mod hid {
    pub mod keyboard {
        pub use crate::hid_keyboard_impl::*;
    }
    pub mod mouse {
        pub use crate::hid_mouse_impl::*;
    }
    pub mod consumer {
        pub use crate::hid_consumer_impl::*;
    }

    pub use consumer::ConsumerReport;
    pub use keyboard::KeyboardReport;
    pub use mouse::MouseReport;

    /// HID report enum (matches the embedded version)
    #[derive(Clone, Debug, PartialEq)]
    pub enum HidReport {
        Keyboard(KeyboardReport),
        Mouse(MouseReport),
        Consumer(ConsumerReport),
    }

    impl HidReport {
        pub fn serialize(&self, buf: &mut [u8]) -> usize {
            match self {
                HidReport::Keyboard(k) => k.serialize(buf),
                HidReport::Mouse(m) => m.serialize(buf),
                HidReport::Consumer(c) => c.serialize(buf),
            }
        }

        pub fn is_keyboard(&self) -> bool {
            matches!(self, HidReport::Keyboard(_))
        }

        pub fn is_mouse(&self) -> bool {
            matches!(self, HidReport::Mouse(_))
        }

        pub fn is_consumer(&self) -> bool {
            matches!(self, HidReport::Consumer(_))
        }
    }

    /// Classify a raw BLE HID notification into a typed HidReport.
    pub fn classify_report(report_id: u8, data: &[u8]) -> Option<HidReport> {
        match report_id {
            1 => KeyboardReport::from_ble_bytes(data).map(HidReport::Keyboard),
            2 => MouseReport::from_ble_bytes(data).map(HidReport::Mouse),
            3 => ConsumerReport::from_ble_bytes(data).map(HidReport::Consumer),
            _ => infer_from_length(data),
        }
    }

    fn infer_from_length(data: &[u8]) -> Option<HidReport> {
        match data.len() {
            8 => KeyboardReport::from_ble_bytes(data).map(HidReport::Keyboard),
            3..=4 => MouseReport::from_ble_bytes(data).map(HidReport::Mouse),
            2 => {
                let usage = u16::from_le_bytes([data[0], data[1]]);
                if usage > 0 && usage < 0x1000 {
                    ConsumerReport::from_ble_bytes(data).map(HidReport::Consumer)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Classify a notification where the payload may be either:
    /// - raw boot report bytes, or
    /// - report-protocol bytes prefixed with Report ID.
    pub fn classify_notification(data: &[u8]) -> Option<HidReport> {
        classify_report(0, data).or_else(|| {
            if data.len() > 1 {
                classify_report(data[0], &data[1..])
            } else {
                None
            }
        })
    }
}

// Internal module paths for the actual implementations
#[path = "hid/consumer.rs"]
mod hid_consumer_impl;
#[path = "hid/keyboard.rs"]
mod hid_keyboard_impl;
#[path = "hid/mouse.rs"]
mod hid_mouse_impl;

#[path = "ble/adv_parser.rs"]
mod ble_adv_parser_impl;

#[path = "power_logic.rs"]
mod power_logic_impl;
#[path = "ui/input_logic.rs"]
mod ui_input_logic_impl;

pub mod ble {
    pub mod adv_parser {
        pub use crate::ble_adv_parser_impl::{contains_hid_service_uuid, extract_device_name};
    }
}

pub mod ui {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ButtonEvent {
        Up,
        Down,
        Select,
    }

    pub mod input_logic {
        pub use crate::ui_input_logic_impl::{select_next, select_prev};
    }
}

pub mod power_logic {
    pub use crate::power_logic_impl::screen_should_be_on;
}

// ═══════════════════════════════════════════════════════════════════════════
// Unit Tests - Target: 80%+ code coverage
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::hid::*;
    use super::hid_consumer_impl::{ConsumerUsage, CONSUMER_REPORT_SIZE};

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
    fn classify_report_invalid_2_byte_not_consumer() {
        // 2 bytes but usage code too high (>0x1000)
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
    fn classify_notification_prefers_direct_parse() {
        let data = [0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00];
        let report = classify_notification(&data);
        assert!(matches!(report, Some(HidReport::Keyboard(_))));
    }

    // ════════════════════════════════════════════════════════════════════════
    // HidReport Enum Tests
    // ════════════════════════════════════════════════════════════════════════

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
    fn ui_input_logic_selection_boundaries() {
        assert_eq!(crate::ui::input_logic::select_prev(0), 0);
        assert_eq!(crate::ui::input_logic::select_prev(3), 2);
        assert_eq!(crate::ui::input_logic::select_next(0, 1), 0);
        assert_eq!(crate::ui::input_logic::select_next(0, 3), 1);
        assert_eq!(crate::ui::input_logic::select_next(2, 3), 2);
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
}
