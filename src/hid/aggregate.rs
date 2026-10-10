//! Merge the absolute held state of the two BLE slots before USB delivery.
//!
//! Keyboard usages/modifiers and mouse buttons form a union. Six-key rollover
//! uses the standard ErrorRollOver array. Consumer USB supports one usage, so
//! the lowest numbered active slot wins, with fallback on release/disconnect.
//! Relative motion belongs only to the report currently being processed.

use super::consumer::{ConsumerReport, MAX_CONSUMER_USAGE};
use super::delivery::HidEvent;
use super::keyboard::KeyboardReport;
use super::mouse::MouseReport;
use super::{wake, HidReport};
use heapless::Vec;

/// One source per BLE link ([`crate::config::BLE_MAX_CONNECTIONS`]).
pub const SOURCES: usize = crate::config::BLE_MAX_CONNECTIONS;

#[derive(Clone, Copy, Default)]
struct Source {
    keyboard: KeyboardReport,
    mouse_buttons: u8,
    consumer: ConsumerReport,
}

#[derive(Default)]
pub struct InputAggregator {
    sources: [Source; SOURCES],
}

pub struct InputUpdate {
    pub reports: Vec<HidReport, 3>,
    pub wake: bool,
}

impl InputAggregator {
    pub fn apply(&mut self, event: HidEvent) -> InputUpdate {
        let mut update = InputUpdate {
            reports: Vec::new(),
            wake: false,
        };
        match event {
            HidEvent::Report { source, report } => {
                let Some(slot) = self.sources.get_mut(source) else {
                    return update;
                };
                let merged = match report {
                    HidReport::Keyboard(mut next) => {
                        next.reserved = 0;
                        update.wake = wake::new_press(
                            &HidReport::Keyboard(slot.keyboard),
                            &HidReport::Keyboard(next),
                        );
                        slot.keyboard = next;
                        HidReport::Keyboard(self.keyboard())
                    }
                    HidReport::Mouse(mut next) => {
                        next.buttons &= 0x1f;
                        update.wake = wake::new_press(
                            &HidReport::Mouse(MouseReport {
                                buttons: slot.mouse_buttons,
                                ..MouseReport::default()
                            }),
                            &HidReport::Mouse(next),
                        );
                        slot.mouse_buttons = next.buttons;
                        next.buttons = self.mouse_buttons();
                        HidReport::Mouse(next)
                    }
                    HidReport::Consumer(next) => {
                        if next.usage > MAX_CONSUMER_USAGE {
                            return update;
                        }
                        update.wake = wake::new_press(
                            &HidReport::Consumer(slot.consumer),
                            &HidReport::Consumer(next),
                        );
                        slot.consumer = next;
                        HidReport::Consumer(self.consumer())
                    }
                };
                let _ = update.reports.push(merged);
            }
            HidEvent::Disconnected { source } => {
                let Some(slot) = self.sources.get_mut(source) else {
                    return update;
                };
                *slot = Source::default();
                // Always publish the new union, including idle endpoints. This
                // repairs any stale USB state without clearing another slot.
                let _ = update.reports.push(HidReport::Keyboard(self.keyboard()));
                let _ = update.reports.push(HidReport::Mouse(MouseReport {
                    buttons: self.mouse_buttons(),
                    ..MouseReport::default()
                }));
                let _ = update.reports.push(HidReport::Consumer(self.consumer()));
            }
        }
        update
    }

    fn keyboard(&self) -> KeyboardReport {
        let mut report = KeyboardReport::default();
        let mut keys = [false; 256];
        let mut overflow = false;
        for source in &self.sources {
            report.modifier |= source.keyboard.modifier;
            for key in source.keyboard.keycodes {
                if (1..=3).contains(&key) {
                    overflow = true;
                }
                if key > 3 {
                    keys[key as usize] = true;
                }
            }
        }
        let mut count = 0;
        for (key, pressed) in keys.iter().enumerate().skip(4) {
            if !pressed {
                continue;
            }
            if count == report.keycodes.len() {
                overflow = true;
                break;
            }
            report.keycodes[count] = key as u8;
            count += 1;
        }
        if overflow {
            report.keycodes = [1; 6];
        }
        report
    }

    fn mouse_buttons(&self) -> u8 {
        self.sources
            .iter()
            .fold(0, |held, source| held | source.mouse_buttons)
    }

    fn consumer(&self) -> ConsumerReport {
        self.sources
            .iter()
            .map(|source| source.consumer)
            .find(|report| report.usage != 0)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send(aggregate: &mut InputAggregator, source: usize, report: HidReport) -> InputUpdate {
        aggregate.apply(HidEvent::Report { source, report })
    }

    fn key(codes: [u8; 6], modifier: u8) -> HidReport {
        HidReport::Keyboard(KeyboardReport {
            keycodes: codes,
            modifier,
            reserved: 0,
        })
    }

    #[test]
    fn same_key_held_by_two_sources_survives_release_and_disconnect() {
        let mut aggregate = InputAggregator::default();
        send(&mut aggregate, 0, key([4, 5, 0, 0, 0, 0], 1));
        let update = send(&mut aggregate, 1, key([4, 6, 0, 0, 0, 0], 2));
        assert_eq!(update.reports[0], key([4, 5, 6, 0, 0, 0], 3));
        let update = send(&mut aggregate, 0, key([0; 6], 0));
        assert_eq!(update.reports[0], key([4, 6, 0, 0, 0, 0], 2));
        assert!(!update.wake);
        let update = aggregate.apply(HidEvent::Disconnected { source: 0 });
        assert_eq!(update.reports[0], key([4, 6, 0, 0, 0, 0], 2));
        assert!(!update.wake);
    }

    #[test]
    fn keyboard_rollover_recovers_when_one_source_disconnects() {
        let mut aggregate = InputAggregator::default();
        send(&mut aggregate, 0, key([4, 5, 6, 7, 8, 9], 0));
        let update = send(&mut aggregate, 1, key([10, 0, 0, 0, 0, 0], 0));
        assert_eq!(update.reports[0], key([1; 6], 0));
        assert_eq!(
            aggregate
                .apply(HidEvent::Disconnected { source: 1 })
                .reports[0],
            key([4, 5, 6, 7, 8, 9], 0)
        );
    }

    #[test]
    fn mouse_unions_buttons_without_replaying_other_sources_motion() {
        let mut aggregate = InputAggregator::default();
        send(
            &mut aggregate,
            0,
            HidReport::Mouse(MouseReport {
                buttons: 1,
                x: 10,
                ..MouseReport::default()
            }),
        );
        let update = send(
            &mut aggregate,
            1,
            HidReport::Mouse(MouseReport {
                buttons: 2,
                x: 3,
                ..MouseReport::default()
            }),
        );
        assert_eq!(
            update.reports[0],
            HidReport::Mouse(MouseReport {
                buttons: 3,
                x: 3,
                ..MouseReport::default()
            })
        );
        let update = aggregate.apply(HidEvent::Disconnected { source: 0 });
        assert_eq!(
            update.reports[1],
            HidReport::Mouse(MouseReport {
                buttons: 2,
                ..MouseReport::default()
            })
        );
        assert!(!update.wake);
    }

    #[test]
    fn consumer_priority_falls_back_and_shared_usage_survives_disconnect() {
        let mut aggregate = InputAggregator::default();
        let a = HidReport::Consumer(ConsumerReport { usage: 0xe9 });
        let b = HidReport::Consumer(ConsumerReport { usage: 0xea });
        send(&mut aggregate, 0, a.clone());
        assert_eq!(send(&mut aggregate, 1, b.clone()).reports[0], a);
        assert_eq!(
            aggregate
                .apply(HidEvent::Disconnected { source: 0 })
                .reports[2],
            b
        );
        send(&mut aggregate, 0, b.clone());
        assert_eq!(
            aggregate
                .apply(HidEvent::Disconnected { source: 1 })
                .reports[2],
            b
        );
    }

    #[test]
    fn invalid_sources_cannot_modify_state_or_wake() {
        let mut aggregate = InputAggregator::default();
        let update = send(&mut aggregate, SOURCES, key([4, 0, 0, 0, 0, 0], 0));
        assert!(update.reports.is_empty());
        assert!(!update.wake);
        assert!(aggregate
            .apply(HidEvent::Disconnected { source: usize::MAX })
            .reports
            .is_empty());
    }
}
