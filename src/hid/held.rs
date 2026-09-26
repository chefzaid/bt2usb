//! Tracks which inputs a BLE link currently holds down on the USB side, so they
//! can be released when that link ends.
//!
//! USB HID keyboard and mouse-button reports are *state*: the host keeps a key
//! or button pressed until a report says otherwise. If a BLE link drops while a
//! key is held (keyboard asleep, out of range, battery pulled), the release
//! never arrives and the host auto-repeats that key indefinitely. The slot
//! feeds every report it forwards through [`HeldInputs::note`] and, when the
//! link ends, sends [`HeldInputs::releases`].

use super::consumer::ConsumerReport;
use super::keyboard::KeyboardReport;
use super::mouse::MouseReport;
use super::HidReport;
use heapless::Vec;

/// Which report kinds were last forwarded in a non-released state.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct HeldInputs {
    keyboard: bool,
    mouse_buttons: bool,
    consumer: bool,
}

impl HeldInputs {
    pub const fn new() -> Self {
        Self {
            keyboard: false,
            mouse_buttons: false,
            consumer: false,
        }
    }

    /// Record a report that was forwarded to USB.
    pub fn note(&mut self, report: &HidReport) {
        match report {
            HidReport::Keyboard(k) => self.keyboard = *k != KeyboardReport::default(),
            // Motion/wheel are relative one-shots; only buttons are held state.
            HidReport::Mouse(m) => self.mouse_buttons = m.buttons != 0,
            HidReport::Consumer(c) => self.consumer = *c != ConsumerReport::default(),
        }
    }

    /// All-released reports for every kind still held, then forget them.
    ///
    /// Only kinds this link holds are released, so a link ending doesn't
    /// clear keys held through the *other* connection slot.
    pub fn releases(&mut self) -> Vec<HidReport, 3> {
        let mut out = Vec::new();
        if self.keyboard {
            let _ = out.push(HidReport::Keyboard(KeyboardReport::default()));
        }
        if self.mouse_buttons {
            let _ = out.push(HidReport::Mouse(MouseReport::default()));
        }
        if self.consumer {
            let _ = out.push(HidReport::Consumer(ConsumerReport::default()));
        }
        *self = Self::new();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: u8) -> HidReport {
        HidReport::Keyboard(KeyboardReport {
            modifier: 0,
            reserved: 0,
            keycodes: [code, 0, 0, 0, 0, 0],
        })
    }

    fn mouse(buttons: u8, x: i8) -> HidReport {
        HidReport::Mouse(MouseReport {
            buttons,
            x,
            ..MouseReport::default()
        })
    }

    #[test]
    fn nothing_forwarded_needs_no_release() {
        assert!(HeldInputs::new().releases().is_empty());
    }

    #[test]
    fn held_key_is_released() {
        let mut h = HeldInputs::new();
        h.note(&key(0x04));
        let r = h.releases();
        assert_eq!(
            r.as_slice(),
            &[HidReport::Keyboard(KeyboardReport::default())]
        );
    }

    #[test]
    fn held_modifier_alone_is_released() {
        let mut h = HeldInputs::new();
        h.note(&HidReport::Keyboard(KeyboardReport {
            modifier: 0x02, // Left Shift
            ..KeyboardReport::default()
        }));
        assert_eq!(h.releases().len(), 1);
    }

    #[test]
    fn key_already_released_needs_nothing() {
        let mut h = HeldInputs::new();
        h.note(&key(0x04));
        h.note(&HidReport::Keyboard(KeyboardReport::default()));
        assert!(h.releases().is_empty());
    }

    #[test]
    fn mouse_motion_without_buttons_needs_nothing() {
        let mut h = HeldInputs::new();
        h.note(&mouse(0, 10));
        assert!(h.releases().is_empty());
    }

    #[test]
    fn held_mouse_button_is_released() {
        let mut h = HeldInputs::new();
        h.note(&mouse(0b1, 5));
        assert_eq!(
            h.releases().as_slice(),
            &[HidReport::Mouse(MouseReport::default())]
        );
    }

    #[test]
    fn held_consumer_key_is_released() {
        let mut h = HeldInputs::new();
        h.note(&HidReport::Consumer(ConsumerReport { usage: 0x00E9 }));
        assert_eq!(
            h.releases().as_slice(),
            &[HidReport::Consumer(ConsumerReport::default())]
        );
    }

    #[test]
    fn releases_every_held_kind_once() {
        let mut h = HeldInputs::new();
        h.note(&key(0x04));
        h.note(&mouse(0b10, 0));
        h.note(&HidReport::Consumer(ConsumerReport { usage: 0x00CD }));
        assert_eq!(h.releases().len(), 3);
        // Forgotten after releasing.
        assert!(h.releases().is_empty());
    }

    #[test]
    fn released_reports_serialize_to_all_zero() {
        let mut h = HeldInputs::new();
        h.note(&key(0x04));
        h.note(&mouse(1, 0));
        h.note(&HidReport::Consumer(ConsumerReport { usage: 0x00E9 }));
        for r in h.releases() {
            let mut buf = [0xFFu8; 8];
            let n = r.serialize(&mut buf);
            assert!(
                buf[..n].iter().all(|&b| b == 0),
                "{:?} -> {:?}",
                r,
                &buf[..n]
            );
        }
    }
}
