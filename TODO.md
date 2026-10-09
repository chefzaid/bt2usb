# Work Plan

This is bt2usb's complete work plan: every task the project has taken on or
still needs, grouped by theme. Checked items are implemented and name the files
that implement them. Unchecked items are open; each carries a priority and says
what evidence closes it.

A checked item records code or documentation present in the repository. It does
not certify hardware compatibility or a production deployment. No board has yet
passed the [first-flash checklist](docs/first-flash.md), so firmware behavior is
verified in software at most: pure modules by host tests, the task shells only
by embedded builds and Clippy, and the button, UI, and coordinator path by the
Renode scenario. Such items are marked *(hardware evidence pending)*.
Evidence belongs in the
[testing guide](docs/testing.md#hardware-acceptance-evidence) or a
[hardware-result issue](.github/ISSUE_TEMPLATE/hardware-result.md).

Priorities: **P0** blocks a managed production deployment; **P1** improves
reliability, interoperability, and maintainability; **P2** is a future feature.
*(hardware)* marks an open item that needs a physical board, peripheral, debug
probe, or USB host to close.

## Status At A Glance

| Section | Done | Open | Open P0 |
| --- | ---: | ---: | ---: |
| [BLE Central And Pairing](#ble-central-and-pairing) | 11 | 5 | 3 |
| [HID Report Parsing And Translation](#hid-report-parsing-and-translation) | 3 | 2 | 0 |
| [USB HID Device](#usb-hid-device) | 4 | 3 | 2 |
| [Input Aggregation And Delivery](#input-aggregation-and-delivery) | 3 | 2 | 2 |
| [Pairing Storage](#pairing-storage) | 4 | 5 | 3 |
| [UI, Display And Power](#ui-display-and-power) | 8 | 3 | 1 |
| [Platform, Memory And Recovery](#platform-memory-and-recovery) | 6 | 6 | 3 |
| [Device Security And Provisioning](#device-security-and-provisioning) | 1 | 3 | 2 |
| [Board Bring-Up And Hardware Acceptance](#board-bring-up-and-hardware-acceptance) | 2 | 5 | 3 |
| [Verification And Code Quality](#verification-and-code-quality) | 4 | 6 | 0 |
| [Release, Provenance And Supply Chain](#release-provenance-and-supply-chain) | 6 | 8 | 3 |
| [Developer Experience](#developer-experience) | 6 | 1 | 0 |
| [Documentation](#documentation) | 3 | 1 | 0 |
| [Product Extensions](#product-extensions) | 0 | 7 | 0 |
| **Total** | **61** | **57** | **22** |

**Most important next step:** the
[first board bring-up](#board-bring-up-and-hardware-acceptance). Install
SoftDevice S140 v7.3.0 on an nRF52840-DK, run `mask selftest`, then work
through [first-flash.md](docs/first-flash.md) and file the result. That single
record turns most *(hardware evidence pending)* marks into evidence or into
concrete defects, and it supplies the SoftDevice RAM and stack numbers the
memory budget needs.

## Contribution Rules For This Plan

- Keep this file the single plan. Add new work here before starting it, as an
  unchecked item with a priority and an acceptance criterion; do not track open
  work only in a guide, an issue, or a code comment.
- Close hardware and security gates before adding product features. A P2 item
  must not weaken a guarantee that a P0 or P1 item depends on.
- Put decisions in hardware-free modules with host tests and keep task code thin
  ([ADR 0003](docs/adr/0003-pure-core-and-task-shell.md)). Bound and validate
  every peer- or flash-controlled value before use.
- A change that crosses an [ADR trigger](docs/architecture.md#adr-process) adds
  or supersedes an ADR in the same change. The items titled "ADR:" below must
  be Accepted before the implementation item that follows them starts.
- Describe behavior as implemented, software-verified, or hardware-verified,
  and never present an open item as a feature in another guide.

## BLE Central And Pairing

Scanning, GATT HID discovery, bonding, and the two connection slots. Context:
[features](docs/features.md#ble-central),
[architecture](docs/architecture.md#pairing-and-storage), and
[security](docs/security.md#pairing-and-authentication).

- [x] BLE central scan, GATT HID discovery, report-reference classification,
  bonding/encryption, and two connection slots (`src/ble/`).
  *(hardware evidence pending)*
- [x] Keep connection-slot, scan-merge, and command/event decisions in a pure
  coordinator reducer that returns actions for the async shell to execute,
  with host tests (`src/ble/coordinator.rs`, `src/ble/coordinator_tests.rs`,
  `src/ble/multi_conn.rs`; [ADR 0003](docs/adr/0003-pure-core-and-task-shell.md)).
- [x] Serialize SoftDevice scan and connection setup with one shared GAP
  procedure lock, and bound each connection attempt by
  `BLE_CONNECT_TIMEOUT_SECS` (6 s) so a user scan does not wait indefinitely
  behind a reconnect (`src/ble/mod.rs` `GAP_PROCEDURE`, `src/ble/multi_conn.rs`,
  `src/config.rs`). *(hardware evidence pending)*
- [x] Stored pairing/bond records, boot reconnect planning, identity-key
  matching, and retries after link loss; a lost or not-yet-seen paired device is
  retried, with a `BLE_RECONNECT_BACKOFF_MS` pause between attempts, while its
  slot stays reserved (`src/storage.rs`, `src/ble/reconnect.rs`,
  `src/ble/multi_conn.rs`).
  *(hardware evidence pending)*
- [x] Peer-identity-scoped bond replacement and key lookup, stable identity
  persistence, and private-address resolution on background reconnect
  (`src/ble/multi_conn.rs`, `src/ble/scanner.rs`, `src/storage.rs`).
  *(hardware evidence pending)*
- [x] Require encrypted links before HID discovery, restrict
  application-initiated fresh pairing to explicit user connection attempts, and
  handle cancellation during owned-link security/discovery
  (`src/ble/multi_conn.rs`). *(hardware evidence pending)*
- [x] Preserve UTF-8 advertising names, merge scan-response names, and release
  the radio procedure lock before delivering UI scan results (`src/ble/`).
  *(hardware evidence pending)*
- [x] Discover a BLE keyboard's LED output report during HID discovery and write
  each host LED change to it from the slot that owns the link
  (`src/ble/hid_client.rs` `write_leds`, `src/ble/multi_conn.rs`; the USB side
  is under [USB HID Device](#usb-hid-device)). *(hardware evidence pending)*
- [x] Read complete GATT Report Maps by offset up to 512 bytes, distinguish
  missing maps from invalid/unreadable/oversized maps, and restrict legacy
  classification fallback to an absent characteristic (`src/ble/long_read.rs`,
  `src/ble/hid_client.rs`, `vendor/nrf-softdevice/README.bt2usb.md`).
  *(hardware evidence pending)*
- [x] Remove peer-triggerable panics and unbounded loops from vendored GATT
  discovery: resume after a truncated characteristic response, reject
  descriptor overflow and out-of-range, non-advancing, or empty responses, and
  saturate handle arithmetic. HID discovery failures now log the cause
  (`vendor/nrf-softdevice`, `src/ble/hid_client.rs`;
  [ADR 0007](docs/adr/0007-vendored-softdevice-patch.md)). Needs real-peripheral
  evidence under the Report Map interoperability item below.
- [x] Vendor `nrf-softdevice` at the pinned upstream commit through a root
  Cargo `[patch]`, adding `gatt_client::read_by_offset` (ATT Read Blob). ATT
  read timeouts return `ReadError::Timeout` instead of leaving the read waiting,
  and discovery and MTU-exchange timeouts return errors instead of panicking
  (`vendor/nrf-softdevice/`, `Cargo.toml`,
  `vendor/nrf-softdevice/README.bt2usb.md`).
- [ ] **P0** **ADR: authenticated pairing and enrollment.** Decide, per
  supported device class, between passkey entry and numeric comparison, how user
  presence is confirmed with the OLED and three buttons, the pairing-window
  length, and whether devices limited to Just Works are rejected. Accept when
  the ADR is Accepted, supersedes
  [ADR 0011](docs/adr/0011-interim-just-works-pairing.md), and is listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
- [ ] **P0** **Authenticated pairing and enrollment policy.** *(hardware)*
  Implement the ADR above: define whether each supported device uses
  authenticated pairing, how user presence is checked, and whether weaker
  devices are rejected; add a bounded pairing window and visible state. Accept
  when downgrade, unsolicited pairing, timeout, and reconnect cases have
  automated tests plus representative real-device evidence
  ([security](docs/security.md#pairing-and-authentication)).
- [ ] **P0** **Refuse peer-initiated pairing on background reconnects.**
  *(hardware)* The application starts pairing only for an explicit user
  connection, but the vendored crate answers a peer's Security Request by
  requesting pairing when no keys are found
  (`BLE_GAP_EVTS_BLE_GAP_EVT_SEC_REQUEST` in
  `vendor/nrf-softdevice/src/ble/gap.rs`). Background reconnects also target
  stored peers without a bond, so such a peer can start pairing. Accept when a
  Security Request on a link whose request does not allow pairing cannot create
  or replace a bond, shown by a test or by recorded on-air evidence
  ([security](docs/security.md#threat-model)).
- [ ] **P1** **Report Map interoperability and legacy policy.** *(hardware)*
  Validate the implemented 512-byte fragmented GATT reader against real
  peripherals with long maps and different MTUs. Decide whether the absent-map
  compatibility fallback remains allowed for deployment. Accept when captures
  demonstrate full reads and error handling, and same-length incompatible
  layouts are rejected by the supported descriptor-driven translation policy
  ([architecture](docs/architecture.md#hid-path-and-limits)).
- [ ] **P1** **Scan list under crowding.** The scan keeps the first
  `BLE_MAX_DISCOVERED` (8) HID advertisers it hears, so a crowded room, or
  deliberate fake advertisers, can hide the intended device. Decide how the
  bounded list chooses entries (for example by signal strength or by letting
  the user rescan with a name filter). Accept when host tests show the intended
  device is listed with more than eight HID advertisers present
  ([security](docs/security.md#threat-model)).

## HID Report Parsing And Translation

Turning peer descriptors and notifications into the fixed internal report
types. Context: [architecture](docs/architecture.md#hid-path-and-limits) and
[data model](docs/data-model.md#usb-hid-report-contracts).

- [x] Bounded HID collection/global-state parsing, Push/Pop handling, malformed
  descriptor rejection, overflow-safe report-size arithmetic, and strict
  known-report routing (`src/hid/report_protocol.rs`, `src/hid/mod.rs`, and
  descriptor and classification regression tests in
  `src/hid_descriptor_tests.rs`).
- [x] Consumer usage bounds, exact report-reference direction handling,
  rejection of oversized notifications, and distinct GATT payload classification
  (`src/hid/consumer.rs`, `src/ble/hid_client.rs`).
- [x] Five-button/scroll-capable mouse and consumer report types, alongside the
  six-key USB keyboard format: BLE mouse reports of 3 to 5 bytes map to five
  buttons, X/Y, wheel, and horizontal scroll (AC Pan), and consumer usages are
  limited to `MAX_CONSUMER_USAGE` (`0x0FFF`) (`src/hid/`).
  *(hardware evidence pending)*
- [ ] **P1** **ADR: descriptor-driven HID report translation.** Choose how
  fields are located by usage, bit offset, width, signedness, and report ID, what
  the bounded translation tables look like, and how unsupported layouts fail.
  Accept when the ADR is Accepted and listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
- [ ] **P1** **Descriptor-driven report translation.** Decode fields by usage,
  bit offset, width, signedness, and report ID for NKRO, packed mouse buttons,
  and 16-bit movement. Accept when a fixture corpus covers supported layouts and
  unsupported layouts fail explicitly without misclassifying input
  ([architecture](docs/architecture.md#hid-path-and-limits)).

## USB HID Device

The composite keyboard, mouse, and consumer-control device the PC sees.
Context: [features](docs/features.md#usb-hid-device) and
[hardware](docs/hardware.md#configuration-defaults).

- [x] Composite USB keyboard, mouse, and consumer interfaces, keyboard LED
  forwarding, remote-wakeup requests, and software VBUS event handling
  (`src/usb/hid_device.rs`). *(hardware evidence pending)*
- [x] USB boot/report protocol negotiation, three-byte boot mouse
  serialization, keyboard LED control-request validation, reset state cleanup,
  and stable factory-derived per-unit USB serials (`src/usb/hid_device.rs`,
  `src/hid/mouse.rs`). *(hardware evidence pending)*
- [x] Publish the host's keyboard LED output report through a `Watch` that both
  BLE slots observe, and clear it on USB reset (`src/usb/hid_device.rs`
  `KEYBOARD_LEDS`, `src/hid/keyboard.rs` `KeyboardLeds`).
  *(hardware evidence pending)*
- [x] Enable the SoftDevice's USB detected, power-ready, and removed events and
  seed the software VBUS detector from `USBREGSTATUS`, so unplug and replug are
  noticed after boot (`src/sd_setup.rs` `enable_usb_power_events`,
  `src/main.rs`). *(hardware evidence pending)*
- [ ] **P0** **ADR: production USB identity and unit identity.** Decide the
  VID/PID source, manufacturer and product strings, revision numbering, and how
  the factory-derived serial is used for unit identity. Accept when the ADR is
  Accepted and listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
- [ ] **P0** **USB production identity.** *(hardware)* Obtain an assigned
  VID/PID and define product, manufacturer, revision, and unit-identity policy;
  `src/config.rs` still uses the development `0x1209`/`0x0001`. Accept when
  descriptors and host inventory distinguish two units and development
  identifiers are absent from production builds
  ([hardware](docs/hardware.md#configuration-defaults)).
- [ ] **P1** **HID/USB conformance.** *(hardware)* Define and implement
  supported `GET_REPORT` and `SET_IDLE` behavior, validate boot/report protocol
  transitions, and reject invalid control requests consistently. Accept when
  descriptor/request tests and USB captures from a real host demonstrate the
  supported behavior ([testing](docs/testing.md#known-verification-gaps)).

## Input Aggregation And Delivery

Merging two BLE sources and delivering them through independent USB endpoint
workers. Context: [architecture](docs/architecture.md#hid-path-and-limits),
[features](docs/features.md#input-delivery), and
[ADR 0005](docs/adr/0005-two-slots-and-independent-endpoints.md).

- [x] Per-source held-input aggregation and bounded coalescing/delivery policy
  with host-testable logic (`src/hid/aggregate.rs`, `src/hid/coalesce.rs`,
  `src/hid/delivery.rs`); residual load and hardware acceptance work is open
  below.
- [x] Aggregate source-tagged input across two BLE slots, preserve the
  surviving source on disconnect, implement six-key rollover and deterministic
  consumer priority, and run independent bounded USB endpoint workers with
  retained-state replay and new-press-only wake (`src/hid/aggregate.rs`,
  `src/hid/delivery.rs`, `src/hid/wake.rs`, `src/usb/hid_device.rs`). Host
  regressions include actual asynchronous workers with fake endpoint sinks.
  *(hardware evidence pending)*
- [x] Release every key, mouse button, and consumer usage a BLE link was
  holding when that link ends for any reason: the slot worker sends
  `HidEvent::Disconnected` after its run phase, so a lost keyboard cannot leave
  a key repeating on the host (`src/ble/multi_conn.rs`, `src/hid/aggregate.rs`).
  *(hardware evidence pending)*
- [ ] **P0** **Multi-device aggregation hardware acceptance.** *(hardware)*
  Validate the implemented per-source key/modifier/button unions and
  consumer-slot priority with two real peripherals sharing an endpoint. Accept
  when releasing/disconnecting one source preserves the other's held state and
  the six-key rollover/consumer-priority behavior matches the documented policy
  on the USB host
  ([first flash](docs/first-flash.md#6-device-management-and-degraded-display)).
- [ ] **P0** **Backpressure behavior and bounded recovery.** *(hardware)*
  Specify whether transient taps and accumulated motion may be lost while USB is
  stalled. Test sustained input, queue saturation, suspend, unplug, and a
  simultaneous BLE disconnect. Accept when final releases reach a recovered host
  within a measured bound, neither slot starves, and any intentional loss policy
  is documented ([architecture](docs/architecture.md#hid-path-and-limits)).

## Pairing Storage

The versioned, fail-closed paired-device store in four reserved flash pages.
Context: [data model](docs/data-model.md#pairing-store),
[ADR 0006](docs/adr/0006-fail-closed-pairing-store.md), and
[security](docs/security.md#key-storage-and-deletion).

- [x] Frame the paired-device blob with a magic byte, version, record count, and
  per-record length prefixes in a pure, host-tested module; an unversioned blob
  loads through the legacy parser and is written in the versioned format on the
  next save (`src/storage/framing.rs`, `src/storage.rs`).
- [x] Validate complete storage frames and record metadata, reject unknown
  formats/types and malformed bond lengths, abort partial serialization, and
  preserve unreadable stores by disabling writes (`src/storage/`,
  `src/storage.rs`).
- [x] Retry a flash write up to three times, 20 ms apart, because SoftDevice
  flash operations need radio-idle timeslots and can fail transiently while
  links are active, and skip the write when only the RSSI hint changed
  (`src/storage.rs` `FLASH_WRITE_ATTEMPTS`, `DeviceStore::add`).
  *(hardware evidence pending)*
- [x] Add saved-device management with default-Cancel confirmations, stable
  selected identities, worker quiescence before mutation, transactional cached
  bond updates, and explicit reset recovery of unreadable storage (`src/ui/`,
  `src/ble/management.rs`, `src/ble/multi_conn.rs`, `src/storage.rs`).
  *(hardware evidence pending)*
- [ ] **P0** **ADR: power-loss-safe persistence and storage migration.**
  Decide the commit protocol for interrupted writes and garbage collection, the
  version-upgrade and downgrade rules, and whether a major `sequential-storage`
  upgrade may change the on-flash layout. Accept when the ADR is Accepted and
  listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
- [ ] **P0** **Forget/reset hardware and interruption acceptance.**
  *(hardware)* Validate the implemented confirmation, worker shutdown,
  persistence, and cache-update paths on a board. Accept when forgotten peers
  cannot reconnect across reboot, failed writes retain the documented state,
  and power interruption during both logical deletion and unreadable-store
  recovery has a tested outcome
  ([first flash](docs/first-flash.md#6-device-management-and-degraded-display)).
- [ ] **P0** **Power-loss-safe persistence.** *(hardware)* Fault-inject flash
  writes, garbage collection, malformed records, and version changes. Accept
  when interrupted writes retain the last committed valid state or take a
  documented safe reset path, without accepting corrupted bond material or
  silently replacing it ([data model](docs/data-model.md#write-rules)).
- [ ] **P1** **Versioned storage migration.** Document each wire version and
  supported upgrade/downgrade paths. The legacy parser and the identity merge in
  `DeviceStore` live in `src/storage.rs`, which the host crate does not compile,
  so they have no host tests today. Accept when fixture tests reject unknown
  versions and cover valid legacy conversion, malformed data, and capacity
  changes ([data model](docs/data-model.md#schema-change-rules)).
- [ ] **P1** **Pairing region survives application reflash.** *(hardware)*
  Normal application flashing is expected to preserve pages 240 to 243, but the
  erase behavior of `probe-rs` in `mask run --release` has not been checked.
  Accept when a board with saved peers is reflashed with the documented command
  and the peers reconnect without pairing again, with the probe-rs version and
  erase mode recorded ([deployment](docs/deployment.md#flash-a-development-unit)).

## UI, Display And Power

The OLED, three buttons, UI state machine, and display power policy. Context:
[features](docs/features.md#local-ui-and-power),
[architecture](docs/architecture.md#execution-and-power-choices), and
[ADR 0009](docs/adr/0009-isolated-display-task.md).

- [x] Async OLED rendering, three debounced buttons, and activity-driven
  display power policy (`src/ui/`, `src/power_logic.rs`).
  *(hardware evidence pending)*
- [x] Drive the SSD1306 through async I2C (`ssd1306` `async` feature with
  `embedded-hal-async`) so a display transfer yields to other tasks, and enable
  the TWIM internal pull-ups for modules without their own (`src/ui/display.rs`,
  `src/main.rs`, `Cargo.toml`). *(hardware evidence pending)*
- [x] Keep screen transitions and button handling in pure, host-tested
  reducers (`src/ui/ui_logic.rs`, `src/ui/input_logic.rs`,
  `src/ui/display_logic.rs`).
- [x] Scroll the discovered-device list to keep its selection visible, reject
  invalid selections, use a persistent UI ticker, and handle button levels and
  suspend-aware activity (`src/ui/`, `src/main.rs`, `src/power_logic.rs`).
- [x] Retain errors and completion notices until acknowledgment; isolate OLED
  rendering from input/UI tasks, retry completed I2C failures, and request STOP
  after the display deadline without unsafely dropping an active DMA future
  (`src/main.rs`, `src/ui/display.rs`, `src/ui/ui_logic.rs`;
  [ADR 0009](docs/adr/0009-isolated-display-task.md)).
  *(hardware evidence pending)*
- [x] Tag saved-device list, Forget, and reset requests with a unique ID and
  allow one at a time, so a delayed or duplicate reply cannot complete a newer
  action (`src/ui/ui_logic.rs` `ManagementRequests`, `src/main.rs`,
  `src/ble/multi_conn.rs`). Reducer tests cover stale IDs and ID wraparound.
- [x] Hold TWIM NACK/overrun errors until the peripheral reports STOPPED. The
  pinned driver returns before STOPPED, which could release display-interface's
  transfer buffer during the stop sequence and let the retry clear the pending
  event (`src/ui/display.rs` `StopSafeI2c`, also used by the self-test).
  *(hardware evidence pending)*
- [x] Count live HID traffic as activity so typing keeps the OLED on, and never
  enter System-OFF on bus power, so BLE links and USB enumeration stay up
  (`src/power.rs` `note_hid_activity`, called from `src/usb/hid_device.rs`;
  `src/power_logic.rs`;
  [ADR 0012](docs/adr/0012-bus-powered-no-system-off.md)).
  *(hardware evidence pending)*
- [ ] **P0** **Power budget and USB suspend current.** *(hardware)* Measure
  supply current while idle, scanning, with two links, with the OLED on and off,
  and during USB suspend, where the bridge keeps its BLE links. Compare with the
  100 mA the configuration descriptor declares (`src/usb/hid_device.rs`
  `max_power`) and with the USB suspend-current limit. Accept when the measured
  margins are recorded and either meet those limits or a reviewed exception
  updates [ADR 0012](docs/adr/0012-bus-powered-no-system-off.md)
  ([release gates](docs/deployment.md#release-gates)).
- [ ] **P1** **Bounded management wait in the UI.** While a saved-devices list,
  Forget, or reset request is pending, the UI ignores every button until the BLE
  coordinator replies. A hung coordinator therefore needs a power cycle. Define
  a deadline and a "result unknown, reopen saved devices" state that never
  claims success or failure it did not observe. Accept when reducer tests cover
  the lost-reply path and a late reply is rejected by its request ID
  ([features](docs/features.md#manage-saved-devices)).
- [ ] **P1** **Visible storage/security errors.** Surface pairing persistence
  failure, full-store replacement, unsupported reports, and security failures
  with useful user actions. Today a failed save shows only the generic
  `Storage failed` error, a link that cannot be secured shows `Connect failed`
  (`ble_error_message` in `src/main.rs`), and full-store replacement is only a
  `Paired device store full - evicting oldest entry` log line. Accept when UI
  tests cover every state and a user
  can distinguish a temporary link failure from a peer that was not saved
  ([operations](docs/operations.md#recovery-and-diagnostics)).

## Platform, Memory And Recovery

The MCU platform, interrupt and memory layout, and recovery from stuck
subsystems. Context: [hardware](docs/hardware.md#memory-layout),
[ADR 0002](docs/adr/0002-nrf52840-softdevice-embassy.md), and
[ADR 0010](docs/adr/0010-static-memory-layout.md).

- [x] Build on the nRF52840 with Nordic SoftDevice S140 v7.3.0 and Embassy's
  async executor, with static allocation and no heap (`Cargo.toml`,
  `src/main.rs`; [ADR 0002](docs/adr/0002-nrf52840-softdevice-embassy.md)).
- [x] Share one SoftDevice configuration (two central links, 64-byte ATT MTU,
  no advertising or peripheral role) between the bridge and the self-test
  (`src/sd_setup.rs`).
- [x] Run the USBD, TWISPI0 (OLED I2C), GPIOTE, and time-driver interrupts at
  priority 2, outside the levels the SoftDevice reserves (0, 1, and 4), so they
  cannot preempt its critical sections (`src/main.rs`).
  *(hardware evidence pending)*
- [x] Separate application and pairing flash regions, linker assertions, and
  stack high-water instrumentation (`memory_sd.x`, `src/stack.rs`).
- [x] Link with plain rust-lld instead of flip-link, because nrf-softdevice
  passes `__sdata` to the SoftDevice as its application RAM base; `memory_sd.x`
  asserts that `.data` starts at `ORIGIN(RAM)` and the stack sits at the top of
  RAM (`.cargo/config.toml`, `memory_sd.x`;
  [ADR 0010](docs/adr/0010-static-memory-layout.md)).
  *(hardware evidence pending)*
- [x] Select `memory_sd.x` or `memory_sim.x` by feature in `build.rs` and copy
  it to `OUT_DIR/memory.x`. Neither source is named `memory.x`, so no layout in
  the crate root shadows the copied one (rust-lld searches the current directory
  first), and the script reruns when the `sim` feature toggles (`build.rs`).
- [ ] **P0** **ADR: watchdog and progress-based recovery.** Decide which tasks
  must prove progress before the watchdog is fed, the timeout, what state
  survives a reset, and how reset causes are recorded. Accept when the ADR is
  Accepted and listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
- [ ] **P0** **Watchdog and recoverable failures.** *(hardware)* Define
  progress-based watchdog feeding and recovery for stuck I2C, stalled USB, flash
  errors, and BLE task failure; today no watchdog exists and a stuck I2C bus
  leaves the display task degraded. Stuck-bus cases need an I2C fault-injection
  fixture, which the first-flash OLED isolation check also lacks. Accept when
  injected faults cannot leave permanent held input or require an undocumented
  recovery sequence; record reset causes
  ([operations](docs/operations.md#recovery-and-diagnostics)).
- [ ] **P0** **Memory and endurance budget.** *(hardware)* Measure SoftDevice
  RAM at enable (`softdevice RAM: N bytes`) and worst-case stack high-water
  (`stack high-water: X of Y bytes`) with two links, scanning, display, and
  persistence, as the
  [first-flash checklist](docs/first-flash.md#2-self-test-image) asks. Verify
  flash size and storage wear assumptions. Accept when reviewed margins are
  recorded and `memory_sd.x` matches the measured requirement; do not reduce
  its current 24 KiB reservation based only on estimates.
- [ ] **P1** **Stack overflow detection.** Without flip-link a stack overflow
  grows into `.bss` and corrupts statics silently; no MPU guard region is
  configured, and the painted-stack high-water mark is evidence after the fact.
  Evaluate a guard compatible with the SoftDevice RAM layout. Accept when a
  deliberate overflow in a test build faults with a diagnosable message instead
  of corrupting memory ([ADR 0010](docs/adr/0010-static-memory-layout.md)).
- [ ] **P1** **Single source for the pairing flash range.**
  `STORAGE_FLASH_PAGE_START`/`STORAGE_FLASH_PAGE_COUNT` in `src/config.rs` and
  the end of `FLASH` in `memory_sd.x` define the same boundary twice, and
  nothing checks that they agree. Accept when changing either one alone fails
  the build or a CI check ([hardware](docs/hardware.md#memory-layout)).
- [ ] **P1** **Diagnostics without sensitive input.** Add firmware/build
  identification, reset reasons, bounded counters for reconnect/queue/write
  failures, and a documented collection method. Accept when reports support
  reproduction without logging key material or keystroke content
  ([operations](docs/operations.md#reporting-a-defect)).

## Device Security And Provisioning

Physical access, key protection, and production provisioning. Context:
[security](docs/security.md#security-posture-summary) and
[data model](docs/data-model.md#privacy-and-retention).

- [x] Keep bond keys and HID report contents out of application logs: no
  `defmt` statement under `src/` formats bond keys, BLE input reports,
  notification bytes, or flash record bytes (the host's keyboard LED state is
  logged as `Host LEDs: num={} caps={} scroll={}`); CI and release-package
  builds log at `info` (`src/`, `.github/workflows/ci.yml`,
  `scripts/release.py`;
  [security](docs/security.md#logging-and-privacy)).
- [ ] **P0** **ADR: provisioning, debug access, and readout protection.**
  Decide the production debug-port policy, readout protection, key lifetime and
  disposal, service recovery, and whether bond records need protection beyond
  readout protection. Accept when the ADR is Accepted and listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
- [ ] **P0** **Provisioning and physical key protection.** *(hardware)* Define
  production debug access, readout protection, key lifetime, disposal, and
  service recovery. Accept when the threat model and provisioning procedure are
  reviewed and readout/recovery behavior is demonstrated on a
  production-equivalent board
  ([security](docs/security.md#key-storage-and-deletion)).
- [ ] **P1** **Vendored debug and trace logs.** Local builds log at `debug`
  (`.cargo/config.toml`), where the vendored `nrf-softdevice` records each peer
  address (`connected role={:?} peer_addr={:?}` in
  `vendor/nrf-softdevice/src/ble/central.rs`), and at `trace` it logs raw
  notification bytes, which are keystrokes
  (`GATT_HVX write handle={:?} type={:?} data={:?}` in
  `vendor/nrf-softdevice/src/ble/gatt_client.rs`).
  Accept when the default local build records no peer addresses, notification
  data can be logged only through an explicit, documented opt-in, and the debug
  output of `embassy-usb`, `embassy-nrf`, and `sequential-storage` has been
  reviewed ([security](docs/security.md#logging-and-privacy)).

## Board Bring-Up And Hardware Acceptance

Getting firmware onto real boards and recording what works. Context:
[first flash](docs/first-flash.md),
[testing](docs/testing.md#hardware-acceptance-evidence), and
[ADR 0004](docs/adr/0004-layered-verification.md).

- [x] Board self-test and reusable first-flash checklist (`src/selftest.rs`,
  [first-flash.md](docs/first-flash.md)).
- [x] Fix self-test flash ownership and pairing-record buffer capacity; reject
  combined firmware/simulation features (`src/selftest.rs`, `build.rs`).
- [ ] **P0** **First board bring-up.** *(hardware)* Install S140 v7.3.0, run
  `mask selftest`, then run the bridge on an nRF52840-DK with the documented
  wiring, one BLE keyboard, one BLE mouse, a direct host connection, and then a
  monitor hub. Accept when a
  [hardware-result issue](.github/ISSUE_TEMPLATE/hardware-result.md) records the
  commit and ELF hash, a self-test summary line with zero failures
  (`==== self-test done: P passed, 0 failed, S skipped ====`), the SoftDevice
  RAM and stack values, and pass, fail, or skipped-with-reason for every
  [first-flash](docs/first-flash.md) step; each failure gets its own issue.
- [ ] **P0** **Hardware compatibility baseline.** *(hardware)* Run
  [first-flash.md](docs/first-flash.md) on declared keyboard/mouse models,
  Windows/Linux/macOS hosts, monitor hubs, and BIOS/UEFI or KVM targets. Accept
  when the matrix identifies exact versions, pass/fail results, known
  limitations, and the artifact hash.
- [ ] **P0** **Production hardware definition.** *(hardware)* Choose and
  document the deployed board (the DK or a custom PCB), display module, buttons,
  supply arrangement, and enclosure; move any pin changes into the board setup
  in `src/main.rs`, `src/selftest.rs`, and `src/sim.rs`. Accept when
  [hardware](docs/hardware.md#parts-and-wiring) lists the production bill of
  materials and pin map and that board has a passing first-flash record
  ([release gates](docs/deployment.md#release-gates)).
- [ ] **P1** **Hardware-in-the-loop check.** *(hardware)* No CI job flashes a
  board; the self-test and first-flash layers are manual. Attach a board and
  probe to a dedicated runner that flashes `bt2usb-selftest` for changes to
  the BLE, USB, storage, or memory-map boundary, kept apart from the release
  jobs. Accept when that job fails on any `[FAIL]` self-test line and its RTT
  log is kept as an artifact
  ([testing](docs/testing.md#known-verification-gaps)).
- [ ] **P1** **Soak and latency measurements.** *(hardware)* Run a defined
  multi-day keyboard/mouse workload with disconnects, monitor power changes, and
  flash updates. Accept when latency percentiles, reconnect time,
  dropped-input policy, reset count, and stack/flash margins meet published
  limits ([testing](docs/testing.md#known-verification-gaps)).

## Verification And Code Quality

Host tests, simulation, and code-health work. Context:
[testing](docs/testing.md), [code quality](docs/code-quality.md), and
[ADR 0004](docs/adr/0004-layered-verification.md).

- [x] Pure shared host-test modules, unit/integration tests, and coverage tasks
  (`src/lib.rs`, `tests/`, `maskfile.md`).
- [x] Share one implementation between firmware and host tests: split the
  oversized `src/lib.rs`, `src/ble/coordinator.rs`, and `src/storage.rs` into
  sibling test and codec files, and drop the duplicate HID classifier from the
  host library (`src/lib.rs`, `src/lib_tests.rs`, `src/lib_logic_tests.rs`,
  `src/ble/coordinator_tests.rs`, `src/hid_descriptor_tests.rs`,
  `src/storage/codec.rs`; commit `e3bc620`). Four files are over 500 lines
  again today (`src/ble/multi_conn.rs`, `src/ui/ui_logic.rs`,
  `src/usb/hid_device.rs`, `src/lib_tests.rs`, counted with `wc -l`).
- [x] SoftDevice-free Renode build and a headless GPIO/UI/coordinator scenario
  (`src/sim.rs`, `memory_sim.x`, `renode/bt2usb-sim.resc`,
  `renode/bt2usb-sim.robot`).
- [x] Custom Renode GPIO/GPIOTE models of the pin SENSE, LATCH, DETECT, and
  GPIOTE PORT event chain that embassy-nrf uses for async edge waits, so the
  Robot test presses UP, DOWN, and SELECT through the real button tasks and
  `BUTTON_CHANNEL` (`renode/nrf52840_sense_gpio.cs`,
  `renode/nrf52840-sense-gpio.repl`;
  [ADR 0014](docs/adr/0014-renode-gpio-models.md)).
- [ ] **P1** **Parser fuzzing and property tests.** Add bounded fuzz targets for
  HID descriptors, advertisements, report classification, and persistence
  framing. Accept when CI runs a seed corpus and scheduled fuzzing records no
  panics, out-of-bounds access, excessive work, or invalid accepted output
  ([testing](docs/testing.md#known-verification-gaps)).
- [ ] **P1** **Async task fault tests.** Exercise command cancellation, full
  event channels, scan/reconnect contention, repeated security failures, and
  device disappearance during discovery. Accept when deterministic test cases
  assert completion, retry policy, and UI state without relying only on reducer
  tests ([testing](docs/testing.md#known-verification-gaps)).
- [ ] **P1** **Run the scanner's advertisement tests on the host.** The ten
  `#[test]` functions in `src/ble/scanner.rs` never run: `src/lib.rs` does not
  compile `scanner`, and the firmware target has no test harness. Move them
  beside `ble::adv_parser`, which the host crate compiles, or delete the ones
  that duplicate existing host tests. Accept when every `#[test]` function in
  `src/` is compiled by `cargo test --locked --lib --tests`
  ([testing](docs/testing.md#tests-that-do-not-run)).
- [ ] **P1** **Host tests for the I/O shells.** The connection workers, GATT
  HID client, storage shell and codec, USB device, and display driver
  (`src/ble/multi_conn.rs`, `src/ble/hid_client.rs`, `src/storage.rs`,
  `src/storage/codec.rs`, `src/usb/hid_device.rs`, `src/ui/display.rs`) have no
  host tests; only the pure modules they call do. Move remaining decisions into
  hardware-free modules or test the shells against fakes. Accept when each has
  host tests for its error paths, or the testing guide records why it cannot
  ([testing](docs/testing.md#modules-without-host-tests)).
- [ ] **P1** **Broaden the Renode scenarios.** The Robot test runs one scripted
  scenario; it does not exercise the OLED task, the saved-device management
  screens, or the link-loss slot-reservation path. Add scenarios for each.
  Accept when `mask sim-test` and the CI simulation job assert those paths
  through UART output ([testing](docs/testing.md#renode-scenario-map)).
- [ ] **P1** **Coverage and firmware documentation in CI.** CI measures no
  coverage and builds rustdoc only for the host library. Publish the
  `cargo llvm-cov` report as a CI artifact, set a threshold once a baseline is
  recorded, and build firmware rustdoc with warnings denied. Accept when a
  coverage drop below the threshold or a firmware rustdoc warning fails CI
  ([code quality](docs/code-quality.md)).

## Release, Provenance And Supply Chain

CI, tagged releases, provenance, and dependency maintenance. Context:
[deployment](docs/deployment.md),
[testing](docs/testing.md#continuous-integration),
[security](docs/security.md#supply-chain), and
[ADR 0008](docs/adr/0008-attested-draft-releases.md).

- [x] GitHub Actions build/test/simulation workflow and tag-based firmware
  artifact generation (`.github/workflows/ci.yml`).
- [x] Pin Rust/dependency resolution and BLE Git revision, align Cargo license
  metadata with the existing GPL license, and use locked build tasks
  (`rust-toolchain.toml`, `Cargo.lock`, `Cargo.toml`, `maskfile.md`;
  [ADR 0013](docs/adr/0013-pinned-toolchain-and-mask-tasks.md)).
- [x] Configure Linux/Windows host checks, embedded/simulation lint and build,
  scheduled dependency auditing, pinned action revisions, restricted default
  workflow permissions, and draft release artifacts with checksums/build inputs
  (`.github/workflows/ci.yml`, `.github/dependabot.yml`). GitHub Actions has
  confirmed the push and scheduled path: push runs 36441995385 (commit
  `8a04b25`, 2026-09-28) and 37932436721 (commit `7fc99d6`, 2026-10-09) and
  scheduled run 37338711407 (2026-10-05) passed all five check jobs. The
  tag-only release jobs have never run; see the hosted
  provenance item below.
- [x] Install actionlint 1.7.12 from its upstream release and verify its
  SHA-256 before use, replacing an install-action fallback that failed the Linux
  host job (`.github/workflows/ci.yml`).
- [x] Validate exact release tag/Cargo version equality, including
  prereleases; reuse the successful embedded build by immutable artifact ID;
  verify source, build-input and firmware hashes before packaging; configure
  SHA-pinned GitHub provenance attestations separately from draft publication
  permissions (`scripts/release.py`, `.github/workflows/ci.yml`). Twelve helper
  regression tests pass on Windows and WSL; actionlint 1.7.12 passes. Hosted
  issuance and downloaded-release verification remain open below.
- [x] Stage release inputs and Renode results in the runner's temporary
  directory instead of the Rust-cached `target/`. Fail the release job before
  upload when the tag's release is already published; reruns may update only a
  draft. Action pin comments name the exact upstream tag each SHA resolves to,
  except `Swatinem/rust-cache` and `taiki-e/install-action`, whose comments name
  only the moving major tag `v2` (open under CI runtime maintenance below)
  (`.github/workflows/ci.yml`,
  [deployment](docs/deployment.md#re-running-a-tag-workflow)).
- [ ] **P0** **Hosted provenance and release recovery acceptance.**
  *(hardware)* Push the first release tag (none exists yet), run the configured
  tag/attestation workflow, verify its downloaded artifacts against the approved
  commit from a clean machine, and document storage migrations, rollback
  constraints, and service flashing. Accept when provenance verification and
  failed-update recovery have evidence; local helper tests and workflow lint do
  not exercise GitHub signing or device recovery
  ([deployment](docs/deployment.md#verify-before-flashing)).
- [ ] **P0** **Release notes for deployment releases.** The release job uses
  generated notes only. Add a release-notes template and fill it for each
  deployment draft. Accept when a draft's notes list supported versions,
  compatibility limits, migrations, rollback constraints, checksums, and the
  exact SoftDevice prerequisite
  ([release gates](docs/deployment.md#release-gates)).
- [ ] **P0** **Security maintenance ownership.** Publish a private reporting
  contact, supported-version policy, triage ownership, and response/update
  expectations. Accept when dependency/advisory review, license inventory, and
  remediation tracking are part of a documented release procedure
  ([security policy](SECURITY.md)).
- [ ] **P1** **Supply-chain and tooling maintenance.** Extend the dependency
  audit and update automation with license checks, an SBOM artifact, and
  verified digests for downloaded non-Cargo tooling/SoftDevice inputs; today
  `scripts/install-renode.sh` and `mask softdevice` download archives without a
  digest check. Accept when a license conflict or digest mismatch fails the
  relevant check with an actionable message and dependency alerts have an
  assigned review process ([security](docs/security.md#supply-chain)).
- [ ] **P1** **Replace unmaintained transitive dependencies.** The 2026-09-28
  audit reported `bare-metal 0.2.5` (`RUSTSEC-2026-0110`) and
  `proc-macro-error 1.0.4` (`RUSTSEC-2024-0370`) as unmaintained; both are still
  in `Cargo.lock`. Trace their dependency chains, track the upstream migration,
  and adopt maintained replacements through dependency upgrades; CI runs
  `cargo audit` without a deny option, so these warnings do not fail the job
  today. Accept when the lockfile no longer selects these affected versions,
  the audit is clean without advisory suppression, the CI audit fails on
  unmaintained-crate warnings, and host/firmware/simulation regression checks
  pass ([testing](docs/testing.md#validation-record--2026-09-28)).
- [ ] **P1** **Major dependency upgrades.** *(hardware)* Dependabot proposed
  embassy-nrf 0.11, embassy-sync 0.8, and sequential-storage 8. Pull requests
  #7 and #8 (the first two) were closed unmerged, and the open sequential-storage
  8.0.2 pull request #9 fails the embedded Clippy step. Upgrade deliberately:
  recheck the driver assumptions documented in `src/usb/hid_device.rs`
  (`UsbReportSink::write` cancellation) and `src/ui/display.rs` (`StopSafeI2c`,
  `finish_or_stop`), and prove the pairing store still loads. Accept when CI
  passes, existing stores load after the upgrade, and the affected first-flash
  sections pass on a board
  ([security](docs/security.md#supply-chain)).
- [ ] **P1** **CI runtime maintenance.** The 2026-10-09 run reports that the
  pinned `actions/checkout` v4.4.0 and `actions/upload-artifact` v4.6.2 target
  the deprecated Node.js 20 runtime, and that `ubuntu-latest` moves to Ubuntu 26
  from 2026-10-19. Move to maintained action releases (Dependabot pull
  requests #4 and #5 were closed unmerged) and pin or validate the runner
  image. Replace the `# v2` comments on the `Swatinem/rust-cache` and
  `taiki-e/install-action` pins with the exact release each SHA resolves to.
  Accept when a run shows no deprecation annotations and every pin comment
  names its exact tag
  ([testing](docs/testing.md#continuous-integration)).
- [ ] **P1** **Reproducible firmware evidence.** Compare artifacts from two
  clean environments, document remaining nondeterminism, and enforce release
  size budgets. Accept when the release record identifies compiler,
  dependency, external-tool, source, and artifact hashes
  ([testing](docs/testing.md#known-verification-gaps)).

## Developer Experience

Tasks, tooling, and environments for working on the firmware. Context:
[development](docs/development.md), the [task reference](maskfile.md), and
[ADR 0013](docs/adr/0013-pinned-toolchain-and-mask-tasks.md).

- [x] WSL-aware task tooling and a VS Code devcontainer
  (`scripts/run-tool.sh`, `.devcontainer/`).
- [x] Wrap build, flash, self-test, host tests, coverage, size, simulation,
  SoftDevice, and probe tasks as 31 Bash `mask` recipes; `mask ci` runs the
  local formatting, lint, test, and build subset (`maskfile.md`).
- [x] Build host tests for the native platform by leaving `build.target` unset,
  and scope the probe-rs runner and ARM linker flags to
  `cfg(all(target_arch = "arm", target_os = "none"))` (`.cargo/config.toml`).
- [x] Install portable Renode and the `renode-test` Python dependencies into
  the user's home without root (`scripts/install-renode.sh`, `mask sim-setup`).
- [x] Resolve WSL tool fallbacks only from the current Windows user's profile;
  fix the devcontainer build recipe and stop SoftDevice download/flash tasks on
  download or extraction failure (`scripts/run-tool.sh`, `maskfile.md`).
- [x] Stop devcontainer setup when required tool installation or its host-test
  smoke check fails (`.devcontainer/post-create.sh`). Scoped probe permissions
  and optional-tool/container pinning remain open below.
- [ ] **P1** **Development environment hardening.** *(hardware)* Pin optional
  tools/container inputs and replace blanket container privilege with scoped
  probe access where feasible. Today the devcontainer runs `--privileged` on the
  `mcr.microsoft.com/devcontainers/rust:1-bookworm` tag, and `mask deps` runs
  `cargo install` without `--locked`. Accept when fresh Linux/WSL setups pass
  checks and a no-probe setup still works
  ([development](docs/development.md#devcontainer-and-wsl2)).

## Documentation

Guides, ADRs, and this plan. Context:
[ADR 0001](docs/adr/0001-documentation-structure.md) and the
[ADR process](docs/architecture.md#adr-process).

- [x] Reorganized setup/use, hardware, architecture, development, testing,
  operations, and security documentation; separated this backlog from completed
  work and removed unqualified compatibility/coverage claims (2026-09-28).
- [x] Restructured documentation by reader: lower-case guides in `docs/`, a
  short README, a security reference separate from the reporting policy, a
  data-model reference, eight ADRs in `docs/adr/`, and GitHub issue templates
  (2026-10-09; see [ADR 0001](docs/adr/0001-documentation-structure.md)).
- [x] Deepened every guide into a complete reference, added the
  [code quality](docs/code-quality.md) guide and ADRs 0009 to 0014 for
  decisions already in the code, simplified the README, and rebuilt this file as
  the complete work plan with done and open work side by side (2026-10-09).
- [ ] **P1** **Automated documentation checks.** Validate local links, command
  examples, and configuration/memory-map consistency in CI. Accept when a broken
  link or stale documented constant produces a targeted failure
  ([testing](docs/testing.md#known-verification-gaps)).

## Product Extensions

Future features. None is started; each needs its decision first where one is
listed. Context: [features](docs/features.md#current-technical-boundaries).

- [ ] **P2** **ADR: bootloader, flash partitioning, and signed DFU.** Choose a
  bootloader, the flash partition layout (no bootloader or DFU region is
  allocated today), image signing, secure boot, and anti-rollback. Accept when
  the ADR is Accepted and listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
- [ ] **P2** **Signed USB/BLE DFU.** *(hardware)* Choose a bootloader and flash
  partition layout; implement signed image validation, rollback policy,
  interrupted-update recovery, and physical recovery. Accept only after
  power-cut and invalid-image tests
  ([security](docs/security.md#firmware-integrity-and-updates)).
- [ ] **P2** **ADR: multiple BLE profile sets.** Decide profile selection,
  storage layout and migration, and which slots a profile owns. Accept when the
  ADR is Accepted and listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
- [ ] **P2** **Multiple BLE profile sets.** *(hardware)* Design selection,
  storage, migration, and connection ownership. Accept when switching profiles
  releases old inputs and only reconnects the selected profile's authorized
  peers.
- [ ] **P2** **Monitor-input-aware switching.** *(hardware)* Identify an
  explicit supported signal from the monitor/host, define fallback behavior, and
  prototype against named hardware. Accept when input reaches the intended PC
  without leaking held keys during a switch.
- [ ] **P2** **Windows/macOS companion app.** *(hardware)* Define a versioned,
  authenticated management protocol and installation/update policy before
  implementing the tray UI. Accept when settings, diagnostics, access control,
  and firmware compatibility are tested end to end.
- [ ] **P2** **Additional MCU/board targets.** *(hardware)* Select a concrete
  target, isolate board configuration, and implement its radio/USB/storage
  integration. Accept when it has a maintained build and hardware acceptance
  record; alternatives listed in
  [hardware](docs/hardware.md#possible-future-ports) are not supported ports
  today.

## Updating This Checklist

- Check an item only when its acceptance criterion is met with evidence. Add
  the implementing paths to the item when you check it.
- Record the evidence in the [testing guide](docs/testing.md) validation record
  or in a [hardware-result issue](.github/ISSUE_TEMPLATE/hardware-result.md),
  and name the commit and artifact hash it covers. Record skipped tests as
  skipped, not passed.
- Describe a new capability in [features](docs/features.md), and update the
  guide that owns its details.
- Add an ADR, or supersede one, when a decision changed; see the
  [ADR process](docs/architecture.md#adr-process).
- A hardware gate stays unchecked until board evidence exists, even when the
  supporting code is merged. When part of an item is done, split it: check the
  finished part and leave the rest open with its own acceptance criterion.
- Remove *(hardware evidence pending)* from a done item only when a recorded
  first-flash or acceptance run covers it.
