//! The BLE coordinator task's loop: run the coordinator's decisions for each
//! UI command and slot-worker event, generic over the store, the radio, and
//! the channels so host tests run it against fakes.
//!
//! [`run`] loads the saved devices, starts the power-up reconnects, then
//! handles UI [`Command`]s before queued [`SlotEvent`]s, asking the pure
//! reducers in [`coordinator`] what to do and performing their [`Action`]s.
//! A Forget or factory reset is a transaction: the slots holding an affected
//! device are quiesced behind a [`Quiescence`] barrier, their late reports
//! are dropped, and the store changes only once every one has acknowledged,
//! so no queued `Connected` can save a device again after it was removed.
//! UI commands wait meanwhile. The firmware's [`CoordinatorIo`], in
//! `multi_conn.rs`, runs the store in flash, the security handler's keys,
//! and user scans.

use core::future::Future;

use heapless::Vec;

use super::coordinator::{self, Action, ConnManager, DeviceInfo, ErrorTag, MAX_CONNECTIONS};
use super::management::{self, Quiescence};
use super::messages::{Command, Event};
use super::slot_loop::{SlotCommand, SlotEvent};
use crate::config::{BLE_MAX_DISCOVERED, MAX_PAIRED_DEVICES};

/// What the coordinator handles next.
#[derive(Clone, Debug, PartialEq)]
pub enum Input<A> {
    Command(Command<A>),
    Slot(SlotEvent<A>),
}

/// What the coordinator needs from the firmware: the UI and slot channels,
/// the paired-device store with the keys it holds, and user scans.
pub trait CoordinatorIo {
    type Address: Copy + PartialEq;
    /// A saved device, as a Forget finds and removes it.
    type Saved;

    /// Load the saved devices and their keys; `false` when the store cannot
    /// be written.
    fn load(&mut self) -> impl Future<Output = bool>;
    /// The saved devices to reconnect at power-up, newest first, at most one
    /// per slot: only those with keys, since a background reconnect never
    /// pairs.
    fn reconnect_targets(
        &mut self,
    ) -> impl Future<Output = Vec<DeviceInfo<Self::Address>, MAX_CONNECTIONS>>;
    /// The next UI command or slot event; a command first when both wait.
    fn next(&mut self) -> impl Future<Output = Input<Self::Address>>;
    /// The next slot event; UI commands stay queued.
    fn slot_event(&mut self) -> impl Future<Output = SlotEvent<Self::Address>>;
    /// Report to the UI; waits while its channel is full.
    fn emit(&mut self, event: Event<Self::Address>) -> impl Future<Output = ()>;
    /// Send slot `slot`'s worker a command; waits while its channel is full.
    fn command(
        &mut self,
        slot: usize,
        command: SlotCommand<Self::Address>,
    ) -> impl Future<Output = ()>;
    /// A user scan, which reports what it hears to the UI itself; `None`
    /// when it failed.
    fn scan(
        &mut self,
    ) -> impl Future<Output = Option<Vec<DeviceInfo<Self::Address>, BLE_MAX_DISCOVERED>>>;
    /// Whether two addresses belong to one peripheral: equal, or resolved by
    /// one bonded peer's identity key.
    fn same_peer(&self, held: &Self::Address, seen: &Self::Address) -> bool;
    /// The saved devices, newest first.
    fn paired(
        &mut self,
    ) -> impl Future<Output = Vec<DeviceInfo<Self::Address>, MAX_PAIRED_DEVICES>>;
    /// The saved device with this identity address.
    fn find(&mut self, identity: Self::Address) -> impl Future<Output = Option<Self::Saved>>;
    /// Whether `address`, a slot's live or reserved address, is `saved`'s.
    fn holds(&self, saved: &Self::Saved, address: &Self::Address) -> bool;
    /// Remove `saved`, or every saved device for `None`, from flash, then
    /// their keys; `false`, keeping the keys, when the write failed.
    fn forget(&mut self, saved: Option<Self::Saved>) -> impl Future<Output = bool>;
    /// Save a device whose link came up, with the keys of its pairing, and
    /// tell the UI what that did ([`Action::PersistDevice`]).
    fn persist(&mut self, device: DeviceInfo<Self::Address>) -> impl Future<Output = ()>;
    /// [`Action::DiscardUnsavedBond`].
    fn discard_unsaved_bond(&mut self, address: Self::Address);
}

/// Run the coordinator forever.
pub async fn run<Io: CoordinatorIo>(io: &mut Io) -> ! {
    if !io.load().await {
        io.emit(Event::Error(ErrorTag::StorageFailed)).await;
    }
    let mut manager = ConnManager::new();
    // The last user scan's results, which `Command::Connect` indexes.
    let mut scan: Vec<DeviceInfo<Io::Address>, BLE_MAX_DISCOVERED> = Vec::new();
    let mut token = 0u32;

    // Auto-reconnect the most recently added devices (up to the number of
    // connection slots) so a keyboard + mouse pair both come back after a
    // reboot without manual re-selection. Start at once, without a full scan
    // first: each slot's reconnect scan resolves a rotating private address
    // itself, and one slot's scan also finds the other slot's device, so the
    // keyboard can type as soon as it advertises.
    for (slot, device) in io.reconnect_targets().await.into_iter().enumerate() {
        let attempt = manager.reserve_retry(slot, &device);
        // A paired device that's asleep or off right now will advertise
        // once it wakes, so keep trying rather than failing once.
        io.command(slot, SlotCommand::Reconnect { device, attempt })
            .await;
    }

    loop {
        let command = match io.next().await {
            Input::Command(command) => command,
            Input::Slot(event) => {
                handle_slot_event(io, &mut manager, event).await;
                continue;
            }
        };
        match command {
            Command::StartScan => {
                for action in coordinator::plan_start_scan(&manager) {
                    execute(io, action).await;
                }
                scan = io.scan().await.unwrap_or_default();
            }
            Command::Connect(index) => {
                let same_peer = |held: &Io::Address, seen: &Io::Address| io.same_peer(held, seen);
                let actions = coordinator::plan_connect(&mut manager, &scan, index, same_peer);
                for action in actions {
                    execute(io, action).await;
                }
            }
            Command::Disconnect => {
                for action in coordinator::plan_disconnect(&manager) {
                    execute(io, action).await;
                }
            }
            Command::ListPaired { id } => {
                let devices = io.paired().await;
                io.emit(Event::PairedDevices { id, devices }).await;
            }
            Command::Forget { id, address } => {
                token = token.wrapping_add(1);
                let result = manage(io, &mut manager, Some(address), token).await;
                io.emit(Event::ManagementResult { id, result }).await;
            }
            Command::FactoryReset { id } => {
                token = token.wrapping_add(1);
                let result = manage(io, &mut manager, None, token).await;
                // Explicit reset also invalidates the old discovery snapshot.
                scan.clear();
                io.emit(Event::ManagementResult { id, result }).await;
            }
        }
    }
}

async fn handle_slot_event<Io: CoordinatorIo>(
    io: &mut Io,
    manager: &mut ConnManager<Io::Address>,
    event: SlotEvent<Io::Address>,
) {
    let actions: Vec<Action<Io::Address>, 3> = match event {
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
        } => coordinator::on_slot_error(manager, slot, attempt, tag, retrying),
        // Only a management transaction waits for these.
        SlotEvent::Quiesced { .. } => return,
    };
    for action in actions {
        execute(io, action).await;
    }
}

/// Forget one saved device (`Some` identity address) or all of them (`None`).
/// Stop the workers holding them before the store changes, and drop their
/// queued Connected/LinkLost events until a command-specific barrier confirms
/// that the BLE link, USB source state and retry target have all been
/// released. Then report the links still up.
async fn manage<Io: CoordinatorIo>(
    io: &mut Io,
    manager: &mut ConnManager<Io::Address>,
    identity: Option<Io::Address>,
    token: u32,
) -> Result<(), ErrorTag> {
    let saved = match identity {
        Some(identity) => match io.find(identity).await {
            Some(saved) => Some(saved),
            None => return Err(ErrorTag::ManagementFailed),
        },
        None => None,
    };
    let targets = match &saved {
        None => [true; MAX_CONNECTIONS],
        Some(saved) => management::forget_targets(manager, |current| io.holds(saved, current)),
    };
    for (slot, _) in targets.iter().enumerate().filter(|(_, &target)| target) {
        io.command(slot, SlotCommand::Quiesce(token)).await;
    }
    let mut barrier = Quiescence::new(targets, token);
    while !barrier.complete() {
        let event = io.slot_event().await;
        let slot = match &event {
            SlotEvent::Quiesced { slot, token } => {
                if barrier.acknowledge(*slot, *token) {
                    for action in coordinator::on_slot_quiesced(manager, *slot) {
                        execute(io, action).await;
                    }
                }
                continue;
            }
            SlotEvent::Connected { slot, .. }
            | SlotEvent::Disconnected { slot, .. }
            | SlotEvent::LinkLost { slot, .. }
            | SlotEvent::Error { slot, .. } => *slot,
        };
        if !barrier.suppresses(slot) {
            handle_slot_event(io, manager, event).await;
        }
    }
    // No targeted worker can reconnect, emit a stale connection save or invoke
    // a late security callback after its quiescence acknowledgement.
    let written = io.forget(saved).await;
    io.emit(coordinator::link_state(manager).into()).await;
    if written {
        Ok(())
    } else {
        Err(ErrorTag::StorageFailed)
    }
}

/// Perform one coordinator [`Action`]: drive a slot worker, save a device,
/// drop unsaved keys, or report to the UI.
async fn execute<Io: CoordinatorIo>(io: &mut Io, action: Action<Io::Address>) {
    match action {
        Action::DisconnectSlot(slot) => io.command(slot, SlotCommand::Disconnect).await,
        Action::ConnectSlot {
            slot,
            device,
            attempt,
        } => {
            io.command(slot, SlotCommand::Connect { device, attempt })
                .await;
        }
        Action::PersistDevice(device) => io.persist(device).await,
        Action::DiscardUnsavedBond(address) => io.discard_unsaved_bond(address),
        Action::Emit(event) => io.emit(event.into()).await,
    }
}

#[cfg(test)]
#[path = "coordinator_loop_bench.rs"]
mod bench;

#[cfg(test)]
#[path = "coordinator_loop_tests.rs"]
mod tests;
