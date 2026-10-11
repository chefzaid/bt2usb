//! The bounded device list a user scan builds: which advertisers enter it,
//! which one a newcomer replaces when it is full, and what a later response
//! from a listed device updates.
//!
//! Pure and SoftDevice-free like [`coordinator`](crate::ble::coordinator),
//! whose [`DeviceInfo`] entries it holds, so host tests run it directly (see
//! docs/architecture.md#module-layers-and-dependency-rules). The live scan in
//! `ble::scanner` and the Renode simulation both call [`merge_advertisement`].

use crate::ble::coordinator::DeviceInfo;
use heapless::Vec;

/// The RSSI a controller reports when it has no measurement (Bluetooth Core
/// Specification, Vol 4, Part E, 7.7.65.2).
const RSSI_UNAVAILABLE: i8 = 127;

/// Signal strength for comparing scan entries: higher is stronger, and an
/// unavailable reading ranks below every measured one.
fn signal_rank(rssi: i8) -> i16 {
    if rssi == RSSI_UNAVAILABLE {
        i16::MIN
    } else {
        i16::from(rssi)
    }
}

/// Merge an advertisement or active-scan response into the bounded result
/// list. A later name-only response may update an already identified HID peer,
/// and every response from a listed peer refreshes its RSSI.
///
/// When the list is full, a new HID advertiser replaces the listed device with
/// the weakest signal, but only if it is received more strongly. The list
/// therefore ends a scan holding the `N` strongest HID advertisers instead of
/// the first `N` heard, so distant devices, or a burst of fake advertisers
/// weaker than the device in pairing mode next to the bridge, cannot hide it.
/// Returns `true` only when a new device entered the list.
pub fn merge_advertisement<A: PartialEq, const N: usize>(
    found: &mut Vec<DeviceInfo<A>, N>,
    address: A,
    rssi: i8,
    data: &[u8],
) -> bool {
    use crate::ble::adv_parser::{advertised_name, contains_hid_service_uuid, extract_device_name};

    if let Some(existing) = found.iter_mut().find(|d| d.address == address) {
        if let Some(name) = advertised_name(data) {
            existing.name = name;
        }
        existing.rssi = rssi;
        return false;
    }
    if !contains_hid_service_uuid(data) {
        return false;
    }
    let device = DeviceInfo {
        address,
        name: extract_device_name(data),
        rssi,
    };
    let Err(device) = found.push(device) else {
        return true;
    };
    let weakest = found
        .iter_mut()
        .min_by_key(|listed| signal_rank(listed.rssi));
    match weakest {
        Some(listed) if signal_rank(device.rssi) > signal_rank(listed.rssi) => {
            *listed = device;
            true
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "scan_list_tests.rs"]
mod tests;
