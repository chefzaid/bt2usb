# Testing Guide

Software checks and board checks cover different failure modes. Passing host
tests or Renode does not establish radio interoperability, USB compliance,
flash durability, security certification, or production readiness.

## Test Layers

Each layer owns a failure class the previous one cannot see
([ADR 0004](adr/0004-layered-verification.md)).

| Layer | What it exercises | What it cannot establish |
| --- | --- | --- |
| Host unit/integration tests | Shared HID parsing/serialization, reducers, advert parsing, power/UI rules, report coalescing, storage framing and record validation | Async driver timing and peripheral behavior |
| Embedded checks | Firmware/self-test compile and lint for the ARM target, linker reservations | Runtime correctness on hardware |
| Renode | SoftDevice-free ARM boot, GPIO/timer paths, UI/coordinator scenario | Actual BLE, USB, SoftDevice, or flash persistence |
| Board self-test | SoftDevice enable, flash access, USB enumeration/report, OLED, buttons, scan | Long-term reliability or the full peripheral matrix |
| Hardware acceptance | Pairing, reconnect, held-input release, monitor hub, sleep/wake, pre-OS operation | Untested host/peripheral combinations |

## Host Tests And Coverage

```sh
cargo test --locked --lib --tests
cargo test --locked --lib coordinator
cargo test --locked --lib ui_logic
mask coverage
mask coverage --html
```

The library exposes the same hardware-free code used by firmware, not a separate
implementation. Some hardware-coupled modules are absent from the host crate;
the coverage percentage measures only the instrumented selection. Report the
commit, toolchain, command, and excluded modules with coverage results. Avoid
putting an undated coverage percentage or test count in the README.

## Renode Simulation

The simulation runs the real `ble::coordinator` and `ui::ui_logic` modules on an
emulated nRF52840. Its BLE events are a scripted scenario. UART0 carries logs,
so no probe or defmt decoder is required.

Build from the repository root:

```sh
mask sim-build
# target/thumbv7em-none-eabihf/debug/bt2usb-sim
```

Install portable Renode and the Python test dependencies in Linux/WSL:

```sh
mask sim-setup
renode --version
```

The installer puts commands in `~/.local/bin`; ensure that directory is on PATH.
Run it inside WSL on Windows, from the repository directory. A separately
installed Renode distribution can also be used; record the version with results.

For interactive execution:

```sh
mask sim
```

In the Renode monitor, an active-low SELECT press and release is:

```text
gpio0 OnGPIO 24 false
gpio0 OnGPIO 24 true
```

Leave the press active longer than the configured debounce interval; UP is pin
11, DOWN is pin 12. The custom models in
[nrf52840_sense_gpio.cs](../renode/nrf52840_sense_gpio.cs) and the corresponding
REPL implement the SENSE/LATCH/GPIOTE PORT-event path used by Embassy. GPIO edges
therefore go through the actual button task and `BUTTON_CHANNEL` to the UI.

For repeatable headless execution:

```sh
mask sim-test
# Direct command after building:
renode-test renode/bt2usb-sim.robot
```

The Robot test asserts boot and coordinator output and uses GPIO presses to
check screen transitions. Run this when changing the simulation, GPIO path, or
shared reducers. A simulated scan completes immediately with scenario devices;
this is not a test of scan timing or advertisement interoperability.

## Continuous Integration

[ci.yml](../.github/workflows/ci.yml) runs on pushes to `main`, pull requests,
tags, manual dispatch, and a weekly schedule:

| Job | Checks |
| --- | --- |
| Host tests (Linux, Windows) | Formatting, release-helper tests, actionlint, tag/version match, host tests, Clippy, rustdoc |
| Dependency security audit | `cargo audit` |
| Embedded build & clippy | ARM Clippy with warnings denied, release firmware and self-test build, staged artifacts |
| Renode simulation test | Simulation Clippy and build, headless Robot scenario |
| Release packaging (tags only) | Version, input, and firmware verification, provenance attestation, draft release |

CI installs the toolchain pinned in `rust-toolchain.toml`, so a local run with
the same toolchain reproduces its results. `mask ci` runs the local subset.

## Hardware Acceptance Evidence

Complete [first-flash.md](first-flash.md) for a new board and after changes to
the BLE/USB/storage boundary. Save a separate result record so the template
remains reusable. Include:

- Commit/tag, ELF SHA-256, Rust version, SoftDevice version, and build profile.
- Board revision, pin changes, supply arrangement, and debug probe.
- Peripheral make/model/firmware, host OS version, BIOS/UEFI, monitor and hub.
- Each checklist result as pass, fail, or skipped with a reason.
- SoftDevice RAM requirement, stack high-water, timings, and sanitized RTT logs.

An unchecked or skipped item is unverified, not a pass. For deployment, also run
the stress, corruption, and security cases in [TODO.md](../TODO.md). Release
review uses the [release gates](deployment.md#release-gates).

## Known Verification Gaps

Not yet first-class:

- fuzzing and property tests for descriptors, advertisements, reports, and storage
- deterministic async fault tests (cancellation, full channels, contention)
- power-loss and flash fault injection
- USB conformance captures and a peripheral/host/hub compatibility matrix
- soak, latency, and reconnect-time measurements
- automated documentation link and constant checks
- reproducible-build comparison across clean environments

Track these in [TODO.md](../TODO.md), not as existing coverage.

## Validation Record — 2026-09-28

This record covers the current uncommitted hardening changes, not a published
release or a completed hardware qualification. Associate the final commit and
artifact hash with this record when preparing a release.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Windows | Passed: 219 unit tests and 3 integration tests |
| Headless Renode scenario | Renode 1.16.1 through WSL | Passed: 1 scenario |
| Dependency audit | Current locked dependency graph | No vulnerability errors; 2 unmaintained dependency warnings below |
| Release helper policy/integrity tests | Windows Python 3.14 and WSL Python 3.12 | Passed: 12 tests on each platform |
| Workflow syntax and expressions | actionlint 1.7.12 (checksum-verified) with ShellCheck 0.11.0 | Passed locally; the published-release guard's `gh --jq` filter was not executed locally |
| Clippy with warnings denied | Host tests, embedded, simulation | Passed |
| Release bridge and self-test builds | Rust 1.95.0, ARM target | Passed |
| Simulation build | Rust 1.95.0, ARM target | Passed |
| Formatting and host API documentation | Rustfmt; rustdoc with warnings denied | Passed |
| Invalid combined feature selection | Cargo check with `--keep-going` | Rejected by the explicit firmware/simulation build guard |
| Tooling and documentation | Workflow YAML, action pins, shell scripts, 31 mask recipes, local links | Validated; every action SHA resolves to its commented upstream tag (`git ls-remote`); WSL-to-Windows Cargo fallback smoke check passed |
| GitHub Actions workflow | Remote CI | Not yet verified by a workflow run |
| Board/radio/USB acceptance | Physical hardware | Not performed in this validation session |

The dependency audit reports `bare-metal 0.2.5` (`RUSTSEC-2026-0110`) and
`proc-macro-error 1.0.4` (`RUSTSEC-2024-0370`) as unmaintained transitive
dependencies. These warnings remain open maintenance work in
[TODO.md](../TODO.md); the audit result is not a warning-free dependency bill.

## Related Guides

- [Development](development.md)
- [First flash](first-flash.md)
- [Architecture and ADRs](architecture.md)
- [Deployment](deployment.md)
