//! Host tests for `merge_advertisement`: scan responses that update or enroll
//! a device, and the strongest-first list in a crowded scan.

use super::*;

// Trivial stand-in for the embedded `Address` type.
type Addr = u8;

#[test]
fn name_only_scan_response_updates_known_hid_even_when_list_is_full() {
    let mut found = heapless::Vec::<_, 1>::new();
    assert!(merge_advertisement(
        &mut found,
        1u8,
        -60,
        &[3, 3, 0x12, 0x18]
    ));
    assert_eq!(found[0].name.as_str(), "Unknown");
    assert!(!merge_advertisement(
        &mut found,
        1,
        -50,
        &[3, 9, b'K', b'B']
    ));
    assert_eq!(found[0].name.as_str(), "KB");
    assert_eq!(found[0].rssi, -50);
    assert!(!merge_advertisement(
        &mut found,
        2,
        -70,
        &[3, 3, 0x12, 0x18]
    ));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].address, 1);
    assert!(!merge_advertisement(&mut found, 1, -40, &[2, 1, 6]));
    assert_eq!(found[0].name.as_str(), "KB");
}

#[test]
fn name_without_hid_uuid_cannot_enroll_an_unknown_device() {
    let mut found = heapless::Vec::<_, 2>::new();
    assert!(!merge_advertisement(
        &mut found,
        1u8,
        -20,
        &[3, 9, b'K', b'B']
    ));
    assert!(found.is_empty());
}

// ── Crowded scans ──────────────────────────────────────────────────────

/// An advertisement carrying only the HID service UUID.
const HID_ADV: &[u8] = &[3, 3, 0x12, 0x18];

fn addresses<const N: usize>(found: &heapless::Vec<DeviceInfo<Addr>, N>) -> Vec<Addr, N> {
    let mut list: Vec<Addr, N> = found.iter().map(|d| d.address).collect();
    list.sort_unstable();
    list
}

#[test]
fn crowded_scan_still_lists_the_device_next_to_the_bridge() {
    const N: usize = crate::config::BLE_MAX_DISCOVERED;
    let mut found = heapless::Vec::<_, N>::new();
    // Twenty HID advertisers, from -70 dBm down to -89 dBm, fill the list
    // before the keyboard in pairing mode is heard.
    for address in 0..20u8 {
        merge_advertisement(&mut found, address, -70 - address as i8, HID_ADV);
    }
    assert_eq!(found.len(), N);
    let keyboard = [3, 3, 0x12, 0x18, 3, 9, b'K', b'B'];
    assert!(merge_advertisement(&mut found, 100, -40, &keyboard));
    // The distant devices keep advertising; none displaces the keyboard.
    for address in 0..20u8 {
        merge_advertisement(&mut found, address, -70 - address as i8, HID_ADV);
    }
    let keyboard = found.iter().find(|d| d.address == 100).expect("listed");
    assert_eq!(keyboard.name.as_str(), "KB");
    assert_eq!(addresses(&found).as_slice(), &[0, 1, 2, 3, 4, 5, 6, 100]);
}

#[test]
fn only_a_stronger_newcomer_replaces_the_weakest_entry() {
    let mut found = heapless::Vec::<_, 2>::new();
    assert!(merge_advertisement(&mut found, 1u8, -50, HID_ADV));
    assert!(merge_advertisement(&mut found, 2, -60, HID_ADV));
    assert!(!merge_advertisement(&mut found, 3, -60, HID_ADV));
    assert!(!merge_advertisement(&mut found, 3, -75, HID_ADV));
    assert_eq!(addresses(&found).as_slice(), &[1, 2]);
    assert!(merge_advertisement(&mut found, 3, -55, HID_ADV));
    assert_eq!(addresses(&found).as_slice(), &[1, 3]);
}

#[test]
fn latest_rssi_decides_which_entry_is_weakest() {
    let mut found = heapless::Vec::<_, 2>::new();
    merge_advertisement(&mut found, 1u8, -40, HID_ADV);
    merge_advertisement(&mut found, 2, -60, HID_ADV);
    // Device 1 moves away, so its later advertisements arrive weaker.
    assert!(!merge_advertisement(&mut found, 1, -80, HID_ADV));
    assert!(merge_advertisement(&mut found, 3, -70, HID_ADV));
    assert_eq!(addresses(&found).as_slice(), &[2, 3]);
}

#[test]
fn unavailable_rssi_ranks_below_every_measurement() {
    let mut found = heapless::Vec::<_, 1>::new();
    assert!(merge_advertisement(
        &mut found,
        1u8,
        RSSI_UNAVAILABLE,
        HID_ADV
    ));
    assert!(merge_advertisement(&mut found, 2, -100, HID_ADV));
    assert!(!merge_advertisement(
        &mut found,
        3,
        RSSI_UNAVAILABLE,
        HID_ADV
    ));
    assert_eq!(addresses(&found).as_slice(), &[2]);
}

#[test]
fn replaced_device_needs_its_hid_uuid_to_return() {
    let mut found = heapless::Vec::<_, 1>::new();
    merge_advertisement(&mut found, 1u8, -70, HID_ADV);
    assert!(merge_advertisement(&mut found, 2, -40, HID_ADV));
    // A name-only scan response from the replaced device cannot re-enroll it.
    assert!(!merge_advertisement(
        &mut found,
        1,
        -30,
        &[3, 9, b'K', b'B']
    ));
    assert_eq!(addresses(&found).as_slice(), &[2]);
}

#[test]
fn zero_capacity_list_stays_empty() {
    let mut found = heapless::Vec::<DeviceInfo<Addr>, 0>::new();
    assert!(!merge_advertisement(&mut found, 1, -40, HID_ADV));
    assert!(found.is_empty());
}
