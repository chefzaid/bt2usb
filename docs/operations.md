# Operations Runbook

This runbook covers a flashed bt2usb unit: what to look at, what a healthy unit
logs, how to diagnose and recover common failures, and how to report a defect.
Use it with the [first-flash checklist](first-flash.md) for new boards, the
[deployment guide](deployment.md) for updates, rollback, and release gates, and
the [security reference](security.md) for key-handling limits. Firmware builds
are development artifacts until the applicable release gates have evidence
attached.

The device has no network connection and sends no telemetry. Everything an
operator can observe comes from four places: `defmt` logs over RTT through a
debug probe, the OLED, the USB host's view of the device, and the self-test
image. There is no watchdog and no recorded reset cause, so a hung or panicked
unit stays down until it is reset or power-cycled.

Behavior described here is implemented in the source and, where noted, covered
by host tests or the Renode scenario. Unless a step says otherwise it is not a
hardware-verified result; record board results with the
[hardware acceptance evidence](testing.md#hardware-acceptance-evidence).

## Runtime Surfaces

| Surface | Where | What it shows |
| --- | --- | --- |
| RTT log | Debug probe, `mask run --release` or `mask rtt` | `defmt` boot, BLE, USB, storage, display, power, stack high-water, and event counter logs, each with an uptime timestamp |
| OLED | On the device | Current screen, connected devices, retained errors and notices |
| USB enumeration | Host device manager, `lsusb` | Keyboard, mouse, and consumer interfaces; VID/PID, strings, per-unit serial |
| Self-test | `mask selftest` | Staged `[PASS]`/`[FAIL]`/`[SKIP]` results for each peripheral |
| Simulation UART | Renode `mask sim` | Logs from the SoftDevice-free build; no probe needed |
| Release metadata | `BUILD-INFO.json`, `SHA256SUMS` | Which build a unit was flashed with; the firmware's boot line reports the same commit ([boot sequence](#boot-sequence)) |

On the USB host the bridge appears with the development identity from
[config.rs](../src/config.rs):

| Field | Value |
| --- | --- |
| Vendor/product ID | `1209:0001` (pid.codes test VID; not a production identity) |
| Manufacturer string | `bt2usb` |
| Product string | `BT-to-USB HID Bridge` |
| Serial number | 16 uppercase hexadecimal characters from the chip's factory `FICR.DEVICEID` words; stable across reflashing and USB ports |
| Interfaces | Keyboard (boot subclass), mouse (boot subclass), consumer control; 1 ms polling interval |
| Power request | 100 mA, remote wakeup supported |

On Linux, `lsusb -d 1209:0001 -v` shows these fields. Record the serial number
to tell units apart. The descriptors are specified in the
[data model](data-model.md#usb-device-identity).

## Collecting Logs

### RTT With A Debug Probe

Logs travel over RTT through the debug probe, not over the bridge's own USB
port. Keep the probe connected while reproducing a problem: `defmt-rtt` keeps
only a small in-RAM buffer, so lines emitted while nothing is attached are
mostly lost.

| Goal | Command |
| --- | --- |
| Build, flash, and stream logs | `mask run --release` |
| Attach to a unit already running a release build from this checkout | `mask rtt` |
| Attach to a debug build from this checkout | `probe-rs attach --chip nRF52840_xxAA target/thumbv7em-none-eabihf/debug/bt2usb` |
| Attach to a unit flashed from a release | `probe-rs attach --chip nRF52840_xxAA bt2usb-vX.Y.Z.elf` |
| List probes | `mask probe-list` |

`defmt` sends compact indices, not text; the host decodes them with the string
table in the ELF. Always decode with the exact ELF that was flashed. A HEX file
cannot decode logs, and an ELF from a different build prints wrong text or fails
to decode. `mask rtt` always uses `target/thumbv7em-none-eabihf/release/bt2usb`,
so it is only correct for a release build from the current checkout.

Save a session with ordinary shell redirection, for example
`mask run --release 2>&1 | tee bt2usb-rtt.log`. On Windows, mask recipes need
Bash; [scripts/run-tool.sh](../scripts/run-tool.sh) also finds the current
Windows user's Cargo tools from WSL. Probe access from WSL and the devcontainer
is covered in [development](development.md#devcontainer-and-wsl2).

A panic is printed by `panic-probe` through `defmt` and then stops the core.
When `probe-rs run` is attached, the session ends with the panic message.

### Log Levels

`DEFMT_LOG` filters log statements at compile time, so changing it needs a
rebuild and a reflash. Local builds log at `debug` and CI and release
artifacts at `info`; [development](development.md#log-levels) owns how the
level is set and overridden. What each level reveals, including the typing
rhythm that any `trace` build records and the peer addresses and keystroke
bytes that only the `log-sensitive-data` build feature adds, is in
[security](security.md#logging-and-privacy). Note the level and any extra
feature with any log you share ([reporting a defect](#reporting-a-defect)).

### Renode UART

The simulation build (`bt2usb-sim`) has no SoftDevice, USB, or flash storage. It
writes plain text to UART0, which `mask sim` shows in the Renode analyzer window
and `mask sim-test` asserts headlessly. In CI, failed or passing Robot results
are uploaded as the `renode-results-<attempt>` artifact. Its lines, such as
`bt2usb-sim starting (SoftDevice-free Renode build)` and
`entering sim UI loop (screen=Home)`, come only from
[sim.rs](../src/sim.rs) and never from a real board. See
[Renode simulation](testing.md#renode-simulation).

### Self-Test Output

The self-test image ([selftest.rs](../src/selftest.rs)) logs
`==== bt2usb self-test ====`, one `[PASS] {stage}: {detail}`,
`[FAIL] {stage}: {detail}`, or `[SKIP] {stage}: {detail}` line per stage, and
finally `==== self-test done: {} passed, {} failed, {} skipped ====`. Stages are
`softdevice`, `flash`, `usb enumeration`, `usb hid report`, `oled i2c`,
`oled render`, one per button, `ble scan`, and `stack`. It replaces the bridge
firmware, so flash the bridge again afterwards. The
[first-flash checklist](first-flash.md#2-self-test-image) explains each result.

## What Healthy Looks Like

### Boot Sequence

The main task logs these lines in this order, with no `.await` between them, so
no other task can interleave. At the release `info` level no other line
appears between them, except an `embassy-nrf` warning right after
`reset reason` when UICR already holds a different reset-pin or
NFC-pin setting ([dependency logs](security.md#dependency-logs)); a local
`debug` build adds dependency lines. Placeholders are shown as `N`, `X`, and
`<...>`.

```text
bt2usb firmware starting: version <version>, commit <commit>, <profile> build, DEFMT_LOG=<filter>
reset reason: <causes>
softdevice RAM: N bytes
You're giving more RAM to the softdevice than needed. You can change your app's RAM start address to X
USB power: vbus=true ready=true
USB HID composite device initialised (keyboard + mouse + consumer)
SoftDevice started
USB HID device started
BLE task started
UI and isolated OLED tasks started
```

- The first line names the build ([diagnostics.rs](../src/diagnostics.rs)):
  the version from `Cargo.toml`, the git commit `build.rs` found (40
  hexadecimal digits, with `-dirty` when tracked files differed from it, or
  `unknown` for a build outside git), the Cargo profile, and the `DEFMT_LOG`
  filter the build was compiled with. A release package's `BUILD-INFO.json`
  names the same commit, and staging refuses an image that reports another
  one ([deployment](deployment.md#version-and-build-policy)).
- `reset reason` decodes the nRF52840's `POWER.RESETREAS` register, which
  `main` reads and clears before the SoftDevice takes the POWER peripheral, so
  it names only the resets since the previous boot; see
  [Reset Reasons](#reset-reasons).
- `softdevice RAM: N bytes` is the RAM the configured SoftDevice needs. N must
  be at most 24576 (the `0x6000` reservation in [memory_sd.x](../memory_sd.x)).
- The "giving more RAM" warning appears only when N is below 24576. It is
  harmless; see [Stack And Memory Checks](#stack-and-memory-checks).
- `USB power` shows `vbus=false ready=false` when the native USB port is not
  powered yet; enumeration starts when it is plugged in.
- If the SoftDevice USB power events cannot be enabled, the `USB power` line is
  replaced by `failed to enable USB power events; assuming VBUS present`. If
  only the regulator status read fails, the line is simply missing; in both
  cases [sd_setup.rs](../src/sd_setup.rs) assumes VBUS is present.
- On the first boot after UICR was erased, `bt2usb firmware starting` can
  appear twice: `embassy_nrf::init` writes the reset-pin setting (and, on
  chips with build code `F` or later, the debug-port setting) to UICR and
  resets the chip once before the rest of the sequence, and the second boot
  reports `reset reason: soft reset`. Later boots log it once. This is source-derived, not
  observed on a board; see
  [security](security.md#physical-access-and-debug-port).

The [boot lifecycle](architecture.md#boot-and-initialization) explains what
each step does.

The spawned tasks then start. Their order depends on the executor:

```text
USB device task started
HID dispatcher and three endpoint workers started
Loaded N devices from flash            (or: No paired devices in flash)
Loaded N BLE bonds into security handler
OLED initialized/recovered
USB configured by host: true
stack high-water: X of Y bytes
```

`USB configured by host: true` appears once the host finishes enumeration. The
host usually sends its LED state soon after, logged as
`Host LEDs: num=<bool> caps=<bool> scroll=<bool>`. The stack line first appears
about one second after boot and again only when the high-water mark grows.

### Reconnecting Saved Peripherals

With saved peers, the BLE task hands the two most recently added peers that
have a bond to the two connection slots at once, without a scan. Each slot listens for both saved
peers and connects to whichever it hears; a slot that hears the other slot's
peer hands it over and logs `slot N scan found slot M's device`. For a
keyboard:

```text
slot 0 connecting to <name>
BLE security mode updated: <mode>
Discovering HID service...
HID service discovered (N report characteristics)
Set HID protocol to Report mode
Complete report map (N bytes): keyboard=<bool> mouse=<bool> consumer=<bool>
Found keyboard LED output report
Subscribed to N of M HID report characteristics
HID notification loop started
```

The same sequence follows for slot 1. Scans, reconnect scans, and connection
setup take turns on the radio through one GAP procedure lock, so slot 1's
connection waits until slot 0's connection is established or times out; the
peer that wakes first connects first. When the peripheral asks for other
connection parameters, `peer connection parameters granted: …` or
`peer asked for connection parameters …; granting …` follows the connection
lines. Once the host has sent its LED state, a keyboard link writes it as soon
as its notification loop starts. The
lock is released before security and HID discovery, so the two slots'
discovery lines can interleave.
`Set HID protocol to Report mode` appears only when the peer has a Protocol Mode
characteristic, and `Found keyboard LED output report` only for keyboards with
an LED output report. A reconnect of a known peer whose record did not change
writes nothing to flash and logs no storage line at the info level.

A first pairing adds `Added paired device - now storing N` and
`Saved N devices to flash`. A link that drops and recovers logs
`HID notification loop ended (connection closed)`, then
`slot N link lost; reconnecting`, then the connection sequence again.

### Steady State

- No `warn` or `error` lines while idle or typing.
- `stack high-water` stops growing after the paths in use have run.
- No `diagnostics:` line while nothing goes wrong. Links lost and reconnect
  attempts when peripherals sleep and wake are normal; failures, overflows,
  and flash counts that keep rising are not ([event counters](#event-counters)).
- `Power: Active -> Idle` after 60 seconds without activity. Typing, mouse
  traffic, button presses, and BLE connections count as activity. With no BLE
  link, `Power: Idle -> LowPower` follows after a further 60 seconds.
- When the PC sleeps: `Power: usb_suspended=true` and a transition to
  `LowPower`. On resume: `Power: usb_suspended=false`.

### On The OLED

| Situation | Screen |
| --- | --- |
| Boot with no saved peers | `bt2usb / Idle`, `SELECT: scan`, `UP: saved devices` |
| Boot with saved peers | The Home screen until a saved peer connects, then `Connected` with the device name, or `2 devices` |
| 120 seconds without activity | Display off; the first button press only turns it back on |
| PC asleep (USB suspended) | Display off until the host resumes |

Errors and completion notices stay on screen until acknowledged
([features](features.md#wake-and-display)).

## First Checks After Flashing

1. The log shows the [boot sequence](#boot-sequence) through
   `UI and isolated OLED tasks started`, with no panic.
2. `USB configured by host: true` appears and the host lists the device.
3. `OLED initialized/recovered` appears and the OLED shows the Home screen.
4. Saved peers reconnect, and a held key is released when its link drops.

Anything beyond that is covered by the [first-flash checklist](first-flash.md).

## Log Message Reference

Messages are the exact format strings from the source; `{}` and `{:?}` are
values filled in at run time. Panics end the boot; everything else is a log
line. Lines from the vendored SoftDevice wrapper are marked "(vendor)".

### Boot, SoftDevice, And Faults

| Level | Message | Meaning | Action |
| --- | --- | --- | --- |
| info | `bt2usb firmware starting: version {=str}, commit {=str}, {=str} build, DEFMT_LOG={=str}` | `main` started; names the build | Quote the line in a report; twice in a row on the first boot after a UICR erase is expected ([boot sequence](#boot-sequence)) |
| info | `reset reason: {}` | The causes of the last reset, joined with ` + `, and any undefined register bits | See [Reset Reasons](#reset-reasons) |
| info | `softdevice RAM: {:?} bytes` (vendor) | RAM the SoftDevice configuration needs | Record it; must be at most 24576 |
| warn | `You're giving more RAM to the softdevice than needed. You can change your app's RAM start address to {:x}` (vendor) | The 24 KiB reservation exceeds the requirement | None required; see [memory checks](#stack-and-memory-checks) |
| panic | `too little RAM for softdevice. Change your app's RAM start address to {:x}` (vendor) | The reservation in `memory_sd.x` is too small | [RAM panic incident](#boot-panics-while-enabling-the-softdevice) |
| panic | `selected configuration has too high RAM requirements.` (vendor) | The SoftDevice configuration exceeds what S140 supports | Revert the change to `sd_setup.rs` |
| panic | `sd_softdevice_enable err {:?}`, `sd_ble_enable err {:?}`, `sd_ble_cfg_set {:?} err {:?}` (vendor) | The SoftDevice rejected enable or configuration | Check the SoftDevice version and recent configuration changes |
| panic | Starts with `Softdevice assertion failed:`, `Softdevice memory access violation.`, or `Softdevice unknown fault` (vendor) | SoftDevice fault handler | Report with the full message and PC value; see [panics](#firmware-panics-or-stops-responding) |
| info | `SoftDevice started`, `USB HID device started`, `BLE task started`, `UI and isolated OLED tasks started` | Each group of tasks was spawned | None |
| info | `stack high-water: {} of {} bytes` | Deepest stack use since reset, of the stack region size | Keep well under half; see [memory checks](#stack-and-memory-checks) |
| info | `diagnostics: {}` | Every event counter since boot, as `name N` pairs; logged when they change, at most once a minute | Quote the latest line in a report; see [Event Counters](#event-counters) |

### USB And Power

| Level | Message | Meaning | Action |
| --- | --- | --- | --- |
| info | `USB power: vbus={} ready={}` | USB regulator state read at boot | `false` means the native port was unpowered at boot |
| warn | `failed to enable USB power events; assuming VBUS present` | SoftDevice USB power events could not be enabled | Unplug/replug will not be noticed; reset the board with the cable attached |
| info | `USB HID composite device initialised (keyboard + mouse + consumer)` | Descriptors built | None |
| info | `USB device task started` | USB stack running | None |
| info | `HID dispatcher and three endpoint workers started` | Input path running | None |
| info | `USB configured by host: {}` | Host finished (`true`) or removed (`false`) configuration | `true` must appear for input to reach the host |
| info | `Host LEDs: num={} caps={} scroll={}` | Host keyboard LED state, forwarded to BLE keyboards | None |
| warn | `USB HID endpoint unavailable; retaining current input state` | A configured, unsuspended endpoint missed its 100 ms write deadline or failed | [Endpoint incident](#a-usb-endpoint-stalls) |
| info | `USB remote wakeup sent` | Wake request sent to a suspended host | None |
| info | `USB remote wakeup not possible: {}` | Host has not enabled remote wakeup for this device | [Wake incident](#host-does-not-wake-from-sleep) |
| info | `Power: usb_suspended={}` | Host suspended or resumed the bus | None |
| info | `Power: {:?} -> {:?}` | Power state change (`Active`, `Idle`, `LowPower`) | None |

### BLE Scan And Connection

| Level | Message | Meaning | Action |
| --- | --- | --- | --- |
| info | `BLE scan starting ({} s window)` | Scan acquired the radio | None |
| info | `BLE scan complete - {} devices found` | HID advertisers listed (at most 8) | 0 means [scan finds nothing](#scan-finds-no-devices) |
| info | `BLE scan hit hard timeout backstop` | No advertisement arrived after the 8-second window closed, so the 10-second backstop ended the scan | Normal in a quiet radio environment |
| warn | `BLE scan ended with error` | The SoftDevice scan failed; the OLED shows `Scan failed` | Retry; check for `sd_ble_gap_scan_start err` |
| info | `Loaded {} BLE bonds into security handler` | Bonds available for reconnect | 0 with saved peers means records have no keys or the store is unreadable |
| info | `slot {} connecting to {}` | A connection attempt started (6 s limit) | For a device chosen from a scan it appears at once. For a background reconnect it appears only after a reconnect scan, this slot's or the other slot's, has heard the device, so no line means the device is not being heard ([reconnect incident](#saved-peripheral-does-not-reconnect)) |
| info | `slot {} scan found slot {}'s device` | This slot's reconnect scan heard the other slot's saved device, recorded the sighting, and woke that slot, which connects to it next; this slot scans again after its 500 ms pause | None; normal while both slots are reconnecting |
| info | `BLE security mode updated: {}` | Link encryption changed | None |
| info | `peer connection parameters granted: {}` | The peripheral asked for connection parameters inside the bridge's limits and got them as asked. Values are in Core units: intervals in 1.25 ms, latency in connection events, timeout in 10 ms | None |
| info | `peer asked for connection parameters {}; granting {}` | The request was bounded to the [ADR 0016](adr/0016-bounded-peer-connection-parameters.md) limits; the granted interval is still inside the range the peripheral asked for | None |
| warn | `peer asked for connection parameters {}; granting {}, outside its interval range` | The granted interval is outside the requested range: the peripheral's fastest requested interval is slower than 30 ms, or its whole range is below the Core's 7.5 ms minimum | If the peripheral then disconnects, record its name and this line for the compatibility baseline |
| warn | `slot {} failed to secure BLE link` | Encryption or pairing failed, the link dropped, or security did not complete within 5 s | [Reconnect incident](#saved-peripheral-does-not-reconnect) |
| info | `slot {} link dropped during HID discovery` | The link dropped while its HID service was discovered or subscribed, for example a keyboard going back to sleep; the attempt failed as `Connect failed` | None for a background reconnect, which retries; for a selection, select the device again while it is awake |
| info | `slot {} link lost; reconnecting` | An established link dropped; held input was released | None; retries follow |
| info | `slot {} link lost; no keys to reconnect` | An established link dropped, and the bridge holds no keys for that device (its pairing showed `Pairing not saved`, the peripheral paired without bonding, or saving a newer device evicted its record and keys); held input was released and the slot was freed | Select the device from a scan to pair it again |
| info | `slot {} has no keys to reconnect` | A background reconnect stopped before an attempt because the bridge no longer holds the device's keys: saving a newer device evicted its record, and its keys with it; the slot was freed | Select the device from a scan to pair it again |
| warn | `Bond table full - dropped the oldest unsaved pairing` | A new pairing found the four saved devices' keys and two unsaved pairings held, and dropped the older unsaved one; unsaved pairings are discarded when their connection ends, so this is not expected | Report it with the log around it |
| warn | `sd_ble_gap_connect err {:?}`, `sd_ble_gap_scan_start err {:?}`, `sd_ble_gap_authenticate err {:?}` (vendor) | A SoftDevice GAP call was rejected | Note the error; report if it repeats |
| warn | `att mtu exchange refused: {:?}; keeping the default mtu` (vendor) | The peripheral answered the bridge's Exchange MTU Request with an ATT error, usually Request Not Supported. The link continues at the default 23-byte MTU, so the Report Map is read in 22-byte pieces | None; record the peripheral for the compatibility baseline |
| warn | `sd_ble_gatts_exchange_mtu_reply err {:?}`, `sd_ble_gatts_sys_attr_set err {:?}` (vendor) | The SoftDevice rejected the bridge's answer to a peripheral's own MTU exchange or to its access of the bridge's Service Changed CCCD; the peripheral's request then times out after 30 s, and it may stop sending reports on that link | Record the peripheral and the error; if its input stops about 30 s after connecting, report it |
| warn | `sd_ble_gap_disconnect err {:?}` (vendor) | The bridge asked to disconnect a link the SoftDevice no longer accepts a disconnect for, because it is already gone; the disconnect event that follows ends it as usual | None |
| warn | `unhandled timeout src {:?}` (vendor) | The SoftDevice reported a GAP timeout the bridge does not act on. Source 3 is the authenticated payload timeout: an encrypted link carried no packet with a valid MIC for 480 s, which a compliant peripheral prevents by answering LE Ping. The link stays up, and every report that arrives is still authenticated | Record the peripheral and this line for the compatibility baseline |

### HID Discovery And Input

| Level | Message | Meaning | Action |
| --- | --- | --- | --- |
| info | `Discovering HID service...` | GATT discovery started on an encrypted link | None |
| warn | `HID discovery failed: {:?}` | No usable HID service; the OLED shows `No HID service` | [HID error incident](#connect-fails-with-an-hid-error) |
| info | `HID service discovered ({} report characteristics)` | Service found | None |
| info | `Set HID protocol to Report mode` | Protocol Mode written | None |
| warn | `Could not set report protocol (using device default)` | Protocol Mode write failed | Usually harmless |
| warn | `HID report map absent; using legacy report classification` | The peer has no Report Map; reports are classified by length | Check input carefully; see [HID path](architecture.md#hid-path-and-limits) |
| info | `Complete report map ({} bytes): keyboard={} mouse={} consumer={}` | Report Map read and parsed | Confirms which report kinds were recognized |
| info | `Found keyboard LED output report` | LED forwarding target found | None |
| warn | `Skipping unknown or ambiguous HID report reference` | A report ID did not map to a supported kind | Expected for unsupported reports |
| warn | `Could not enable notifications on a report characteristic` | A CCCD write failed | Report if input is missing |
| warn | `No HID report characteristics could be subscribed` | Nothing to listen to; the OLED shows `Notify failed` | [HID error incident](#connect-fails-with-an-hid-error) |
| info | `Subscribed to {} of {} HID report characteristics` | Subscription result | None |
| info | `HID notification loop started` / `HID notification loop ended (connection closed)` | Input flowing / link closed by the peer or radio | None |
| warn | `Failed to write LED state to BLE keyboard` | LED write rejected | Cosmetic |
| warn | `Unknown HID report length: {}` | In the length-based fallback (no Report Map, or a map without report IDs and an unresolved report), a report of an unexpected length was dropped | Unsupported layout; report with the Report Map line |

### Pairing Storage

| Level | Message | Meaning | Action |
| --- | --- | --- | --- |
| info | `No paired devices in flash` | Empty store | None |
| info | `Loaded {} devices from flash` | Store read and validated | None |
| error | `Invalid or unsupported device store; writes disabled` | The stored frame failed validation | [Storage incident](#storage-unreadable-and-writes-disabled) |
| error | `Flash read error: {:?}` | The storage map could not be read; writes disabled | [Storage incident](#storage-unreadable-and-writes-disabled) |
| error | `Device store is unreadable; refusing to overwrite stored bonds` | A save was refused because the store is read-only | [Storage incident](#storage-unreadable-and-writes-disabled) |
| error | `Device store exceeds serialization capacity; save aborted` | Should be impossible (a compile-time check bounds it) | Report as a defect |
| info | `Added paired device - now storing {}` | New peer cached | A save follows |
| info | `Updated existing paired device` | Address, name, or keys changed | A save follows |
| warn | `Paired device store full - evicting oldest entry` | A fifth peer replaced the oldest-added one | Expected at capacity (4) |
| warn | `Bond refused: identity address is not public or random static` | The peer named a private, anonymous, or reserved identity address during pairing, or distributed no identity key while connecting from a private address (the vendored crate then uses that address as the identity); the security handler kept no keys, nothing is stored for the device, and the OLED shows `Pairing not saved` | Record the peripheral and report it. Once the link ends, background reconnects cannot bring it back, because they never pair; select the device from a scan to pair it again |
| warn | `Device store refused a bond whose identity is not public or random static` | Should be impossible: the security handler refuses such a bond first. The store refused the device and its keys were dropped from the security handler | Report as a defect with the preceding log |
| error | `Paired device address has a reserved type; not stored` | Should be impossible: the SoftDevice gives every link a defined address type | Report as a defect |
| info | `Saved {} devices to flash` | The store was written | None |
| warn | `Flash write busy (attempt {}), retrying` | A write attempt failed; retried after 20 ms | [Flash incident](#flash-writes-report-busy-or-fail) |
| error | `Flash write failed after {} attempts: {:?}` | All three attempts failed; the OLED shows `Storage failed` | [Flash incident](#flash-writes-report-busy-or-fail) |
| warn | `sd_flash_write err {:?}`, `sd_flash_page_erase err {:?}` (vendor) | The SoftDevice flash call failed | Read with the storage lines around it |
| debug | `DeviceStore: no changes to save` | Nothing to write | None |

### Display And Buttons

| Level | Message | Meaning | Action |
| --- | --- | --- | --- |
| info | `OLED initialized/recovered` | The display accepted initialization and a frame | None |
| warn | `OLED operation failed; retry in {} ms (bridge remains active)` | An I2C operation failed; retry after 1 s, doubling to 30 s | [OLED incident](#oled-is-dark-or-the-i2c-bus-is-stuck) |
| warn | `OLED I2C stalled; requesting STOP, display task degraded until DMA completes` | An operation passed its 500 ms deadline | [OLED incident](#oled-is-dark-or-the-i2c-bus-is-stuck) |
| warn | `OLED I2C error: STOP not complete; requesting again` | The bus has not stopped after an error | Likely a stuck bus; power off and check wiring |
| info | `Button: {}` | A debounced press (`Up`, `Down`, or `Select`) | None |
| warn | `management request got no reply; result unknown` | A saved-device list, Forget, or reset got no answer within `UI_MANAGEMENT_TIMEOUT_SECS` | [Pending incident](#a-management-action-stays-pending) |

### OLED Messages

What raises each error text is defined once in the
[data model](data-model.md#error-tags-and-ui-messages). The next step for each
message:

| Screen text | Next step |
| --- | --- |
| `Scan failed` | Retry the scan; see [scan incident](#scan-finds-no-devices) |
| `Connect failed` | Wake the peripheral, put it in pairing mode, retry |
| `No HID service`, `Notify failed` | [HID error incident](#connect-fails-with-an-hid-error) |
| `HID map read failed`, `HID map too large`, `Unsupported HID map` | [HID error incident](#connect-fails-with-an-hid-error) |
| `Storage failed` | [Storage](#storage-unreadable-and-writes-disabled) and [flash](#flash-writes-report-busy-or-fail) incidents |
| `Pairing not saved` | Record the peripheral and the `Bond refused` log line and report it; the device is not saved, keeps working until it disconnects, and then does not reconnect by itself; select it from a scan to pair it again |
| `Action failed; retry` | Reopen the saved-device list and retry |
| `Busy; try again` | Wait a few seconds and retry |
| `Device changed; retry` | Reopen the saved-device list |
| `No devices found` | [Scan incident](#scan-finds-no-devices) |
| `Complete` / `Device forgotten` or `Pairings reset` | A management change was stored; SELECT to dismiss |
| `Please wait...` | A management request is pending; see [pending incident](#a-management-action-stays-pending) |
| `No reply` / `Forget result unknown`, `Reset result unknown`, or `List not loaded` | The BLE task did not answer within 30 s; see [pending incident](#a-management-action-stays-pending) |

## Common Incidents

Each incident lists what you see, the likely causes traced to the code path,
how to confirm, and the fix. Keep an independent keyboard available while
debugging.

### Probe Not Found Or Flashing Fails

**Symptoms:** `mask probe-list` shows nothing, or `mask run --release` fails
before any `defmt` line appears.

**Likely causes:** a charge-only cable or the wrong board port (use the
debugger port for the probe); missing USB permissions on Linux; on Windows with
WSL, the probe not attached to WSL; the devcontainer started before the probe
was attached.

**Confirm:** run `mask probe-list` (it wraps `probe-rs list`). Under WSL, check
that `usbipd-win` attached the probe.

**Fix:** use a data cable on the debugger port, attach the probe to WSL as in
[development](development.md#devcontainer-and-wsl2), and retry. If the probe
works but the target does not respond, power-cycle the board.

### No Log Output After Flashing

**Symptoms:** flashing succeeds, but `bt2usb firmware starting` never appears,
USB does not enumerate, and the OLED stays dark.

**Likely causes:** the SoftDevice is missing. The application is linked at
`0x00027000` and relies on the SoftDevice image below it to start; after a
full-chip erase, or on a blank board, nothing hands control to the
application. A different SoftDevice version or variant is also unsupported.
Less likely: an ELF that does not match the flashed image, so logs cannot be
decoded.

**Confirm:** flash the self-test (`mask selftest`). If it also prints nothing,
or does not reach `[PASS] softdevice`, the SoftDevice is absent or wrong. This
symptom description is derived from the memory layout; the exact behavior of a
board without a SoftDevice has not been recorded.

**Fix:** install S140 v7.3.0 with `mask softdevice`
([SoftDevice installation](deployment.md#softdevice-installation)), then flash
the application again. A full-chip erase also removed the stored bonds, so pair
every peripheral again.

### Boot Panics While Enabling The SoftDevice

**Symptoms:** the log shows `bt2usb firmware starting` and
`softdevice RAM: N bytes`, then a panic such as
`too little RAM for softdevice. Change your app's RAM start address to X`.
USB never enumerates.

**Likely causes:** the SoftDevice configuration in
[sd_setup.rs](../src/sd_setup.rs) (two central links, 64-byte ATT MTU, two
security contexts) needs more RAM than the 24 KiB reserved by
[memory_sd.x](../memory_sd.x), for example after a link count or the MTU was
raised.
`selected configuration has too high RAM requirements.` means the configuration
itself is beyond what the SoftDevice supports. `sd_softdevice_enable err` or
`sd_ble_enable err` with another error suggests a wrong SoftDevice version.

**Confirm:** compare N with 24576 and the address X with `0x20006000`.

**Fix:** set `RAM : ORIGIN = X` in `memory_sd.x` and reduce `LENGTH` by the
same amount, so the region still ends at `0x20040000`. Keep the linker
assertion that `.data` starts at `ORIGIN(RAM)`; do not add `flip-link`. Rebuild,
flash, and record the new value. Update the
[memory layout](hardware.md#memory-layout) in the same change.

### Firmware Panics Or Stops Responding

**Symptoms:** input stops, the OLED freezes, the host may keep repeating the
last held key, and with a probe attached the log ends in a panic message.

**Likely causes:** a panic anywhere in the firmware or the SoftDevice fault
handler (`Softdevice assertion failed:` or `Softdevice memory access
violation.`). The panic lists
([application](code-quality.md#panic-paths-no-lint-flags) and
[vendored](code-quality.md#vendored-nrf-softdevice)) name every panic path the
code review found and why it should not fire. Any panic at runtime means one
of those reasons is wrong; quote the message in the report.
There is no watchdog, so the core stays stopped. A USB host
generally keeps the last report it received from a device that stops
responding, which is why a held key can keep repeating.

**Confirm:** reproduce with the probe attached through `mask run --release` and
capture the panic message and the lines before it.

**Fix:** unplug the bridge's USB cable or power-cycle the board; the host then
drops all held input and the bridge boots again. File a defect with the log.
Watchdog recovery is open work in
[TODO.md](../TODO.md#platform-memory-and-recovery); the
[architecture](architecture.md#what-is-fatal) lists which failures are fatal.

### USB Device Does Not Enumerate

**Symptoms:** the host does not list `1209:0001`; the log never shows
`USB configured by host: true`.

**Likely causes:**

- The cable is on the debugger port instead of the nRF52840's native USB port,
  or it is a charge-only cable.
- The native port was unpowered at boot (`USB power: vbus=false ready=false`).
  That line means the SoftDevice USB power events were enabled, so plugging in
  the native cable later should be reported and start enumeration; if it does
  not, the plug-in was not noticed.
- The SoftDevice USB power events could not be enabled
  (`failed to enable USB power events; assuming VBUS present`, logged instead
  of the `USB power` line). The firmware then treats VBUS as present from boot
  and never notices an unplug or a replug.
- The firmware panicked before USB started (no `USB HID device started`).
- The host or a hub blocks new USB devices, or the monitor's hub has no
  upstream cable to the PC.

**Confirm:** read the boot sequence up to `USB device task started`. Plug the
native port straight into the PC to rule out the hub. The self-test's
`usb enumeration` stage waits 10 seconds for configuration and says when no
VBUS is present.

**Fix:** use a data cable on the native port, connect directly to the PC during
bring-up, reset the board with the cable attached, and check the host's device
list or system log. Unplugging the native cable may log nothing; plugging it
back in logs `USB configured by host: true` again. VBUS detection is described
in [hardware](hardware.md#usb-connection).

### Keys Or Buttons Stay Pressed On The Host

**Symptoms:** a key auto-repeats or a mouse button stays down after the user
released it or the peripheral went away.

**Likely causes:**

- The BLE link dropped while the key was held. The release cannot arrive over
  BLE, so the bridge releases that slot's input when the SoftDevice reports the
  disconnect, after the supervision timeout. The bridge requests 4 seconds
  (`BLE_SUP_TIMEOUT`); a peripheral can negotiate different connection
  parameters. Repeating for up to about 4 seconds is expected.
- The other connected device still holds the same key or button. Held input
  from the two slots is a union, so it stays down until both release it.
- USB was unavailable when the release happened. Each endpoint keeps its
  current state and replays it when USB recovers, so the release should follow
  once the host polls again.
- The firmware stopped; see [panics](#firmware-panics-or-stops-responding).

**Confirm:** check for `slot N link lost; reconnecting` and its timestamp
relative to the stuck input, and for
`USB HID endpoint unavailable; retaining current input state`.

**Fix:** wait out the supervision timeout; release the key on the other device;
if input stays stuck, unplug and replug the native USB cable, which makes the
host drop held input. Record the case: held-input release is a release gate
([TODO.md](../TODO.md#input-aggregation-and-delivery)).

### Saved Peripheral Does Not Reconnect

**Symptoms:** a previously paired keyboard or mouse stays disconnected after a
reboot or after it slept.

**Likely causes:**

- The peripheral is asleep, switched off, out of range, or connected to another
  host. Retries continue: each attempt first runs a reconnect scan of up to
  6 seconds, shared by both slots, that matches the peer by its identity key
  or the address it last used, and only a device it hears gets a 6-second
  connection attempt. Attempts are 500 ms apart; a slot
  whose device the other slot's scan heard connects without waiting. The scan
  listens at the fast duty cycle for the first 30 seconds after a slot starts
  reconnecting, then at the slower default
  ([ADR 0015](adr/0015-shared-reconnect-scan.md)).
- The peripheral no longer has the bond, for example after being reset or paired
  with another host. Encryption fails and the log repeats
  `slot N failed to secure BLE link`. The application never starts pairing on
  a background reconnect ([security](security.md#pairing-and-authentication)),
  and the bridge still holds keys for this peer, so this does not resolve on
  its own. Selecting the device from a scan turns the retry into a user
  connection, which shows `Connect failed` while the slot keeps retrying;
  forget it and pair it again. (Separately, the vendored crate answers a
  peer's Security Request by requesting pairing when it finds no keys, which
  a background reconnect meets only when saving another device evicts this
  one's record and keys while its attempt is under way or while the link it
  opened is up; refusing that is open work in
  [TODO.md](../TODO.md#ble-central-and-pairing).)
- Only the two most recently added saved peers with a bond get a slot at
  boot. A third or fourth stored peer, or one saved without a bond because it
  paired without bonding, is connected only when selected from a scan.
- The bridge no longer holds the peer's keys: saving a fifth device evicted
  its record and keys, and a slot still retrying it stopped with
  `slot N has no keys to reconnect`. A pairing that was not saved evicts
  nothing.
- Retries were stopped: DOWN on the Connected screen disconnects every slot; a
  scan started while both slots were occupied disconnects both first; Forget or
  Factory reset stops the affected slots. A background reconnect retries
  connection and security failures, and a link that drops during HID
  discovery (`slot N link dropped during HID discovery`); a failure during HID
  discovery or subscription on a link that stays up (for example
  `No HID service`) ends that slot's retries and shows the error.
- The peer was never saved: the store is read-only or the save failed
  (`Storage failed`), or it was evicted
  (`Paired device store full - evicting oldest entry`).

**Confirm:** at boot, look for `Loaded N devices from flash`,
`Loaded N BLE bonds into security handler`, and a `slot N connecting to <name>`
line for the peer once it advertises; with no such line the device is not being
heard (see the next incident). Open the saved-device list (UP) to check that it
is stored.

**Fix:** wake the peripheral (press a key). Reset the board to restart boot
reconnects. If the bond is gone on the peripheral, Forget it on the bridge, put
the peripheral in pairing mode, scan, and select it.

### Bonded Peer With A Private Address Is Not Found

**Symptoms:** the peripheral is awake, but after `slot N link lost; reconnecting`
(or at boot) no `slot N connecting to <name>` line appears for it.

**Likely causes:** most BLE peripherals advertise with a resolvable private
address that changes over time. For a bonded peer, each background attempt runs
a passive scan of up to 6 seconds that resolves advertisers against the peer's
stored identity resolving key (IRK) in [scanner.rs](../src/ble/scanner.rs),
counting only connectable advertisements and accepting them without the HID
service UUID. Nothing is logged while it is not found; the slot waits 500 ms
and scans again. It is never found when:

- the peripheral is not advertising (asleep, or connected elsewhere);
- the peripheral replaced its IRK, for example after a reset or a new pairing
  elsewhere, so the stored key no longer matches;
- the peripheral advertises only non-connectable or scannable sets while it
  waits, which a connection could not use.

**Confirm:** put the peripheral in pairing mode and start a scan. If it appears
in `Select device`, the radio hears it and the stored identity is stale.

**Fix:** select it from that scan. An explicit connection may pair again when
the bridge has no matching keys; the new keys replace the stored bond for the
same identity address. If the peripheral now uses a different identity address,
a new record is added, so Forget the old one. Private-address resolution is
implemented, and the shared reconnect table around it is host-tested
([reconnect.rs](../src/ble/reconnect.rs)); it is not hardware-verified. The
[background reconnect lifecycle](architecture.md#background-reconnect) shows
the full loop.

### Storage Unreadable And Writes Disabled

**Symptoms:** the OLED shows `Storage failed` right after boot; saved peers do
not reconnect; new pairings work until the next reboot and then are forgotten.

**Likely causes:** the pairing store failed validation
(`Invalid or unsupported device store; writes disabled`) or could not be read
(`Flash read error: {:?}`). Possible sources are a store written by a newer
firmware with a different storage version, damaged flash contents, or other
data written to pages 240–243. The store is deliberately kept read-only so bonds
are never silently replaced
([ADR 0006](adr/0006-fail-closed-pairing-store.md)).
Every later save logs
`Device store is unreadable; refusing to overwrite stored bonds`.

**Confirm:** read the boot log. The self-test's `flash` stage logs
`flash: saved pairing record present ({} bytes)` and tests a scratch write, but
it replaces the bridge firmware.

**Fix:** if the store came from a newer firmware, flash that firmware again
([rollback](deployment.md#rollback)). Otherwise, accept losing the stored
pairings and run Factory reset: press UP, choose the last entry,
`Factory reset`, and press SELECT; then press DOWN to select `Reset` and SELECT
to confirm. Only this explicit path erases the four pages and writes an empty
store. A success notice
(`Pairings reset`) means the write completed; `Storage failed` means it did
not. Pair the peripherals again afterwards. See the
[load rules](data-model.md#load-rules).

### Flash Writes Report Busy Or Fail

**Symptoms:** `Flash write busy (attempt N), retrying`, possibly followed by
`Flash write failed after 3 attempts: {:?}` and `Storage failed` on the OLED.

**Likely causes:** SoftDevice flash operations wait for gaps in radio activity,
and saves run right after a connection is established, when two links and a
scan may be active. The "busy" wording is used for any failed write attempt.
Vendor lines `sd_flash_write err` or `sd_flash_page_erase err` show a rejected
SoftDevice call.

**Confirm:** check whether a final `Saved N devices to flash` follows the
warnings. One or two warnings followed by a save are harmless.

**Fix:** for a new pairing, the record stays in memory and marked unsaved, so
the next connection of any peer retries the save; reconnect the device (switch
it off and on) and look for `Saved N devices to flash` before powering the
bridge down. For Forget or Factory reset, a failed write leaves the cached
state unchanged and is reported as `Storage failed`; retry the action. One
exception: a Factory reset of an unreadable store erases the four pages before
writing the empty store, so if that final write fails the pages may already be
erased while the cache still lists the old peers. Repeated failures with the
radio idle point to a flash problem; run the self-test's `flash` stage.

### OLED Is Dark Or The I2C Bus Is Stuck

**Symptoms:** the display is blank while input still works.

**Likely causes:**

- Normal power policy: the display turns off after 120 seconds without
  activity and while the host has USB suspended. The first button press only
  wakes it.
- No display response: wiring (SDA P0.26, SCL P0.27, VDD (not 5 V), common
  ground) or a module strapped to I2C address `0x3D`. The firmware uses the
  `ssd1306` crate's default interface at `0x3C`, the address the self-test
  probes; a `0x3D` module needs a code change.
- A stuck bus: SDA or SCL held low. The display task logs that it requested
  STOP and then waits for the hardware; it can stay degraded while the rest of
  the bridge runs ([ADR 0009](adr/0009-isolated-display-task.md)).

**Confirm:** a healthy display logged `OLED initialized/recovered`. Repeated
`OLED operation failed; retry in N ms (bridge remains active)` lines, with N
growing from 1000 to 30000, mean the display never answers.
`OLED I2C stalled; requesting STOP, display task degraded until DMA completes`
followed by `OLED I2C error: STOP not complete; requesting again` means the bus
is stuck. The self-test's `oled i2c` and `oled render` stages isolate wiring.

**Fix:** press a button to rule out the power policy. Otherwise power the board
off, fix the wiring, and power on; reconnect wiring only with power off. Live
recovery from an electrically stuck bus is not implemented. Bus settings are in
[hardware](hardware.md#oled-display).

### Scan Finds No Devices

**Symptoms:** after `Scanning`, the OLED shows `ERROR` / `No devices found`, or
the wanted device is missing from `Select device`.

**Likely causes:**

- The peripheral is not in pairing mode, or is connected to another host and
  not advertising.
- Its advertisement does not include the HID service UUID (`0x1812`); only HID
  advertisers are listed.
- The list holds at most 8 devices (`BLE_MAX_DISCOVERED`), the eight HID
  advertisers received most strongly. In a crowded room a distant peripheral
  can be left out; hold it next to the bridge and scan again.
- A radio or antenna problem. A scan that fails outright logs
  `BLE scan ended with error` and shows `Scan failed`.

A scan waits for any connection attempt or reconnect scan that holds the radio,
each limited to 6 seconds, so `Scanning` can last longer than the 8-second
window while a slot is reconnecting.

**Confirm:** `BLE scan complete - 0 devices found` in the log. The self-test's
`ble scan` stage counts every advertisement it hears, separating "radio hears
nothing" from "nothing advertises HID".

**Fix:** put the peripheral in pairing mode close to the board and scan again.
If the self-test hears no advertisements at all, check the board and antenna.

### Host Does Not Wake From Sleep

**Symptoms:** the PC stays asleep when a key is pressed on the BLE keyboard.

**Likely causes:**

- The host did not enable remote wakeup for the device; the log shows
  `USB remote wakeup not possible: {}`.
- The press was not a new press: only a newly pressed key or modifier, consumer
  key, or mouse button requests wake. Movement, scroll, and releases do not.
- The BLE keyboard itself disconnected while idle, so the press first has to
  reconnect it. Whether that press is then delivered depends on the peripheral;
  this has not been characterized.
- The monitor or hub cut power to the bridge during sleep. The bridge then
  reboots when power returns, and its boot log shows the restart.

**Confirm:** `Power: usb_suspended=true` must appear when the PC sleeps.
`USB remote wakeup sent` means the bridge did its part.

**Fix:** allow the device to wake the computer in the host's settings (on
Windows: Device Manager, the keyboard, Power Management). Keep the hub powered
during sleep where the monitor allows it. The
[suspend and wake lifecycle](architecture.md#usb-suspend-remote-wakeup-and-resume)
describes the mechanism, and the
[first-flash checklist](first-flash.md#5-in-the-monitor) has the acceptance
steps.

### A USB Endpoint Stalls

**Symptoms:** `USB HID endpoint unavailable; retaining current input state`;
one kind of input (for example media keys) stops while the others still work.

**Likely causes:** the host is configured and not suspended but did not poll
that interface within the 100 ms write deadline, or the write failed. Each of
the keyboard, mouse, and consumer endpoints has its own worker, queue, and
retry backoff (20 ms doubling to 1 second), so one stalled endpoint does not
block the others ([ADR 0005](adr/0005-two-slots-and-independent-endpoints.md)).
A host without a driver for that interface, or a pre-OS environment that drives
only some interfaces, could cause this; neither has been characterized.

**Confirm:** the warning is logged once per failure streak. Check the host's
device list for a driver error on one interface.

**Fix:** none is needed when the host recovers: the endpoint replays the current
held state without replaying mouse motion. Short taps or motion made while it
was stalled can be lost; this loss policy is open work in
[TODO.md](../TODO.md#input-aggregation-and-delivery). If it persists, unplug
and replug the native USB cable.

### A Management Action Stays Pending

**Symptoms:** the OLED shows `Please wait...` and ignores every button, then
after 30 seconds **No reply** with `Forget result unknown`,
`Reset result unknown`, or `List not loaded`.

**Likely causes:** while a saved-device list, Forget, or Factory reset request
is pending, the UI ignores all buttons until the BLE task replies. The BLE task
handles one thing at a time: a user scan (8 seconds, plus waiting for the
radio), or a Forget or reset that first waits for each affected slot to
stop. A slot in the middle of a connection attempt stops only after the
attempt ends (up to 6 seconds). After `UI_MANAGEMENT_TIMEOUT_SECS` (30 s) the
UI stops waiting, logs `management request got no reply; result unknown`, and
shows **No reply**. That is far longer than any of these waits, so a timeout
means the BLE task is stuck, for example in `close_connection`.

**Confirm:** watch the log for the scan or connection lines that are in
progress, then for `Saved N devices to flash` or an error. The
[request lifecycle](data-model.md#management-request-lifecycle) explains the
request IDs.

**Fix:** wait for the operation in progress to finish: a scan takes 8 to 10
seconds, and a connection attempt or reconnect scan up to 6 seconds. On
**No reply**, press UP to reopen saved devices: a list that loads shows what
was stored, and a Forget or reset may have been written even though no answer
came. If the list does not load either, the BLE task is stuck; reset the board,
then reopen the list. A reset during a flash write is not covered by tested
power-loss behavior. A late answer to the abandoned request is ignored by its
request ID.

### Connect Fails With An HID Error

**Symptoms:** the OLED shows `No HID service`, `Notify failed`,
`HID map read failed`, `HID map too large`, or `Unsupported HID map` after
`Connecting...`.

**Likely causes:** the peripheral is not a BLE HID-over-GATT device, discovery
timed out, it has no notifiable input report, or its Report Map is over 512
bytes or rejected by the parser in
[report_protocol.rs](../src/hid/report_protocol.rs) (malformed items,
unbalanced collections, alternative usage sets, or no keyboard, mouse, or
consumer input). The bridge rejects these rather than guessing. A link that
drops during discovery is not one of these: it shows `Connect failed` and logs
`slot N link dropped during HID discovery`.

Field layouts are a different case. The parser keeps only routing metadata, so
an NKRO keyboard or a mouse with 16-bit axes connects normally. Its reports are
then dropped at notification time because the decoders in
[hid/mod.rs](../src/hid/mod.rs) accept only fixed byte-aligned lengths: the
device shows as connected but some or all of its input does nothing. With a
known report kind this drop is silent; only the length-based fallback logs
`Unknown HID report length: {}`.

**Confirm:** the log shows `HID discovery failed: {:?}`,
`No HID report characteristics could be subscribed`, or the Report Map line.

**Fix:** retry once with the peripheral close by. If it persists, the
peripheral is unsupported today; report it with the sanitized log lines.
Interoperability work is tracked in [TODO.md](../TODO.md#ble-central-and-pairing).

## Recovery And Diagnostics

| Symptom | First checks | Incident |
| --- | --- | --- |
| Probe missing | Data cable, power, probe permissions; WSL attachment; `mask probe-list` | [Probe not found](#probe-not-found-or-flashing-fails) |
| No firmware boot | Matching ELF, SoftDevice installed, RAM boundary log, correct native board target | [No log output](#no-log-output-after-flashing), [boot panic](#boot-panics-while-enabling-the-softdevice) |
| No USB device | Native USB port, data cable, direct host connection, `USB configured by host` log | [USB does not enumerate](#usb-device-does-not-enumerate) |
| No OLED | Common ground, VDD (not 5 V), pin mapping, module address, I2C error/retry logs; input bridge tasks remain independent | [OLED is dark](#oled-is-dark-or-the-i2c-bus-is-stuck) |
| No scan result | BLE HID/HOGP peripheral in pairing mode, nearby radio, scan completion | [Scan finds no devices](#scan-finds-no-devices) |
| Reconnect fails | Peripheral awake, bond state on both sides, sanitized security/disconnect log | [Saved peripheral does not reconnect](#saved-peripheral-does-not-reconnect), [private address not found](#bonded-peer-with-a-private-address-is-not-found) |
| Newly paired peer is not remembered | Visible storage failure and flash logs; an invalid/unsupported store is preserved until an explicit Factory reset | [Storage unreadable](#storage-unreadable-and-writes-disabled), [flash writes fail](#flash-writes-report-busy-or-fail) |
| Host will not wake | Hub remains powered, host wake permission, suspend/wakeup logs | [Host does not wake](#host-does-not-wake-from-sleep) |

Use the least disruptive step that fixes the problem. Each later step loses
more state:

1. Acknowledge the OLED message (DOWN on an error, SELECT on a notice).
2. Unplug and replug the native USB cable. The host drops held input. If the
   board is powered from another source (such as the debugger port), BLE links
   are kept; otherwise this is a power cycle.
3. Reset or power-cycle the board. Links restart; pairings are kept.
4. Reflash a known-working application with a probe
   ([flashing methods](deployment.md#flashing-methods)). Pairings are kept.
   If that build is older than the one running, follow
   [rollback](deployment.md#rollback) and check
   [storage compatibility](deployment.md#storage-compatibility) first: some
   older builds may overwrite a store they cannot read.
5. Factory reset from the saved-device menu. All pairings are deleted.
6. Full-chip erase, reinstall the SoftDevice, and flash the application. The
   SoftDevice and all pairings are deleted. When the application is an older
   build, use the [rollback](deployment.md#rollback) steps to pick and verify
   it; the first boot afterwards may log `bt2usb firmware starting` twice
   ([boot sequence](#boot-sequence)).

Keep an independent keyboard available during debugging. If firmware hangs,
reset or power-cycle the board, then reflash a known-working application using a
probe. If SoftDevice was erased, reinstall it before the application. The
firmware does not yet provide a watchdog recovery guarantee.

### Reset Reasons

The boot line `reset reason: {}` says why the chip last restarted. `main`
clears the register after reading it, so the causes are those since the
previous boot; several are joined with ` + ` when more than one happened
before a boot could clear them. The firmware never resets itself, enters
System OFF, or starts the watchdog, so most causes come from outside it.

| Logged cause | What restarted the chip | What it tells a report |
| --- | --- | --- |
| `power-on or brown-out` | Power was applied, or the supply fell below the brown-out level | The usual first boot. After a hang, it means the unit was unplugged or power-cycled: a panic leaves the core stopped until then ([panics](#firmware-panics-or-stops-responding)). Unexpected during use, it points at the USB supply, such as a hub port that sags |
| `reset pin` | The nRESET pin, such as the board's reset button | Someone pressed reset, or a probe reset the chip through the pin |
| `soft reset` | `AIRCR.SYSRESETREQ` | A probe restarting the chip after flashing, or `embassy_nrf::init` after writing UICR on the first boot ([boot sequence](#boot-sequence)); the bridge's own code never requests it |
| `watchdog` | The watchdog timer | Not expected: this firmware does not start the watchdog ([ADR 0020](adr/0020-watchdog-and-progress-based-recovery.md) is Proposed) |
| `CPU lock-up` | The core locked up, for example after a fault inside the HardFault handler | A defect: report it with the log before the reset, if a probe caught one |
| `wake from System OFF (...)` | A wake-up from System OFF by GPIO, LPCOMP, debug interface, NFC, or VBUS | Not expected: the bridge never enters System OFF ([ADR 0012](adr/0012-bus-powered-no-system-off.md)); another image ran before this one |

`(undefined bits 0x...)` after the causes quotes register bits the nRF52840
does not define; include it in a report as printed.

### Event Counters

The firmware counts events that a report needs to size a problem, without
recording what was typed or which device was involved. Each count is a
number of occurrences since boot and nothing else: no address, name, key,
or report content
([`diagnostics.rs`](../src/diagnostics.rs)). A line lists every counter in
this order:

```text
diagnostics: links lost 3, reconnect attempts 3, reconnect failures 1, reports coalesced 0, endpoint overflows 0, USB write failures 0, LED write failures 0, flash write retries 0, flash write failures 0
```

| Counter | Counted when | Normal | Worth reporting |
| --- | --- | --- | --- |
| `links lost` | An established link closed without the user asking: the peripheral slept or moved out of range, or the radio dropped it | Rises as peripherals sleep, about one per sleep | Rising while a peripheral is in use and close by: [reconnect incident](#saved-peripheral-does-not-reconnect) |
| `reconnect attempts` | A background reconnect found its saved device advertising and started to connect | About one per lost link or power-up | Far above `links lost`, which means attempts keep failing |
| `reconnect failures` | A background reconnect attempt ended without a working link | Occasional, such as a device that stops advertising mid-attempt | Close to `reconnect attempts`: [reconnect incident](#saved-peripheral-does-not-reconnect), [private address](#bonded-peer-with-a-private-address-is-not-found) |
| `reports coalesced` | A report from a peripheral replaced or merged into the previous one before that reached the bridge's report channel, usually because the USB side was behind | 0 while typing; mouse movement can merge now and then, which loses no motion | Rising during typing: a fast tap may have been missed ([stuck input](#keys-or-buttons-stay-pressed-on-the-host)) |
| `endpoint overflows` | A USB endpoint's 16-report queue was full, so it collapsed to the latest state and dropped the reports queued before it | 0 | Any: the host stopped polling that endpoint for a while; the final state still went out ([endpoint stalls](#a-usb-endpoint-stalls)) |
| `USB write failures` | A USB endpoint's write failed or timed out after it last worked or the bus last reset or resumed; its retries count once | 0 in use; a host that stops polling an endpoint, which a firmware setup screen may do for an interface it does not use (inferred, not measured), adds one each time | Rising in normal use: [endpoint stalls](#a-usb-endpoint-stalls) |
| `LED write failures` | The host's Caps/Num/Scroll Lock state could not be written to a keyboard | 0 | Any, if the keyboard's lock LEDs then stay wrong |
| `flash write retries` | A device store write failed and was retried, as happens while the radio leaves the SoftDevice no time to write | Occasional at connect time | Many per save: [flash writes](#flash-writes-report-busy-or-fail) |
| `flash write failures` | A device store save failed all three attempts, or a factory reset could not erase an unreadable store | 0 | Any: a pairing or Forget did not reach flash ([flash writes](#flash-writes-report-busy-or-fail)) |

How to collect them:

1. Attach a debug probe and read the RTT log (`mask rtt`, or `mask run
   --release` when flashing); see [RTT with a debug probe](#rtt-with-a-debug-probe).
   The counters have no other output: no USB interface, OLED screen, or flash
   record carries them.
2. Reproduce the problem. The first change after a quiet spell is logged at
   the next one-second housekeeping tick; while the counts keep changing,
   the next line waits until 60 seconds after the previous one
   (`DIAGNOSTICS_REPORT_INTERVAL_SECS`) and then carries the latest counts.
   Counts that stay the same are never logged, so a quiet log means nothing
   was counted.
3. Quote the last `diagnostics:` line with the boot lines. Each line has every
   count since boot, so the latest line is enough; its uptime timestamp
   places it against the failure.

The counts start at zero at every boot and are lost on reset, so read the
log before power-cycling a unit that misbehaves. Each count stops at
4294967295 instead of wrapping. The line is logged at `info`, so a build
whose `DEFMT_LOG` is `warn` or stricter does not print it. The Renode
simulation build does not run the BLE, USB, or flash shells that count, so
it never prints the line.

## Stack And Memory Checks

The memory map is fixed at link time ([hardware](hardware.md#memory-layout),
[ADR 0010](adr/0010-static-memory-layout.md)): the SoftDevice owns the first
156 KiB of flash and 24 KiB of RAM, the application has 804 KiB of flash and
232 KiB of RAM, and pages 240–243 hold the pairing store. There is no heap.
These are reservations, not measurements. Measure them for each build that
matters and record the numbers with the artifact hash.

| Check | How | Healthy |
| --- | --- | --- |
| SoftDevice RAM | `softdevice RAM: N bytes` at boot | N at most 24576 |
| Stack high-water | `stack high-water: X of Y bytes`, logged when it grows | X well under half of Y after exercising the paths below |
| Flash and static RAM size | `mask size` (`cargo size -A` on the release `bt2usb`) | Code and read-only data within 804 KiB; static data within 232 KiB with room left for the stack |
| Largest code contributors | `mask bloat` (needs `cargo-bloat`) | No unexpected growth between builds |
| Self-test stack stage | `[PASS] stack: under half the stack region used` | Pass |

### Reading The Stack High-Water Mark

`cortex-m-rt`'s `paint-stack` feature fills the stack region with `0xCCCCCCCC`
at reset. Once a second, [stack.rs](../src/stack.rs) scans up from the bottom
for the first overwritten word: X is the deepest use since reset, and Y is the
size of the stack region (`_stack_end` to `_stack_start`, the top of RAM).
Every Embassy task and every application interrupt handler runs on this one
stack. Nordic documents that the SoftDevice's own handlers also use the
application's main stack; that is general Nordic guidance, not something this
project has measured. The number only covers what has run since reset, so exercise
the worst case before trusting it: two links with active typing and mouse
movement, a user scan while a slot reconnects, OLED updates, and a flash save.
A reset clears the measurement.

The project does not use `flip-link`, because it would move `.data` away from
the RAM origin that the SoftDevice uses as the application RAM boundary. A
stack overflow therefore runs into static data instead of faulting, and can
corrupt state silently. Treat a high-water mark above half of Y as a defect to
investigate, not as headroom.

### Changing The Memory Map

- If the SoftDevice needs more RAM, follow the
  [RAM panic incident](#boot-panics-while-enabling-the-softdevice).
- The "giving more RAM to the softdevice than needed" warning prints the RAM
  start that would release the rest. Do not shrink the reservation based on one
  boot; the
  [memory budget task](../TODO.md#platform-memory-and-recovery) requires
  measurements with two links, scanning, display, and persistence.
- `memory_sd.x`, `STORAGE_FLASH_PAGE_START`/`COUNT` in
  [config.rs](../src/config.rs), and the hardware guide change together. The
  linker refuses code in the pairing pages, fails when `FLASH` does not end
  where the page constants put the store, and asserts the stack and `.data`
  placement.

## Saved Devices And Storage

To forget a device or clear all peers, use the saved-device menu described in
[features](features.md#manage-saved-devices). A success notice appears only
after storage commits; a failure preserves cached records/bonds and reports an
error. Targeted connections are stopped before the write, so a failed operation
can leave a previously saved device disconnected. Retry or reconnect explicitly.

A full-chip erase destroys stored keys and SoftDevice, and requires pairing
again after reinstallation. It is a recovery/provisioning action, not a routine
update. The UI's Factory reset clears the pairing store without reinstalling the
application or SoftDevice. An unreadable store is erased only by this explicit
reset path. Logical deletion or overwriting a record is not evidence of physical
key erasure. Storage migration and downgrade compatibility must be evaluated
before restoring an older version; see
[storage compatibility](deployment.md#storage-compatibility).

The store holds at most four peers. Pairing a fifth evicts the one added
earliest, and at boot the two most recently added peers with a bond get the
connection slots. Stored keys are never backed up or exported; the only way to restore a
lost pairing is to pair again.

To replace a peer's keys, for example after suspected exposure, Forget it on the
bridge, remove the bridge from the peripheral's own pairing list where the
peripheral allows it, and pair again. Bond keys are stored unencrypted in
internal flash; see [key storage](security.md#key-storage-and-deletion).

## Reporting A Defect

For a defect report, record the artifact hash, exact firmware version, board,
peripheral, host/hub, reproduction sequence, and relevant log interval. The
first two boot lines carry the version, the source commit, the profile, the
`DEFMT_LOG` filter, and the reset reason ([boot sequence](#boot-sequence)), so
quote them rather than retyping the build details. Add the last
`diagnostics:` line logged before the failure or after reproducing it
([event counters](#event-counters)). Sanitize device
identifiers and never share bond keys or private input captures publicly.
Suspected vulnerabilities follow the [security policy](../SECURITY.md) instead.

Use the [bug report template](../.github/ISSUE_TEMPLATE/bug.md), which asks
for the same environment fields. For a board run, the
[hardware result template](../.github/ISSUE_TEMPLATE/hardware-result.md)
records a completed first-flash checklist. A useful log interval starts at
`bt2usb firmware starting` and runs past the failure; note the build's
`DEFMT_LOG` level and the uptime timestamps of the failing steps.

At `info` and `debug`, the firmware's own lines carry device names and slot
numbers but no key material, addresses, or keystroke content. At `trace`, the
vendored SoftDevice wrapper logs one
`GATT_HVX write handle={:?} type={:?} len={}` line per HID notification, whose
timestamps record when keys go down and up. Only a build with the
`log-sensitive-data` feature adds the peer's BLE address on connect
(`connected role={:?} peer_addr={:?}`, at `debug`) and each notification's raw
bytes (`data={:?}`, at `trace`), which are the user's keystrokes. Never use a
`trace` build, or one with that feature, on a unit used for real typing
([logging and privacy](security.md#logging-and-privacy)). Mask addresses,
device names, and the USB serial, remove any `trace` output and any output
added locally for debugging, and follow the checklist in the bug template
before sharing a log.

## Related Guides

- [First flash](first-flash.md)
- [Deployment](deployment.md)
- [Features](features.md)
- [Hardware](hardware.md)
- [Architecture and ADRs](architecture.md)
- [Data model](data-model.md)
- [Testing](testing.md)
- [Development](development.md)
- [Security](security.md)
- [Task reference](../maskfile.md)
