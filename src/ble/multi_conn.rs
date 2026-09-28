//! Multi-device BLE connection manager.
//!
//! Supports up to two concurrent BLE HID peripheral links (typical:
//! keyboard + mouse) with secure pairing and bonding.

use core::cell::RefCell;

use crate::ble::coordinator::{self, Action, ConnManager, UiEvent, MAX_CONNECTIONS};
use crate::ble::management::Quiescence;
use crate::ble::scanner::ScanResult;
use crate::ble::{
    hid_client, reconnect, scanner, BleCommand, BleErrorTag, BleEvent, DiscoveredDevice,
};
use crate::config;
use crate::config::MAX_PAIRED_DEVICES;
use crate::hid::delivery::HidEvent;
use crate::storage::{BondInfo, PairedDevice, DEVICE_STORE};
use defmt::{info, warn};
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Receiver, Sender};
use embassy_time::{Duration, Timer};
use heapless::Vec;
use nrf_softdevice::ble::security::{IoCapabilities, SecurityHandler};
use nrf_softdevice::ble::{
    central, Address, Connection, EncryptError, EncryptionInfo, IdentityKey, MasterId, SecurityMode,
};
use nrf_softdevice::raw;
use nrf_softdevice::Softdevice;
use static_cell::StaticCell;

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

struct Bonder {
    peers: RefCell<Vec<BondInfo, MAX_PAIRED_DEVICES>>,
}

impl Bonder {
    fn new() -> Self {
        Self {
            peers: RefCell::new(Vec::new()),
        }
    }

    fn load_bonds(&self, bonds: &Vec<BondInfo, MAX_PAIRED_DEVICES>) {
        let mut peers = self.peers.borrow_mut();
        peers.clear();
        for bond in bonds {
            if let Some(existing) = peers
                .iter_mut()
                .find(|p| p.peer_id.addr == bond.peer_id.addr)
            {
                *existing = *bond;
            } else {
                let _ = peers.push(*bond);
            }
        }
        info!("Loaded {} BLE bonds into security handler", peers.len());
    }

    fn bond_for_address(&self, address: Address) -> Option<BondInfo> {
        self.peers
            .borrow()
            .iter()
            .find(|p| p.peer_id.is_match(address))
            .copied()
    }

    fn forget(&self, address: Address) {
        self.peers
            .borrow_mut()
            .retain(|bond| !bond.peer_id.is_match(address));
    }

    fn clear(&self) {
        self.peers.borrow_mut().clear();
    }
}

impl SecurityHandler for Bonder {
    fn io_capabilities(&self) -> IoCapabilities {
        IoCapabilities::None
    }

    fn can_bond(&self, _conn: &Connection) -> bool {
        true
    }

    fn on_bonded(
        &self,
        conn: &Connection,
        master_id: MasterId,
        key: EncryptionInfo,
        peer_id: IdentityKey,
    ) {
        let mut peers = self.peers.borrow_mut();
        // MasterId is not a peer identity (LE Secure Connections can use the
        // same all-zero EDIV/RAND for multiple peers). Re-pairing replaces only
        // this peer's keys and must not overwrite another keyboard's bond.
        if let Some(existing) = peers
            .iter_mut()
            .find(|p| p.peer_id.addr == peer_id.addr || p.peer_id.is_match(conn.peer_address()))
        {
            existing.master_id = master_id;
            existing.key = key;
            existing.peer_id = peer_id;
            return;
        }

        if peers.is_full() {
            peers.remove(0);
        }

        let _ = peers.push(BondInfo {
            master_id,
            key,
            peer_id,
        });
    }

    fn get_key(&self, conn: &Connection, master_id: MasterId) -> Option<EncryptionInfo> {
        self.peers.borrow().iter().find_map(|p| {
            (p.master_id == master_id && p.peer_id.is_match(conn.peer_address())).then_some(p.key)
        })
    }

    fn get_peripheral_key(&self, conn: &Connection) -> Option<(MasterId, EncryptionInfo)> {
        self.peers.borrow().iter().find_map(|p| {
            p.peer_id
                .is_match(conn.peer_address())
                .then_some((p.master_id, p.key))
        })
    }

    fn on_security_update(&self, _conn: &Connection, mode: SecurityMode) {
        info!("BLE security mode updated: {}", mode);
    }
}

/// The single BLE bonder/security handler, shared by every connection slot.
///
/// `Bonder` holds a `RefCell` so it is `!Sync` and can't live in a `static`
/// directly (nor in `LazyLock`, which requires `Sync`). `StaticCell` only
/// requires `Send`, so it backs the storage; the first caller initialises it and
/// caches the `&'static` in an `AtomicPtr` so later calls don't re-`init` (which
/// would panic). On the single-threaded cooperative executor the init can't
/// race, so the spin fallback is just defensive.
fn bonder() -> &'static Bonder {
    use core::sync::atomic::{AtomicPtr, Ordering};

    static BONDER: StaticCell<Bonder> = StaticCell::new();
    static BONDER_REF: AtomicPtr<Bonder> = AtomicPtr::new(core::ptr::null_mut());

    let ptr = BONDER_REF.load(Ordering::Acquire);
    if !ptr.is_null() {
        // SAFETY: pointer came from StaticCell::try_init; the Bonder is 'static.
        unsafe { &*ptr }
    } else if let Some(b) = BONDER.try_init(Bonder::new()) {
        BONDER_REF.store(b as *mut Bonder, Ordering::Release);
        b
    } else {
        loop {
            let ptr = BONDER_REF.load(Ordering::Acquire);
            if !ptr.is_null() {
                // SAFETY: as above.
                break unsafe { &*ptr };
            }
        }
    }
}

pub async fn ble_task(
    sd: &'static Softdevice,
    cmd_rx: &Receiver<'static, CriticalSectionRawMutex, BleCommand, 4>,
    event_tx: &Sender<'static, CriticalSectionRawMutex, BleEvent, 8>,
    slot0_tx: &Sender<'static, CriticalSectionRawMutex, SlotCommand, 2>,
    slot1_tx: &Sender<'static, CriticalSectionRawMutex, SlotCommand, 2>,
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

    // Auto-reconnect the most-recently-used devices (up to the number of
    // connection slots) so a keyboard + mouse pair both come back after a
    // reboot without manual re-selection.
    let peers: Vec<(DiscoveredDevice, Option<BondInfo>), MAX_CONNECTIONS> = {
        let store = DEVICE_STORE.lock().await;
        let mut v = Vec::new();
        for paired in store.iter_recent().take(MAX_CONNECTIONS) {
            let device = DiscoveredDevice {
                address: paired.address,
                name: paired.name.clone(),
                rssi: paired.last_rssi,
            };
            let _ = v.push((device, paired.bond));
        }
        v
    };

    if !peers.is_empty() {
        // Devices that use a rotating Resolvable Private Address advertise under
        // a random address that differs from the one stored at pairing time, so
        // a whitelist connect to the stored address would never match. Scan
        // first and resolve each stored peer's IRK against the live
        // advertisements so we reconnect to its *current* address.
        let scan = scanner::scan(sd, event_tx).await.ok();
        let scanned: &[DiscoveredDevice] =
            scan.as_ref().map(|s| s.devices.as_slice()).unwrap_or(&[]);

        let targets = reconnect::resolve_reconnect_targets(peers.len(), scanned.len(), |p, s| {
            let (stored, bond) = &peers[p];
            let advertised = scanned[s].address;
            // Resolve a rotating RPA by IRK, or match a stable address directly.
            bond.map(|b| b.peer_id.is_match(advertised))
                .unwrap_or(false)
                || stored.address == advertised
        });

        for (slot, target) in targets.iter().enumerate() {
            let (stored, _) = &peers[target.peer];
            let device = match target.scanned {
                // Connect to the live (resolved) address, keeping the stored name.
                Some(i) => DiscoveredDevice {
                    address: scanned[i].address,
                    name: stored.name.clone(),
                    rssi: scanned[i].rssi,
                },
                // Not seen this scan — fall back to the stored address.
                None => stored.clone(),
            };
            manager.reserve_slot(slot, &device);
            // A paired device that's asleep or off right now will advertise
            // once it wakes, so keep trying rather than failing once.
            send_slot_cmd(slot, SlotCommand::Reconnect(device), slot0_tx, slot1_tx).await;
        }

        last_scan = scan;
    }

    // The coordinator below is a thin interpreter: it asks the pure
    // `coordinator` reducers (host-tested) what to do for each command/event,
    // then performs the resulting I/O via `execute_action`.
    loop {
        match select(cmd_rx.receive(), slot_event_rx.receive()).await {
            Either::First(cmd) => match cmd {
                BleCommand::StartScan => {
                    for action in coordinator::plan_start_scan(&manager) {
                        execute_action(action, event_tx, slot0_tx, slot1_tx, &mut flash).await;
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
                        execute_action(action, event_tx, slot0_tx, slot1_tx, &mut flash).await;
                    }
                }
                BleCommand::Disconnect => {
                    for action in coordinator::plan_disconnect(&manager) {
                        execute_action(action, event_tx, slot0_tx, slot1_tx, &mut flash).await;
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
                        slot0_tx,
                        slot1_tx,
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
                        slot0_tx,
                        slot1_tx,
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
                handle_slot_event(
                    event,
                    &mut manager,
                    event_tx,
                    slot0_tx,
                    slot1_tx,
                    &mut flash,
                )
                .await;
            }
        }
    }
}

async fn handle_slot_event(
    event: SlotEvent,
    manager: &mut MultiConnectionManager,
    event_tx: &Sender<'static, CriticalSectionRawMutex, BleEvent, 8>,
    slot0_tx: &Sender<'static, CriticalSectionRawMutex, SlotCommand, 2>,
    slot1_tx: &Sender<'static, CriticalSectionRawMutex, SlotCommand, 2>,
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
        execute_action(action, event_tx, slot0_tx, slot1_tx, flash).await;
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
#[allow(clippy::too_many_arguments)]
async fn manage_devices(
    address: Option<Address>,
    token: u32,
    manager: &mut MultiConnectionManager,
    event_tx: &Sender<'static, CriticalSectionRawMutex, BleEvent, 8>,
    slot0_tx: &Sender<'static, CriticalSectionRawMutex, SlotCommand, 2>,
    slot1_tx: &Sender<'static, CriticalSectionRawMutex, SlotCommand, 2>,
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
            send_slot_cmd(slot, SlotCommand::Quiesce(token), slot0_tx, slot1_tx).await;
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
            handle_slot_event(event, manager, event_tx, slot0_tx, slot1_tx, flash).await;
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
    slot0_tx: &Sender<'static, CriticalSectionRawMutex, SlotCommand, 2>,
    slot1_tx: &Sender<'static, CriticalSectionRawMutex, SlotCommand, 2>,
    flash: &mut nrf_softdevice::Flash,
) {
    match action {
        Action::DisconnectSlot(slot) => {
            send_slot_cmd(slot, SlotCommand::Disconnect, slot0_tx, slot1_tx).await;
        }
        Action::ConnectSlot { slot, device } => {
            send_slot_cmd(slot, SlotCommand::Connect(device), slot0_tx, slot1_tx).await;
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
    let mut led_rx = crate::usb::hid_device::keyboard_led_receiver();

    loop {
        let cmd = match (pending_cmd.take(), retry.take()) {
            (Some(cmd), _) => cmd,
            (None, None) => cmd_rx.receive().await,
            // Between attempts, stay responsive: any command from the
            // coordinator replaces the retry.
            (None, Some(device)) => {
                let backoff = Timer::after(Duration::from_millis(config::BLE_RECONNECT_BACKOFF_MS));
                match select(cmd_rx.receive(), backoff).await {
                    Either::First(SlotCommand::Disconnect) => {
                        // The coordinator still counts this slot as reserved;
                        // tell it the slot is free now that retrying stopped.
                        slot_event_tx.send(SlotEvent::Disconnected { slot }).await;
                        continue;
                    }
                    Either::First(cmd) => cmd,
                    Either::Second(()) => SlotCommand::Reconnect(device),
                }
            }
        };

        let (mut device, silent) = match cmd {
            SlotCommand::Connect(device) => (device, false),
            SlotCommand::Reconnect(device) => (device, true),
            SlotCommand::Disconnect => {
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
            if let Some(bond) = bonder().bond_for_address(device.address) {
                // RPA rotation can happen at any time, not just at boot. A
                // whitelist retry against the previous address never recovers.
                match select(
                    cmd_rx.receive(),
                    scanner::resolve_bonded_peer(sd, &bond.peer_id),
                )
                .await
                {
                    Either::First(next) => {
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
            // secured): nothing new to report, try again.
            SlotOutcome::Failed(BleErrorTag::ConnectFailed) if silent => {
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
    }
}

async fn send_slot_cmd(
    slot: usize,
    cmd: SlotCommand,
    slot0_tx: &Sender<'static, CriticalSectionRawMutex, SlotCommand, 2>,
    slot1_tx: &Sender<'static, CriticalSectionRawMutex, SlotCommand, 2>,
) {
    match slot {
        0 => slot0_tx.send(cmd).await,
        1 => slot1_tx.send(cmd).await,
        _ => {}
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
    led_rx: Option<&mut crate::usb::hid_device::LedReceiver>,
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
