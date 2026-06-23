//! Backpressure-safe coalescing of HID reports.
//!
//! The BLE→USB path forwards reports through a bounded channel. When the USB
//! sink falls behind (host slow to poll, bus busy) that channel fills up. The
//! old behaviour was to `try_send` and silently drop on a full channel — which
//! can drop a key-*up* (release) report and leave a key stuck on the host.
//!
//! [`ReportCoalescer`] decouples the *synchronous* GATT notification callback
//! from the *async* channel writer. The callback `push`es every report; the
//! writer `pop`s them and `send().await`s with backpressure, so nothing is ever
//! dropped on the channel. To keep memory O(1) under a synchronous producer
//! that can briefly outrun the consumer, pending reports are coalesced per
//! endpoint:
//!
//! - **Keyboard / Consumer** reports carry the *absolute* current state, so a
//!   newer report supersedes an unsent older one (latest-wins). The final
//!   report for an endpoint is therefore always delivered — a release is never
//!   lost. The only thing sustained backpressure can drop is an *intermediate*
//!   state (e.g. a very fast tap), never the resting state, so a key can never
//!   be left stuck.
//! - **Mouse** movement is *relative*, so deltas are accumulated (saturating)
//!   and the latest button state wins — coalescing preserves total travel
//!   instead of discarding motion.
//!
//! This is a pure, hardware-free module (the "functional core"); the async
//! plumbing that drives it lives in [`crate::ble::hid_client`].

use crate::hid::consumer::ConsumerReport;
use crate::hid::keyboard::KeyboardReport;
use crate::hid::mouse::MouseReport;
use crate::hid::HidReport;

/// Number of distinct USB HID endpoints we coalesce independently.
const ENDPOINTS: u8 = 3;

/// Per-endpoint coalescing buffer for the BLE→USB report path.
///
/// Holds at most one pending report per endpoint. See the module docs for the
/// per-endpoint merge policy and why it guarantees release reports survive
/// backpressure.
#[derive(Default)]
pub struct ReportCoalescer {
    keyboard: Option<KeyboardReport>,
    mouse: Option<MouseReport>,
    consumer: Option<ConsumerReport>,
    /// Round-robin cursor so a continuously-busy endpoint can't starve the
    /// others when the writer drains.
    next: u8,
}

impl ReportCoalescer {
    /// Create an empty coalescer.
    pub const fn new() -> Self {
        Self {
            keyboard: None,
            mouse: None,
            consumer: None,
            next: 0,
        }
    }

    /// Enqueue a report, merging it into any unsent pending report for the same
    /// endpoint per the module's policy.
    pub fn push(&mut self, report: HidReport) {
        match report {
            HidReport::Keyboard(k) => self.keyboard = Some(k),
            HidReport::Consumer(c) => self.consumer = Some(c),
            HidReport::Mouse(m) => {
                self.mouse = Some(match self.mouse {
                    Some(pending) => pending.merged_with(&m),
                    None => m,
                });
            }
        }
    }

    /// Remove and return the next pending report, cycling endpoints
    /// round-robin so no endpoint is starved. Returns `None` when empty.
    pub fn pop(&mut self) -> Option<HidReport> {
        for _ in 0..ENDPOINTS {
            let slot = self.next % ENDPOINTS;
            self.next = (self.next + 1) % ENDPOINTS;
            let taken = match slot {
                0 => self.keyboard.take().map(HidReport::Keyboard),
                1 => self.mouse.take().map(HidReport::Mouse),
                _ => self.consumer.take().map(HidReport::Consumer),
            };
            if taken.is_some() {
                return taken;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hid::consumer::ConsumerUsage;

    fn keyboard(modifier: u8, key: u8) -> HidReport {
        HidReport::Keyboard(KeyboardReport {
            modifier,
            reserved: 0,
            keycodes: [key, 0, 0, 0, 0, 0],
        })
    }

    fn mouse(buttons: u8, x: i8, y: i8, wheel: i8) -> HidReport {
        HidReport::Mouse(MouseReport {
            buttons,
            x,
            y,
            wheel,
            pan: 0,
        })
    }

    #[test]
    fn empty_pops_nothing() {
        let mut c = ReportCoalescer::new();
        assert!(c.pop().is_none());
    }

    #[test]
    fn single_report_round_trips() {
        let mut c = ReportCoalescer::new();
        c.push(keyboard(0, 0x04)); // press 'a'
        assert_eq!(c.pop(), Some(keyboard(0, 0x04)));
        assert!(c.pop().is_none());
    }

    #[test]
    fn keyboard_latest_state_wins_but_release_survives() {
        // Press then release pile up behind a busy sink. The resting state
        // (release) must be what's ultimately delivered — never a stuck key.
        let mut c = ReportCoalescer::new();
        c.push(keyboard(0, 0x04)); // 'a' down
        c.push(HidReport::Keyboard(KeyboardReport::empty())); // all up
        assert_eq!(c.pop(), Some(HidReport::Keyboard(KeyboardReport::empty())));
        assert!(c.pop().is_none());
    }

    #[test]
    fn consumer_latest_state_wins() {
        let mut c = ReportCoalescer::new();
        c.push(HidReport::Consumer(ConsumerReport::new(
            ConsumerUsage::VolumeUp,
        )));
        c.push(HidReport::Consumer(ConsumerReport::empty())); // release
        assert_eq!(c.pop(), Some(HidReport::Consumer(ConsumerReport::empty())));
    }

    #[test]
    fn mouse_motion_accumulates_and_latest_buttons_win() {
        let mut c = ReportCoalescer::new();
        c.push(mouse(0, 3, -2, 0));
        c.push(mouse(1, 4, -1, 1)); // left button now down
        assert_eq!(c.pop(), Some(mouse(1, 7, -3, 1)));
        assert!(c.pop().is_none());
    }

    #[test]
    fn mouse_accumulation_saturates() {
        let mut c = ReportCoalescer::new();
        c.push(mouse(0, 100, -100, 0));
        c.push(mouse(0, 100, -100, 0));
        // i8 saturation: 100+100 -> 127, -100-100 -> -128.
        assert_eq!(c.pop(), Some(mouse(0, 127, -128, 0)));
    }

    #[test]
    fn pop_is_round_robin_across_endpoints() {
        let mut c = ReportCoalescer::new();
        c.push(keyboard(0, 0x04));
        c.push(mouse(0, 1, 0, 0));
        c.push(HidReport::Consumer(ConsumerReport::new(
            ConsumerUsage::Mute,
        )));

        // Each endpoint is drained exactly once, in keyboard→mouse→consumer
        // order, then the buffer is empty.
        let mut kinds = [false; 3];
        for _ in 0..3 {
            match c.pop().expect("pending report") {
                HidReport::Keyboard(_) => kinds[0] = true,
                HidReport::Mouse(_) => kinds[1] = true,
                HidReport::Consumer(_) => kinds[2] = true,
            }
        }
        assert_eq!(kinds, [true, true, true]);
        assert!(c.pop().is_none());
    }

    #[test]
    fn endpoints_are_independent() {
        // A flood of keyboard updates must not disturb a pending mouse report.
        let mut c = ReportCoalescer::new();
        c.push(mouse(0, 5, 5, 0));
        c.push(keyboard(0, 0x04));
        c.push(keyboard(0, 0x05));
        c.push(keyboard(0, 0x06));

        // Mouse delta is intact; keyboard collapsed to its latest state.
        let mut got_mouse = None;
        let mut got_kb = None;
        while let Some(r) = c.pop() {
            match r {
                HidReport::Mouse(m) => got_mouse = Some(m),
                HidReport::Keyboard(k) => got_kb = Some(k),
                HidReport::Consumer(_) => unreachable!(),
            }
        }
        assert_eq!(
            got_mouse,
            Some(MouseReport {
                buttons: 0,
                x: 5,
                y: 5,
                wheel: 0,
                pan: 0
            })
        );
        assert_eq!(got_kb.unwrap().keycodes[0], 0x06);
    }
}
