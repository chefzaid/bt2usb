# ADR 0004: Verify In Layers, From Host Tests To Hardware Acceptance

- Status: Accepted
- Date: 2026-10-09

## Context

The goal is a board that works the first time it is flashed. No single check
can establish that: host tests cannot see radios, an emulator cannot run the
SoftDevice, and a board check covers only the peripherals on the bench.

## Decision

Verify every change through layers that each own a distinct failure class:

1. Host unit and integration tests for the shared pure modules.
2. Embedded format, Clippy with warnings denied, and release builds of the
   firmware and self-test, including linker-script assertions on the memory map.
3. A SoftDevice-free `sim` build run headless in Renode with custom
   GPIO/GPIOTE models, so real button edges drive the real button task, UI
   reducer, and BLE coordinator.
4. The `bt2usb-selftest` image, which brings up SoftDevice, flash, USB, OLED,
   buttons, and scanning stage by stage on a real board.
5. The [first-flash checklist](../first-flash.md) for pairing, reconnect,
   held-input release, monitor hubs, sleep and wake, and pre-OS use, recorded
   as a dated result.

CI runs layers 1–3 and a dependency audit on pushes, pull requests, and a
weekly schedule. Layers 4–5 need a board and a person.

## Rationale

Each layer is cheap where the previous one is blind. Recording which layers ran
for a change keeps claims honest: a feature can be implemented and
software-verified while still unverified on hardware.

## Consequences

- `embedded` and `sim` are mutually exclusive features; `--all-features` is
  rejected by the build script.
- Validation records list the exact layers and versions used, and skipped
  layers are reported as skipped, not passed.
- Hardware release gates in [TODO.md](../../TODO.md) stay open until layer 5
  evidence exists.
