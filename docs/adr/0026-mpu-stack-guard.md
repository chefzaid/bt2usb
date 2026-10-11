# ADR 0026: Guard The Bottom Of The Stack With An MPU Region

- Status: Accepted
- Date: 2026-10-11

## Context

The bridge runs every Embassy task, every interrupt handler, and the
SoftDevice's own handlers on one main stack. That stack grows down from the
top of RAM (`_stack_start`, `0x20040000`) towards the end of the statics
(`_stack_end`), with `.data`, `.bss`, and `.uninit` directly below it
([ADR 0010](0010-static-memory-layout.md)). An overflow therefore writes into
the statics: the bond table, the device store's RAM copy, the channels between
tasks, the defmt RTT buffer, and the executor's task storage. Nothing faults.
The bridge keeps running on corrupted state and fails later somewhere
unrelated, or never visibly, which is the hardest kind of defect to report.

The firmware used to link through flip-link, which moves the stack below the
statics so that an overflow runs off the bottom of RAM and faults. ADR 0010
removed it: nrf-softdevice reports `__sdata`, the start of `.data`, to the
SoftDevice as the application's RAM base, and with flip-link `.data` moved so
high that the SoftDevice claimed, and write-protected, almost all of RAM,
including the stack. `memory_sd.x` now asserts that `.data` starts at the RAM
origin and the stack ends at the top, so flip-link cannot come back. ADR 0010
lists the lost overflow fault as a negative consequence, and the painted-stack
high-water mark (`stack high-water: X of Y bytes`) only shows after the fact
how deep the stack went, if a probe was attached to read it.

The P1 item "Stack overflow detection" in
[TODO.md](../../TODO.md#platform-memory-and-recovery) asked for a guard that
works with the SoftDevice's RAM layout, accepted "when a deliberate overflow in
a test build faults with a diagnosable message instead of corrupting memory".

Facts that shaped the decision:

- **The core has an MPU nobody uses.** The Cortex-M4F in the nRF52840 has an
  ARMv7-M PMSA memory protection unit with eight regions. No crate in the build
  programs it: cortex-m-rt, embassy-nrf, embassy-executor, embassy-usb,
  defmt-rtt, panic-probe, and the vendored nrf-softdevice do not touch it
  (checked by searching their sources for the MPU's registers), and the S140 specification protects the
  SoftDevice's RAM and peripherals with the Memory Watch Unit
  ([Memory isolation and runtime protection](https://docs.nordicsemi.com/r/HUqmNFbWu_oifN3fS6YR4w/Fgj3EB18~O_upis7jDsPkA),
  S140 SoftDevice Specification) and does not mention the MPU. That the
  SoftDevice leaves the MPU alone at run time is inferred from the
  specification, not measured, which is why the self-test reads the region
  back (see Decision).
- **An MPU region's base must be a multiple of its size**, which must be a
  power of two of at least 32 bytes. `_stack_end` moves with every change to
  the statics, so the region cannot simply start there.
- **Stacking a fault's frame into a no-access region faults too.** On ARMv7-M
  the exception entry pushes eight words (26 with the FPU's lazy context) below
  the stack pointer using the interrupted code's privilege and priority, so
  when the stack pointer is already at the guard the push itself violates the
  region: `CFSR.MSTKERR` is set, the stack pointer still moves down, and the
  frame holds whatever the guard held before. The faulting PC is lost; the
  stack pointer is not.
- **The MPU can be off inside HardFault.** With `MPU_CTRL.HFNMIENA` clear, the
  MPU is disabled while the core runs at priority -1 or -2 (HardFault, NMI,
  or with `FAULTMASK` set). The MemManage exception is disabled at reset
  (`SHCSR.MEMFAULTENA` clear), so an MPU violation escalates to HardFault, and
  that handler can use the guard's bytes as its stack.
- **Frames are under 4 KiB.** A scan of the release images' disassembly for
  stack adjustments (`sub sp, #n`, `sub.w sp, sp, #n`, and `subw sp, sp, #n`;
  none subtracts a register) finds the largest single frame at 2,668 bytes in
  the bridge, the display task's poll, and 3,412 bytes in the self-test, its
  main task's poll; DWARF names both. A guard of 4 KiB therefore cannot be
  stepped over by one firmware frame: every byte of a frame that starts above
  the guard lies above the guard's base. The SoftDevice's handlers run on the
  same stack; the scan cannot see their frames.
- **Renode 1.16.1 models the PMSAv7 MPU but not every fault rule.** In the
  simulation, an MPU violation enters the MemManage handler even with the
  exception disabled, the stack pointer does not move for a frame that could
  not be stacked, and `EXC_RETURN` in `LR` then names the process stack, which
  the firmware never uses.

## Decision

**Place a no-access MPU region at the bottom of the stack and report a fault
taken with the stack pointer in or at it as a stack overflow.**

- **Placement.** `StackGuard::place` in the pure
  [stack_logic.rs](../../src/stack_logic.rs) puts the guard at the first
  address at or above `_stack_end` that is a multiple of `STACK_GUARD_BYTES`
  (4096, [config.rs](../../src/config.rs)), which also makes it a valid MPU
  region. The bytes between `_stack_end` and the guard (up to 4 KiB - 1) are
  never used: the stack hits the guard first.
- **Registers.** Region 0 covers the guard with `MPU_RBAR` = its base and
  `MPU_RASR` = execute-never, access permission 0 (no access at any privilege
  level), normal write-back memory (`TEX` 0, `C` 1, `B` 1, as the default map
  treats SRAM), `SIZE` = log2(size) - 1, enabled: `0x1003_0017` for 4 KiB.
  `MPU_CTRL` = `ENABLE | PRIVDEFENA` (`0b101`): the default memory map for every
  other address, so no access the firmware or the SoftDevice makes elsewhere
  changes meaning, and `HFNMIENA` clear, so the MPU is off in HardFault.
- **Enable early.** `stack::enable_guard` checks that the stack pointer is
  above the guard and that the MPU reports regions, then programs region 0 with
  the MPU off and turns it on, followed by `dsb` and `isb`. The bridge calls it
  first thing in `main`, before the reset-reason read and before the SoftDevice
  starts, and logs `stack guard: 4096 bytes at 0x2000d000..0x2000e000` (the
  addresses of the current release build), or `stack guard off: <reason>`
  (`BadSize`, `StackInUse`, `NoMpu`) and carries on unguarded. The self-test
  and the simulation do the same.
- **Report.** `stack.rs` replaces cortex-m-rt's default HardFault handler. When
  the stack pointer at the fault is at or below the guard's top
  (`!StackGuard::below`), it logs
  `stack overflow: stack pointer 0x2000dfc8, guard 0x2000d000..0x2000e000,
  PC not stacked`. It names the PC only when `CFSR.MSTKERR` is clear; a real
  overflow always sets it, because the frame could not be stacked. Then, as the default handler did, the core
  spins until it is reset. Any other HardFault, including the one panic-probe
  raises after printing a panic, also spins as before.
- **Keep the report across a reset** (added the same day, after the logging
  failure under Consequences was found). Before it logs, the handler stores
  the report as an `OverflowRecord` ([stack_logic.rs](../../src/stack_logic.rs)):
  six words, a tag that also says whether a PC is present, the stack
  pointer, the guard's bounds, the PC, and a check word, in a static in
  `.uninit`, the RAM the reset handler neither zeroes nor paints. Every image
  calls `take_previous_overflow` at boot, right after the guard line, which
  clears the record and, if it decodes, logs
  `previous boot: stack overflow: stack pointer ..., guard ..., PC ...` at
  warning level. The nRF52 product specifications say RAM is never reset,
  though some reset sources can corrupt it, so the record survives a lock-up,
  a soft reset, and the reset button; the check word rejects the RAM's
  power-on contents and a corrupted record, and a power cycle loses it. The
  words are atomics, so storing and reading them needs no `unsafe`.
- **MemManage.** A stackless MemManage handler, written in assembly in
  `stack.rs`, turns the MPU off and calls the same HardFault handler with the
  handler's stack pointer as the frame. The firmware never enables MemManage,
  so on the board it runs only if something else does; in Renode it is the
  path every overflow takes.
- **Simulation output.** The simulation reads its log from UART0, whose
  driver task cannot run again after a fault, so its report is written to
  the UARTE registers directly (`uart_write_blocking` in
  [sim.rs](../../src/sim.rs)).
- **High-water mark.** `stack::high_water` now scans from the guard's top,
  since reading the guard faults while it is on, and reports the stack above
  the guard as the total.
- **Evidence.** The Renode scenario moves the stack pointer 256 bytes into the
  guard while the core sleeps and expects the overflow line and a halted core,
  then resets the machine, which keeps RAM, and expects the next boot's
  `previous boot: stack overflow` line with the same guard.
  The self-test reads the MPU registers back after its SoftDevice, USB, and BLE
  stages (`[PASS] stack guard: ...`), and, if SELECT is held within 10 s at the
  end, recurses until the guard faults so a tester can see the board's report.

## Alternatives Considered

- **Bring flip-link back.** It gives the same fault for free, but it moves
  `.data` up and makes nrf-softdevice report that address as the
  application's RAM base, so the SoftDevice would claim and write-protect the
  stack. Changing the vendored crate to report the configured RAM origin
  instead would work, but would add a patch to the SoftDevice's memory
  contract to save one MPU region, and flip-link reorders the image in a way
  every linker-script check would have to follow. Rejected.
- **Move the stack below the statics by hand in `memory_sd.x`.** That is what
  flip-link does, and it breaks the same `__sdata` assumption. Rejected.
- **Software canaries.** A known word at `_stack_end`, checked periodically or
  in the idle loop, finds an overflow only after the statics below it were
  already overwritten, and finds none that skips the canary or crashes first.
  The painted-stack high-water mark already gives that after-the-fact view.
  Rejected as the detection; kept as the measurement.
- **A stack limit register.** ARMv8-M has `MSPLIM`; the Cortex-M4 does not.
  Not available.
- **Enable MemManage and report from its handler.** Its handler runs at
  priority 0 with the MPU still on and the stack pointer in the guard, so any
  push it makes faults again and escalates to HardFault anyway. Rejected for
  the board; the stackless MemManage handler exists only to reach the HardFault
  handler on that path.
- **A guard of 32 bytes, the smallest region.** It costs nothing but is
  stepped over by any frame larger than what is left above it, and a fault
  handler running inside 32 bytes would overwrite the statics below. 4 KiB
  is larger than any firmware frame found, gives the handler room to run
  inside the guard, and costs at most 8 KiB with the alignment gap out of about
  200 KiB of stack.
- **Watchdog only.** A watchdog would reset a hung bridge but not say why, and
  a corrupted bridge may not hang. It is a separate decision
  ([ADR 0020](0020-watchdog-and-progress-based-recovery.md), Proposed) that
  this one complements.

## Rationale

The guard turns silent corruption into an immediate fault with a log line
that says what happened, at the cost of one MPU region and at most 8 KiB of
RAM that the stack could only have used by already overflowing the measured
high-water mark many times over. It changes nothing in the SoftDevice's RAM
contract, the linker script, or the vendored crate. Keeping the placement and
register values in a pure module lets the host tests pin them, including the
value read back on the board, and the simulation exercises the fault path on
the same handler code the board runs.

## Consequences

Positive:

- An overflow now stops the bridge with
  `stack overflow: stack pointer ..., guard ..., PC not stacked` instead of
  corrupting statics; the statics below the guard are intact when it stops.
- The self-test shows on each board that the region is still programmed after
  the SoftDevice has run, and can show a real overflow on demand.
- `stack high-water: X of Y bytes` reports the usable stack (above the guard)
  as Y.
- An overflow on a board with no probe attached, or one whose log line was
  lost, is still reported: the first boot after a reset that keeps RAM logs
  `previous boot: stack overflow: ...`.

Negative:

- The PC of the overflowing code is lost: the frame could not be stacked. The
  stack pointer and the guard's address are all the line carries; the
  high-water mark and the log before the fault narrow it down.
- A frame larger than the guard plus the stack left above it can step over the
  guard into the statics without faulting. The self-test's main task frame is
  684 bytes under the guard's size and the bridge's largest 1,428 bytes under
  it, but nothing checks that a change keeps every frame under 4 KiB, and the
  SoftDevice's frames are not visible.
- The HardFault handler runs on the guard's bytes with the MPU off. If the
  stack pointer went deeper than the handler's own frame (about 300 bytes in
  the simulation, less with defmt) can absorb, the handler's frame lands in
  the statics just below the guard, after the fault was already taken.
- The bridge stops at an overflow and stays stopped until reset, like any
  other fault, until a watchdog exists.
- An overflow that happens while a `defmt` line is being written cannot log
  at once: the handler's own log call finds the logger taken, defmt-rtt
  panics, panic-probe's `udf` faults inside the HardFault handler, and the
  core locks up, which the nRF52840 turns into a reset. The statics are still
  intact, and the next boot logs the stored
  `previous boot: stack overflow: ...` line, then `reset reason: CPU lock-up`.
- After a power cycle, which on this bus-powered board is what unplugging it
  does, the record is gone: an overflow that was never logged leaves no trace.
- The record takes 24 bytes of `.uninit`, which moves `_stack_end` up by as
  much; the guard stays at the same 4 KiB boundary while the gap allows.
- A debug session that catches HardFaults does not show the line. `probe-rs run`,
  the Cargo runner, sets the core's HardFault vector catch by default and
  halts at the handler's first instruction, printing
  `Firmware exited unexpectedly: Exception` and a backtrace that starts from
  the frame the overflow could not stack. `mask selftest` passes
  `--no-catch-hardfault`, so the deliberate overflow logs the firmware's line.
  `mask run` keeps the catch, because it gives a panic its backtrace; the
  [operations runbook](../operations.md#the-stack-overflows) gives the command
  that reproduces an overflow with the catch off.
- Up to 8 KiB of RAM (the guard and its alignment gap) is unavailable to the
  stack.
- Renode's fault model differs from the core's in three ways (see Context);
  the Renode test proves the guard faults and the report path works, not the
  core's exact exception sequence, which only the self-test's deliberate
  overflow shows.

## Implementation

| Concern | Where |
| --- | --- |
| Placement and register values | `StackGuard` and `MPU_CTRL` in [stack_logic.rs](../../src/stack_logic.rs), with six host tests |
| Report kept across a reset | `OverflowRecord` in [stack_logic.rs](../../src/stack_logic.rs), with five host tests; `OVERFLOW_RECORD`, `take_previous_overflow`, `log_previous_overflow`, and `describe_overflow` (simulation) in [stack.rs](../../src/stack.rs) |
| Size | `STACK_GUARD_BYTES` in [config.rs](../../src/config.rs) |
| Programming, read-back, high-water | `enable_guard`, `enable_guard_logged`, `guard_is_on`, and `high_water` in [stack.rs](../../src/stack.rs) |
| Fault report | `HardFault`, the assembly `MemoryManagement` handler, and `report_overflow` in [stack.rs](../../src/stack.rs) |
| Callers | `main` in [main.rs](../../src/main.rs), [selftest.rs](../../src/selftest.rs) (with stage 7's read-back and the optional `offer_overflow`), and [sim.rs](../../src/sim.rs) with `uart_write_blocking` |
| Debug session | `mask selftest` in [maskfile.md](../../maskfile.md) passes `--no-catch-hardfault` to `probe-rs run` |
| Renode test | "A Stack Overflow Faults In The Guard And Is Reported" in [bt2usb-sim.robot](../../renode/bt2usb-sim.robot) |

### Verification Status

- **Implemented:** everything above, on 2026-10-11.
- **Software-verified:** the eleven `stack_logic` host tests pin the
  placement, the register values, the read-back comparison, and the
  record's layout, round trip, and rejection of a cleared, corrupted, or
  swapped record. The Renode scenario checks the boot line, that a stack
  pointer moved into the guard ends in the `stack overflow` line and a
  halted core, and that the boot after a machine reset logs the record;
  with the MPU left off it fails (checked by a run with `MPU_CTRL` written
  as 0, in which the simulation crashed on the corrupted stack instead), and
  with the record's store removed it fails at the `previous boot` line.
  Clippy, rustdoc, and the release builds pass for every binary.
- **Hardware-verified:** not yet. The self-test's `stack guard` stage, its
  optional deliberate overflow, and the `previous boot` line after the reset
  button, recorded through the
  [first-flash checklist](../first-flash.md#2-self-test-image), are the evidence
  still to collect. That the nRF52840 keeps the record through a lock-up and
  the reset button rests on the product specification, not on a board run.

## Related

- [ADR 0010: Fix The Memory Map In The Linker Script And Assert It](0010-static-memory-layout.md)
- [ADR 0020: Watchdog And Progress-Based Recovery](0020-watchdog-and-progress-based-recovery.md)
- [ADR 0025: Deny Panic-Prone Constructs With Clippy And List The Ones It Cannot See](0025-panic-lints-and-inventory.md)
- [Operations: stack and memory checks](../operations.md#stack-and-memory-checks)
- [Hardware: memory layout](../hardware.md#memory-layout)
- [Code quality: binary size and memory budgets](../code-quality.md#binary-size-and-memory-budgets)
