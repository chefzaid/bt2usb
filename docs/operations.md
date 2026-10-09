# Operations Runbook

This runbook covers a flashed bt2usb unit: what to look at, how to recover it,
and how to report a defect. Use it with the [first-flash checklist](first-flash.md)
for new boards, the [deployment guide](deployment.md) for updates and release
gates, and the [security reference](security.md) for key-handling limits.
Firmware builds are development artifacts until the applicable release gates
have evidence attached.

## Runtime Surfaces

| Surface | Where | What it shows |
| --- | --- | --- |
| RTT log | Debug probe, `mask run --release` | `defmt` boot, BLE, USB, storage, and stack high-water logs |
| OLED | On the device | Current screen, connected devices, retained errors and notices |
| USB enumeration | Host device manager or `lsusb` | Keyboard, mouse, and consumer interfaces; per-unit serial |
| Self-test | `mask selftest` | Staged `[PASS]`/`[FAIL]` results for each peripheral |
| Simulation UART | Renode `mask sim` | Logs from the SoftDevice-free build; no probe needed |

## First Checks After Flashing

1. The log shows `SoftDevice started`, `USB HID device started`, and no panic.
2. `USB configured by host: true` appears and the host lists the device.
3. The OLED shows the Home screen.
4. Saved peers reconnect, and a held key is released when its link drops.

Anything beyond that is covered by the [first-flash checklist](first-flash.md).

## Recovery And Diagnostics

| Symptom | First checks |
| --- | --- |
| Probe missing | Data cable, power, probe permissions; WSL attachment; `mask probe-list` |
| No firmware boot | Matching ELF, SoftDevice installed, RAM boundary log, correct native board target |
| No USB device | Native USB port, data cable, direct host connection, `USB configured by host` log |
| No OLED | Common ground, 3.3 V, pin mapping, module address, I2C error/retry logs; input bridge tasks remain independent |
| No scan result | BLE HID/HOGP peripheral in pairing mode, nearby radio, scan completion |
| Reconnect fails | Peripheral awake, bond state on both sides, sanitized security/disconnect log |
| Newly paired peer is not remembered | Visible storage failure and flash logs; an invalid/unsupported store is preserved until an explicit Factory reset |
| Host will not wake | Hub remains powered, host wake permission, suspend/wakeup logs |

Keep an independent keyboard available during debugging. If firmware hangs,
reset or power-cycle the board, then reflash a known-working application using a
probe. If SoftDevice was erased, reinstall it before the application. The
firmware does not yet provide a watchdog recovery guarantee.

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
before restoring an older version.

## Reporting A Defect

For a defect report, record the artifact hash, exact firmware version, board,
peripheral, host/hub, reproduction sequence, and relevant log interval. Sanitize
device identifiers and never share bond keys or private input captures publicly.
Suspected vulnerabilities follow the [security policy](../SECURITY.md) instead.

## Related Guides

- [First flash](first-flash.md)
- [Deployment](deployment.md)
- [Features](features.md)
- [Hardware](hardware.md)
- [Security](security.md)
