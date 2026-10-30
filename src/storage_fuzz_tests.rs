//! Fuzz targets for the paired-device store's flash item: its framing, its
//! device records, and loading a whole item. What a save writes must load
//! back unchanged, and whatever else flash holds must be refused or loaded
//! without breaking that. [`crate::fuzz`] drives each check over the seed
//! corpus and mutated inputs.

use crate::config::MAX_PAIRED_DEVICES;
use crate::fuzz::{run, Target};
use crate::storage::codec::{self, MAX_DEVICE_RECORD};
use crate::storage::devices::{DeviceList, StoredDevice, MAX_RECORD_SIZE};
use crate::storage::framing::{self, Writer};

const STORE: &str = include_str!("../tests/fuzz-corpus/store.txt");

/// A stand-in for the SoftDevice's address resolution that some inputs pass,
/// so loading merges private addresses into bonded identities as it would on
/// the device.
fn resolve(irk: &[u8; 16], address: &[u8; 6]) -> bool {
    irk[0] == address[0]
}

// ── Framing ──────────────────────────────────────────────────────────────────

fn check_framing(data: &[u8]) {
    let declared = framing::declared_count(data);
    assert_eq!(framing::is_versioned(data), declared.is_some());
    assert!(declared.is_none() || framing::has_magic(data));

    // Each record is the non-empty run its length prefix announces, in order.
    let records: Vec<&[u8]> = framing::records(data).collect();
    assert!(records.len() <= usize::from(declared.unwrap_or(0)));
    let mut end = 3;
    for record in &records {
        assert!(!record.is_empty());
        assert_eq!(usize::from(data[end]), record.len());
        assert_eq!(&data[end + 1..end + 1 + record.len()], *record);
        end += 1 + record.len();
    }

    // A complete frame is exactly what writing its records produces.
    if framing::is_complete(data) {
        assert_eq!(Some(records.len()), declared.map(usize::from));
        assert_eq!(end, data.len());
        let mut buf = vec![0u8; data.len()];
        let mut writer = Writer::new(&mut buf).expect("a complete frame has a header");
        for record in &records {
            assert!(writer.push(|slot| copy_into(slot, record)));
        }
        assert_eq!(writer.finish(), data.len());
        assert_eq!(buf, data);
    }
}

/// Write `record` at the start of `slot`, or nothing if it does not fit.
fn copy_into(slot: &mut [u8], record: &[u8]) -> usize {
    slot.get_mut(..record.len()).map_or(0, |start| {
        start.copy_from_slice(record);
        record.len()
    })
}

/// Push records of the lengths `data` lists into a buffer of the size its
/// first byte gives: every push that fits is kept, every other leaves the
/// frame as it was, and the result reads back as the records kept.
fn check_writer(data: &[u8]) {
    let Some((&size, lengths)) = data.split_first() else {
        return;
    };
    let mut buf = vec![0xA5; usize::from(size) * 2];
    let capacity = buf.len();
    let Some(mut writer) = Writer::new(&mut buf) else {
        assert!(capacity < 3);
        return;
    };
    let mut kept = Vec::new();
    let mut used = 3;
    for (index, &length) in lengths.iter().enumerate() {
        let record = vec![index as u8; usize::from(length)];
        let fits = length > 0 && used + 1 + record.len() <= capacity && kept.len() < 255;
        assert_eq!(writer.push(|slot| copy_into(slot, &record)), fits);
        if fits {
            used += 1 + record.len();
            kept.push(record);
        }
    }
    assert_eq!(writer.finish(), used);
    assert!(framing::is_complete(&buf[..used]));
    assert_eq!(framing::records(&buf[..used]).collect::<Vec<_>>(), kept);
}

#[test]
fn fuzz_store_framing() {
    let target = Target {
        name: "store-framing",
        corpus: STORE,
        max_len: MAX_RECORD_SIZE,
    };
    run(&target, &[], |data| {
        check_framing(data);
        check_writer(data);
    });
}

// ── Device records and whole items ───────────────────────────────────────────

/// The stored devices, oldest first, the order a save writes them in.
fn oldest_first(list: &DeviceList) -> Vec<StoredDevice> {
    let mut devices: Vec<StoredDevice> = list.iter_recent().cloned().collect();
    devices.reverse();
    devices
}

/// The record `encode_device` writes for `device`.
fn encoded(device: &StoredDevice) -> Vec<u8> {
    let mut buf = [0u8; MAX_DEVICE_RECORD];
    let length = codec::encode_device(device, &mut buf);
    assert!(length > 0, "{device:?} has no record");
    buf[..length].to_vec()
}

fn check_store(data: &[u8]) {
    // A record the codec accepts is byte for byte the one it would write, so
    // no two records load as the same device.
    for record in framing::records(data).chain([data]) {
        if let Some(device) = codec::decode_device(record) {
            assert_eq!(encoded(&device), record);
        }
    }

    let mut list = DeviceList::new();
    let loaded = list.load(data, &resolve);
    assert_eq!(list.is_writable(), loaded);
    assert!(loaded || list.len() == 0, "a refused item left devices");
    assert!(list.len() <= MAX_PAIRED_DEVICES);
    let devices = oldest_first(&list);
    if !loaded {
        return;
    }

    // What a save writes for the loaded list loads back as the same list.
    let mut item = [0u8; MAX_RECORD_SIZE];
    let mut writer = Writer::new(&mut item).expect("the item buffer holds a header");
    for device in &devices {
        let record = encoded(device);
        assert!(writer.push(|slot| copy_into(slot, &record)));
    }
    let length = writer.finish();
    let mut reloaded = DeviceList::new();
    assert!(
        reloaded.load(&item[..length], &resolve),
        "a saved list is refused"
    );
    assert_eq!(oldest_first(&reloaded), devices);
}

#[test]
fn fuzz_store_items() {
    let target = Target {
        name: "store-item",
        corpus: STORE,
        max_len: MAX_RECORD_SIZE,
    };
    run(&target, &[], check_store);
}
