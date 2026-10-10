//! Persistent storage for paired devices and BLE bonding keys.
//!
//! Uses the nRF52840's internal flash via `sequential-storage` crate
//! to store BLE addresses, display names, RSSI hints, and bonding keys
//! for previously paired devices so they can be auto-reconnected on power-up.
//!
//! Storage layout:
//!   - The whole store is one `sequential-storage` map item under
//!     `KEY_PAIRED_DEVICES`, rewritten in full on each save.
//!   - That item is a versioned frame (see `framing`) holding one
//!     length-prefixed record per paired device: a serialized `PairedDevice`
//!     with optional `BondInfo`.
//!   - `sequential-storage` manages the flash pages (wear levelling and GC).

mod codec;
mod framing;
mod record;

use codec::{
    deserialize_address, deserialize_bond, serialize_address, serialize_bond, ADDRESS_RECORD_SIZE,
    BOND_RECORD_SIZE,
};

use crate::config::{MAX_PAIRED_DEVICES, STORAGE_FLASH_END, STORAGE_FLASH_START};
use defmt::{debug, error, info, warn};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer};
use heapless::Vec;
use nrf_softdevice::ble::{Address, EncryptionInfo, IdentityKey, MasterId};
use sequential_storage::cache::NoCache;

/// Key for the paired devices list in the map storage.
const KEY_PAIRED_DEVICES: u8 = 0x01;

// Versioned multi-record framing (magic/version/length prefixes) lives in
// `framing`; per-record wire sizes (ADDRESS_RECORD_SIZE, BOND_RECORD_SIZE) in `codec`.

/// Retry budget for a flash write that races BLE radio timeslots.
const FLASH_WRITE_ATTEMPTS: u8 = 3;
const FLASH_RETRY_BACKOFF_MS: u64 = 20;

/// Maximum serialized size for paired device records.
/// 4 devices × (address/name metadata + BLE bond keys) plus versioning overhead.
const MAX_RECORD_SIZE: usize = 512;
const _: () =
    assert!(3 + MAX_PAIRED_DEVICES * (1 + 9 + 32 + 1 + BOND_RECORD_SIZE) <= MAX_RECORD_SIZE);

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
    /// RSSI the device had when it was last stored (a change to it alone does
    /// not cause a flash write). Nothing sorts or displays by it yet.
    pub last_rssi: i8,
    /// BLE bonding keys for reconnecting without pairing again.
    pub bond: Option<BondInfo>,
}

impl PairedDevice {
    /// Create a new paired device record.
    pub fn new(address: Address, name: &str, rssi: i8) -> Self {
        let mut n: heapless::String<32> = heapless::String::new();
        // Truncate name to fit heapless::String<32> capacity.
        for c in name.chars() {
            if n.push(c).is_err() {
                break;
            }
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
        let (text, end) = record::base(data)?;
        let address = deserialize_address(&data[..ADDRESS_RECORD_SIZE])?;
        let rssi = data[7] as i8;
        let mut name: heapless::String<32> = heapless::String::new();
        name.push_str(text).ok()?;

        Some((
            Self {
                address,
                name,
                last_rssi: rssi,
                bond: None,
            },
            end,
        ))
    }

    /// Deserialize a versioned record from bytes.
    fn deserialize(data: &[u8]) -> Option<Self> {
        let (mut device, offset) = Self::deserialize_base(data)?;
        if let Some(bytes) = record::bond(data, offset)? {
            device.bond = Some(deserialize_bond(bytes)?);
        }
        Some(device)
    }
}

/// In-memory cache of paired devices, synced with flash.
#[derive(Clone)]
pub struct DeviceStore {
    /// Cached list of paired devices.
    devices: Vec<PairedDevice, MAX_PAIRED_DEVICES>,
    /// Dirty flag - true if cache differs from flash.
    dirty: bool,
    /// Preserve unreadable or unsupported data until an explicit recovery flow
    /// exists, rather than silently overwriting bonds after a failed load.
    writable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    Unreadable,
    Serialization,
    Flash,
    NotFound,
}

impl DeviceStore {
    /// Create an empty store.
    pub const fn new() -> Self {
        Self {
            devices: Vec::new(),
            dirty: false,
            writable: true,
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
        let config =
            sequential_storage::map::MapConfig::new(STORAGE_FLASH_START..STORAGE_FLASH_END);
        let mut map = sequential_storage::map::MapStorage::<u8, _, _>::new(flash, config, NoCache);

        match map.fetch_item::<&[u8]>(&mut buf, &KEY_PAIRED_DEVICES).await {
            Ok(Some(data)) => {
                self.devices.clear();
                self.writable = self.deserialize_all(data);
                if self.writable {
                    info!("Loaded {} devices from flash", self.devices.len());
                } else {
                    error!("Invalid or unsupported device store; writes disabled");
                }
            }
            Ok(None) => {
                info!("No paired devices in flash");
                self.devices.clear();
                self.writable = true;
            }
            Err(e) => {
                error!("Flash read error: {:?}", defmt::Debug2Format(&e));
                self.devices.clear();
                self.writable = false;
            }
        }
        self.dirty = false;
    }

    /// Persist all paired devices to flash.
    pub async fn save_to_flash(
        &mut self,
        flash: &mut impl embedded_storage_async::nor_flash::NorFlash,
    ) -> Result<(), StoreError> {
        if !self.dirty {
            debug!("DeviceStore: no changes to save");
            return Ok(());
        }
        if !self.writable {
            error!("Device store is unreadable; refusing to overwrite stored bonds");
            return Err(StoreError::Unreadable);
        }

        let mut buf = [0u8; MAX_RECORD_SIZE];
        let mut data_buf = [0u8; MAX_RECORD_SIZE];

        let Some(len) = self.serialize_all(&mut data_buf) else {
            error!("Device store exceeds serialization capacity; save aborted");
            return Err(StoreError::Serialization);
        };
        let item: &[u8] = &data_buf[..len];

        let config =
            sequential_storage::map::MapConfig::new(STORAGE_FLASH_START..STORAGE_FLASH_END);
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
                    return Ok(());
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
        Err(StoreError::Flash)
    }

    pub fn find(&self, address: Address) -> Option<&PairedDevice> {
        self.devices.iter().find(|device| {
            device.address == address
                || device
                    .bond
                    .is_some_and(|bond| bond.peer_id.is_match(address))
        })
    }

    pub fn is_writable(&self) -> bool {
        self.writable
    }

    /// Transactionally remove a stable identity. Failed flash writes leave the
    /// in-memory cache untouched, so the bonder can retain the previous keys.
    pub async fn forget(
        &mut self,
        address: Address,
        flash: &mut impl embedded_storage_async::nor_flash::NorFlash,
    ) -> Result<(), StoreError> {
        if !self.writable {
            return Err(StoreError::Unreadable);
        }
        let index = self
            .devices
            .iter()
            .position(|device| device.address == address)
            .ok_or(StoreError::NotFound)?;
        let mut candidate = self.clone();
        candidate.devices.remove(index);
        candidate.dirty = true;
        crate::ble::management::commit(self, candidate, async |next| {
            next.save_to_flash(flash).await
        })
        .await
    }

    /// Reset a readable store with the same atomic append as ordinary changes.
    /// Only an explicitly requested reset may erase an unreadable storage area.
    pub async fn factory_reset(
        &mut self,
        flash: &mut impl embedded_storage_async::nor_flash::NorFlash,
    ) -> Result<(), StoreError> {
        let recover = !self.writable;
        let mut candidate = Self::new();
        candidate.dirty = true;
        crate::ble::management::commit(self, candidate, async |next| {
            if recover {
                flash
                    .erase(STORAGE_FLASH_START, STORAGE_FLASH_END)
                    .await
                    .map_err(|_| StoreError::Flash)?;
            }
            next.save_to_flash(flash).await
        })
        .await
    }

    /// Serialize all devices to a byte buffer using the versioned framing.
    fn serialize_all(&self, buf: &mut [u8]) -> Option<usize> {
        let mut writer = framing::Writer::new(buf)?;
        for device in &self.devices {
            if !writer.push(|slot| device.serialize(slot)) {
                return None;
            }
        }
        Some(writer.finish())
    }

    /// Deserialize all devices from a byte buffer.
    fn deserialize_all(&mut self, data: &[u8]) -> bool {
        if data.is_empty() {
            return false;
        }

        let valid = if framing::has_magic(data) {
            framing::is_complete(data) && self.deserialize_versioned(data)
        } else {
            self.deserialize_legacy(data)
        };
        if !valid {
            self.devices.clear();
        }
        valid
    }

    fn deserialize_versioned(&mut self, data: &[u8]) -> bool {
        if data[2] as usize > MAX_PAIRED_DEVICES {
            return false;
        }
        for record in framing::records(data) {
            let Some(device) = PairedDevice::deserialize(record) else {
                return false;
            };
            // Older firmware could store multiple RPAs for the same bonded
            // peer. Normalize and merge them while loading, otherwise both
            // connection slots would attempt to reconnect the same device.
            self.add(device);
        }
        true
    }

    fn deserialize_legacy(&mut self, data: &[u8]) -> bool {
        let count = data[0] as usize;
        if count > MAX_PAIRED_DEVICES {
            return false;
        }
        let mut offset = 1;

        for _ in 0..count {
            if offset >= data.len() {
                return false;
            }

            // Read name length to determine record size.
            if offset + 9 > data.len() {
                return false;
            }
            let name_len = data[offset + 8] as usize;
            let record_len = 9 + name_len;

            if offset + record_len > data.len() {
                return false;
            }

            let Some((device, _)) =
                PairedDevice::deserialize_base(&data[offset..offset + record_len])
            else {
                return false;
            };
            self.add(device);

            offset += record_len;
        }
        offset == data.len()
    }

    /// Add a newly paired device.
    pub fn add(&mut self, mut device: PairedDevice) {
        // Persist a stable identity address when keys are available. Saving the
        // currently advertised RPA would create a new entry on every rotation
        // and eventually evict the other paired peripherals.
        if let Some(bond) = device.bond {
            device.address = bond.peer_id.addr;
        }
        // If already stored (same address), update the record. Only persist
        // (mark dirty) when something we care about for reconnect actually
        // changed — RSSI churns on every reconnect and is just a UI hint, so
        // updating it alone must not cause a flash write (avoidable wear).
        if let Some(existing) = self.devices.iter_mut().find(|d| {
            d.address == device.address
                || d.bond.is_some_and(|b| b.peer_id.is_match(device.address))
                || device.bond.is_some_and(|b| b.peer_id.is_match(d.address))
        }) {
            let address_changed = existing.address != device.address;
            let name_changed = existing.name != device.name;
            let bond_changed = device.bond.is_some() && existing.bond != device.bond;

            existing.last_rssi = device.last_rssi;
            existing.address = device.address;
            if name_changed {
                existing.name = device.name.clone();
            }
            if bond_changed {
                existing.bond = device.bond;
            }
            if address_changed || name_changed || bond_changed {
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
}

/// Global device store (protected by mutex for async access).
pub static DEVICE_STORE: Mutex<CriticalSectionRawMutex, DeviceStore> =
    Mutex::new(DeviceStore::new());
