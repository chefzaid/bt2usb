//! The simulated BLE side of the Renode build: the firmware's coordinator
//! decisions, scan merging, saved-device management, and paired-device list,
//! with the radio, the connection workers, and flash replaced by stand-ins
//! that answer at once.
//!
//! Without the SoftDevice nothing connects over the air, so a simulated
//! connection worker reports success as soon as it is told to connect or
//! disconnect, a scan hears the fixed advertisements in [`AIRWAVES`], and the
//! pairing item is written to RAM and read back. What runs is the firmware's
//! decision code: `coordinator::{plan_*, on_slot_*, merge_advertisement,
//! link_state}`, `management::{forget_targets, Quiescence, commit}`, and
//! `storage::devices::DeviceList` with its record codec and framing, called in
//! the order `ble::multi_conn` calls them.

use heapless::Vec;

use crate::ble::adv_parser::extract_device_name;
use crate::ble::coordinator::{self, Action, ConnManager, DeviceInfo, ErrorTag, MAX_CONNECTIONS};
use crate::ble::management::{self, Quiescence};
use crate::ble::messages::{Command, Event};
use crate::config::BLE_MAX_DISCOVERED;
use crate::storage::devices::{
    AddressKind, DeviceList, PeerAddress, StoreError, StoredDevice, MAX_RECORD_SIZE,
};
use crate::Console;

/// Stand-in for the SoftDevice `Address` type, which is unavailable without the
/// radio stack. The coordinator, messages, and UI controller are generic over
/// the address type precisely so the same logic runs here and in the firmware.
pub type SimAddr = u32;

/// The events for the UI that one command or scenario step produces.
pub type Events = Vec<Event<SimAddr>, 12>;

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

/// Everything a simulated scan hears, in the order it hears it.
const AIRWAVES: [Advertiser; 3] = [KEYBOARD, PHONE, MOUSE];

fn peripheral(advertiser: &Advertiser) -> DeviceInfo<SimAddr> {
    DeviceInfo {
        address: advertiser.address,
        name: extract_device_name(advertiser.data),
        rssi: advertiser.rssi,
    }
}

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
    fn save(&mut self, console: &mut Console, list: &mut DeviceList) -> Result<(), StoreError> {
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

    fn erase(&mut self, console: &mut Console) {
        self.len = 0;
        slog!(console, "  store: pairing pages erased");
    }
}

/// The coordinator task's state, as `multi_conn::ble_task` holds it.
pub struct SimBle {
    manager: ConnManager<SimAddr>,
    /// The scenario's keyboard and mouse, which connect on their own the way
    /// saved devices reconnect.
    peripherals: [DeviceInfo<SimAddr>; 2],
    /// Results of the last user scan, which `Command::Connect` indexes.
    scan: Vec<DeviceInfo<SimAddr>, BLE_MAX_DISCOVERED>,
    store: DeviceList,
    flash: Flash,
    management_token: u32,
    step: u32,
}

impl SimBle {
    pub fn new() -> Self {
        Self {
            manager: ConnManager::new(),
            peripherals: [peripheral(&KEYBOARD), peripheral(&MOUSE)],
            scan: Vec::new(),
            store: DeviceList::new(),
            flash: Flash {
                item: [0; MAX_RECORD_SIZE],
                len: 0,
            },
            management_token: 0,
            step: 0,
        }
    }

    /// Run a command from the UI and return the events it produces, as the
    /// coordinator would send them.
    pub async fn command(&mut self, console: &mut Console, command: Command<SimAddr>) -> Events {
        let mut events = Events::new();
        match command {
            Command::StartScan => {
                for action in coordinator::plan_start_scan(&self.manager) {
                    self.execute(console, action, &mut events);
                }
                self.run_scan(console, &mut events);
            }
            Command::Connect(index) => {
                let scan = self.scan.clone();
                for action in
                    coordinator::plan_connect(&mut self.manager, &scan, index, PartialEq::eq)
                {
                    self.execute(console, action, &mut events);
                }
            }
            Command::Disconnect => {
                for action in coordinator::plan_disconnect(&self.manager) {
                    self.execute(console, action, &mut events);
                }
            }
            Command::ListPaired { id } => {
                let devices = self
                    .store
                    .iter_recent()
                    .map(|stored| DeviceInfo {
                        address: sim_address(stored.address),
                        name: stored.name.clone(),
                        rssi: stored.last_rssi,
                    })
                    .collect();
                let _ = events.push(Event::PairedDevices { id, devices });
            }
            Command::Forget { id, address } => {
                let result = self.manage(console, Some(address), &mut events).await;
                let _ = events.push(Event::ManagementResult { id, result });
            }
            Command::FactoryReset { id } => {
                let result = self.manage(console, None, &mut events).await;
                // An explicit reset also invalidates the old scan results.
                self.scan.clear();
                let _ = events.push(Event::ManagementResult { id, result });
            }
        }
        events
    }

    /// Advance the scripted peripheral behavior by one step: the keyboard
    /// connects, the mouse connects, the keyboard's link drops (its slot stays
    /// reserved while the worker reconnects), then every link is closed.
    pub fn scenario_step(&mut self, console: &mut Console) -> Events {
        let mut events = Events::new();
        let step = self.step % 4;
        self.step = self.step.wrapping_add(1);
        match step {
            0 | 1 => {
                let index = step as usize;
                #[expect(
                    clippy::indexing_slicing,
                    reason = "this arm makes `index` 0 or 1, and `peripherals` is [_; 2]"
                )]
                let device = &self.peripherals[index];
                slog!(
                    console,
                    "scenario: connect device {} ({})",
                    index,
                    device.name
                );
                let peripherals = self.peripherals.clone();
                for action in
                    coordinator::plan_connect(&mut self.manager, &peripherals, index, PartialEq::eq)
                {
                    self.execute(console, action, &mut events);
                }
            }
            2 => {
                slog!(console, "scenario: slot 0 link lost");
                let device = self.manager.slot_address(0).and_then(|address| {
                    self.peripherals
                        .iter()
                        .find(|device| device.address == *address)
                        .cloned()
                });
                match device {
                    Some(device) if self.manager.is_slot_occupied(0) => {
                        for action in coordinator::on_slot_link_lost(&mut self.manager, 0, &device)
                        {
                            self.execute(console, action, &mut events);
                        }
                        let kept = self.manager.is_slot_occupied(0)
                            && self.manager.slot_address(0) == Some(&device.address);
                        slog!(
                            console,
                            "scenario: slot 0 {} for {:#x}",
                            if kept { "kept reserved" } else { "released" },
                            device.address
                        );
                    }
                    _ => slog!(console, "scenario: slot 0 has no link to lose"),
                }
            }
            _ => {
                slog!(console, "scenario: disconnect all");
                for action in coordinator::plan_disconnect(&self.manager) {
                    self.execute(console, action, &mut events);
                }
            }
        }
        slog!(
            console,
            "scenario: active_count={} occupied_count={}",
            self.manager.active_count(),
            self.manager.occupied_count()
        );
        events
    }

    /// Perform one coordinator action as `multi_conn::execute_action` does,
    /// with a connection worker that reports back at once.
    fn execute(&mut self, console: &mut Console, action: Action<SimAddr>, events: &mut Events) {
        match action {
            Action::ConnectSlot { slot, device } => {
                slog!(
                    console,
                    "  action: ConnectSlot slot={} addr={:#x}",
                    slot,
                    device.address
                );
                // The worker connects and finds the HID service at once.
                for next in coordinator::on_slot_connected(&mut self.manager, slot, &device) {
                    self.execute(console, next, events);
                }
            }
            Action::DisconnectSlot(slot) => {
                slog!(console, "  action: DisconnectSlot({})", slot);
                for next in coordinator::on_slot_disconnected(&mut self.manager, slot) {
                    self.execute(console, next, events);
                }
            }
            Action::PersistDevice(device) => {
                slog!(
                    console,
                    "  action: PersistDevice addr={:#x}",
                    device.address
                );
                let stored =
                    StoredDevice::new(peer_address(device.address), &device.name, device.rssi);
                self.store.add(stored, &no_irk);
                if self.flash.save(console, &mut self.store).is_err() {
                    let _ = events.push(Event::Error(ErrorTag::StorageFailed));
                }
            }
            Action::Emit(event) => {
                let _ = events.push(event.into());
            }
        }
    }

    /// A user scan: merge every advertisement heard through the firmware's
    /// scan reducer and report each HID device that enters the list.
    fn run_scan(&mut self, console: &mut Console, events: &mut Events) {
        self.scan.clear();
        let _ = events.push(Event::ScanStarted);
        for advertiser in &AIRWAVES {
            let listed = coordinator::merge_advertisement(
                &mut self.scan,
                advertiser.address,
                advertiser.rssi,
                advertiser.data,
            );
            if let Some(device) = self
                .scan
                .iter()
                .find(|device| listed && device.address == advertiser.address)
            {
                let _ = events.push(Event::DeviceFound(device.clone()));
            }
        }
        slog!(
            console,
            "  scan: heard {} advertisers, listed {} HID devices",
            AIRWAVES.len(),
            self.scan.len()
        );
        let _ = events.push(Event::ScanComplete);
    }

    /// Forget one saved device (`Some`) or reset them all (`None`) as
    /// `multi_conn::manage_devices` does: quiesce the affected slots behind a
    /// barrier, publish the new list only once it is saved, then report the
    /// links still up. Simulated workers acknowledge the barrier at once.
    async fn manage(
        &mut self,
        console: &mut Console,
        address: Option<SimAddr>,
        events: &mut Events,
    ) -> Result<(), ErrorTag> {
        let peer = match address {
            Some(address) => match self.store.find(peer_address(address), &no_irk) {
                Some(stored) => Some(stored.address),
                None => return Err(ErrorTag::ManagementFailed),
            },
            None => None,
        };
        self.management_token = self.management_token.wrapping_add(1);
        let token = self.management_token;
        let targets = match peer {
            None => [true; MAX_CONNECTIONS],
            Some(peer) => {
                management::forget_targets(&self.manager, |current| peer_address(*current) == peer)
            }
        };
        let mut barrier = Quiescence::new(targets, token);
        for (slot, &target) in targets.iter().enumerate() {
            if target && barrier.acknowledge(slot, token) {
                self.manager.disconnect_slot(slot);
                slog!(console, "  quiesce: slot {} released", slot);
            }
        }
        slog!(
            console,
            "  quiesce: {} for token {}",
            if barrier.complete() {
                "complete"
            } else {
                "INCOMPLETE"
            },
            token
        );
        let flash = &mut self.flash;
        let result = match peer {
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
        let _ = events.push(coordinator::link_state(&self.manager).into());
        slog!(
            console,
            "  slots: active_count={} occupied_count={}",
            self.manager.active_count(),
            self.manager.occupied_count()
        );
        result.map_err(|_| ErrorTag::StorageFailed)
    }
}
