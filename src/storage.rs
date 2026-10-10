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
//!     length-prefixed record per paired device (see `codec`): address, RSSI,
//!     name, and an optional bond.
//!   - `sequential-storage` manages the flash pages (wear levelling and GC).
//!
//! What to store, merge, evict, or refuse is decided by the host-tested
//! `devices` module on SoftDevice-free types; this shell converts addresses
//! and keys to and from them, does the flash I/O, and logs the outcome.

mod codec;
mod devices;
mod framing;
mod record;

pub use devices::{irk_present, StoreError};
use devices::{
    truncated_name, AddOutcome, AddressKind, DeviceList, PeerAddress, StoredBond, StoredDevice,
    MAX_RECORD_SIZE,
};

use crate::config::{MAX_PAIRED_DEVICES, STORAGE_FLASH_END, STORAGE_FLASH_START};
use crate::sd_setup::FlashBuffer;
use defmt::{debug, error, info, warn};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer};
use heapless::Vec;
use nrf_softdevice::ble::{Address, AddressType, EncryptionInfo, IdentityKey, MasterId};
use nrf_softdevice::raw;
use sequential_storage::cache::NoCache;

/// Key for the paired devices list in the map storage.
const KEY_PAIRED_DEVICES: u8 = 0x01;

/// Retry budget for a flash write that races BLE radio timeslots.
const FLASH_WRITE_ATTEMPTS: u8 = 3;
const FLASH_RETRY_BACKOFF_MS: u64 = 20;

/// [`DeviceStore::add`] kept the device without its bond, or did not keep it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BondRefused;

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
        Self {
            address,
            name: truncated_name(name),
            last_rssi: rssi,
            bond: None,
        }
    }

    fn from_stored(device: &StoredDevice) -> Self {
        Self {
            address: to_address(device.address),
            name: device.name.clone(),
            last_rssi: device.last_rssi,
            bond: device.bond.as_ref().map(to_bond_info),
        }
    }
}

/// The address as the store keeps it, or `None` when its type is reserved.
/// It decodes the raw type rather than call `Address::address_type`, which
/// `unwrap!`s and halts the chip on a reserved type: a peer chooses the type
/// of the identity address it sends during pairing.
fn to_peer(address: Address) -> Option<PeerAddress> {
    Some(PeerAddress {
        kind: AddressKind::from_gap_type(address.as_raw().addr_type())?,
        bytes: address.bytes(),
        resolved: address.is_resolved_peer_id(),
    })
}

fn to_address(address: PeerAddress) -> Address {
    let address_type = match address.kind {
        AddressKind::Public => AddressType::Public,
        AddressKind::RandomStatic => AddressType::RandomStatic,
        AddressKind::RandomPrivateResolvable => AddressType::RandomPrivateResolvable,
        AddressKind::RandomPrivateNonResolvable => AddressType::RandomPrivateNonResolvable,
        AddressKind::Anonymous => AddressType::Anonymous,
    };
    let mut converted = Address::new(address_type, address.bytes);
    converted.flags |= u8::from(address.resolved);
    converted
}

/// The bond as the store keeps it, or `None` when its identity is not a
/// public or random static address, which a reload refuses
/// ([`AddressKind::is_identity`]).
fn to_stored_bond(bond: &BondInfo) -> Option<StoredBond> {
    let identity = to_peer(bond.peer_id.addr).filter(|address| address.kind.is_identity())?;
    Some(StoredBond {
        ediv: bond.master_id.ediv,
        rand: bond.master_id.rand,
        ltk: bond.key.ltk,
        flags: bond.key.flags,
        irk: bond.peer_id.as_raw().id_info.irk,
        identity,
    })
}

fn to_bond_info(bond: &StoredBond) -> BondInfo {
    BondInfo {
        master_id: MasterId {
            ediv: bond.ediv,
            rand: bond.rand,
        },
        key: EncryptionInfo {
            ltk: bond.ltk,
            flags: bond.flags,
        },
        peer_id: IdentityKey::from_raw(raw::ble_gap_id_key_t {
            id_info: raw::ble_gap_irk_t { irk: bond.irk },
            id_addr_info: *to_address(bond.identity).as_raw(),
        }),
    }
}

/// Resolve a private address with a peer's IRK through the SoftDevice's AES
/// block, as `IdentityKey::is_match` does.
fn resolve(irk: &[u8; 16], address: &[u8; 6]) -> bool {
    let key = IdentityKey::from_raw(raw::ble_gap_id_key_t {
        id_info: raw::ble_gap_irk_t { irk: *irk },
        id_addr_info: *Address::new(AddressType::Public, [0; 6]).as_raw(),
    });
    key.is_match(Address::new(AddressType::RandomPrivateResolvable, *address))
}

/// In-memory cache of paired devices, synced with flash. The decisions live
/// in the host-tested [`DeviceList`]; this shell does the flash I/O and logs.
#[derive(Clone)]
pub struct DeviceStore {
    list: DeviceList,
}

impl DeviceStore {
    /// Create an empty store.
    pub const fn new() -> Self {
        Self {
            list: DeviceList::new(),
        }
    }

    /// Async load from flash using sequential-storage.
    pub async fn load_from_flash(
        &mut self,
        flash: &mut impl embedded_storage_async::nor_flash::NorFlash,
    ) {
        let mut buf = FlashBuffer::<MAX_RECORD_SIZE>::new();

        // sequential-storage 7 exposes a stateful `MapStorage` (the standalone
        // `map::fetch_item` free function was removed). It borrows the flash for
        // the duration of the access and is dropped before we return. The
        // range check in `MapConfig::new` runs at compile time.
        let config = const {
            sequential_storage::map::MapConfig::new(STORAGE_FLASH_START..STORAGE_FLASH_END)
        };
        let mut map = sequential_storage::map::MapStorage::<u8, _, _>::new(flash, config, NoCache);

        match map
            .fetch_item::<&[u8]>(&mut buf.0, &KEY_PAIRED_DEVICES)
            .await
        {
            Ok(Some(data)) => {
                if self.list.load(data, &resolve) {
                    info!("Loaded {} devices from flash", self.list.len());
                } else {
                    error!("Invalid or unsupported device store; writes disabled");
                }
            }
            Ok(None) => {
                info!("No paired devices in flash");
                self.list.load_empty();
            }
            Err(e) => {
                error!("Flash read error: {:?}", defmt::Debug2Format(&e));
                self.list.load_unreadable();
            }
        }
    }

    /// Persist all paired devices to flash.
    pub async fn save_to_flash(
        &mut self,
        flash: &mut impl embedded_storage_async::nor_flash::NorFlash,
    ) -> Result<(), StoreError> {
        let mut buf = FlashBuffer::<MAX_RECORD_SIZE>::new();
        let mut data_buf = [0u8; MAX_RECORD_SIZE];

        let len = match self.list.pending_item(&mut data_buf) {
            Ok(Some(len)) => len,
            Ok(None) => {
                debug!("DeviceStore: no changes to save");
                return Ok(());
            }
            Err(StoreError::Unreadable) => {
                error!("Device store is unreadable; refusing to overwrite stored bonds");
                return Err(StoreError::Unreadable);
            }
            Err(e) => {
                error!("Device store exceeds serialization capacity; save aborted");
                return Err(e);
            }
        };
        // `len` is what `pending_item` wrote into `data_buf`, so it fits.
        let Some(item) = data_buf.get(..len) else {
            return Err(StoreError::Serialization);
        };

        let config = const {
            sequential_storage::map::MapConfig::new(STORAGE_FLASH_START..STORAGE_FLASH_END)
        };
        let mut map = sequential_storage::map::MapStorage::<u8, _, _>::new(flash, config, NoCache);

        // SoftDevice flash operations need radio-idle timeslots and can fail with
        // a transient busy/timeout error while BLE links are active (this save
        // runs right at connect time). Retry a few times with a short backoff.
        for attempt in 1..=FLASH_WRITE_ATTEMPTS {
            match map
                .store_item::<&[u8]>(&mut buf.0, &KEY_PAIRED_DEVICES, &item)
                .await
            {
                Ok(_) => {
                    info!("Saved {} devices to flash", self.list.len());
                    self.list.mark_saved();
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

    /// The device stored at `address`, or whose bond resolves it.
    pub fn find(&self, address: Address) -> Option<PairedDevice> {
        self.list
            .find(to_peer(address)?, &resolve)
            .map(PairedDevice::from_stored)
    }

    pub fn is_writable(&self) -> bool {
        self.list.is_writable()
    }

    /// Transactionally remove a stable identity. Failed flash writes leave the
    /// in-memory cache untouched, so the bonder can retain the previous keys.
    pub async fn forget(
        &mut self,
        address: Address,
        flash: &mut impl embedded_storage_async::nor_flash::NorFlash,
    ) -> Result<(), StoreError> {
        let address = to_peer(address).ok_or(StoreError::NotFound)?;
        let candidate = Self {
            list: self.list.without(address)?,
        };
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
        let reset = self.list.reset();
        let candidate = Self {
            list: reset.candidate,
        };
        crate::ble::management::commit(self, candidate, async |next| {
            if reset.erase_first {
                flash
                    .erase(STORAGE_FLASH_START, STORAGE_FLASH_END)
                    .await
                    .map_err(|_| StoreError::Flash)?;
            }
            next.save_to_flash(flash).await
        })
        .await
    }

    /// Add a newly paired device. A bond whose identity is not a public or
    /// random static address is left out and `Err(BondRefused)` returned: a
    /// reload refuses such a bond, and with it the whole store, so saving it
    /// would lose every pairing on the next boot. The device is then stored
    /// without keys, like a peer that did not bond.
    pub fn add(&mut self, device: PairedDevice) -> Result<(), BondRefused> {
        let Some(address) = to_peer(device.address) else {
            // The SoftDevice gives every link and advertiser a defined type,
            // so this does not happen; storing nothing is still safer than a
            // record the next boot would refuse.
            error!("Paired device address has a reserved type; not stored");
            return Err(BondRefused);
        };
        let bond = device.bond.as_ref().map(to_stored_bond);
        let stored = StoredDevice {
            address,
            name: device.name,
            last_rssi: device.last_rssi,
            bond: bond.flatten(),
        };
        match self.list.add(stored, &resolve) {
            AddOutcome::Unchanged => {}
            AddOutcome::Updated => info!("Updated existing paired device"),
            AddOutcome::Added => {
                info!("Added paired device - now storing {}", self.list.len());
            }
            AddOutcome::AddedAfterEviction => {
                warn!("Paired device store full - evicting oldest entry");
                info!("Added paired device - now storing {}", self.list.len());
            }
        }
        if bond == Some(None) {
            warn!("Bond refused: identity address is not public or random static; stored without keys");
            return Err(BondRefused);
        }
        Ok(())
    }

    /// Iterate paired devices most-recently-added first, for auto-reconnect of
    /// multiple links (e.g. keyboard + mouse) on boot.
    pub fn iter_recent(&self) -> impl Iterator<Item = PairedDevice> + '_ {
        self.list.iter_recent().map(PairedDevice::from_stored)
    }

    /// Return all stored BLE bonds.
    pub fn bonds(&self) -> Vec<BondInfo, MAX_PAIRED_DEVICES> {
        self.list.bonds().map(|bond| to_bond_info(&bond)).collect()
    }
}

/// Global device store (protected by mutex for async access).
pub static DEVICE_STORE: Mutex<CriticalSectionRawMutex, DeviceStore> =
    Mutex::new(DeviceStore::new());
