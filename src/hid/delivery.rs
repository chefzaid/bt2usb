//! Bounded, per-endpoint USB delivery state, independent of the USB driver.
//!
//! Normal traffic keeps short press/release sequences in FIFO order. An
//! unavailable or failed endpoint retains the latest absolute state; recovery
//! never replays relative mouse movement. A full FIFO collapses to the latest
//! state so an unpolled endpoint has bounded memory and cannot lose its final
//! release. Intermediate taps/motion can be lost under sustained overload.

use super::{mouse::MouseReport, HidReport};
use core::future::{poll_fn, Future};
use core::pin::pin;
use core::task::Poll;
use heapless::Deque;

/// BLE sources must serialize disconnect after their final report.
#[derive(Clone, Debug, PartialEq)]
pub enum HidEvent {
    Report { source: usize, report: HidReport },
    Disconnected { source: usize },
}

pub const ENDPOINT_QUEUE_CAPACITY: usize = 16;

pub struct PendingReport {
    pub report: HidReport,
    pub epoch: u32,
}

pub struct EndpointDelivery {
    current: HidReport,
    pending: Deque<HidReport, ENDPOINT_QUEUE_CAPACITY>,
    epoch: u32,
    recovering: bool,
}

impl EndpointDelivery {
    pub const fn new(initial: HidReport) -> Self {
        Self {
            current: initial,
            pending: Deque::new(),
            epoch: 0,
            recovering: false,
        }
    }

    /// Queue a report for the endpoint. Returns `true` when the FIFO was full
    /// and collapsed to this latest report, dropping the reports still queued
    /// ([`Counter::EndpointOverflows`](crate::diagnostics::Counter::EndpointOverflows)).
    /// Holding only the current state while the endpoint is unavailable or
    /// recovering is not an overflow.
    pub fn publish(&mut self, report: HidReport, available: bool) -> bool {
        self.current = durable(&report);
        if !available || self.recovering {
            self.recovering = true;
            self.pending.clear();
            let _ = self.pending.push_back(self.current.clone());
            return false;
        }
        let overflowed = self.pending.is_full();
        if overflowed {
            self.pending.clear();
        }
        let _ = self.pending.push_back(report);
        overflowed
    }

    /// Invalidate in-flight work when bus/protocol state changes, then replay
    /// only held state. Reports arriving later follow this snapshot in order.
    pub fn replay(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.recovering = true;
        self.pending.clear();
        let _ = self.pending.push_back(self.current.clone());
    }

    pub fn take(&mut self) -> Option<PendingReport> {
        self.pending.pop_front().map(|report| PendingReport {
            report,
            epoch: self.epoch,
        })
    }

    /// An old transfer finishing after reset must not erase post-reset input.
    /// Failure uses current state, never the stale failed packet.
    pub fn failed(&mut self, epoch: u32) {
        if self.epoch == epoch {
            self.replay();
        }
    }

    pub fn succeeded(&mut self, epoch: u32) {
        if self.epoch == epoch {
            self.recovering = false;
        }
    }

    pub fn is_current(&self, epoch: u32) -> bool {
        self.epoch == epoch
    }
}

fn durable(report: &HidReport) -> HidReport {
    match report {
        HidReport::Mouse(mouse) => HidReport::Mouse(MouseReport {
            buttons: mouse.buttons,
            ..MouseReport::default()
        }),
        _ => report.clone(),
    }
}

/// Synchronous state access and asynchronous notification supplied by a
/// per-endpoint mailbox. No method may hold a lock across an await.
pub trait DeliveryQueue {
    fn available(&self) -> bool;
    fn take(&self) -> Option<PendingReport>;
    /// Clear old lifecycle notifications and validate this transfer's epoch.
    fn start(&self, epoch: u32) -> bool;
    fn failed(&self, epoch: u32, first_failure: bool);
    fn succeeded(&self, epoch: u32);
    fn pending(&self) -> impl Future<Output = ()>;
    fn lifecycle(&self) -> impl Future<Output = ()>;
}

/// The write future MUST be safe to cancel on timeout or USB lifecycle change.
/// Success means the hardware accepted the report, not a host application ack.
pub trait ReportSink {
    fn write(&mut self, report: &HidReport) -> impl Future<Output = Result<(), ()>>;
}

pub trait RetryClock {
    fn after_ms(&self, milliseconds: u64) -> impl Future<Output = ()>;
}

enum First<A, B> {
    Left(A),
    Right(B),
}

async fn first<A: Future, B: Future>(a: A, b: B) -> First<A::Output, B::Output> {
    let mut a = pin!(a);
    let mut b = pin!(b);
    poll_fn(|cx| {
        if let Poll::Ready(value) = a.as_mut().poll(cx) {
            return Poll::Ready(First::Left(value));
        }
        b.as_mut().poll(cx).map(First::Right)
    })
    .await
}

/// One independent endpoint worker. Timeouts and retries are intentionally
/// per-worker; even an endpoint that never gets polled cannot block another.
pub async fn run_endpoint(
    queue: &impl DeliveryQueue,
    sink: &mut impl ReportSink,
    clock: &impl RetryClock,
) -> ! {
    let mut retry_ms = 20;
    loop {
        if !queue.available() {
            queue.pending().await;
            continue;
        }
        let Some(pending) = queue.take() else {
            queue.pending().await;
            continue;
        };
        if !queue.start(pending.epoch) {
            continue;
        }
        // Lifecycle wins if reset/resume and endpoint-ready happen together.
        let outcome = first(
            queue.lifecycle(),
            first(sink.write(&pending.report), clock.after_ms(100)),
        )
        .await;
        match outcome {
            First::Left(()) => retry_ms = 20,
            First::Right(First::Left(Ok(()))) => {
                queue.succeeded(pending.epoch);
                retry_ms = 20;
            }
            First::Right(First::Left(Err(()))) | First::Right(First::Right(())) => {
                queue.failed(pending.epoch, retry_ms == 20);
                first(clock.after_ms(retry_ms), queue.lifecycle()).await;
                retry_ms = (retry_ms * 2).min(1000);
            }
        }
    }
}

#[cfg(test)]
#[path = "delivery_tests.rs"]
mod async_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hid::{consumer::ConsumerReport, keyboard::KeyboardReport};

    fn key(code: u8) -> HidReport {
        HidReport::Keyboard(KeyboardReport {
            keycodes: [code, 0, 0, 0, 0, 0],
            ..KeyboardReport::default()
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
    fn short_taps_keep_press_release_order() {
        let mut endpoint = EndpointDelivery::new(key(0));
        endpoint.publish(key(4), true);
        endpoint.publish(key(0), true);
        assert_eq!(endpoint.take().unwrap().report, key(4));
        assert_eq!(endpoint.take().unwrap().report, key(0));
        assert!(endpoint.take().is_none());
    }

    #[test]
    fn failed_press_recovers_latest_release_not_failed_packet() {
        let mut endpoint = EndpointDelivery::new(key(0));
        endpoint.publish(key(4), true);
        let failed = endpoint.take().unwrap();
        endpoint.publish(key(0), true);
        endpoint.failed(failed.epoch);
        assert_eq!(endpoint.take().unwrap().report, key(0));
    }

    #[test]
    fn failure_resume_and_reset_never_replay_relative_motion() {
        let mut endpoint = EndpointDelivery::new(mouse(0, 0));
        endpoint.publish(mouse(1, 20), true);
        let failed = endpoint.take().unwrap();
        endpoint.failed(failed.epoch);
        assert_eq!(endpoint.take().unwrap().report, mouse(1, 0));
        endpoint.publish(mouse(2, 30), false);
        endpoint.replay();
        assert_eq!(endpoint.take().unwrap().report, mouse(2, 0));
        endpoint.replay();
        assert_eq!(endpoint.take().unwrap().report, mouse(2, 0));
    }

    #[test]
    fn stale_completion_cannot_erase_post_reset_input() {
        let mut endpoint = EndpointDelivery::new(key(0));
        endpoint.publish(key(4), true);
        let old = endpoint.take().unwrap();
        endpoint.replay();
        endpoint.publish(key(5), true);
        endpoint.publish(key(0), true);
        endpoint.failed(old.epoch);
        assert_eq!(endpoint.take().unwrap().report, key(0));
        assert!(endpoint.take().is_none());
    }

    #[test]
    fn queue_overflow_preserves_final_release() {
        let mut endpoint = EndpointDelivery::new(key(0));
        for _ in 0..100 {
            endpoint.publish(key(4), true);
        }
        endpoint.publish(key(0), true);
        let mut last = None;
        while let Some(report) = endpoint.take() {
            last = Some(report.report);
        }
        assert_eq!(last, Some(key(0)));
    }

    #[test]
    fn publish_reports_only_a_full_queue_collapsing() {
        let mut endpoint = EndpointDelivery::new(key(0));
        for code in 0..ENDPOINT_QUEUE_CAPACITY {
            assert!(!endpoint.publish(key(code as u8), true));
        }
        // The queue is full: this report replaces every queued one.
        assert!(endpoint.publish(key(0), true));
        assert_eq!(endpoint.take().unwrap().report, key(0));
        assert!(endpoint.take().is_none());
        // Holding the current state while the endpoint is unavailable or
        // recovering is not an overflow, however much input arrives.
        for _ in 0..2 * ENDPOINT_QUEUE_CAPACITY {
            assert!(!endpoint.publish(key(4), false));
        }
        for _ in 0..2 * ENDPOINT_QUEUE_CAPACITY {
            assert!(!endpoint.publish(key(5), true));
        }
    }

    #[test]
    fn blocked_consumer_does_not_block_keyboard_or_mouse_state() {
        let mut keyboard = EndpointDelivery::new(key(0));
        let mut mouse_endpoint = EndpointDelivery::new(mouse(0, 0));
        let mut consumer = EndpointDelivery::new(HidReport::Consumer(ConsumerReport::default()));
        consumer.publish(HidReport::Consumer(ConsumerReport { usage: 0xe9 }), true);
        let _blocked_transfer = consumer.take().unwrap();
        for _ in 0..100 {
            consumer.publish(HidReport::Consumer(ConsumerReport { usage: 0xe9 }), true);
        }
        keyboard.publish(key(4), true);
        keyboard.publish(key(0), true);
        mouse_endpoint.publish(mouse(1, 4), true);
        assert_eq!(keyboard.take().unwrap().report, key(4));
        assert_eq!(keyboard.take().unwrap().report, key(0));
        assert_eq!(mouse_endpoint.take().unwrap().report, mouse(1, 4));
    }
}
