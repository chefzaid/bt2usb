//! Multi-device BLE connection manager.
//!
//! Supports up to two concurrent BLE HID peripheral links (typical:
//! keyboard + mouse) with secure pairing and bonding.

use core::cell::RefCell;
use core::fmt::Write;

use crate::ble::scanner::ScanResult;
use crate::ble::{hid_client, scanner, BleCommand, BleErrorTag, BleEvent, DiscoveredDevice};
use crate::config;
use crate::config::MAX_PAIRED_DEVICES;
use crate::hid::HidReport;
use crate::storage::{PairedDevice, DEVICE_STORE};
use defmt::{info, warn};
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Receiver, Sender};
use embassy_time::{Duration, Timer};
use heapless::{String, Vec};
use nrf_softdevice::ble::security::{IoCapabilities, SecurityHandler};
use nrf_softdevice::ble::{
    central, Address, Connection, EncryptError, EncryptionInfo, IdentityKey, MasterId, SecurityMode,
};
use nrf_softdevice::raw;
use nrf_softdevice::Softdevice;
use static_cell::StaticCell;

/// Maximum simultaneous BLE connections.
pub const MAX_CONNECTIONS: usize = 2;

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

#[derive(Clone)]
pub struct ConnectionSlot {
    pub address: Option<Address>,
    pub name: String<32>,
    pub connected: bool,
}

impl ConnectionSlot {
    pub const fn empty() -> Self {
        Self {
            address: None,
            name: String::new(),
            connected: false,
        }
    }
}

pub struct MultiConnectionManager {
    slots: [ConnectionSlot; MAX_CONNECTIONS],
}

impl MultiConnectionManager {
    pub const fn new() -> Self {
        Self {
            slots: [ConnectionSlot::empty(), ConnectionSlot::empty()],
        }
    }

    pub fn find_empty_slot(&self) -> Option<usize> {
        self.slots.iter().position(|s| !s.connected)
    }

    pub fn active_count(&self) -> usize {
        self.slots.iter().filter(|s| s.connected).count()
    }

    pub fn is_connected_address(&self, address: &Address) -> bool {
        self.slots
            .iter()
            .any(|slot| slot.connected && slot.address.as_ref() == Some(address))
    }

    pub fn connect_slot(&mut self, slot: usize, device: &DiscoveredDevice) {
        if slot < MAX_CONNECTIONS {
            self.slots[slot] = ConnectionSlot {
                address: Some(device.address),
                name: device.name.clone(),
                connected: true,
            };
        }
    }

    pub fn disconnect_slot(&mut self, slot: usize) {
        if slot < MAX_CONNECTIONS {
            self.slots[slot].address = None;
            self.slots[slot].name.clear();
            self.slots[slot].connected = false;
        }
    }

    pub fn get_connected_names(&self) -> heapless::Vec<String<32>, MAX_CONNECTIONS> {
        let mut names = heapless::Vec::new();
        for slot in &self.slots {
            if slot.connected {
                let _ = names.push(slot.name.clone());
            }
        }
        names
    }
}

struct PeerBond {
    master_id: MasterId,
    key: EncryptionInfo,
    peer_id: IdentityKey,
}

struct Bonder {
    peers: RefCell<Vec<PeerBond, MAX_PAIRED_DEVICES>>,
}

impl Bonder {
    fn new() -> Self {
        Self {
            peers: RefCell::new(Vec::new()),
        }
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

        let _ = peers.push(PeerBond {
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

fn bonder() -> &'static Bonder {
    static BONDER: StaticCell<Bonder> = StaticCell::new();
    BONDER.init(Bonder::new())
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
    }

    let mut manager = MultiConnectionManager::new();
    let mut last_scan: Option<ScanResult> = None;

    if let Some(paired) = {
        let store = DEVICE_STORE.lock().await;
        store.first().cloned()
    } {
        let auto = DiscoveredDevice {
            address: paired.address,
            name: paired.name,
            rssi: paired.last_rssi,
        };
        manager.connect_slot(0, &auto);
        slot0_tx.send(SlotCommand::Connect(auto)).await;
    }

    loop {
        match select(cmd_rx.receive(), slot_event_rx.receive()).await {
            Either::First(cmd) => match cmd {
                BleCommand::StartScan => {
                    if manager.active_count() >= MAX_CONNECTIONS {
                        for slot in 0..MAX_CONNECTIONS {
                            if manager.slots[slot].connected {
                                send_slot_cmd(slot, SlotCommand::Disconnect, slot0_tx, slot1_tx)
                                    .await;
                            }
                        }
                    }

                    match scanner::scan(sd, event_tx).await {
                        Ok(result) => {
                            last_scan = Some(result);
                        }
                        Err(_) => {
                            last_scan = None;
                        }
                    }
                }
                BleCommand::Connect(index) => {
                    let Some(scan) = &last_scan else {
                        event_tx
                            .send(BleEvent::Error(BleErrorTag::ConnectFailed))
                            .await;
                        continue;
                    };

                    let Some(device) = scan.devices.get(index) else {
                        event_tx
                            .send(BleEvent::Error(BleErrorTag::ConnectFailed))
                            .await;
                        continue;
                    };

                    if manager.is_connected_address(&device.address) {
                        warn!("Device already connected");
                        continue;
                    }

                    let Some(slot) = manager.find_empty_slot() else {
                        warn!("No free connection slots");
                        event_tx
                            .send(BleEvent::Error(BleErrorTag::ConnectFailed))
                            .await;
                        continue;
                    };

                    manager.connect_slot(slot, device);
                    send_slot_cmd(
                        slot,
                        SlotCommand::Connect(device.clone()),
                        slot0_tx,
                        slot1_tx,
                    )
                    .await;
                }
                BleCommand::Disconnect => {
                    for slot in 0..MAX_CONNECTIONS {
                        if manager.slots[slot].connected {
                            send_slot_cmd(slot, SlotCommand::Disconnect, slot0_tx, slot1_tx).await;
                        }
                    }
                }
            },
            Either::Second(event) => match event {
                SlotEvent::Connected { slot, device } => {
                    manager.connect_slot(slot, &device);
                    {
                        let mut store = DEVICE_STORE.lock().await;
                        store.add(PairedDevice::new(
                            device.address,
                            device.name.as_str(),
                            device.rssi,
                        ));
                        store.save_to_flash(&mut flash).await;
                    }

                    let summary = connection_summary(&manager);
                    event_tx.send(BleEvent::Connected(summary)).await;
                }
                SlotEvent::Disconnected { slot } => {
                    manager.disconnect_slot(slot);
                    if manager.active_count() == 0 {
                        event_tx.send(BleEvent::Disconnected).await;
                    } else {
                        let summary = connection_summary(&manager);
                        event_tx.send(BleEvent::Connected(summary)).await;
                    }
                }
                SlotEvent::Error { slot, tag } => {
                    manager.disconnect_slot(slot);
                    event_tx.send(BleEvent::Error(tag)).await;
                    if manager.active_count() == 0 {
                        event_tx.send(BleEvent::Disconnected).await;
                    } else {
                        let summary = connection_summary(&manager);
                        event_tx.send(BleEvent::Connected(summary)).await;
                    }
                }
            },
        }
    }
}

pub async fn connection_slot_task(
    slot: usize,
    sd: &'static Softdevice,
    cmd_rx: &Receiver<'static, CriticalSectionRawMutex, SlotCommand, 2>,
    slot_event_tx: &Sender<'static, CriticalSectionRawMutex, SlotEvent, 8>,
    report_tx: &Sender<'static, CriticalSectionRawMutex, HidReport, 16>,
) -> ! {
    let mut pending_cmd: Option<SlotCommand> = None;

    loop {
        let cmd = match pending_cmd.take() {
            Some(cmd) => cmd,
            None => cmd_rx.receive().await,
        };

        match cmd {
            SlotCommand::Connect(device) => {
                let connect_fut =
                    connect_and_run_secure(sd, &device, report_tx, slot_event_tx, slot);
                match select(cmd_rx.receive(), connect_fut).await {
                    Either::First(next_cmd) => match next_cmd {
                        SlotCommand::Disconnect => {
                            slot_event_tx.send(SlotEvent::Disconnected { slot }).await;
                        }
                        SlotCommand::Connect(next_device) => {
                            slot_event_tx.send(SlotEvent::Disconnected { slot }).await;
                            pending_cmd = Some(SlotCommand::Connect(next_device));
                        }
                    },
                    Either::Second(result) => match result {
                        Ok(()) => {
                            slot_event_tx.send(SlotEvent::Disconnected { slot }).await;
                        }
                        Err(tag) => {
                            slot_event_tx.send(SlotEvent::Error { slot, tag }).await;
                        }
                    },
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

fn connection_summary(manager: &MultiConnectionManager) -> String<32> {
    let names = manager.get_connected_names();
    match names.len() {
        0 => {
            let mut s = String::new();
            let _ = s.push_str("Connected");
            s
        }
        1 => names[0].clone(),
        n => {
            let mut s = String::new();
            let _ = write!(&mut s, "{} devices", n);
            s
        }
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
) -> Result<(), BleErrorTag> {
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

    let conn = central::connect_with_security(sd, &conn_cfg, bonder())
        .await
        .map_err(|_| BleErrorTag::ConnectFailed)?;

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
        return Err(BleErrorTag::ConnectFailed);
    }

    let (client, descriptor) = hid_client::discover_and_subscribe(&conn).await?;

    slot_event_tx
        .send(SlotEvent::Connected {
            slot,
            device: device.clone(),
        })
        .await;

    hid_client::run_notification_loop(&conn, &client, descriptor, report_tx).await;

    Ok(())
}
