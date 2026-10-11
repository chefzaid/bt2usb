//! One connection slot's worker: serve the coordinator's commands and
//! reconnect in the background.
//!
//! [`connection_slot_task`] runs once per link. It takes [`SlotCommand`]s from
//! the coordinator in [`multi_conn`](crate::ble::multi_conn), reports
//! [`SlotEvent`]s back, and between commands keeps retrying a saved device
//! whose link dropped or that has not been seen since power-up, through the
//! shared reconnect scan in [`scanner`]. A silent retry never pairs, so it
//! runs only while the bonder holds the device's keys.
//!
//! Every event carries the number of the attempt it reports on (see
//! [`ConnManager`](crate::ble::coordinator::ConnManager)). A `Connect` for the
//! device a background retry is after takes the retry over: a link or an
//! attempt already under way is kept and reported under the new number, and
//! if the user's connection fails while the device's keys remain, the worker
//! reports the error and goes back to the retry. Each attempt itself (connect,
//! secure, discover, run) is in [`slot_link`](crate::ble::slot_link).

use crate::ble::bonder::bonder;
use crate::ble::multi_conn::{SlotCommand, SlotEvent};
use crate::ble::scanner::SavedPeer;
use crate::ble::slot_link::{connect_and_run_secure, ConnectionRequest, SlotOutcome};
use crate::ble::{scanner, BleErrorTag, DiscoveredDevice};
use crate::config;
use crate::diagnostics::{Counter, COUNTERS};
use crate::hid::delivery::HidEvent;
use defmt::info;
use embassy_futures::select::{select, select3, Either, Either3};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Receiver, Sender};
use embassy_time::{Duration, Timer};
use nrf_softdevice::Softdevice;

pub async fn connection_slot_task(
    slot: usize,
    sd: &'static Softdevice,
    cmd_rx: &Receiver<'static, CriticalSectionRawMutex, SlotCommand, 2>,
    slot_event_tx: &Sender<'static, CriticalSectionRawMutex, SlotEvent, 8>,
    report_tx: &Sender<'static, CriticalSectionRawMutex, HidEvent, 16>,
) -> ! {
    let mut pending_cmd: Option<SlotCommand> = None;
    // A paired device this slot keeps trying to reach: set after its link
    // drops (keyboards/mice disconnect when they sleep and advertise again on
    // the next key press) or when a boot-time reconnect hasn't found it yet.
    let mut retry: Option<DiscoveredDevice> = None;
    // The retry target the command handled next replaces, resumed should that
    // command be a user connection that fails while the device's keys remain.
    // Taken on every pass, so it never outlives that command.
    let mut replaced: Option<DiscoveredDevice> = None;
    // The number of the attempt this slot serves, carried on every event.
    let mut attempt = 0;
    // One host-LED receiver per slot (taken once; reused across reconnects). The
    // slot that holds the keyboard writes LED state through; others ignore it.
    let mut led_rx = crate::usb::host_requests::keyboard_led_receiver();

    loop {
        let cmd = match (pending_cmd.take(), retry.take()) {
            (Some(cmd), _) => cmd,
            (None, None) => cmd_rx.receive().await,
            // Between attempts, stay responsive: any command from the
            // coordinator replaces the retry.
            // The other slot's scan may see this slot's device first; it
            // wakes the slot so it connects without waiting out the pause.
            (None, Some(device)) => {
                let backoff = Timer::after(Duration::from_millis(config::BLE_RECONNECT_BACKOFF_MS));
                match select3(cmd_rx.receive(), backoff, scanner::reconnect_sighted(slot)).await {
                    Either3::First(cmd) => {
                        replaced = Some(device);
                        cmd
                    }
                    Either3::Second(()) | Either3::Third(()) => {
                        SlotCommand::Reconnect { device, attempt }
                    }
                }
            }
        };
        let replaced_retry = replaced.take();
        // Only a background retry keeps this slot's reconnect target.
        if !matches!(cmd, SlotCommand::Reconnect { .. }) {
            scanner::clear_reconnect(slot);
        }

        let (mut device, silent) = match cmd {
            SlotCommand::Connect {
                device,
                attempt: next,
            } => {
                attempt = next;
                (device, false)
            }
            SlotCommand::Reconnect {
                device,
                attempt: next,
            } => {
                attempt = next;
                (device, true)
            }
            SlotCommand::Disconnect => {
                // Also ends a retry between attempts: the coordinator still
                // counts this slot as reserved, so tell it the slot is free.
                slot_event_tx
                    .send(SlotEvent::Disconnected { slot, attempt })
                    .await;
                continue;
            }
            SlotCommand::Quiesce(token) => {
                slot_event_tx
                    .send(SlotEvent::Quiesced { slot, token })
                    .await;
                continue;
            }
        };

        if silent {
            // A background reconnect never pairs, so without the device's
            // keys (a newer pairing evicted them, or it was saved without
            // any) it could never secure the link: free the slot instead of
            // holding it for good.
            let Some(identity) = bonder()
                .bond_for_address(device.address)
                .map(|bond| bond.peer_id)
            else {
                info!("slot {} has no keys to reconnect", slot);
                scanner::clear_reconnect(slot);
                slot_event_tx
                    .send(SlotEvent::Disconnected { slot, attempt })
                    .await;
                continue;
            };
            // RPA rotation can happen at any time, not just at boot. A
            // whitelist retry against the previous address never recovers, so
            // find the device's live address first.
            scanner::register_reconnect(
                slot,
                SavedPeer {
                    address: device.address,
                    identity: Some(identity),
                },
            );
            match select(cmd_rx.receive(), scanner::find_saved_peer(sd, slot)).await {
                Either::First(next) => {
                    scanner::clear_reconnect(slot);
                    // Only the `Connect` handled next can resume the retry;
                    // anything left here would reach a later, unrelated one.
                    if matches!(next, SlotCommand::Connect { .. }) {
                        replaced = Some(device);
                    }
                    pending_cmd = superseded(slot, attempt, next, slot_event_tx).await;
                    continue;
                }
                Either::Second(Some(address)) => {
                    COUNTERS.bump(Counter::ReconnectAttempts);
                    device.address = address;
                }
                Either::Second(None) => {
                    retry = Some(device);
                    continue;
                }
            }
        }

        let outcome = connect_and_run_secure(
            sd,
            ConnectionRequest {
                device: &device,
                allow_pairing: !silent,
                attempt: &mut attempt,
            },
            report_tx,
            slot_event_tx,
            slot,
            cmd_rx,
            led_rx.as_mut(),
        )
        .await;
        match outcome {
            SlotOutcome::Closed => COUNTERS.bump(Counter::LinksLost),
            SlotOutcome::Failed(_) if silent => COUNTERS.bump(Counter::ReconnectFailures),
            _ => {}
        }
        match outcome {
            // Without keys for the device (its pairing was refused, the
            // peripheral paired without bonding, or a newer pairing evicted
            // them), a background reconnect, which never pairs, could never
            // secure the link: free the slot instead.
            SlotOutcome::Closed if bonder().bond_for_address(device.address).is_none() => {
                info!("slot {} link lost; no keys to reconnect", slot);
                slot_event_tx
                    .send(SlotEvent::Disconnected { slot, attempt })
                    .await;
            }
            SlotOutcome::Closed => {
                // The peer dropped a working link: keep the slot for it and
                // reconnect as soon as it advertises again.
                info!("slot {} link lost; reconnecting", slot);
                slot_event_tx
                    .send(SlotEvent::LinkLost {
                        slot,
                        attempt,
                        device: device.clone(),
                    })
                    .await;
                retry = Some(device);
            }
            // The user's connection took over this background attempt, which
            // then failed: make that connection now, with pairing allowed,
            // and come back to this retry if it fails too.
            SlotOutcome::TakenOver(next) => {
                replaced = Some(device);
                pending_cmd = Some(SlotCommand::Connect {
                    device: next,
                    attempt,
                });
            }
            // Not found in this attempt's window (or the link couldn't be
            // secured): nothing new to report, try again. Meanwhile the other
            // slot's scans ignore this device for a while, in case it
            // advertises but will not connect.
            SlotOutcome::Failed(BleErrorTag::ConnectFailed) if silent => {
                scanner::reconnect_attempt_failed(slot);
                retry = Some(device);
            }
            // A failed user connection that took over a background retry
            // hands the slot back to that retry while the device's keys
            // remain, so it still reconnects when it next advertises.
            SlotOutcome::Failed(tag) => {
                let resume = replaced_retry
                    .filter(|target| bonder().bond_for_address(target.address).is_some());
                slot_event_tx
                    .send(SlotEvent::Error {
                        slot,
                        attempt,
                        tag,
                        retrying: resume.is_some(),
                    })
                    .await;
                retry = resume;
            }
            SlotOutcome::Superseded(next_cmd) => {
                pending_cmd = superseded(slot, attempt, next_cmd, slot_event_tx).await;
            }
        }
        if retry.is_none() {
            scanner::clear_reconnect(slot);
        }
    }
}

/// A command ended this slot's attempt or background retry; returns the
/// command still to handle. The attempt is reported over. When the command is
/// a `Connect` that took over the retry, the coordinator has already given it
/// a new attempt number and ignores the report. A `Disconnect` needs nothing
/// more, since the attempt is already torn down.
async fn superseded(
    slot: usize,
    attempt: u32,
    next: SlotCommand,
    slot_event_tx: &Sender<'_, CriticalSectionRawMutex, SlotEvent, 8>,
) -> Option<SlotCommand> {
    slot_event_tx
        .send(SlotEvent::Disconnected { slot, attempt })
        .await;
    (!matches!(next, SlotCommand::Disconnect)).then_some(next)
}
