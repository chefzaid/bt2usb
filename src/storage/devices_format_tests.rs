//! Host tests for the flash format of the paired-device list: record codec
//! round trips, and loading valid, legacy, malformed, and unreadable items.

use super::*;
use crate::storage::{codec, framing};

/// A versioned item holding exactly these records, without merging them.
fn raw_item(devices: &[StoredDevice]) -> std::vec::Vec<u8> {
    let mut buf = [0u8; MAX_RECORD_SIZE];
    let mut writer = framing::Writer::new(&mut buf).unwrap();
    for stored in devices {
        assert!(writer.push(|slot| codec::encode_device(stored, slot)));
    }
    let len = writer.finish();
    buf[..len].to_vec()
}

fn loaded(data: &[u8]) -> (DeviceList, bool) {
    let mut list = DeviceList::new();
    let valid = list.load(data, &resolve);
    (list, valid)
}

// ── Codec ──────────────────────────────────────────────────────────────

#[test]
fn device_records_round_trip_with_and_without_a_bond() {
    let plain = device(identity(1), "Mouse");
    let keyed = bonded(2, 0, "Keyboard");
    for (stored, expected_len) in [(&plain, 9 + 5 + 1), (&keyed, 9 + 8 + 1 + 50)] {
        let mut buf = [0u8; codec::MAX_DEVICE_RECORD];
        let len = codec::encode_device(stored, &mut buf);
        assert_eq!(len, expected_len);
        assert_eq!(codec::decode_device(&buf[..len]).as_ref(), Some(stored));
        assert!(codec::decode_device(&buf[..len - 1]).is_none());
    }
}

#[test]
fn every_address_kind_round_trips_and_unknown_kinds_are_rejected() {
    let kinds = [
        AddressKind::Public,
        AddressKind::RandomStatic,
        AddressKind::RandomPrivateResolvable,
        AddressKind::RandomPrivateNonResolvable,
        AddressKind::Anonymous,
    ];
    for (byte, kind) in kinds.into_iter().enumerate() {
        let buf = codec::encode_address(address(kind, 9));
        assert_eq!(buf[6] as usize, byte);
        assert_eq!(codec::decode_address(&buf), Some(address(kind, 9)));
    }
    assert!(codec::decode_address(&[0, 0, 0, 0, 0, 0, 5]).is_none());
    assert!(codec::decode_address(&[0; 6]).is_none());
}

#[test]
fn bond_records_keep_every_key_field_in_place() {
    let keys = bond(3, 0x42);
    let buf = codec::encode_bond(&keys);
    assert_eq!(&buf[..2], &[0x34, 0x12]);
    assert_eq!(&buf[2..10], &[3; 8]);
    assert_eq!(&buf[10..26], &[0x42; 16]);
    assert_eq!(buf[26], 0x03);
    assert_eq!(&buf[27..30], &[0xA0, 3, 0x5A]);
    assert_eq!(buf[49], 1);
    assert_eq!(codec::decode_bond(&buf), Some(keys));
    assert!(codec::decode_bond(&buf[..49]).is_none());
}

#[test]
fn a_device_record_needs_its_whole_buffer() {
    let keyed = bonded(4, 0, "Pad");
    let len = 9 + 3 + 1 + 50;
    let mut buf = [0u8; 9 + 3 + 1 + 50];
    assert_eq!(codec::encode_device(&keyed, &mut buf[..len - 1]), 0);
    assert_eq!(codec::encode_device(&keyed, &mut buf), len);
    assert_eq!(
        codec::encode_device(&device(identity(4), "Pad"), &mut buf[..12]),
        0
    );
}

#[test]
fn a_bond_identity_must_be_public_or_random_static() {
    let mut keyed = bonded(5, 0, "K");
    let mut buf = [0u8; codec::MAX_DEVICE_RECORD];
    let len = codec::encode_device(&keyed, &mut buf);
    assert!(codec::decode_device(&buf[..len]).is_some());
    keyed.bond.as_mut().unwrap().identity = private_address(5, 1);
    let len = codec::encode_device(&keyed, &mut buf);
    assert!(codec::decode_device(&buf[..len]).is_none());
}

#[test]
fn names_truncate_at_a_character_boundary() {
    assert_eq!(truncated_name(&"a".repeat(40)).len(), 32);
    let accented = truncated_name(&"é".repeat(17));
    assert_eq!(accented.len(), 32);
    assert_eq!(accented.chars().count(), 16);
    assert_eq!(truncated_name("Keyboard").as_str(), "Keyboard");
}

// ── Loading ────────────────────────────────────────────────────────────

#[test]
fn a_full_store_round_trips_through_the_flash_item() {
    let mut list = DeviceList::new();
    for id in 0..MAX_PAIRED_DEVICES as u8 {
        let mut stored = bonded(id, 0, &"é".repeat(16));
        stored.last_rssi = -40 - id as i8;
        assert_eq!(list.add(stored, &resolve), AddOutcome::Added);
    }
    let data = item(&list);
    assert!(data.len() <= MAX_RECORD_SIZE);
    let (restored, valid) = loaded(&data);
    assert!(valid && restored.is_writable());
    let original: std::vec::Vec<_> = list.iter_recent().cloned().collect();
    let read_back: std::vec::Vec<_> = restored.iter_recent().cloned().collect();
    assert_eq!(read_back, original);
    let mut buf = [0u8; MAX_RECORD_SIZE];
    assert_eq!(restored.pending_item(&mut buf), Ok(None));
}

#[test]
fn an_empty_flash_area_gives_a_writable_store() {
    let mut list = DeviceList::new();
    list.load_unreadable();
    list.load_empty();
    assert!(list.is_writable() && list.len() == 0);
    let mut buf = [0u8; MAX_RECORD_SIZE];
    assert_eq!(list.pending_item(&mut buf), Ok(None));
    list.add(device(identity(1), "Mouse"), &resolve);
    assert!(matches!(list.pending_item(&mut buf), Ok(Some(_))));
}

fn legacy_record(address: PeerAddress, rssi: i8, name: &str) -> std::vec::Vec<u8> {
    let mut record = codec::encode_address(address).to_vec();
    record.push(rssi as u8);
    record.push(name.len() as u8);
    record.extend_from_slice(name.as_bytes());
    record
}

#[test]
fn a_legacy_store_loads_without_bonds_and_is_rewritten_versioned() {
    let mut data = vec![2];
    data.extend(legacy_record(identity(1), -50, "Mouse"));
    data.extend(legacy_record(
        address(AddressKind::Public, 2),
        -70,
        "Keyboard",
    ));
    let (mut list, valid) = loaded(&data);
    assert!(valid && list.is_writable());
    assert_eq!(
        addresses(&list),
        [address(AddressKind::Public, 2), identity(1)]
    );
    assert!(list.bonds().next().is_none());
    assert_eq!(list.iter_recent().last().unwrap().name.as_str(), "Mouse");
    let mut buf = [0u8; MAX_RECORD_SIZE];
    assert_eq!(list.pending_item(&mut buf), Ok(None));
    list.add(device(identity(3), "Pad"), &resolve);
    assert_eq!(&item(&list)[..3], &[0xB2, 0x01, 3]);
}

#[test]
fn a_legacy_store_with_a_bad_count_or_length_is_refused() {
    let record = legacy_record(identity(1), -50, "Mouse");
    let mut too_many = vec![MAX_PAIRED_DEVICES as u8 + 1];
    too_many.extend(record.iter().cycle().take(record.len() * 5));
    let mut truncated = vec![1];
    truncated.extend(&record[..record.len() - 1]);
    let mut trailing = vec![1];
    trailing.extend(&record);
    trailing.push(0);
    let mut short_header = vec![1];
    short_header.extend(&record[..8]);
    for data in [too_many, truncated, trailing, short_header] {
        let (list, valid) = loaded(&data);
        assert!(!valid && !list.is_writable() && list.len() == 0);
    }
    // A zero count with nothing after it is a valid, empty legacy store.
    let (list, valid) = loaded(&[0]);
    assert!(valid && list.is_writable() && list.len() == 0);
}

#[test]
fn a_malformed_or_future_item_disables_saves() {
    let good = item(&{
        let mut list = DeviceList::new();
        list.add(bonded(1, 0, "Keyboard"), &resolve);
        list
    });
    let mut bad_record = good.clone();
    bad_record[4 + 8] = 33; // name length beyond 32 bytes
    let mut future = good.clone();
    future[1] = 0x02;
    let mut over_capacity = raw_item(&[
        device(identity(1), "A"),
        device(identity(2), "B"),
        device(identity(3), "C"),
        device(identity(4), "D"),
    ]);
    over_capacity[2] = MAX_PAIRED_DEVICES as u8 + 1;
    let cases: [&[u8]; 6] = [
        &[],
        &[0xB2],
        &good[..good.len() - 1],
        &bad_record,
        &future,
        &over_capacity,
    ];
    for data in cases {
        let (mut list, valid) = loaded(data);
        assert!(!valid && !list.is_writable() && list.len() == 0);
        list.add(device(identity(9), "New"), &resolve);
        let mut buf = [0u8; MAX_RECORD_SIZE];
        assert_eq!(list.pending_item(&mut buf), Err(StoreError::Unreadable));
    }
}

#[test]
fn an_unreadable_store_refuses_saves_and_forget_until_reset() {
    let mut list = DeviceList::new();
    list.add(device(identity(1), "Mouse"), &resolve);
    list.load_unreadable();
    assert!(!list.is_writable() && list.len() == 0);
    assert_eq!(
        list.without(identity(1)).unwrap_err(),
        StoreError::Unreadable
    );
    list.add(device(identity(1), "Mouse"), &resolve);
    let mut buf = [0u8; MAX_RECORD_SIZE];
    assert_eq!(list.pending_item(&mut buf), Err(StoreError::Unreadable));
    let reset = list.reset();
    assert!(reset.erase_first);
    assert!(reset.candidate.is_writable() && reset.candidate.len() == 0);
    assert_eq!(item(&reset.candidate), [0xB2, 0x01, 0]);
}

#[test]
fn a_readable_store_resets_by_appending_an_empty_item() {
    let mut list = DeviceList::new();
    list.add(bonded(1, 0, "Keyboard"), &resolve);
    let reset = list.reset();
    assert!(!reset.erase_first);
    assert_eq!(item(&reset.candidate), [0xB2, 0x01, 0]);
    assert_eq!(list.len(), 1);
}

#[test]
fn a_save_reports_an_item_that_does_not_fit_its_buffer() {
    let mut list = DeviceList::new();
    list.add(device(identity(1), "Mouse"), &resolve);
    assert_eq!(
        list.pending_item(&mut [0u8; 8]),
        Err(StoreError::Serialization)
    );
    let mut buf = [0u8; MAX_RECORD_SIZE];
    assert!(matches!(list.pending_item(&mut buf), Ok(Some(_))));
    list.mark_saved();
    assert_eq!(list.pending_item(&mut buf), Ok(None));
}

#[test]
fn loading_merges_records_of_one_bonded_peer() {
    let data = raw_item(&[
        device(private_address(1, 1), "Keyboard"),
        bonded(1, 2, "Keyboard"),
        device(identity(2), "Mouse"),
    ]);
    let (list, valid) = loaded(&data);
    assert!(valid);
    assert_eq!(addresses(&list), [identity(2), identity(1)]);
    assert_eq!(list.bonds().count(), 1);
}
