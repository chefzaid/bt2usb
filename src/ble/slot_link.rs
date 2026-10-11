//! One connection attempt of a slot worker: connect, secure the link,
//! discover HID, and run the notification loop, racing each phase against the
//! slot's commands.
//!
//! [`connection_slot_task`](crate::ble::slot_worker::connection_slot_task)
//! calls [`connect_and_run_secure`] once per attempt and decides from the
//! returned [`SlotOutcome`] whether to report, retry, or handle a new command.
//! A `Connect` for the device being reached takes the attempt over instead of
//! ending it (see [`take_over`]). A link that drops while it is being secured
//! or while its HID service is discovered fails as `ConnectFailed`, like a
//! peer that never connected, so a background reconnect retries it; a
//! discovery failure on a link that is still up keeps its own error.

use crate::ble::bonder::bonder;
use crate::ble::multi_conn::{SlotCommand, SlotEvent};
use crate::ble::{hid_client, scanner, BleErrorTag, DiscoveredDevice};
use crate::config;
use crate::hid::delivery::HidEvent;
use crate::hid::report_protocol::HidDescriptor;
use core::pin::pin;
use defmt::{info, warn};
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Receiver, Sender};
use embassy_time::{Duration, Timer};
use nrf_softdevice::ble::{central, Connection, EncryptError, SecurityMode};
use nrf_softdevice::raw;
use nrf_softdevice::Softdevice;

/// Outcome of a single slot connection attempt + run.
pub(crate) enum SlotOutcome {
    /// The peer closed the link (or it ended normally).
    Closed,
    /// The connection attempt failed before the link was usable. A link that
    /// dropped while being secured or discovered fails as `ConnectFailed`.
    Failed(BleErrorTag),
    /// A `Connect` for this attempt's device took over the attempt, which then
    /// failed before the link was usable. The user's connection, already
    /// numbered with the new attempt, is returned to be made next.
    TakenOver(DiscoveredDevice),
    /// A new command arrived and superseded the active link, which has already
    /// been explicitly disconnected. The command is returned to be processed next.
    Superseded(SlotCommand),
}

/// What one attempt connects to, and how.
pub(crate) struct ConnectionRequest<'a> {
    pub(crate) device: &'a DiscoveredDevice,
    /// False for a background reconnect, which never initiates pairing.
    pub(crate) allow_pairing: bool,
    /// The number of the attempt being served, which a `Connect` that takes
    /// the attempt over replaces.
    pub(crate) attempt: &'a mut u32,
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

pub(crate) async fn connect_and_run_secure(
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
            // The vendored crate marks the link disconnected before it fails
            // the pending GATT procedure, so a link gone by now dropped during
            // discovery, for example a keyboard going back to sleep right
            // after the key press that woke it. Like a link that never came
            // up, that is a connection failure, which a background reconnect
            // retries.
            let reported = tag.for_failed_setup(conn.handle().is_some());
            if reported != tag {
                info!("slot {} link dropped during HID discovery", slot);
            }
            close_connection(&conn).await;
            return match takeover {
                Some(next) => SlotOutcome::TakenOver(next),
                None => SlotOutcome::Failed(reported),
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
