//! BLE GAP scanner - discovers nearby peripherals.
//!
//! Uses the SoftDevice Central-role scanning API.  Discovered devices
//! are filtered by the presence of the HID Service UUID (0x1812) in
//! their advertisement data, then pushed into the UI event channel.

use crate::ble::adv_parser::{contains_hid_service_uuid, extract_device_name};
use crate::ble::{BleErrorTag, BleEvent, DiscoveredDevice};
use crate::config::{BLE_MAX_DISCOVERED, BLE_SCAN_DURATION_SECS};
use defmt::info;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Sender;
use embassy_time::Duration;
use heapless::Vec;
use nrf_softdevice::ble::central;
use nrf_softdevice::Softdevice;

/// Result of a single scan pass.
pub struct ScanResult {
    pub devices: Vec<DiscoveredDevice, BLE_MAX_DISCOVERED>,
}

/// Run a BLE scan for `BLE_SCAN_DURATION_SECS` seconds.
///
/// Discovered HID peripherals are sent to `event_tx` as `BleEvent::DeviceFound`
/// in real time.  Once the scan window closes, `BleEvent::ScanComplete` is sent.
///
/// Returns the accumulated list so the connection manager can index into it.
pub async fn scan(
    sd: &Softdevice,
    event_tx: &Sender<'_, CriticalSectionRawMutex, BleEvent, 8>,
) -> Result<ScanResult, BleErrorTag> {
    info!("BLE scan starting ({} s window)", BLE_SCAN_DURATION_SECS);
    event_tx.send(BleEvent::ScanStarted).await;

    let mut found: Vec<DiscoveredDevice, BLE_MAX_DISCOVERED> = Vec::new();

    let config = central::ScanConfig {
        // Active scan to retrieve scan-response data (device names).
        active: true,
        ..Default::default()
    };

    // We set up a deadline so the scan doesn't run forever.
    let deadline = embassy_time::Instant::now() + Duration::from_secs(BLE_SCAN_DURATION_SECS);

    // The SoftDevice scan callback receives each advertisement.
    // We use a closure that captures our state.
    let scan_result = central::scan(sd, &config, |params| {
        let data =
            unsafe { core::slice::from_raw_parts(params.data.p_data, params.data.len as usize) };

        // Check if we've exceeded our time budget.
        if embassy_time::Instant::now() > deadline {
            return Some(()); // Signal scan to stop
        }

        // Parse advertisement data looking for the HID Service UUID.
        let has_hid_service = contains_hid_service_uuid(data);

        if has_hid_service {
            // Extract device name from advertisement or scan response.
            let name = extract_device_name(data);

            let device = DiscoveredDevice {
                address: nrf_softdevice::ble::Address::from_raw(params.peer_addr),
                name,
                rssi: params.rssi,
            };

            // Avoid duplicates (same address).
            let already_seen = found.iter().any(|d| d.address == device.address);
            if !already_seen && !found.is_full() {
                info!("Found: {} (RSSI {})", device.name.as_str(), device.rssi);
                // We can't await inside this closure, so the event_tx send
                // happens after scan completes.  Instead, buffer here.
                let _ = found.push(device);
            }
        }

        // Return None to keep scanning, Some(()) to stop.
        if found.is_full() {
            Some(()) // Buffer full - stop early
        } else {
            None
        }
    })
    .await;

    if let Err(_e) = scan_result {
        defmt::warn!("BLE scan ended with error");
        event_tx
            .send(BleEvent::Error(BleErrorTag::ScanFailed))
            .await;
        return Err(BleErrorTag::ScanFailed);
    }

    // Now send all found devices to the UI.
    for device in found.iter() {
        event_tx.send(BleEvent::DeviceFound(device.clone())).await;
    }
    event_tx.send(BleEvent::ScanComplete).await;

    info!("BLE scan complete - {} devices found", found.len());

    Ok(ScanResult { devices: found })
}

// ═══════════════════════════════════════════════════════════════════════════
// Unit Tests (run on host, not embedded)
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_hid_uuid_in_advertisement() {
        // AD structure: len=3, type=0x03 (Complete 16-bit UUIDs), UUID=0x1812
        let ad_data = [
            0x03, 0x03, 0x12, 0x18, // HID Service UUID in little-endian
        ];
        assert!(contains_hid_service_uuid(&ad_data));
    }

    #[test]
    fn no_hid_uuid_in_advertisement() {
        // AD structure with Battery Service UUID (0x180F) instead
        let ad_data = [
            0x03, 0x03, 0x0F, 0x18, // Battery Service UUID
        ];
        assert!(!contains_hid_service_uuid(&ad_data));
    }

    #[test]
    fn hid_uuid_among_multiple_uuids() {
        // Multiple 16-bit UUIDs: 0x180F (Battery), 0x1812 (HID), 0x1801 (GATT)
        let ad_data = [
            0x07, 0x03, // len=7, type=0x03 (Complete 16-bit UUIDs)
            0x0F, 0x18, // Battery
            0x12, 0x18, // HID - this should be found
            0x01, 0x18, // GATT
        ];
        assert!(contains_hid_service_uuid(&ad_data));
    }

    #[test]
    fn incomplete_uuid_list() {
        // AD type 0x02 = Incomplete 16-bit UUIDs (should still be checked)
        let ad_data = [
            0x03, 0x02, 0x12, 0x18, // HID Service UUID
        ];
        assert!(contains_hid_service_uuid(&ad_data));
    }

    #[test]
    fn empty_advertisement_data() {
        let ad_data: [u8; 0] = [];
        assert!(!contains_hid_service_uuid(&ad_data));
    }

    #[test]
    fn malformed_ad_length_zero() {
        let ad_data = [0x00]; // len=0 should break parsing
        assert!(!contains_hid_service_uuid(&ad_data));
    }

    #[test]
    fn extract_complete_local_name() {
        // AD structure: len=8, type=0x09 (Complete Local Name), "Keyboard"
        let ad_data = [
            0x09, 0x09, // len=9, type=0x09
            b'K', b'e', b'y', b'b', b'o', b'a', b'r', b'd',
        ];
        let name = extract_device_name(&ad_data);
        assert_eq!(name.as_str(), "Keyboard");
    }

    #[test]
    fn extract_shortened_local_name() {
        // AD structure: len=4, type=0x08 (Shortened Local Name), "BT K"
        let ad_data = [
            0x05, 0x08, // len=5, type=0x08
            b'B', b'T', b' ', b'K',
        ];
        let name = extract_device_name(&ad_data);
        assert_eq!(name.as_str(), "BT K");
    }

    #[test]
    fn no_name_in_advertisement() {
        // Only flags, no name
        let ad_data = [
            0x02, 0x01, 0x06, // Flags: LE General Discoverable
        ];
        let name = extract_device_name(&ad_data);
        assert_eq!(name.as_str(), "Unknown");
    }

    #[test]
    fn name_truncated_to_32_chars() {
        // Very long name that exceeds 32 characters
        let mut ad_data = [0u8; 40];
        ad_data[0] = 35; // len
        ad_data[1] = 0x09; // Complete Local Name
        for i in 2..37 {
            ad_data[i] = b'X';
        }
        let name = extract_device_name(&ad_data);
        assert_eq!(name.len(), 32); // Truncated to heapless::String<32> capacity
    }
}
