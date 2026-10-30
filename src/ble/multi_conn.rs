//! Multi-device BLE connection manager on the radio: the firmware's
//! [`CoordinatorIo`] for the loop in [`coordinator_loop`].
//!
//! Supports up to two concurrent BLE HID peripheral links (typical:
//! keyboard + mouse) with secure pairing and bonding. [`ble_task`] runs once.
//! The loop it runs decides what to do with each UI command and slot event
//! and is host-tested against fakes; this file connects it to the UI and slot
//! channels, the paired-device store in flash, the security handler's keys,
//! and user scans in [`scanner`]. The slot workers live in
//! [`slot_worker`](crate::ble::slot_worker) and the security handler in
//! [`bonder`](crate::ble::bonder).

use crate::ble::bonder::{bonder, key_matches};
use crate::ble::coordinator::MAX_CONNECTIONS;
use crate::ble::coordinator_loop::{self, CoordinatorIo, Input};
use crate::ble::management;
use crate::ble::slot_loop;
use crate::ble::{scanner, BleCommand, BleErrorTag, BleEvent, DiscoveredDevice};
use crate::config::{BLE_MAX_DISCOVERED, MAX_PAIRED_DEVICES};
use crate::storage::{PairedDevice, Stored, DEVICE_STORE};
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Receiver, Sender};
use heapless::Vec;
use nrf_softdevice::ble::Address;
use nrf_softdevice::{Flash, Softdevice};

/// Command senders to the slot workers, one per link, indexed by slot. An
/// array of [`MAX_CONNECTIONS`] makes `main.rs` build one channel per link.
pub type SlotSenders = [Sender<'static, CriticalSectionRawMutex, SlotCommand, 2>; MAX_CONNECTIONS];

/// A coordinator command to one slot worker, over the SoftDevice address
/// ([`slot_loop::SlotCommand`]).
pub type SlotCommand = slot_loop::SlotCommand<Address>;

/// A slot worker's report, over the SoftDevice address
/// ([`slot_loop::SlotEvent`]).
pub type SlotEvent = slot_loop::SlotEvent<Address>;

/// The channels, flash, and radio the coordinator loop runs on.
struct Shell<'a> {
    sd: &'static Softdevice,
    flash: Flash,
    cmd_rx: &'a Receiver<'static, CriticalSectionRawMutex, BleCommand, 4>,
    event_tx: &'a Sender<'static, CriticalSectionRawMutex, BleEvent, 8>,
    slot_txs: &'a SlotSenders,
    slot_event_rx: &'a Receiver<'static, CriticalSectionRawMutex, SlotEvent, 8>,
}

fn device_info(paired: PairedDevice) -> DiscoveredDevice {
    DiscoveredDevice {
        address: paired.address,
        name: paired.name,
        rssi: paired.last_rssi,
    }
}

impl CoordinatorIo for Shell<'_> {
    type Address = Address;
    type Saved = PairedDevice;

    async fn load(&mut self) -> bool {
        let mut store = DEVICE_STORE.lock().await;
        store.load_from_flash(&mut self.flash).await;
        bonder().load_bonds(&store.bonds());
        store.is_writable()
    }

    async fn reconnect_targets(&mut self) -> Vec<DiscoveredDevice, MAX_CONNECTIONS> {
        let store = DEVICE_STORE.lock().await;
        store
            .iter_recent()
            // A background reconnect never pairs, so a device saved without
            // keys (it paired without bonding) can only connect by being
            // selected again.
            .filter(|paired| paired.bond.is_some())
            .take(MAX_CONNECTIONS)
            .map(device_info)
            .collect()
    }

    async fn next(&mut self) -> Input<Address> {
        match select(self.cmd_rx.receive(), self.slot_event_rx.receive()).await {
            Either::First(command) => Input::Command(command),
            Either::Second(event) => Input::Slot(event),
        }
    }

    async fn slot_event(&mut self) -> SlotEvent {
        self.slot_event_rx.receive().await
    }

    async fn emit(&mut self, event: BleEvent) {
        self.event_tx.send(event).await;
    }

    async fn command(&mut self, slot: usize, command: SlotCommand) {
        if let Some(tx) = self.slot_txs.get(slot) {
            tx.send(command).await;
        }
    }

    async fn scan(&mut self) -> Option<Vec<DiscoveredDevice, BLE_MAX_DISCOVERED>> {
        let result = scanner::scan(self.sd, self.event_tx).await;
        result.ok().map(|scan| scan.devices)
    }

    fn same_peer(&self, held: &Address, seen: &Address) -> bool {
        bonder().same_peer(*held, *seen)
    }

    async fn paired(&mut self) -> Vec<DiscoveredDevice, MAX_PAIRED_DEVICES> {
        let store = DEVICE_STORE.lock().await;
        // `collect` into a heapless `Vec` panics past its capacity; `take`
        // keeps the list within it whatever the store holds.
        store
            .iter_recent()
            .take(MAX_PAIRED_DEVICES)
            .map(device_info)
            .collect()
    }

    async fn find(&mut self, identity: Address) -> Option<PairedDevice> {
        DEVICE_STORE.lock().await.find(identity)
    }

    fn holds(&self, saved: &PairedDevice, address: &Address) -> bool {
        *address == saved.address
            || saved
                .bond
                .is_some_and(|bond| key_matches(&bond.peer_id, *address))
    }

    async fn forget(&mut self, saved: Option<PairedDevice>) -> bool {
        let result = {
            let mut store = DEVICE_STORE.lock().await;
            match saved.as_ref() {
                Some(peer) => store.forget(peer.address, &mut self.flash).await,
                None => store.factory_reset(&mut self.flash).await,
            }
        };
        if result.is_ok() {
            match saved {
                Some(peer) => bonder().forget(peer.address),
                None => bonder().clear(),
            }
        }
        result.is_ok()
    }

    async fn persist(&mut self, device: DiscoveredDevice) {
        // `Bonder::on_bonded` refused the pairing's bond and logged why. The
        // device is not stored either: without keys it would sit at the
        // private address it connected from, and each pairing after that
        // address rotates would add a record, evicting a bonded peer once the
        // store is full.
        if bonder().take_refused(device.address) {
            self.emit(BleEvent::Error(BleErrorTag::BondRefused)).await;
            return;
        }
        let mut store = DEVICE_STORE.lock().await;
        let mut paired = PairedDevice::new(device.address, device.name.as_str(), device.rssi);
        let bond = bonder().bond_for_address(device.address);
        paired.bond = bond;
        let stored = match store.add(paired) {
            // RAM now holds what the store does: the new keys are saved, and
            // the keys of a device evicted to make room are gone.
            Ok(stored) => {
                if let Some(bond) = bond {
                    bonder().mark_saved(&bond);
                }
                if let Stored::RemovedOldest(Some(identity)) = stored {
                    bonder().forget_identity(identity);
                }
                Some(stored)
            }
            // The store refuses what `Bonder` should already have refused;
            // drop exactly those keys so RAM holds what flash will.
            Err(_) => {
                if let Some(bond) = bond {
                    bonder().forget_bond(&bond);
                }
                self.event_tx
                    .send(BleEvent::Error(BleErrorTag::BondRefused))
                    .await;
                None
            }
        };
        let saved = store.save_to_flash(&mut self.flash).await.is_ok();
        if let Some(event) = management::persist_report(stored.as_ref(), saved) {
            self.event_tx.send(event).await;
        }
    }

    fn discard_unsaved_bond(&mut self, address: Address) {
        bonder().discard_unsaved(address);
    }
}

pub async fn ble_task(
    sd: &'static Softdevice,
    cmd_rx: &Receiver<'static, CriticalSectionRawMutex, BleCommand, 4>,
    event_tx: &Sender<'static, CriticalSectionRawMutex, BleEvent, 8>,
    slot_txs: &SlotSenders,
    slot_event_rx: &Receiver<'static, CriticalSectionRawMutex, SlotEvent, 8>,
) -> ! {
    let mut shell = Shell {
        sd,
        // `Flash::take` panics on a second call; `main` spawns this task once
        // and it never returns.
        flash: Flash::take(sd),
        cmd_rx,
        event_tx,
        slot_txs,
        slot_event_rx,
    };
    coordinator_loop::run(&mut shell).await
}
