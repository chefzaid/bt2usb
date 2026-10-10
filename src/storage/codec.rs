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

/// The six address bytes followed by the address type.
pub fn encode_address(address: PeerAddress) -> [u8; ADDRESS_RECORD_SIZE] {
    let [b0, b1, b2, b3, b4, b5] = address.bytes;
    [b0, b1, b2, b3, b4, b5, kind_to_byte(address.kind)]
}

pub fn decode_address(data: &[u8]) -> Option<PeerAddress> {
    let data: &[u8; ADDRESS_RECORD_SIZE] = data.try_into().ok()?;
    let mut bytes = [0u8; 6];
    bytes.copy_from_slice(&data[0..6]);
    Some(PeerAddress::new(byte_to_kind(data[6])?, bytes))
}

/// EDIV, Rand, LTK, the key flags, IRK, and the identity address.
pub fn encode_bond(bond: &StoredBond) -> [u8; BOND_RECORD_SIZE] {
    let mut buf = [0u8; BOND_RECORD_SIZE];
    buf[0..2].copy_from_slice(&bond.ediv.to_le_bytes());
    buf[2..10].copy_from_slice(&bond.rand);
    buf[10..26].copy_from_slice(&bond.ltk);
    buf[26] = bond.flags;
    buf[27..43].copy_from_slice(&bond.irk);
    buf[43..50].copy_from_slice(&encode_address(bond.identity));
    buf
}

pub fn decode_bond(data: &[u8]) -> Option<StoredBond> {
    let data: &[u8; BOND_RECORD_SIZE] = data.try_into().ok()?;
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
        // A reload refuses what a save leaves out (`AddressKind::is_identity`).
        identity: decode_address(&data[43..50]).filter(|address| address.kind.is_identity())?,
    })
}

/// Encode one device record into `buf`; returns its length, or 0 when `buf`
/// is too small.
///
/// Format: `[7 address + type][1 rssi][1 name length][name][1 bond flag]`,
/// then the 50-byte bond when the flag is 1.
pub fn encode_device(device: &StoredDevice, buf: &mut [u8]) -> usize {
    write_device(device, buf).unwrap_or(0)
}

/// [`encode_device`], returning `None` with `buf` unchanged when it is too
/// small.
fn write_device(device: &StoredDevice, buf: &mut [u8]) -> Option<usize> {
    let name = device.name.as_bytes();
    let bond = device.bond.as_ref().map(encode_bond);
    let bond_bytes = bond.as_ref().map_or(&[][..], |bond| &bond[..]);
    let total = ADDRESS_RECORD_SIZE + 2 + name.len() + 1 + bond_bytes.len();
    // Split the whole record off `buf` before writing any of it; the name and
    // bond parts are then exactly as long as the bytes copied into them.
    let (address, rest) = buf
        .get_mut(..total)?
        .split_first_chunk_mut::<ADDRESS_RECORD_SIZE>()?;
    let ([rssi, name_len], rest) = rest.split_first_chunk_mut::<2>()?;
    let (name_buf, rest) = rest.split_at_mut_checked(name.len())?;
    let (flag, bond_buf) = rest.split_first_mut()?;
    *address = encode_address(device.address);
    *rssi = device.last_rssi as u8;
    *name_len = name.len() as u8;
    name_buf.copy_from_slice(name);
    *flag = u8::from(bond.is_some());
    bond_buf.copy_from_slice(bond_bytes);
    Some(total)
}

/// Decode the address, RSSI, and name shared by legacy and versioned records;
/// returns the device without a bond and the offset after the name.
pub fn decode_base(data: &[u8]) -> Option<(StoredDevice, usize)> {
    let (name, end) = record::base(data)?;
    let address = decode_address(data.get(..ADDRESS_RECORD_SIZE)?)?;
    let rssi = *data.get(ADDRESS_RECORD_SIZE)? as i8;
    let mut device = StoredDevice::new(address, "", rssi);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_buffer_too_small_for_its_record_is_left_unchanged() {
        let address = PeerAddress::new(AddressKind::Public, [1; 6]);
        let bond = StoredBond {
            ediv: 1,
            rand: [2; 8],
            ltk: [3; 16],
            flags: 4,
            irk: [5; 16],
            identity: address,
        };
        let mut device = StoredDevice::new(address, "Pad", -40);
        device.bond = Some(bond);
        // One byte short: the record also holds the bond flag.
        let mut buf = [0xEE; ADDRESS_RECORD_SIZE + 2 + 3 + BOND_RECORD_SIZE];
        assert_eq!(encode_device(&device, &mut buf), 0);
        assert_eq!(buf, [0xEE; ADDRESS_RECORD_SIZE + 2 + 3 + BOND_RECORD_SIZE]);
    }
}
