# Testing and validation

Software checks and board checks cover different failure modes. Passing host
tests or Renode does not establish radio interoperability, USB compliance,
flash durability, security certification, or production readiness.

## Layers

| Layer | What it exercises | What it cannot establish |
| --- | --- | --- |
| Host unit/integration tests | Shared HID parsing/serialization, reducers, advert parsing, power/UI rules, report coalescing, storage framing and record validation | Async driver timing and peripheral behavior |
| Embedded checks | Firmware/self-test compile and lint for the ARM target, linker reservations | Runtime correctness on hardware |
| Renode | SoftDevice-free ARM boot, GPIO/timer paths, UI/coordinator scenario | Actual BLE, USB, SoftDevice, or flash persistence |
| Board self-test | SoftDevice enable, flash access, USB enumeration/report, OLED, buttons, scan | Long-term reliability or the full peripheral matrix |
| Hardware acceptance | Pairing, reconnect, held-input release, monitor hub, sleep/wake, pre-OS operation | Untested host/peripheral combinations |

## Host tests and coverage

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

## Renode simulation

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

## Hardware acceptance evidence

Complete [FIRST_FLASH.md](FIRST_FLASH.md) for a new board and after changes to
the BLE/USB/storage boundary. Save a separate result record so the template
remains reusable. Include:

- Commit/tag, ELF SHA-256, Rust version, SoftDevice version, and build profile.
- Board revision, pin changes, supply arrangement, and debug probe.
- Peripheral make/model/firmware, host OS version, BIOS/UEFI, monitor and hub.
- Each checklist result as pass, fail, or skipped with a reason.
- SoftDevice RAM requirement, stack high-water, timings, and sanitized RTT logs.

An unchecked or skipped item is unverified, not a pass. For deployment, also run
the stress, corruption, and security cases in [TODO.md](../TODO.md). Release
review uses [Operations and releases](OPERATIONS.md).

## Working-tree validation — 2026-09-28

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
