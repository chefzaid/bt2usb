//! Byte-level wire format of the paired-device and bond records in flash.
//!
//! Pure encoding and decoding of [`StoredDevice`], [`PeerAddress`], and
//! [`StoredBond`]; `record` validates the boundaries and `framing` the item
//! that holds the records.

use super::devices::{AddressKind, PeerAddress, StoredBond, StoredDevice};
use super::record;

/// Serialized size of a BLE address: 6 address bytes + 1 address-type byte.
pub use super::record::ADDRESS_RECORD_SIZE;
/// Serialized size of a bond record (ediv + rand + ltk + flags + irk + address).
pub use super::record::BOND_RECORD_SIZE;

/// Largest device record: address, RSSI, name length, a 32-byte name, the
/// bond flag, and a bond.
pub const MAX_DEVICE_RECORD: usize = ADDRESS_RECORD_SIZE + 2 + 32 + 1 + BOND_RECORD_SIZE;

fn kind_to_byte(kind: AddressKind) -> u8 {
    match kind {
        AddressKind::Public => 0,
        AddressKind::RandomStatic => 1,
        AddressKind::RandomPrivateResolvable => 2,
        AddressKind::RandomPrivateNonResolvable => 3,
        AddressKind::Anonymous => 4,
    }
}

fn byte_to_kind(value: u8) -> Option<AddressKind> {
    Some(match value {
        0 => AddressKind::Public,
        1 => AddressKind::RandomStatic,
        2 => AddressKind::RandomPrivateResolvable,
        3 => AddressKind::RandomPrivateNonResolvable,
        4 => AddressKind::Anonymous,
        _ => return None,
    })
}

pub fn encode_address(address: PeerAddress, buf: &mut [u8]) {
    buf[0..6].copy_from_slice(&address.bytes);
    buf[6] = kind_to_byte(address.kind);
}

pub fn decode_address(data: &[u8]) -> Option<PeerAddress> {
    if data.len() != ADDRESS_RECORD_SIZE {
        return None;
    }
    let mut bytes = [0u8; 6];
    bytes.copy_from_slice(&data[0..6]);
    Some(PeerAddress::new(byte_to_kind(data[6])?, bytes))
}

pub fn encode_bond(bond: &StoredBond, buf: &mut [u8]) {
    buf[0..2].copy_from_slice(&bond.ediv.to_le_bytes());
    buf[2..10].copy_from_slice(&bond.rand);
    buf[10..26].copy_from_slice(&bond.ltk);
    buf[26] = bond.flags;
    buf[27..43].copy_from_slice(&bond.irk);
    encode_address(bond.identity, &mut buf[43..50]);
}

pub fn decode_bond(data: &[u8]) -> Option<StoredBond> {
    if data.len() != BOND_RECORD_SIZE {
        return None;
    }
    let mut rand = [0u8; 8];
    rand.copy_from_slice(&data[2..10]);
    let mut ltk = [0u8; 16];
    ltk.copy_from_slice(&data[10..26]);
    let mut irk = [0u8; 16];
    irk.copy_from_slice(&data[27..43]);
    Some(StoredBond {
        ediv: u16::from_le_bytes([data[0], data[1]]),
        rand,
        ltk,
        flags: data[26],
        irk,
        identity: decode_address(&data[43..50])?,
    })
}

/// Encode one device record into `buf`; returns its length, or 0 when `buf`
/// is too small.
///
/// Format: `[7 address + type][1 rssi][1 name length][name][1 bond flag]`,
/// then the 50-byte bond when the flag is 1.
pub fn encode_device(device: &StoredDevice, buf: &mut [u8]) -> usize {
    let name = device.name.as_bytes();
    let base = ADDRESS_RECORD_SIZE + 2 + name.len();
    let total = base
        + 1
        + if device.bond.is_some() {
            BOND_RECORD_SIZE
        } else {
            0
        };
    if buf.len() < total {
        return 0;
    }
    encode_address(device.address, &mut buf[..ADDRESS_RECORD_SIZE]);
    buf[7] = device.last_rssi as u8;
    buf[8] = name.len() as u8;
    buf[9..base].copy_from_slice(name);
    match &device.bond {
        Some(bond) => {
            buf[base] = 1;
            encode_bond(bond, &mut buf[base + 1..total]);
        }
        None => buf[base] = 0,
    }
    total
}

/// Decode the address, RSSI, and name shared by legacy and versioned records;
/// returns the device without a bond and the offset after the name.
pub fn decode_base(data: &[u8]) -> Option<(StoredDevice, usize)> {
    let (name, end) = record::base(data)?;
    let address = decode_address(&data[..ADDRESS_RECORD_SIZE])?;
    let mut device = StoredDevice::new(address, "", data[7] as i8);
    device.name.push_str(name).ok()?;
    Some((device, end))
}

/// Decode one versioned record, which must end exactly after its bond flag
/// or its bond.
pub fn decode_device(data: &[u8]) -> Option<StoredDevice> {
    let (mut device, offset) = decode_base(data)?;
    if let Some(bytes) = record::bond(data, offset)? {
        device.bond = Some(decode_bond(bytes)?);
    }
    Some(device)
}
