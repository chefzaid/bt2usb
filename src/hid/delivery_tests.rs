//! Poll the production endpoint worker against cancel-safe fake USB endpoints.

use super::*;
use crate::hid::{consumer::ConsumerReport, keyboard::KeyboardReport};
use core::cell::{Cell, RefCell};
use core::task::{Context, Waker};
use std::rc::Rc;
use std::vec::Vec;

struct Queue {
    state: RefCell<EndpointDelivery>,
    available: Cell<bool>,
    pending: Cell<bool>,
    lifecycle: Cell<bool>,
}

impl Queue {
    fn new(report: HidReport) -> Self {
        Self {
            state: RefCell::new(EndpointDelivery::new(report)),
            available: Cell::new(true),
            pending: Cell::new(false),
            lifecycle: Cell::new(false),
        }
    }

    fn publish(&self, report: HidReport) {
        self.state
            .borrow_mut()
            .publish(report, self.available.get());
        self.pending.set(true);
    }

    fn bus(&self, available: bool) {
        self.available.set(available);
        self.state.borrow_mut().replay();
        self.pending.set(true);
        self.lifecycle.set(true);
    }
}

async fn signal(flag: &Cell<bool>) {
    poll_fn(|_| {
        if flag.replace(false) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
}

impl DeliveryQueue for Queue {
    fn available(&self) -> bool {
        self.available.get()
    }
    fn take(&self) -> Option<PendingReport> {
        self.state.borrow_mut().take()
    }
    fn start(&self, epoch: u32) -> bool {
        self.lifecycle.set(false);
        self.state.borrow().is_current(epoch) && self.available.get()
    }
    fn failed(&self, epoch: u32, _: bool) {
        self.state.borrow_mut().failed(epoch);
    }
    fn succeeded(&self, epoch: u32) {
        self.state.borrow_mut().succeeded(epoch);
    }
    async fn pending(&self) {
        signal(&self.pending).await;
    }
    async fn lifecycle(&self) {
        signal(&self.lifecycle).await;
    }
}

#[derive(Default)]
struct SinkState {
    blocked: Cell<bool>,
    error: Cell<bool>,
    writes: RefCell<Vec<HidReport>>,
}

struct Sink(Rc<SinkState>);

impl ReportSink for Sink {
    async fn write(&mut self, report: &HidReport) -> Result<(), ()> {
        poll_fn(|_| {
            if self.0.blocked.get() {
                return Poll::Pending;
            }
            if self.0.error.get() {
                return Poll::Ready(Err(()));
            }
            self.0.writes.borrow_mut().push(report.clone());
            Poll::Ready(Ok(()))
        })
        .await
    }
}

#[derive(Default)]
struct Clock {
    now: Cell<u64>,
    sleeps: RefCell<Vec<u64>>,
}

impl RetryClock for Clock {
    async fn after_ms(&self, milliseconds: u64) {
        self.sleeps.borrow_mut().push(milliseconds);
        let due = self.now.get() + milliseconds;
        poll_fn(|_| {
            if self.now.get() >= due {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }
}

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
fn unpolled_consumer_allows_actual_keyboard_and_mouse_workers_to_write() {
    let keyboard_queue = Queue::new(key(0));
    let mouse_queue = Queue::new(mouse(0, 0));
    let consumer_queue = Queue::new(HidReport::Consumer(ConsumerReport::default()));
    let keyboard = Rc::new(SinkState::default());
    let mouse_sink = Rc::new(SinkState::default());
    let consumer = Rc::new(SinkState::default());
    consumer.blocked.set(true);
    let mut keyboard_writer = Sink(keyboard.clone());
    let mut mouse_writer = Sink(mouse_sink.clone());
    let mut consumer_writer = Sink(consumer.clone());
    let clock = Clock::default();
    let mut workers = pin!(async {
        // All three futures are polled on each wake, matching Embassy join4.
        let mut k = pin!(run_endpoint(&keyboard_queue, &mut keyboard_writer, &clock));
        let mut m = pin!(run_endpoint(&mouse_queue, &mut mouse_writer, &clock));
        let mut c = pin!(run_endpoint(&consumer_queue, &mut consumer_writer, &clock));
        poll_fn(|cx| {
            let _ = k.as_mut().poll(cx);
            let _ = m.as_mut().poll(cx);
            let _ = c.as_mut().poll(cx);
            Poll::<()>::Pending
        })
        .await;
    });
    let mut cx = Context::from_waker(Waker::noop());
    consumer_queue.publish(HidReport::Consumer(ConsumerReport { usage: 0xe9 }));
    assert!(workers.as_mut().poll(&mut cx).is_pending());
    keyboard_queue.publish(key(4));
    keyboard_queue.publish(key(0));
    mouse_queue.publish(mouse(1, 7));
    assert!(workers.as_mut().poll(&mut cx).is_pending());
    assert_eq!(*keyboard.writes.borrow(), [key(4), key(0)]);
    assert_eq!(*mouse_sink.writes.borrow(), [mouse(1, 7)]);
    assert!(consumer.writes.borrow().is_empty());
}

#[test]
fn timed_out_press_retries_latest_release_after_backoff() {
    let queue = Queue::new(key(0));
    let sink = Rc::new(SinkState::default());
    sink.blocked.set(true);
    let mut writer = Sink(sink.clone());
    let clock = Clock::default();
    let mut worker = pin!(run_endpoint(&queue, &mut writer, &clock));
    let mut cx = Context::from_waker(Waker::noop());
    queue.publish(key(4));
    assert!(worker.as_mut().poll(&mut cx).is_pending());
    queue.publish(key(0));
    clock.now.set(100);
    assert!(worker.as_mut().poll(&mut cx).is_pending());
    sink.blocked.set(false);
    clock.now.set(119);
    assert!(worker.as_mut().poll(&mut cx).is_pending());
    assert!(sink.writes.borrow().is_empty());
    clock.now.set(120);
    assert!(worker.as_mut().poll(&mut cx).is_pending());
    assert_eq!(*sink.writes.borrow(), [key(0)]);
}

#[test]
fn bus_change_cancels_old_motion_before_endpoint_can_accept_it() {
    let queue = Queue::new(mouse(0, 0));
    let sink = Rc::new(SinkState::default());
    sink.blocked.set(true);
    let mut writer = Sink(sink.clone());
    let clock = Clock::default();
    let mut worker = pin!(run_endpoint(&queue, &mut writer, &clock));
    let mut cx = Context::from_waker(Waker::noop());
    queue.publish(mouse(1, 20));
    assert!(worker.as_mut().poll(&mut cx).is_pending());
    queue.bus(false);
    queue.publish(mouse(2, 30));
    queue.bus(true);
    // Even if endpoint readiness and resume arrive together, lifecycle wins.
    sink.blocked.set(false);
    assert!(worker.as_mut().poll(&mut cx).is_pending());
    assert_eq!(*sink.writes.borrow(), [mouse(2, 0)]);
    queue.publish(mouse(2, 4));
    assert!(worker.as_mut().poll(&mut cx).is_pending());
    assert_eq!(*sink.writes.borrow(), [mouse(2, 0), mouse(2, 4)]);
}

#[test]
fn repeated_usb_errors_use_capped_backoff_and_eventually_recover() {
    let queue = Queue::new(key(0));
    let sink = Rc::new(SinkState::default());
    sink.error.set(true);
    let mut writer = Sink(sink.clone());
    let clock = Clock::default();
    let mut worker = pin!(run_endpoint(&queue, &mut writer, &clock));
    let mut cx = Context::from_waker(Waker::noop());
    queue.publish(key(4));
    for step in 0..10 {
        clock.now.set(step * 1000);
        assert!(worker.as_mut().poll(&mut cx).is_pending());
    }
    assert_eq!(
        &clock.sleeps.borrow()[..7],
        &[20, 40, 80, 160, 320, 640, 1000]
    );
    assert!(clock.sleeps.borrow().iter().all(|delay| *delay <= 1000));
    queue.publish(key(0));
    sink.error.set(false);
    clock.now.set(10000);
    assert!(worker.as_mut().poll(&mut cx).is_pending());
    // A release received during backoff replaces the obsolete recovery press.
    assert_eq!(*sink.writes.borrow(), [key(0)]);
}
