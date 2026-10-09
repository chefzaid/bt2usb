---
name: Hardware acceptance result
about: Record a first-flash checklist run on a real board
labels: hardware-evidence
---

Use [docs/first-flash.md](https://github.com/chefzaid/bt2usb/blob/main/docs/first-flash.md) and paste the completed
checklist below. Mark every item pass, fail, or skipped with a reason; an
unchecked item is unverified, not a pass.

## Build

- Commit or tag:
- ELF SHA-256:
- Rust version and build profile:
- SoftDevice version and archive hash:

## Setup

- Board and revision, pin changes, supply, debug probe:
- Peripherals (make, model, firmware):
- Host OS and version, BIOS/UEFI, monitor and hub:

## Measurements

- SoftDevice RAM requirement:
- Stack high-water (bytes of total):
- Reconnect and release timings:

## Checklist results

Paste sections 0–7 here.

- [ ] Logs are sanitized: no bond keys, raw flash, or private keystrokes.
