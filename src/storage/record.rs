//! Hardware-free validation of paired-device record boundaries and metadata.

pub const ADDRESS_RECORD_SIZE: usize = 7;
pub const BOND_RECORD_SIZE: usize = 50;

/// Decode the common address/RSSI/name prefix, shared by legacy and v1 blobs.
pub fn base(data: &[u8]) -> Option<(&str, usize)> {
    let name_len = *data.get(ADDRESS_RECORD_SIZE + 1)? as usize;
    if data[ADDRESS_RECORD_SIZE - 1] > 4 || name_len > 32 {
        return None;
    }
    let start = ADDRESS_RECORD_SIZE + 2;
    let end = start + name_len;
    let name = core::str::from_utf8(data.get(start..end)?).ok()?;
    Some((name, end))
}

/// Validate the v1 bond flag and exact record size. The optional returned slice
/// contains the bond bytes; malformed data is distinct from an absent bond.
pub fn bond(data: &[u8], base_end: usize) -> Option<Option<&[u8]>> {
    match data.get(base_end)? {
        0 if data.len() == base_end + 1 => Some(None),
        1 if data.len() == base_end + 1 + BOND_RECORD_SIZE => {
            let bytes = &data[base_end + 1..];
            // An identity address must be public or random static, never an
            // anonymous/private advertising address or an unknown type.
            (bytes[49] <= 1).then_some(Some(bytes))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_name_encoding_capacity_and_base_length() {
        let mut data = [0u8; 41];
        data[8] = 32;
        data[9..].fill(b'a');
        assert_eq!(base(&data).unwrap().0.len(), 32);
        assert!(base(&data[..40]).is_none());
        data[8] = 33;
        assert!(base(&data).is_none());
        data[8] = 1;
        data[9] = 0xFF;
        assert!(base(&data).is_none());
        data[9] = b'a';
        data[6] = 5;
        assert!(base(&data).is_none());
        for end in 0..9 {
            assert!(base(&data[..end]).is_none());
        }
    }

    #[test]
    fn bond_flag_and_record_size_must_agree_exactly() {
        let mut data = [0u8; 10 + BOND_RECORD_SIZE];
        let (_, end) = base(&data).unwrap();
        assert_eq!(bond(&data[..10], end), Some(None));
        assert_eq!(bond(&data[..9], end), None);
        assert_eq!(bond(&data[..11], end), None);
        data[9] = 2;
        assert_eq!(bond(&data[..10], end), None);
        data[9] = 1;
        assert!(bond(&data, end).unwrap().is_some());
        for size in 10..data.len() {
            assert!(bond(&data[..size], end).is_none());
        }
        data[59] = 2;
        assert!(bond(&data, end).is_none());
    }

    #[test]
    fn utf8_names_use_bytes_not_character_counts() {
        let data = [0, 0, 0, 0, 0, 0, 1, 200, 2, 0xC3, 0xA9, 0];
        assert_eq!(base(&data), Some(("é", 11)));
        assert_eq!(bond(&data, 11), Some(None));
    }
}
