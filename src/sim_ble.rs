//! The simulated BLE side of the Renode build: the firmware's coordinator
//! loop, `ble::coordinator_loop::run`, on a [`CoordinatorIo`] whose radio,
//! connection workers, and flash are stand-ins that answer at once.
//!
//! Without the SoftDevice nothing connects over the air, so a simulated
//! connection worker reports back as soon as it is told to connect,
//! disconnect, or quiesce, a scan hears the fixed advertisements in
//! [`AIRWAVES`], and the pairing item is written to RAM and read back. Links
//! drop and come back only when the scripted scenario says so
//! ([`Bus::scenario_step`]). Everything the loop decides runs as in the
//! firmware: the coordinator reducers, the management barrier, and the
//! loop's own order of calls, with `scan_list` merging what the scan hears
//! and `management::commit` publishing `storage::devices::DeviceList` with
//! its record codec and framing.

use core::cell::{Cell, RefCell};

use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use heapless::Vec;

use crate::ble::coordinator::{DeviceInfo, ErrorTag, MAX_CONNECTIONS};
use crate::ble::coordinator_loop::{CoordinatorIo, Input};
use crate::ble::management;
use crate::ble::messages::{Command, Event};
use crate::ble::scan_list;
use crate::ble::slot_loop::{SlotCommand, SlotEvent};
use crate::config::{BLE_MAX_DISCOVERED, MAX_PAIRED_DEVICES};
use crate::storage::devices::{
    AddressKind, DeviceList, PeerAddress, StoreError, StoredDevice, MAX_RECORD_SIZE,
};
use crate::Console;

/// Stand-in for the SoftDevice `Address` type, which is unavailable without the
/// radio stack. The coordinator, messages, and UI controller are generic over
/// the address type precisely so the same logic runs here and in the firmware.
pub type SimAddr = u32;

/// An advertiser the simulated scan hears.
struct Advertiser {
    address: SimAddr,
    rssi: i8,
    /// Advertising data: length-type-value structures, as on the air.
    data: &'static [u8],
}

/// Flags, the complete list of 16-bit service UUIDs holding HID (0x1812), and
/// the complete local name, as a keyboard in pairing mode advertises.
const KEYBOARD: Advertiser = Advertiser {
    address: 0xA1,
    rssi: -42,
    data: &[
        0x02, 0x01, 0x06, 0x03, 0x03, 0x12, 0x18, 0x09, 0x09, b'K', b'e', b'y', b'b', b'o', b'a',
        b'r', b'd',
    ],
};

/// The same structures for a mouse.
const MOUSE: Advertiser = Advertiser {
    address: 0xB2,
    rssi: -55,
    data: &[
        0x02, 0x01, 0x06, 0x03, 0x03, 0x12, 0x18, 0x06, 0x09, b'M', b'o', b'u', b's', b'e',
    ],
};

/// A phone: received more strongly than both, but without the HID service, so
/// the scan must not list it.
const PHONE: Advertiser = Advertiser {
    address: 0xC3,
    rssi: -30,
    data: &[0x02, 0x01, 0x06, 0x06, 0x09, b'P', b'h', b'o', b'n', b'e'],
};

/// Everything a simulated scan hears, in the order it hears it: the mouse
/// first, so the list shows the stronger keyboard above it only once sorted.
const AIRWAVES: [Advertiser; 3] = [MOUSE, PHONE, KEYBOARD];

/// The stored form of a simulated address: a random static address whose low
/// four bytes are the `u32`.
fn peer_address(address: SimAddr) -> PeerAddress {
    let [b0, b1, b2, b3] = address.to_le_bytes();
    PeerAddress::new(AddressKind::RandomStatic, [b0, b1, b2, b3, 0x00, 0xC0])
}

fn sim_address(address: PeerAddress) -> SimAddr {
    let [b0, b1, b2, b3, ..] = address.bytes;
    u32::from_le_bytes([b0, b1, b2, b3])
}

/// Simulated peers never pair, so no stored bond has an IRK to resolve with.
fn no_irk(_irk: &[u8; 16], _address: &[u8; 6]) -> bool {
    false
}

/// RAM stand-in for the pairing pages: the item the last save wrote.
struct Flash {
    item: [u8; MAX_RECORD_SIZE],
    len: usize,
}

impl Flash {
    /// Write the list's pending item, as `DeviceStore::save_to_flash` does,
    /// then read it back as boot would, so the record codec and framing run
    /// on the target.
    fn save(&mut self, console: &Console, list: &mut DeviceList) -> Result<(), StoreError> {
        let mut buf = [0u8; MAX_RECORD_SIZE];
        let Some(len) = list.pending_item(&mut buf)? else {
            return Ok(());
        };
        // Both buffers hold MAX_RECORD_SIZE bytes, the most `pending_item` writes.
        let (Some(item), Some(stored)) = (buf.get(..len), self.item.get_mut(..len)) else {
            return Err(StoreError::Serialization);
        };
        stored.copy_from_slice(item);
        self.len = len;
        list.mark_saved();
        let mut reloaded = DeviceList::new();
        let matches =
            reloaded.load(stored, &no_irk) && reloaded.iter_recent().eq(list.iter_recent());
        slog!(
            console,
            "  store: item of {} bytes holds {} device(s); reload {}",
            len,
            list.len(),
            if matches { "matches" } else { "DIFFERS" }
        );
        Ok(())
    }

    fn erase(&mut self, console: &Console) {
        self.len = 0;
        slog!(console, "  store: pairing pages erased");
    }
}

fn device_info(stored: &StoredDevice) -> DeviceInfo<SimAddr> {
    DeviceInfo {
        address: sim_address(stored.address),
        name: stored.name.clone(),
        rssi: stored.last_rssi,
    }
}

/// A simulated connection worker.
#[derive(Default)]
struct Worker {
    /// The number of the attempt it runs, or ran last, as a slot worker
    /// reports it.
    attempt: u32,
    /// The device it holds a link to or reconnects to; `None` when idle.
    device: Option<DeviceInfo<SimAddr>>,
    /// Whether the link to `device` is up; otherwise it is reconnecting.
    linked: bool,
}

/// What the simulation's two loops share, as the firmware's channels connect
/// its tasks: the UI loop's commands to the coordinator, the coordinator's
/// events to the UI, and the connection workers, which the coordinator's
/// commands and the scenario's steps both drive. The capacities are the
/// firmware's.
pub struct Bus {
    pub commands: Channel<NoopRawMutex, Command<SimAddr>, 4>,
    pub events: Channel<NoopRawMutex, Event<SimAddr>, 8>,
    slot_events: Channel<NoopRawMutex, SlotEvent<SimAddr>, 8>,
    workers: RefCell<[Worker; MAX_CONNECTIONS]>,
    step: Cell<u32>,
}

impl Bus {
    pub fn new() -> Self {
        Self {
            commands: Channel::new(),
            events: Channel::new(),
            slot_events: Channel::new(),
            workers: RefCell::new(Default::default()),
            step: Cell::new(0),
        }
    }

    /// Advance the scripted peripheral behavior by one step: the link in
    /// slot 0 drops, then the one in slot 1, and each worker reconnects in
    /// the background; then slot 0's device advertises again and its worker
    /// connects, then slot 1's. A step with nothing to change says so.
    pub fn scenario_step(&self, console: &Console) {
        let step = self.step.get();
        self.step.set(step.wrapping_add(1));
        let phase = step as usize % (2 * MAX_CONNECTIONS);
        let slot = phase % MAX_CONNECTIONS;
        let wakes = phase >= MAX_CONNECTIONS;
        let report = {
            let mut workers = self.workers.borrow_mut();
            let Some(worker) = workers.get_mut(slot) else {
                return;
            };
            match worker.device.clone() {
                Some(device) if worker.linked && !wakes => {
                    slog!(
                        console,
                        "scenario: slot {} link lost, reconnecting to {:#x}",
                        slot,
                        device.address
                    );
                    worker.linked = false;
                    SlotEvent::LinkLost {
                        slot,
                        attempt: worker.attempt,
                        device,
                    }
                }
                Some(device) if !worker.linked && wakes => {
                    slog!(
                        console,
                        "scenario: slot {} reconnected to {:#x}",
                        slot,
                        device.address
                    );
                    worker.linked = true;
                    SlotEvent::Connected {
                        slot,
                        attempt: worker.attempt,
                        device,
                    }
                }
                _ => {
                    let change = if wakes { "reconnect" } else { "lose" };
                    slog!(console, "scenario: slot {} has nothing to {}", slot, change);
                    return;
                }
            }
        };
        self.report(console, report);
    }

    /// Slot `slot`'s worker carries out `command` at once, as
    /// `slot_loop::run` would once the radio answered, and reports.
    fn work(&self, console: &Console, slot: usize, command: SlotCommand<SimAddr>) {
        let report = {
            let mut workers = self.workers.borrow_mut();
            let Some(worker) = workers.get_mut(slot) else {
                return;
            };
            match command {
                SlotCommand::Connect { device, attempt } => {
                    slog!(
                        console,
                        "  slot {}: Connect addr={:#x}",
                        slot,
                        device.address
                    );
                    // It connects and finds the HID service at once.
                    *worker = Worker {
                        attempt,
                        device: Some(device.clone()),
                        linked: true,
                    };
                    SlotEvent::Connected {
                        slot,
                        attempt,
                        device,
                    }
                }
                SlotCommand::Reconnect { device, attempt } => {
                    slog!(
                        console,
                        "  slot {}: Reconnect addr={:#x}",
                        slot,
                        device.address
                    );
                    // Retrying until a scenario step brings the device back.
                    *worker = Worker {
                        attempt,
                        device: Some(device),
                        linked: false,
                    };
                    return;
                }
                SlotCommand::Disconnect => {
                    slog!(console, "  slot {}: Disconnect", slot);
                    worker.device = None;
                    SlotEvent::Disconnected {
                        slot,
                        attempt: worker.attempt,
                    }
                }
                SlotCommand::Quiesce(token) => {
                    slog!(console, "  slot {}: Quiesce token {}", slot, token);
                    worker.device = None;
                    SlotEvent::Quiesced { slot, token }
                }
            }
        };
        self.report(console, report);
    }

    /// Queue a worker's report for the coordinator, which is the only reader:
    /// waiting here could never end.
    fn report(&self, console: &Console, event: SlotEvent<SimAddr>) {
        if self.slot_events.try_send(event).is_err() {
            slog!(console, "  slot event lost: channel full");
        }
    }
}

/// The coordinator loop's I/O: the bus, the paired-device list, and the RAM
/// stand-in for its flash pages.
pub struct SimIo<'a> {
    console: &'a Console,
    bus: &'a Bus,
    store: DeviceList,
    flash: Flash,
}

impl<'a> SimIo<'a> {
    pub fn new(console: &'a Console, bus: &'a Bus) -> Self {
        Self {
            console,
            bus,
            store: DeviceList::new(),
            flash: Flash {
                item: [0; MAX_RECORD_SIZE],
                len: 0,
            },
        }
    }
}

impl CoordinatorIo for SimIo<'_> {
    type Address = SimAddr;
    type Saved = PeerAddress;

    async fn load(&mut self) -> bool {
        // The RAM item is empty at every boot, as blank pairing pages are.
        self.store.load_empty();
        self.store.is_writable()
    }

    async fn reconnect_targets(&mut self) -> Vec<DeviceInfo<SimAddr>, MAX_CONNECTIONS> {
        // Simulated peers hold no keys, so unlike the firmware this needs
        // none; the store loads empty, so there is nothing to reconnect.
        self.store
            .iter_recent()
            .take(MAX_CONNECTIONS)
            .map(device_info)
            .collect()
    }

    async fn next(&mut self) -> Input<SimAddr> {
        match select(self.bus.commands.receive(), self.bus.slot_events.receive()).await {
            Either::First(command) => Input::Command(command),
            Either::Second(event) => Input::Slot(event),
        }
    }

    async fn slot_event(&mut self) -> SlotEvent<SimAddr> {
        self.bus.slot_events.receive().await
    }

    async fn emit(&mut self, event: Event<SimAddr>) {
        self.bus.events.send(event).await;
    }

    async fn command(&mut self, slot: usize, command: SlotCommand<SimAddr>) {
        self.bus.work(self.console, slot, command);
    }

    /// Merge every advertisement heard through the firmware's scan reducer,
    /// then report the list strongest first, as `ble::scanner` does when its
    /// window closes.
    async fn scan(&mut self) -> Option<Vec<DeviceInfo<SimAddr>, BLE_MAX_DISCOVERED>> {
        self.emit(Event::ScanStarted).await;
        let mut found = Vec::new();
        for advertiser in &AIRWAVES {
            scan_list::merge_advertisement(
                &mut found,
                advertiser.address,
                advertiser.rssi,
                advertiser.data,
            );
        }
        scan_list::strongest_first(&mut found);
        slog!(
            self.console,
            "  scan: heard {} advertisers, listed {} HID devices",
            AIRWAVES.len(),
            found.len()
        );
        for device in &found {
            self.emit(Event::DeviceFound(device.clone())).await;
        }
        self.emit(Event::ScanComplete).await;
        Some(found)
    }

    fn same_peer(&self, held: &SimAddr, seen: &SimAddr) -> bool {
        held == seen
    }

    async fn paired(&mut self) -> Vec<DeviceInfo<SimAddr>, MAX_PAIRED_DEVICES> {
        self.store
            .iter_recent()
            .take(MAX_PAIRED_DEVICES)
            .map(device_info)
            .collect()
    }

    async fn find(&mut self, identity: SimAddr) -> Option<PeerAddress> {
        self.store
            .find(peer_address(identity), &no_irk)
            .map(|stored| stored.address)
    }

    fn holds(&self, saved: &PeerAddress, address: &SimAddr) -> bool {
        peer_address(*address) == *saved
    }

    async fn forget(&mut self, saved: Option<PeerAddress>) -> bool {
        let console = self.console;
        let flash = &mut self.flash;
        let result = match saved {
            Some(peer) => match self.store.without(peer) {
                Ok(candidate) => {
                    management::commit(&mut self.store, candidate, async |next| {
                        flash.save(console, next)
                    })
                    .await
                }
                Err(error) => Err(error),
            },
            None => {
                let reset = self.store.reset();
                management::commit(&mut self.store, reset.candidate, async |next| {
                    if reset.erase_first {
                        flash.erase(console);
                    }
                    flash.save(console, next)
                })
                .await
            }
        };
        result.is_ok()
    }

    /// Save the device without keys, which simulated peers never have, so
    /// unlike the firmware there are no notices: only `StorageFailed` for a
    /// failed save.
    async fn persist(&mut self, device: DeviceInfo<SimAddr>) {
        let stored = StoredDevice::new(peer_address(device.address), &device.name, device.rssi);
        self.store.add(stored, &no_irk);
        if self.flash.save(self.console, &mut self.store).is_err() {
            self.emit(Event::Error(ErrorTag::StorageFailed)).await;
        }
    }

    // The simulation pairs nothing, so it holds no keys to discard.
    fn discard_unsaved_bond(&mut self, _address: SimAddr) {}
}
