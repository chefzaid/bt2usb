//! Pure, I/O-free coordination logic for the multi-connection BLE manager.
//!
//! This is the **functional core** of the BLE subsystem: it owns the
//! connection-slot state machine and the decisions the coordinator makes in
//! response to UI commands and slot-worker events. Those decisions are returned
//! as data ([`Action`]s) which the async **imperative shell** in
//! [`crate::ble::multi_conn`] then executes (channel sends, flash writes).
//!
//! Because this module is free of SoftDevice / Embassy / USB types, it compiles
//! and runs on the host and is exercised directly by unit tests (the
//! orchestration layer of the README "Testing Strategy"). It is generic over the
//! BLE address type so tests can substitute a trivial stand-in for
//! `nrf_softdevice::ble::Address`.

use core::fmt::Write;
use heapless::{String, Vec};

/// Maximum simultaneous BLE connections.
pub const MAX_CONNECTIONS: usize = 2;

/// Lightweight error tag surfaced to the UI (no dynamic allocation).
///
/// On the embedded target this is re-exported as `BleErrorTag`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ErrorTag {
    ScanFailed,
    ConnectFailed,
    HidNotFound,
    NotifyFailed,
}

/// Minimal device identity the coordinator needs.
///
/// Generic over the address type `A` so host tests don't depend on the
/// embedded `Address` type. On the embedded target `DiscoveredDevice` is a type
/// alias for `DeviceInfo<Address>`.
///
/// `Debug`/`PartialEq` are derived with the usual bounds, so they only require
/// `A: Debug`/`A: PartialEq` where actually used (e.g. host tests); the embedded
/// `Address` never needs them.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct DeviceInfo<A> {
    pub address: A,
    pub name: String<32>,
    pub rssi: i8,
}

/// One connection slot.
#[derive(Clone)]
pub struct Slot<A> {
    address: Option<A>,
    name: String<32>,
    connected: bool,
    connecting: bool,
}

impl<A> Slot<A> {
    const fn empty() -> Self {
        Self {
            address: None,
            name: String::new(),
            connected: false,
            connecting: false,
        }
    }

    fn is_occupied(&self) -> bool {
        self.connected || self.connecting
    }
}

/// The connection-slot state machine.
pub struct ConnManager<A> {
    slots: [Slot<A>; MAX_CONNECTIONS],
}

impl<A: Clone + PartialEq> Default for ConnManager<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Clone + PartialEq> ConnManager<A> {
    pub const fn new() -> Self {
        Self {
            slots: [Slot::empty(), Slot::empty()],
        }
    }

    /// First slot that is neither connected nor connecting.
    pub fn find_empty_slot(&self) -> Option<usize> {
        self.slots.iter().position(|s| !s.is_occupied())
    }

    /// Number of slots with an established (connected) link.
    pub fn active_count(&self) -> usize {
        self.slots.iter().filter(|s| s.connected).count()
    }

    /// Number of slots that are connected or mid-connect.
    pub fn occupied_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_occupied()).count()
    }

    /// Is the given slot index connected or mid-connect?
    pub fn is_slot_occupied(&self, slot: usize) -> bool {
        slot < MAX_CONNECTIONS && self.slots[slot].is_occupied()
    }

    /// Is this address already in use by an occupied slot?
    pub fn is_connected_address(&self, address: &A) -> bool {
        self.slots
            .iter()
            .any(|s| s.is_occupied() && s.address.as_ref() == Some(address))
    }

    /// Mark a slot as connecting (reserved) for the given device.
    pub fn reserve_slot(&mut self, slot: usize, device: &DeviceInfo<A>) {
        if slot < MAX_CONNECTIONS {
            self.slots[slot] = Slot {
                address: Some(device.address.clone()),
                name: device.name.clone(),
                connected: false,
                connecting: true,
            };
        }
    }

    /// Mark a slot as fully connected for the given device.
    pub fn connect_slot(&mut self, slot: usize, device: &DeviceInfo<A>) {
        if slot < MAX_CONNECTIONS {
            self.slots[slot] = Slot {
                address: Some(device.address.clone()),
                name: device.name.clone(),
                connected: true,
                connecting: false,
            };
        }
    }

    /// Clear a slot.
    pub fn disconnect_slot(&mut self, slot: usize) {
        if slot < MAX_CONNECTIONS {
            self.slots[slot] = Slot::empty();
        }
    }

    /// Names of all connected (not merely connecting) devices.
    pub fn get_connected_names(&self) -> Vec<String<32>, MAX_CONNECTIONS> {
        let mut names = Vec::new();
        for slot in &self.slots {
            if slot.connected {
                let _ = names.push(slot.name.clone());
            }
        }
        names
    }
}

/// A short human-readable summary of the current connections for the UI.
pub fn connection_summary<A: Clone + PartialEq>(manager: &ConnManager<A>) -> String<32> {
    let names = manager.get_connected_names();
    match names.len() {
        0 => {
            let mut s = String::new();
            let _ = s.push_str("Connected");
            s
        }
        1 => names[0].clone(),
        n => {
            let mut s = String::new();
            let _ = write!(&mut s, "{} devices", n);
            s
        }
    }
}

/// UI-facing events the coordinator wants emitted.
#[derive(Clone, PartialEq, Debug)]
pub enum UiEvent {
    Connected(String<32>),
    Disconnected,
    Error(ErrorTag),
}

/// Side effects the imperative shell must perform, as data.
#[derive(Clone, PartialEq, Debug)]
pub enum Action<A> {
    /// Tell a slot worker to disconnect.
    DisconnectSlot(usize),
    /// Tell a slot worker to connect to a device.
    ConnectSlot { slot: usize, device: DeviceInfo<A> },
    /// Persist a newly connected device (+ its bond) to flash.
    PersistDevice(DeviceInfo<A>),
    /// Emit a UI event.
    Emit(UiEvent),
}

// ─── Reducers ──────────────────────────────────────────────────────────────
//
// Each takes the current manager state (sometimes mutating it the same way the
// live system would) and returns the actions the shell should execute.

/// Decide what must happen before a new scan starts: if every slot is busy we
/// free them all so the user can pick a fresh device.
pub fn plan_start_scan<A: Clone + PartialEq>(
    manager: &ConnManager<A>,
) -> Vec<Action<A>, MAX_CONNECTIONS> {
    let mut actions = Vec::new();
    if manager.occupied_count() >= MAX_CONNECTIONS {
        for slot in 0..MAX_CONNECTIONS {
            if manager.is_slot_occupied(slot) {
                let _ = actions.push(Action::DisconnectSlot(slot));
            }
        }
    }
    actions
}

/// Decide how to handle a connect request for `devices[index]`, reserving a
/// slot on success.
pub fn plan_connect<A: Clone + PartialEq>(
    manager: &mut ConnManager<A>,
    devices: &[DeviceInfo<A>],
    index: usize,
) -> Vec<Action<A>, 1> {
    let mut actions = Vec::new();

    let Some(device) = devices.get(index) else {
        let _ = actions.push(Action::Emit(UiEvent::Error(ErrorTag::ConnectFailed)));
        return actions;
    };

    if manager.is_connected_address(&device.address) {
        // Already connected — ignore (no error, no duplicate link).
        return actions;
    }

    let Some(slot) = manager.find_empty_slot() else {
        let _ = actions.push(Action::Emit(UiEvent::Error(ErrorTag::ConnectFailed)));
        return actions;
    };

    manager.reserve_slot(slot, device);
    let _ = actions.push(Action::ConnectSlot {
        slot,
        device: device.clone(),
    });
    actions
}

/// Disconnect every occupied slot (user pressed "disconnect").
pub fn plan_disconnect<A: Clone + PartialEq>(
    manager: &ConnManager<A>,
) -> Vec<Action<A>, MAX_CONNECTIONS> {
    let mut actions = Vec::new();
    for slot in 0..MAX_CONNECTIONS {
        if manager.is_slot_occupied(slot) {
            let _ = actions.push(Action::DisconnectSlot(slot));
        }
    }
    actions
}

/// A slot worker reported a successful connection.
pub fn on_slot_connected<A: Clone + PartialEq>(
    manager: &mut ConnManager<A>,
    slot: usize,
    device: &DeviceInfo<A>,
) -> Vec<Action<A>, 2> {
    let mut actions = Vec::new();
    manager.connect_slot(slot, device);
    let _ = actions.push(Action::PersistDevice(device.clone()));
    let _ = actions.push(Action::Emit(UiEvent::Connected(connection_summary(
        manager,
    ))));
    actions
}

/// A slot worker reported a disconnection.
pub fn on_slot_disconnected<A: Clone + PartialEq>(
    manager: &mut ConnManager<A>,
    slot: usize,
) -> Vec<Action<A>, 1> {
    let mut actions = Vec::new();
    manager.disconnect_slot(slot);
    let event = if manager.active_count() == 0 {
        UiEvent::Disconnected
    } else {
        UiEvent::Connected(connection_summary(manager))
    };
    let _ = actions.push(Action::Emit(event));
    actions
}

/// A slot worker reported an error.
pub fn on_slot_error<A: Clone + PartialEq>(
    manager: &mut ConnManager<A>,
    slot: usize,
    tag: ErrorTag,
) -> Vec<Action<A>, 2> {
    let mut actions = Vec::new();
    manager.disconnect_slot(slot);
    let _ = actions.push(Action::Emit(UiEvent::Error(tag)));
    let event = if manager.active_count() == 0 {
        UiEvent::Disconnected
    } else {
        UiEvent::Connected(connection_summary(manager))
    };
    let _ = actions.push(Action::Emit(event));
    actions
}

#[cfg(test)]
#[path = "coordinator_tests.rs"]
mod tests;
