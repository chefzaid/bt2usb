# Operations and releases

Use this guide with the [hardware checklist](FIRST_FLASH.md) and
[security limits](../SECURITY.md). Firmware builds are development artifacts until
the applicable release gates have evidence attached.

## Update a development unit

1. Record the installed version/commit, board revision, and known-working
   SoftDevice version. Keep the previous ELF and its checksum for recovery.
2. Review changed pin assignments, memory reservations, and storage format.
3. Build from the intended revision using the pinned toolchain and `--locked`.
4. Flash with `mask run --release`, keeping the debugger and native USB paths
   connected appropriately.
5. Check boot logs, enumeration, reconnect, input release, and the relevant
   hardware acceptance cases. Preserve the results with the artifact checksum.

The application image does not include SoftDevice. Install S140 v7.3.0 separately
on a blank board or after a full-chip erase. Normal application flashing should
preserve the reserved pairing region; verify the flashing tool's erase settings
and test this assumption for the selected workflow before relying on it.

## Recovery and diagnostics

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

To forget a device, press UP from Home/Connected/Error, select its saved entry,
and open the confirmation. Cancel is selected initially; DOWN selects Forget and
SELECT commits it. The final saved-list entry opens Factory reset, which clears
all peers using the same deliberate confirmation. A success notice appears only
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

For a defect report, record the artifact hash, exact firmware version, board,
peripheral, host/hub, reproduction sequence, and relevant log interval. Sanitize
device identifiers and never share bond keys or private input captures publicly.

## Release gates

The workflow builds on pushes/pull requests and runs scheduled dependency checks.
Tags matching the exact Cargo package version produce a **draft** release after
checks pass. Packaging reuses the successful embedded job's firmware rather than
rebuilding it. The package contains application ELF/HEX, a self-test ELF,
manifest/lockfile/toolchain inputs, `BUILD-INFO.json`, `SHA256SUMS`, and a signed
provenance bundle. Follow [Release provenance](RELEASING.md) to verify the expected
workflow, source commit, and downloaded bytes before flashing. The draft keeps
hardware/release review explicit; it is not evidence that those reviews passed.

An automated tag build alone is not production approval. Before publishing a
deployment release, reviewers should confirm:

- Reproducible source revision, pinned toolchain/dependencies, passing host,
  embedded, and Renode checks, and recorded build/size results.
- Hardware results for the supported peripheral/host/hub matrix, including
  sleep/wake, USB reconnect, link loss with held inputs, and both active slots.
- Security policy, pairing authentication/enrollment decision, key deletion and
  physical-access policy, and resolved or explicitly accepted security findings.
- Assigned USB identity, unit-unique identification, documented production
  hardware, memory and power margins, and a tested recovery procedure.
- Release notes listing supported versions, compatibility limits, migrations,
  rollback constraints, checksums, and the exact SoftDevice prerequisite.
- Dependency/license review, provenance/signing strategy, a security contact,
  ownership of support, and an update/support lifetime.

Open implementation and validation tasks live in [TODO.md](../TODO.md). Move an
item to completed only when its acceptance criteria are met; attach hardware or
release evidence rather than inferring it from compilation.
