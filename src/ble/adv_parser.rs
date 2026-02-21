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
            for chunk in uuid_data.chunks_exact(2) {
                if chunk == hid_uuid_le {
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
    let mut i = 0;
    while i < data.len() {
        let len = data[i] as usize;
        if len == 0 || i + len >= data.len() {
            break;
        }
        let ad_type = data[i + 1];
        if ad_type == 0x08 || ad_type == 0x09 {
            let name_bytes = &data[i + 2..i + 1 + len];
            let mut name = String::new();
            for &b in name_bytes {
                if name.push(b as char).is_err() {
                    break;
                }
            }
            return name;
        }
        i += len + 1;
    }

    let mut s = String::new();
    let _ = s.push_str("Unknown");
    s
}
