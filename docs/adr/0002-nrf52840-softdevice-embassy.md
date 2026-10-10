# ADR 0002: Build On nRF52840, Nordic SoftDevice S140, And Embassy

- Status: Accepted
- Date: 2026-02-21

## Context

The bridge has to do several things at once on one small device:

- act as a BLE central to HID-over-GATT keyboards and mice, with two
  simultaneous links and bonding, so peripherals reconnect after a reboot
  without pairing again
- act as a USB HID device with keyboard, mouse, and consumer-control
  interfaces, so the PC needs no Bluetooth stack, driver, or program
- follow the USB host through reset, suspend, and resume while keeping BLE
  links up
- drive an SSD1306 OLED over I2C and read three buttons
- be buildable by one person from a development kit that is easy to buy
- share its decision logic with host tests
  ([ADR 0003](0003-pure-core-and-task-shell.md)), and parse peer-controlled
  data in memory-safe code

The repository did not start as firmware. From its first code on 2024-12-07
(`0bf0446`, "Basic converter") to 2025-05-28 (`fe5662f`) it was a host-side
Rust program built on `btleplug`, `hidapi`, and `tokio`, later with a
`crossterm` terminal UI, that connected to a peripheral and subscribed to its
HID input characteristic from the PC itself. That approach needs a Bluetooth
adapter and a running program on every host, and it cannot provide input before
the operating system and that program have started.

The embedded implementation replaced it on 2026-02-21 (`8e6dd17`, "Full
implementation"). That commit introduced the nRF52840 target, Nordic SoftDevice
S140 v7.3.0 through the `nrf-softdevice` bindings, Embassy, `no_std` with static
allocation, `defmt`, and `probe-rs`. Its README compared the alternatives under
"Alternative MCU Targets" and "Why Embassy Instead of an RTOS?"; this record
keeps that reasoning. On 2026-06-23 (`dc11b4a`) the Embassy crates were
upgraded from executor 0.7, nrf 0.3, time 0.4, and usb 0.4 to the current 0.10,
0.7, 0.5, and 0.6, without changing the decision.

Facts the decision has to live with, as configured today:

| Fact | Value | Source |
| --- | --- | --- |
| nRF52840 memory | 1 MiB flash, 256 KiB RAM | `memory_sd.x` header |
| S140 v7.3.0 flash | `0x00000000–0x00027000` (156 KiB) | `memory_sd.x` |
| S140 RAM reservation | `0x20000000–0x20006000` (24 KiB), not yet measured on a board | `memory_sd.x` |
| BLE roles | 2 central links, 2 central security contexts, no advertising, no peripheral role | `softdevice_config()` in `src/sd_setup.rs` |
| ATT MTU | 64 bytes | `softdevice_config()` |
| Connection event length | 6 units (7.5 ms) so two links interleave | `BLE_CONN_EVENT_LENGTH` in `src/config.rs` |
| Low-frequency clock | Internal RC oscillator, 500 ppm accuracy | `softdevice_config()` |
| Reserved interrupt priorities | 0, 1, and 4 belong to the SoftDevice | comment and settings in `src/main.rs` |

## Decision

- **Target.** Build for the nRF52840 (`thumbv7em-none-eabihf`), with the
  nRF52840-DK pin mapping as the reference board. Other boards and MCUs are not
  supported ports.
- **BLE stack.** Use Nordic SoftDevice S140 v7.3.0, flashed separately from the
  application, in the central role only. Reach it through the `nrf-softdevice`
  and `nrf-softdevice-s140` crates at a pinned revision with the
  `ble-central`, `ble-gatt-client`, `ble-sec`, and `critical-section-impl`
  features. The revision and its small patch are
  [ADR 0007](0007-vendored-softdevice-patch.md).
- **Runtime.** Write `#![no_std]`, `#![no_main]` Rust on Embassy's thread-mode
  executor for Cortex-M (`embassy-executor` with `platform-cortex-m` and
  `executor-thread`), with `embassy-nrf` as the HAL, `embassy-time` on the RTC1
  time driver, `embassy-usb` for the composite device, and `embassy-sync`
  channels, signals, and mutexes over `CriticalSectionRawMutex`. All tasks run
  cooperatively on one stack.
- **Memory.** Allocate statically only: no global allocator, bounded `heapless`
  collections, `StaticCell` for one-time initialization, and fixed-capacity
  channels. The memory map is [ADR 0010](0010-static-memory-layout.md).
- **Platform rules imposed by the SoftDevice.** Application interrupts run at
  priority 2; the SoftDevice owns the POWER peripheral, so USB VBUS state
  arrives as SoftDevice SoC events; flash is written only through
  `nrf_softdevice::Flash`, which shares radio timeslots.
- **Diagnostics and flashing.** Log with `defmt` over RTT, report panics with
  `panic-probe`, and flash and run with `probe-rs`. Local builds log at `debug`
  (`DEFMT_LOG` in `.cargo/config.toml`); CI builds, including the release
  artifacts, log at `info`.

## Alternatives Considered

- **Keep the host-side program.** It was the project's first form (2024–2025).
  It needs Bluetooth and a running program on every PC, cannot serve a BIOS,
  UEFI, or a freshly booted system, and does not let one keyboard follow a
  monitor's USB hub between computers. The firmware approach was chosen to
  remove all of that from the host.
- **ESP32-S3.** It also has an on-chip BLE radio and USB OTG, and the
  2026-02-21 comparison rated it a "strong alternative". It would mean a
  different BLE stack, HAL, and USB driver, with none of the existing
  integration reused. It remains a design idea in
  [hardware](../hardware.md#possible-future-ports).
- **RP2040 or STM32 with an external BLE module.** Both have Embassy HALs
  (`embassy-rp`, `embassy-stm32`), but the radio would sit behind a link to a
  second chip running its own firmware. The 2026-02-21 comparison rated them
  "higher integration work" and "flexible but more complex".
- **Zephyr or the nRF Connect SDK.** This is Nordic's main C path, with a
  mature BLE host and USB HID support. It was not chosen because the firmware
  would be C (or Rust bindings over C), and the decision logic could no longer
  be the same `no_std` modules that the host tests exercise.
- **RTIC instead of Embassy.** RTIC schedules work by interrupt priority with
  compile-time resource locking and can also run async tasks. bt2usb's HAL, USB
  stack, time driver, and SoftDevice bindings come from the Embassy ecosystem
  and are used with Embassy's executor, so RTIC would add a second concurrency
  model without removing any dependency.
- **Nordic's SoftDevice Controller (`nrf-sdc`) with the pure-Rust `trouble`
  host.** This links Nordic's controller library into the application instead
  of flashing a separate SoftDevice, and would remove the vendored
  `nrf-softdevice` patch. It would also replace the whole BLE layer (scanner,
  connection workers, GATT HID client, bonder). bt2usb has not evaluated it
  against its own needs: two central links, bonding with identity resolution,
  and long GATT reads from real peripherals. Revisit it if S140 blocks a
  roadmap item, such as a DFU flash layout, or if the patch becomes costly to
  carry.
- **A heap allocator.** An allocator would make variable-size data easier, but
  it adds fragmentation and allocation failure to firmware paths. The data set
  is small and bounded, so static allocation was preferred, and adding a heap
  now needs its own ADR.

## Rationale

- **One chip for both ends.** The nRF52840 has the 2.4 GHz radio and a USB
  device controller on the same die, so there is no second chip, host
  interface, or second firmware to keep in step. The development kit adds an
  on-board debugger, a native USB connector, and on-board buttons on P0.11,
  P0.12, and P0.24, the pins bt2usb uses for UP, DOWN, and SELECT.
- **The most hardware-dependent part uses the most proven component.** S140 is
  Nordic's own precompiled protocol stack for this chip. It provides the
  central role, several simultaneous links, and bonding with encryption and
  identity resolution, and it owns radio timing so application code cannot
  break it. Interoperability with arbitrary peripherals is the biggest risk in
  this product, so it should rest on the vendor stack rather than on new code.
- **Async Rust fits the workload.** The firmware mostly waits: on two BLE
  links, three USB endpoints, USB bus events, three buttons, the display, a
  housekeeping ticker, and SoftDevice events. Embassy expresses each wait as an
  `.await` in a task on one stack, without per-task stacks or an RTOS kernel.
  `nrf-softdevice` is maintained in the embassy-rs organization and exposes the
  SoftDevice as async functions, and the HAL, USB stack, and time driver come
  from the same project, so every layer shares one execution model.
- **Shared, memory-safe logic.** `no_std` Rust lets the same modules compile for
  the board and for host tests, and the compiler rules out the buffer and
  lifetime mistakes that matter most when parsing data from any peripheral in
  radio range.
- **Visible worst cases.** Static allocation puts every buffer and queue in the
  link map, removes allocator failure from firmware paths, and leaves the stack
  high-water mark (`src/stack.rs`) as the main runtime memory measurement.
- **Small diagnostics.** `defmt` keeps format strings off the device, and one
  `probe-rs` command flashes, runs, and decodes RTT.

## Consequences

Positive:

- BLE timing, link management, and bonding come from the vendor stack, and the
  application handles only GATT, policy, and data.
- One executor and one stack keep concurrency explicit, and the stack
  high-water mark is a single number to watch.
- The pure modules compile for both the ARM target and the host.

Negative:

- The SoftDevice is a separate binary that is not in the repository or in a
  release. It is downloaded from Nordic (`mask softdevice` does this without a
  digest check), flashed once per board, and must be reinstalled after a
  full-chip erase, which also destroys the stored bonds.
- Flash and RAM below the application belong to the SoftDevice. The 24 KiB RAM
  reservation is a comment-level estimate ("generous for 2 central connections
  with MTU 64") until it is measured on a board.
- The SoftDevice constrains the rest of the firmware: interrupt priorities 0,
  1, and 4 are off limits, the POWER peripheral is not available to the
  application, flash writes contend with the radio and need retries, and
  linkers that move `.data` (such as flip-link) break the RAM boundary.
- Blocking work in any task delays every task, because the executor is
  cooperative and single-threaded.
- The SoftDevice-free simulation needs its own critical-section implementation
  (`cortex-m/critical-section-single-core`) and memory map (`memory_sim.x`),
  so the `embedded` and `sim` features are mutually exclusive.
- The BLE layer depends on `nrf-softdevice`, a git dependency that bt2usb pins
  and patches ([ADR 0007](0007-vendored-softdevice-patch.md)).

Follow-up obligations, tracked in [TODO.md](../../TODO.md):

- "Memory and endurance budget": record the measured SoftDevice RAM and stack
  high-water on a board before changing `memory_sd.x`.
- "Supply-chain and tooling maintenance": verify the SoftDevice download by
  digest.
- "Additional MCU/board targets": any other MCU is a new port with its own
  radio, USB, storage, memory, and acceptance record.
- "Signed USB/BLE DFU": choosing a bootloader must fit around the SoftDevice's
  fixed flash region.

## Implementation

| Concern | Where |
| --- | --- |
| Target, features, dependencies | `Cargo.toml` (`embedded` feature, Embassy and `nrf-softdevice` dependencies); `rust-toolchain.toml` installs `thumbv7em-none-eabihf` |
| Runner and link flags | `.cargo/config.toml`: `probe-rs run --chip nRF52840_xxAA`, `-Tlink.x`, `-Tdefmt.x`, `--nmagic`, and `DEFMT_LOG = "debug"`; CI sets `DEFMT_LOG: info` in [ci.yml](../../.github/workflows/ci.yml) |
| Memory map | [memory_sd.x](../../memory_sd.x), selected by [build.rs](../../build.rs); the `ASSERT` keeps `.data` at the RAM origin because nrf-softdevice reports `__sdata` to the SoftDevice as the application RAM base |
| SoftDevice configuration | `softdevice_config()` in [sd_setup.rs](../../src/sd_setup.rs), shared by the bridge and the self-test |
| USB power events | `enable_usb_power_events()` in `sd_setup.rs`; `softdevice_task` in [main.rs](../../src/main.rs) forwards `PowerUsbDetected`, `PowerUsbRemoved`, and `PowerUsbPowerReady` to the `SoftwareVbusDetect` used by [hid_device.rs](../../src/usb/hid_device.rs) |
| Interrupt priorities | `main.rs` and `selftest.rs` set GPIOTE and the time driver through `embassy_nrf::config::Config`, then USBD and TWISPI0 explicitly, all to `Priority::P2` |
| Tasks | `main.rs` spawns `softdevice_task`, `usb_device_task`, `hid_writer_task`, one `ble_slot_task` per link, `ble_task`, `display_task`, and three button tasks; the UI loop runs in `main` |
| Flash access | `nrf_softdevice::Flash::take(sd)` in `ble_task` ([multi_conn.rs](../../src/ble/multi_conn.rs)) and in the self-test |
| Build profiles | Release: `opt-level = "s"`, fat LTO, one codegen unit, `debug = 2` for probe-rs; dev: `opt-level = 1`, which the manifest notes is required by SoftDevice timing |
| Stack measurement | `cortex-m-rt/paint-stack` and `high_water()` in [stack.rs](../../src/stack.rs), logged as `stack high-water: {} of {} bytes` |

At boot the bridge logs `bt2usb firmware starting`, then nrf-softdevice prints
`softdevice RAM: N bytes` (format string `softdevice RAM: {:?} bytes`),
`enable_usb_power_events()` logs `USB power: vbus={} ready={}`, and
`hid_device::init` logs
`USB HID composite device initialised (keyboard + mouse + consumer)`. `main`
then logs `SoftDevice started`, `USB HID device started`, `BLE task started`,
and `UI and isolated OLED tasks started`; the spawned tasks add their own
start-up lines. If the reservation in `memory_sd.x` is too small,
`Softdevice::enable` panics with
`too little RAM for softdevice. Change your app's RAM start address to ...`;
[first flash](../first-flash.md#2-self-test-image) explains the fix.

### Verification Status

- **Implemented:** everything in the table above.
- **Software-verified:** the CI "Embedded build & clippy" job lints the
  `embedded` feature with warnings denied and builds the release bridge and
  self-test for `thumbv7em-none-eabihf` against `memory_sd.x`. It passed on
  GitHub-hosted runners in push runs 36441995385 (`8a04b25`, 2026-09-28) and
  37932436721 (`7fc99d6`, 2026-10-09) and scheduled run 37338711407
  (2026-10-05). Builds do not run the SoftDevice: CI has no board, and the
  Renode simulation is SoftDevice-free
  ([ADR 0014](0014-renode-gpio-models.md)).
- **Hardware-verified:** not yet. The repository holds no board record of the
  boot log, the `softdevice RAM: N bytes` value, USB power events, or the
  interrupt-priority setup; [first flash](../first-flash.md) asks for them.

## Related

- [Architecture: system at a glance](../architecture.md#system-at-a-glance)
- [Hardware: memory layout](../hardware.md#memory-layout)
- [First flash: SoftDevice](../first-flash.md#1-softdevice-once-per-board)
- [ADR 0003: Hardware-free decision modules](0003-pure-core-and-task-shell.md)
- [ADR 0007: Vendored nrf-softdevice patch](0007-vendored-softdevice-patch.md)
- [ADR 0010: Static memory layout](0010-static-memory-layout.md)
- [ADR 0011: Interim Just Works pairing](0011-interim-just-works-pairing.md)
- [ADR 0012: Bus-powered, no System-OFF](0012-bus-powered-no-system-off.md)
- [ADR 0013: Pinned toolchain and mask tasks](0013-pinned-toolchain-and-mask-tasks.md)
