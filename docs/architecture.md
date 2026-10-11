# Architecture Overview And ADR Index

This guide describes how the bt2usb firmware is put together at runtime: its
layers and tasks, the path a command or an input report takes through them, how
failures surface and recover, and the rules that keep the design testable. It
also indexes the architecture decision records (ADRs) that explain why.
Byte-level formats live in the [data model](data-model.md); pins, constants, and
the memory map live in [hardware](hardware.md); how each layer is checked lives
in [testing](testing.md).

Status words in this guide are used precisely. **Implemented** means the code is
in this repository. **Host-tested** means `cargo test --lib --tests` exercises
the logic on a PC. **Simulated** means the Renode scenario exercises it on an
emulated nRF52840. **Hardware-verified** requires a recorded
[first-flash](first-flash.md) result. The repository holds no such record yet:
the [2026-09-28 validation record](testing.md#validation-record--2026-09-28) lists
board, radio, and USB acceptance as not performed. The commit history
mentions fixes made during board bring-up (`f477d4c`), but without stored
evidence. Every lifecycle below is therefore at most host-tested or simulated.

## System At A Glance

bt2usb uses `no_std` Rust, static allocation, and Embassy's cooperative async
executor ([ADR 0002](adr/0002-nrf52840-softdevice-embassy.md)). Nordic SoftDevice
S140 supplies the BLE central stack. USB uses the nRF52840 device peripheral
through Embassy. The UI runs in `main`; a separate display task renders its
latest state over async I2C.

```mermaid
flowchart LR
    BT[BLE keyboard / mouse] -->|BLE HID over GATT| FW[nRF52840 bt2usb]
    FW -->|USB HID: keyboard, mouse, consumer| HUB[Monitor USB hub]
    HUB --> PC[PC USB upstream]
    UI[OLED + three buttons] <--> FW
    FW --> FLASH[(Pairing store in internal flash)]
```

The repository builds three firmware binaries from one source tree:

| Binary | Feature | Purpose |
| --- | --- | --- |
| `bt2usb` | `embedded` | The bridge |
| `bt2usb-selftest` | `embedded` | Staged board bring-up; see [first flash](first-flash.md) |
| `bt2usb-sim` | `sim` | SoftDevice-free Renode build; see [testing](testing.md#renode-simulation) |

The host library (`src/lib.rs`) exports the hardware-free modules those binaries
share ([ADR 0003](adr/0003-pure-core-and-task-shell.md)). The three binaries use
the same modules, not copies: the self-test reuses the SoftDevice setup, USB
device, and display driver, and the simulation reuses the button driver, the
display task, the UI controller, and the BLE coordinator and storage modules,
with custom Renode GPIO models ([ADR 0014](adr/0014-renode-gpio-models.md))
and TWIM and SSD1306 models ([ADR 0024](adr/0024-renode-oled-models.md)).

## Source Map

| Area | Layer | Responsibility |
| --- | --- | --- |
| [main.rs](../src/main.rs) | Entry point | Hardware setup, task spawning, channels, and the UI loop that runs the UI controller |
| [sim.rs](../src/sim.rs) | Entry point | SoftDevice-free Renode entry point: UART console, button tasks, the display task, and the UI loop over the UI controller |
| [sim_ble.rs](../src/sim_ble.rs) | Simulation shell | Renode stand-in for the BLE coordinator task: the real coordinator reducers, scan merging, management barrier and commit, and device list, with workers, radio, and flash that answer at once |
| [selftest.rs](../src/selftest.rs) | Entry point | Staged board bring-up image |
| [lib.rs](../src/lib.rs) | Host crate | Host-test entry point for hardware-free logic |
| [config.rs](../src/config.rs) | Constants | Timing, scan, connection, USB identity, and storage constants |
| [diagnostics.rs](../src/diagnostics.rs) | Pure core | The build identity the boot line reports (version, source commit, profile, `DEFMT_LOG` filter), the decoding of `POWER.RESETREAS` into reset causes, and the saturating event counters (`COUNTERS`) with the policy for when the UI loop logs them (`CounterReport`) |
| [build.rs](../build.rs) | Build script | Writes the linker memory layout with the storage bounds, and passes the build identity (git commit, profile, `DEFMT_LOG`) to the crate |
| [sd_setup.rs](../src/sd_setup.rs) | Board shell | Shared SoftDevice setup and USB power events |
| [power.rs](../src/power.rs) | Board shell | Activity tracking and power state over `embassy-time` |
| [power_logic.rs](../src/power_logic.rs) | Pure core | Pure power/display policy |
| [stack.rs](../src/stack.rs) | Board shell | Painted-stack high-water measurement, the MPU stack guard (programming and read-back), and the HardFault and MemManage handlers that report a stack overflow |
| [stack_logic.rs](../src/stack_logic.rs) | Pure core | Where the stack guard goes (`StackGuard::place`) and the MPU register values for it ([ADR 0026](adr/0026-mpu-stack-guard.md)) |
| [ble/mod.rs](../src/ble/mod.rs) | Board shell | `BleCommand`, `BleEvent`, and `DiscoveredDevice` over the SoftDevice `Address`, and the GAP procedure lock |
| [ble/adv_parser.rs](../src/ble/adv_parser.rs) | Pure core | HID service UUID and device-name parsing from advertisements |
| [ble/bond_table.rs](../src/ble/bond_table.rs) | Pure core | The bonding keys held in RAM, each saved or not: a new pairing never displaces a saved device's keys, and one that is not saved is discarded |
| [ble/conn_params.rs](../src/ble/conn_params.rs) | Pure core | Bounds for a peripheral's connection parameter request |
| [ble/coordinator.rs](../src/ble/coordinator.rs) | Pure core | Pure connection-slot state and command reducers; the slot-event reducers are in its child module [coordinator_events.rs](../src/ble/coordinator_events.rs) |
| [ble/scan_list.rs](../src/ble/scan_list.rs) | Pure core | The bounded user-scan device list: HID-UUID admission, strongest-signal replacement when full, name updates |
| [ble/reconnect.rs](../src/ble/reconnect.rs) | Pure core | Background-reconnect table shared by both slots: saved-device identity, targets, sightings handed between slots, wakes, scan duty |
| [ble/long_read.rs](../src/ble/long_read.rs) | Pure core | Bounded fragmented Report Map acquisition |
| [ble/management.rs](../src/ble/management.rs) | Pure core | Peer-management targets, quiescence, and transactional commit primitives |
| [ble/messages.rs](../src/ble/messages.rs) | Pure core | UI-to-coordinator commands and coordinator-to-UI events, generic over the address type |
| [ble/pnp_id.rs](../src/ble/pnp_id.rs) | Pure core | Parsing of the Device Information Service PnP ID the bridge logs per link |
| [ble/multi_conn.rs](../src/ble/multi_conn.rs) | Board shell | BLE coordinator task, slot command and event types, user scans, saved-device management |
| [ble/slot_worker.rs](../src/ble/slot_worker.rs) | Board shell | Connection worker run once per slot: serves the coordinator's commands, reports slot events, retries in the background |
| [ble/slot_link.rs](../src/ble/slot_link.rs) | Board shell | One attempt of a slot worker: connect, secure, discover HID, and run the notification loop, racing each phase against the slot's commands |
| [ble/bonder.rs](../src/ble/bonder.rs) | Board shell | SoftDevice security handler: bond storage and the answer to connection-parameter requests |
| [ble/hid_client.rs](../src/ble/hid_client.rs) | Board shell | GATT discovery, subscriptions, HID classification, notification loop |
| [ble/device_info.rs](../src/ble/device_info.rs) | Board shell | Device Information Service discovery and the PnP ID read and log line, once per link |
| [ble/scanner.rs](../src/ble/scanner.rs) | Board shell | User scan with HID advertisement filtering, and the shared reconnect scan |
| [hid/mod.rs](../src/hid/mod.rs) | Pure core | Internal report type and notification classification |
| [hid/report_protocol.rs](../src/hid/report_protocol.rs) | Pure core | Report Map parser and Report Reference descriptor |
| [hid/keyboard.rs](../src/hid/keyboard.rs), [mouse.rs](../src/hid/mouse.rs), [consumer.rs](../src/hid/consumer.rs) | Pure core | Report types, USB report descriptors, serialization |
| [hid/coalesce.rs](../src/hid/coalesce.rs) | Pure core | Per-connection backpressure coalescing |
| [hid/aggregate.rs](../src/hid/aggregate.rs) | Pure core | Per-source union of held input |
| [hid/delivery.rs](../src/hid/delivery.rs) | Pure core | Endpoint queue state and the endpoint worker loop |
| [hid/wake.rs](../src/hid/wake.rs) | Pure core | Remote-wakeup eligibility |
| [hid/host_leds.rs](../src/hid/host_leds.rs) | Pure core | Host LED forwarding: the current state when a link starts, then each change |
| [usb/hid_device.rs](../src/usb/hid_device.rs) | Board shell | Composite device, VBUS, suspend, report writing |
| [usb/host_requests.rs](../src/usb/host_requests.rs) | Board shell | Host control requests: boot/report protocol and the keyboard LED output report |
| [storage.rs](../src/storage.rs) | Board shell | Flash load and save with write retries, conversion to and from SoftDevice address and key types, storage log lines |
| [storage/devices.rs](../src/storage/devices.rs) | Pure core | Paired-device list: fail-closed load, legacy format, identity merge, eviction, forget, factory reset |
| [storage/framing.rs](../src/storage/framing.rs), [record.rs](../src/storage/record.rs) | Pure core | Versioned frame and record validation |
| [storage/codec.rs](../src/storage/codec.rs) | Pure core | Device, address, and bond record codec |
| [ui/controller.rs](../src/ui/controller.rs) | Pure core | UI loop decisions: the command each button press sends, how each coordinator event changes the view, management request IDs and deadlines, error messages |
| [ui/ui_logic.rs](../src/ui/ui_logic.rs) | Pure core | Screens, button reducer, view model |
| [ui/input_logic.rs](../src/ui/input_logic.rs) | Pure core | Device-list window and scan spinner |
| [ui/display_logic.rs](../src/ui/display_logic.rs) | Pure core | OLED retry backoff policy |
| [ui/layout.rs](../src/ui/layout.rs) | Pure core | Each screen's text lines and their baselines on the 128×64 panel |
| [ui/display.rs](../src/ui/display.rs) | Board shell | OLED task, TWIM0 setup, panel initialization, drawing the layout's lines, STOP-safe I2C wrapper |
| [ui/buttons.rs](../src/ui/buttons.rs) | Board shell | Debounced GPIO button task |

## Module Layers And Dependency Rules

The code is split into a pure core and a board shell
([ADR 0003](adr/0003-pure-core-and-task-shell.md)). The split is enforced by
what `src/lib.rs` compiles: the host crate mounts only pure modules, so a pure
module that imports a hardware crate stops `cargo test` from building.

| Layer | Modules | Compiled into | Verified by |
| --- | --- | --- | --- |
| Pure core | `hid::*`, `ble::{adv_parser, bond_table, conn_params, coordinator, scan_list, reconnect, long_read, management, messages, pnp_id}`, `diagnostics`, `stack_logic`, `power_logic`, `ui::{controller, ui_logic, input_logic, layout, display_logic}`, `storage::{codec, devices, framing, record}` | Host crate and each firmware binary that declares them | Host tests; the Renode scenario runs the UI, layout, coordinator, management, and storage modules on the simulated target |
| Board shell | `ble::{mod, multi_conn, slot_worker, slot_link, bonder, hid_client, device_info, scanner}`, `usb::{hid_device, host_requests}`, the `storage` shell, `power`, `sd_setup`, `stack`, `ui::{display, buttons}` | Firmware binaries only | Embedded build and Clippy; board self-test; hardware acceptance; the Renode scenario also runs `ui::{display, buttons}` on modelled peripherals |
| Entry points | `main.rs`, `selftest.rs`, `sim.rs` (with `sim_ble.rs`) | One binary each | Embedded or simulation build; Renode for `sim.rs` |
| Constants | `config.rs` | Host crate, each firmware binary, and `build.rs` | Review; documented in [hardware](hardware.md#configuration-defaults); the linker checks the storage range |

`lib.rs` mounts the shared files with `#[path]` attributes and re-exports them
through `pub mod ble`, `pub mod ui`, and `pub mod power_logic` facades, so a
path such as `crate::ble::coordinator` resolves the same way in the library and
the firmware. `hid` is exported whole. `storage::{codec, devices, framing,
record}` are compiled into the library only under `cfg(test)`, inside an inline
`storage` module so that they reach each other through `super::` in both
builds; the `storage` shell is not, because it converts SoftDevice address and
key types and does the flash I/O.

```mermaid
flowchart TD
    subgraph entry [Entry points]
        MAIN["main.rs"]
        SELF["selftest.rs"]
        SIM["sim.rs and sim_ble.rs"]
    end
    subgraph shell [Board shell]
        MC["ble::multi_conn"]
        SW["ble::slot_worker and slot_link"]
        BND["ble::bonder"]
        HC["ble::hid_client"]
        DI["ble::device_info"]
        SC["ble::scanner"]
        ST["storage shell"]
        USB["usb::hid_device and host_requests"]
        PWR["power"]
        DSP["ui::display"]
        BTN["ui::buttons"]
        SDS["sd_setup"]
        STK["stack"]
    end
    subgraph core [Pure core exported by lib.rs]
        BC["ble::coordinator, scan_list, bond_table, messages, reconnect, conn_params, management, long_read, adv_parser, pnp_id"]
        DG["diagnostics"]
        SL["stack_logic"]
        HID["hid modules"]
        UIL["ui::controller, ui_logic, input_logic, display_logic"]
        PL["power_logic"]
        SF["storage::devices, codec, framing, record"]
    end
    MAIN --> MC & SW & USB & DSP & BTN & PWR & SDS & STK & UIL & HID & DG
    SELF --> USB & DSP & SDS & STK & BC & HID & DG
    SIM --> BC & UIL & BTN & SF & STK & DG
    STK --> SL
    MC --> BND & SC & ST & BC
    SW --> MC & BND & HC & SC & USB & BC & HID & DG
    BND --> ST & BC
    HC --> DI & HID & BC & USB & DG
    DI --> BC
    SC --> BC
    ST --> SF & BC & DG
    USB --> HID & PWR & DG
    PWR --> PL
    DSP --> UIL
    BTN --> UIL
    UIL --> BC
```

The edges above come from the `use` statements in each file. The rules they
follow:

- Dependencies point from entry points to the shell and from the shell to the
  core. No pure module imports a shell module.
- Pure modules import only `core`, `heapless`, and other pure modules. They do
  not import `embassy_*`, `nrf_softdevice`, `cortex_m`, or `sequential_storage`;
  `defmt` appears only behind `#[cfg(feature = "defmt")]` or `cfg_attr`.
- Where the real type is hardware-bound, the core is generic or takes a
  callback: `DeviceInfo<A>`, `ConnManager<A>`, and `Action<A>` are generic over
  the address type; `reconnect::ReconnectTable` is generic over the target and
  address types and `reconnect::owner_of` takes a matcher, so IRK resolution
  stays in the SoftDevice; `host_leds::forward_host_leds` takes the `HostLeds`
  trait and a write closure; `delivery::run_endpoint` takes the
  `DeliveryQueue`, `ReportSink`, and `RetryClock` traits; `management::commit`
  takes an async persist closure; `display_logic::Recovery` takes milliseconds.
- Pure modules size their buffers from `config.rs` where a value is shared
  with the shell (the link count `BLE_MAX_CONNECTIONS` behind
  `coordinator::MAX_CONNECTIONS` and `aggregate::SOURCES`, and the
  `UiState` list capacities `BLE_MAX_DISCOVERED` and `MAX_PAIRED_DEVICES`).
  Limits only one module uses stay in that module (`ENDPOINT_QUEUE_CAPACITY`,
  `MAX_ATTRIBUTE_LEN`) or come in as parameters. `config.rs` holds constants
  only, with no imports, because `build.rs` compiles it as well.
- Two shell links cross subsystem boundaries on purpose: the connection
  workers take a `usb::host_requests::LedReceiver` for host keyboard LEDs, and the
  USB dispatcher calls `power::note_hid_activity`.

A new module goes into the pure core when its decisions can be expressed
without hardware types; mount it in `lib.rs` and add tests with it. Task code
that wires it up stays in the shell. See also [code quality](code-quality.md).

## Tasks And Data Flow

```mermaid
flowchart TD
    SD[SoftDevice task] --> BLE[BLE coordinator]
    BLE --> SLOT[Two BLE connection workers]
    SLOT -->|Source-tagged input and disconnect events| DISPATCH[Input aggregator / dispatcher]
    DISPATCH --> KB[Keyboard endpoint worker]
    DISPATCH --> MOUSE[Mouse endpoint worker]
    DISPATCH --> MEDIA[Consumer endpoint worker]
    KB --> USB[USB device task]
    MOUSE --> USB
    MEDIA --> USB
    BUTTONS[Three GPIO button tasks] --> UI[Main UI loop]
    UI -->|Commands| BLE
    BLE -->|Events| UI
    UI -->|Latest display state| DISPLAY[OLED task]
    USB -->|Latest keyboard LED state| SLOT
    SD -->|USB power events| USB
```

Bounded Embassy channels use `CriticalSectionRawMutex`. The BLE coordinator sends
commands to per-slot workers; a shared GAP mutex serializes scan and connection
establishment because SoftDevice permits one such procedure at a time. A link
does not hold that mutex for its whole lifetime. A requested scan can therefore
wait for an in-flight connection attempt.

| Channel | Direction | Capacity |
| --- | --- | --- |
| `HID_REPORT_CHANNEL` | BLE workers → input aggregator/dispatcher | 16 |
| `BLE_CMD_CHANNEL` | UI → coordinator | 4 |
| `BLE_EVENT_CHANNEL` | Coordinator → UI | 8 |
| `BLE_SLOT_CMD_CHANNELS` (one per link) | Coordinator → worker | 2 each |
| `BLE_SLOT_EVENT_CHANNEL` | Workers → coordinator | 8 |
| `BUTTON_CHANNEL` | GPIO tasks → UI | 4 |

The USB keyboard LED state uses a `Watch`, so both slots can observe the latest
state. USB suspend/resume and wake requests use signals and atomic state. The
full list of signals and shared state is in
[concurrency](#channels-signals-and-shared-state).

The dispatcher does not wait for USB writes
([ADR 0005](adr/0005-two-slots-and-independent-endpoints.md)). The dispatcher
and the keyboard, mouse, and consumer workers are four futures joined inside
the one `hid_writer_task`, so they run concurrently on the executor. Each
worker has a 16-report FIFO, a 100 ms write deadline, and 20–1000 ms retry
backoff. An unpolled endpoint cannot block the other two. Reset,
configuration, resume, and protocol transitions invalidate old transfers and
replay current held state; relative mouse motion is never replayed.

## Key Runtime Lifecycles

Each lifecycle below is traced from the source. Log strings are quoted as they
appear in the code and reach the host through RTT; see
[operations](operations.md) for reading them.

### Boot And Initialization

`main` in [main.rs](../src/main.rs) runs these steps in order:

1. Logs `"bt2usb firmware starting: ..."` with the build identity from
   [diagnostics.rs](../src/diagnostics.rs) (version, source commit, profile,
   `DEFMT_LOG` filter, which `build.rs` supplies), turns on the MPU stack guard
   and logs `"stack guard: {} bytes at {}..{}"` (or `"stack guard off: {}"`;
   [ADR 0026](adr/0026-mpu-stack-guard.md)), reads and clears
   `POWER.RESETREAS` and logs the decoded `"reset reason: {}"` while the POWER
   peripheral is still the application's, then calls `embassy_nrf::init` with
   the GPIOTE and time-driver interrupt priorities set to P2.
2. Sets the USBD and TWISPI0 interrupts to P2, because the SoftDevice reserves
   priorities 0, 1, and 4 (see [interrupt priorities](#interrupt-priorities)).
3. Enables the SoftDevice with `sd_setup::softdevice_config()`: the internal RC
   low-frequency clock declared at 500 ppm with periodic calibration, two
   central links with `BLE_CONN_EVENT_LENGTH`, a 64-byte ATT MTU, no
   advertising or peripheral role, and two central security contexts. The
   vendored `nrf-softdevice` logs `"softdevice RAM: {:?} bytes"` and panics if
   [memory_sd.x](../memory_sd.x) reserves too little RAM.
4. `sd_setup::enable_usb_power_events` turns on the SoftDevice USB detected,
   removed, and power-ready events and reads `USBREGSTATUS`, logging
   `"USB power: vbus={} ready={}"`. If enabling the events fails it logs
   `"failed to enable USB power events; assuming VBUS present"` and seeds VBUS
   as present; if only the `USBREGSTATUS` read fails it also assumes VBUS
   present, without a log line.
5. `hid_device::init` builds the composite keyboard, mouse, and consumer device
   on a `SoftwareVbusDetect` seeded with those values. The serial number comes
   from the two `FICR.DEVICEID` words; remote wakeup is advertised.
6. Spawns `softdevice_task` with the VBUS detector (`"SoftDevice started"`),
   then `usb_device_task` and `hid_writer_task` (`"USB HID device started"`),
   then both connection workers and `ble_task` (`"BLE task started"`).
7. Creates TWIM0 on P0.26/P0.27 with internal pull-ups and a 64-byte RAM
   transmit buffer (`ui::display::new_twim`, which the self-test and the
   simulation also use), spawns the display task (`ui::display::task`) and the
   three button tasks, and logs `"UI and isolated OLED tasks started"`.
8. Creates the `UiController` (which holds the `UiState`, the management
   request tracker, and the saved-device addresses), the `PowerManager`, and a
   1-second housekeeping ticker, publishes the first frame, and enters the UI
   loop.

Spawning only queues a task. None of the spawned tasks runs until `main`
reaches its first `.await`, the `select4` at the top of the UI loop, so the
order of `spawn` calls is not an ordering guarantee between tasks.

`main` does not touch flash. `ble_task` in
[multi_conn.rs](../src/ble/multi_conn.rs) loads the pairing store when it first
runs, then plans boot reconnects:

1. Takes the SoftDevice flash handle, locks `DEVICE_STORE`, and calls
   `load_from_flash`, which logs one of `"Loaded {} devices from flash"`,
   `"No paired devices in flash"`, `"Invalid or unsupported device store;
   writes disabled"`, or `"Flash read error: {:?}"`.
2. Loads every stored bond into the shared `Bonder` security handler in
   [bonder.rs](../src/ble/bonder.rs)
   (`"Loaded {} BLE bonds into security handler"`).
3. If the store is not writable, sends `BleEvent::Error(StorageFailed)`; the UI
   shows "Storage failed". The store stays read-only
   ([ADR 0006](adr/0006-fail-closed-pairing-store.md)).
4. Takes up to `MAX_CONNECTIONS` (2) records with a bond from
   `iter_recent()`, which reverses insertion order and so yields the most
   recently added record first. Updating a record does not move it, so
   "recent" means most recently added, not most recently connected (see the
   [in-memory cache](data-model.md#in-memory-cache)). A record without a bond
   is skipped: a background reconnect never pairs, so it could never secure
   that device's link.
5. Reserves slot *i* for record *i* under a new attempt number and sends it
   `SlotCommand::Reconnect` at once, without scanning first. Each worker then finds its device's current
   address as described in [background reconnect](#background-reconnect):
   whichever slot holds the radio looks for both saved devices at the fast duty
   cycle, so a keyboard can connect as soon as it advertises, whether or not
   the mouse is awake.
6. Enters its loop, which serves UI commands and slot events one at a time.

```mermaid
sequenceDiagram
    participant M as main
    participant SD as SoftDevice
    participant T as Spawned tasks
    participant C as ble_task
    participant F as Pairing flash
    participant W as Slot workers
    M->>M: embassy_nrf init, interrupt priorities P2
    M->>SD: enable with sd_setup config
    M->>SD: enable USB power events, read USBREGSTATUS
    M->>M: hid_device init builds the composite device
    M->>T: spawn SoftDevice, USB, HID writer, slots, coordinator, display, buttons
    M->>M: publish first frame, await select4
    C->>F: take flash, load_from_flash
    F-->>C: records, empty store, or fail-closed
    C->>C: load bonds into Bonder
    opt at least one stored peer with a bond
        C->>W: Reconnect per bonded peer, no scan first
        W->>SD: shared reconnect scan, then connect, under GAP lock
    end
    C->>C: serve commands and slot events
```

Boot publishes no scan events: the UI stays on Home until a saved device
connects, and power-up never shows a "No devices found" error. Before
2026-10-09 the coordinator ran a full 8-second scan before it sent any
`Reconnect`, so no keyboard could type for at least 8 seconds after power-up,
which can be too late for a firmware setup key.

### User Scan, Connect, Pairing And HID Discovery

1. SELECT on Home, on an error screen, or on Connected (to add a second device)
   moves the UI to Scanning and yields `UiCommand::StartScan`, which
   `UiController::button` returns as `BleCommand::StartScan`. `main` uses
   `try_send` for every BLE command; a full command channel shows
   "Busy; try again" (`UiController::command_not_sent`) instead of blocking
   the UI loop.
2. The coordinator runs `plan_start_scan`, which disconnects both slots only
   when both are occupied, including slots that are reconnecting in the
   background. It then calls `scanner::scan`, which sends `ScanStarted`
   *before* taking the GAP lock, then runs an active scan
   (`"BLE scan starting ({} s window)"`). Each advertisement goes through
   `merge_advertisement` ([scan_list.rs](../src/ble/scan_list.rs)): a new
   entry needs the HID service UUID `0x1812`; a
   later name-only scan response updates a known entry, and every response
   refreshes the entry's RSSI. The list holds at most `BLE_MAX_DISCOVERED` (8)
   devices. When it is full, a new HID advertiser replaces the entry with the
   weakest latest RSSI if it is received more strongly, so the scan ends with
   the eight strongest HID advertisers rather than the first eight heard.
3. The scan stops at the 8-second deadline, checked when an advertisement
   arrives, or at a 10-second wall-clock backstop
   (`"BLE scan hit hard timeout backstop"`). The GAP lock is released before
   `DeviceFound` events and `ScanComplete` are sent. The coordinator keeps the
   result as its connect snapshot.
4. The UI collects names while on Scanning and moves to the device list, or to
   "No devices found" when the list is empty.
5. SELECT on a device yields `Connect(index)`. `plan_connect` checks the index
   against the coordinator's snapshot; a stale index reports `ConnectFailed`.
   It then looks for a slot that already holds the device: by the same
   address, or, through `Bonder::same_peer`, by one bonded peer's identity key
   that resolves both the held and the selected address. A connected one
   re-emits `Connected`; a slot retrying it in the background (reserved at
   power-up or after a link loss) is reserved again for this connection and
   sent `SlotCommand::Connect`, which takes the retry over
   ([attempt numbers and retry takeover](#attempt-numbers-and-retry-takeover)),
   because a background reconnect reports nothing until it succeeds, which it
   never does for a peer that rejects its keys; a connection the user already
   asked for produces no action. Otherwise the first empty slot is reserved
   and sent `SlotCommand::Connect`, or, with no free slot, `ConnectFailed` is
   reported. Each reservation gets a new attempt number, which the command
   carries.
6. The worker in [slot_worker.rs](../src/ble/slot_worker.rs), through
   `connect_and_run_secure` in [slot_link.rs](../src/ble/slot_link.rs), logs
   `"slot {} connecting to {}"`, takes the GAP lock, and calls
   `central::connect_with_security` with a whitelist of that one address, a
   6-second scan timeout (`BLE_CONNECT_TIMEOUT_SECS`) at the fast duty cycle
   of a 50 ms window every 100 ms (`BLE_FAST_SCAN_INTERVAL`,
   `BLE_FAST_SCAN_WINDOW`), because the device was just seen advertising, a
   7.5–15 ms interval,
   zero peripheral latency, and a 4-second supervision timeout. The call also
   performs the MTU exchange; the link uses the smaller of 64 and the
   peripheral's offer, or the default 23 when the peripheral refuses the
   exchange. The GAP lock is released when it returns.
7. Security: `encrypt()` uses stored keys. If the `Bonder` holds no keys for
   the peer (`PeerKeysNotFound`) and this is a user connection, the worker calls
   `request_pairing()`. The bond handler reports no input/output capability and
   requests no MITM protection, so pairing is unauthenticated LE legacy Just
   Works ([ADR 0011](adr/0011-interim-just-works-pairing.md)). The worker
   polls the security mode every 200 ms, up to 25 times, and accepts only an
   encrypted mode (`JustWorks`, `Mitm`, or `LescMitm`); otherwise it logs
   `"slot {} failed to secure BLE link"` and fails with `ConnectFailed`. New
   keys land in the in-RAM `Bonder` through `on_bonded`, replacing that peer's
   previous keys or, for a new peer, held as unsaved beside every saved
   device's keys until step 10 saves them
   ([bond_table.rs](../src/ble/bond_table.rs)).
8. HID discovery in `hid_client::discover_and_subscribe`
   (`"Discovering HID service..."`): it collects every Report characteristic
   (up to 8) and the Report Map and Protocol Mode handles, writes Report
   Protocol mode when that characteristic exists, reads the Report Map by
   offset into a 512-byte bounded buffer and parses it, then reads each
   report's Report Reference. A report without a CCCD is not subscribed; a
   keyboard output report among them becomes the LED sink. An explicit Output
   or Feature direction is never subscribed even with a CCCD, and an unknown
   report ID is skipped when the map uses report IDs. Each remaining input gets
   its CCCD set to notifications
   (`"Subscribed to {} of {} HID report characteristics"`).
9. Steps 7 and 8 race the slot's command channel. A new command closes the
   link, waiting until the SoftDevice reports the connection gone, and is
   processed next. A `Connect` for the same device is the exception: it takes
   over a background attempt, which carries on under the new attempt number.
   When an attempt ends with no link up, such as before step 10, its
   `SlotEvent::Error` or `SlotEvent::Disconnected` makes the coordinator
   return `Action::DiscardUnsavedBond` for the device first, which drops the
   keys of an unsaved pairing it made (`Bonder::discard_unsaved`); a saved
   device's keys stay, including when a retry after a lost link ends.
10. On success the worker sends `SlotEvent::Connected` with the attempt
    number. `on_slot_connected` ignores the event when the number is not the
    slot's current one; otherwise it
    marks the slot connected and returns two actions in order: persist the
    device with its bond (`store.add`, then `save_to_flash`), then emit the
    connection summary. Persisting marks the bond saved in `Bonder`, and when
    the store evicts its oldest record to make room, drops that record's keys
    too (`Bonder::forget_identity`), so the keys in RAM follow the store. The UI shows Connected after the flash write finishes;
    a failed write shows "Storage failed" while the link stays up. A pairing
    whose identity is not a public or random static address is refused in
    `Bonder::on_bonded`: no keys are kept, nothing is stored for the device,
    and the UI shows "Pairing not saved" ([write rules](data-model.md#write-rules)).
11. The worker enters the notification loop (`"HID notification loop started"`),
    described in [one input report](#one-input-report-from-ble-to-usb). Its
    third future runs the link's GATT client procedures one at a time,
    because the vendored client allows one per link: `device_info::log_pnp_id`
    discovers the Device Information Service and reads and logs its PnP ID
    (`"slot {} PnP ID: {}"`, or a line saying there is none), then the LED
    writer below starts. Input already flows meanwhile, since notifications
    use their own portal.

```mermaid
sequenceDiagram
    actor U as User
    participant UI as UI loop
    participant C as ble_task
    participant W as Slot worker
    participant SD as SoftDevice
    participant P as Peripheral
    U->>UI: SELECT on Home
    UI->>C: StartScan with try_send
    C-->>UI: ScanStarted
    C->>SD: active scan under GAP lock, 8 s
    C-->>UI: DeviceFound list, then ScanComplete
    U->>UI: SELECT on a device
    UI->>C: Connect index
    C->>C: plan_connect reserves a free slot
    C->>W: SlotCommand Connect
    W->>SD: connect_with_security under GAP lock, up to 6 s
    SD->>P: connect and MTU exchange
    W->>P: encrypt, or request pairing when no keys exist
    W->>P: discover HID, read Report Map, enable notifications
    W-->>C: SlotEvent Connected
    C->>C: persist device and bond to flash
    C-->>UI: Connected summary
```

DOWN on Connected sends `Disconnect`. `plan_disconnect` sends
`SlotCommand::Disconnect` to every occupied slot, including reconnecting ones.
A worker in the run phase closes its link and releases its held input; a
worker waiting to retry drops its target. Stored records and bonds are kept.

### Background Reconnect

A slot reconnects silently, without pairing, in four cases: `ble_task` sends
it `Reconnect` at power-up; an established link closes (`"slot {} link lost;
reconnecting"`), including one that the user connected, unless `Bonder` holds
no keys for the device; a silent attempt fails with `ConnectFailed`; or a user
connection that took over a retry fails while `Bonder` still holds the
device's keys ([takeover](#attempt-numbers-and-retry-takeover)). A
silent attempt never pairs, so the retry runs only while `Bonder` holds the
device's keys: power-up skips a saved device without a bond, and every
attempt checks the keys first.

```mermaid
flowchart TD
    P[Reconnect command at power-up] --> K
    T[Retry target kept] --> B{During the 500 ms backoff}
    B -->|Disconnect| X[Drop target, report Disconnected]
    B -->|Connect for the same device| N[Takeover: connect allowing pairing]
    B -->|Other command| O[Drop target, handle the command]
    B -->|Pause ends, or the other slot saw the device| K{Bonder holds the device's keys}
    K -->|No: report Disconnected| F[Slot released, retries stop]
    K -->|Yes| S{Fresh sighting for this slot}
    S -->|Yes| C[Whitelist connect to the seen address, up to 6 s]
    S -->|No| R[Shared reconnect scan under GAP lock, up to 6 s]
    R -->|This slot's device seen| C
    R -->|Other slot's device seen: record it, wake that slot| T
    R -->|Timeout or scan error| T
    R -->|Connect for the same device: report Disconnected under the old number| N
    R -->|Other command: report Disconnected| O
    C -->|Secured and HID ready| L[Drop target, Connected event, notification loop]
    C -->|ConnectFailed: hide device from other slot's scans 6.5 s| T
    C -->|HID or Report Map error| E[Drop target, error event, slot released, retries stop]
    L -->|Peer closes link| D[HidEvent Disconnected]
    D -->|Bonder holds keys: LinkLost| T
    D -->|No keys: Disconnected| F
```

- Before each silent attempt the worker asks `Bonder::bond_for_address` for
  the device's keys. Without them (saving a newer device evicted its record and
  keys) it clears the target, sends `SlotEvent::Disconnected`, and logs
  `"slot {} has no keys to reconnect"`; the attempt could never secure the
  link and would hold the slot for good. Power-up reserves slots only for the
  two most recently added saved devices that have a bond, so a record without
  one (a device that paired without bonding, or one loaded from the legacy
  format) connects only when selected from a scan.
- Each silent attempt then registers the slot's target in `RECONNECTS`, the
  `reconnect::ReconnectTable` both slots share in
  [scanner.rs](../src/ble/scanner.rs): the stored or last live address and
  the bonded peer's identity key from the `Bonder`. The target is dropped when
  the slot connects (just before `SlotEvent::Connected`), when a command other
  than `Reconnect` reaches the slot, when an attempt ends in an error that
  stops retries, and when the keys are gone, so a connected or released slot
  never claims an advertisement.
- `scanner::find_saved_peer` first takes a sighting that the other slot's scan
  recorded for this slot in the last 2 seconds (`BLE_RECONNECT_SIGHTING_TTL_MS`), and
  connects to it without scanning. Otherwise it runs one passive scan, bounded
  by `BLE_CONNECT_TIMEOUT_SECS`, that ignores non-connectable advertisements
  and matches every connectable one against every registered target: through the identity key, which follows a rotated
  private address on every attempt, or by the stored address. For each
  advertisement it copies the targets out of the table and resolves the
  address outside the critical section, because `IdentityKey::is_match` calls
  the SoftDevice's AES block. The scan accepts devices whose advertisement
  omits the HID UUID and does not change the UI's scan results.
- The scan stops at the first advertisement from any slot's device. Its own
  device is connected at once. Another slot's device is recorded for that slot
  (`"slot {} scan found slot {}'s device"`), which is woken through its
  `RECONNECT_WAKE` signal, and the scanning slot goes back to its backoff. A
  saved device that is asleep therefore cannot hold the radio while the other
  slot's device is already advertising. When both slots would match one
  advertisement, the lower slot gets it.
- After a silent attempt fails with `ConnectFailed`, the other slot's scans
  ignore that slot's device for `BLE_FAILED_RECONNECT_HOLDOFF_MS` (6.5 s: the
  500 ms pause plus one 6-second scan), through
  `ReconnectTable::attempt_failed` and the per-slot view
  `targets(scanning_slot, now)`; the slot's own scans still look for it. A
  device that advertises but will not connect, for example one paired again
  with another computer that has a filter accept list, or one that deleted its
  bond, would otherwise end every scan of the other slot at its first
  advertisement and wake its own slot for another failing attempt, so the other
  slot's device would be heard only when it happened to advertise first. With
  the holdoff the two slots alternate as they did with per-slot scans: one
  failing attempt, then one full scan for the other device.
- Each slot's `RECONNECT_WAKE` signal mirrors `ReconnectTable::wake_pending`:
  set while another slot's scan has handed the slot a sighting it has not
  taken, reset once it takes the sighting at the start of its next reconnect
  scan, when an attempt fails, which drops the sighting, when its target is
  cleared, and when it registers a different device. Every change to the table
  goes through one helper in `scanner.rs` that applies the change and then sets
  or resets the signal, so a wake left from a sighting the slot already used or
  lost cannot skip a later backoff. A sighting for a slot that stopped
  reconnecting after the scan copied the targets records nothing, wakes nobody,
  and lets the scan go on.
- The scan uses the fast duty cycle, a 50 ms window every 100 ms
  (`BLE_FAST_SCAN_INTERVAL`, `BLE_FAST_SCAN_WINDOW`), while any target was
  registered less than `BLE_FAST_RECONNECT_SECS` (30 s) ago, and otherwise
  the vendored default of a 312.5 ms window every 1.7 s
  (`central::ScanConfig::default()`). A retry re-registers the same device
  without restarting that window: a bonded device is the same device under any
  private address because its identity key identifies it. The window starts
  again after the device connects and its link is lost again, or at power-up.
- Between attempts the worker waits `BLE_RECONNECT_BACKOFF_MS` (500 ms) while
  listening for commands and for the other slot's wake, which leaves the radio
  free for a user scan.
- Background attempts never start pairing. A peer that has lost its keys fails
  encryption with `ConnectFailed`, which a silent attempt retries
  indefinitely; only the RTT log shows `"slot {} failed to secure BLE link"`.
  Selecting the device from a scan ends the wait on Connecting with
  `Connect failed` (see the takeover below); forgetting it and pairing again
  recovers it.
- A link that drops while its HID service is discovered or subscribed, for
  example a keyboard going back to sleep right after the key press that woke
  it, fails as `ConnectFailed` (`"slot {} link dropped during HID
  discovery"`), so a silent attempt retries it like a peer that never
  connected. The vendored crate marks the link disconnected before it fails
  the pending GATT procedure, so `connect_and_run_secure` reads the link's
  handle after the failure (`ErrorTag::for_failed_setup`). A peer that fails
  discovery on a link that stays up and then disconnects at once can be
  taken for a dropped link and retried.
- Any other error during a silent attempt (`HidNotFound`, `NotifyFailed`, or a
  Report Map error on a link that is still up) is reported to the coordinator,
  which releases the slot and shows the error; that slot stops retrying.
- On link loss the worker first sends `HidEvent::Disconnected` so the
  aggregator releases that source's held input, then `SlotEvent::LinkLost`.
  `on_slot_link_lost` keeps the slot reserved in the connecting state, under
  the same attempt number, so a connect request for another device cannot
  take it (selecting the same device turns the retry into a user connection,
  [below](#attempt-numbers-and-retry-takeover)), and the UI shows only links
  that are up. When `Bonder::bond_for_address` finds no keys for the device
  (its pairing was refused, the peripheral paired without bonding, or saving a
  newer device evicted its record and keys), the worker sends
  `SlotEvent::Disconnected` instead and logs
  `"slot {} link lost; no keys to reconnect"`: a silent attempt never pairs,
  so it could never secure the link, and would hold the slot until the user
  disconnected.
- A background reconnect that reaches Connected moves the UI from Home or
  Connecting to Connected. A scan the user started keeps its picker on screen;
  an error or notice also stays until acknowledged.

### Attempt Numbers And Retry Takeover

`ble_task` waits on UI commands and slot events with one `select` that polls
commands first, so the coordinator can act on a selection while a worker's
report on the attempt that selection replaces is still queued. Attempt
numbers keep those late reports from undoing the newer decision.

- `ConnManager` gives every reservation (`reserve_slot` for a user
  connection, `reserve_retry` at power-up) the next number of a wrapping
  counter that skips 0. Connecting and losing the link keep the number; a free
  slot holds 0, which no attempt is given. `Action::ConnectSlot`,
  `SlotCommand::Connect`, and `SlotCommand::Reconnect` carry the number.
- The worker keeps the number of the command it serves and puts it on every
  `SlotEvent` except `Quiesced`. `on_slot_connected`, `on_slot_disconnected`,
  `on_slot_link_lost`, and `on_slot_error` return no actions for an event
  whose number is not the occupied slot's current one: it reports on an
  attempt the coordinator has since replaced or ended.
- The coordinator sends `Connect` to an occupied slot only when it is
  retrying the selected peer in the background (step 5 of
  [the user connect](#user-scan-connect-pairing-and-hid-discovery)). The
  worker treats a `Connect` for the same peer, by address or by one bonded
  peer's identity key (`Bonder::same_peer`), as a takeover:

| Where the worker is | What the takeover does |
| --- | --- |
| Pausing between attempts, or in the reconnect scan | Remembers the retry target and connects to the selected address with pairing allowed; the scan reports `Disconnected` under the old number, which the coordinator ignores |
| In `connect_with_security` | The command waits in the channel. A failed connect returns to the pause, where the takeover proceeds as above; a link that comes up goes on to security as below |
| Securing the link or discovering HID | The attempt carries on under the new number. Success reports `Connected` with it; failure closes the link and connects again with pairing allowed, without reporting the silent failure |
| Running, with `Connected` sent under the old number but not yet handled | Keeps the link and reports `Connected` again under the new number |

- When the takeover's connection fails and `Bonder` still holds the replaced
  target's keys, the worker sends `SlotEvent::Error` with `retrying` set and
  resumes the retry under the same number. `on_slot_error` then keeps the
  slot reserved and retrying, emits the error (`Connect failed` for a peer
  that rejects its keys), and a later selection can take the retry over
  again. Without the keys the error frees the slot.
- Any other command that arrives during an attempt supersedes it: the worker
  closes the link, reports `Disconnected` under the old number, and handles
  the command next (a `Disconnect` needs nothing more).
- To carry an attempt on through a takeover, the worker pins the security and
  discovery future, and then the notification loop, in a block that returns
  the result or the superseding command; the future is dropped when that
  block ends, before the worker closes the link. The vendored GATT client
  keeps one portal per connection handle
  (`vendor/nrf-softdevice/src/util/portal.rs`), and a wait that outlived its
  link could erase a wait the other slot had registered on the reused handle,
  leaving that slot's connect waiting forever with the GAP procedure lock
  held. Since 2026-10-11 a `bt2usb patch:` makes an ended wait clear the
  portal only while it holds that wait's own closure, which also covers a
  link the peer drops, where the failed wait's task runs again only after the
  SoftDevice may have reused the handle; the drop order above stays as
  defence in depth ([ADR 0007](adr/0007-vendored-softdevice-patch.md)).

### Peripheral Connection Parameter Requests

The bridge opens each link with a 7.5 to 15 ms interval, no peripheral
latency, and a 4-second supervision timeout. A peripheral can ask for other
values at any time afterwards, with an L2CAP Connection Parameter Update
Request or the Link Layer Connection Parameters Request procedure; the
SoftDevice reports both as `BLE_GAP_EVT_CONN_PARAM_UPDATE_REQUEST` and waits
for the central to answer.

1. The vendored crate's GAP event handler passes the request to the link's
   security handler through `SecurityHandler::conn_param_update_request`, a
   bt2usb patch ([ADR 0007](adr/0007-vendored-softdevice-patch.md)). Upstream
   granted every request unchanged, so a peripheral could move to a long
   interval, which delays every report, or to a supervision timeout of up to
   32 seconds, during which a key held when the link silently fails stays held
   on the host.
2. `Bonder` in [bonder.rs](../src/ble/bonder.rs) answers with
   `conn_params::bound_request` and `PEER_CONN_PARAM_LIMITS`. The interval
   range becomes its overlap with 7.5 to 15 ms (`BLE_CONN_INTERVAL_MIN`,
   `BLE_CONN_INTERVAL_MAX`). A peripheral that asks only for slower intervals
   gets its own fastest one, up to 30 ms (`BLE_PEER_MAX_CONN_INTERVAL`), as a
   single value: some peripherals, such as ones built on Nordic's nRF5 SDK
   `ble_conn_params` module with `disconnect_on_fail`, disconnect after a few
   attempts when the interval they get lies outside the range they asked for,
   and the bridge would then lose and reconnect the link every minute or two.
   Latency is capped at 20 connection events
   (`BLE_MAX_PERIPHERAL_LATENCY`). The supervision timeout is kept between
   1 second (`BLE_MIN_SUP_TIMEOUT`) and 4 seconds (`BLE_SUP_TIMEOUT`) and
   raised when needed to meet the Bluetooth Core rule that it exceeds
   `(1 + latency) × interval × 2`; latency is lowered first if even 4 seconds
   could not meet it.
3. It logs `"peer connection parameters granted: {}"` for a request inside the
   limits, `"peer asked for connection parameters {}; granting {}"` for one it
   changed, and, at warning level,
   `"peer asked for connection parameters {}; granting {}, outside its interval range"`
   when the interval is outside the requested range (only for a peripheral
   whose fastest requested interval is slower than 30 ms, or whose whole range
   is below 7.5 ms). The event handler then answers with
   `sd_ble_gap_conn_param_update`.

The bounds hold for the life of every link. A held key is released at most
4 seconds after a link silently fails. Input still leaves the peripheral at
the next connection event, at most 15 ms away, or 30 ms for a peripheral that
refuses anything faster, because latency only lets a peripheral skip events
when it has nothing to send; what latency delays is traffic to the
peripheral, so an LED write can wait up to 21 events, about 315 ms at 15 ms.
The policy is host-tested in [conn_params.rs](../src/ble/conn_params.rs),
including a sweep over every boundary of the policy and over values outside
the Core's legal ranges; the parameters real peripherals end up with are not
recorded yet ([TODO.md](../TODO.md#ble-central-and-pairing)).

### One Input Report From BLE To USB

```mermaid
sequenceDiagram
    participant P as BLE peripheral
    participant SD as softdevice_task
    participant W as Slot worker drain
    participant D as HID dispatcher
    participant E as Endpoint worker
    participant H as USB host
    P->>SD: GATT notification
    SD->>SD: on_hvx, then classify_gatt_notification
    SD->>W: ReportCoalescer push and wake signal
    W->>D: HID_REPORT_CHANNEL send, waits when full
    D->>D: note_hid_activity, InputAggregator apply
    D->>E: EndpointMailbox publish
    E->>H: HidWriter write with 100 ms deadline
    alt write fails or times out
        E->>E: replay current state, back off 20 to 1000 ms
    end
```

1. **Notification callback.** The SoftDevice signals pending events through the
   SWI2/EGU2 interrupt, whose handler only wakes `softdevice_task`. That task
   pulls each BLE event and hands GATT notifications to the waiting
   `gatt_client::run` closure synchronously, inside its own poll. The callback
   therefore runs in `softdevice_task` and must not await. `on_hvx` drops
   indications, notifications on unsubscribed handles, and payloads longer
   than 32 bytes; it rejects rather than truncates.
2. **Classification.** `hid::classify_gatt_notification` uses the kind resolved
   from the Report Reference when there is one, and accepts only exact
   layouts: 8 bytes for a keyboard, 3–5 bytes for a mouse, 2 bytes for
   consumer control. Without a resolved kind, a numbered Report Map rejects the
   report, an unnumbered map limits the fallback to the kinds it advertises, and
   an absent map uses the legacy length rules. Anything else is dropped.
3. **Coalescing.** The callback pushes the report into the connection's
   `ReportCoalescer`, which holds at most one pending report per endpoint:
   keyboard and consumer state is latest-wins, mouse motion accumulates with
   saturation and the latest buttons. It then signals the drain future.
4. **Drain.** A future in the slot worker pops round-robin and awaits
   `HID_REPORT_CHANNEL.send(HidEvent::Report { source, report })`. Backpressure
   stops here; nothing is dropped at the channel, and the coalescer keeps the
   memory bound while it waits.
5. **Aggregation.** `dispatch_reports` in `hid_writer_task` receives the event,
   records HID activity for the power manager, and calls
   `InputAggregator::apply`. Keyboard keys and modifiers from both slots are
   unioned, with the `ErrorRollOver` array beyond six keys; mouse buttons are
   masked to five and unioned while motion comes only from this report; a
   consumer usage above `0x0FFF` is ignored and the lowest active slot wins.
   `wake::new_press` decides whether this report is a new press; if USB is
   suspended, the dispatcher signals a remote wakeup.
6. **Mailbox.** Each resulting report goes to its endpoint's
   `EndpointDelivery::publish`. The retained state drops mouse motion and keeps
   buttons. While USB is unavailable or the endpoint is recovering, the queue
   collapses to that retained state; otherwise reports join a 16-entry FIFO,
   which is cleared before the push when full.
7. **Endpoint worker.** `run_endpoint` waits until USB is configured and not
   suspended and a report is pending, discards transfers from an old epoch, and
   races the USB lifecycle signal against the write and a 100 ms timer. Success
   ends recovery. Failure or timeout replays the current state rather than the
   failed packet, logs `"USB HID endpoint unavailable; retaining current input
   state"` on the first failure in a row, and backs off 20, 40, … up to
   1000 ms. A lifecycle event (reset, configuration, suspend or resume, or a
   protocol change) that interrupts a write or ends a backoff wait early resets
   the backoff to 20 ms, as does a spell with USB unavailable, so the first
   failure after it logs, and counts as a `USB write failures` event, again.
8. **Write.** `UsbReportSink::write` serializes into an 8-byte buffer, using
   the 3-byte boot layout for the mouse when the host selected boot protocol,
   and calls `HidWriter::write`. Success means the USB hardware accepted the
   packet, not that a host application consumed it.

When a link ends, reports still in its coalescer are discarded and the worker
sends `HidEvent::Disconnected { source }` on the same channel, after its last
report. The aggregator clears that source and publishes the new union on all
three endpoints, which releases anything the source was holding without
touching the other source. The policy is host-tested in
[aggregate.rs](../src/hid/aggregate.rs), [coalesce.rs](../src/hid/coalesce.rs),
[delivery.rs](../src/hid/delivery.rs), and
[delivery_tests.rs](../src/hid/delivery_tests.rs), which polls the real
`run_endpoint` against fake endpoints. Real-host timing is not verified.

### Host Keyboard LEDs To A BLE Keyboard

1. The host sends a one-byte output report to the keyboard interface.
   `BootRequestHandler::set_report` in
   [host_requests.rs](../src/usb/host_requests.rs) rejects anything else, logs
   `"Host LEDs: num={} caps={} scroll={}"`, and sends the value to the
   `KEYBOARD_LEDS` watch.
2. Each connection worker holds one of the watch's two receivers for its
   whole life, across links. When a link's notification loop has read the
   peer's PnP ID (step 11 of the [connect sequence](#user-scan-connect-pairing-and-hid-discovery)),
   `host_leds::forward_host_leds` writes the host's latest LED state, if the
   host has sent one since enumeration, to the peer's keyboard LED output
   report, then writes each later change. Taking the latest state marks it
   seen, so it is not written twice. A keyboard that wakes and reconnects, or
   connects to a slot that already passed the latest change to an earlier
   link, therefore shows the host's state at once, as a wired keyboard does
   when it is plugged in. Writes happen only when discovery found a keyboard
   LED output report; a failed write logs
   `"Failed to write LED state to BLE keyboard"` and the next change is still
   written.
3. A USB reset publishes the default (all off) LED state the same way.

### USB Suspend, Remote Wakeup And Resume

```mermaid
sequenceDiagram
    participant H as USB host
    participant U as usb_device_task
    participant D as HID dispatcher
    participant E as Endpoint workers
    participant M as UI loop
    participant O as display task
    H->>U: bus suspend
    U->>U: suspended true, clear REMOTE_WAKE, set USB_SUSPENDED
    U->>E: replay, endpoints unavailable
    U->>M: USB_SUSPEND_SIGNAL true
    M->>O: frame with panel off
    Note over D,E: BLE input keeps updating held state
    D->>U: REMOTE_WAKE on a new press
    U->>H: remote_wakeup
    H->>U: resume
    U->>U: suspended false, clear USB_SUSPENDED
    U->>E: replay current held state
    U->>M: USB_SUSPEND_SIGNAL false
    M->>O: frame with panel on
```

1. When embassy-usb reports a suspend, `UsbPowerHandler::suspended(true)`
   clears any stale wake request, sets `USB_SUSPENDED`, signals the UI loop, and
   replays all three endpoints. `run_usb_device` returns from
   `run_until_suspend` and waits for either a resume or a wake request.
2. The UI loop calls `PowerManager::set_usb_suspended(true)` (`"Power:
   usb_suspended={}"`). The power state becomes low power, so the next frame
   turns the panel off. A button press while suspended counts as activity but
   is consumed and does not turn the panel on.
3. BLE links stay connected ([ADR 0012](adr/0012-bus-powered-no-system-off.md)).
   Input keeps flowing into the aggregator; each endpoint keeps only its latest
   held state.
4. Only a newly pressed key or modifier, a new non-zero consumer usage, or a new
   mouse button raises `REMOTE_WAKE`. Motion, scroll, releases, repeated held
   state, disconnect cleanup, and replays never do.
5. `usb_device_task` calls `remote_wakeup()` and logs `"USB remote wakeup
   sent"`, or `"USB remote wakeup not possible: {}"` when the host has not
   enabled remote wakeup for the device, in which case the device stays
   suspended until the host resumes the bus.
6. `UsbPowerHandler` has no separate resume callback; it relies on embassy-usb
   calling `suspended(false)` when the bus resumes (upstream behavior, not
   checked against the embassy-usb source or on hardware here). That call
   clears `USB_SUSPENDED`, signals the UI loop, and replays the endpoints. Each
   worker writes the current held state, with zero mouse motion. A key that was
   pressed and released before the resume wakes the host but is not typed; only
   input still held at resume reaches it.
7. The UI loop records the resume as activity, so the panel turns back on.

Related bus events follow the same replay rule. `reset()` (also run when the
device is disabled) clears the configured, suspended, and boot-protocol flags
and the wake request, publishes default LEDs, signals "not suspended", and
replays. `configured()` updates the configured flag, replays, and logs
`"USB configured by host: {}"`. `SET_PROTOCOL` replays the affected endpoint.
VBUS changes reach the driver as SoftDevice SoC events forwarded by
`softdevice_task` to the software VBUS detector. The wake rule is host-tested
in [wake.rs](../src/hid/wake.rs); suspend and wake on real hosts are hardware
acceptance items.

### Forget And Factory Reset

```mermaid
sequenceDiagram
    participant UI as UI loop
    participant C as ble_task
    participant W as Targeted slot worker
    participant A as Aggregator
    participant F as Pairing store
    UI->>C: ListPaired with request id
    C-->>UI: PairedDevices with the same id
    UI->>C: Forget id and address, or FactoryReset id
    C->>W: Quiesce with a new token
    W->>W: close link and wait for GAP disconnect, drop retry target
    W->>A: HidEvent Disconnected when the link was running
    W-->>C: Disconnected, then Quiesced with the token
    Note over C,W: events from targeted slots are dropped until every ack
    C->>F: forget or factory_reset, commit then publish
    F-->>C: Ok or StoreError
    C->>C: on success remove the bond or clear all bonds
    C-->>UI: current link status
    C-->>UI: ManagementResult with the request id
```

1. UP on Home, Connected, an error, or a notice shows "Please wait..." and sends
   `ListPaired`. In `UiController::button`, `ManagementRequests::begin`
   assigns the next request ID and a deadline `UI_MANAGEMENT_TIMEOUT_SECS`
   (30 s) away, and allows one outstanding request. While it is pending, the
   controller ignores button actions. The 1 s housekeeping tick calls
   `UiController::tick`, which calls `ManagementRequests::expire`; past the
   deadline it drops the request and its saved-device snapshot, and
   `UiState::management_timed_out` shows **No reply** with a message that names
   no outcome (or keeps an error already showing). A later reply carries the
   dropped ID and is ignored.
2. The coordinator replies with `PairedDevices`, most recently added first.
   The controller accepts only the reply whose ID matches, keeps the
   addresses, and shows Saved devices unless an error is visible.
3. Choosing a device, or the final "Factory reset" row, opens a confirmation
   with Cancel selected. Confirming sends `Forget { id, address }`, using the
   stable address from the snapshot rather than the list index, or
   `FactoryReset { id }`. If the snapshot no longer has the row, the UI shows
   "Device changed; retry".
4. The coordinator increments its management token and runs
   `manage_devices`. Forget finds the record by stable identity, matching the
   stored address or resolving through its IRK; a missing record fails with
   `ManagementFailed`. Forget targets the slots whose reserved address matches
   that peer (`management::forget_targets`); Factory reset targets both slots.
   Each target receives `Quiesce(token)`.
5. A worker acknowledges once it holds nothing. An idle worker acknowledges at
   once. A worker in backoff or a reconnect scan drops its target, and its entry in the shared reconnect table, first. A
   worker in security, discovery, or the run phase closes the link, waits until
   the SoftDevice reports the handle gone, and in the run phase also sends
   `HidEvent::Disconnected`; it then reports `Disconnected` and, on its next
   loop iteration, `Quiesced`. A worker inside the GAP connect call sees the
   command only when that call returns, within about 6 seconds.
6. The `Quiescence` barrier accepts only acknowledgements carrying the current
   token, clears each acknowledged slot in the `ConnManager`, and drops every
   other event from targeted slots, so a late `Connected` cannot re-save a
   forgotten bond. Events from untargeted slots are handled normally.
7. `DeviceStore::forget` or `factory_reset` builds a candidate store and passes
   it to `management::commit`, which publishes it only after `save_to_flash`
   succeeds. Factory reset of an unreadable store first erases the four storage
   pages. On success the `Bonder` forgets that peer's bond or clears all bonds;
   on failure the in-memory store and the bonds keep their previous contents.
8. Unless the Forget lookup failed, the coordinator sends the current link
   status. It then sends `ManagementResult` with the request ID: `Ok`, or
   `StorageFailed` or `ManagementFailed`. Factory reset also clears the scan
   snapshot.
9. The UI accepts only the matching result and shows "Device forgotten" or
   "Pairings reset", or the error message. A success notice does not replace
   an error already on screen.

The targets, barrier, and commit are host-tested in
[management.rs](../src/ble/management.rs), the confirmation rules in
[ui_logic.rs](../src/ui/ui_logic.rs), and the request-ID, reply, and timeout
rules in [controller.rs](../src/ui/controller.rs). The Renode scenario runs
the same UI controller, targets, barrier, commit, and device list on the
simulated target, with connection workers that acknowledge at once
([testing](testing.md#renode-scenario-map)). The worker shutdown on a board
and power loss during deletion are open
[TODO.md](../TODO.md) items. Storage rules are in the
[data model](data-model.md#write-rules).

### Display Update

```mermaid
flowchart LR
    E[Button, BLE event, suspend change or 1 s tick] --> S[UI loop updates UiState and PowerManager]
    S -->|publish overwrites| F[FRAMES latest frame]
    F --> D[display task]
    D --> Q{New frame and backoff elapsed}
    Q -->|No| W[Wait for next frame or backoff timer]
    Q -->|Yes| R[Init if needed, draw, flush, set panel on or off]
    R -->|Ok| A[Remember applied frame]
    R -->|Error| B[Mark uninitialized, back off 1 s doubling to 30 s]
    R -->|Over 500 ms| P[Request TWIM STOP, keep awaiting the DMA]
    P --> R
    A --> W
    B --> W
    W --> Q
```

1. After every event it handles, including the 1-second tick, `main` calls
   `ui::display::publish(&ui.state, power.display_on())`. That overwrites a
   `Signal` with a copy of the view model; `main` never waits for I2C
   ([ADR 0009](adr/0009-isolated-display-task.md)).
2. The display task (`ui::display::task`) owns TWIM0 and the panel. It renders only when the newest
   frame differs from the last one applied and the retry backoff allows it.
3. `render` initializes the panel on first use and after any failure, draws
   and flushes the 128×64 framebuffer when the panel should be on, and then
   switches the panel on or off. Initialization (`initialize`) first clears
   the panel's RAM while power-up still holds the panel off, then runs the
   `ssd1306` crate's `init`, which ends by turning the panel on, and turns it
   off again until the frame is in, so the panel never shows the random RAM
   content it powers up with. A render that initializes sends two full
   frames, about 0.22 s at 100 kHz, well inside the 500 ms deadline. The
   drawing is the lines
   `ui::layout::lines` gives the screen, in `FONT_6X10`, so the layout is
   host-tested and the Renode panel model reads the same lines back.
4. `finish_or_stop` gives each render 500 ms. On overrun it logs `"OLED I2C
   stalled; requesting STOP, display task degraded until DMA completes"`,
   triggers TWIM RESUME and STOP, and keeps awaiting the same future; cancelling
   a pending DMA future is unsafe with the pinned HAL.
5. `StopSafeI2c` holds an address NACK, data NACK, or overrun error until the
   peripheral reports STOPPED, polling every 100 µs and re-requesting STOP
   every 5,000 polls with the log
   `"OLED I2C error: STOP not complete; requesting again"`.
6. A failure marks the panel uninitialized and schedules a retry after 1, 2, 4,
   8, 16, then 30 seconds (`"OLED operation failed; retry in {} ms (bridge
   remains active)"`). New frames replace the pending frame but do not bypass
   the backoff. The next success logs `"OLED initialized/recovered"` and resets
   the backoff.
7. `PowerManager::display_on` decides the panel state. The power state is
   Active, Idle after 60 seconds without activity, or low power while USB is
   suspended or after more than 120 seconds idle with no BLE link. The panel is
   on in Active and Idle, unless the 120-second auto-off has expired. HID
   traffic counts as activity: the dispatcher sets a flag that the next tick
   folds in.

If the bus never completes a transfer, the display task stays blocked. The UI
loop keeps publishing, and BLE, USB, and buttons keep working.

## HID Path And Limits

Each input travels through GATT notification decoding, descriptor/report-reference
classification, a fixed internal report type, and a USB endpoint; the steps are
traced in [one input report](#one-input-report-from-ble-to-usb). Supported USB
reports are an 8-byte, six-key keyboard report, mouse buttons with signed 8-bit
movement and scroll, and consumer control. Mouse translation includes five
buttons and horizontal scrolling for compatible layouts. The keyboard and mouse
interfaces advertise the USB boot subclass and handle boot/report protocol
selection; boot mouse output uses the three-byte layout. Actual pre-OS behavior
is a hardware acceptance item.

The descriptor parser is a classifier for supported report layouts, not a full
HID bit-field translator. NKRO, 16-bit movement, and arbitrary vendor layouts need
additional translation. Descriptor parsing bounds collection/global-state
nesting and supports Push/Pop, but retains at most one report ID per supported
kind. GATT discovery and notification buffers are bounded; see the current
constants in `hid_client.rs` when adding report families.

Report Maps are read by offset across negotiated MTU boundaries into a bounded
512-byte buffer using a small vendored SoftDevice API patch
([ADR 0007](adr/0007-vendored-softdevice-patch.md)). Present but
unreadable, malformed, or oversized maps fail discovery with a specific error.
Only an absent Report Map characteristic permits legacy fixed-layout/length
classification. Neither that fallback nor report-kind classification proves
an arbitrary same-length layout is supported; field-level translation remains
open work.

The aggregator tracks each BLE slot independently. Keyboard keys/modifiers and
mouse buttons are unioned, so releasing or disconnecting one source preserves
the other's held input. More than six unique keys produces the keyboard
`ErrorRollOver` array. The one-usage consumer interface gives the lowest active
slot priority and falls back to the other slot when that input is released.
Mouse movement belongs only to the current event and is not unioned as held state.

Normal endpoint traffic retains FIFO ordering. While unavailable or recovering,
an endpoint keeps the latest absolute state. Saturated queues collapse to current
state, so intermediate taps or relative motion may be lost under sustained
overload. The policy prioritizes bounded memory and final releases. Host tests
include actual asynchronous workers with fake sinks; real-device timing and
recovery still need hardware acceptance.

## Pairing And Storage

The store records BLE addresses, names, RSSI hints, and optional bonding keys in
four reserved flash pages through `sequential-storage`
([ADR 0006](adr/0006-fail-closed-pairing-store.md)). Its codec and versioned
framing are separate from flash I/O; the [data model](data-model.md#pairing-store)
defines the layout and validation rules. An invalid, unsupported, or unreadable
store disables writes instead of being silently replaced.

Bonded records use stable peer identities. Background reconnects resolve a peer's
current advertising address on each attempt, including when its private address
rotates after boot. Up to two stored devices with a bond are selected at
boot, the most recently added first
([in-memory cache](data-model.md#in-memory-cache)). New pairing is initiated
for explicit user connections; background reconnects use existing keys and run
only while the `Bonder` holds them. HID discovery waits for an encrypted link, and commands can cancel
security/discovery once the connection is owned. Pairing is unauthenticated
Just Works for now ([ADR 0011](adr/0011-interim-just-works-pairing.md)).

Disconnecting closes connections while retaining their records. Separate
confirmation-based Forget and Factory reset operations stop affected workers and
wait for their source/retry cleanup before changing records. The in-memory store
and bonder change only after persistence succeeds. Factory reset can explicitly
erase an unreadable storage region to recover it; ordinary writes cannot.
Logical deletion is not a physical key-erasure guarantee. Power-loss behavior,
authenticated pairing, and physical flash protection remain release work in
[TODO.md](../TODO.md).

The coordinator task is the only writer. It owns the flash handle, and
`DEVICE_STORE` is an async mutex held across flash I/O. A write is attempted up
to three times, 20 ms apart, because SoftDevice flash operations compete with
radio activity (`"Flash write busy (attempt {}), retrying"`). `add` stores the
bond's identity address rather than the advertised private address, and skips
the flash write when only the RSSI hint changed. The full sequences are in
[boot](#boot-and-initialization) and
[Forget and Factory reset](#forget-and-factory-reset).

## Concurrency And Interrupt Model

### Executor And Tasks

Each binary runs one Embassy thread-mode executor (`embassy-executor` with
`platform-cortex-m` and `executor-thread`, started by
`#[embassy_executor::main]`). There is no interrupt executor. Tasks are
cooperative: a task runs until it awaits, and every task shares the one stack,
whose deepest use is painted and measured by [stack.rs](../src/stack.rs) and
logged from the 1-second tick, whenever it grows, as
`"stack high-water: {} of {} bytes"`
([ADR 0010](adr/0010-static-memory-layout.md)). A no-access MPU region at the
bottom of that stack makes an overflow fault before it reaches the statics
([ADR 0026](adr/0026-mpu-stack-guard.md)). The same tick logs the event
counters as `"diagnostics: {}"` when they change, at most once per
`DIAGNOSTICS_REPORT_INTERVAL_SECS` (60 s)
([event counters](operations.md#event-counters)). There is no heap; buffers are
`static`, `StaticCell`, or fixed-capacity `heapless` types.

The tasks in `bt2usb`, that is `main` (`#[embassy_executor::main]`) and the
`#[embassy_executor::task]` functions:

| Task | Role | Mainly waits on |
| --- | --- | --- |
| `main` | Setup, then the UI loop: owns the `UiController` (view model, management request, saved-device addresses) and the `PowerManager` | Suspend signal, 1 s ticker, `BUTTON_CHANNEL`, `BLE_EVENT_CHANNEL` |
| `softdevice_task` | Pulls SoftDevice BLE and SoC events and dispatches them; forwards USB power events to the VBUS detector; scan and GATT callbacks run inside it | SWI2/EGU2 wake-ups |
| `ble_task` | Coordinator: owns the `ConnManager`, flash handle, scan snapshot, and management token; runs scans itself | `BLE_CMD_CHANNEL`, `BLE_SLOT_EVENT_CHANNEL` |
| `ble_slot_task` (one per link, slots 0 and 1) | Connection workers: connect, secure, discover, then the notification loop, coalescer drain, and GATT procedures (the PnP ID read, then the LED writer) | Slot command channel, SoftDevice, `HID_REPORT_CHANNEL` space |
| `usb_device_task` | `run_usb_device`: enumeration, control requests, suspend, resume, remote wakeup | USB bus events, `REMOTE_WAKE` |
| `hid_writer_task` | `join4` of the dispatcher and the keyboard, mouse, and consumer endpoint workers | `HID_REPORT_CHANNEL`, endpoint signals, host polling |
| `ui::display::task` | Sole owner of TWIM0 and the OLED | `FRAMES`, I2C DMA, retry timer |
| `button_up_task`, `button_down_task`, `button_select_task` | Debounce one pin each and send a `ButtonEvent` | GPIO level, `BUTTON_CHANNEL` space |

`bt2usb-selftest` spawns only `softdevice_task` and `usb_device_task` and runs
its stages in `main`. `bt2usb-sim` spawns three instances of one
`button_task` declared with `pool_size = 3` and the same `ui::display::task`
as the bridge, and runs its UI loop and the simulated coordinator
(`sim_ble::SimBle`) in `main`.

### Interrupt Priorities

The SoftDevice reserves interrupt priorities 0, 1, and 4. The comment in
`main` states the rule: "Every application peripheral interrupt must run at 2,
3, 5, 6, or 7 or it will preempt SoftDevice critical sections and fault."

| Interrupt | Priority | Set by |
| --- | --- | --- |
| GPIOTE (buttons) | P2 | `nrf_config.gpiote_interrupt_priority` |
| RTC1 (`embassy-time` driver, `time-driver-rtc1`) | P2 | `nrf_config.time_interrupt_priority` |
| USBD | P2 | `interrupt::USBD.set_priority` |
| TWISPI0 (OLED TWIM) | P2 | `interrupt::TWISPI0.set_priority` |
| SWI2/EGU2 (SoftDevice event notification) | Not set by bt2usb | Unmasked by `nrf-softdevice`; its handler only wakes `softdevice_task` |
| SoftDevice-owned interrupts (POWER_CLOCK, RADIO, RTC0, TIMER0, RNG, ECB, CCM_AAR, TEMP, SWI5/EGU5) | Reserved levels 0, 1, 4 | SoftDevice; the list is `RESERVED_IRQS` in the vendored `critical_section_impl.rs` |

The self-test sets the same four priorities. The simulation has no SoftDevice
and uses the `embassy-nrf` defaults. A new peripheral interrupt must be given an
allowed priority before it is enabled.

### Critical Sections And Locks

- **Critical-section implementation.** Firmware builds use `nrf-softdevice`'s
  `critical-section-impl`, which masks application interrupts in the NVIC but
  leaves the SoftDevice's reserved interrupts enabled, so a critical section
  cannot delay radio timing. The simulation uses
  `cortex-m/critical-section-single-core`.
- **`CriticalSectionRawMutex`** backs every channel, signal, watch, and mutex,
  and is held only for short synchronous sections.
- **Async mutexes** (`embassy_sync::mutex::Mutex`), the only locks held across
  an await: `GAP_PROCEDURE` serializes scan and connection establishment, and
  `DEVICE_STORE` guards the pairing cache during flash I/O.
- **Blocking mutex around a `RefCell`** for each endpoint mailbox's
  `EndpointDelivery`; no USB write or timer is awaited while it is held.
- **Plain `RefCell`** for the `Bonder` (shared by SoftDevice security
  callbacks, the connection workers, and the coordinator) and each
  connection's `ReportCoalescer` (shared by the GATT callback and the drain
  future). This is sound only because all of them run on the one thread-mode
  executor and no borrow spans an await.
- **Atomics** with relaxed ordering for `USB_CONFIGURED`, `USB_SUSPENDED`, the
  two boot-protocol flags, and `HID_ACTIVITY`; acquire/release for the cached
  `Bonder` pointer.

### Channels, Signals And Shared State

Channel capacities are listed in [tasks and data flow](#tasks-and-data-flow).
The other shared primitives:

| Primitive | Type | Producer → consumer | Semantics |
| --- | --- | --- | --- |
| `USB_SUSPEND_SIGNAL` | `Signal<bool>` | USB handler → UI loop | Latest value |
| `REMOTE_WAKE` | `Signal<()>` | Dispatcher → `usb_device_task` | Cleared on every suspend change and reset |
| `KEYBOARD_LEDS` | `Watch<KeyboardLeds, 2>` | USB control handler → connection workers | Latest value, one receiver per slot; each link reads the latest value when it starts |
| `RECONNECTS` | Blocking mutex over `ReconnectTable` | Both workers ↔ reconnect scan callback | One registered target, at most one 2 s sighting, and a holdoff after a failed attempt per slot |
| `RECONNECT_WAKE` | `Signal<()>` per slot | Reconnect scan callback → the other slot's worker | Wake-up between attempts; always equal to `ReconnectTable::wake_pending` |
| `FRAMES` | `Signal<Frame>` | UI loop → the display task | Latest frame wins |
| Endpoint `pending` and `lifecycle` | `Signal<()>` per endpoint | Dispatcher and USB handler → endpoint worker | Wake-ups; `lifecycle` is cleared when each transfer starts |
| Notification `wake` | `Signal<()>` per connection | GATT callback → drain future | Wake-up |
| `GAP_PROCEDURE` | Async mutex | Scanner and both workers | One GAP scan or connect at a time |
| `DEVICE_STORE` | Async mutex | Coordinator | Pairing cache |
| `COUNTERS` | `AtomicU32` per counter | Connection workers, notification loops, endpoint mailboxes, store → UI loop | Saturating counts since boot; each bump is one atomic update, so no lock |

Ownership of the data behind them is listed in the
[data model](data-model.md#data-ownership-rules).

### What May Block

- **Synchronous code blocks everything.** The scan callback, the GATT
  notification callback, and critical sections run without yielding. They do
  bounded work: merge into at most eight scan entries, match one reconnect
  advertisement against at most two registered targets (one AES block each),
  or classify at most 32 bytes and push one report. The simulation's UART logging is also blocking.
- **The UI loop never awaits a send.** BLE commands use `try_send`; the loop
  waits only in its `select4`.
- **The coordinator works inline.** It awaits sends to the UI event channel
  and the slot command channels, runs each scan (up to 10 seconds) and each
  flash write itself, and handles no other command or slot event meanwhile.
  Slot events queue up to the channel capacity; a worker blocks once it is
  full.
- **Slot workers.** The GAP connect call is not raced against commands, so a
  command waits up to the 6-second attempt plus the MTU exchange. The secure
  wait is bounded at 5 seconds. `close_connection` polls every 10 ms until the
  SoftDevice reports the link gone, with no deadline. The drain blocks while
  `HID_REPORT_CHANNEL` is full.
- **GAP contention.** A user scan waits for an in-flight connect attempt or
  reconnect scan. Each scans for at most 6 seconds; a connect attempt that
  finds its peer also holds the lock through connection setup and the MTU
  exchange.
- **USB.** The dispatcher never waits for an endpoint; each endpoint worker
  waits for host polling under its own 100 ms deadline.
- **Display.** The display task can wait indefinitely for a DMA that never
  completes; the isolation exists so that this blocks nothing else.
- **Buttons.** A button task blocks when `BUTTON_CHANNEL` is full and re-arms
  only after a stable release.

## Error Handling And Recovery Strategy

### How Errors Surface

BLE and storage failures reach the UI as `BleErrorTag` values (re-exported
from `coordinator::ErrorTag`), carried by `BleEvent::Error` or a
`ManagementResult`; the UI controller maps each tag to one fixed OLED message
in `controller::error_message`, and the UI adds a few messages of its own. `DeviceStore`
returns `StoreError::{Unreadable, Serialization, Flash, NotFound}`, and the
coordinator collapses every one of them into `StorageFailed`; a load failure
produces no `StoreError` but disables writes and raises `StorageFailed` once at
boot. A silent reconnect attempt that fails with `ConnectFailed` is retried
without telling the UI. Display faults never become tags: the display task logs
them and retries with backoff while the rest of the bridge keeps running.
Lost links, background reconnect attempts and failures, coalesced and
overflowed reports, and failed USB, LED, and flash writes are also counted in
`diagnostics::COUNTERS`, which the UI loop logs and nothing acts on
([event counters](operations.md#event-counters)).

The tag, every cause that raises it, and its OLED text are defined in one
place, the [data model](data-model.md#error-tags-and-ui-messages); what a user
should do about each message is in
[features](features.md#notices-and-errors).

### Retries, Deadlines And Backoff

| Path | Bound | On failure | Source |
| --- | --- | --- | --- |
| User scan | 8 s window, 10 s backstop | Results so far are used; a SoftDevice error shows "Scan failed" | [scanner.rs](../src/ble/scanner.rs) |
| Connect attempt | 6 s whitelist scan | User connect: error, and a connection that took over a background retry goes back to it while the device's keys remain. Silent: retry after 500 ms | [slot_worker.rs](../src/ble/slot_worker.rs), [slot_link.rs](../src/ble/slot_link.rs) |
| Reconnect scan | 6 s; fast duty cycle for 30 s after power-up or a lost link | Retry after 500 ms, or at once when the other slot's scan sees the device | [scanner.rs](../src/ble/scanner.rs), [reconnect.rs](../src/ble/reconnect.rs) |
| Encryption | 25 polls, 200 ms apart | `ConnectFailed` | [slot_link.rs](../src/ble/slot_link.rs) |
| GATT reads, writes, discovery, MTU exchange | SoftDevice ATT timeout, returned as a `Timeout` error by the vendored `gatt_client` instead of a panic | Service discovery: `HidNotFound`; Report Map read: `ReportMapReadFailed`; MTU exchange inside the connect call: `ConnectFailed`. When the link itself dropped, any discovery failure becomes `ConnectFailed`, which a background reconnect retries. A failed Report Reference read, Protocol Mode write, or CCCD write is logged or skipped, not fatal | [vendor/nrf-softdevice](../vendor/nrf-softdevice/README.bt2usb.md), [hid_client.rs](../src/ble/hid_client.rs) |
| Flash write | 3 attempts, 20 ms apart | `StorageFailed`; cache and bonds unchanged for Forget and Factory reset | [storage.rs](../src/storage.rs) |
| USB endpoint write | 100 ms deadline | Replay current state; back off 20 ms doubling to 1 s | [delivery.rs](../src/hid/delivery.rs) |
| OLED operation | 500 ms, then STOP and keep waiting | Re-initialize after 1 s doubling to 30 s | [display.rs](../src/ui/display.rs), [display_logic.rs](../src/ui/display_logic.rs) |
| Management request | `UI_MANAGEMENT_TIMEOUT_SECS` (30 s), checked on the 1 s tick | The UI stops waiting and shows No reply; a late reply carries the abandoned ID and is ignored | [controller.rs](../src/ui/controller.rs) |

What is retried is current state, not a stale packet or a stale address: USB
recovery replays the latest held input, and every bonded reconnect resolves the
peer's address again.

### What Is Fatal

All three firmware binaries use `panic-probe` with its `print-defmt` feature
(`use panic_probe as _`). A panic prints its message through `defmt` over RTT
and then halts the core; this is panic-probe's documented behavior, not
checked against the crate source here. The firmware defines no watchdog or
reset-on-panic path, so the device stays stopped until it is reset or
power-cycled. In the
simulation the message goes to the RTT buffer, not UART0. Host tests use the
standard test harness.

A stack overflow is fatal too. It faults in the MPU guard at the bottom of the
stack, and the HardFault handler in [stack.rs](../src/stack.rs), the only
`#[exception]` handler the firmware defines, logs
`"stack overflow: stack pointer {}, guard {}..{}, PC not stacked"` (on UART0
in the simulation) and spins. Every other HardFault spins there without a
line, as cortex-m-rt's default handler did
([ADR 0026](adr/0026-mpu-stack-guard.md)). A `probe-rs run` session, which is
what `mask run` starts, halts the core on entry to the handler before it logs
and prints `Firmware exited unexpectedly: Exception` instead; `mask selftest`
turns that catch off ([the runbook](operations.md#the-stack-overflows)).

Data from a BLE peer, the USB host, or flash must not be able to reach a
panic. Since 2026-10-10 Clippy rejects indexing, slicing, `unwrap`, `expect`,
and the panic macros outside tests, and the
[code quality guide](code-quality.md#panic-paths-no-lint-flags) lists every
remaining panic path, application and vendored, with the reason it cannot fire
([ADR 0025](adr/0025-panic-lints-and-inventory.md)). What remains:

- `Softdevice::enable` when the RAM reservation is too small (`"too little RAM
  for softdevice. Change your app's RAM start address to {:x}"`) or the
  configuration is rejected. It runs once per boot with a constant
  configuration, so it fails on every boot of a bad build or never.
- The SoftDevice fault handler in `nrf-softdevice`, for an internal assertion or
  an access to SoftDevice-reserved memory or peripherals.
- An error other than "no event" while `nrf-softdevice` fetches events, which
  the SoftDevice's API rules out. The event buffer is 256 bytes
  (`evt-max-size-256`), and a compile-time check in `sd_setup.rs` keeps it at
  least the largest event the configured ATT MTU allows (132 bytes at MTU 64),
  so a peer's discovery response cannot overflow it.
- Programming errors guarded at startup: `unwrap!` on each task spawn, a second
  `StaticCell` initialization, a full embassy-usb handler or interface list,
  and the `expect` that formats the USB serial.
- Contract checks inside the vendored crate and other dependencies, such as a
  second waiter on one SoftDevice event portal or a flash write future dropped
  before it completes; bt2usb's call patterns rule each out
  ([vendored list](code-quality.md#vendored-nrf-softdevice)).

Configuration that Rust can check at compile time fails the build instead of
the boot: the link count fits a `u8`, four device records with bonds fit one
flash item, the event buffer fits the largest event, and the pairing store's
flash range is valid for `sequential-storage` (`MapConfig::new` runs in a
`const` block). The memory-layout checks in `memory_sd.x` (RAM placement and
the end of `FLASH` at the pairing store) are link-time `ASSERT`s, not runtime
panics.

### Not Yet Handled

- No watchdog. The nRF52840 WDT is not configured and reset causes are not
  recorded. A task stuck forever, for example in `close_connection`, needs a
  reset or power cycle; when it is the BLE task, the UI shows **No reply** 30
  seconds after a management request and stays usable, but nothing restarts
  the BLE task. This is the P0 "Watchdog and
  recoverable failures" item in [TODO.md](../TODO.md); Proposed
  [ADR 0020](adr/0020-watchdog-and-progress-based-recovery.md) drafts the
  design.
- A background reconnect to a peer that has lost its keys retries without
  telling the user.
- Power loss during a flash write, during garbage collection, or during the
  Factory reset erase has no tested outcome.
- An electrically stuck I2C bus leaves the display degraded until reset.

## Execution And Power Choices

The design rules are:

- Tasks yield on I/O on one cooperative executor with no per-task stacks and
  no allocator, so blocking work in any task delays all of them
  ([what may block](#what-may-block)).
- The bridge is bus-powered and stays connected: it never enters System-OFF,
  and power states only decide whether the OLED is on
  ([ADR 0012](adr/0012-bus-powered-no-system-off.md)).
- Only a new press during USB suspend may wake the host; motion, releases, held
  state, and replays never do
  ([remote wakeup](#usb-suspend-remote-wakeup-and-resume)).
- The display is isolated: the UI publishes its latest state and never waits
  for I2C, and a stuck transfer degrades only the display task
  ([ADR 0009](adr/0009-isolated-display-task.md),
  [display update](#display-update)).

Power states, timeouts, and the declared USB current are listed in
[hardware](hardware.md#power).

## Configuration

All configuration is fixed at build time; there is no runtime settings store,
and the pairing store is the only state that survives a reset. Changing a
setting means rebuilding and reflashing.

- **Constants.** Shared timing, scan, connection, USB identity, button,
  display, and storage constants live in [src/config.rs](../src/config.rs),
  which the firmware binaries, the host crate, and `build.rs` compile. Pure
  modules size shared buffers from it (the link count and the UI list
  capacities); limits only one pure module enforces are defined in that module
  (see [module layers](#module-layers-and-dependency-rules)). Pins are chosen
  where each binary claims its peripherals, in `main.rs`, `selftest.rs`, and
  `sim.rs`. The values are listed in
  [hardware: configuration defaults](hardware.md#configuration-defaults) and
  [hardware: constants outside config.rs](hardware.md#constants-outside-configrs).
- **Builds.** The `embedded` feature (bridge and self-test, with the
  SoftDevice) and the `sim` feature (Renode, without SoftDevice, USB, or flash
  storage) are mutually exclusive: [build.rs](../build.rs) refuses to build
  both and selects the matching memory map. The library builds with no feature.
  Commands, features, and memory maps per configuration are in
  [development](development.md#build-configurations); the toolchain is pinned
  ([ADR 0013](adr/0013-pinned-toolchain-and-mask-tasks.md)).
- **Storage reservation.** `STORAGE_FLASH_PAGE_START`/`COUNT`, `memory_sd.x`,
  and the [memory map](hardware.md#memory-layout) change together; the link
  fails when the first two disagree
  ([ADR 0010](adr/0010-static-memory-layout.md)).

## Architecture Constraints

- Firmware paths use static allocation only; adding a heap needs an ADR.
- Decision logic lives in hardware-free modules with host tests; task code
  performs I/O and stays thin.
- Every buffer, queue, descriptor field, and stored record that a peer or flash
  can influence is bounded and validated before use.
- Held input must always be releasable: link loss, USB reset, suspend, and
  endpoint recovery end in a state with no stuck keys or buttons.
- The SoftDevice owns radio timing; scan and connect go through the GAP lock,
  and flash writes tolerate radio contention.
- Application interrupts run at priority 2, 3, 5, 6, or 7, never at the
  SoftDevice's 0, 1, or 4.
- SoftDevice callbacks run synchronously inside `softdevice_task`; they stay
  short and never await.
- No lock is held across an await except the `GAP_PROCEDURE` and
  `DEVICE_STORE` async mutexes.
- `memory_sd.x`, `STORAGE_FLASH_PAGE_START`/`COUNT`, and the
  [memory map](hardware.md#memory-layout) change together; the linker enforces
  the first two.
- Documentation separates implemented, software-verified, and hardware-verified
  behavior.

## ADR Process

Add an ADR when a change:

- changes task, channel, or module boundaries
- changes a persisted format, a USB descriptor, or a BLE security policy
- changes the memory map, flash partitioning, or SoftDevice version
- changes interrupt priorities, the executor model, or what may block
- introduces a material dependency, toolchain, or vendored patch
- changes input-delivery, loss, or wake guarantees
- changes how firmware is built, verified, released, or flashed

Name it with the next sequence number:

```text
docs/adr/NNNN-short-title.md
```

Statuses:

- **Proposed**: under review, not yet the project's direction
- **Accepted**: current direction
- **Superseded**: kept for history and linked to its replacement

Every ADR includes an "Alternatives Considered" section and a
"Verification Status" subsection under "Implementation" that separates
implemented, software-verified, and hardware-verified evidence, as in the
[template](#adr-template).

When an ADR or a code change alters a lifecycle traced in this guide, update
the matching [key runtime lifecycle](#key-runtime-lifecycles) in the same
change.

## Accepted ADRs

- [ADR 0001: Keep The README Short And Organize Detailed Docs By Reader](adr/0001-documentation-structure.md)
- [ADR 0002: Build On nRF52840, Nordic SoftDevice S140, And Embassy](adr/0002-nrf52840-softdevice-embassy.md)
- [ADR 0003: Keep Decisions In Hardware-Free Modules And I/O In Thin Tasks](adr/0003-pure-core-and-task-shell.md)
- [ADR 0004: Verify In Layers, From Host Tests To Hardware Acceptance](adr/0004-layered-verification.md)
- [ADR 0005: Aggregate Two BLE Sources Into Independent USB Endpoint Workers](adr/0005-two-slots-and-independent-endpoints.md)
- [ADR 0006: Persist Pairings In A Versioned, Fail-Closed Flash Store](adr/0006-fail-closed-pairing-store.md)
- [ADR 0007: Vendor A Minimal nrf-softdevice Patch At A Pinned Revision](adr/0007-vendored-softdevice-patch.md)
- [ADR 0008: Release Exact-Version Tags As Attested Drafts Of Checked Builds](adr/0008-attested-draft-releases.md)
- [ADR 0009: Isolate The OLED In Its Own Task With Stop-Safe I2C](adr/0009-isolated-display-task.md)
- [ADR 0010: Fix The Memory Map In The Linker Script And Assert It](adr/0010-static-memory-layout.md)
- [ADR 0011: Accept Just Works Bonding As An Interim Pairing Policy](adr/0011-interim-just-works-pairing.md)
- [ADR 0012: Stay Connected While Bus-Powered Instead Of Entering System-OFF](adr/0012-bus-powered-no-system-off.md)
- [ADR 0013: Pin The Toolchain And Wrap Workflows In Mask Tasks](adr/0013-pinned-toolchain-and-mask-tasks.md)
- [ADR 0014: Model nRF52840 GPIO SENSE/LATCH And GPIOTE PORT Events In Renode](adr/0014-renode-gpio-models.md)
- [ADR 0015: Reconnect Saved Devices At Power-Up With One Shared Scan](adr/0015-shared-reconnect-scan.md)
- [ADR 0016: Bound A Peripheral's Connection Parameter Requests In The Application](adr/0016-bounded-peer-connection-parameters.md)
- [ADR 0023: Hold Host Line Coverage At A Floor As A Regression Guard](adr/0023-host-coverage-floor.md)
- [ADR 0024: Model The TWIM And SSD1306 In Renode And Read The Panel's Text Back](adr/0024-renode-oled-models.md)
- [ADR 0025: Deny Panic-Prone Constructs With Clippy And List The Ones It Cannot See](adr/0025-panic-lints-and-inventory.md)
- [ADR 0026: Guard The Bottom Of The Stack With An MPU Region](adr/0026-mpu-stack-guard.md)

## Proposed ADRs

Drafted on 2026-10-10 for the P0 and P1 roadmap decisions below, each with a
recommendation, the alternatives weighed, and the facts checked against the
code. None is the project's direction until the owner answers its questions
in [Needs Your Input](../TODO.md#needs-your-input) and it is marked Accepted;
until then the Accepted ADRs above describe the firmware.

| ADR | Recommends | Would change |
| --- | --- | --- |
| [0017: Require LE Secure Connections And Authenticate Keyboards In A Bounded Pairing Window](adr/0017-authenticated-pairing-and-enrollment.md) | LE Secure Connections only; keyboards type an OLED passkey, Just Works devices get mouse reports only after a SELECT, bonds only inside a 60 s enrollment window; a background reconnect can never pair | Supersedes ADR 0011; amends ADR 0007 |
| [0018: Identify Releases With A pid.codes Product ID And Units By Their Factory Serial](adr/0018-production-usb-identity.md) | A pid.codes PID for tag builds only, `bcdDevice` from the version, the FICR serial kept and never logged, and a release check of the identity | USB descriptors and release packaging |
| [0019: Commit Pairing-Store Writes With A Generation Anchor And Migrate Formats Only By Rewriting](adr/0019-power-loss-safe-persistence.md) | Keep the atomic item append; add a CRC and generation (frame version 2) confirmed by an anchor item, so a rollback fails closed; pin `sequential-storage` exactly | Amends ADR 0006 |
| [0020: Supervise Progress With Deadline Leases And Reset Through The Watchdog](adr/0020-watchdog-and-progress-based-recovery.md) | An 8 s WDT fed only while no deadline lease is overdue; reset reasons recorded; arming stops after three early watchdog resets | New boot and task supervision |
| [0021: Lock The Debug Port In Production Images And Erase The Pairing Pages On Factory Reset](adr/0021-provisioning-debug-access-and-readout-protection.md) | Production images lock APPROTECT, an open service twin ships beside them, and every Factory reset erases the pairing pages | Amends ADRs 0006 and 0008 |
| [0022: Translate BLE HID Reports Through A Bounded Field Table Built From The Report Map](adr/0022-descriptor-driven-report-translation.md) | A per-link field table built from the Report Map translates reports field by field into the unchanged USB reports | Amends ADR 0005 |

## Decisions Needed For Roadmap Work

Write an ADR before implementing any of these [TODO.md](../TODO.md) items. The
first six have Proposed ADRs (above) waiting on the owner:

- authenticated pairing and enrollment policy (passkey or numeric comparison,
  pairing window, rejection of weaker devices): Proposed
  [ADR 0017](adr/0017-authenticated-pairing-and-enrollment.md)
- watchdog and progress-based recovery strategy: Proposed
  [ADR 0020](adr/0020-watchdog-and-progress-based-recovery.md)
- power-loss-safe persistence and storage-version migration policy: Proposed
  [ADR 0019](adr/0019-power-loss-safe-persistence.md)
- descriptor-driven HID report translation: Proposed
  [ADR 0022](adr/0022-descriptor-driven-report-translation.md)
- production USB identity and unit-identity policy: Proposed
  [ADR 0018](adr/0018-production-usb-identity.md)
- provisioning, debug access, and readout protection: Proposed
  [ADR 0021](adr/0021-provisioning-debug-access-and-readout-protection.md)
- bootloader, flash partitioning, and signed USB/BLE DFU
- multiple BLE profile sets
- persistent device settings (storage location, versioning, and defaults,
  without letting a settings record lock the pairing store)
- more than two simultaneous peripherals (would supersede
  [ADR 0005](adr/0005-two-slots-and-independent-endpoints.md) in part)
- USB interface and report extensions (System Control, NKRO, high-resolution
  mouse, battery strength, and the compatibility configuration)
- HID passthrough interfaces for device classes the fixed translation cannot
  represent
- a host management interface and how its changes are confirmed on the bridge

## ADR Template

```markdown
# ADR NNNN: Title

- Status: Proposed | Accepted | Superseded by ADR NNNN
- Date: YYYY-MM-DD

## Context

What forces, constraints, and current facts made this decision necessary?

## Decision

What did we decide?

## Alternatives Considered

Which other options were weighed, and why was each rejected?

## Rationale

Why this option over the alternatives?

## Consequences

What becomes easier, harder, riskier, or more constrained?

## Implementation

Where the decision lives in the code, configuration, and tooling.

### Verification Status

- **Implemented:** what is in the repository.
- **Software-verified:** which host tests or Renode scenarios exercise it.
- **Hardware-verified:** which recorded first-flash result covers it, or "not
  yet".

## Related

Links to guides, other ADRs, and TODO.md items.
```

## Related Guides

- [Features](features.md)
- [Data Model](data-model.md)
- [Hardware](hardware.md)
- [Development](development.md)
- [Code Quality](code-quality.md)
- [Testing](testing.md)
- [First Flash](first-flash.md)
- [Deployment](deployment.md)
- [Operations](operations.md)
- [Security](security.md)
