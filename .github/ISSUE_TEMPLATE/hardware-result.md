---
name: Hardware acceptance result
about: Record a first-flash checklist run on a real board
title: "Hardware result: <board> / <firmware commit or tag>"
labels: hardware-evidence
assignees: ""
---

Work through [docs/first-flash.md](https://github.com/chefzaid/bt2usb/blob/main/docs/first-flash.md)
and record each check below. The sections match its steps 0–8; section 8 is
optional. Write `pass`, `fail`, or `skip: <reason>` in every Result cell; an
empty or skipped cell is unverified, not a pass. Quote the log line or
measurement that supports each result in Notes.

## Build

- Commit or tag:
- Boot line (`bt2usb firmware starting: version ..., commit ..., ... build, DEFMT_LOG=...`):
- ELF SHA-256 (and HEX SHA-256 for a release package):
- Rust version:
- SoftDevice version and archive SHA-256:

## Setup

- Board and revision, pin changes, supply, debug probe:
- Peripherals (make, model, firmware, and each one's `slot N PnP ID: ...` line):
- Host OS and version, BIOS/UEFI, monitor and hub:

## Measurements

- SoftDevice RAM requirement (`softdevice RAM: N bytes`):
- Stack high-water after section 4 (`stack high-water: X of Y bytes`):
- Event counters from the section 4 Counters check, and the latest line after section 5 (`diagnostics: links lost N, ...`):
- Reconnect and release timings (sections 4 and 5):

## [0. Before you start](https://github.com/chefzaid/bt2usb/blob/main/docs/first-flash.md#0-before-you-start)

| Check | Result | Notes |
| --- | --- | --- |
| `mask ci` passes | | |
| Board is an nRF52840-DK or another nRF52840 with USB wired out | | |
| Debugger and nRF USB cables connected as described | | |
| OLED and button wiring matches the table (list any pin changes) | | |
| OLED module answers at I2C address 0x3C | | |
| `mask probe-list` shows the probe | | |

## [1. SoftDevice (once per board)](https://github.com/chefzaid/bt2usb/blob/main/docs/first-flash.md#1-softdevice-once-per-board)

| Check | Result | Notes |
| --- | --- | --- |
| SoftDevice source, version, and archive hash recorded | | |
| Download finishes without errors | | |

## [2. Self-test image](https://github.com/chefzaid/bt2usb/blob/main/docs/first-flash.md#2-self-test-image)

| Check | Result | Notes |
| --- | --- | --- |
| `softdevice RAM: N bytes` (record N; at most 24576) | | N = |
| `[PASS] softdevice` | | |
| `[PASS] flash` | | |
| `[PASS] usb enumeration` | | |
| `[PASS] usb hid report` | | |
| `[PASS] oled i2c` and `[PASS] oled render` | | |
| `[PASS] button UP (P0.11)`, `DOWN (P0.12)`, `SELECT (P0.24)` | | |
| `[PASS] ble scan` | | Advertisements / HID devices heard: |
| `stack high-water: X of Y bytes` and `[PASS] stack` | | X = , Y = |
| `stack guard: 4096 bytes at ...` and `[PASS] stack guard` | | Guard range: |
| `==== self-test done: P passed, F failed, S skipped ====` shows 0 failed | | P = , F = , S = |
| Optional deliberate overflow ends in `stack overflow: ... PC not stacked` | | Line: |

## [3. Real firmware](https://github.com/chefzaid/bt2usb/blob/main/docs/first-flash.md#3-real-firmware)

| Check | Result | Notes |
| --- | --- | --- |
| Boot line reports the commit under test, without `-dirty` for a clean checkout | | |
| `reset reason:` matches how the board was started (`soft reset` after `mask run`, `power-on or brown-out` after plugging it in) | | Cause: |
| Boot log shows `SoftDevice started`, `USB HID device started`, `UI and isolated OLED tasks started`, `OLED initialized/recovered`, and no panic | | |
| `USB configured by host: true`; the PC lists keyboard, mouse, and consumer-control interfaces | | |
| OLED shows the Home screen | | |
| `stack guard: 4096 bytes at ...` follows the boot line | | Guard range: |
| `stack high-water: X of Y bytes` lines appear | | |

## [4. Pairing and daily use](https://github.com/chefzaid/bt2usb/blob/main/docs/first-flash.md#4-pairing-and-daily-use)

| Check | Result | Notes |
| --- | --- | --- |
| Pair a keyboard | | |
| Pair a mouse; both work and the OLED shows "2 devices" | | |
| Media keys | | |
| PnP ID logged for each peripheral, or a line saying it has none | | |
| Five-button mouse / horizontal scroll | | |
| Caps Lock LED follows the host | | |
| Reboot reconnect | | |
| Sleep reconnect (`slot N link lost; reconnecting`) | | |
| Absent at boot | | |
| No stuck keys (record release latency) | | |
| No stuck mouse/media input (record release latency) | | |
| Scan while reconnecting (record the delay) | | |
| Stack high-water after the above | | Bytes: |
| Counters after a reset and one sleep per peripheral: lost links and reconnect attempts 2; overflows and write failures 0 | | `diagnostics:` line: |

## [5. In the monitor](https://github.com/chefzaid/bt2usb/blob/main/docs/first-flash.md#5-in-the-monitor)

| Check | Result | Notes |
| --- | --- | --- |
| Enumerates through the hub; paired devices reconnect and type | | |
| Unplug/replug | | |
| Input during USB outage | | |
| PC sleep and wake (`USB remote wakeup sent`) | | |
| Wake filtering | | |
| Independent USB interfaces (attach or link the USB capture) | | Recovery time: |
| BIOS / boot menu | | |
| Monitor off and on | | |
| Unit identity: USB serial unchanged after reflash and port change | | The serial may be masked |

## [6. Device management and degraded display](https://github.com/chefzaid/bt2usb/blob/main/docs/first-flash.md#6-device-management-and-degraded-display)

| Check | Result | Notes |
| --- | --- | --- |
| Cancel by default | | |
| Forget one peer; it does not reconnect after reset | | |
| Factory reset; no prior peer auto-reconnects | | |
| Failure reporting (record cached/persistent state after reboot; `skip: no fixture` until a fault-injection hook exists) | | |
| Two sources sharing an endpoint (record supported pairs) | | |
| OLED failure isolation | | |

## [7. Afterwards](https://github.com/chefzaid/bt2usb/blob/main/docs/first-flash.md#7-afterwards)

| Check | Result | Notes |
| --- | --- | --- |
| `memory_sd.x` change committed with margin evidence, or none needed | | |
| Deviations filed as separate issues (link them) | | |
| Result archived; [release gates](https://github.com/chefzaid/bt2usb/blob/main/docs/deployment.md#release-gates) reviewed | | |

## [8. Extended Acceptance (Optional)](https://github.com/chefzaid/bt2usb/blob/main/docs/first-flash.md#8-extended-acceptance-optional)

| Check | Result | Notes |
| --- | --- | --- |
| Reflash with `mask run --release` while peers are saved; peers reconnect without pairing again | | probe-rs version: , erase mode: |
| Supply current, idle | | mA: |
| Supply current, scanning | | mA: |
| Supply current, two links active | | mA: |
| Supply current, OLED on / OLED off | | mA on: , mA off: |
| Supply current during USB suspend | | mA: |
| Measured currents compared with the declared 100 mA and the USB suspend-current limit | | Meter, debugger cable connected or not: |
| USB stays stable without an HFXO request (no unexpected drop-off or re-enumeration) | | Host USB errors, if any: |

## Sanitizing

- [ ] No bond keys (LTK/IRK), raw flash dumps, or private keystrokes are attached.
- [ ] Device names, BLE addresses, and the USB serial are removed or masked
      where they identify a person; no log from a `trace`-level build is attached
      ([logging and privacy](https://github.com/chefzaid/bt2usb/blob/main/docs/security.md#logging-and-privacy)).
