//! Multi-device BLE connection manager.
//!
//! Supports up to two concurrent BLE HID peripheral links (typical:
//! keyboard + mouse) with secure pairing and bonding. [`ble_task`] runs the
//! pure coordinator, executes its [`Action`]s, owns the paired-device store,
//! runs user scans and saved-device management, and starts the power-up
//! reconnects; the slot workers live in
//! [`slot_worker`](crate::ble::slot_worker) and the security handler in
//! [`bonder`](crate::ble::bonder).

use crate::ble::bonder::bonder;
use crate::ble::coordinator::{self, Action, ConnManager, UiEvent, MAX_CONNECTIONS};
use crate::ble::management::Quiescence;
use crate::ble::scanner::ScanResult;
use crate::ble::{scanner, BleCommand, BleErrorTag, BleEvent, DiscoveredDevice};
use crate::storage::{PairedDevice, DEVICE_STORE};
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Receiver, Sender};
use heapless::Vec;
use nrf_softdevice::ble::Address;
use nrf_softdevice::Softdevice;

/// Command senders to the slot workers, one per link, indexed by slot. An
/// array of [`MAX_CONNECTIONS`] makes `main.rs` build one channel per link.
pub type SlotSenders = [Sender<'static, CriticalSectionRawMutex, SlotCommand, 2>; MAX_CONNECTIONS];

/// The connection-slot state machine, specialised to the SoftDevice address
/// type. The logic lives in (and is host-tested via)
/// [`crate::ble::coordinator`]; here it is just instantiated.
type MultiConnectionManager = ConnManager<Address>;

#[derive(Clone)]
pub enum SlotCommand {
    /// Connect to a device the user just picked; a failure is reported.
    Connect(DiscoveredDevice),
    /// Keep silently retrying a paired device until it connects (boot-time
    /// auto-reconnect). Ends only on success or a new command.
    Reconnect(DiscoveredDevice),
    Disconnect,
    /// Acknowledge only after the link and automatic retry target are gone.
    Quiesce(u32),
}

#[derive(Clone)]
pub enum SlotEvent {
    Quiesced {
        slot: usize,
        token: u32,
    },
    Connected {
        slot: usize,
        device: DiscoveredDevice,
    },
    Disconnected {
        slot: usize,
    },
    /// An established link dropped; the slot is now silently reconnecting.
    LinkLost {
        slot: usize,
        device: DiscoveredDevice,
    },
    Error {
        slot: usize,
        tag: BleErrorTag,
    },
}

pub async fn ble_task(
    sd: &'static Softdevice,
    cmd_rx: &Receiver<'static, CriticalSectionRawMutex, BleCommand, 4>,
    event_tx: &Sender<'static, CriticalSectionRawMutex, BleEvent, 8>,
    slot_txs: &SlotSenders,
    slot_event_rx: &Receiver<'static, CriticalSectionRawMutex, SlotEvent, 8>,
) -> ! {
    let mut flash = nrf_softdevice::Flash::take(sd);
    {
        let mut store = DEVICE_STORE.lock().await;
        store.load_from_flash(&mut flash).await;
        bonder().load_bonds(&store.bonds());
        if !store.is_writable() {
            event_tx
                .send(BleEvent::Error(BleErrorTag::StorageFailed))
                .await;
        }
    }

    let mut manager = MultiConnectionManager::new();
    let mut last_scan: Option<ScanResult> = None;
    let mut management_token = 0u32;

    // Auto-reconnect the most recently added devices (up to the number of
    // connection slots) so a keyboard + mouse pair both come back after a
    // reboot without manual re-selection. Start at once, without a full scan
    // first: each slot's reconnect scan resolves a rotating private address
    // itself, and one slot's scan also finds the other slot's device, so the
    // keyboard can type as soon as it advertises.
    let peers: Vec<DiscoveredDevice, MAX_CONNECTIONS> = {
        let store = DEVICE_STORE.lock().await;
        store
            .iter_recent()
            .take(MAX_CONNECTIONS)
            .map(|paired| DiscoveredDevice {
                address: paired.address,
                name: paired.name.clone(),
                rssi: paired.last_rssi,
            })
            .collect()
    };
    for (slot, device) in peers.into_iter().enumerate() {
        manager.reserve_slot(slot, &device);
        // A paired device that's asleep or off right now will advertise
        // once it wakes, so keep trying rather than failing once.
        send_slot_cmd(slot, SlotCommand::Reconnect(device), slot_txs).await;
    }

    // The coordinator below is a thin interpreter: it asks the pure
    // `coordinator` reducers (host-tested) what to do for each command/event,
    // then performs the resulting I/O via `execute_action`.
    loop {
        match select(cmd_rx.receive(), slot_event_rx.receive()).await {
            Either::First(cmd) => match cmd {
                BleCommand::StartScan => {
                    for action in coordinator::plan_start_scan(&manager) {
                        execute_action(action, event_tx, slot_txs, &mut flash).await;
                    }
                    match scanner::scan(sd, event_tx).await {
                        Ok(result) => last_scan = Some(result),
                        Err(_) => last_scan = None,
                    }
                }
                BleCommand::Connect(index) => {
                    let devices: &[DiscoveredDevice] = match &last_scan {
                        Some(scan) => scan.devices.as_slice(),
                        None => &[],
                    };
                    for action in coordinator::plan_connect(&mut manager, devices, index) {
                        execute_action(action, event_tx, slot_txs, &mut flash).await;
                    }
                }
                BleCommand::Disconnect => {
                    for action in coordinator::plan_disconnect(&manager) {
                        execute_action(action, event_tx, slot_txs, &mut flash).await;
                    }
                }
                BleCommand::ListPaired { id } => publish_paired_devices(id, event_tx).await,
                BleCommand::Forget { id, address } => {
                    management_token = management_token.wrapping_add(1);
                    let result = manage_devices(
                        Some(address),
                        management_token,
                        &mut manager,
                        event_tx,
                        slot_txs,
                        slot_event_rx,
                        &mut flash,
                    )
                    .await;
                    event_tx
                        .send(BleEvent::ManagementResult { id, result })
                        .await;
                }
                BleCommand::FactoryReset { id } => {
                    management_token = management_token.wrapping_add(1);
                    let result = manage_devices(
                        None,
                        management_token,
                        &mut manager,
                        event_tx,
                        slot_txs,
                        slot_event_rx,
                        &mut flash,
                    )
                    .await;
                    // Explicit reset also invalidates the old discovery snapshot.
                    last_scan = None;
                    event_tx
                        .send(BleEvent::ManagementResult { id, result })
                        .await;
                }
            },
            Either::Second(event) => {
                handle_slot_event(event, &mut manager, event_tx, slot_txs, &mut flash).await;
            }
        }
    }
}

async fn handle_slot_event(
    event: SlotEvent,
    manager: &mut MultiConnectionManager,
    event_tx: &Sender<'static, CriticalSectionRawMutex, BleEvent, 8>,
    slot_txs: &SlotSenders,
    flash: &mut nrf_softdevice::Flash,
) {
    let actions: Vec<Action<Address>, 2> = match event {
        SlotEvent::Connected { slot, device } => {
            coordinator::on_slot_connected(manager, slot, &device)
        }
        SlotEvent::Disconnected { slot } => coordinator::on_slot_disconnected(manager, slot)
            .into_iter()
            .collect(),
        SlotEvent::LinkLost { slot, device } => {
            coordinator::on_slot_link_lost(manager, slot, &device)
                .into_iter()
                .collect()
        }
        SlotEvent::Error { slot, tag } => coordinator::on_slot_error(manager, slot, tag),
        SlotEvent::Quiesced { .. } => return,
    };
    for action in actions {
        execute_action(action, event_tx, slot_txs, flash).await;
    }
}

async fn publish_paired_devices(
    id: u32,
    event_tx: &Sender<'static, CriticalSectionRawMutex, BleEvent, 8>,
) {
    let devices = {
        let store = DEVICE_STORE.lock().await;
        store
            .iter_recent()
            .map(|paired| DiscoveredDevice {
                address: paired.address,
                name: paired.name.clone(),
                rssi: paired.last_rssi,
            })
            .collect()
    };
    event_tx.send(BleEvent::PairedDevices { id, devices }).await;
}

/// Stop workers before changing persistent identities. Suppress their queued
/// Connected/LinkLost events until a command-specific barrier confirms that the
/// BLE link, USB source state and retry target have all been released.
async fn manage_devices(
    address: Option<Address>,
    token: u32,
    manager: &mut MultiConnectionManager,
    event_tx: &Sender<'static, CriticalSectionRawMutex, BleEvent, 8>,
    slot_txs: &SlotSenders,
    slot_event_rx: &Receiver<'static, CriticalSectionRawMutex, SlotEvent, 8>,
    flash: &mut nrf_softdevice::Flash,
) -> Result<(), BleErrorTag> {
    let paired = if let Some(identity) = address {
        let store = DEVICE_STORE.lock().await;
        let Some(paired) = store.find(identity).cloned() else {
            return Err(BleErrorTag::ManagementFailed);
        };
        Some(paired)
    } else {
        None
    };
    let mut targets = [false; MAX_CONNECTIONS];
    for (slot, target) in targets.iter_mut().enumerate() {
        *target = match &paired {
            None => true,
            Some(peer) => manager.slot_address(slot).is_some_and(|current| {
                *current == peer.address
                    || peer
                        .bond
                        .is_some_and(|bond| bond.peer_id.is_match(*current))
            }),
        };
        if *target {
            send_slot_cmd(slot, SlotCommand::Quiesce(token), slot_txs).await;
        }
    }
    let mut barrier = Quiescence::new(targets, token);
    while !barrier.complete() {
        let event = slot_event_rx.receive().await;
        if let SlotEvent::Quiesced { slot, token } = event {
            if barrier.acknowledge(slot, token) {
                manager.disconnect_slot(slot);
            }
            continue;
        }
        let slot = match &event {
            SlotEvent::Connected { slot, .. }
            | SlotEvent::Disconnected { slot }
            | SlotEvent::LinkLost { slot, .. }
            | SlotEvent::Error { slot, .. } => *slot,
            SlotEvent::Quiesced { .. } => unreachable!(),
        };
        if !barrier.suppresses(slot) {
            handle_slot_event(event, manager, event_tx, slot_txs, flash).await;
        }
    }
    // No targeted worker can reconnect, emit a stale connection save or invoke
    // a late security callback after its quiescence acknowledgement.
    let result = {
        let mut store = DEVICE_STORE.lock().await;
        match paired.as_ref() {
            Some(peer) => store.forget(peer.address, flash).await,
            None => store.factory_reset(flash).await,
        }
    };
    if result.is_ok() {
        match paired {
            Some(peer) => bonder().forget(peer.address),
            None => bonder().clear(),
        }
    }
    let state = if manager.active_count() == 0 {
        BleEvent::Disconnected
    } else {
        BleEvent::Connected(coordinator::connection_summary(manager))
    };
    event_tx.send(state).await;
    result.map_err(|_| BleErrorTag::StorageFailed)
}

/// Perform the I/O for one coordinator [`Action`]: drive slot workers, persist
/// to flash, or emit UI events. This is the only place the pure decisions touch
/// hardware/channels.
async fn execute_action(
    action: Action<Address>,
    event_tx: &Sender<'static, CriticalSectionRawMutex, BleEvent, 8>,
    slot_txs: &SlotSenders,
    flash: &mut nrf_softdevice::Flash,
) {
    match action {
        Action::DisconnectSlot(slot) => {
            send_slot_cmd(slot, SlotCommand::Disconnect, slot_txs).await;
        }
        Action::ConnectSlot { slot, device } => {
            send_slot_cmd(slot, SlotCommand::Connect(device), slot_txs).await;
        }
        Action::PersistDevice(device) => {
            let mut store = DEVICE_STORE.lock().await;
            let mut paired = PairedDevice::new(device.address, device.name.as_str(), device.rssi);
            paired.bond = bonder().bond_for_address(device.address);
            store.add(paired);
            if store.save_to_flash(flash).await.is_err() {
                event_tx
                    .send(BleEvent::Error(BleErrorTag::StorageFailed))
                    .await;
            }
        }
        Action::Emit(ui) => {
            let event = match ui {
                UiEvent::Connected(summary) => BleEvent::Connected(summary),
                UiEvent::Disconnected => BleEvent::Disconnected,
                UiEvent::Error(tag) => BleEvent::Error(tag),
            };
            event_tx.send(event).await;
        }
    }
}

async fn send_slot_cmd(slot: usize, cmd: SlotCommand, slot_txs: &SlotSenders) {
    if let Some(tx) = slot_txs.get(slot) {
        tx.send(cmd).await;
    }
}
