# ADR 0010: Fix The Memory Map In The Linker Script And Assert It

- Status: Accepted
- Date: 2026-09-26

This record was written retroactively on 2026-10-09 from the source and the
commit history. The current SoftDevice layout, its linker assertion, the
removal of flip-link, and the stack measurement landed in `f477d4c` on
2026-09-26. The separate simulation layout dates from `e3bc620` on 2026-06-22,
and the build-script guard against combining the `embedded` and `sim`
features from `2479c79` on 2026-09-28.

## Context

Three parties share the nRF52840's 1 MiB of flash and 256 KiB of RAM: the
Nordic SoftDevice S140 v7.3.0, the application, and the pairing store. Each
has hard requirements that the compiler cannot see.

- **The SoftDevice owns the bottom of both memories.** It occupies flash
  `0x00000000–0x00027000` and needs RAM from `0x20000000` up to an address that
  depends on its configuration (links, ATT MTU, roles). At enable time the
  application tells the SoftDevice where application RAM begins
  (`APP_RAM_BASE`). The pinned `nrf-softdevice` crate does not take that value
  as a parameter: `get_app_ram_base()` in
  [softdevice.rs](../../vendor/nrf-softdevice/src/softdevice.rs) returns the
  address of the linker symbol `__sdata`, the start of `.data`.
- **The pairing store is erased at runtime.** `sequential-storage` erases and
  rewrites flash pages 240–243 (`0x000F0000–0x000F4000`), the range that
  [config.rs](../../src/config.rs) defines and [storage.rs](../../src/storage.rs)
  turns into `STORAGE_START..STORAGE_END`. Until `f477d4c`, the linker's
  `FLASH` region ran to the end of the chip (`LENGTH = 868K`), so nothing
  stopped a growing image from placing code or read-only data on pages the
  store would erase.
- **flip-link broke the RAM boundary.** From the first embedded commit
  (`8e6dd17`, 2026-02-21) the ARM build linked through flip-link for
  stack-overflow protection. flip-link places the stack below `.data` and
  `.bss` and rewrites the RAM origin, which moves `__sdata` near the top of
  RAM. The bring-up commit records the result: "nrf-softdevice passes __sdata
  to the SoftDevice as APP_RAM_BASE, and flip-link moved it so high that the
  SoftDevice claimed nearly all RAM."
- **The simulation needs a different map.** The SoftDevice-free `sim` build
  for Renode has no SoftDevice, so its vector table and RAM must start at the
  bottom of each memory. Until `e3bc620`, the only layout was a `memory.x` in
  the crate root that [build.rs](../../build.rs) copied into `OUT_DIR`. The
  build script's header records that, while the simulation was being added,
  such a root `memory.x` shadowed the copy in `OUT_DIR`, because rust-lld
  resolves `INCLUDE memory.x` from the current directory first, and the
  simulation silently linked at the SoftDevice offset. No commit contains that
  broken state: `e3bc620` already ships the renamed `memory_sd.x` and
  `memory_sim.x`.
- **There is one stack.** Embassy runs every task on the single main stack,
  plus interrupt frames. Task futures live in static storage, so the stack
  holds only the polling call chain, but nothing reported how deep it got.

## Decision

Define the memory map once per build mode in a linker script, keep the
pairing pages outside the application's flash region, assert the RAM layout
that nrf-softdevice depends on, and measure the stack instead of relying on a
linker trick.

- **SoftDevice layout** in [memory_sd.x](../../memory_sd.x):

  | Region | Range, end exclusive | Size | Owner |
  | --- | --- | --- | --- |
  | SoftDevice flash | `0x00000000–0x00027000` | 156 KiB | S140 v7.3.0 |
  | `FLASH` | `0x00027000–0x000F0000` | 804 KiB | Application code and read-only data |
  | Pairing store | `0x000F0000–0x000F4000` | 16 KiB | `sequential-storage`, pages 240–243 |
  | Unallocated | `0x000F4000–0x00100000` | 48 KiB | Nothing |
  | SoftDevice RAM | `0x20000000–0x20006000` | 24 KiB | S140, reserved, not yet measured |
  | `RAM` | `0x20006000–0x20040000` | 232 KiB | `.data`, `.bss`, then the stack |

- **Pairing pages outside `FLASH`.** `FLASH` ends at `0x000F0000`, so an image
  that would reach the pairing pages fails to link instead of being placed on
  pages that the store erases.
- **One pairing range, checked by the linker** (2026-10-10). `config.rs`
  derives `STORAGE_FLASH_START` and `STORAGE_FLASH_END` from the page
  constants. `build.rs` compiles `config.rs` too and writes them ahead of the
  layout as `__bt2usb_storage_start` and `__bt2usb_storage_end`, and
  `memory_sd.x` asserts:

  ```text
  ASSERT(ORIGIN(FLASH) + LENGTH(FLASH) == __bt2usb_storage_start, …);
  ASSERT(__bt2usb_storage_end <= 0x00100000, …);
  ```

  Changing the pages or the `FLASH` length alone fails the link. The
  `FLASH` line keeps its literal length so the map stays readable.
- **Assert the RAM layout.** `memory_sd.x` ends with:

  ```text
  ASSERT(__sdata == ORIGIN(RAM) && _stack_start == ORIGIN(RAM) + LENGTH(RAM),
         "stack must sit at the top of RAM and .data at ORIGIN(RAM): the SoftDevice uses __sdata as APP_RAM_BASE (is flip-link in use?)");
  ```

  This keeps `APP_RAM_BASE` equal to the configured RAM origin and the stack at
  the top of RAM.
- **Plain rust-lld, no flip-link.** [.cargo/config.toml](../../.cargo/config.toml)
  links ARM builds with `-Tlink.x` (cortex-m-rt), `-Tdefmt.x`, and `--nmagic`,
  and explains in a comment why flip-link is not used.
- **A separate simulation layout.** [memory_sim.x](../../memory_sim.x) gives the
  `sim` build all of flash from `0x00000000` and all of RAM from `0x20000000`.
  [build.rs](../../build.rs) writes `memory_sim.x` when the `sim` feature is on
  and `memory_sd.x` otherwise to `OUT_DIR/memory.x`, after the two storage
  symbols, adds `OUT_DIR` to the link search path, and re-runs when either
  file, `src/config.rs`, or the `sim` feature changes. No
  file in the crate root is named `memory.x`. The build script also refuses
  to build with both features:
  ``features `embedded` and `sim` are mutually exclusive; build each separately``.
- **Change the SoftDevice RAM reservation only from a measurement.** The
  24 KiB reservation is the linker script's estimate ("generous for 2 central
  connections with MTU 64"). When `Softdevice::enable` runs, nrf-softdevice
  logs `softdevice RAM: {:?} bytes`. If the reservation is too small it panics
  with `too little RAM for softdevice. Change your app's RAM start address to {:x}`;
  if it is larger than needed it warns with the address that would be enough.
  Move `ORIGIN(RAM)` and `LENGTH(RAM)` together, and only from that logged
  value on a board.
- **Measure the stack.** The `embedded` feature enables cortex-m-rt's
  `paint-stack`, which fills `_stack_end.._stack_start` with `0xCCCC_CCCC` at
  reset. `high_water()` in [stack.rs](../../src/stack.rs) scans up from
  `_stack_end` to the first overwritten word and returns `(used, total)`. The
  bridge checks it on every 1-second housekeeping tick and logs
  `stack high-water: {} of {} bytes` whenever the value grows. The self-test
  logs the same line and passes its stack stage only when less than half the
  region has been used.

## Alternatives Considered

- **Keep flip-link and give the SoftDevice a different RAM base.** The pinned
  `nrf-softdevice` derives `APP_RAM_BASE` from `__sdata` with no override, so
  this would need another change to the vendored crate, beyond the reviewed
  GATT patch in [ADR 0007](0007-vendored-softdevice-patch.md). Keeping `.data`
  at the RAM origin needs no patch.
- **Let `FLASH` run to the end of the chip** (the layout before `f477d4c`).
  However large the image is today, an overlap would fail silently and
  destructively: the first sign would be firmware erased by the pairing store.
  A linker error costs nothing.
- **A single `memory.x` in the crate root.** That was the original setup
  (`8e6dd17`, before the simulation existed). According to the build script's
  header, keeping a root `memory.x` is what made the simulation link at the
  SoftDevice offset during development, because the linker found the root
  file before the copy in `OUT_DIR`.
- **Size the SoftDevice reservation from documentation or estimates alone.**
  The requirement depends on the exact configuration in
  [sd_setup.rs](../../src/sd_setup.rs), and nrf-softdevice reports it exactly at
  enable. The reservation stays at 24 KiB until a board measurement exists.

## Rationale

Every constraint here would otherwise fail at runtime, on a board, in a way
that is hard to diagnose: a SoftDevice memory fault, a stack the SoftDevice
treats as its own, or code erased by a flash write. A linker region and a
linker assertion turn each of those into a build failure with a message that
names the cause.

Dropping flip-link gives up an automatic stack-overflow fault, but the
alternative was a RAM layout the SoftDevice cannot work with. The painted-stack
measurement is the compensating control. Because Embassy keeps task state in
statics, worst-case stack use is the deepest poll chain plus interrupt frames.
A high-water mark records the deepest path exercised since reset, so it is only
as good as the workload that ran before it was read.

Selecting the layout in `build.rs` keeps one source file per mode and makes it
impossible for a stale or shadowing `memory.x` to decide which map is used.

## Consequences

Positive:

- Overlapping the pairing pages, changing the pairing range in `config.rs` or
  `memory_sd.x` alone, moving `.data` off the RAM origin, or moving the stack
  off the top of RAM fails the link with a specific message.
- The bridge and the self-test report stack use on every board, and the
  self-test turns it into a pass or fail.
- The simulation and the firmware cannot be linked with each other's map.

Negative:

- A stack overflow is not detected. The stack grows down toward `.bss`, and an
  overflow corrupts statics silently. No MPU guard region is configured. The
  high-water mark is evidence after the fact, not protection.
- The pairing boundary is still written twice, as page constants in
  `config.rs` and as the `FLASH` length in `memory_sd.x`, so a change edits
  both; the link fails until they agree. Generating the `FLASH` line instead
  would remove the second copy but leave `memory_sd.x` unreadable on its own.
  Update [hardware](../hardware.md#memory-layout) and the
  [data model](../data-model.md#pairing-store) with them.
- The SoftDevice RAM requirement and worst-case stack depth have not been
  measured on a board. If a configuration change (a third link, a larger ATT
  MTU) needs more RAM, `Softdevice::enable` panics at boot with the address to
  use.
- A full-chip erase removes the SoftDevice and the pairing store together.
- The 48 KiB at the top of flash is unallocated. A bootloader or DFU layout
  must be decided before anything uses it.

Follow-up obligations, tracked in [TODO.md](../../TODO.md):

- "Memory and endurance budget": measure the SoftDevice RAM at enable and the
  worst-case stack high-water mark with two links, scanning, display, and
  persistence, and record reviewed margins before changing `memory_sd.x`.
- "Stack overflow detection": evaluate a guard compatible with the SoftDevice
  RAM layout, so an overflow faults with a diagnosable message.
- "Automated documentation checks": catch disagreement between the storage
  constants, the linker map, and the documented memory map.
- "Signed USB/BLE DFU": choose a bootloader and flash partition layout around
  the regions above, and record it in a new ADR.

## Implementation

| Concern | Where |
| --- | --- |
| SoftDevice memory map, RAM assertion, and pairing-boundary assertions | [memory_sd.x](../../memory_sd.x) |
| Simulation memory map | [memory_sim.x](../../memory_sim.x) |
| Layout selection, storage symbols, and feature guard | [build.rs](../../build.rs) |
| Link arguments and the flip-link note | [.cargo/config.toml](../../.cargo/config.toml) |
| Pairing page constants | `FLASH_PAGE_SIZE`, `STORAGE_FLASH_PAGE_START`, `STORAGE_FLASH_PAGE_COUNT`, and the derived `STORAGE_FLASH_START` and `STORAGE_FLASH_END` in [config.rs](../../src/config.rs), used by [storage.rs](../../src/storage.rs), `selftest.rs`, and `build.rs` |
| `APP_RAM_BASE` source | `get_app_ram_base()` in [softdevice.rs](../../vendor/nrf-softdevice/src/softdevice.rs) |
| SoftDevice configuration that sets the RAM need | `softdevice_config()` in [sd_setup.rs](../../src/sd_setup.rs) |
| Stack painting | `cortex-m-rt/paint-stack` in the `embedded` feature of [Cargo.toml](../../Cargo.toml) |
| Stack measurement | `high_water()` in [stack.rs](../../src/stack.rs); logged from the housekeeping tick in [main.rs](../../src/main.rs) and stage 7 of [selftest.rs](../../src/selftest.rs) |
| Pairing-region round trip on a board | `check_flash` in `selftest.rs`, using `STORAGE_FLASH_START` and `STORAGE_FLASH_END` |

### Verification Status

- **Implemented:** both layouts, the assertion, the feature guard, and the
  stack measurement.
- **Software-verified:** every embedded build links against `memory_sd.x`
  (and so checks its regions and assertion), and every simulation build links
  against `memory_sim.x`. The
  [2026-09-28 validation record](../testing.md#validation-record--2026-09-28)
  reports the release bridge, self-test, and simulation builds passing
  locally, and the combined-feature build being rejected by the guard. On
  GitHub-hosted runners, the embedded and simulation jobs linked both layouts
  successfully in push runs 36441995385 (`8a04b25`, 2026-09-28) and
  37932436721 (`7fc99d6`, 2026-10-09) and scheduled run 37338711407
  (2026-10-05). No CI job builds the combined feature set, so the guard is
  checked only locally.
- **Hardware-verified:** not yet. The SoftDevice RAM value, the stack
  high-water mark, and the self-test flash stage have no board record in the
  repository; [first flash](../first-flash.md#2-self-test-image) asks for them.

## Related

- [Hardware: memory layout](../hardware.md#memory-layout)
- [Data model: pairing store](../data-model.md#pairing-store)
- [First flash: self-test image](../first-flash.md#2-self-test-image)
- [ADR 0002: nRF52840, SoftDevice S140, and Embassy](0002-nrf52840-softdevice-embassy.md)
- [ADR 0006: Fail-closed pairing store](0006-fail-closed-pairing-store.md)
- [ADR 0007: Vendored nrf-softdevice patch](0007-vendored-softdevice-patch.md)
- [ADR 0014: Renode GPIO models](0014-renode-gpio-models.md)
