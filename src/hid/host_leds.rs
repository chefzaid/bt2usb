//! Keep a BLE keyboard's lock-key LEDs in step with the USB host.
//!
//! The host owns the Num Lock, Caps Lock and Scroll Lock state and sends it to
//! the bridge's USB keyboard as an output report. A BLE keyboard shows that
//! state only when the bridge writes it to the keyboard's own output report.
//!
//! A keyboard that powers up, wakes, or reconnects starts with its LEDs off.
//! Forwarding only *changes* would leave it dark until the user next toggles a
//! lock key, so the keyboard would show Caps Lock off while the host has it on.
//! [`forward_host_leds`] therefore writes the host's current state as soon as
//! a link starts, then every change after it.

use super::keyboard::KeyboardLeds;
use core::future::Future;

/// The host's LED state, as the USB keyboard's output report delivers it.
pub trait HostLeds {
    /// The host's latest LED state, or `None` if it has not sent one since
    /// USB enumeration. The returned state counts as seen, so
    /// [`changed`](Self::changed) waits for a newer one.
    fn current(&mut self) -> Option<KeyboardLeds>;

    /// Wait for an LED state newer than the last one seen, and return it.
    fn changed(&mut self) -> impl Future<Output = KeyboardLeds>;
}

/// Forward host LED state to a keyboard for the life of one link.
///
/// Writes the host's current state first, then each change. Never returns:
/// the caller drops it when the link ends. `write` sends one state to the
/// keyboard; a failed write is the caller's to log, and the next change is
/// written regardless.
pub async fn forward_host_leds<H, W, F>(host: &mut H, mut write: W)
where
    H: HostLeds,
    W: FnMut(KeyboardLeds) -> F,
    F: Future<Output = ()>,
{
    if let Some(leds) = host.current() {
        write(leds).await;
    }
    loop {
        let leds = host.changed().await;
        write(leds).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::RefCell;
    use core::future::poll_fn;
    use core::pin::pin;
    use core::task::{Context, Poll, Waker};
    use std::rc::Rc;
    use std::vec::Vec;

    /// The USB side: the latest LED state and a count of states sent, as the
    /// firmware's `Watch` keeps them.
    #[derive(Default)]
    struct Host {
        leds: Option<KeyboardLeds>,
        sent: u64,
    }

    impl Host {
        fn send(&mut self, byte: u8) {
            self.leds = Some(KeyboardLeds::from_byte(byte));
            self.sent += 1;
        }
    }

    /// One slot's receiver: it remembers which state it last saw, across links.
    struct Receiver {
        host: Rc<RefCell<Host>>,
        seen: u64,
    }

    impl HostLeds for Receiver {
        fn current(&mut self) -> Option<KeyboardLeds> {
            let host = self.host.borrow();
            self.seen = host.sent;
            host.leds
        }

        async fn changed(&mut self) -> KeyboardLeds {
            poll_fn(|_| {
                let host = self.host.borrow();
                match host.leds {
                    Some(leds) if host.sent != self.seen => {
                        self.seen = host.sent;
                        Poll::Ready(leds)
                    }
                    _ => Poll::Pending,
                }
            })
            .await
        }
    }

    fn setup() -> (Rc<RefCell<Host>>, Receiver) {
        let host = Rc::new(RefCell::new(Host::default()));
        let receiver = Receiver {
            host: host.clone(),
            seen: 0,
        };
        (host, receiver)
    }

    /// Run one link's forwarder until it blocks, returning the bytes written.
    fn run_link(receiver: &mut Receiver, steps: &mut dyn FnMut(usize, &RefCell<Host>)) -> Vec<u8> {
        let writes = RefCell::new(Vec::new());
        let host = receiver.host.clone();
        {
            let mut link = pin!(forward_host_leds(receiver, |leds: KeyboardLeds| {
                writes.borrow_mut().push(leds.byte());
                async {}
            }));
            let mut cx = Context::from_waker(Waker::noop());
            for step in 0.. {
                assert!(link.as_mut().poll(&mut cx).is_pending());
                let before = host.borrow().sent;
                steps(step, &host);
                if host.borrow().sent == before {
                    break;
                }
            }
        }
        writes.into_inner()
    }

    #[test]
    fn a_new_link_gets_the_hosts_current_state_first() {
        let (host, mut receiver) = setup();
        host.borrow_mut().send(0x02); // Caps Lock on, before the keyboard connects.
        assert_eq!(run_link(&mut receiver, &mut |_, _| {}), [0x02]);
    }

    #[test]
    fn a_reconnecting_keyboard_gets_a_state_this_slot_already_forwarded() {
        let (host, mut receiver) = setup();
        host.borrow_mut().send(0x02);
        // First link: the keyboard is told Caps Lock is on.
        assert_eq!(run_link(&mut receiver, &mut |_, _| {}), [0x02]);
        // The keyboard sleeps and reconnects with its LEDs off. The host has
        // not changed anything, so only a write of the current state fixes it.
        assert_eq!(run_link(&mut receiver, &mut |_, _| {}), [0x02]);
    }

    #[test]
    fn nothing_is_written_before_the_host_sends_a_state() {
        let (_host, mut receiver) = setup();
        assert!(run_link(&mut receiver, &mut |_, _| {}).is_empty());
    }

    #[test]
    fn every_change_after_the_first_write_is_forwarded_in_order() {
        let (host, mut receiver) = setup();
        host.borrow_mut().send(0x00);
        let written = run_link(&mut receiver, &mut |step, host| match step {
            0 => host.borrow_mut().send(0x02),
            1 => host.borrow_mut().send(0x03),
            _ => {}
        });
        assert_eq!(written, [0x00, 0x02, 0x03]);
    }

    #[test]
    fn the_first_state_the_host_sends_is_forwarded() {
        let (_host, mut receiver) = setup();
        let written = run_link(&mut receiver, &mut |step, host| {
            if step == 0 {
                host.borrow_mut().send(0x01);
            }
        });
        assert_eq!(written, [0x01]);
    }
}
