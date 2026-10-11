//! Pure, I/O-free coordination logic for the multi-connection BLE manager.
//!
//! This is the **functional core** of the BLE subsystem: it owns the
//! connection-slot state machine and the decisions the coordinator makes in
//! response to UI commands and slot-worker events. Those decisions are returned
//! as data ([`Action`]s) which the async **imperative shell** in
//! `ble::multi_conn` (firmware only) then executes (channel sends, flash
//! writes).
//!
//! Because this module is free of SoftDevice / Embassy / USB types, it compiles
//! and runs on the host and is exercised directly by unit tests (it is in the
//! pure-core layer, see docs/architecture.md#module-layers-and-dependency-rules).
//! It is generic over the BLE address type so tests can substitute a trivial
//! stand-in for `nrf_softdevice::ble::Address`.

use core::fmt::Write;
use heapless::{String, Vec};

/// Maximum simultaneous BLE connections ([`crate::config::BLE_MAX_CONNECTIONS`]).
pub const MAX_CONNECTIONS: usize = crate::config::BLE_MAX_CONNECTIONS;

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
    StorageFailed,
    /// A pairing's bond names an identity address the store cannot keep (not
    /// public or random static), so neither the device nor its new keys were
    /// stored; or the device's own address has a reserved type and nothing
    /// was stored.
    BondRefused,
    ManagementFailed,
    ReportMapReadFailed,
    ReportMapTooLarge,
    ReportMapInvalid,
}

impl ErrorTag {
    /// The error a slot reports when securing a new link or discovering its
    /// HID service fails. A link that dropped meanwhile failed to connect, as
    /// if it had never come up, which a background reconnect retries; on a
    /// link still up the step's own error stands.
    pub fn for_failed_setup(self, link_up: bool) -> Self {
        if link_up {
            self
        } else {
            Self::ConnectFailed
        }
    }
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
    /// The connecting slot is a background reconnect, which never pairs.
    retrying: bool,
    /// The number of the attempt the slot's worker serves; 0 for a free slot.
    attempt: u32,
}

impl<A> Slot<A> {
    const fn empty() -> Self {
        Self {
            address: None,
            name: String::new(),
            connected: false,
            connecting: false,
            retrying: false,
            attempt: 0,
        }
    }

    fn is_occupied(&self) -> bool {
        self.connected || self.connecting
    }
}

/// The connection-slot state machine.
///
/// Each reservation gets an attempt number, which the slot's worker carries
/// on every event it reports for it. An event whose number is not the slot's
/// current one comes from an attempt the coordinator has since replaced or
/// ended, and the reducers ignore it: the coordinator handles UI commands
/// before queued worker events, so a selection can replace a background
/// reconnect whose Connected, Error or link-loss report is already queued.
pub struct ConnManager<A> {
    slots: [Slot<A>; MAX_CONNECTIONS],
    /// The number given to the latest reservation.
    last_attempt: u32,
}

impl<A: Clone + PartialEq> Default for ConnManager<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Clone + PartialEq> ConnManager<A> {
    pub const fn new() -> Self {
        Self {
            slots: [const { Slot::empty() }; MAX_CONNECTIONS],
            last_attempt: 0,
        }
    }

    /// A new attempt number. Numbers wrap but skip 0, which no event
    /// carries, so an event can never match a free slot.
    fn next_attempt(&mut self) -> u32 {
        self.last_attempt = self.last_attempt.wrapping_add(1).max(1);
        self.last_attempt
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
        self.slots.get(slot).is_some_and(Slot::is_occupied)
    }

    /// Identity currently reserved by a slot, including background retries.
    pub fn slot_address(&self, slot: usize) -> Option<&A> {
        self.slots.get(slot).and_then(|s| s.address.as_ref())
    }

    /// The number of the attempt a slot serves; 0, which no event carries,
    /// for a free or out-of-range slot.
    pub fn slot_attempt(&self, slot: usize) -> u32 {
        self.slots.get(slot).map_or(0, |s| s.attempt)
    }

    /// Whether an event numbered `attempt` comes from the attempt `slot`
    /// currently serves, rather than from one since replaced or ended.
    fn is_current(&self, slot: usize, attempt: u32) -> bool {
        self.slots
            .get(slot)
            .is_some_and(|s| s.is_occupied() && s.attempt == attempt)
    }

    /// Mark a slot as connecting (reserved) for the given device, on a
    /// connection the user asked for. Returns the attempt's number.
    pub fn reserve_slot(&mut self, slot: usize, device: &DeviceInfo<A>) -> u32 {
        let attempt = self.next_attempt();
        self.set_slot(slot, device, attempt, false, false);
        attempt
    }

    /// Mark a slot as reserved for a background reconnect to the given device
    /// at power-up. Returns the attempt's number.
    pub fn reserve_retry(&mut self, slot: usize, device: &DeviceInfo<A>) -> u32 {
        let attempt = self.next_attempt();
        self.set_slot(slot, device, attempt, false, true);
        attempt
    }

    /// Mark a slot as fully connected for the given device, keeping the
    /// number of the attempt that connected it; a slot connected without a
    /// reservation gets a new one. Returns the number.
    pub fn connect_slot(&mut self, slot: usize, device: &DeviceInfo<A>) -> u32 {
        let attempt = match self.slot_attempt(slot) {
            0 => self.next_attempt(),
            attempt => attempt,
        };
        self.set_slot(slot, device, attempt, true, false);
        attempt
    }

    fn set_slot(
        &mut self,
        slot: usize,
        device: &DeviceInfo<A>,
        attempt: u32,
        connected: bool,
        retrying: bool,
    ) {
        if let Some(entry) = self.slots.get_mut(slot) {
            *entry = Slot {
                address: Some(device.address.clone()),
                name: device.name.clone(),
                connected,
                connecting: !connected,
                retrying,
                attempt,
            };
        }
    }

    /// Clear a slot.
    pub fn disconnect_slot(&mut self, slot: usize) {
        if let Some(entry) = self.slots.get_mut(slot) {
            *entry = Slot::empty();
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
    match names.as_slice() {
        [] => {
            let mut s = String::new();
            let _ = s.push_str("Connected");
            s
        }
        [name] => name.clone(),
        many => {
            let mut s = String::new();
            let _ = write!(&mut s, "{} devices", many.len());
            s
        }
    }
}

/// The UI event that reports the links now up: `Disconnected` when none is,
/// otherwise `Connected` with the [`connection_summary`].
pub fn link_state<A: Clone + PartialEq>(manager: &ConnManager<A>) -> UiEvent {
    if manager.active_count() == 0 {
        UiEvent::Disconnected
    } else {
        UiEvent::Connected(connection_summary(manager))
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
    /// Tell a slot worker to connect to a device, numbering its events with
    /// `attempt`.
    ConnectSlot {
        slot: usize,
        device: DeviceInfo<A>,
        attempt: u32,
    },
    /// Persist a newly connected device (+ its bond) to flash.
    PersistDevice(DeviceInfo<A>),
    /// Drop the keys of any pairing made with the device at this address that
    /// the store has not saved: its attempt ended with no link up, so nothing
    /// will save them. Saved keys stay.
    DiscardUnsavedBond(A),
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
///
/// `same_peer` says whether two addresses belong to one peripheral: equal, or
/// both resolved by one bonded peer's identity key, which takes the
/// SoftDevice on the firmware. A slot already holding the selected peer is
/// used instead of a second one.
pub fn plan_connect<A: Clone + PartialEq>(
    manager: &mut ConnManager<A>,
    devices: &[DeviceInfo<A>],
    index: usize,
    same_peer: impl Fn(&A, &A) -> bool,
) -> Vec<Action<A>, 1> {
    let mut actions = Vec::new();

    let Some(device) = devices.get(index) else {
        let _ = actions.push(Action::Emit(UiEvent::Error(ErrorTag::ConnectFailed)));
        return actions;
    };

    let held = manager.slots.iter().enumerate().find(|(_, slot)| {
        slot.is_occupied()
            && slot
                .address
                .as_ref()
                .is_some_and(|address| same_peer(address, &device.address))
    });
    if let Some((slot, held)) = held {
        if held.connected {
            // Selecting an established peer must finish the UI's Connecting
            // state even though no new worker operation (or flash write) is
            // necessary.
            let _ = actions.push(Action::Emit(UiEvent::Connected(connection_summary(
                manager,
            ))));
        } else if held.retrying {
            // A background reconnect reports nothing until it succeeds, which
            // it never does for a peer that rejects the keys it holds, so
            // waiting for it would leave the UI on Connecting for good. The
            // selection becomes a connection the user asked for, which may
            // pair and reports its failure. The worker keeps a link or an
            // attempt already under way for the device, and goes back to the
            // background reconnect if the connection fails while it still
            // holds the device's keys.
            let attempt = manager.reserve_slot(slot, device);
            let _ = actions.push(Action::ConnectSlot {
                slot,
                device: device.clone(),
                attempt,
            });
        }
        // Otherwise a connection the user asked for still owns its slot and
        // will publish its own success or error; never report it as
        // established prematurely.
        return actions;
    }

    let Some(slot) = manager.find_empty_slot() else {
        let _ = actions.push(Action::Emit(UiEvent::Error(ErrorTag::ConnectFailed)));
        return actions;
    };

    let attempt = manager.reserve_slot(slot, device);
    let _ = actions.push(Action::ConnectSlot {
        slot,
        device: device.clone(),
        attempt,
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

// The reducers for slot-worker events live in a child module, which can read
// the slots' private state as this module does.
#[path = "coordinator_events.rs"]
mod events;
pub use events::*;

#[cfg(test)]
#[path = "coordinator_tests.rs"]
mod tests;
