# ADR 0020: Supervise Progress With Deadline Leases And Reset Through The Watchdog

- Status: Proposed
- Date: 2026-10-10

## Context

Nothing restarts the bridge today. No file under `src/` uses the nRF52840
watchdog timer (WDT) or reads the reset reason. All binaries link `panic-probe`
(`use panic_probe as _` in [main.rs](../../src/main.rs)), which in 1.0.0 masks
interrupts, prints over RTT and executes `udf`; with no `#[exception]` handler
that ends in `cortex-m-rt` 0.7.7's default `HardFault_`, a `loop {}`. A panic,
a SoftDevice assertion or a task that never finishes stops the unit until it is
unplugged ([not yet handled](../architecture.md#not-yet-handled)), and since
hosts keep a silent device's last report, a held key repeats on the PC
([operations](../operations.md#firmware-panics-or-stops-responding)), in
firmware setup or on whichever computer the KVM has selected.

All tasks share one cooperative executor
([executor and tasks](../architecture.md#executor-and-tasks)). Most waits are
unbounded because they wait for the outside world; a few have no application
deadline and are bounded only by the SoftDevice, the Core or the hardware:

| Task | Waits without bound for | Waits with no application deadline |
| --- | --- | --- |
| `main` UI loop ([main.rs](../../src/main.rs), the `loop` at the end of `main`) | Nothing: its `select4` includes the 1 s housekeeping `Ticker` (`housekeeping`) that runs `PowerManager::tick` | None |
| `ble_task` ([multi_conn.rs](../../src/ble/multi_conn.rs)) | A command or slot event (the `select` in its `loop`) | Store writes (`save_to_flash` in `execute_action`), Forget and Factory reset (`store.forget` and `store.factory_reset` in `manage_devices`) after a quiescence barrier that waits for the workers (its `Quiescence` loop) |
| `ble_slot_task`, running `connection_slot_task` ([slot_worker.rs](../../src/ble/slot_worker.rs)) | A command (the idle `cmd_rx.receive()`); notifications while the link lives (`run_notification_loop`) | `connect_with_security`, raced against neither commands nor a timer (in `connect_and_run_secure`); `close_connection`, which polls every 10 ms with no deadline |
| `usb_device_task`, running `run_usb_device` ([hid_device.rs](../../src/usb/hid_device.rs)) | Bus events and resume | `remote_wakeup`, which waits for USBWUALLOWED, RESUME or USBRESET (`embassy-nrf` `src/usb/mod.rs` lines 361 to 394) |
| `hid_writer_task` ([hid_device.rs](../../src/usb/hid_device.rs), with `dispatch_reports`) | Reports and host polling | None: each endpoint write has a 100 ms deadline and at most 1 s backoff (`run_endpoint` in [delivery.rs](../../src/hid/delivery.rs)) |
| `ui::display::task` ([display.rs](../../src/ui/display.rs)) | Frames | DMA on an electrically stuck bus, forever (`wait_stopped` and `finish_or_stop`; [ADR 0009](0009-isolated-display-task.md)) |
| `softdevice_task`, three button tasks | SoftDevice events; GPIO edges | None. The `Bonder` callbacks ([bonder.rs](../../src/ble/bonder.rs)) run synchronously inside `softdevice_task`, as the boot-protocol and LED handlers ([host_requests.rs](../../src/usb/host_requests.rs)) do inside `usb_device_task`, so a hang there stalls the executor |

Scans end under `with_timeout` after 6 s or 10 s
([scanner.rs](../../src/ble/scanner.rs) `find_saved_peer` and `scan`); the UI
abandons a management reply after 30 s with **No reply**, restarting nothing.
The other waits cannot simply get a timeout. The vendored flash driver arms a
`DropBomb` around `sd_flash_write` and `sd_flash_page_erase` while it awaits the
SoC event (`Flash::write` and `Flash::erase` in
`vendor/nrf-softdevice/src/flash.rs`),
so dropping the future panics. `connect_with_security` returns after the 6 s
whitelist scan and the MTU exchange, which ends on a response, a disconnect or
the GATT client timeout (the `att_mtu_exchange` call in `connect_inner`,
`vendor/nrf-softdevice/src/ble/central.rs`); an ATT transaction times out after 30 s (Core Specification Vol 3,
Part F, section 3.3.3; general knowledge). `close_connection` ends with the
disconnect event, and termination completes on acknowledgement or when the
supervision timer expires (Vol 6, Part B, section 5.1.6; general knowledge), at
most 4 s here ([ADR 0016](0016-bounded-peer-connection-parameters.md)). The
secure wait has an application bound of 25 polls 200 ms apart
(`wait_for_secure_link` in `slot_worker.rs`); discovery, Report
Map reads and LED writes are paced by the peer, bounded only per ATT step.

Platform facts:

- **Driver.** `embassy-nrf` 0.7.0 `src/wdt.rs`: `Watchdog::try_new::<N>` with 1
  to 8 handles (RR0 to RR7, each enabled one reloaded every period); `Config`
  has `timeout_ticks` at 32,768 Hz and `action_during_sleep` and
  `action_during_debug_halt` (`RUN` or `PAUSE`), default 1 s, `RUN`, `RUN`. On a
  running WDT `try_new` fails unless CONFIG, CRV and the handle count match
  (lines 98 to 106); `wdt::Config::try_new` reads them. Starting it also sets
  INTEN.TIMEOUT (line 112), which only delays the reset by two 32.768 kHz
  cycles (lines 132 to 135). A started WDT cannot be stopped (line 4).
- **Resets.** The nRF52840 Product Specification's reset-behavior table keeps
  the WDT running through soft reset and lockup, stops it on watchdog, pin,
  brownout and power-on reset, and warns that RAM may be corrupted (general
  knowledge). `RESETREAS` (POWER `0x400`, `nrf-pac` 0.1.0) holds RESETPIN (bit
  0), DOG (1), SREQ (2), LOCKUP (3) and System OFF wake-ups (16 to 20); bits
  accumulate until written with 1, and 0 means power-on or brownout (general
  knowledge). The `embassy-nrf` `reset` module is built only for nRF5340, so
  the PAC is the path until `Softdevice::enable`, after which POWER belongs to
  the SoftDevice (`sd_power_reset_reason_get`). `embassy_nrf::init` resets once
  after writing UICR ([security](../security.md#physical-access-and-debug-port))
  and probe-rs resets the same way (general knowledge), so SREQ is common.
- **SoftDevice.** The WDT interrupt is not in the vendored `RESERVED_IRQS`
  (`critical_section_impl.rs`); to our knowledge the S140
  Specification neither blocks nor restricts the WDT. Radio events preempt at
  priority 0 and a page erase halts the CPU for at most about 85 ms (NVMC
  electrical specification; general knowledge), far below seconds.
- **Retained RAM.** `cortex-m-rt` places a `NOLOAD` `.uninit` above `.bss` and
  below the painted stack (`link.x.in` lines 179 to 186). It holds only
  `defmt-rtt`'s 1,024-byte buffer ([validation record](../testing.md#validation-record--2026-10-10)),
  and with no stack guard ([ADR 0010](0010-static-memory-layout.md)) an overflow
  reaches it first.
- **Logging and simulation.** `defmt-rtt` 1.3.0 says logging can block forever
  once probe-rs, which selects blocking mode, disconnects. Renode 1.16.1's
  `nrf52840.repl` maps `Timers.NRF52840_Watchdog`, whose upstream source resets
  the machine on timeout but keeps CONFIG as a dummy, ignoring SLEEP and HALT;
  its `NRF_CLOCK` model at the POWER address has no RESETREAS.

## Decision

**Supervise leases, not heartbeats.** A lease is a deadline a task arms when it
starts an operation that something other than the peer bounds, and releases
when it returns. A task waiting for input or for another supervised task (a
channel, the `GAP_PROCEDURE` lock) holds none; whoever it waits for does. One
supervisor task reloads the WDT only while no lease is overdue, so a stall, an
interrupt storm, the fault loop or a blocked RTT write also stops the reloads.

| Lease slot | Checkpoint | Armed / released | Budget | Basis |
| --- | --- | --- | --- | --- |
| `Ui` | `UiTick` | Renewed at each housekeeping tick | 5 s | Five missed 1 s ticks |
| `Coordinator` | `CoordinatorBoot` | Start of `ble_task` / first reaching its `select` | 30 s | Memory-mapped store read, bond import, two sends |
| `Coordinator` | `CoordinatorStep` | Each command or slot event / back at the `select` | 180 s | Two connect leases, one close, one store write (130 s) plus margin; catches a channel wait cycle with a worker |
| `Slot0`, `Slot1` | `SlotConnect` | After taking `GAP_PROCEDURE` / when `connect_with_security` returns | 45 s | 6 s scan plus the 30 s ATT timeout, plus margin |
| `Slot0`, `Slot1` | `SlotClose` | Around `close_connection` | 10 s | 4 s supervision timeout times 2.5 |
| `Usb` | `UsbRemoteWakeup` | Around `device.remote_wakeup()` | 5 s | Answered within milliseconds on a powered bus (general knowledge) |
| `Storage` | `StorageWrite` | Around each `save_to_flash`, `forget` and `factory_reset` call | 30 s (estimate) | Three attempts, each perhaps a 4 KiB erase, plus the 16 KiB recovery erase in `factory_reset` ([storage.rs](../../src/storage.rs)); replace with four times the longest measured |

The display, USB enumeration and delivery, `softdevice_task`, the buttons and
every peer-paced phase hold no lease. A reset drops both links and enumeration,
while a stuck I2C bus survives it and is not needed to type, so ADR 0009
already rejected that trade; a host that stops polling or a suspended bus is
not a fault. A slow peer must never reset the bridge: a peer-paced phase that
needs a bound gets an application deadline that fails the link, raced like the
command against `prepare` in `connect_and_run_secure` (`slot_worker.rs`).

**Feeding rule.** `watchdog_task` in a new `src/watchdog.rs`, spawned first,
snapshots the six slots every `WDT_SUPERVISOR_PERIOD_MS` (1,000 ms) and calls
the pure `Supervisor::evaluate(now_ms, &leases)`:

1. Nothing overdue: reload RR0.
2. Something overdue while `Storage` is armed and within budget: keep reloading
   and log once; the supervisor never resets during a flash write in budget.
3. Otherwise commit, once and irreversibly: log
   `watchdog: {slot} {checkpoint} overdue by {} ms; resetting`, store the
   verdict in the reset record, `try_send` `HidEvent::Disconnected` for both
   slots so held input ends now, and stop reloading.

A lease is one `AtomicU32` (zero when idle, else a 5-bit checkpoint and a 27-bit
millisecond deadline compared modulo 2^27, about 37 hours), set by single stores
through a guard that releases on drop, also for branches a `select` cancels.

**WDT settings.** `WDT_TIMEOUT_SECS` = 8 (262,144 ticks). Sleep `RUN`: the
executor sleeps between events, so a paused counter would never expire on a
deadlock in which every task waits; reloads continue through USB suspend
([ADR 0012](0012-bus-powered-no-system-off.md)). Debug halt `PAUSE`: a
breakpoint, a probe-rs halt or its HardFault catch never resets a unit under
inspection. One handle, owned by `watchdog_task`; no handler is bound to the
TIMEOUT interrupt the driver enables, and its NVIC line stays disabled. `main`
starts the WDT after `embassy_nrf::init`, before `Softdevice::enable`, and
adopts one still running after a soft or lockup reset through
`wdt::Config::try_new` (`watchdog already running ({} ticks); adopted`); an
inherited handle count other than one is reloaded through the PAC.

**Reset causes.** Before `Softdevice::enable`, `main` reads and clears
`POWER.RESETREAS` and decodes it with the pure `ResetCause`, preferring LOCKUP,
then DOG, RESETPIN and SREQ (a DOG followed by `init`'s UICR reset reads DOG and
SREQ). A 20-byte record in `.uninit.bt2usb.reset` (magic, saturating
`watchdog_resets` u16, `early` u8, flags, verdict, fault PC, check word) is used
only when magic and check word are valid. A new `#[exception] HardFault`
handler stores the stacked PC and loops until the WDT fires.

| Boot finds | Log line | OLED notice |
| --- | --- | --- |
| DOG and a verdict | `reset cause: watchdog, {slot} {checkpoint} overdue` | `Restarted: link 1` (or `link 2`, `BLE`, `storage`, `controls`, `USB`) |
| DOG and a fault PC, or LOCKUP | `reset cause: fault at {:#010x}`, `reset cause: lockup` | `Restarted: fault` |
| DOG and neither | `reset cause: watchdog, executor stalled` | `Restarted: stalled` |
| SREQ, RESETPIN or 0 | `soft reset`, `pin reset`, `power-on or brownout` | None |

A panic reaches the handler through `udf`, so its PC names `panic-probe`'s
`hard_fault`; the message needs a probe. The notice is retained like existing
notices, dismissed with SELECT, and never blocks input.

**Reset loops.** A DOG or LOCKUP boot increments `early`; any other cause or
300 s of uptime (`WDT_STABLE_UPTIME_SECS`) clears it, so the third such reset in
a row, the last two within 300 s of boot, reaches `WDT_MAX_EARLY_RESETS` (3).
That boot leaves the WDT off (or reloads an inherited one unconditionally), runs
the supervisor report-only, logs `watchdog not armed after 3 early resets;
unplug to retry` and shows `Watchdog off` until a power cycle, a pin reset
(P0.18) or a reflash.

**What survives.** Flash and the record. Bonds reload at boot
([ADR 0006](0006-fail-closed-pairing-store.md)), both slots reconnect
([ADR 0015](0015-shared-reconnect-scan.md)), and the USBD reset drops the D+
pull-up, so the host sees a detach, releases held input and enumerates again,
renegotiating protocol and LEDs. A reset during a flash write is a power cut
for the store ([ADR 0019](0019-power-loss-safe-persistence.md), proposed), which
the storage grace keeps the supervisor from causing. `bt2usb-selftest` never
starts the WDT but adopts and reloads one it inherits; `bt2usb-sim` adds a
scenario step that never releases a lease.

### Open Questions For The Owner

1. **WDT timeout:** `8s` (recommended), `4s`.
2. **Stalled display:** `exclude` (recommended), `include`.
3. **Repeated early watchdog resets:** `latch` after three (recommended), `loop`.
4. **Restart notice on the OLED:** `show` (recommended), `log-only`.

## Alternatives Considered

- **No watchdog.** A hang holds a key on the host until the bridge is unplugged.
- **Reload periodically without checking progress.** It covers stalls and
  faults but not an async task stuck in `close_connection`, a flash event that
  never arrives, or a wait cycle, while the executor stays healthy.
- **Per-task heartbeats, or one reload register per task.** Event-driven tasks
  wait without bound by design, so each idle await would need a timer branch
  only to bump a counter, which proves nothing about a stalled sibling branch.
  One RR register per task would put the policy in hardware, where it cannot be
  host-tested, name the culprit or grant the storage grace.
- **Supervise the display.** The bus stays stuck after a reset, so the bridge
  would reset every few seconds and never keep a link.
- **Restart the stuck task, or time out every wait.** Embassy cannot cancel or
  respawn a task, a stuck SoftDevice call cannot be abandoned without disabling
  the SoftDevice, a dropped flash future panics, and timeouts miss stalls and
  faults. Timeouts remain right for peer-paced phases.
- **`SCB::sys_reset` at the commit.** It saves up to 8 s but records SREQ, as
  probe-rs does; waiting keeps DOG as evidence and one reset path.
- **Causes in flash or GPREGRET.** Flash adds wear and a writer beside the
  pairing store; GPREGRET and GPREGRET2 hold 8 bits each, belong to the
  SoftDevice after enable, and may not survive a watchdog reset (unconfirmed).

## Rationale

Leases supervise exactly the waits with a known bound, in the code that knows
it, and a wait on another task is covered by that task's lease, so the bridge
resets only when something ran past its worst legitimate duration; the step
lease covers a cycle through bounded channels. Neither a slow keyboard nor a
loose OLED cable can cost the user both links.

The timeout covers only executor stalls, which last milliseconds, plus the 1 s
reload period; flash, connects and scans are async and leased. Eight seconds
leaves room for development: probe-rs runs its flash algorithm on the core
(general knowledge), and erasing and programming the release image (about
123 KB of `.text`, `.rodata` and `.data`) takes up to about 3.9 s at the
specification's maximum timings (estimate). The price is a frozen bridge holding
a key for up to 8 s, not 4 s, since a freeze begins after the last reload.
[ADR 0019](0019-power-loss-safe-persistence.md) expects the watchdog to sit
above the worst measured flash operation; here the storage budget does. The
record lives in RAM because the resets it explains keep power, and the latch
turns a deterministic fault into a stable, explained state, where a loop would
re-enumerate every few minutes.

## Consequences

Positive:

- A stall or fault resets within 8 s of the last reload, an overdue lease within
  8 s of its deadline, since the last reload comes no later than the deadline
  (plus tick jitter); held keys end at the reset or, for a lease, at once.
- A probe unplugged mid-session no longer freezes the bridge for good.
- Each reset is logged and, without a probe, named on the OLED; a stuck I2C bus
  still never resets the bridge.

Negative:

- A reset costs an input gap: boot, enumeration, and reconnection after each
  peripheral's supervision timeout, estimated at 5 to 10 s.
- A budget set too short causes spurious resets; budgets are asserted against
  `config.rs` but need soak evidence. Every new wait without an application
  bound needs a lease, which becomes an architecture constraint.
- Images started by a soft reset inherit a running WDT: one without this code,
  such as an older release, resets once after 8 s, and a reflash the WDT cuts
  off leaves it stopped, so the retry succeeds.
- The record is best-effort; the `HardFault` handler and `.uninit` static add
  `unsafe` code. A halted core never resets, and a latched unit runs unwatched
  until power-cycled.
- Input that crashes the firmware causes up to three resets, then the latch,
  instead of one hang; both deny service.

Product promise: a reset is a detach and attach, so no host needs software and
a monitor-hub KVM sees the same device (emulating KVMs unverified); firmware
setup must accept USB hot-plug, as most UEFI does (general knowledge,
unverified); bus power is unchanged. Estimated cost: about 2 KiB of flash, 50
bytes of statics plus a task future under 200 bytes, one wake-up per second.

## Implementation

| Concern | Where |
| --- | --- |
| Policy | New `src/watchdog_logic.rs`, in the host library through `#[path]`: `LeaseSlot`, `Checkpoint`, budgets, `Lease`, `LeaseGuard`, `Supervisor`, `ResetCause`, `ResetRecord`, `boot_decision` |
| Constants | `WDT_TIMEOUT_SECS`, `WDT_SUPERVISOR_PERIOD_MS`, `WDT_STABLE_UPTIME_SECS`, `WDT_MAX_EARLY_RESETS`, `ATT_TRANSACTION_TIMEOUT_MS` in [config.rs](../../src/config.rs) |
| Shell | New `src/watchdog.rs`: lease statics, `arm`, `start`, `watchdog_task`, reset reason, `.uninit` record; boot steps and `HardFault` in [main.rs](../../src/main.rs) |
| Leases | [multi_conn.rs](../../src/ble/multi_conn.rs), [slot_worker.rs](../../src/ble/slot_worker.rs), `run_usb_device` in [hid_device.rs](../../src/usb/hid_device.rs), the `main` tick |
| UI and images | Notice in [ui_logic.rs](../../src/ui/ui_logic.rs) and [display.rs](../../src/ui/display.rs); [selftest.rs](../../src/selftest.rs); [sim.rs](../../src/sim.rs) and `renode/bt2usb-sim.robot` |

Host tests ([ADR 0003](0003-pure-core-and-task-shell.md)) cover every
checkpoint, the 2^27 ms and 49.7-day wraps, guard drop, feeding within budget,
withholding 1 ms past one and naming the most overdue lease, the storage grace,
a commit surviving release, report-only mode, budget derivations as constant
assertions, every RESETREAS combination, rejected records, and each boot
decision including the latch. Renode ([ADR 0004](0004-layered-verification.md)):
30 s without a second `bt2usb-sim starting`, then the stuck lease brings one
within its budget plus 9 s (8 s and a 1 s margin), once the robot suite adds
the ELF-reloading `reset` macro of `bt2usb-sim.resc` (general knowledge).
Hardware ([first flash](../first-flash.md#6-device-management-and-degraded-display)):
each reset cause; an injected stall while a BLE key is held, the host key log
showing the repeat end within 8 s; an endless `SlotClose`; an I2C fixture (a
switch or MOSFET holding SDA, then SCL, to ground) with 30 minutes of typing and
no reset; long debug halts and USB suspends; the storage grace; the latch; five
reflashes of a running unit; a reset in BIOS setup and behind the KVM.

This unblocks "Watchdog and recoverable failures" and the reset reasons of
"Diagnostics without sensitive input" ([TODO.md](../../TODO.md#platform-memory-and-recovery)),
supplies the fixture for the first-flash live-bus OLED check, and informs
"Stack overflow detection" and "Soak and latency measurements".

### Verification Status

- **Implemented:** nothing of this proposal. Today no WDT runs, RESETREAS is
  never read, panics stop in `HardFault_`, the display isolates I2C stalls,
  endpoint writes are bounded and scans end under `with_timeout`.
- **Software-verified:** nothing covers leases or reset causes. Related: 2 host
  tests in `display_logic.rs` (display backoff), 4 in `delivery_tests.rs`
  (bounded async endpoint workers); Renode never starts the WDT.
- **Hardware-verified:** not yet; no first-flash record or I2C fixture exists.

## Related

- Architecture: [what is fatal](../architecture.md#what-is-fatal), [not yet handled](../architecture.md#not-yet-handled), [what may block](../architecture.md#what-may-block), [retries, deadlines and backoff](../architecture.md#retries-deadlines-and-backoff), [boot and initialization](../architecture.md#boot-and-initialization), [decisions needed](../architecture.md#decisions-needed-for-roadmap-work)
- Operations: [firmware panics](../operations.md#firmware-panics-or-stops-responding), [stuck keys](../operations.md#keys-or-buttons-stay-pressed-on-the-host), [stuck I2C bus](../operations.md#oled-is-dark-or-the-i2c-bus-is-stuck), [recovery and diagnostics](../operations.md#recovery-and-diagnostics)
- [Hardware: power](../hardware.md#power), [memory layout](../hardware.md#memory-layout), [pin and peripheral usage](../hardware.md#pin-and-peripheral-usage); [security: physical access and debug port](../security.md#physical-access-and-debug-port), [threat model](../security.md#threat-model)
- [Testing: Renode simulation](../testing.md#renode-simulation), [hardware acceptance evidence](../testing.md#hardware-acceptance-evidence), [known verification gaps](../testing.md#known-verification-gaps); [features: current technical boundaries](../features.md#current-technical-boundaries)
- [ADR 0003](0003-pure-core-and-task-shell.md), [ADR 0004](0004-layered-verification.md), [ADR 0005](0005-two-slots-and-independent-endpoints.md), [ADR 0006](0006-fail-closed-pairing-store.md), [ADR 0009](0009-isolated-display-task.md), [ADR 0010](0010-static-memory-layout.md), [ADR 0012](0012-bus-powered-no-system-off.md), [ADR 0015](0015-shared-reconnect-scan.md), [ADR 0016](0016-bounded-peer-connection-parameters.md), [ADR 0019](0019-power-loss-safe-persistence.md) (proposed)
- TODO.md: [platform, memory and recovery](../../TODO.md#platform-memory-and-recovery), [pairing storage](../../TODO.md#pairing-storage), [UI, display and power](../../TODO.md#ui-display-and-power), [board bring-up and hardware acceptance](../../TODO.md#board-bring-up-and-hardware-acceptance)
