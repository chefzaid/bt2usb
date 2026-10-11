//! One connection slot's worker: connect, secure, run the HID client, and
//! reconnect in the background.
//!
//! [`connection_slot_task`] runs once per link. It takes [`SlotCommand`]s from
//! the coordinator in [`multi_conn`](crate::ble::multi_conn), reports
//! [`SlotEvent`]s back, and between commands keeps retrying a saved device
//! whose link dropped or that has not been seen since power-up, through the
//! shared reconnect scan in [`scanner`]. A silent retry never pairs.

use crate::ble::bonder::bonder;
use crate::ble::multi_conn::{SlotCommand, SlotEvent};
use crate::ble::scanner::SavedPeer;
use crate::ble::{hid_client, scanner, BleErrorTag, DiscoveredDevice};
use crate::config;
use crate::hid::delivery::HidEvent;
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
    /// A new command arrived and superseded the active link, which has already
    /// been explicitly disconnected. The command is returned to be processed next.
    Superseded(SlotCommand),
}

struct ConnectionRequest<'a> {
    device: &'a DiscoveredDevice,
    allow_pairing: bool,
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
                    Either3::First(cmd) => cmd,
                    Either3::Second(()) | Either3::Third(()) => SlotCommand::Reconnect(device),
                }
            }
        };
        // Only a background retry keeps this slot's reconnect target.
        if !matches!(cmd, SlotCommand::Reconnect(_)) {
            scanner::clear_reconnect(slot);
        }

        let (mut device, silent) = match cmd {
            SlotCommand::Connect(device) => (device, false),
            SlotCommand::Reconnect(device) => (device, true),
            SlotCommand::Disconnect => {
                // Also ends a retry between attempts: the coordinator still
                // counts this slot as reserved, so tell it the slot is free.
                slot_event_tx.send(SlotEvent::Disconnected { slot }).await;
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
            // RPA rotation can happen at any time, not just at boot. A
            // whitelist retry against the previous address never recovers, so
            // find the device's live address first. A record saved without a
            // bond is matched by its stored address.
            scanner::register_reconnect(
                slot,
                SavedPeer {
                    address: device.address,
                    identity: bonder()
                        .bond_for_address(device.address)
                        .map(|bond| bond.peer_id),
                },
            );
            match select(cmd_rx.receive(), scanner::find_saved_peer(sd, slot)).await {
                Either::First(next) => {
                    scanner::clear_reconnect(slot);
                    slot_event_tx.send(SlotEvent::Disconnected { slot }).await;
                    pending_cmd = Some(next);
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
            },
            report_tx,
            slot_event_tx,
            slot,
            cmd_rx,
            led_rx.as_mut(),
        )
        .await
        {
            // Without keys for the device (its pairing was refused, or a newer
            // pairing evicted them), a background reconnect, which never
            // pairs, could never secure the link: free the slot instead.
            SlotOutcome::Closed if bonder().bond_for_address(device.address).is_none() => {
                info!("slot {} link lost; no keys to reconnect", slot);
                slot_event_tx.send(SlotEvent::Disconnected { slot }).await;
            }
            SlotOutcome::Closed => {
                // The peer dropped a working link: keep the slot for it and
                // reconnect as soon as it advertises again.
                info!("slot {} link lost; reconnecting", slot);
                slot_event_tx
                    .send(SlotEvent::LinkLost {
                        slot,
                        device: device.clone(),
                    })
                    .await;
                retry = Some(device);
            }
            // Not found in this attempt's window (or the link couldn't be
            // secured): nothing new to report, try again. Meanwhile the other
            // slot's scans ignore this device for a while, in case it
            // advertises but will not connect.
            SlotOutcome::Failed(BleErrorTag::ConnectFailed) if silent => {
                scanner::reconnect_attempt_failed(slot);
                retry = Some(device);
            }
            SlotOutcome::Failed(tag) => {
                slot_event_tx.send(SlotEvent::Error { slot, tag }).await;
            }
            SlotOutcome::Superseded(next_cmd) => {
                slot_event_tx.send(SlotEvent::Disconnected { slot }).await;
                // Re-process a superseding connect (Disconnect is a no-op here
                // since the link is already torn down).
                if !matches!(next_cmd, SlotCommand::Disconnect) {
                    pending_cmd = Some(next_cmd);
                }
            }
        }
        if retry.is_none() {
            scanner::clear_reconnect(slot);
        }
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

async fn connect_and_run_secure(
    sd: &'static Softdevice,
    request: ConnectionRequest<'_>,
    report_tx: &Sender<'_, CriticalSectionRawMutex, HidEvent, 16>,
    slot_event_tx: &Sender<'_, CriticalSectionRawMutex, SlotEvent, 8>,
    slot: usize,
    cmd_rx: &Receiver<'_, CriticalSectionRawMutex, SlotCommand, 2>,
    led_rx: Option<&mut crate::usb::host_requests::LedReceiver>,
) -> SlotOutcome {
    let device = request.device;
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

    let prepare = async {
        let secure_ok = match conn.encrypt() {
            Ok(()) => wait_for_secure_link(&conn).await,
            // A background reconnect must never initiate a replacement pairing.
            // Fresh pairing is restricted to an explicit user connection.
            Err(EncryptError::PeerKeysNotFound) if request.allow_pairing => {
                if conn.request_pairing().is_ok() {
                    wait_for_secure_link(&conn).await
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
        hid_client::discover_and_subscribe(&conn).await
    };

    // Once a link is owned, disconnect promptly when the user cancels during
    // security or GATT discovery, not only after notifications start flowing.
    let (client, descriptor) = match select(cmd_rx.receive(), prepare).await {
        Either::First(next_cmd) => {
            close_connection(&conn).await;
            return SlotOutcome::Superseded(next_cmd);
        }
        Either::Second(Ok(value)) => value,
        Either::Second(Err(tag)) => {
            close_connection(&conn).await;
            return SlotOutcome::Failed(tag);
        }
    };

    // The device is connected: other slots' reconnect scans must stop
    // claiming its advertisements.
    scanner::clear_reconnect(slot);
    slot_event_tx
        .send(SlotEvent::Connected {
            slot,
            device: device.clone(),
        })
        .await;

    // Run phase. A live `Connection` now exists, so race the notification loop
    // against incoming commands. If a command supersedes us, explicitly tear
    // the link down (dropping the future alone does NOT disconnect the radio
    // link in the SoftDevice, which would leak a central connection slot).
    let run_fut =
        hid_client::run_notification_loop(&conn, &client, descriptor, report_tx, led_rx, slot);
    let outcome = match select(cmd_rx.receive(), run_fut).await {
        Either::First(next_cmd) => {
            close_connection(&conn).await;
            SlotOutcome::Superseded(next_cmd)
        }
        Either::Second(()) => SlotOutcome::Closed,
    };

    // Whatever this link was holding down on the host (a key, a mouse button)
    // would otherwise stay pressed — and auto-repeat — since its release can
    // no longer arrive over BLE.
    report_tx
        .send(HidEvent::Disconnected { source: slot })
        .await;
    outcome
}
