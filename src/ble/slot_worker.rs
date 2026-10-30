//! One connection slot's worker on the radio: the firmware's [`SlotIo`] for
//! the loop in [`slot_loop`].
//!
//! [`connection_slot_task`] runs once per link. The loop it runs decides what
//! to do with each command and attempt and is host-tested against fakes;
//! this file connects it to the coordinator's channels, the bonder's keys,
//! the shared reconnect scan in [`scanner`], and each attempt (connect,
//! secure, discover, run) in [`slot_link`](crate::ble::slot_link).

use crate::ble::bonder::bonder;
use crate::ble::multi_conn::{SlotCommand, SlotEvent};
use crate::ble::scanner::SavedPeer;
use crate::ble::slot_link::{connect_and_run_secure, ConnectionRequest, SlotOutcome};
use crate::ble::slot_loop::{self, Note, SlotIo};
use crate::ble::{scanner, DiscoveredDevice};
use crate::config;
use crate::hid::delivery::HidEvent;
use crate::usb::host_requests::LedReceiver;
use defmt::info;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Receiver, Sender};
use embassy_time::{Duration, Timer};
use nrf_softdevice::ble::{Address, IdentityKey};
use nrf_softdevice::Softdevice;

/// The radio, channels, and keys one slot's worker loop runs on.
struct Radio<'a> {
    slot: usize,
    sd: &'static Softdevice,
    cmd_rx: &'a Receiver<'static, CriticalSectionRawMutex, SlotCommand, 2>,
    slot_event_tx: &'a Sender<'static, CriticalSectionRawMutex, SlotEvent, 8>,
    report_tx: &'a Sender<'static, CriticalSectionRawMutex, HidEvent, 16>,
    /// One host-LED receiver per slot, taken once and reused across
    /// reconnects. The slot that holds the keyboard writes LED state through;
    /// others ignore it.
    led_rx: Option<LedReceiver>,
}

impl SlotIo for Radio<'_> {
    type Address = Address;
    type Identity = IdentityKey;

    async fn command(&self) -> SlotCommand {
        self.cmd_rx.receive().await
    }

    async fn report(&self, event: SlotEvent) {
        self.slot_event_tx.send(event).await;
    }

    async fn backoff(&self) {
        Timer::after(Duration::from_millis(config::BLE_RECONNECT_BACKOFF_MS)).await;
    }

    fn identity(&self, address: Address) -> Option<IdentityKey> {
        bonder().bond_for_address(address).map(|bond| bond.peer_id)
    }

    fn register_reconnect(&self, peer: SavedPeer) {
        scanner::register_reconnect(self.slot, peer);
    }

    fn clear_reconnect(&self) {
        scanner::clear_reconnect(self.slot);
    }

    fn reconnect_attempt_failed(&self) {
        scanner::reconnect_attempt_failed(self.slot);
    }

    async fn reconnect_sighted(&self) {
        scanner::reconnect_sighted(self.slot).await;
    }

    async fn find_saved_peer(&self) -> Option<Address> {
        scanner::find_saved_peer(self.sd, self.slot).await
    }

    async fn attempt(
        &mut self,
        device: &DiscoveredDevice,
        allow_pairing: bool,
        attempt: &mut u32,
    ) -> SlotOutcome {
        connect_and_run_secure(
            self.sd,
            ConnectionRequest {
                device,
                allow_pairing,
                attempt,
            },
            self.report_tx,
            self.slot_event_tx,
            self.slot,
            self.cmd_rx,
            self.led_rx.as_mut(),
        )
        .await
    }

    fn note(&self, note: Note) {
        let slot = self.slot;
        match note {
            Note::NoKeysToReconnect => info!("slot {} has no keys to reconnect", slot),
            Note::LinkLostReconnecting => info!("slot {} link lost; reconnecting", slot),
            Note::LinkLostWithoutKeys => info!("slot {} link lost; no keys to reconnect", slot),
        }
    }
}

pub async fn connection_slot_task(
    slot: usize,
    sd: &'static Softdevice,
    cmd_rx: &Receiver<'static, CriticalSectionRawMutex, SlotCommand, 2>,
    slot_event_tx: &Sender<'static, CriticalSectionRawMutex, SlotEvent, 8>,
    report_tx: &Sender<'static, CriticalSectionRawMutex, HidEvent, 16>,
) -> ! {
    let mut radio = Radio {
        slot,
        sd,
        cmd_rx,
        slot_event_tx,
        report_tx,
        led_rx: crate::usb::host_requests::keyboard_led_receiver(),
    };
    slot_loop::run(&mut radio, slot).await
}
