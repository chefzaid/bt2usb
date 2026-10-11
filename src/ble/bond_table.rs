//! The bonding keys the security handler holds in RAM, and which of them the
//! device store has saved.
//!
//! The bonder answers the SoftDevice's key requests from this table, so it
//! must hold every saved device's keys and, until the device is saved, the
//! keys of a pairing just made. The table keeps the two apart: a new pairing
//! never displaces a saved device's keys, a saved device's keys go only when
//! the store evicts or forgets its record, and a pairing whose connection ends
//! before it is saved is discarded. The table therefore needs room for the
//! saved devices plus one pairing per link ([`BOND_SLOTS`]).
//!
//! Generic over the bond type, with the caller deciding which bonds belong to
//! one peer, so the policy is host-tested without SoftDevice types; the
//! firmware's `Bonder` wraps it.

use crate::config::{BLE_MAX_CONNECTIONS, MAX_PAIRED_DEVICES};
use heapless::Vec;

/// Bonds the table holds: every device the store can save, plus a pairing
/// not yet saved on each link.
pub const BOND_SLOTS: usize = MAX_PAIRED_DEVICES + BLE_MAX_CONNECTIONS;

#[derive(Clone, Copy)]
struct Entry<B> {
    bond: B,
    /// The device store holds this bond.
    saved: bool,
}

/// What [`BondTable::insert`] did, for the shell's log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inserted {
    /// Replaced the keys of a peer already held, saved or not.
    Replaced,
    /// Added as a pairing the store has not saved yet.
    Added,
    /// Added after dropping the oldest pairing the store had not saved: the
    /// table was full, which the [`BOND_SLOTS`] sizing leaves only for
    /// pairings whose connections ended without being reported.
    AddedAfterDroppingUnsaved,
    /// Added after dropping the oldest saved bond, because every entry was
    /// saved. Only a table smaller than [`BOND_SLOTS`] gets here.
    AddedAfterDroppingSaved,
}

/// Bonds held in RAM, oldest first, each marked saved or not.
pub struct BondTable<B, const N: usize> {
    entries: Vec<Entry<B>, N>,
}

impl<B: Copy + PartialEq, const N: usize> Default for BondTable<B, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: Copy + PartialEq, const N: usize> BondTable<B, N> {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Replace the table with the store's bonds, all saved. A bond for the
    /// same peer as an earlier one (`same_peer`) replaces it.
    pub fn load(&mut self, bonds: impl IntoIterator<Item = B>, same_peer: impl Fn(&B, &B) -> bool) {
        self.entries.clear();
        for bond in bonds {
            let entry = Entry { bond, saved: true };
            if let Some(existing) = self.entries.iter_mut().find(|e| same_peer(&e.bond, &bond)) {
                *existing = entry;
            } else {
                let _ = self.entries.push(entry);
            }
        }
    }

    /// Hold the keys of a pairing that just completed. A peer already held
    /// (`same_peer`) gets the new keys and stays saved or unsaved; any other
    /// is added unsaved, after dropping the oldest unsaved pairing when the
    /// table is full.
    pub fn insert(&mut self, bond: B, same_peer: impl Fn(&B) -> bool) -> Inserted {
        if let Some(existing) = self.entries.iter_mut().find(|e| same_peer(&e.bond)) {
            existing.bond = bond;
            return Inserted::Replaced;
        }
        let mut inserted = Inserted::Added;
        if self.entries.is_full() {
            inserted = match self.entries.iter().position(|e| !e.saved) {
                Some(index) => {
                    self.entries.remove(index);
                    Inserted::AddedAfterDroppingUnsaved
                }
                None => {
                    self.entries.remove(0);
                    Inserted::AddedAfterDroppingSaved
                }
            };
        }
        let _ = self.entries.push(Entry { bond, saved: false });
        inserted
    }

    /// Mark exactly `bond` saved: the store now holds it.
    pub fn mark_saved(&mut self, bond: &B) {
        for entry in self.entries.iter_mut().filter(|e| e.bond == *bond) {
            entry.saved = true;
        }
    }

    /// Drop every bond `matches` selects, saved or not.
    pub fn forget(&mut self, matches: impl Fn(&B) -> bool) {
        self.entries.retain(|e| !matches(&e.bond));
    }

    /// Drop every unsaved bond `matches` selects: its connection ended
    /// before the device was saved. Saved bonds stay.
    pub fn discard_unsaved(&mut self, matches: impl Fn(&B) -> bool) {
        self.entries.retain(|e| e.saved || !matches(&e.bond));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Every bond held, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &B> {
        self.entries.iter().map(|e| &e.bond)
    }

    /// How many bonds the store holds.
    pub fn saved_len(&self) -> usize {
        self.entries.iter().filter(|e| e.saved).count()
    }
}

#[cfg(test)]
#[path = "bond_table_tests.rs"]
mod tests;
