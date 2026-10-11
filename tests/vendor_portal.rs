//! Host tests for the vendored nrf-softdevice event portal, compiled from
//! `vendor/nrf-softdevice/src/util/portal.rs` itself.
//!
//! A wait that ends, completed or cancelled, must clear the portal only while
//! the portal still holds its own closure. The vendored crate fails a link's
//! pending GATT waits on `DISCONNECTED`, and the SoftDevice can give the freed
//! connection handle to the other slot's link, whose MTU exchange then waits
//! on the same portal, before the failed wait's task runs again (ADR 0007,
//! `vendor/nrf-softdevice/README.bt2usb.md`).

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

// The portal's `use embassy_sync::...` resolves to this crate: the version the
// vendored crate depends on, renamed in `Cargo.toml` because the firmware
// depends on an older one under the plain name.
extern crate portal_embassy_sync as embassy_sync;

// The vendored sources keep their own formatting: `rustfmt::skip` stops
// `cargo fmt` from following these modules into `vendor/`.
#[rustfmt::skip]
#[allow(unused, clippy::all, clippy::restriction)]
#[path = "../vendor/nrf-softdevice/src/util/on_drop.rs"]
mod on_drop;

#[rustfmt::skip]
#[allow(unexpected_cfgs, unused, clippy::all, clippy::restriction)]
#[path = "../vendor/nrf-softdevice/src/util/portal.rs"]
mod portal;

/// What the portal's `use crate::util::OnDrop` resolves to.
mod util {
    pub use crate::on_drop::OnDrop;
}

use portal::Portal;

/// The event that fails every pending GATT wait of a link.
const DISCONNECTED: u8 = 0;

fn poll<F: Future + ?Sized>(fut: Pin<&mut F>) -> Poll<F::Output> {
    fut.poll(&mut Context::from_waker(Waker::noop()))
}

/// A GATT wait as the vendored client writes them: done on `DISCONNECTED`, or
/// on the response it waits for (any other event here).
fn gatt_wait(portal: &Portal<u8>) -> Pin<Box<impl Future<Output = u8> + '_>> {
    Box::pin(portal.wait_many(Some))
}

/// Runs `test` on a thread named `main`. Without the crate's
/// `usable-from-interrupts` feature, as in the firmware, the portal locks a
/// `ThreadModeRawMutex`, which on the host accepts only that thread.
fn in_thread_mode(test: impl FnOnce() + Send + 'static) {
    let finished = std::thread::Builder::new()
        .name("main".into())
        .spawn(test)
        .map(|thread| thread.join());
    assert!(matches!(finished, Ok(Ok(()))), "the test thread failed");
}

#[test]
fn a_wait_failed_by_disconnect_leaves_the_next_links_wait_registered() {
    in_thread_mode(|| {
        let portal = Portal::<u8>::new();
        let mut old = gatt_wait(&portal);
        assert!(poll(old.as_mut()).is_pending());
        assert!(portal.call(DISCONNECTED));
        // The other slot's link gets the freed handle and starts its MTU
        // exchange before the old link's task runs again.
        let mut next = gatt_wait(&portal);
        assert!(poll(next.as_mut()).is_pending());
        assert_eq!(poll(old.as_mut()), Poll::Ready(DISCONNECTED));
        // The next link's wait still gets its response.
        assert!(portal.call(7));
        assert_eq!(poll(next.as_mut()), Poll::Ready(7));
    });
}

#[test]
fn a_wait_dropped_after_its_disconnect_leaves_the_next_links_wait_registered() {
    in_thread_mode(|| {
        let portal = Portal::<u8>::new();
        // An LED write that a notification loop's select drops, unpolled,
        // once the link's notification wait ends on `DISCONNECTED`.
        let mut old = gatt_wait(&portal);
        assert!(poll(old.as_mut()).is_pending());
        assert!(portal.call(DISCONNECTED));
        let mut next = gatt_wait(&portal);
        assert!(poll(next.as_mut()).is_pending());
        drop(old);
        assert!(portal.call(7));
        assert_eq!(poll(next.as_mut()), Poll::Ready(7));
    });
}

#[test]
fn a_completed_single_wait_leaves_a_later_wait_registered() {
    in_thread_mode(|| {
        let portal = Portal::<u8>::new();
        let mut old = Box::pin(portal.wait_once(|event| event));
        assert!(poll(old.as_mut()).is_pending());
        assert!(portal.call(DISCONNECTED));
        let mut next = Box::pin(portal.wait_once(|event| event));
        assert!(poll(next.as_mut()).is_pending());
        assert_eq!(poll(old.as_mut()), Poll::Ready(DISCONNECTED));
        assert!(portal.call(7));
        assert_eq!(poll(next.as_mut()), Poll::Ready(7));
    });
}

#[test]
fn a_cancelled_wait_clears_its_own_registration() {
    in_thread_mode(|| {
        let portal = Portal::<u8>::new();
        for cancelled in [
            Box::pin(portal.wait_once(|event| event)) as Pin<Box<dyn Future<Output = u8>>>,
            gatt_wait(&portal),
        ] {
            let mut cancelled = cancelled;
            assert!(poll(cancelled.as_mut()).is_pending());
            drop(cancelled);
            // Nothing is left registered, so a new wait can register.
            assert!(!portal.call(1));
        }
        let mut next = gatt_wait(&portal);
        assert!(poll(next.as_mut()).is_pending());
        assert!(portal.call(2));
        assert_eq!(poll(next.as_mut()), Poll::Ready(2));
    });
}

#[test]
fn a_wait_that_keeps_waiting_stays_registered() {
    in_thread_mode(|| {
        let portal = Portal::<u8>::new();
        // A discovery wait skips an event it did not expect (bt2usb patch in
        // gatt_client.rs) and keeps its registration.
        let mut wait = Box::pin(portal.wait_many(|event| (event != 9).then_some(event)));
        assert!(poll(wait.as_mut()).is_pending());
        assert!(portal.call(9));
        assert!(poll(wait.as_mut()).is_pending());
        assert!(portal.call(DISCONNECTED));
        assert_eq!(poll(wait.as_mut()), Poll::Ready(DISCONNECTED));
        assert!(!portal.call(3));
    });
}
