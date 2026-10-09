# ADR 0012: Stay Connected While Bus-Powered Instead Of Entering System-OFF

- Status: Accepted
- Date: 2026-06-24

This record was written retroactively on 2026-10-09 from the source and the
commit history. The bus-powered policy was stated in `45d8ae1` on 2026-06-24;
suspend-aware activity and new-press-only remote wakeup were added in
`2479c79` on 2026-09-28.

## Context

bt2usb is powered by the USB port it serves: in normal use, a monitor's USB
hub. It has no battery. Its value comes from being ready the moment the user
reaches for the keyboard, including in a BIOS or boot menu and after the PC
wakes from sleep.

The nRF52840 offers System ON, where the CPU can sleep between events while
peripherals keep running, and System OFF, the deepest mode, which stops the
clocks and radio and wakes through a reset. The first embedded version
(`8e6dd17`, 2026-02-21) described power management in battery terms: its
`power.rs` header listed "BLE idle mode", "USB suspend handling", and "System
sleep when inactive", and it exposed a `ble_low_power()` hook. The UI loop
called that hook but discarded its result, so it changed nothing, and the hook
was removed on 2026-06-22 (`e3bc620`). The README of that commit still listed
"BLE connection-parameter relaxation and System-OFF sleep" as missing, and
noted that inactivity tracked only button and connection events, not
keystrokes. On 2026-06-24 (`45d8ae1`) the project recorded the opposite
direction: on a bus-powered device those modes cost availability and latency
for power that does not matter, so they are deliberately not used. That commit
also made live HID traffic count as activity.

Two refinements followed in `2479c79`:

- `f477d4c` (2026-09-26) had made the bridge request USB remote wakeup for any
  report that arrived while the host was suspended, including mouse motion and
  key releases. `2479c79` limited wake requests to newly pressed inputs.
- Before `2479c79`, any recorded activity forced the power state back to
  `Active`, even while the USB bus was suspended. Since then activity cannot
  override a suspend; only the resume event does.

## Decision

Keep the radio, the BLE links, and the USB device fully operational at all
times, and use the power state only to decide whether the OLED is lit.

- **No System OFF.** The firmware never enters System OFF; no code requests
  it. The Embassy executor lets the CPU idle between events, which the
  [power.rs](../../src/power.rs) header describes as WFE / System ON idle.
- **No link-parameter relaxation.** Every connection requests a 7.5–15 ms
  interval (`BLE_CONN_INTERVAL_MIN` 6 and `BLE_CONN_INTERVAL_MAX` 12, in
  1.25 ms units) with zero slave latency, whatever the activity level.
- **Power state drives only the display.** `next_power_state` in
  [power_logic.rs](../../src/power_logic.rs) returns:

  | State | When | Effect |
  | --- | --- | --- |
  | `LowPower` | USB bus suspended, or more than 120 s without activity and no BLE link | OLED off |
  | `Idle` | More than 60 s without activity | None beyond `Active` |
  | `Active` | Otherwise | OLED may be on |

  The thresholds come from `IDLE_TIMEOUT_SECS` (60) in `power.rs` and twice
  that value. Independently, `screen_should_be_on` turns the OLED off after
  `SCREEN_AUTO_OFF_TIMEOUT_SECS` (120 s) without activity while
  `SCREEN_AUTO_OFF_ENABLED` is `true`. State changes log
  `Power: {:?} -> {:?}`.
- **Activity sources.** A button press, a new BLE connection, the USB bus
  resuming, and any HID report forwarded from a peripheral count as activity.
  HID traffic sets a lock-free flag (`note_hid_activity`) on the report path,
  and the UI loop folds it in on its 1-second housekeeping tick, so typing
  keeps the screen on even though reports never pass through the UI.
- **Suspend wins.** `set_usb_suspended(true)` moves the state to `LowPower`
  and logs `Power: usb_suspended={}`. While the bus is suspended, activity
  updates the timestamp but cannot leave `LowPower`. Resume records activity
  and returns to `Active`.
- **The first press only wakes the screen.** When the OLED is off, a button
  press records activity and is not passed to the UI reducer. While the bus is
  suspended the screen stays off, so button presses have no visible effect
  until the host resumes.
- **Remote wakeup only for new presses.** The USB configuration advertises
  remote-wakeup support. The input aggregator marks an update as a wake
  candidate only when `wake::new_press` reports a newly pressed key code above
  3 (codes 1–3 are keyboard error indications), a new modifier bit, a new
  mouse button among the five supported, or a new nonzero consumer usage.
  Motion, wheel, pan, releases, repeated held state, disconnect cleanup, and
  endpoint replay never wake the host. A candidate requests wakeup only while
  the bus is suspended, and each suspend starts with no pending request. The
  USB task logs `USB remote wakeup sent`, or
  `USB remote wakeup not possible: {}` when the host has not enabled remote
  wakeup; in that case, per the source comment in `run_usb_device`, the device
  stays suspended until the host resumes the bus itself.
- **Declared bus power.** The USB configuration declares a maximum of 100 mA.

## Alternatives Considered

- **System OFF after inactivity** (the battery-oriented sketch of 2026-02-21).
  System OFF drops USB enumeration and both BLE links, and leaving it is a
  reset. The PC would lose its keyboard, input could not wake a sleeping PC
  because no BLE link would be up to carry it, and every wake would mean
  re-enumeration and reconnection before the first key arrives.
- **Relax the connection interval or add slave latency when idle.** This saves
  a little current at the cost of slower first keystrokes and extra connection
  parameter updates. On bus power the saving does not matter, as the
  `power.rs` header puts it: "a few mA that wall power makes irrelevant".
- **Disconnect peripherals while the host is suspended.** A key press could
  then no longer wake the PC.
- **Wake on any input during suspend** (the behavior from `f477d4c` to
  `2479c79`). A mouse nudged on the desk, a key released, or a peripheral
  disconnecting would wake the PC.
- **Let activity override a suspend** (the behavior before `2479c79`). The
  OLED would light up while the PC sleeps, and the reported power state would
  disagree with the bus.

## Rationale

The device exists to be an always-ready wired keyboard and mouse. Staying in
System ON with live links is what lets a BLE keyboard work in a boot menu and
wake a sleeping PC, and it keeps reconnection out of the latency path.
The only power-related behavior a user sees is whether the OLED is lit, so the
power policy is a display policy, driven by real activity including typing.

USB suspend is the only reliable "nobody is using this PC" signal a
bus-powered device gets, so it overrides local activity. Waking the host is a
user-visible action, so it is reserved for an input a person deliberately
pressed.

## Consequences

Positive:

- Peripherals stay connected through PC sleep and screen-off, so the first key
  press is delivered, or wakes the PC when the host allows it.
- Mouse motion, releases, and link cleanup cannot wake a sleeping PC.
- The power policy is a pure, host-tested function.

Negative:

- Power draw is unmeasured. The USB 2.0 specification limits how much current
  a suspended device may draw from the bus (2.5 mA; this figure comes from the
  specification, not from the repository), and bt2usb keeps the radio, its
  links, and the USB peripheral running while suspended. Whether a board stays
  within that limit, or within the 100 mA it declares, has not been measured.
- The bridge loses power whenever the monitor turns off its hub. Peripherals
  reconnect after the next boot, but a flash write in progress at that moment
  is interrupted; power-loss behavior of the pairing store is an open item.
- Remote wakeup depends on the host enabling it for the device; otherwise the
  log reports that it was not possible and the PC stays asleep.
- Local buttons do nothing visible while the host is suspended.
- The `Idle` state has no effect today beyond logging.

Follow-up obligations, tracked in [TODO.md](../../TODO.md):

- "Power budget and USB suspend current" (P0): measure supply current idle,
  scanning, with two links, with the OLED on and off, and during USB suspend,
  and compare it with the declared 100 mA and the suspend-current limit. A
  reviewed exception to those limits updates this record.
- "Power-loss-safe persistence": fault-inject interrupted flash writes.
- "Soak and latency measurements": measure latency, reconnect time, and resets
  over multi-day use with monitor power changes.
- The "PC sleep and wake" and "Wake filtering" checks in
  [first flash](../first-flash.md#5-in-the-monitor) need board evidence.

## Implementation

| Concern | Where |
| --- | --- |
| Power manager, activity flag, idle timeout | `PowerManager`, `note_hid_activity`, and `IDLE_TIMEOUT_SECS` in [power.rs](../../src/power.rs) |
| State and screen policy | `next_power_state` and `screen_should_be_on` in [power_logic.rs](../../src/power_logic.rs) |
| Screen timeout settings | `SCREEN_AUTO_OFF_ENABLED` and `SCREEN_AUTO_OFF_TIMEOUT_SECS` in [config.rs](../../src/config.rs) |
| Connection parameters | `BLE_CONN_INTERVAL_MIN`, `BLE_CONN_INTERVAL_MAX`, and `BLE_SLAVE_LATENCY` in `config.rs`, applied in `connect_and_run_secure` ([multi_conn.rs](../../src/ble/multi_conn.rs)) |
| UI loop wiring | Suspend signal, housekeeping tick, and first-press handling in [main.rs](../../src/main.rs) |
| Wake rule | `new_press` in [wake.rs](../../src/hid/wake.rs), applied per source by `InputAggregator::apply` in [aggregate.rs](../../src/hid/aggregate.rs) |
| Suspend tracking and wake request | `UsbPowerHandler::suspended`, `dispatch_reports`, and `run_usb_device` in [hid_device.rs](../../src/usb/hid_device.rs) |
| USB configuration | `supports_remote_wakeup = true` and `max_power = 100` in `hid_device.rs` |

### Verification Status

- **Implemented:** everything above.
- **Software-verified:** host tests cover the policy and the wake rule:
  five tests in `power_logic.rs`, three screen-policy tests in
  [lib_logic_tests.rs](../../src/lib_logic_tests.rs), three tests in
  `wake.rs`, and aggregator tests that assert releases, disconnects, and
  invalid sources do not wake (for example
  `invalid_sources_cannot_modify_state_or_wake`). CI runs them on Linux and
  Windows, and they passed on GitHub-hosted runners in push runs 36441995385
  (`8a04b25`, 2026-09-28) and 37932436721 (`7fc99d6`, 2026-10-09) and
  scheduled run 37338711407 (2026-10-05). The suspend handling in
  `hid_device.rs` and the `PowerManager` glue are not host-tested.
- **Hardware-verified:** not yet. The repository holds no board record of PC
  sleep and wake, wake filtering, or power draw.

## Related

- [Features: wake and display](../features.md#wake-and-display)
- [Architecture: execution and power choices](../architecture.md#execution-and-power-choices)
- [First flash: in the monitor](../first-flash.md#5-in-the-monitor)
- [ADR 0005: Two slots and independent endpoints](0005-two-slots-and-independent-endpoints.md)
- [ADR 0009: Isolated display task](0009-isolated-display-task.md)
