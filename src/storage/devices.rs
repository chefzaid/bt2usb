//! Hardware-free paired-device list: identity merge, eviction, forget, factory
//! reset, and the versioned flash item that holds the list.
//!
//! The embedded `storage` shell converts SoftDevice addresses and keys to and
//! from these types, performs the flash I/O, and logs what changed. Resolving
//! a private address with a peer's IRK needs the SoftDevice's AES block, so
//! every method that compares addresses takes that check as a `resolve`
//! function of the IRK and the address bytes.

use heapless::{String, Vec};

use super::{codec, framing};
use crate::config::MAX_PAIRED_DEVICES;

/// Largest flash item the store writes or reads.
pub const MAX_RECORD_SIZE: usize = 512;
// Header, then one length-prefixed record per device at its largest: a full
// store with 32-byte names and bonds always fits.
const _: () = assert!(3 + MAX_PAIRED_DEVICES * (1 + codec::MAX_DEVICE_RECORD) <= MAX_RECORD_SIZE);

/// Whether a resolvable private address (its six bytes) resolves with a
/// peer's IRK. The firmware asks the SoftDevice's AES block; tests pass a fake.
pub trait Resolve: Fn(&[u8; 16], &[u8; 6]) -> bool {}

impl<F: Fn(&[u8; 16], &[u8; 6]) -> bool> Resolve for F {}

/// BLE address type, numbered as the flash record stores it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressKind {
    Public,
    RandomStatic,
    RandomPrivateResolvable,
    RandomPrivateNonResolvable,
    Anonymous,
}

/// A BLE device address as the store keeps it.
#[derive(Clone, Copy, Debug)]
pub struct PeerAddress {
    pub kind: AddressKind,
    /// Address bytes in SoftDevice order, least significant byte first.
    pub bytes: [u8; 6],
    /// The SoftDevice reported this identity address after resolving a private
    /// one. Never stored, and ignored by equality, as in
    /// `nrf_softdevice::ble::Address`.
    pub resolved: bool,
}

impl PartialEq for PeerAddress {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && self.bytes == other.bytes
    }
}

impl Eq for PeerAddress {}

impl PeerAddress {
    pub const fn new(kind: AddressKind, bytes: [u8; 6]) -> Self {
        Self {
            kind,
            bytes,
            resolved: false,
        }
    }
}

/// The keys of one bond: the LTK the peer distributed, with its master ID
/// (EDIV and RAND), and the peer's identity (IRK and identity address).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoredBond {
    pub ediv: u16,
    pub rand: [u8; 8],
    pub ltk: [u8; 16],
    /// `ble_gap_enc_info_t` flags: LE Secure Connections, MITM, key length.
    pub flags: u8,
    pub irk: [u8; 16],
    pub identity: PeerAddress,
}

impl StoredBond {
    /// Whether `address` belongs to this peer, decided as
    /// `IdentityKey::is_match` decides it: a public or static address must
    /// equal the identity address, a resolvable private address must resolve
    /// with the IRK, and any other address never matches.
    pub fn matches(&self, address: PeerAddress, resolve: &impl Resolve) -> bool {
        match address.kind {
            AddressKind::Public | AddressKind::RandomStatic => self.identity == address,
            AddressKind::RandomPrivateResolvable => resolve(&self.irk, &address.bytes),
            AddressKind::RandomPrivateNonResolvable | AddressKind::Anonymous => false,
        }
    }
}

/// One paired device: where to reconnect, what to show, and its bond.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredDevice {
    pub address: PeerAddress,
    pub name: String<32>,
    /// RSSI when the device was last stored; a change to it alone is not
    /// written to flash. Nothing sorts or displays by it yet.
    pub last_rssi: i8,
    pub bond: Option<StoredBond>,
}

impl StoredDevice {
    pub fn new(address: PeerAddress, name: &str, rssi: i8) -> Self {
        Self {
            address,
            name: truncated_name(name),
            last_rssi: rssi,
            bond: None,
        }
    }
}

/// The longest prefix of `name`, in whole characters, that fits 32 bytes.
pub fn truncated_name(name: &str) -> String<32> {
    let mut truncated = String::new();
    for c in name.chars() {
        if truncated.push(c).is_err() {
            break;
        }
    }
    truncated
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    Unreadable,
    Serialization,
    Flash,
    NotFound,
}

/// What [`DeviceList::add`] did, for the shell's log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddOutcome {
    /// Already stored with the same address, name, and keys (RSSI may differ).
    Unchanged,
    /// Already stored; the address, name, or keys changed.
    Updated,
    /// Appended to a list with room.
    Added,
    /// Appended after evicting the oldest-added device from a full list.
    AddedAfterEviction,
}

/// The candidate store a factory reset publishes once persisted, and whether
/// the pairing pages must be erased first because the old item is unreadable.
pub struct Reset {
    pub candidate: DeviceList,
    pub erase_first: bool,
}

/// Paired devices in the order they were added, oldest first, with whether
/// the list differs from flash and whether flash may be overwritten.
#[derive(Clone, Debug)]
pub struct DeviceList {
    devices: Vec<StoredDevice, MAX_PAIRED_DEVICES>,
    dirty: bool,
    /// Cleared when the flash item could not be read or decoded, so a later
    /// save cannot replace bonds that are still in flash (ADR 0006).
    writable: bool,
}

impl Default for DeviceList {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceList {
    pub const fn new() -> Self {
        Self {
            devices: Vec::new(),
            dirty: false,
            writable: true,
        }
    }

    /// Flash holds no item: the store is empty and writable.
    pub fn load_empty(&mut self) {
        self.devices.clear();
        self.writable = true;
        self.dirty = false;
    }

    /// The flash item could not be read: the store is empty, and saves are
    /// refused until a factory reset.
    pub fn load_unreadable(&mut self) {
        self.devices.clear();
        self.writable = false;
        self.dirty = false;
    }

    /// Load the flash item. Returns whether it was valid; an invalid item
    /// leaves the store empty and refusing saves, like an unreadable one.
    pub fn load(&mut self, data: &[u8], resolve: &impl Resolve) -> bool {
        self.devices.clear();
        self.writable = self.decode(data, resolve);
        self.dirty = false;
        self.writable
    }

    /// The item a save must write into `buf`: `Ok(None)` when flash already
    /// matches, the item length otherwise.
    pub fn pending_item(&self, buf: &mut [u8]) -> Result<Option<usize>, StoreError> {
        if !self.dirty {
            return Ok(None);
        }
        if !self.writable {
            return Err(StoreError::Unreadable);
        }
        self.encode(buf).map(Some).ok_or(StoreError::Serialization)
    }

    /// Record that the pending item was written.
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    pub fn is_writable(&self) -> bool {
        self.writable
    }

    pub fn len(&self) -> usize {
        self.devices.len()
    }

    /// The device stored at `address`, or whose bond resolves it.
    pub fn find(&self, address: PeerAddress, resolve: &impl Resolve) -> Option<&StoredDevice> {
        self.devices.iter().find(|device| {
            device.address == address
                || device
                    .bond
                    .is_some_and(|bond| bond.matches(address, resolve))
        })
    }

    /// The store without the device stored at exactly `address`, to publish
    /// once persisted. The current list is left unchanged.
    pub fn without(&self, address: PeerAddress) -> Result<Self, StoreError> {
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
        Ok(candidate)
    }

    /// An empty store to publish once persisted. Only a reset of an
    /// unreadable store erases the pairing pages first.
    pub fn reset(&self) -> Reset {
        let mut candidate = Self::new();
        candidate.dirty = true;
        Reset {
            candidate,
            erase_first: !self.writable,
        }
    }

    /// Store a paired device, merging it with the entry for the same peer.
    pub fn add(&mut self, mut device: StoredDevice, resolve: &impl Resolve) -> AddOutcome {
        // Keep the stable identity address when keys are available. Storing
        // the currently advertised private address would add a new entry on
        // every rotation and eventually evict the other paired peripherals.
        if let Some(bond) = device.bond {
            device.address = bond.identity;
        }
        if let Some(existing) = self.devices.iter_mut().find(|stored| {
            stored.address == device.address
                || stored
                    .bond
                    .is_some_and(|bond| bond.matches(device.address, resolve))
                || device
                    .bond
                    .is_some_and(|bond| bond.matches(stored.address, resolve))
        }) {
            // RSSI changes on every reconnect and is only a hint, so it alone
            // must not cause a flash write.
            let address_changed = existing.address != device.address;
            let name_changed = existing.name != device.name;
            let bond_changed = device.bond.is_some() && existing.bond != device.bond;
            existing.last_rssi = device.last_rssi;
            existing.address = device.address;
            if name_changed {
                existing.name = device.name;
            }
            if bond_changed {
                existing.bond = device.bond;
            }
            if address_changed || name_changed || bond_changed {
                self.dirty = true;
                return AddOutcome::Updated;
            }
            return AddOutcome::Unchanged;
        }
        let evicted = self.devices.is_full();
        if evicted {
            self.devices.remove(0);
        }
        let _ = self.devices.push(device);
        self.dirty = true;
        if evicted {
            AddOutcome::AddedAfterEviction
        } else {
            AddOutcome::Added
        }
    }

    /// Devices newest first, the order boot reconnects them in.
    pub fn iter_recent(&self) -> impl Iterator<Item = &StoredDevice> {
        self.devices.iter().rev()
    }

    /// Every stored bond, oldest device first.
    pub fn bonds(&self) -> impl Iterator<Item = StoredBond> + '_ {
        self.devices.iter().filter_map(|device| device.bond)
    }

    fn encode(&self, buf: &mut [u8]) -> Option<usize> {
        let mut writer = framing::Writer::new(buf)?;
        for device in &self.devices {
            if !writer.push(|slot| codec::encode_device(device, slot)) {
                return None;
            }
        }
        Some(writer.finish())
    }

    fn decode(&mut self, data: &[u8], resolve: &impl Resolve) -> bool {
        let valid = if framing::has_magic(data) {
            self.decode_versioned(data, resolve)
        } else {
            self.decode_legacy(data, resolve)
        };
        if !valid {
            self.devices.clear();
        }
        valid
    }

    /// The versioned format: a complete frame of at most
    /// `MAX_PAIRED_DEVICES` records.
    fn decode_versioned(&mut self, data: &[u8], resolve: &impl Resolve) -> bool {
        if !framing::is_complete(data)
            || framing::declared_count(data).is_none_or(|count| count as usize > MAX_PAIRED_DEVICES)
        {
            return false;
        }
        for record in framing::records(data) {
            let Some(device) = codec::decode_device(record) else {
                return false;
            };
            // Older firmware could store several private addresses of one
            // bonded peer; merging them here keeps both connection slots from
            // reconnecting the same device.
            self.add(device, resolve);
        }
        true
    }

    /// The pre-versioning format: a count byte, then records without a bond.
    /// An empty item lacks even the count byte and is invalid.
    fn decode_legacy(&mut self, data: &[u8], resolve: &impl Resolve) -> bool {
        let Some(&count) = data.first() else {
            return false;
        };
        if count as usize > MAX_PAIRED_DEVICES {
            return false;
        }
        let mut offset = 1;
        for _ in 0..count {
            let Some(&name_len) = data.get(offset + 8) else {
                return false;
            };
            let end = offset + 9 + name_len as usize;
            let Some((device, _)) = data.get(offset..end).and_then(codec::decode_base) else {
                return false;
            };
            self.add(device, resolve);
            offset = end;
        }
        offset == data.len()
    }
}

#[cfg(test)]
#[path = "devices_tests.rs"]
mod tests;
