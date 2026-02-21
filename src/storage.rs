//! Persistent storage for paired device addresses.
//!
//! Uses the nRF52840's internal flash via `sequential-storage` crate
//! to store BLE addresses of previously paired devices so they can
//! be auto-reconnected on power-up.
//!
//! Storage layout:
//!   - Each record is a serialized `PairedDeviceRecord` (key-value map).
//!   - Records are appended sequentially; the flash pages are managed
//!     by `sequential-storage` which handles wear levelling and GC.

use crate::config::{MAX_PAIRED_DEVICES, STORAGE_FLASH_PAGE_COUNT, STORAGE_FLASH_PAGE_START};
use defmt::{debug, error, info, warn};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use heapless::Vec;
use nrf_softdevice::ble::Address;

/// Flash page size for nRF52840 (4 KB).
const FLASH_PAGE_SIZE: u32 = 4096;

/// Start address of our storage region.
const STORAGE_START: u32 = STORAGE_FLASH_PAGE_START * FLASH_PAGE_SIZE;

/// End address (exclusive) of our storage region.
const STORAGE_END: u32 = (STORAGE_FLASH_PAGE_START + STORAGE_FLASH_PAGE_COUNT) * FLASH_PAGE_SIZE;

/// Key for the paired devices list in the map storage.
const KEY_PAIRED_DEVICES: u8 = 0x01;

/// Maximum serialized size for paired device records.
/// 4 devices × (6 addr + 1 type + 1 name_len + 32 name) = 160 bytes max.
const MAX_RECORD_SIZE: usize = 256;

/// A paired device record stored in flash.
#[derive(Clone, Debug)]
pub struct PairedDevice {
    /// BLE address (6 bytes + 1 address type byte).
    pub address: Address,
    /// Device name (for UI display, truncated to 32 bytes).
    pub name: heapless::String<32>,
    /// Last RSSI seen (for sorting by signal strength).
    pub last_rssi: i8,
}

impl PairedDevice {
    /// Create a new paired device record.
    pub fn new(address: Address, name: &str, rssi: i8) -> Self {
        let mut n: heapless::String<32> = heapless::String::new();
        // Truncate name if too long.
        for c in name.chars().take(31) {
            let _ = n.push(c);
        }
        Self {
            address,
            name: n,
            last_rssi: rssi,
        }
    }

    /// Serialize to bytes for flash storage.
    fn serialize(&self, buf: &mut [u8]) -> usize {
        let addr_bytes = self.address.bytes();
        let addr_type = match self.address.address_type() {
            nrf_softdevice::ble::AddressType::Public => 0u8,
            nrf_softdevice::ble::AddressType::RandomStatic => 1u8,
            nrf_softdevice::ble::AddressType::RandomPrivateResolvable => 2u8,
            nrf_softdevice::ble::AddressType::RandomPrivateNonResolvable => 3u8,
            nrf_softdevice::ble::AddressType::Anonymous => 4u8,
        };
        let name_bytes = self.name.as_bytes();
        let name_len = name_bytes.len() as u8;

        // Format: [6 addr][1 type][1 rssi][1 name_len][name_bytes...]
        let total = 6 + 1 + 1 + 1 + name_bytes.len();
        if buf.len() < total {
            return 0;
        }

        buf[0..6].copy_from_slice(&addr_bytes);
        buf[6] = addr_type;
        buf[7] = self.last_rssi as u8;
        buf[8] = name_len;
        buf[9..9 + name_bytes.len()].copy_from_slice(name_bytes);
        total
    }

    /// Deserialize from bytes.
    fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 9 {
            return None;
        }

        let mut addr_bytes = [0u8; 6];
        addr_bytes.copy_from_slice(&data[0..6]);
        let addr_type = match data[6] {
            0 => nrf_softdevice::ble::AddressType::Public,
            1 => nrf_softdevice::ble::AddressType::RandomStatic,
            2 => nrf_softdevice::ble::AddressType::RandomPrivateResolvable,
            3 => nrf_softdevice::ble::AddressType::RandomPrivateNonResolvable,
            4 => nrf_softdevice::ble::AddressType::Anonymous,
            _ => nrf_softdevice::ble::AddressType::RandomStatic, // default fallback
        };
        let rssi = data[7] as i8;
        let name_len = data[8] as usize;

        if data.len() < 9 + name_len {
            return None;
        }

        let name_slice = &data[9..9 + name_len];
        let mut name: heapless::String<32> = heapless::String::new();
        if let Ok(s) = core::str::from_utf8(name_slice) {
            for c in s.chars().take(31) {
                let _ = name.push(c);
            }
        }

        Some(Self {
            address: Address::new(addr_type, addr_bytes),
            name,
            last_rssi: rssi,
        })
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
        let flash_range = STORAGE_START..STORAGE_END;
        let mut buf = [0u8; MAX_RECORD_SIZE];

        match sequential_storage::map::fetch_item::<u8, &[u8], _>(
            flash,
            flash_range,
            &mut sequential_storage::cache::NoCache::new(),
            &mut buf,
            &KEY_PAIRED_DEVICES,
        )
        .await
        {
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

        let flash_range = STORAGE_START..STORAGE_END;
        let mut buf = [0u8; MAX_RECORD_SIZE];
        let mut data_buf = [0u8; MAX_RECORD_SIZE];

        let len = self.serialize_all(&mut data_buf);
        let item = &data_buf[..len];

        match sequential_storage::map::store_item::<u8, &[u8], _>(
            flash,
            flash_range,
            &mut sequential_storage::cache::NoCache::new(),
            &mut buf,
            &KEY_PAIRED_DEVICES,
            &item,
        )
        .await
        {
            Ok(_) => {
                info!("Saved {} devices to flash", self.devices.len());
                self.dirty = false;
            }
            Err(e) => {
                error!("Flash write error: {:?}", defmt::Debug2Format(&e));
            }
        }
    }

    /// Serialize all devices to a byte buffer.
    fn serialize_all(&self, buf: &mut [u8]) -> usize {
        let mut offset = 0;

        // First byte: device count.
        buf[0] = self.devices.len() as u8;
        offset += 1;

        for device in &self.devices {
            let written = device.serialize(&mut buf[offset..]);
            offset += written;
        }

        offset
    }

    /// Deserialize all devices from a byte buffer.
    fn deserialize_all(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }

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

            if let Some(device) = PairedDevice::deserialize(&data[offset..offset + record_len]) {
                if !self.devices.is_full() {
                    let _ = self.devices.push(device);
                }
            }

            offset += record_len;
        }
    }

    /// Add a newly paired device.
    pub fn add(&mut self, device: PairedDevice) {
        // If already stored (same address), update the record.
        if let Some(existing) = self
            .devices
            .iter_mut()
            .find(|d| d.address == device.address)
        {
            existing.name = device.name.clone();
            existing.last_rssi = device.last_rssi;
            self.dirty = true;
            info!("Updated existing paired device");
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

    /// Get the first (most recently used) paired device for auto-reconnect.
    pub fn first(&self) -> Option<&PairedDevice> {
        self.devices.last() // Last added = most recent
    }
}

/// Global device store (protected by mutex for async access).
pub static DEVICE_STORE: Mutex<CriticalSectionRawMutex, DeviceStore> =
    Mutex::new(DeviceStore::new());
