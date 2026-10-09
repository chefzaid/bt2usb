# First Flash: Bring-Up Checklist

Work through this top to bottom the first time bt2usb goes onto a new board.
Each step says what to look for, so a problem shows up at the stage that
causes it instead of as "the keyboard doesn't work".

Host tests (`mask test`), embedded build/lint checks, and the Renode simulation
(`mask sim-test`) cover shared logic and selected simulated paths. Real radio,
USB, flash behavior, I2C wiring, power, timing, and interoperability still need
this board checklist. See [Testing](testing.md) for the coverage boundaries.

Tick each box as you go. Write down the numbers the log gives you (SoftDevice
RAM, stack high-water); they feed back into the configuration.

Save a separate copy of the completed checklist with the firmware commit/tag,
ELF hash, board revision, SoftDevice version, peripheral models, host OS, and
monitor/hub model. Mark skipped checks explicitly. Expected results below are
acceptance targets, not a claim that every device combination has passed.
[Recording The Result](#recording-the-result) explains where the copy goes.

When a step fails, fix it before moving on. The symptom table in the
[operations runbook](operations.md#recovery-and-diagnostics) lists the first
checks for a missing probe, no boot, no USB device, no OLED, no scan result,
failed reconnects, and a host that will not wake.

## Reading The Logs

Every image in this checklist logs over RTT through the debug probe.
`mask selftest` and `mask run --release` flash the image and stay attached to
show the log; `mask rtt` attaches to an already running release build. The `defmt`
logger adds a level and an uptime timestamp in front of each message, so look
for the message text quoted here, which is copied from the source. Capital
letters such as N, X, Y, P, F, and S, and placeholders such as NAME, stand for
the values the source formats with `{}`; for example the source's
`==== self-test done: {} passed, {} failed, {} skipped ====` is quoted as
`==== self-test done: P passed, F failed, S skipped ====`. The
[log message reference](operations.md#log-message-reference) explains each
message the bridge can print.

Local builds log at `debug` level (`DEFMT_LOG` in
[.cargo/config.toml](../.cargo/config.toml)); CI builds release artifacts at
`info`. Every line quoted below is logged at `info` or higher, so it appears in
both. Self-test `[PASS]` lines are `info`; `[FAIL]` and `[SKIP]` lines are
`warn`.

## 0. Before you start

- [ ] `mask ci` passes on your machine (format check, host, embedded and
      simulation Clippy, host tests, release firmware and simulation builds).
- [ ] The board is an nRF52840-DK (or another nRF52840 with USB wired out).
      The pins below are the DK defaults; they are instantiated in
      `src/main.rs` and `src/selftest.rs` and documented in `src/config.rs`.
- [ ] Two USB cables: the **debugger** port (J2 on the DK) for flashing and
      logs, and the **nRF USB** port (J3, labelled "nRF USB") that the PC will
      see as the keyboard/mouse. During bring-up, plug the nRF USB port straight
      into the PC; move it to the monitor hub at step 5.
- [ ] Wiring:

  | Part          | Pin    | Other side |
  | ------------- | ------ | ---------- |
  | OLED SDA      | P0.26  |            |
  | OLED SCL      | P0.27  |            |
  | OLED VCC      | VDD    | The board's I/O rail; never 5 V |
  | OLED GND      | GND    | Common ground with the board |
  | Button UP     | P0.11  | GND        |
  | Button DOWN   | P0.12  | GND        |
  | Button SELECT | P0.24  | GND        |

  Connect OLED VCC to VDD (the board's I/O rail; never 5 V), so any pull-ups
  on the module pull SDA and SCL to the nRF52840's I/O voltage; its GPIOs are
  not 5 V tolerant. See [Wiring](hardware.md#wiring) and
  [OLED Display](hardware.md#oled-display) in the hardware reference.

  On the DK, P0.11/P0.12/P0.24 are also wired to the on-board buttons 1, 2
  and 3, so those work as UP/DOWN/SELECT without extra switches. The firmware
  enables the internal pull-ups on the buttons and on SDA/SCL.

- [ ] The OLED module answers at I2C address 0x3C. The self-test probes only
      0x3C, and the firmware builds its display interface with the ssd1306
      crate's default-address constructor (`I2CDisplayInterface::new` in
      `src/ui/display.rs`), so a module strapped to 0x3D needs a code change.
- [ ] `mask probe-list` shows the probe. If it does not, see the probe row of
      the [recovery table](operations.md#recovery-and-diagnostics).

## 1. SoftDevice (once per board)

Obtain `s140_nrf52_7.3.0_softdevice.hex` from Nordic's S140 v7.3.0 distribution
and place it in the project root, or substitute its full path in the command.
Record the source/version and archive hash with the test evidence.

```bash
probe-rs download s140_nrf52_7.3.0_softdevice.hex --chip nRF52840_xxAA --format hex
```

`mask softdevice` does the same in one step: if the hex file is not already in
the project root, it downloads `s140_nrf52_7.3.0.zip` from the Nordic URL in
[maskfile.md](../maskfile.md), extracts the hex, and flashes it. The recipe does
not verify a checksum, so record the archive hash yourself. The
[SoftDevice installation](deployment.md#softdevice-installation) section of the
deployment guide covers provenance for releases.

- [ ] The download finishes without errors.

## 2. Self-test image

```bash
mask selftest
```

This flashes `bt2usb-selftest`, which uses the same SoftDevice settings, USB
device and pins as the real firmware and reports each stage over RTT. It never
types on the PC: the only HID report it sends is an all-zero mouse report.

The stages always run in this order (`src/selftest.rs`). The longest waits are
USB enumeration (up to 10 s), each button prompt (up to 20 s), and the BLE scan
(8 s).

| Stage | Log lines to look for | Good | If it fails |
| ----- | --------------------- | ---- | ----------- |
| Start | `==== bt2usb self-test ====` | Printed first | No output at all: check the probe connection and that step 1 completed |
| SoftDevice | `softdevice RAM: N bytes`, then `[PASS] softdevice: enabled (…)` | N is at most 24576 (0x6000) | A panic saying `too little RAM for softdevice. Change your app's RAM start address to X`: set `RAM : ORIGIN` in `memory_sd.x` to `0x` followed by X (printed in hex without a prefix) and shrink `LENGTH` by the same amount. Other panics in `Softdevice::enable`, such as `sd_ble_enable err …` or `selected configuration has too high RAM requirements.`, also stop here. No `[PASS] softdevice` line at all usually means the SoftDevice isn't flashed (step 1) or is the wrong version |
| Flash | `flash: no saved pairings yet` or `flash: saved pairing record present (N bytes)`, then `[PASS] flash: write, read-back and remove OK` | Write, read-back and remove all OK; saved pairings are only read | `[FAIL] flash:` with `reading the pairing region failed`, `write failed`, `read-back didn't match what was written`, or `remove failed`. The pairing region (pages 240–243, `0x000F0000–0x000F4000`, end exclusive) can't be used; check nothing else uses it on this board |
| USB enumeration | `[PASS] usb enumeration: configured by the PC` | The PC lists "BT-to-USB HID Bridge" from "bt2usb" | `USB: no VBUS yet; plug the nRF USB port (not the debugger port) into the PC`, then `[FAIL] usb enumeration: not configured within 10 s: …` and `[SKIP] usb hid report: needs enumeration`. Wrong port, a charge-only cable, or a PC that blocks new USB devices |
| USB report | `[PASS] usb hid report: idle mouse report accepted` | Idle mouse report accepted | `[FAIL] usb hid report: host never polled the endpoint (1 s)`: enumerated, but the host did not poll the mouse interface within 1 s. `[FAIL] usb hid report: endpoint write failed`: the endpoint returned an error. For either, check the PC's device manager for a driver error |
| OLED ACK | `[PASS] oled i2c: SSD1306 acknowledged at 0x3C` | The display acknowledges its address | `[FAIL] oled i2c: no ACK at 0x3C: check SDA=P0.26, SCL=P0.27, VCC, GND (or a 0x3D module)`. A preceding `OLED I2C stalled; requesting STOP, display task degraded until DMA completes` means a transfer did not finish within 500 ms; SDA or SCL may be shorted or held low |
| OLED render | `[PASS] oled render: initialization and Home framebuffer sent` | The Home screen appears: `bt2usb / Idle`, `SELECT: scan`, `UP: saved devices` | `[FAIL] oled render: SSD1306 initialization failed` or `framebuffer transfer failed`. Runs only after the ACK stage passes |
| Buttons (×3) | `>>> press button UP (P0.11) now (20 s)`, then `[PASS] button UP (P0.11): press detected`; the same for `button DOWN (P0.12)` and `button SELECT (P0.24)` | Each press is seen when prompted | `[FAIL] … reads pressed at rest: shorted to GND or wrong pin` (no prompt follows). `[SKIP] … no press seen: check wiring to GND`: no press arrived in 20 s |
| BLE scan | `BLE: scanning 8 s; …`, one `BLE: HID device 'NAME' (RSSI -NN)` per HID device, `BLE: N advertisements, M HID devices`, then `[PASS] ble scan: radio receives advertisements` | Some advertisements heard; HID devices listed | `[FAIL] ble scan: heard nothing in 8 s: check the antenna, or test near any BLE device`, or `SoftDevice refused to scan`. Put a keyboard in pairing mode during the scan to see it listed by name |
| Stack | `stack high-water: X of Y bytes`, then `[PASS] stack: under half the stack region used` | X is less than half of Y | `[FAIL] stack: over half the stack region used` |
| Summary | `==== self-test done: P passed, F failed, S skipped ====` | `11 passed, 0 failed, 0 skipped` | Fix the first failure before going on |

- [ ] Recorded: SoftDevice RAM = ______ bytes. If it is below 24576,
      nrf-softdevice also logs `You're giving more RAM to the softdevice than
      needed. You can change your app's RAM start address to X`. Tightening
      `memory_sd.x` is optional (there's ample RAM), but the value informs the
      memory-budget task in [TODO.md](../TODO.md); do not shrink the
      reservation on this one measurement alone.
- [ ] Self-test: 0 failed.

## 3. Real firmware

```bash
mask run --release
```

The main task logs these lines in order before it first waits, so no other
task's output comes between them:

1. `bt2usb firmware starting`
2. `softdevice RAM: N bytes`, logged by nrf-softdevice; the same value as in the
   self-test
3. `USB power: vbus=true ready=true` (`false` until the nRF USB port is powered)
4. `USB HID composite device initialised (keyboard + mouse + consumer)`
5. `SoftDevice started`
6. `USB HID device started`
7. `BLE task started`
8. `UI and isolated OLED tasks started`

The spawned tasks log afterwards, in an order set by the executor, for example
`USB device task started`, `HID dispatcher and three endpoint workers started`,
`OLED initialized/recovered`, and `USB configured by host: true`. The
[boot sequence](operations.md#boot-sequence) in the operations runbook shows
the full healthy log.

The storage load reports `No paired devices in flash` or
`Loaded N devices from flash`. Either
`Invalid or unsupported device store; writes disabled` or
`Flash read error: …` means the store was not accepted; see
[saved devices and storage](operations.md#saved-devices-and-storage).

- [ ] Log shows `SoftDevice started`, `USB HID device started`,
      `UI and isolated OLED tasks started`, and `OLED initialized/recovered`,
      with no panic.
- [ ] `USB configured by host: true` appears, and the PC lists the device
      (keyboard, mouse and consumer-control interfaces).
- [ ] The OLED shows the Home screen.
- [ ] `stack high-water: X of Y bytes` lines appear; the firmware logs a new one
      whenever the high-water mark grows.

## 4. Pairing and daily use

Do these with the nRF USB port still plugged straight into the PC. Useful log
lines: `BLE scan starting (8 s window)`, `BLE scan complete - N devices found`,
`slot N connecting to NAME`, and
`Subscribed to N of M HID report characteristics`. A peer whose link cannot be
encrypted logs `slot N failed to secure BLE link` and is not used. Pairing is
currently unauthenticated Just Works bonding; see
[ADR 0011](adr/0011-interim-just-works-pairing.md) and the
[security reference](security.md#pairing-and-authentication).

- [ ] **Pair a keyboard.** Put it in pairing mode, press SELECT to scan,
      pick it with UP/DOWN and press SELECT. The OLED shows it connected and
      typing reaches the PC.
- [ ] **Pair a mouse** the same way (SELECT on the Connected screen starts
      another scan). Both keep working at once, and the OLED shows "2 devices".
- [ ] **Media keys** (volume, play/pause) work, if the keyboard has them.
- [ ] **Five-button mouse / horizontal scroll:** each supported extra button and
      scroll direction works; mark unsupported peripheral features as skipped.
- [ ] **Caps Lock LED:** press Caps Lock; the keyboard's own LED follows
      (for keyboards that have one and accept LED writes over BLE). The log
      shows `Host LEDs: num=… caps=… scroll=…`. The bridge must have logged
      `Found keyboard LED output report` when the keyboard connected; a failed
      write logs `Failed to write LED state to BLE keyboard`.
- [ ] **Lock keys after a reconnect:** turn Caps Lock on, then leave the
      keyboard idle until it sleeps (or switch it off and on). Wake it with a
      key other than a lock key. Once it reconnects, its Caps Lock LED is lit
      again without pressing anything, because the bridge writes the host's
      current state to every keyboard link when it starts.
- [ ] **Connection parameters:** for each peripheral, record the
      `peer connection parameters granted: …` or
      `peer asked for connection parameters …; granting …` lines, if any.
      Typing, mouse movement, and the Caps Lock LED stay responsive afterwards,
      and the link stays up for at least five minutes; a warning ending in
      `outside its interval range` names a peripheral that may disconnect.
- [ ] **Reboot reconnect:** press the DK's reset button. Both devices come
      back without re-pairing and without a scan screen. The log shows a
      `slot N connecting to NAME` line for each, and possibly
      `slot N scan found slot M's device` when one slot heard the other's
      device first; connection procedures run one at a time.
- [ ] **Sleep reconnect:** leave the keyboard idle until it sleeps (or switch
      it off), then press a key (or switch it on). The log shows
      `slot N link lost; reconnecting`, then it reconnects by itself and
      typing works. Nothing needs pressing on bt2usb.
- [ ] **Absent at boot:** switch the keyboard off, reset the board, wait
      30 s, and switch the keyboard on. It connects by itself. Repeat with the
      mouse off instead: the keyboard connects and types within a few seconds
      of the reset, without waiting for the mouse.
- [ ] **Saved device that will not connect:** pair the saved mouse with
      another computer (or clear its pairings) so it advertises but refuses
      the bridge, switch the keyboard off, reset the board, and switch the
      keyboard on. The log repeats `slot N failed to secure BLE link` or
      failed attempts for the mouse, and the keyboard still connects within
      a few seconds of advertising. Record the time.
- [ ] **No stuck keys:** hold a key down, and while holding it switch the
      keyboard off (or pull its battery). Within about 4 s the key stops
      repeating on the PC. The 4 s is the BLE supervision timeout
      (`BLE_SUP_TIMEOUT`), which a peripheral cannot lengthen; when the link
      closes, the bridge releases everything that slot was holding.
- [ ] **No stuck mouse/media input:** repeat link-loss tests while holding a
      mouse button and a consumer-control key. Record release latency.
- [ ] **Scan while reconnecting:** with one device switched off (so its slot
      is retrying), press SELECT to scan. The scan still runs after queued radio
      procedures complete and lists nearby devices. Record the observed delay;
      each connection attempt and reconnect scan has a 6-second timeout
      (`BLE_CONNECT_TIMEOUT_SECS`), retries pause 500 ms between attempts, and
      contention between slots can add to the total wait.
- [ ] **Stack:** after all of the above, the latest `stack high-water` line is
      well under half of the total. Recorded: ______ bytes.

## 5. In the monitor

Move the nRF USB cable from the PC to the monitor's USB hub (with the
monitor's upstream USB cable going to the PC). The debugger can stay
connected for logs.

- [ ] The PC enumerates the device through the hub, and the paired devices
      reconnect and type.
- [ ] **Unplug/replug:** unplug the nRF USB cable for a few seconds and plug
      it back in, with the board kept powered from the debugger. The log shows
      `USB configured by host: true` again, and typing works.
- [ ] **Input during USB outage:** hold/release keys and mouse buttons during
      the outage, then reconnect USB. The host has no lingering held input.
      Repeat with both BLE links active and sustained mouse movement. The log
      may show `USB HID endpoint unavailable; retaining current input state`.
- [ ] **PC sleep and wake:** put the PC to sleep and press a key on the BLE
      keyboard. The log shows `Power: usb_suspended=true`, then
      `USB remote wakeup sent`, and the PC wakes. (If it shows
      `USB remote wakeup not possible: …` instead, the OS hasn't allowed this
      device to wake the PC: on Windows, Device Manager → the keyboard → Power
      Management → "Allow this device to wake the computer".)
- [ ] **Wake filtering:** mouse movement/scroll, releasing an already-held input,
      and BLE disconnect cleanup do not wake the host. A newly pressed mouse
      button or consumer-control key requests wake when permitted by the host.
- [ ] **Independent USB interfaces:** with an unpolled or fault-injected endpoint,
      the other interfaces continue delivering input. When the endpoint becomes
      available again, current held/released state is restored without replaying
      mouse motion. Record the USB capture and observed recovery time.
- [ ] **BIOS / boot menu:** reboot the PC and enter its firmware setup with
      the BLE keyboard. This works once the keyboard has reconnected after the
      board powers up, which is when the monitor powers its hub.
- [ ] **Cold start:** with the PC and the monitor both off and the keyboard
      awake, power them on together (or let the PC's power-on switch the hub
      on), and press the firmware setup key (F2, Del, or the PC's own) about
      twice a second from power-on. Firmware setup opens. Record the time from
      power-on to the keyboard's `slot N connecting to NAME` and
      `HID notification loop started` lines, and repeat with the mouse switched
      off.
- [ ] **Monitor off and on:** turn the monitor off and on. The board
      power-cycles with the hub, then the devices reconnect by themselves.
- [ ] **Unit identity:** record the 16-character USB serial. It remains the same
      after reflash and changing ports; if a second board is available, its
      factory-derived serial differs.

## 6. Device management and degraded display

Run destructive pairing tests on test devices whose records can be recreated.
These are logical management checks; they do not establish physical key erasure.
UP opens the "Saved devices" list from Home, Connected, Error, or a notice; the
list ends with a "Factory reset" entry, and UP on the first entry goes back.

- [ ] **Cancel by default:** press UP from Home/Connected/Error, choose a saved
      device, then press SELECT twice. The first press opens the
      "Forget device?" confirmation and the second chooses the default Cancel;
      the record remains.
- [ ] **Forget one peer:** open its confirmation, press DOWN to choose Forget,
      then SELECT. The affected link/input state clears, the "Device forgotten"
      notice stays until acknowledged, and other saved peers remain. Reset the
      board and confirm the forgotten device does not reconnect until
      explicitly paired again.
- [ ] **Factory reset:** choose the final "Factory reset" entry, then DOWN and
      SELECT in its "Reset all pairings?" confirmation. After the
      "Pairings reset" notice, reset the board (the firmware does not reboot by
      itself); no prior peer auto-reconnects. Re-pair the test keyboard/mouse
      afterwards.
- [ ] **Failure reporting:** inject a flash write failure in a controlled test.
      The UI retains a storage error and does not report deletion/enrollment
      success. Record cached/persistent state and behavior after reboot. No
      hook exists yet to make a flash write fail on a board (the host tests in
      `ble/management.rs` inject failures only into the pure `commit`
      function), so record this check as `skip: no fixture` until one exists;
      [Power-loss-safe persistence](../TODO.md#pairing-storage) tracks the
      flash fault-injection work.
- [ ] **Two sources sharing an endpoint:** if available, hold overlapping keys
      or mouse buttons on two devices. Releasing/disconnecting one preserves the
      other's held state. More than six unique keyboard keys produces rollover;
      consumer input follows lowest-active-slot priority. Record supported pairs.
- [ ] **OLED failure isolation:** with power off, disconnect the test display,
      then boot with previously paired peripherals. BLE/USB input still works
      and the log shows
      `OLED operation failed; retry in N ms (bridge remains active)`, with
      retries at 1, 2, 4, 8, and 16 s, then every 30 s. Reconnect wiring only
      with power off, then verify normal display behavior after boot. Live-bus
      recovery requires a controlled fault-injection fixture and a separate
      result record.

## 7. Afterwards

- [ ] If the SoftDevice RAM value let you tighten `memory_sd.x`, commit that
      with the recorded memory/stack margin evidence, and update the applicable
      [TODO.md](../TODO.md) task.
- [ ] Note anything that didn't behave as described here in an issue, with
      the sanitized RTT log around it. Do not include bond keys or private input.
      Check the [recovery table](operations.md#recovery-and-diagnostics) first
      for known causes.
- [ ] Archive the completed result record (see
      [Recording The Result](#recording-the-result)); leave this template
      unchecked for the next board or release. Review remaining
      [release gates](deployment.md#release-gates).

## 8. Extended Acceptance (Optional)

These checks go beyond bring-up: each one supplies evidence for an open
hardware item in [TODO.md](../TODO.md). Run them when the equipment is at
hand, and mark any you do not run as `skip: <reason>`.

- [ ] **Pairing region survives reflash:** with at least one peer saved and
      reconnecting, flash the bridge again with `mask run --release`. The
      Cargo runner is `probe-rs run --chip nRF52840_xxAA` with no erase option
      ([.cargo/config.toml](../.cargo/config.toml)). Record `probe-rs --version`
      and the erase mode used, then confirm the saved peers reconnect without
      pairing again (`Loaded N devices from flash`, then a
      `slot N connecting to NAME` line per peer). The expected erase behavior is
      in [deployment](deployment.md#erase-behavior);
      [Pairing region survives application reflash](../TODO.md#pairing-storage)
      closes with this result.
- [ ] **Supply current:** measure the current the board draws from the nRF
      USB supply while idle, while scanning, with two links active, with the
      OLED on and off, and while the host has suspended USB (the bridge keeps
      its BLE links and radio active then). Record the meter, and whether the
      debugger cable was connected, since the DK can draw from either USB
      connector. Compare the figures with the 100 mA the configuration
      descriptor declares (`max_power` in `src/usb/hid_device.rs`) and with the
      USB suspend-current limit. See [Power](hardware.md#power);
      [Power budget and USB suspend current](../TODO.md#ui-display-and-power)
      closes with this result.
- [ ] **USB stability without an HFXO request:** the application never
      requests the high-frequency crystal oscillator (there is no
      `sd_clock_hfclk_request` call), and how the USB peripheral's clock need is
      met under the SoftDevice has not been checked on hardware
      ([Clocks And Radio](hardware.md#clocks-and-radio)). Across the
      sections above, confirm the device never drops off the bus or
      re-enumerates unexpectedly, including during scans and with two links
      active, and record any USB errors the host logs.

## Recording The Result

Open a GitHub issue from the **Hardware acceptance result** template
([hardware-result.md](../.github/ISSUE_TEMPLATE/hardware-result.md)). It
suggests the title `Hardware result: <board> / <firmware commit or tag>`,
labels the issue `hardware-evidence`, and has these parts:

- **Build:** commit or tag, ELF SHA-256 (and HEX SHA-256 for a release
  package), Rust version, build profile and `DEFMT_LOG` level, SoftDevice
  version and archive SHA-256.
- **Setup:** board and revision, pin changes, supply, debug probe; peripheral
  make, model, and firmware; host OS and version, BIOS/UEFI, monitor and hub.
- **Measurements:** the `softdevice RAM` value, the stack high-water after
  section 4, and reconnect and release timings from sections 4 and 5.
- **One table per section 0–8** of this checklist. Fill in the tables: write
  `pass`, `fail`, or `skip: <reason>` in every Result cell, and quote the
  supporting log line or measurement in Notes. The section 8 table also has
  cells for the probe-rs version and erase mode and for each current reading.
- **Sanitizing:** confirm that no bond keys (LTK/IRK), raw flash dumps, or
  private keystrokes are attached, that device names, BLE addresses, and the
  USB serial are masked where they identify a person, and that no log from a
  `trace`-level build is attached.

An empty or skipped cell is unverified, not a pass; section 8 is optional, so
mark the checks you did not run as `skip: <reason>` rather than leaving them
empty. File each deviation as its own issue and link it from section 7.
[Hardware acceptance evidence](testing.md#hardware-acceptance-evidence) in the
testing guide explains how these records feed release review and the hardware
gates in [TODO.md](../TODO.md). Report a suspected vulnerability privately
under the [security policy](../SECURITY.md), not in this issue.

## Related Guides

- [Hardware](hardware.md)
- [Testing](testing.md)
- [Operations](operations.md)
- [Deployment](deployment.md)
- [Features](features.md)
- [Development](development.md)
