//! The bench the coordinator loop's fault tests (`coordinator_loop_tests.rs`)
//! run on: a scripted store, scanner, and channels for the production loop.
//! The test plays both connection workers, answering the commands the loop
//! sends them with the events a worker reports.

use super::*;
use core::cell::{Cell, RefCell};
use core::future::poll_fn;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use std::collections::VecDeque;
use std::rc::Rc;
use std::vec::Vec as StdVec;

/// The keyboard's saved identity address.
pub const KEYBOARD: u8 = 0x10;
/// The private address the keyboard advertises from now, which its identity
/// key resolves.
pub const KEYBOARD_NOW: u8 = 0x11;
pub const MOUSE: u8 = 0x20;
/// A third saved device, older than both.
pub const REMOTE: u8 = 0x30;

pub fn device(address: u8) -> DeviceInfo<u8> {
    let name = match address {
        KEYBOARD | KEYBOARD_NOW => "Keyboard",
        MOUSE => "Mouse",
        _ => "Remote",
    };
    DeviceInfo {
        address,
        name: name.try_into().unwrap(),
        rssi: -50,
    }
}

pub fn shows(summary: &str) -> Event<u8> {
    Event::Connected(summary.try_into().unwrap())
}

/// What the loop did, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    Send(usize, SlotCommand<u8>),
    Emit(Event<u8>),
    Scan,
    Persist(u8),
    Discard(u8),
    Forget(Option<u8>),
}

/// The scripted firmware. Each queue is consumed as the loop polls; an empty
/// one leaves the loop waiting, as a channel or a scan in progress would.
pub struct State {
    pub commands: RefCell<VecDeque<Command<u8>>>,
    pub events: RefCell<VecDeque<SlotEvent<u8>>>,
    /// Saved devices, newest first; every one has keys.
    pub saved: RefCell<StdVec<u8>>,
    pub writable: Cell<bool>,
    /// The next user scans' results; `None` fails the scan.
    pub scans: RefCell<VecDeque<Option<StdVec<u8>>>>,
    /// The number of the last attempt sent to each slot's worker.
    pub attempts: RefCell<[u32; MAX_CONNECTIONS]>,
    pub calls: RefCell<StdVec<Call>>,
}

fn next<T>(queue: &RefCell<VecDeque<T>>) -> Option<T> {
    queue.borrow_mut().pop_front()
}

/// The keyboard's identity key resolves both of its addresses.
fn same_peer(a: u8, b: u8) -> bool {
    let identity = |address| match address {
        KEYBOARD_NOW => KEYBOARD,
        other => other,
    };
    identity(a) == identity(b)
}

pub struct Fake(Rc<State>);

impl Fake {
    fn call(&self, call: Call) {
        self.0.calls.borrow_mut().push(call);
    }
}

impl CoordinatorIo for Fake {
    type Address = u8;
    type Saved = u8;

    async fn load(&mut self) -> bool {
        self.0.writable.get()
    }

    async fn reconnect_targets(&mut self) -> Vec<DeviceInfo<u8>, MAX_CONNECTIONS> {
        let saved = self.0.saved.borrow();
        saved
            .iter()
            .take(MAX_CONNECTIONS)
            .map(|&a| device(a))
            .collect()
    }

    async fn next(&mut self) -> Input<u8> {
        poll_fn(|_| {
            if let Some(command) = next(&self.0.commands) {
                return Poll::Ready(Input::Command(command));
            }
            next(&self.0.events).map_or(Poll::Pending, |event| Poll::Ready(Input::Slot(event)))
        })
        .await
    }

    async fn slot_event(&mut self) -> SlotEvent<u8> {
        poll_fn(|_| next(&self.0.events).map_or(Poll::Pending, Poll::Ready)).await
    }

    async fn emit(&mut self, event: Event<u8>) {
        self.call(Call::Emit(event));
    }

    async fn command(&mut self, slot: usize, command: SlotCommand<u8>) {
        if let SlotCommand::Connect { attempt, .. } | SlotCommand::Reconnect { attempt, .. } =
            command
        {
            self.0.attempts.borrow_mut()[slot] = attempt;
        }
        self.call(Call::Send(slot, command));
    }

    async fn scan(&mut self) -> Option<Vec<DeviceInfo<u8>, BLE_MAX_DISCOVERED>> {
        self.call(Call::Scan);
        let heard = poll_fn(|_| next(&self.0.scans).map_or(Poll::Pending, Poll::Ready)).await;
        heard.map(|addresses| addresses.into_iter().map(device).collect())
    }

    fn same_peer(&self, held: &u8, seen: &u8) -> bool {
        same_peer(*held, *seen)
    }

    async fn paired(&mut self) -> Vec<DeviceInfo<u8>, MAX_PAIRED_DEVICES> {
        self.0.saved.borrow().iter().map(|&a| device(a)).collect()
    }

    async fn find(&mut self, identity: u8) -> Option<u8> {
        self.0
            .saved
            .borrow()
            .contains(&identity)
            .then_some(identity)
    }

    fn holds(&self, saved: &u8, address: &u8) -> bool {
        same_peer(*saved, *address)
    }

    async fn forget(&mut self, saved: Option<u8>) -> bool {
        self.call(Call::Forget(saved));
        if !self.0.writable.get() {
            return false;
        }
        let mut list = self.0.saved.borrow_mut();
        match saved {
            Some(identity) => list.retain(|&a| a != identity),
            None => list.clear(),
        }
        true
    }

    async fn persist(&mut self, device: DeviceInfo<u8>) {
        self.call(Call::Persist(device.address));
        let mut list = self.0.saved.borrow_mut();
        if !list.iter().any(|&a| same_peer(a, device.address)) {
            list.insert(0, device.address);
        }
    }

    fn discard_unsaved_bond(&mut self, address: u8) {
        self.call(Call::Discard(address));
    }
}

/// The scripted firmware and what the loop did with it.
pub struct Bench {
    pub state: Rc<State>,
}

impl Bench {
    /// A bridge holding these saved devices, newest first.
    pub fn new(saved: &[u8]) -> Self {
        Self {
            state: Rc::new(State {
                commands: RefCell::default(),
                events: RefCell::default(),
                saved: RefCell::new(saved.to_vec()),
                writable: Cell::new(true),
                scans: RefCell::default(),
                attempts: RefCell::default(),
                calls: RefCell::default(),
            }),
        }
    }

    pub fn fake(&self) -> Fake {
        Fake(self.state.clone())
    }

    /// Poll the loop until it waits for something the test has not supplied.
    pub fn settle<F: Future>(&self, coordinator: &mut Pin<&mut F>) {
        let mut cx = Context::from_waker(Waker::noop());
        assert!(coordinator.as_mut().poll(&mut cx).is_pending());
    }

    /// The UI sends `command`.
    pub fn ui<F: Future>(&self, coordinator: &mut Pin<&mut F>, command: Command<u8>) {
        self.state.commands.borrow_mut().push_back(command);
        self.settle(coordinator);
    }

    /// A worker reports `event`.
    pub fn report<F: Future>(&self, coordinator: &mut Pin<&mut F>, event: SlotEvent<u8>) {
        self.state.events.borrow_mut().push_back(event);
        self.settle(coordinator);
    }

    /// The number of the last attempt the loop sent `slot`'s worker.
    pub fn attempt(&self, slot: usize) -> u32 {
        self.state.attempts.borrow()[slot]
    }

    /// `slot`'s worker reports its attempt connected to `address`.
    pub fn connected<F: Future>(&self, coordinator: &mut Pin<&mut F>, slot: usize, address: u8) {
        let attempt = self.attempt(slot);
        let device = device(address);
        self.report(
            coordinator,
            SlotEvent::Connected {
                slot,
                attempt,
                device,
            },
        );
    }

    /// Everything the loop did, after forgetting it.
    pub fn take(&self) -> StdVec<Call> {
        self.state.calls.take()
    }
}
