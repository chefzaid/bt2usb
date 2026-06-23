//! Multi-device BLE connection manager.
//!
//! Supports up to two concurrent BLE HID peripheral links (typical:
//! keyboard + mouse) with secure pairing and bonding.

use core::cell::RefCell;

use crate::ble::coordinator::{self, Action, ConnManager, UiEvent, MAX_CONNECTIONS};
use crate::ble::scanner::ScanResult;
use crate::ble::{
    hid_client, reconnect, scanner, BleCommand, BleErrorTag, BleEvent, DiscoveredDevice,
};
use crate::config;
use crate::config::MAX_PAIRED_DEVICES;
use crate::hid::HidReport;
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
    Connect(DiscoveredDevice),
    Disconnect,
}

#[derive(Clone)]
pub enum SlotEvent {
    Connected {
        slot: usize,
        device: DiscoveredDevice,
    },
    Disconnected {
        slot: usize,
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
            let _ = peers.push(*bond);
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
        _conn: &Connection,
        master_id: MasterId,
        key: EncryptionInfo,
        peer_id: IdentityKey,
    ) {
        let mut peers = self.peers.borrow_mut();
        if let Some(existing) = peers.iter_mut().find(|p| p.master_id == master_id) {
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

    fn get_key(&self, _conn: &Connection, master_id: MasterId) -> Option<EncryptionInfo> {
        self.peers
            .borrow()
            .iter()
            .find_map(|p| (p.master_id == master_id).then_some(p.key))
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
    }

    let mut manager = MultiConnectionManager::new();
    let mut last_scan: Option<ScanResult> = None;

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
            send_slot_cmd(slot, SlotCommand::Connect(device), slot0_tx, slot1_tx).await;
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
            },
            Either::Second(event) => match event {
                SlotEvent::Connected { slot, device } => {
                    for action in coordinator::on_slot_connected(&mut manager, slot, &device) {
                        execute_action(action, event_tx, slot0_tx, slot1_tx, &mut flash).await;
                    }
                }
                SlotEvent::Disconnected { slot } => {
                    for action in coordinator::on_slot_disconnected(&mut manager, slot) {
                        execute_action(action, event_tx, slot0_tx, slot1_tx, &mut flash).await;
                    }
                }
                SlotEvent::Error { slot, tag } => {
                    for action in coordinator::on_slot_error(&mut manager, slot, tag) {
                        execute_action(action, event_tx, slot0_tx, slot1_tx, &mut flash).await;
                    }
                }
            },
        }
    }
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
            store.add(PairedDevice::new(
                device.address,
                device.name.as_str(),
                device.rssi,
            ));
            if let Some(bond) = bonder().bond_for_address(device.address) {
                store.set_bond_for_address(device.address, bond);
            }
            store.save_to_flash(flash).await;
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

pub async fn connection_slot_task(
    slot: usize,
    sd: &'static Softdevice,
    cmd_rx: &Receiver<'static, CriticalSectionRawMutex, SlotCommand, 2>,
    slot_event_tx: &Sender<'static, CriticalSectionRawMutex, SlotEvent, 8>,
    report_tx: &Sender<'static, CriticalSectionRawMutex, HidReport, 16>,
) -> ! {
    let mut pending_cmd: Option<SlotCommand> = None;
    // One host-LED receiver per slot (taken once; reused across reconnects). The
    // slot that holds the keyboard writes LED state through; others ignore it.
    let mut led_rx = crate::usb::hid_device::keyboard_led_receiver();

    loop {
        let cmd = match pending_cmd.take() {
            Some(cmd) => cmd,
            None => cmd_rx.receive().await,
        };

        match cmd {
            SlotCommand::Connect(device) => {
                match connect_and_run_secure(
                    sd,
                    &device,
                    report_tx,
                    slot_event_tx,
                    slot,
                    cmd_rx,
                    led_rx.as_mut(),
                )
                .await
                {
                    SlotOutcome::Closed => {
                        slot_event_tx.send(SlotEvent::Disconnected { slot }).await;
                    }
                    SlotOutcome::Failed(tag) => {
                        slot_event_tx.send(SlotEvent::Error { slot, tag }).await;
                    }
                    SlotOutcome::Superseded(next_cmd) => {
                        slot_event_tx.send(SlotEvent::Disconnected { slot }).await;
                        // Re-process the superseding command (Disconnect is a no-op
                        // here since the link is already torn down).
                        if let SlotCommand::Connect(_) = next_cmd {
                            pending_cmd = Some(next_cmd);
                        }
                    }
                }
            }
            SlotCommand::Disconnect => {}
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

async fn wait_for_secure_link(conn: &Connection) -> bool {
    for _ in 0..25 {
        match conn.security_mode() {
            SecurityMode::NoAccess | SecurityMode::Open => {
                Timer::after(Duration::from_millis(200)).await
            }
            _ => return true,
        }
    }
    false
}

async fn connect_and_run_secure(
    sd: &'static Softdevice,
    device: &DiscoveredDevice,
    report_tx: &Sender<'_, CriticalSectionRawMutex, HidReport, 16>,
    slot_event_tx: &Sender<'_, CriticalSectionRawMutex, SlotEvent, 8>,
    slot: usize,
    cmd_rx: &Receiver<'_, CriticalSectionRawMutex, SlotCommand, 2>,
    led_rx: Option<&mut crate::usb::hid_device::LedReceiver>,
) -> SlotOutcome {
    info!("slot {} connecting to {}", slot, device.name.as_str());

    let whitelist = [&device.address];
    let conn_cfg = central::ConnectConfig {
        scan_config: central::ScanConfig {
            whitelist: Some(&whitelist),
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

    // Establishment phase. This is intentionally not raced against incoming
    // commands: until `connect_with_security` returns there is no live
    // `Connection` to disconnect, so cancelling the future here is leak-free,
    // and once a link exists we must own it so we can explicitly disconnect it.
    let conn = match central::connect_with_security(sd, &conn_cfg, bonder()).await {
        Ok(conn) => conn,
        Err(_) => return SlotOutcome::Failed(BleErrorTag::ConnectFailed),
    };

    let secure_ok = match conn.encrypt() {
        Ok(()) => wait_for_secure_link(&conn).await,
        Err(EncryptError::PeerKeysNotFound) => {
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
        let _ = conn.disconnect();
        return SlotOutcome::Failed(BleErrorTag::ConnectFailed);
    }

    let (client, descriptor) = match hid_client::discover_and_subscribe(&conn).await {
        Ok(v) => v,
        Err(tag) => {
            let _ = conn.disconnect();
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
    let run_fut = hid_client::run_notification_loop(&conn, &client, descriptor, report_tx, led_rx);
    match select(cmd_rx.receive(), run_fut).await {
        Either::First(next_cmd) => {
            let _ = conn.disconnect();
            SlotOutcome::Superseded(next_cmd)
        }
        Either::Second(()) => SlotOutcome::Closed,
    }
}
