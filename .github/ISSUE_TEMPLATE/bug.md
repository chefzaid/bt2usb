---
name: Bug report
about: Firmware behaves differently from the documentation
labels: bug
---

> Suspected vulnerability? Do not file it here; follow [SECURITY.md](https://github.com/chefzaid/bt2usb/blob/main/SECURITY.md).

## Summary

Describe the observed behavior and its impact.

## Environment

- Firmware commit or release tag, and ELF/HEX SHA-256:
- Board and revision, pin changes, power arrangement:
- SoftDevice version:
- BLE peripheral make, model, and firmware:
- Host OS and version; monitor/hub model if used:

## Reproduction

1. Give the smallest sequence that reproduces it.
2. Note whether it reproduces in host tests or Renode.

## Expected behavior

Quote the guide or checklist step that describes the intended result.

## Evidence

Attach the sanitized RTT log around the failure.

- [ ] No bond keys (LTK/IRK), raw flash dumps, or private keystrokes are attached.
- [ ] A regression test is identified, or the reason one is not possible is stated.
