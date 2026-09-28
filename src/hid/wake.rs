//! Remote wakeup requires a newly pressed input. Motion, releases, repeats,
//! disconnect cleanup, and USB state recovery are deliberately ineligible.

use super::HidReport;

pub fn new_press(previous: &HidReport, next: &HidReport) -> bool {
    match (previous, next) {
        (HidReport::Keyboard(old), HidReport::Keyboard(new)) => {
            new.modifier & !old.modifier != 0
                || new.keycodes.iter().any(|key| {
                    // 0 is no key; 1..=3 are keyboard error indications.
                    *key > 3 && !old.keycodes.contains(key)
                })
        }
        (HidReport::Mouse(old), HidReport::Mouse(new)) => (new.buttons & !old.buttons) & 0x1f != 0,
        (HidReport::Consumer(old), HidReport::Consumer(new)) => {
            new.usage != 0 && new.usage != old.usage
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hid::{consumer::ConsumerReport, keyboard::KeyboardReport, mouse::MouseReport};

    #[test]
    fn keyboard_wakes_on_new_key_or_modifier_only() {
        let idle = HidReport::Keyboard(KeyboardReport::default());
        let key = HidReport::Keyboard(KeyboardReport {
            keycodes: [4, 0, 0, 0, 0, 0],
            ..KeyboardReport::default()
        });
        assert!(new_press(&idle, &key));
        assert!(!new_press(&key, &key));
        assert!(!new_press(&key, &idle));
        assert!(new_press(
            &idle,
            &HidReport::Keyboard(KeyboardReport {
                modifier: 2,
                ..KeyboardReport::default()
            })
        ));
        assert!(!new_press(
            &idle,
            &HidReport::Keyboard(KeyboardReport {
                keycodes: [1; 6],
                ..KeyboardReport::default()
            })
        ));
    }

    #[test]
    fn mouse_motion_wheel_and_release_do_not_wake() {
        let idle = HidReport::Mouse(MouseReport::default());
        let motion = HidReport::Mouse(MouseReport {
            x: 7,
            y: -4,
            wheel: 1,
            pan: 2,
            ..MouseReport::default()
        });
        let pressed = HidReport::Mouse(MouseReport {
            buttons: 1,
            ..MouseReport::default()
        });
        assert!(!new_press(&idle, &motion));
        assert!(new_press(&motion, &pressed));
        assert!(!new_press(&pressed, &pressed));
        assert!(!new_press(&pressed, &idle));
    }

    #[test]
    fn consumer_requires_a_new_nonzero_usage() {
        let idle = HidReport::Consumer(ConsumerReport::default());
        let pressed = HidReport::Consumer(ConsumerReport { usage: 0xe9 });
        assert!(new_press(&idle, &pressed));
        assert!(!new_press(&pressed, &pressed));
        assert!(!new_press(&pressed, &idle));
    }
}
