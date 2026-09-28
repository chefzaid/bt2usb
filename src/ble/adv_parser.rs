use heapless::String;

/// Check if raw advertisement data contains the HID Service UUID (0x1812).
pub fn contains_hid_service_uuid(data: &[u8]) -> bool {
    let hid_uuid_le: [u8; 2] = [0x12, 0x18]; // 0x1812 little-endian

    let mut i = 0;
    while i < data.len() {
        let len = data[i] as usize;
        if len == 0 || i + len >= data.len() {
            break;
        }
        let ad_type = data[i + 1];
        if ad_type == 0x02 || ad_type == 0x03 {
            let uuid_data = &data[i + 2..i + 1 + len];
            for chunk in uuid_data.as_chunks::<2>().0 {
                if *chunk == hid_uuid_le {
                    return true;
                }
            }
        }
        i += len + 1;
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
    let mut i = 0;
    while i < data.len() {
        let len = data[i] as usize;
        if len == 0 || i + len >= data.len() {
            break;
        }
        let ad_type = data[i + 1];
        if ad_type == 0x08 || ad_type == 0x09 {
            let name_bytes = &data[i + 2..i + 1 + len];
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
        i += len + 1;
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
    fn missing_empty_or_invalid_utf8_name_does_not_replace_known_name() {
        assert!(advertised_name(&[2, 0x01, 6]).is_none());
        assert!(advertised_name(&[1, 0x09]).is_none());
        assert!(advertised_name(&[2, 0x09, 0xFF]).is_none());
        assert_eq!(extract_device_name(&[2, 0x09, 0xFF]).as_str(), "Unknown");
    }
}
