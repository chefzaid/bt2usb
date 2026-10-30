//! The bench the slot worker loop's fault tests (`slot_loop_tests.rs`) run
//! on: a scripted radio for the production loop, and the real coordinator
//! reducers for slot 0, whose commands go back to the worker and whose UI
//! events are kept.

use super::*;
use crate::ble::coordinator::{self, Action, ConnManager, UiEvent};
use core::cell::{Cell, RefCell};
use core::future::poll_fn;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use std::collections::VecDeque;
use std::rc::Rc;
use std::vec::Vec;

pub const SLOT: usize = 0;
/// The keyboard's saved identity address.
pub const KEYBOARD: u8 = 0x10;
/// The private address the keyboard advertises from now, which its identity
/// key resolves.
pub const KEYBOARD_NOW: u8 = 0x11;

pub fn keyboard() -> DeviceInfo<u8> {
    DeviceInfo {
        address: KEYBOARD,
        name: "Keyboard".try_into().unwrap(),
        rssi: -50,
    }
}

pub fn shows_keyboard() -> UiEvent {
    UiEvent::Connected(["Keyboard".try_into().unwrap()].into_iter().collect())
}

/// What the UI is sent when the slot's attempt fails: the error, then that no
/// link is up.
pub fn failed(tag: ErrorTag) -> [UiEvent; 2] {
    [UiEvent::Error(tag), UiEvent::Disconnected]
}

/// One step of the scripted link an attempt runs on.
pub enum Step {
    /// The link is secured and discovered: report it connected.
    Up,
    /// The user's connection to the same device took the attempt over.
    TakeOver(u32),
    /// The attempt ends.
    End(SlotOutcome<u8>),
}

/// What the worker did, apart from its events.
#[derive(Debug, PartialEq)]
pub enum Call {
    Attempt {
        address: u8,
        pairing: bool,
        attempt: u32,
    },
    Note(Note),
}

/// The scripted radio. Each queue is consumed as the worker polls; an empty
/// one leaves the worker waiting, as a scan or a link in progress would.
#[derive(Default)]
pub struct State {
    pub commands: RefCell<VecDeque<SlotCommand<u8>>>,
    pub events: RefCell<VecDeque<SlotEvent<u8>>>,
    /// The coordinator's event channel is full: reports wait.
    pub channel_full: Cell<bool>,
    /// Identity addresses the bridge holds keys for.
    pub keys: RefCell<Vec<u8>>,
    pub scans: RefCell<VecDeque<Option<u8>>>,
    pub link: RefCell<VecDeque<Step>>,
    /// Backoff pauses allowed to end.
    pub backoffs: Cell<u32>,
    pub sighted: Cell<bool>,
    /// The slot's reconnect scan target.
    pub target: Cell<Option<u8>>,
    pub failed_attempts: Cell<u32>,
    pub calls: RefCell<Vec<Call>>,
}

fn next<T>(queue: &RefCell<VecDeque<T>>) -> Poll<T> {
    queue
        .borrow_mut()
        .pop_front()
        .map_or(Poll::Pending, Poll::Ready)
}

pub struct Fake(Rc<State>);

impl SlotIo for Fake {
    type Address = u8;
    type Identity = u8;

    async fn command(&self) -> SlotCommand<u8> {
        poll_fn(|_| next(&self.0.commands)).await
    }

    async fn report(&self, event: SlotEvent<u8>) {
        poll_fn(|_| match self.0.channel_full.get() {
            true => Poll::Pending,
            false => Poll::Ready(()),
        })
        .await;
        self.0.events.borrow_mut().push_back(event);
    }

    async fn backoff(&self) {
        poll_fn(|_| match self.0.backoffs.get() {
            0 => Poll::Pending,
            left => {
                self.0.backoffs.set(left - 1);
                Poll::Ready(())
            }
        })
        .await;
    }

    fn identity(&self, address: u8) -> Option<u8> {
        let identity = if address == KEYBOARD_NOW {
            KEYBOARD
        } else {
            address
        };
        self.0.keys.borrow().contains(&identity).then_some(identity)
    }

    fn register_reconnect(&self, peer: SavedPeer<u8, u8>) {
        self.0.target.set(Some(peer.address));
    }

    fn clear_reconnect(&self) {
        self.0.target.set(None);
    }

    fn reconnect_attempt_failed(&self) {
        self.0.failed_attempts.set(self.0.failed_attempts.get() + 1);
    }

    async fn reconnect_sighted(&self) {
        poll_fn(|_| match self.0.sighted.replace(false) {
            true => Poll::Ready(()),
            false => Poll::Pending,
        })
        .await;
    }

    async fn find_saved_peer(&self) -> Option<u8> {
        poll_fn(|_| next(&self.0.scans)).await
    }

    async fn attempt(
        &mut self,
        device: &DeviceInfo<u8>,
        allow_pairing: bool,
        attempt: &mut u32,
    ) -> SlotOutcome<u8> {
        self.0.calls.borrow_mut().push(Call::Attempt {
            address: device.address,
            pairing: allow_pairing,
            attempt: *attempt,
        });
        loop {
            match poll_fn(|_| next(&self.0.link)).await {
                // `slot_link` sends Connected itself, through the same channel.
                Step::Up => self.0.events.borrow_mut().push_back(SlotEvent::Connected {
                    slot: SLOT,
                    attempt: *attempt,
                    device: device.clone(),
                }),
                Step::TakeOver(next) => *attempt = next,
                Step::End(outcome) => return outcome,
            }
        }
    }

    fn note(&self, note: Note) {
        self.0.calls.borrow_mut().push(Call::Note(note));
    }
}

/// The worker's state and the coordinator's for slot 0: the real reducers,
/// whose slot commands go to the worker and whose UI events are kept.
pub struct Bench {
    pub state: Rc<State>,
    pub manager: ConnManager<u8>,
    pub ui: Vec<UiEvent>,
    pub discarded: Vec<u8>,
}

impl Bench {
    pub fn new(keys: &[u8]) -> Self {
        let state = State::default();
        state.keys.borrow_mut().extend_from_slice(keys);
        Self {
            state: Rc::new(state),
            manager: ConnManager::new(),
            ui: Vec::new(),
            discarded: Vec::new(),
        }
    }

    pub fn fake(&self) -> Fake {
        Fake(self.state.clone())
    }

    pub fn execute(&mut self, actions: impl IntoIterator<Item = Action<u8>>) {
        for action in actions {
            let command = match action {
                Action::ConnectSlot {
                    slot,
                    device,
                    attempt,
                } => {
                    assert_eq!(slot, SLOT);
                    SlotCommand::Connect { device, attempt }
                }
                Action::DisconnectSlot(_) => SlotCommand::Disconnect,
                Action::Emit(event) => {
                    self.ui.push(event);
                    continue;
                }
                Action::DiscardUnsavedBond(address) => {
                    self.discarded.push(address);
                    continue;
                }
                Action::PersistDevice(_) => continue,
            };
            self.state.commands.borrow_mut().push_back(command);
        }
    }

    fn handle(&mut self, event: SlotEvent<u8>) {
        let manager = &mut self.manager;
        let actions: Vec<Action<u8>> = match event {
            SlotEvent::Connected {
                slot,
                attempt,
                device,
            } => coordinator::on_slot_connected(manager, slot, attempt, &device)
                .into_iter()
                .collect(),
            SlotEvent::Disconnected { slot, attempt } => {
                coordinator::on_slot_disconnected(manager, slot, attempt)
                    .into_iter()
                    .collect()
            }
            SlotEvent::LinkLost {
                slot,
                attempt,
                device,
            } => coordinator::on_slot_link_lost(manager, slot, attempt, &device)
                .into_iter()
                .collect(),
            SlotEvent::Error {
                slot,
                attempt,
                tag,
                retrying,
            } => coordinator::on_slot_error(manager, slot, attempt, tag, retrying)
                .into_iter()
                .collect(),
            SlotEvent::Quiesced { .. } => Vec::new(),
        };
        self.execute(actions);
    }

    /// Poll the worker and hand each event it reports to the coordinator
    /// until it waits with nothing new to report.
    pub fn settle<F: Future>(&mut self, worker: &mut Pin<&mut F>) {
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            assert!(worker.as_mut().poll(&mut cx).is_pending());
            let event = self.state.events.borrow_mut().pop_front();
            match event {
                Some(event) => self.handle(event),
                None => return,
            }
        }
    }

    /// Start a background reconnect, as `ble_task` does at power-up.
    pub fn power_up(&mut self) {
        let device = keyboard();
        let attempt = self.manager.reserve_retry(SLOT, &device);
        let command = SlotCommand::Reconnect { device, attempt };
        self.state.commands.borrow_mut().push_back(command);
    }

    /// The user selects the keyboard from a scan.
    pub fn select(&mut self) {
        let actions = coordinator::plan_connect(&mut self.manager, &[keyboard()], 0, |a, b| {
            a == b || (*a, *b) == (KEYBOARD, KEYBOARD_NOW)
        });
        self.execute(actions);
    }

    pub fn link(&self, steps: impl IntoIterator<Item = Step>) {
        self.state.link.borrow_mut().extend(steps);
    }

    pub fn attempts(&self) -> Vec<(u8, bool)> {
        let calls = self.state.calls.borrow();
        let attempts = calls.iter().filter_map(|call| match call {
            Call::Attempt {
                address, pairing, ..
            } => Some((*address, *pairing)),
            Call::Note(_) => None,
        });
        attempts.collect()
    }
}
