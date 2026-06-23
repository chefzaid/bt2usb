//! Persistent storage for paired devices and BLE bonding keys.
//!
//! Uses the nRF52840's internal flash via `sequential-storage` crate
//! to store BLE addresses, display names, RSSI hints, and bonding keys
//! for previously paired devices so they can be auto-reconnected on power-up.
//!
//! Storage layout:
//!   - Each record is a serialized `PairedDevice` with optional `BondInfo`.
//!   - Records are appended sequentially; the flash pages are managed
//!     by `sequential-storage` which handles wear levelling and GC.

mod codec;

use codec::{
    deserialize_address, deserialize_bond, serialize_address, serialize_bond, ADDRESS_RECORD_SIZE,
    BOND_RECORD_SIZE,
};

use crate::config::{MAX_PAIRED_DEVICES, STORAGE_FLASH_PAGE_COUNT, STORAGE_FLASH_PAGE_START};
use defmt::{debug, error, info, warn};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer};
use heapless::Vec;
use nrf_softdevice::ble::{Address, EncryptionInfo, IdentityKey, MasterId};
use sequential_storage::cache::NoCache;

/// Flash page size for nRF52840 (4 KB).
const FLASH_PAGE_SIZE: u32 = 4096;

/// Start address of our storage region.
const STORAGE_START: u32 = STORAGE_FLASH_PAGE_START * FLASH_PAGE_SIZE;

/// End address (exclusive) of our storage region.
const STORAGE_END: u32 = (STORAGE_FLASH_PAGE_START + STORAGE_FLASH_PAGE_COUNT) * FLASH_PAGE_SIZE;

/// Key for the paired devices list in the map storage.
const KEY_PAIRED_DEVICES: u8 = 0x01;

const STORAGE_MAGIC: u8 = 0xB2;
const STORAGE_VERSION: u8 = 0x01;
// Record wire sizes (ADDRESS_RECORD_SIZE, BOND_RECORD_SIZE) live in `codec`.

/// Retry budget for a flash write that races BLE radio timeslots.
const FLASH_WRITE_ATTEMPTS: u8 = 3;
const FLASH_RETRY_BACKOFF_MS: u64 = 20;

/// Maximum serialized size for paired device records.
/// 4 devices × (address/name metadata + BLE bond keys) plus versioning overhead.
const MAX_RECORD_SIZE: usize = 512;

/// BLE bonding keys stored alongside the paired-device record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BondInfo {
    pub master_id: MasterId,
    pub key: EncryptionInfo,
    pub peer_id: IdentityKey,
}

/// A paired device record stored in flash.
#[derive(Clone, Debug)]
pub struct PairedDevice {
    /// BLE address (6 bytes + 1 address type byte).
    pub address: Address,
    /// Device name (for UI display, truncated to 32 bytes).
    pub name: heapless::String<32>,
    /// Last RSSI seen (for sorting by signal strength).
    pub last_rssi: i8,
    /// BLE bonding keys for reconnecting without pairing again.
    pub bond: Option<BondInfo>,
}

impl PairedDevice {
    /// Create a new paired device record.
    pub fn new(address: Address, name: &str, rssi: i8) -> Self {
        let mut n: heapless::String<32> = heapless::String::new();
        // Truncate name to fit heapless::String<32> capacity.
        for c in name.chars().take(32) {
            let _ = n.push(c);
        }
        Self {
            address,
            name: n,
            last_rssi: rssi,
            bond: None,
        }
    }

    fn serialize_base(&self, buf: &mut [u8]) -> usize {
        let name_bytes = self.name.as_bytes();

        // Format: [7 addr+type][1 rssi][1 name_len][name_bytes...]
        let total = ADDRESS_RECORD_SIZE + 1 + 1 + name_bytes.len();
        if buf.len() < total {
            return 0;
        }

        serialize_address(self.address, &mut buf[..ADDRESS_RECORD_SIZE]);
        buf[7] = self.last_rssi as u8;
        buf[8] = name_bytes.len() as u8;
        buf[9..9 + name_bytes.len()].copy_from_slice(name_bytes);
        total
    }

    /// Serialize to bytes for flash storage.
    fn serialize(&self, buf: &mut [u8]) -> usize {
        let base_len = self.serialize_base(buf);
        if base_len == 0 || buf.len() < base_len + 1 {
            return 0;
        }

        match self.bond {
            Some(bond) => {
                if buf.len() < base_len + 1 + BOND_RECORD_SIZE {
                    return 0;
                }
                buf[base_len] = 1;
                serialize_bond(
                    &bond,
                    &mut buf[base_len + 1..base_len + 1 + BOND_RECORD_SIZE],
                );
                base_len + 1 + BOND_RECORD_SIZE
            }
            None => {
                buf[base_len] = 0;
                base_len + 1
            }
        }
    }

    fn deserialize_base(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 9 {
            return None;
        }

        let address = deserialize_address(&data[..ADDRESS_RECORD_SIZE]);
        let rssi = data[7] as i8;
        let name_len = data[8] as usize;

        if data.len() < 9 + name_len {
            return None;
        }

        let name_slice = &data[9..9 + name_len];
        let mut name: heapless::String<32> = heapless::String::new();
        if let Ok(s) = core::str::from_utf8(name_slice) {
            for c in s.chars().take(32) {
                let _ = name.push(c);
            }
        }

        Some((
            Self {
                address,
                name,
                last_rssi: rssi,
                bond: None,
            },
            9 + name_len,
        ))
    }

    /// Deserialize a versioned record from bytes.
    fn deserialize(data: &[u8]) -> Option<Self> {
        let (mut device, mut offset) = Self::deserialize_base(data)?;
        if offset < data.len() {
            let has_bond = data[offset] != 0;
            offset += 1;
            if has_bond {
                device.bond = deserialize_bond(data.get(offset..offset + BOND_RECORD_SIZE)?);
            }
        }
        Some(device)
    }
}

/// In-memory cache of paired devices, synced with flash.
pub struct DeviceStore {
    /// Cached list of paired devices.
    devices: Vec<PairedDevice, MAX_PAIRED_DEVICES>,
    /// Dirty flag - true if cache differs from flash.
    dirty: bool,
}

impl DeviceStore {
    /// Create an empty store.
    pub const fn new() -> Self {
        Self {
            devices: Vec::new(),
            dirty: false,
        }
    }

    /// Async load from flash using sequential-storage.
    pub async fn load_from_flash(
        &mut self,
        flash: &mut impl embedded_storage_async::nor_flash::NorFlash,
    ) {
        let mut buf = [0u8; MAX_RECORD_SIZE];

        // sequential-storage 7 exposes a stateful `MapStorage` (the standalone
        // `map::fetch_item` free function was removed). It borrows the flash for
        // the duration of the access and is dropped before we return.
        let config = sequential_storage::map::MapConfig::new(STORAGE_START..STORAGE_END);
        let mut map = sequential_storage::map::MapStorage::<u8, _, _>::new(flash, config, NoCache);

        match map.fetch_item::<&[u8]>(&mut buf, &KEY_PAIRED_DEVICES).await {
            Ok(Some(data)) => {
                self.devices.clear();
                self.deserialize_all(data);
                info!("Loaded {} devices from flash", self.devices.len());
            }
            Ok(None) => {
                info!("No paired devices in flash");
                self.devices.clear();
            }
            Err(e) => {
                error!("Flash read error: {:?}", defmt::Debug2Format(&e));
                self.devices.clear();
            }
        }
        self.dirty = false;
    }

    /// Persist all paired devices to flash.
    pub async fn save_to_flash(
        &mut self,
        flash: &mut impl embedded_storage_async::nor_flash::NorFlash,
    ) {
        if !self.dirty {
            debug!("DeviceStore: no changes to save");
            return;
        }

        let mut buf = [0u8; MAX_RECORD_SIZE];
        let mut data_buf = [0u8; MAX_RECORD_SIZE];

        let len = self.serialize_all(&mut data_buf);
        let item: &[u8] = &data_buf[..len];

        let config = sequential_storage::map::MapConfig::new(STORAGE_START..STORAGE_END);
        let mut map = sequential_storage::map::MapStorage::<u8, _, _>::new(flash, config, NoCache);

        // SoftDevice flash operations need radio-idle timeslots and can fail with
        // a transient busy/timeout error while BLE links are active (this save
        // runs right at connect time). Retry a few times with a short backoff.
        for attempt in 1..=FLASH_WRITE_ATTEMPTS {
            match map
                .store_item::<&[u8]>(&mut buf, &KEY_PAIRED_DEVICES, &item)
                .await
            {
                Ok(_) => {
                    info!("Saved {} devices to flash", self.devices.len());
                    self.dirty = false;
                    return;
                }
                Err(e) => {
                    if attempt < FLASH_WRITE_ATTEMPTS {
                        warn!("Flash write busy (attempt {}), retrying", attempt);
                        Timer::after(Duration::from_millis(FLASH_RETRY_BACKOFF_MS)).await;
                    } else {
                        error!(
                            "Flash write failed after {} attempts: {:?}",
                            FLASH_WRITE_ATTEMPTS,
                            defmt::Debug2Format(&e)
                        );
                    }
                }
            }
        }
    }

    /// Serialize all devices to a byte buffer.
    fn serialize_all(&self, buf: &mut [u8]) -> usize {
        if buf.len() < 3 {
            return 0;
        }

        buf[0] = STORAGE_MAGIC;
        buf[1] = STORAGE_VERSION;
        buf[2] = self.devices.len() as u8;
        let mut offset = 3;

        for device in &self.devices {
            if offset >= buf.len() {
                break;
            }

            let record_len_offset = offset;
            offset += 1;
            let written = device.serialize(&mut buf[offset..]);
            if written == 0 || written > u8::MAX as usize {
                offset = record_len_offset;
                break;
            }

            buf[record_len_offset] = written as u8;
            offset += written;
        }

        offset
    }

    /// Deserialize all devices from a byte buffer.
    fn deserialize_all(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }

        if data.len() >= 3 && data[0] == STORAGE_MAGIC && data[1] == STORAGE_VERSION {
            self.deserialize_versioned(data);
        } else {
            self.deserialize_legacy(data);
        }
    }

    fn deserialize_versioned(&mut self, data: &[u8]) {
        let count = data[2] as usize;
        let mut offset = 3;

        for _ in 0..count {
            if offset >= data.len() {
                break;
            }

            let record_len = data[offset] as usize;
            offset += 1;
            if record_len == 0 || offset + record_len > data.len() {
                break;
            }

            if let Some(device) = PairedDevice::deserialize(&data[offset..offset + record_len]) {
                if !self.devices.is_full() {
                    let _ = self.devices.push(device);
                }
            }

            offset += record_len;
        }
    }

    fn deserialize_legacy(&mut self, data: &[u8]) {
        let count = data[0] as usize;
        let mut offset = 1;

        for _ in 0..count {
            if offset >= data.len() {
                break;
            }

            // Read name length to determine record size.
            if offset + 9 > data.len() {
                break;
            }
            let name_len = data[offset + 8] as usize;
            let record_len = 9 + name_len;

            if offset + record_len > data.len() {
                break;
            }

            if let Some((device, _)) =
                PairedDevice::deserialize_base(&data[offset..offset + record_len])
            {
                if !self.devices.is_full() {
                    let _ = self.devices.push(device);
                }
            }

            offset += record_len;
        }
    }

    /// Add a newly paired device.
    pub fn add(&mut self, device: PairedDevice) {
        // If already stored (same address), update the record. Only persist
        // (mark dirty) when something we care about for reconnect actually
        // changed — RSSI churns on every reconnect and is just a UI hint, so
        // updating it alone must not cause a flash write (avoidable wear).
        if let Some(existing) = self
            .devices
            .iter_mut()
            .find(|d| d.address == device.address)
        {
            let name_changed = existing.name != device.name;
            let bond_changed = device.bond.is_some() && existing.bond != device.bond;

            existing.last_rssi = device.last_rssi;
            if name_changed {
                existing.name = device.name.clone();
            }
            if bond_changed {
                existing.bond = device.bond;
            }
            if name_changed || bond_changed {
                self.dirty = true;
                info!("Updated existing paired device");
            }
            return;
        }

        // If at capacity, evict the oldest entry.
        if self.devices.is_full() {
            warn!("Paired device store full - evicting oldest entry");
            self.devices.remove(0);
        }

        let _ = self.devices.push(device);
        self.dirty = true;
        info!("Added paired device - now storing {}", self.devices.len());
    }

    /// Iterate paired devices most-recently-added first, for auto-reconnect of
    /// multiple links (e.g. keyboard + mouse) on boot.
    pub fn iter_recent(&self) -> impl Iterator<Item = &PairedDevice> {
        self.devices.iter().rev()
    }

    /// Return all stored BLE bonds.
    pub fn bonds(&self) -> Vec<BondInfo, MAX_PAIRED_DEVICES> {
        let mut bonds = Vec::new();
        for device in &self.devices {
            if let Some(bond) = device.bond {
                let _ = bonds.push(bond);
            }
        }
        bonds
    }

    /// Attach or update a bond for the matching device.
    pub fn set_bond_for_address(&mut self, address: Address, bond: BondInfo) {
        if let Some(device) = self
            .devices
            .iter_mut()
            .find(|d| d.address == address || bond.peer_id.is_match(d.address))
        {
            if device.bond != Some(bond) {
                device.bond = Some(bond);
                self.dirty = true;
                info!("Updated stored BLE bond");
            }
        }
    }
}

/// Global device store (protected by mutex for async access).
pub static DEVICE_STORE: Mutex<CriticalSectionRawMutex, DeviceStore> =
    Mutex::new(DeviceStore::new());
