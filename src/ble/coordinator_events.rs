//! The coordinator's reducers for slot-worker events: a link reported up,
//! lost, or failed, and a slot reported free. Each ignores an event whose
//! attempt number is not the slot's current one (see [`ConnManager`]).
//!
//! An attempt that ends while its slot has no link up also discards the keys
//! of any unsaved pairing it made ([`Action::DiscardUnsavedBond`]): one that
//! ended before its link was reported up, whose keys the store never saved.
//! A retry after a lost link returns the action too, and it leaves the saved
//! keys alone. The coordinator handles a worker's events in order, so a
//! `Connected`, whose `PersistDevice` saves the keys, is always handled before
//! a later report from the same slot.

use super::*;

/// The device of a slot that has no link up, so any pairing made on it since
/// its link was last reported up was never saved.
fn unsaved_pairing<A: Clone>(manager: &ConnManager<A>, slot: usize) -> Option<A> {
    manager
        .slots
        .get(slot)
        .filter(|entry| !entry.connected)
        .and_then(|entry| entry.address.clone())
}

/// A slot worker reported a successful connection.
pub fn on_slot_connected<A: Clone + PartialEq>(
    manager: &mut ConnManager<A>,
    slot: usize,
    attempt: u32,
    device: &DeviceInfo<A>,
) -> Vec<Action<A>, 2> {
    let mut actions = Vec::new();
    if !manager.is_current(slot, attempt) {
        return actions;
    }
    manager.connect_slot(slot, device);
    let _ = actions.push(Action::PersistDevice(device.clone()));
    let _ = actions.push(Action::Emit(UiEvent::Connected(connection_summary(
        manager,
    ))));
    actions
}

/// A slot worker reported its slot free.
pub fn on_slot_disconnected<A: Clone + PartialEq>(
    manager: &mut ConnManager<A>,
    slot: usize,
    attempt: u32,
) -> Vec<Action<A>, 2> {
    let mut actions = Vec::new();
    if !manager.is_current(slot, attempt) {
        return actions;
    }
    if let Some(address) = unsaved_pairing(manager, slot) {
        let _ = actions.push(Action::DiscardUnsavedBond(address));
    }
    manager.disconnect_slot(slot);
    let _ = actions.push(Action::Emit(link_state(manager)));
    actions
}

/// A slot worker's established link dropped (peer asleep, out of range or
/// powered off) and the worker is now silently trying to reconnect to it.
///
/// The slot stays reserved for that device, under the same attempt number,
/// so a connect request for another device cannot take it (selecting the same
/// device turns the retry into a user connection, see [`plan_connect`]), and
/// the UI shows only the links that are actually up.
pub fn on_slot_link_lost<A: Clone + PartialEq>(
    manager: &mut ConnManager<A>,
    slot: usize,
    attempt: u32,
    device: &DeviceInfo<A>,
) -> Vec<Action<A>, 1> {
    let mut actions = Vec::new();
    if !manager.is_current(slot, attempt) {
        return actions;
    }
    manager.set_slot(slot, device, attempt, false, true);
    let _ = actions.push(Action::Emit(link_state(manager)));
    actions
}

/// A slot worker reported an error. When `retrying`, the failed attempt was
/// a user connection that took over a background reconnect, and the worker
/// has gone back to that reconnect, so the slot stays reserved for the
/// device under the same attempt number; otherwise the slot is free.
pub fn on_slot_error<A: Clone + PartialEq>(
    manager: &mut ConnManager<A>,
    slot: usize,
    attempt: u32,
    tag: ErrorTag,
    retrying: bool,
) -> Vec<Action<A>, 3> {
    let mut actions = Vec::new();
    if !manager.is_current(slot, attempt) {
        return actions;
    }
    if let Some(address) = unsaved_pairing(manager, slot) {
        let _ = actions.push(Action::DiscardUnsavedBond(address));
    }
    match manager.slots.get_mut(slot) {
        Some(entry) if retrying => {
            entry.connected = false;
            entry.connecting = true;
            entry.retrying = true;
        }
        _ => manager.disconnect_slot(slot),
    }
    let _ = actions.push(Action::Emit(UiEvent::Error(tag)));
    let _ = actions.push(Action::Emit(link_state(manager)));
    actions
}
