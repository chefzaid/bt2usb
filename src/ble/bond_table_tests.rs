//! Host tests for the bond table: a new pairing never displaces a saved
//! device's keys, a pairing that is not saved is discarded without touching
//! saved ones, and a store eviction or Forget drops exactly the keys it names.

use super::*;

/// A stand-in bond: the peer's identity and a key generation, so a re-pairing
/// of one peer gives a different bond.
type Bond = (u8, u8);

fn same(a: &Bond, b: &Bond) -> bool {
    a.0 == b.0
}

/// The store's four saved devices, peers 1 to 4.
fn full_store() -> BondTable<Bond, BOND_SLOTS> {
    let mut table = BondTable::new();
    table.load((1..=MAX_PAIRED_DEVICES as u8).map(|peer| (peer, 0)), same);
    table
}

fn peers(table: &BondTable<Bond, BOND_SLOTS>) -> std::vec::Vec<u8> {
    table.iter().map(|bond| bond.0).collect()
}

#[test]
fn loading_marks_every_bond_saved_and_merges_one_peers_bonds() {
    let mut table = BondTable::<Bond, BOND_SLOTS>::new();
    table.insert((9, 0), |b| b.0 == 9);
    table.load([(1, 0), (2, 0), (1, 1)], same);
    assert_eq!(
        table.iter().copied().collect::<std::vec::Vec<_>>(),
        [(1, 1), (2, 0)]
    );
    assert_eq!(table.saved_len(), 2);
}

#[test]
fn a_new_pairing_keeps_every_saved_devices_keys() {
    let mut table = full_store();
    assert_eq!(table.insert((5, 0), |b| b.0 == 5), Inserted::Added);
    assert_eq!(peers(&table), [1, 2, 3, 4, 5]);
    assert_eq!(table.saved_len(), MAX_PAIRED_DEVICES);
    // One pairing per link fits on top of a full store.
    assert_eq!(table.insert((6, 0), |b| b.0 == 6), Inserted::Added);
    assert_eq!(table.iter().count(), BOND_SLOTS);
    assert_eq!(table.saved_len(), MAX_PAIRED_DEVICES);
}

#[test]
fn a_pairing_that_is_not_saved_is_discarded_alone() {
    let mut table = full_store();
    table.insert((5, 0), |b| b.0 == 5);
    // Discarding never drops a saved bond, even one that matches.
    table.discard_unsaved(|b| b.0 == 5 || b.0 == 1);
    assert_eq!(peers(&table), [1, 2, 3, 4]);
    assert_eq!(table.saved_len(), MAX_PAIRED_DEVICES);
}

#[test]
fn saving_a_fifth_device_then_forgetting_the_evicted_one_matches_the_store() {
    let mut table = full_store();
    table.insert((5, 0), |b| b.0 == 5);
    table.mark_saved(&(5, 0));
    // The store evicted its oldest record, peer 1.
    table.forget(|b| b.0 == 1);
    assert_eq!(peers(&table), [2, 3, 4, 5]);
    assert_eq!(table.saved_len(), MAX_PAIRED_DEVICES);
    // Now saved, peer 5 survives a discard.
    table.discard_unsaved(|b| b.0 == 5);
    assert_eq!(peers(&table), [2, 3, 4, 5]);
}

#[test]
fn re_pairing_replaces_the_keys_and_keeps_the_saved_mark() {
    let mut table = full_store();
    assert_eq!(table.insert((2, 1), |b| b.0 == 2), Inserted::Replaced);
    assert_eq!(table.iter().count(), MAX_PAIRED_DEVICES);
    assert!(table.iter().any(|b| *b == (2, 1)));
    // Still saved: a failed connection does not drop a saved device's keys.
    table.discard_unsaved(|b| b.0 == 2);
    assert!(table.iter().any(|b| *b == (2, 1)));

    // An unsaved pairing that pairs again stays unsaved.
    table.insert((5, 0), |b| b.0 == 5);
    assert_eq!(table.insert((5, 1), |b| b.0 == 5), Inserted::Replaced);
    table.discard_unsaved(|b| b.0 == 5);
    assert_eq!(peers(&table), [1, 2, 3, 4]);
}

#[test]
fn a_full_table_drops_the_oldest_unsaved_pairing_first() {
    let mut table = full_store();
    table.insert((5, 0), |b| b.0 == 5);
    table.insert((6, 0), |b| b.0 == 6);
    assert_eq!(
        table.insert((7, 0), |b| b.0 == 7),
        Inserted::AddedAfterDroppingUnsaved
    );
    assert_eq!(peers(&table), [1, 2, 3, 4, 6, 7]);
    assert_eq!(table.saved_len(), MAX_PAIRED_DEVICES);
}

#[test]
fn a_table_of_saved_bonds_only_drops_the_oldest() {
    // More saved bonds than the store holds leave no unsaved one to drop.
    let mut table = BondTable::<Bond, BOND_SLOTS>::new();
    table.load((1..=BOND_SLOTS as u8).map(|peer| (peer, 0)), same);
    let next = BOND_SLOTS as u8 + 1;
    assert_eq!(
        table.insert((next, 0), |b| b.0 == next),
        Inserted::AddedAfterDroppingSaved
    );
    assert_eq!(peers(&table).first(), Some(&2));
    assert_eq!(peers(&table).last(), Some(&next));
}

#[test]
fn marking_saved_needs_the_exact_bond() {
    let mut table = BondTable::<Bond, BOND_SLOTS>::default();
    assert_eq!(table.iter().count(), 0);
    table.insert((5, 1), |b| b.0 == 5);
    // Keys from an older pairing of the same peer are not the ones held.
    table.mark_saved(&(5, 0));
    assert_eq!(table.saved_len(), 0);
    table.mark_saved(&(5, 1));
    assert_eq!(table.saved_len(), 1);
    table.clear();
    assert_eq!(table.iter().count(), 0);
}
