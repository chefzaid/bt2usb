---
name: Bug report
about: Firmware behaves differently from the documentation
title: ""
labels: bug
assignees: ""
---

> Suspected vulnerability? Do not describe it here; follow [SECURITY.md](https://github.com/chefzaid/bt2usb/blob/main/SECURITY.md).
> Recovery steps for common symptoms are in the [operations runbook](https://github.com/chefzaid/bt2usb/blob/main/docs/operations.md#recovery-and-diagnostics).

## Summary

Describe the observed behavior and its impact.

## Environment

- Firmware commit or release tag, and ELF/HEX SHA-256:
- Boot lines `bt2usb firmware starting: ...` and `reset reason: ...`, copied from the log:
- The last `diagnostics: ...` line (event counters) before or after the failure, if the log has one:
- Build: local `mask`/`cargo` or a release package:
- Board and revision, pin changes, power arrangement:
- SoftDevice version:
- BLE peripheral make, model, and firmware, and its `slot N PnP ID: ...` line from the log:
- Host OS and version; monitor/hub model if used:

## Reproduction

1. Give the smallest sequence that reproduces it.
2. Note whether it reproduces in host tests (`mask test`) or Renode (`mask sim-test`).

## Expected behavior

Quote the guide or [first-flash](https://github.com/chefzaid/bt2usb/blob/main/docs/first-flash.md)
checklist step that describes the intended result.

## Evidence

Attach the sanitized RTT log around the failure, from the boot lines on if you
have them. Local builds log at `debug`, which includes device names; only a
build with the `log-sensitive-data` feature adds peer BLE addresses and
keystroke bytes, and such a log must not be shared; see
[logging and privacy](https://github.com/chefzaid/bt2usb/blob/main/docs/security.md#logging-and-privacy).

- [ ] No bond keys (LTK/IRK), raw flash dumps, or private keystrokes are attached.
- [ ] Device names, BLE addresses, and the USB serial are removed or masked.
- [ ] No log from a `trace`-level build is attached.
- [ ] A regression test is identified, or the reason one is not possible is stated.
