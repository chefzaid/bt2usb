//! Read the advertising data of a scanned peripheral.
//!
//! Advertising and scan-response payloads are a sequence of length-type-value
//! structures (Bluetooth Core Specification, Vol 3, Part C, Section 11). The
//! bridge needs two of them: whether the 16-bit HID service UUID (0x1812) is
//! listed, and the complete or shortened local name. A structure whose length
//! is zero or runs past the buffer ends the walk, so malformed data cannot
//! index out of bounds. The module is pure; the coordinator's scan reducer
//! uses it in the firmware, the Renode build, and the host tests.

use heapless::String;

/// Walk the length-type-value structures, yielding each one's AD type and
/// value. A zero length or a structure that runs past the buffer ends the walk.
fn ad_structures(data: &[u8]) -> impl Iterator<Item = (u8, &[u8])> {
    let mut rest = data;
    core::iter::from_fn(move || {
        let (&len, tail) = rest.split_first()?;
        let (structure, next) = tail.split_at_checked(usize::from(len))?;
        let (&ad_type, value) = structure.split_first()?;
        rest = next;
        Some((ad_type, value))
    })
}

/// Check if raw advertisement data contains the HID Service UUID (0x1812).
pub fn contains_hid_service_uuid(data: &[u8]) -> bool {
    let hid_uuid_le: [u8; 2] = [0x12, 0x18]; // 0x1812 little-endian

    for (ad_type, uuid_data) in ad_structures(data) {
        if ad_type == 0x02 || ad_type == 0x03 {
            for chunk in uuid_data.as_chunks::<2>().0 {
                if *chunk == hid_uuid_le {
                    return true;
                }
            }
        }
    }
    false
}

/// Extract complete/shortened local name from advertisement data.
pub fn extract_device_name(data: &[u8]) -> String<32> {
    advertised_name(data).unwrap_or_else(|| {
        let mut name = String::new();
        let _ = name.push_str("Unknown");
        name
    })
}

/// Read an actual advertised UTF-8 name, preferring a complete local name.
/// Returning `None` lets active scanning distinguish a nameless advertisement
/// from a scan response that can improve a previously discovered device.
pub fn advertised_name(data: &[u8]) -> Option<String<32>> {
    let mut shortened = None;
    for (ad_type, name_bytes) in ad_structures(data) {
        if ad_type == 0x08 || ad_type == 0x09 {
            if let Ok(text) = core::str::from_utf8(name_bytes) {
                let mut name = String::new();
                for c in text.chars() {
                    if name.push(c).is_err() {
                        break;
                    }
                }
                if !name.is_empty() {
                    if ad_type == 0x09 {
                        return Some(name);
                    }
                    shortened = Some(name);
                }
            }
        }
    }

    shortened
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_preserves_utf8_and_truncates_at_character_boundary() {
        let mut ad = heapless::Vec::<u8, 80>::new();
        let text = "é".repeat(17);
        ad.extend_from_slice(&[35, 0x09]).unwrap();
        ad.extend_from_slice(text.as_bytes()).unwrap();
        assert_eq!(extract_device_name(&ad).as_str(), "é".repeat(16));
    }

    #[test]
    fn complete_name_wins_over_shortened_name() {
        let ad = [2, 0x08, b'K', 4, 0x09, b'K', b'B', b'D'];
        assert_eq!(extract_device_name(&ad).as_str(), "KBD");
    }

    #[test]
    fn hid_uuid_is_found_among_other_uuids() {
        // Complete 16-bit UUIDs: Battery (0x180F), HID (0x1812), GATT (0x1801).
        let ad = [0x07, 0x03, 0x0F, 0x18, 0x12, 0x18, 0x01, 0x18];
        assert!(contains_hid_service_uuid(&ad));
    }

    #[test]
    fn hid_uuid_in_an_incomplete_list_counts() {
        // AD type 0x02: Incomplete List of 16-bit Service UUIDs.
        assert!(contains_hid_service_uuid(&[0x03, 0x02, 0x12, 0x18]));
    }

    #[test]
    fn empty_advertisement_has_no_hid_uuid_or_name() {
        assert!(!contains_hid_service_uuid(&[]));
        assert_eq!(extract_device_name(&[]).as_str(), "Unknown");
    }

    #[test]
    fn shortened_name_is_used_when_no_complete_name_is_present() {
        let ad = [0x05, 0x08, b'B', b'T', b' ', b'K'];
        assert_eq!(extract_device_name(&ad).as_str(), "BT K");
    }

    #[test]
    fn zero_length_or_overrunning_structure_ends_the_walk() {
        // A zero length stops the walk before the HID UUID list after it.
        assert!(!contains_hid_service_uuid(&[0x00, 0x03, 0x03, 0x12, 0x18]));
        // The UUID list claims five bytes but only three follow its length.
        assert!(!contains_hid_service_uuid(&[0x05, 0x03, 0x12, 0x18]));
        assert!(advertised_name(&[0x04, 0x09, b'K', b'B']).is_none());
        // Structures before the malformed one are still read.
        assert!(contains_hid_service_uuid(&[0x03, 0x03, 0x12, 0x18, 0x09]));
        let ad = [0x02, 0x09, b'K', 0x07, 0x08];
        assert_eq!(extract_device_name(&ad).as_str(), "K");
    }

    #[test]
    fn missing_empty_or_invalid_utf8_name_does_not_replace_known_name() {
        assert!(advertised_name(&[2, 0x01, 6]).is_none());
        assert!(advertised_name(&[1, 0x09]).is_none());
        assert!(advertised_name(&[2, 0x09, 0xFF]).is_none());
        assert_eq!(extract_device_name(&[2, 0x09, 0xFF]).as_str(), "Unknown");
    }
}
