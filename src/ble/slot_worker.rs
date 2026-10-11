//! One connection slot's worker: connect, secure, run the HID client, and
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
//! reports the error and goes back to the retry.

use crate::ble::bonder::bonder;
use crate::ble::multi_conn::{SlotCommand, SlotEvent};
use crate::ble::scanner::SavedPeer;
use crate::ble::{hid_client, scanner, BleErrorTag, DiscoveredDevice};
use crate::config;
use crate::hid::delivery::HidEvent;
use crate::hid::report_protocol::HidDescriptor;
use core::pin::pin;
use defmt::{info, warn};
use embassy_futures::select::{select, select3, Either, Either3};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Receiver, Sender};
use embassy_time::{Duration, Timer};
use nrf_softdevice::ble::{central, Connection, EncryptError, SecurityMode};
use nrf_softdevice::raw;
use nrf_softdevice::Softdevice;

/// Outcome of a single slot connection attempt + run.
enum SlotOutcome {
    /// The peer closed the link (or it ended normally).
    Closed,
    /// The connection attempt failed before the link was usable.
    Failed(BleErrorTag),
    /// A `Connect` for this attempt's device took over the attempt, which then
    /// failed before the link was usable. The user's connection, already
    /// numbered with the new attempt, is returned to be made next.
    TakenOver(DiscoveredDevice),
    /// A new command arrived and superseded the active link, which has already
    /// been explicitly disconnected. The command is returned to be processed next.
    Superseded(SlotCommand),
}

struct ConnectionRequest<'a> {
    device: &'a DiscoveredDevice,
    allow_pairing: bool,
    /// The number of the attempt being served, which a `Connect` that takes
    /// the attempt over replaces.
    attempt: &'a mut u32,
}

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
                Either::Second(Some(address)) => device.address = address,
                Either::Second(None) => {
                    retry = Some(device);
                    continue;
                }
            }
        }

        match connect_and_run_secure(
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
        .await
        {
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

/// Sort a command that arrived during an attempt to `device`: a `Connect`
/// for the same peer (the same address, or one the same bonded peer's identity
/// key resolves) takes the attempt over and is returned as its device and
/// attempt number; any other command supersedes the attempt and is returned
/// as the error.
fn take_over(
    next: SlotCommand,
    device: &DiscoveredDevice,
) -> Result<(DiscoveredDevice, u32), SlotCommand> {
    match next {
        SlotCommand::Connect {
            device: next,
            attempt,
        } if bonder().same_peer(next.address, device.address) => Ok((next, attempt)),
        other => Err(other),
    }
}

/// Await the GAP disconnect event before publishing quiescence. Merely queuing
/// sd_ble_gap_disconnect does not stop in-flight security callbacks.
async fn close_connection(conn: &Connection) {
    let _ = conn.disconnect();
    while conn.handle().is_some() {
        Timer::after(Duration::from_millis(10)).await;
    }
}

async fn wait_for_secure_link(conn: &Connection) -> bool {
    for _ in 0..25 {
        if conn.handle().is_none() {
            return false;
        }
        match conn.security_mode() {
            SecurityMode::JustWorks | SecurityMode::Mitm | SecurityMode::LescMitm => return true,
            // Signed modes authenticate individual writes but do not encrypt
            // the link, so they cannot protect incoming HID notifications.
            _ => Timer::after(Duration::from_millis(200)).await,
        }
    }
    false
}

/// Secure a new link and discover its HID reports. A background reconnect
/// (`allow_pairing` false) never initiates a replacement pairing; fresh
/// pairing is restricted to an explicit user connection.
async fn secure_and_discover(
    conn: &Connection,
    allow_pairing: bool,
    slot: usize,
) -> Result<(hid_client::HidServiceClient, Option<HidDescriptor>), BleErrorTag> {
    let secure_ok = match conn.encrypt() {
        Ok(()) => wait_for_secure_link(conn).await,
        Err(EncryptError::PeerKeysNotFound) if allow_pairing => {
            if conn.request_pairing().is_ok() {
                wait_for_secure_link(conn).await
            } else {
                false
            }
        }
        Err(_) => false,
    };
    if !secure_ok {
        warn!("slot {} failed to secure BLE link", slot);
        return Err(BleErrorTag::ConnectFailed);
    }
    hid_client::discover_and_subscribe(conn).await
}

async fn connect_and_run_secure(
    sd: &'static Softdevice,
    request: ConnectionRequest<'_>,
    report_tx: &Sender<'_, CriticalSectionRawMutex, HidEvent, 16>,
    slot_event_tx: &Sender<'_, CriticalSectionRawMutex, SlotEvent, 8>,
    slot: usize,
    cmd_rx: &Receiver<'_, CriticalSectionRawMutex, SlotCommand, 2>,
    led_rx: Option<&mut crate::usb::host_requests::LedReceiver>,
) -> SlotOutcome {
    let ConnectionRequest {
        device,
        allow_pairing,
        attempt,
    } = request;
    info!("slot {} connecting to {}", slot, device.name.as_str());

    let whitelist = [&device.address];
    let conn_cfg = central::ConnectConfig {
        scan_config: central::ScanConfig {
            whitelist: Some(&whitelist),
            // Bounded, so an absent peer can't hold the radio (and every other
            // scan/connect) forever. Units of 10 ms.
            timeout: config::BLE_CONNECT_TIMEOUT_SECS * 100,
            // The device was just chosen from a scan or seen by a reconnect
            // scan, so it is advertising now: listen often enough to catch
            // its next advertisement.
            interval: config::BLE_FAST_SCAN_INTERVAL,
            window: config::BLE_FAST_SCAN_WINDOW,
            ..Default::default()
        },
        conn_params: raw::ble_gap_conn_params_t {
            min_conn_interval: config::BLE_CONN_INTERVAL_MIN,
            max_conn_interval: config::BLE_CONN_INTERVAL_MAX,
            slave_latency: config::BLE_SLAVE_LATENCY,
            conn_sup_timeout: config::BLE_SUP_TIMEOUT,
        },
        ..Default::default()
    };

    // `connect_with_security` also performs MTU exchange after the GAP
    // connection is created, so keep ownership of its future until it returns.
    let conn = {
        // One GAP procedure at a time: the other slot or the scanner may be
        // mid-procedure, and a concurrent connect would fail immediately.
        let _gap = crate::ble::GAP_PROCEDURE.lock().await;
        match central::connect_with_security(sd, &conn_cfg, bonder()).await {
            Ok(conn) => conn,
            Err(_) => return SlotOutcome::Failed(BleErrorTag::ConnectFailed),
        }
    };

    // Once a link is owned, disconnect promptly when the user cancels during
    // security or GATT discovery, not only after notifications start flowing.
    // The user selecting this device while a background attempt is under way
    // takes the attempt over instead: it carries on under the new number.
    // `prepare` is pinned inside the block so it is dropped before the link is
    // closed: its GATT waits release their per-handle portals while the handle
    // still belongs to this link, and cannot clear a registration another slot
    // makes once the SoftDevice reuses the handle.
    let mut takeover = None;
    let prepared = {
        let mut prepare = pin!(secure_and_discover(&conn, allow_pairing, slot));
        loop {
            match select(cmd_rx.receive(), prepare.as_mut()).await {
                Either::First(next_cmd) => match take_over(next_cmd, device) {
                    Ok((next, next_attempt)) => {
                        *attempt = next_attempt;
                        takeover = Some(next);
                    }
                    Err(next_cmd) => break Err(next_cmd),
                },
                Either::Second(result) => break Ok(result),
            }
        }
    };
    let (client, descriptor) = match prepared {
        Ok(Ok(value)) => value,
        Ok(Err(tag)) => {
            close_connection(&conn).await;
            return match takeover {
                Some(next) => SlotOutcome::TakenOver(next),
                None => SlotOutcome::Failed(tag),
            };
        }
        Err(next_cmd) => {
            close_connection(&conn).await;
            return SlotOutcome::Superseded(next_cmd);
        }
    };

    // The device is connected: other slots' reconnect scans must stop
    // claiming its advertisements.
    scanner::clear_reconnect(slot);
    slot_event_tx
        .send(SlotEvent::Connected {
            slot,
            attempt: *attempt,
            device: device.clone(),
        })
        .await;

    // Run phase. A live `Connection` now exists, so race the notification loop
    // against incoming commands. If a command supersedes us, explicitly tear
    // the link down (dropping the future alone does NOT disconnect the radio
    // link in the SoftDevice, which would leak a central connection slot).
    // A takeover keeps the link: the user selected this device before the
    // coordinator handled its Connected, so confirm it under the new number.
    // As with `prepare`, `run` is dropped before the link is closed.
    let superseding = {
        let mut run = pin!(hid_client::run_notification_loop(
            &conn, &client, descriptor, report_tx, led_rx, slot
        ));
        loop {
            match select(cmd_rx.receive(), run.as_mut()).await {
                Either::First(next_cmd) => match take_over(next_cmd, device) {
                    Ok((_, next_attempt)) => {
                        *attempt = next_attempt;
                        slot_event_tx
                            .send(SlotEvent::Connected {
                                slot,
                                attempt: next_attempt,
                                device: device.clone(),
                            })
                            .await;
                    }
                    Err(next_cmd) => break Some(next_cmd),
                },
                Either::Second(()) => break None,
            }
        }
    };
    let outcome = match superseding {
        Some(next_cmd) => {
            close_connection(&conn).await;
            SlotOutcome::Superseded(next_cmd)
        }
        None => SlotOutcome::Closed,
    };

    // Whatever this link was holding down on the host (a key, a mouse button)
    // would otherwise stay pressed — and auto-repeat — since its release can
    // no longer arrive over BLE.
    report_tx
        .send(HidEvent::Disconnected { source: slot })
        .await;
    outcome
}
