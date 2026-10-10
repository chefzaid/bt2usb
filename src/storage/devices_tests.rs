//! Host tests for the paired-device list: identity merge and bond replacement,
//! eviction, lookup, forget, factory reset, and their transactions. The flash
//! format tests are in `devices_format_tests.rs`.

use super::*;

#[path = "devices_format_tests.rs"]
mod format;

/// Stand-in for the SoftDevice's private-address resolution: an address
/// resolves with an IRK when its first three bytes equal the IRK's.
fn resolve(irk: &[u8; 16], address: &[u8; 6]) -> bool {
    address[..3] == irk[..3]
}

fn never(_: &[u8; 16], _: &[u8; 6]) -> bool {
    false
}

fn address(kind: AddressKind, id: u8) -> PeerAddress {
    PeerAddress::new(kind, [id, 0x11, 0x22, 0x33, 0x44, 0xC5])
}

fn identity(id: u8) -> PeerAddress {
    address(AddressKind::RandomStatic, id)
}

/// A bond whose IRK resolves the private addresses that [`private_address`]
/// derives for the same `id`.
fn bond(id: u8, ltk: u8) -> StoredBond {
    let mut irk = [0u8; 16];
    irk[..3].copy_from_slice(&[0xA0, id, 0x5A]);
    StoredBond {
        ediv: 0x1234,
        rand: [id; 8],
        ltk: [ltk; 16],
        flags: 0x03,
        irk,
        identity: identity(id),
    }
}

fn private_address(id: u8, rotation: u8) -> PeerAddress {
    PeerAddress::new(
        AddressKind::RandomPrivateResolvable,
        [0xA0, id, 0x5A, rotation, 0x00, 0x40],
    )
}

fn device(address: PeerAddress, name: &str) -> StoredDevice {
    StoredDevice::new(address, name, -60)
}

fn bonded(id: u8, rotation: u8, name: &str) -> StoredDevice {
    let mut paired = device(private_address(id, rotation), name);
    paired.bond = Some(bond(id, 0x77));
    paired
}

const ALL_KINDS: [AddressKind; 5] = [
    AddressKind::Public,
    AddressKind::RandomStatic,
    AddressKind::RandomPrivateResolvable,
    AddressKind::RandomPrivateNonResolvable,
    AddressKind::Anonymous,
];

fn addresses(list: &DeviceList) -> std::vec::Vec<PeerAddress> {
    list.iter_recent().map(|stored| stored.address).collect()
}

fn item(list: &DeviceList) -> std::vec::Vec<u8> {
    let mut buf = [0u8; MAX_RECORD_SIZE];
    let len = list.pending_item(&mut buf).unwrap().unwrap();
    buf[..len].to_vec()
}

// ── Identity merge ─────────────────────────────────────────────────────

#[test]
fn a_bonded_device_is_stored_under_its_identity_address() {
    let mut list = DeviceList::new();
    assert_eq!(
        list.add(bonded(1, 7, "Keyboard"), &resolve),
        AddOutcome::Added
    );
    assert_eq!(addresses(&list), [identity(1)]);
}

#[test]
fn rssi_alone_is_not_a_change_but_a_name_is() {
    let mut list = DeviceList::new();
    list.add(bonded(1, 0, "Keyboard"), &resolve);
    list.mark_saved();
    let mut again = bonded(1, 3, "Keyboard");
    again.last_rssi = -90;
    assert_eq!(list.add(again, &resolve), AddOutcome::Unchanged);
    assert_eq!(list.iter_recent().next().unwrap().last_rssi, -90);
    let mut buf = [0u8; MAX_RECORD_SIZE];
    assert_eq!(list.pending_item(&mut buf), Ok(None));
    assert_eq!(
        list.add(bonded(1, 4, "Keyboard 2"), &resolve),
        AddOutcome::Updated
    );
    assert!(matches!(list.pending_item(&mut buf), Ok(Some(_))));
}

#[test]
fn new_keys_replace_the_bond_of_the_same_identity() {
    let mut list = DeviceList::new();
    list.add(bonded(1, 0, "Keyboard"), &resolve);
    let mut repaired = bonded(1, 5, "Keyboard");
    repaired.bond.as_mut().unwrap().ltk = [0x99; 16];
    assert_eq!(list.add(repaired, &resolve), AddOutcome::Updated);
    assert_eq!(list.len(), 1);
    assert_eq!(list.bonds().next().unwrap().ltk, [0x99; 16]);
}

#[test]
fn a_device_without_keys_never_clears_a_stored_bond() {
    let mut list = DeviceList::new();
    list.add(bonded(1, 0, "Keyboard"), &resolve);
    list.mark_saved();
    assert_eq!(
        list.add(device(identity(1), "Keyboard"), &resolve),
        AddOutcome::Unchanged
    );
    assert_eq!(list.bonds().count(), 1);
}

#[test]
fn keys_merge_with_an_entry_stored_under_the_identity_or_a_private_address() {
    for stored_at in [identity(1), private_address(1, 9)] {
        let mut list = DeviceList::new();
        list.add(device(stored_at, "Keyboard"), &resolve);
        assert_eq!(
            list.add(bonded(1, 2, "Keyboard"), &resolve),
            AddOutcome::Updated
        );
        assert_eq!(addresses(&list), [identity(1)]);
        assert_eq!(list.bonds().count(), 1);
    }
    // Without resolution a private address is a different peer.
    let mut list = DeviceList::new();
    list.add(device(private_address(1, 9), "Keyboard"), &never);
    assert_eq!(
        list.add(bonded(1, 2, "Keyboard"), &never),
        AddOutcome::Added
    );
    assert_eq!(list.len(), 2);
}

// ── Identity addresses ─────────────────────────────────────────────────

#[test]
fn softdevice_address_types_decode_and_reserved_ones_are_refused() {
    let defined = [
        (0x00, AddressKind::Public),
        (0x01, AddressKind::RandomStatic),
        (0x02, AddressKind::RandomPrivateResolvable),
        (0x03, AddressKind::RandomPrivateNonResolvable),
        (0x7F, AddressKind::Anonymous),
    ];
    for (gap_type, kind) in defined {
        assert_eq!(AddressKind::from_gap_type(gap_type), Some(kind));
    }
    let reserved = (0..=u8::MAX)
        .filter(|&gap_type| AddressKind::from_gap_type(gap_type).is_none())
        .count();
    assert_eq!(reserved, 256 - defined.len());
}

#[test]
fn a_bond_is_stored_only_with_a_public_or_random_static_identity() {
    for kind in ALL_KINDS {
        let is_identity = matches!(kind, AddressKind::Public | AddressKind::RandomStatic);
        assert_eq!(kind.is_identity(), is_identity, "{kind:?}");
        let mut paired = bonded(1, 0, "Keyboard");
        paired.bond.as_mut().unwrap().identity = address(kind, 1);
        let mut list = DeviceList::new();
        assert_eq!(list.add(paired, &resolve), AddOutcome::Added);
        let stored = list.iter_recent().next().unwrap().clone();
        if is_identity {
            assert_eq!(stored.address, address(kind, 1));
            assert!(stored.bond.is_some());
        } else {
            // Kept without keys, at the address it connected from.
            assert_eq!(stored.address, private_address(1, 0));
            assert_eq!(stored.bond, None, "{kind:?}");
        }
        // Either way the next boot loads what the save wrote.
        let mut reloaded = DeviceList::new();
        assert!(reloaded.load(&item(&list), &resolve), "{kind:?}");
        assert!(reloaded.is_writable());
        assert_eq!(reloaded.iter_recent().next(), Some(&stored));
    }
}

#[test]
fn a_refused_bond_leaves_the_stored_bond_of_the_same_peer() {
    let mut list = DeviceList::new();
    list.add(bonded(1, 0, "Keyboard"), &resolve);
    let mut refused = bonded(1, 3, "Keyboard");
    refused.bond.as_mut().unwrap().identity = private_address(1, 3);
    refused.bond.as_mut().unwrap().ltk = [0x99; 16];
    list.add(refused, &resolve);
    assert_eq!(list.len(), 1);
    assert_eq!(list.bonds().next(), Some(bond(1, 0x77)));
}

#[test]
fn an_all_zero_irk_resolves_no_private_address() {
    assert!(!irk_present(&[0; 16]));
    let mut one_bit = [0u8; 16];
    one_bit[15] = 1;
    assert!(irk_present(&one_bit));

    // A keyboard on a public address that distributed no IRK, and a device
    // whose private address the fake resolver resolves with the zero IRK.
    let mut keyless = bond(1, 0x77);
    keyless.irk = [0; 16];
    keyless.identity = address(AddressKind::Public, 1);
    let crafted = PeerAddress::new(AddressKind::RandomPrivateResolvable, [0, 0, 0, 9, 0, 0x40]);
    assert!(resolve(&keyless.irk, &crafted.bytes));
    assert!(!keyless.matches(crafted, &resolve));
    assert!(keyless.matches(keyless.identity, &resolve));

    let mut list = DeviceList::new();
    let mut keyboard = device(keyless.identity, "Keyboard");
    keyboard.bond = Some(keyless);
    list.add(keyboard.clone(), &resolve);
    assert!(list.find(crafted, &resolve).is_none());
    assert_eq!(
        list.add(device(crafted, "Other"), &resolve),
        AddOutcome::Added
    );
    assert_eq!(list.find(keyless.identity, &resolve), Some(&keyboard));
}

// ── Capacity, lookup, and removal ──────────────────────────────────────

#[test]
fn a_full_store_evicts_the_oldest_added_device() {
    let mut list = DeviceList::new();
    for id in 0..MAX_PAIRED_DEVICES as u8 {
        assert_eq!(
            list.add(device(identity(id), "D"), &resolve),
            AddOutcome::Added
        );
    }
    let newest = identity(MAX_PAIRED_DEVICES as u8);
    assert_eq!(
        list.add(device(newest, "D"), &resolve),
        AddOutcome::AddedAfterEviction
    );
    assert_eq!(list.len(), MAX_PAIRED_DEVICES);
    assert_eq!(addresses(&list).first(), Some(&newest));
    assert!(list.find(identity(0), &resolve).is_none());
    assert!(list.find(identity(1), &resolve).is_some());
}

#[test]
fn find_matches_the_stored_address_or_a_resolvable_private_address() {
    let mut list = DeviceList::new();
    list.add(bonded(1, 0, "Keyboard"), &resolve);
    list.add(device(address(AddressKind::Public, 2), "Mouse"), &resolve);
    assert_eq!(
        list.find(identity(1), &resolve).unwrap().name.as_str(),
        "Keyboard"
    );
    assert!(list.find(private_address(1, 6), &resolve).is_some());
    assert!(list.find(private_address(1, 6), &never).is_none());
    assert!(list.find(private_address(3, 6), &resolve).is_none());
    assert_eq!(
        list.find(address(AddressKind::Public, 2), &never)
            .unwrap()
            .name
            .as_str(),
        "Mouse"
    );
    assert!(list
        .find(address(AddressKind::RandomStatic, 2), &resolve)
        .is_none());
}

#[test]
fn bond_matching_follows_the_identity_key_rules() {
    let keys = bond(1, 0);
    let rpa_bytes = private_address(1, 0).bytes;
    assert!(keys.matches(identity(1), &never));
    assert!(!keys.matches(identity(2), &resolve));
    assert!(!keys.matches(address(AddressKind::Public, 1), &resolve));
    assert!(keys.matches(private_address(1, 0), &resolve));
    assert!(!keys.matches(private_address(1, 0), &never));
    for kind in [
        AddressKind::RandomPrivateNonResolvable,
        AddressKind::Anonymous,
    ] {
        assert!(!keys.matches(PeerAddress::new(kind, rpa_bytes), &resolve));
    }
}

#[test]
fn address_equality_ignores_the_resolved_flag() {
    let mut reported = identity(1);
    reported.resolved = true;
    assert_eq!(reported, identity(1));
    assert_ne!(address(AddressKind::Public, 1), identity(1));
}

#[test]
fn forget_builds_a_candidate_and_leaves_the_list_unchanged() {
    let mut list = DeviceList::new();
    list.add(bonded(1, 0, "Keyboard"), &resolve);
    list.add(device(identity(2), "Mouse"), &resolve);
    list.mark_saved();
    let candidate = list.without(identity(1)).unwrap();
    assert_eq!(addresses(&candidate), [identity(2)]);
    assert!(candidate.bonds().next().is_none());
    assert_eq!(list.len(), 2);
    let mut buf = [0u8; MAX_RECORD_SIZE];
    assert!(matches!(candidate.pending_item(&mut buf), Ok(Some(_))));
    assert_eq!(list.pending_item(&mut buf), Ok(None));
    // Forget takes the stored identity, not an address that resolves to it.
    assert_eq!(
        list.without(private_address(1, 0)).unwrap_err(),
        StoreError::NotFound
    );
    assert_eq!(list.without(identity(3)).unwrap_err(), StoreError::NotFound);
}

#[test]
fn bonds_are_listed_oldest_first_and_devices_newest_first() {
    let mut list = DeviceList::default();
    list.add(bonded(1, 0, "Keyboard"), &resolve);
    list.add(device(identity(2), "Mouse"), &resolve);
    list.add(bonded(3, 0, "Pad"), &resolve);
    let identities: std::vec::Vec<_> = list.bonds().map(|keys| keys.identity).collect();
    assert_eq!(identities, [identity(1), identity(3)]);
    assert_eq!(addresses(&list), [identity(3), identity(2), identity(1)]);
}

// ── Transactions ───────────────────────────────────────────────────────

fn immediate<F: core::future::Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);
    let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
    match future.as_mut().poll(&mut cx) {
        core::task::Poll::Ready(value) => value,
        core::task::Poll::Pending => panic!("fake persistence must finish immediately"),
    }
}

#[test]
fn forget_and_reset_publish_only_after_the_save_succeeds() {
    use crate::ble::management::commit;
    let mut list = DeviceList::new();
    list.add(bonded(1, 0, "Keyboard"), &resolve);
    list.add(device(identity(2), "Mouse"), &resolve);
    list.mark_saved();

    let forget = list.without(identity(1)).unwrap();
    let failed = immediate(commit(&mut list, forget, async |_: &mut DeviceList| {
        Err(StoreError::Flash)
    }));
    assert_eq!(failed, Err(StoreError::Flash));
    assert_eq!(list.len(), 2);
    let reset = list.reset().candidate;
    let failed = immediate(commit(&mut list, reset, async |_: &mut DeviceList| {
        Err(StoreError::Flash)
    }));
    assert_eq!(failed, Err(StoreError::Flash));
    assert_eq!(list.bonds().count(), 1);

    let forget = list.without(identity(1)).unwrap();
    let saved = immediate(commit(&mut list, forget, async |next: &mut DeviceList| {
        next.mark_saved();
        Ok::<(), StoreError>(())
    }));
    assert_eq!(saved, Ok(()));
    assert_eq!(addresses(&list), [identity(2)]);
    assert!(list.bonds().next().is_none());
}
