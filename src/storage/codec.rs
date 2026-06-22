//! Byte-level wire format for paired-device and bond records in flash.
//!
//! Pure (de)serialization of BLE addresses and bonding keys, kept separate from
//! the persistence logic in `storage.rs`.

use super::BondInfo;
use nrf_softdevice::ble::{Address, AddressType, EncryptionInfo, IdentityKey, MasterId};
use nrf_softdevice::raw;

/// Serialized size of a BLE address: 6 address bytes + 1 address-type byte.
pub(super) const ADDRESS_RECORD_SIZE: usize = 7;
/// Serialized size of a bond record (ediv + rand + ltk + flags + irk + address).
pub(super) const BOND_RECORD_SIZE: usize = 50;

fn address_type_to_byte(address_type: AddressType) -> u8 {
    match address_type {
        AddressType::Public => 0,
        AddressType::RandomStatic => 1,
        AddressType::RandomPrivateResolvable => 2,
        AddressType::RandomPrivateNonResolvable => 3,
        AddressType::Anonymous => 4,
    }
}

fn byte_to_address_type(value: u8) -> AddressType {
    match value {
        0 => AddressType::Public,
        1 => AddressType::RandomStatic,
        2 => AddressType::RandomPrivateResolvable,
        3 => AddressType::RandomPrivateNonResolvable,
        4 => AddressType::Anonymous,
        _ => AddressType::RandomStatic,
    }
}

pub(super) fn serialize_address(address: Address, buf: &mut [u8]) {
    buf[0..6].copy_from_slice(&address.bytes());
    buf[6] = address_type_to_byte(address.address_type());
}

pub(super) fn deserialize_address(data: &[u8]) -> Address {
    let mut bytes = [0u8; 6];
    bytes.copy_from_slice(&data[0..6]);
    Address::new(byte_to_address_type(data[6]), bytes)
}

pub(super) fn serialize_bond(bond: &BondInfo, buf: &mut [u8]) {
    buf[0..2].copy_from_slice(&bond.master_id.ediv.to_le_bytes());
    buf[2..10].copy_from_slice(&bond.master_id.rand);
    buf[10..26].copy_from_slice(&bond.key.ltk);
    buf[26] = bond.key.flags;
    buf[27..43].copy_from_slice(&bond.peer_id.as_raw().id_info.irk);
    serialize_address(bond.peer_id.addr, &mut buf[43..50]);
}

pub(super) fn deserialize_bond(data: &[u8]) -> Option<BondInfo> {
    if data.len() < BOND_RECORD_SIZE {
        return None;
    }

    let mut rand = [0u8; 8];
    rand.copy_from_slice(&data[2..10]);
    let mut ltk = [0u8; 16];
    ltk.copy_from_slice(&data[10..26]);
    let mut irk = [0u8; 16];
    irk.copy_from_slice(&data[27..43]);
    let addr = deserialize_address(&data[43..50]);

    Some(BondInfo {
        master_id: MasterId {
            ediv: u16::from_le_bytes([data[0], data[1]]),
            rand,
        },
        key: EncryptionInfo {
            ltk,
            flags: data[26],
        },
        peer_id: IdentityKey::from_raw(raw::ble_gap_id_key_t {
            id_info: raw::ble_gap_irk_t { irk },
            id_addr_info: *addr.as_raw(),
        }),
    })
}
