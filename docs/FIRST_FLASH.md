# First flash: bring-up checklist

Work through this top to bottom the first time bt2usb goes onto a new board.
Each step says what to look for, so a problem shows up at the stage that
causes it instead of as "the keyboard doesn't work".

Everything except BLE radio behaviour and USB enumeration is already covered
without hardware: host tests (`mask test`), the embedded build and clippy, and
the Renode simulation (`mask sim-test`). This list covers what only real
silicon can show.

Tick each box as you go. Write down the numbers the log gives you (SoftDevice
RAM, stack high-water); they feed back into the configuration.

## 0. Before you start

- [ ] `mask ci` passes on your machine (format, host tests, embedded build).
- [ ] The board is an nRF52840-DK (or another nRF52840 with USB wired out).
      The pins below are the DK defaults from `src/config.rs`.
- [ ] Two USB cables: the **debugger** port (J2 on the DK) for flashing and
      logs, and the **nRF USB** port (J3, labelled "nRF USB") that the PC will
      see as the keyboard/mouse. During bring-up, plug the nRF USB port straight
      into the PC; move it to the monitor hub at step 5.
- [ ] Wiring:

  | Part          | Pin    | Other side |
  | ------------- | ------ | ---------- |
  | OLED SDA      | P0.26  |            |
  | OLED SCL      | P0.27  |            |
  | OLED VCC      | VDD    | 3.3 V only |
  | Button UP     | P0.11  | GND        |
  | Button DOWN   | P0.12  | GND        |
  | Button SELECT | P0.24  | GND        |

  On the DK, P0.11/P0.12/P0.24 are also wired to the on-board buttons 1, 2
  and 3, so those work as UP/DOWN/SELECT without extra switches.

- [ ] `mask probe-list` shows the probe.

## 1. SoftDevice (once per board)

```bash
probe-rs download s140_nrf52_7.3.0_softdevice.hex --chip nRF52840_xxAA --format hex
```

- [ ] The download finishes without errors.

## 2. Self-test image

```bash
mask selftest
```

This flashes `bt2usb-selftest`, which uses the same SoftDevice settings, USB
device and pins as the real firmware and reports each stage over RTT. It never
types on the PC: the only HID report it sends is an all-zero mouse report.

| Log line                        | Good                                            | If it fails                                                                                                                                             |
| ------------------------------- | ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `softdevice RAM: N bytes`       | N is at most 24576 (0x6000)                     | A panic saying "too little RAM for softdevice. Change your app's RAM start address to X": set `RAM : ORIGIN = X` in `memory_sd.x` and shrink LENGTH by the same amount |
| `[PASS] softdevice`             | Printed                                         | Anything else means the SoftDevice isn't flashed (step 1) or is the wrong version                                                                       |
| `[PASS] flash`                  | Write, read-back and remove all OK              | The pairing region (pages 240–243) can't be written; check nothing else uses 0xF0000–0xF4000 on this board                                              |
| `[PASS] usb enumeration`        | The PC lists "BT-to-USB HID Bridge"             | Wrong port (use nRF USB, not the debugger), a charge-only cable, or a PC that blocks new USB devices                                                    |
| `[PASS] usb hid report`         | Idle mouse report accepted                      | Enumerated but the host never polls the mouse interface; check the PC's device manager for a driver error                                               |
| `[PASS] oled i2c`               | The Home screen ("bt2usb / Idle") appears       | "no ACK": check SDA/SCL, power, or whether the module is at 0x3D. "bus stuck": SDA or SCL shorted or held low                                            |
| `[PASS] button …` (×3)          | Each press is seen when prompted                | "reads pressed at rest": the pin is shorted to GND. `[SKIP]`: no press arrived in 20 s, so check the button's wiring                                     |
| `[PASS] ble scan`               | Some advertisements heard; HID devices listed   | Nothing heard: antenna or radio problem. Put a keyboard in pairing mode during the scan to see it listed by name                                        |
| `stack high-water: X of Y`      | X well under half of Y                          | —                                                                                                                                                       |
| `self-test done: N passed, 0 failed` | Zero failures                              | Fix the first failure before going on                                                                                                                   |

- [ ] Recorded: SoftDevice RAM = ______ bytes. If it is well below 24576,
      nrf-softdevice also prints the exact RAM start that would free the rest.
      Tightening `memory_sd.x` is optional (there's ample RAM), but the value
      closes the README roadmap item.
- [ ] Self-test: 0 failed.

## 3. Real firmware

```bash
mask run --release
```

- [ ] Log shows `SoftDevice started`, `USB HID device started`,
      `OLED display initialised`, `Entering UI main loop`, with no panic.
- [ ] `USB configured by host: true` appears, and the PC lists the device
      (keyboard, mouse and consumer-control interfaces).
- [ ] The OLED shows the Home screen.

## 4. Pairing and daily use

Do these with the nRF USB port still plugged straight into the PC.

- [ ] **Pair a keyboard.** Put it in pairing mode, press SELECT to scan,
      pick it with UP/DOWN and press SELECT. The OLED shows it connected and
      typing reaches the PC.
- [ ] **Pair a mouse** the same way. Both keep working at once, and the OLED
      shows "2 devices".
- [ ] **Media keys** (volume, play/pause) work, if the keyboard has them.
- [ ] **Caps Lock LED:** press Caps Lock; the keyboard's own LED follows
      (for keyboards that have one and accept LED writes over BLE).
- [ ] **Reboot reconnect:** press the DK's reset button. Both devices come
      back without re-pairing (the log shows slot 0 then slot 1 connecting,
      one after the other).
- [ ] **Sleep reconnect:** leave the keyboard idle until it sleeps (or switch
      it off), then press a key (or switch it on). The log shows
      `slot N link lost; reconnecting`, then it reconnects by itself and
      typing works. Nothing needs pressing on bt2usb.
- [ ] **Absent at boot:** switch the keyboard off, reset the board, wait
      30 s, and switch the keyboard on. It connects by itself.
- [ ] **No stuck keys:** hold a key down, and while holding it switch the
      keyboard off (or pull its battery). Within about 4 s the key stops
      repeating on the PC.
- [ ] **Scan while reconnecting:** with one device switched off (so its slot
      is retrying), press SELECT to scan. The scan still runs (it may start up
      to 6 s late) and lists nearby devices.
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
- [ ] **PC sleep and wake:** put the PC to sleep and press a key on the BLE
      keyboard. The log shows `USB remote wakeup sent` and the PC wakes.
      (If it shows "not possible" instead, the OS hasn't allowed this device
      to wake the PC: on Windows, Device Manager → the keyboard → Power
      Management → "Allow this device to wake the computer".)
- [ ] **BIOS / boot menu:** reboot the PC and enter its firmware setup with
      the BLE keyboard. This works once the keyboard has reconnected after the
      board powers up, which is when the monitor powers its hub.
- [ ] **Monitor off and on:** turn the monitor off and on. The board
      power-cycles with the hub, then the devices reconnect by themselves.

## 6. Afterwards

- [ ] If the SoftDevice RAM value let you tighten `memory_sd.x`, commit that
      and tick the README roadmap item.
- [ ] Note anything that didn't behave as described here in an issue, with
      the RTT log around it.
