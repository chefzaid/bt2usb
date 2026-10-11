# ADR 0004: Verify In Layers, From Host Tests To Hardware Acceptance

- Status: Accepted
- Date: 2026-06-22

## Context

The goal is a board that works the first time it is flashed, and documentation
that never claims more than was checked. No single check can establish that:

- host tests see the decision logic but not radios, USB, flash, or timing
- an emulator can boot the MCU and drive GPIO, but it cannot run the closed
  SoftDevice or model the USB device peripheral
- a board check covers only the peripherals, host, and hub on that bench

The layered approach was first written down on 2026-06-22 (`e3bc620`), when the
README's "Testing Strategy" described host tests, orchestration tests, a
SoftDevice-free Renode build (which `Cargo.toml` still calls "Layer 3"), and
full end-to-end checks on a real nRF52840-DK. The layers were then filled in
over three months:

| Date | Commit | Change |
| --- | --- | --- |
| 2026-06-22 | `e3bc620` | `bt2usb-sim`, `memory_sim.x`, and the first Robot test |
| 2026-06-23 | `dc11b4a` | First GitHub Actions workflow: formatting, host tests, embedded Clippy and build, Renode |
| 2026-06-24 | `4a6975f` | Documents that Renode could not deliver injected button edges to embassy-nrf's GPIOTE wait |
| 2026-09-26 | `f477d4c` | Custom GPIO/GPIOTE models so the Robot test presses real buttons; the `bt2usb-selftest` image; the first-flash checklist; the `memory_sd.x` assertion |
| 2026-09-28 | `2479c79` | Pinned toolchain, Windows host job, dependency audit, weekly schedule, simulation Clippy, release-helper tests, and actionlint |
| 2026-09-28 | `8a04b25` | actionlint installed from its upstream release with a SHA-256 check, after the `install-action` fallback failed the Linux host job on GitHub |

The 2026-09-28 validation record in [testing](../testing.md) shows the result:
every software layer passed locally, the hosted workflow was "Not yet verified
by a workflow run", and board, radio, and USB acceptance was "Not performed".
Hosted runs of the hardened workflow came later, and the tag-only release
jobs and board acceptance have still not run (see
[Verification Status](#verification-status)). The documentation needs a rule
for stating exactly that.

## Decision

Verify every change through five layers. Each owns a failure class that the
layer before it cannot see.

| Layer | What runs | Failure class it owns | Where |
| --- | --- | --- | --- |
| 1. Host tests | Unit and integration tests of the shared hardware-free modules ([ADR 0003](0003-pure-core-and-task-shell.md)) | Wrong decisions: parsing, reducers, aggregation, delivery and replay, storage validation, UI and power rules | CI on Linux and Windows, plus a line-coverage floor on Linux ([ADR 0023](0023-host-coverage-floor.md)); `mask test` |
| 2. Static and build checks | `rustfmt`; Clippy with warnings denied for host, `embedded`, and `sim`; rustdoc with private items and warnings denied for every build; release builds of `bt2usb` and `bt2usb-selftest`; the `memory_sd.x` assertion; the `build.rs` feature guard; release-helper tests; actionlint; Ruff and ShellCheck over the Python helpers, shell scripts, and mask recipes; `cargo audit`; the Markdown checks | Code that does not build for the target, lint regressions, a broken memory map, a mixed feature set, workflow, script, or release-helper mistakes, known vulnerable dependencies, and documentation that disagrees with the code | CI; `mask ci` runs the formatting, Clippy, test, rustdoc, Markdown, and build subset |
| 3. Renode simulation | `bt2usb-sim` on an emulated nRF52840, with injected GPIO edges and a scripted BLE scenario | Boot, the executor and time driver, the GPIO and GPIOTE path, and the real UI and coordinator reducers running on the ARM target | CI `simulation` job; `mask sim-test` |
| 4. Board self-test | `bt2usb-selftest` brings up each peripheral in stages and prints PASS, FAIL, or SKIP | SoftDevice RAM and enable, pairing-region flash, USB enumeration and an endpoint write, OLED, buttons, radio reception, stack margin, the MPU stack guard still programmed, and on request a real overflow caught by it | A board and probe; `mask selftest` |
| 5. Hardware acceptance | The [first-flash checklist](../first-flash.md) on real peripherals, hosts, and hubs | Pairing, reconnect, held-input release, two active slots, LEDs, monitor hubs, sleep and wake, pre-OS use | A board and a person; a dated result record |

Rules:

- CI runs layers 1 to 3 on pushes to `main` or `master`, on pull requests, on
  `v*` tags, on manual dispatch, and every Monday at 07:23 UTC
  (`cron: "23 7 * * 1"`). Release packaging needs all five CI check jobs to pass
  ([ADR 0008](0008-attested-draft-releases.md)).
- Layers 4 and 5 need a board and a person, and their results are recorded with
  the commit, ELF hash, Rust and SoftDevice versions, board, peripherals, host,
  and hub.
- Every claim states the highest layer it reached. "Implemented" means the
  source does it; "software-verified" means layers 1 to 3 pass; and
  "hardware-verified" means a recorded layer 5 result covers it.
- A skipped check is reported as skipped with a reason, never as passed. An
  unchecked first-flash item is unverified.
- A release gate in [TODO.md](../../TODO.md) that depends on a physical board
  stays open until layer 5 evidence exists, even when the code is merged.

## Alternatives Considered

- **Host tests plus informal board testing.** This was the state before June
  2026. It leaves boot, interrupt priorities, the memory map, and the GPIO path
  untested until someone flashes a board, and it leaves no record of what was
  checked.
- **Hardware-in-the-loop CI.** A self-hosted runner with a DK and a probe could
  automate parts of layers 4 and 5. It needs dedicated hardware, a powered
  bench, and runner maintenance, and a self-hosted runner must stay out of
  release production: the verification commands in
  [deployment](../deployment.md#verify-before-flashing) pass
  `--deny-self-hosted-runners`. It remains a possible future addition, not a
  replacement for recorded acceptance.
- **Emulate everything.** The SoftDevice is a closed binary tied to the radio,
  and the USB device peripheral is not modeled, so the emulated layer is
  necessarily SoftDevice-free. Renode was chosen because it ships an nRF52840
  platform description (`@platforms/cpus/nrf52840.repl`) and Robot Framework
  integration (`renode-test`); its GPIO models are
  [ADR 0014](0014-renode-gpio-models.md).
- **Gate on a coverage percentage.** Coverage instruments only the host library,
  so a threshold would reward testing what is already easy and say nothing about
  the shells. Coverage is therefore not a layer and not evidence for a claim.
  Amended on 2026-10-10 by
  [ADR 0023](0023-host-coverage-floor.md): CI now holds host line coverage at
  a floor below the recorded baseline, as a guard against losing tests of the
  pure core, while this reasoning still rules out treating the figure as a
  target or as verification.
- **Acceptance testing only.** Testing only finished firmware on hardware finds
  defects late, cannot cover malformed input or rare races on demand, and cannot
  run on every pull request.

## Rationale

Each layer is cheap where the previous one is blind. Host tests catch most
logic defects in seconds; the build layer catches what only the ARM target or
the linker can see; Renode proves the boot path, interrupts, and GPIO events
reach the real reducers on the real instruction set; the self-test isolates a
wiring or SoftDevice problem to one stage; and acceptance testing covers what
only real peripherals and hosts reveal.

Ordering the layers this way means a fault shows up at the stage that causes
it, instead of as "the keyboard doesn't work". Recording which layers ran keeps
claims honest: a feature can be implemented and software-verified while still
unverified on hardware, and the documentation says so.

## Consequences

Positive:

- Most defects are found before a board is involved, and the ones that need a
  board are isolated to a stage by the self-test.
- The same commands run locally and in CI with the pinned toolchain
  ([ADR 0013](0013-pinned-toolchain-and-mask-tasks.md)).
- Documentation can state precisely how far each feature is verified.

Negative:

- CI takes several jobs and installs Renode on every run.
- The `embedded` and `sim` features are mutually exclusive, so
  `--all-features` is rejected and each build runs separately.
- Renode exercises a scripted BLE scenario; it says nothing about scan timing,
  advertisement interoperability, USB, or flash persistence.
- Layers 4 and 5 depend on a person with a board, so hardware gates can stay
  open long after the code is merged.

Follow-up obligations, tracked in [TODO.md](../../TODO.md):

- Record the first tag run and the verification of its downloaded release
  ("Hosted provenance and release recovery acceptance"). The push and
  scheduled check runs already pass on GitHub-hosted runners.
- Run and record layer 5 on declared peripherals, hosts, and hubs ("Hardware
  compatibility baseline").
- Add the missing layers listed as known gaps: "Parser fuzzing and property
  tests", "Async task fault tests", "Power-loss-safe persistence", "Soak and
  latency measurements", and "Reproducible firmware evidence". "Automated
  documentation checks" was closed on 2026-10-10 by `scripts/check_docs.py`
  in the Linux host job.

## Implementation

- [ci.yml](../../.github/workflows/ci.yml) jobs:
  - "Host tests (ubuntu-24.04, windows-2025)": `cargo fmt --package bt2usb
    -- --check`, `python -m unittest discover -s scripts -p "release_test.py"`,
    actionlint 1.7.12, Ruff 0.16.9, and ShellCheck 0.11.0 (Linux,
    checksum-verified downloads) with `python scripts/lint_scripts.py`, tag
    validation on tags, `cargo test --locked --lib --tests`, host Clippy, and
    host rustdoc with private items and warnings denied.
  - "Host coverage": `cargo llvm-cov --locked --lib --tests` with
    cargo-llvm-cov 0.9.1, the report uploaded as an artifact, then the
    line-coverage floor ([ADR 0023](0023-host-coverage-floor.md)).
  - "Dependency security audit": `cargo audit` with `cargo-audit@0.22.2` and
    [.cargo/audit.toml](../../.cargo/audit.toml), which denies warnings and
    ignores two unmaintained-crate advisories by ID.
  - "Embedded build & clippy": `cargo clippy --locked --features embedded
    --target thumbv7em-none-eabihf -- -D warnings`, rustdoc for the embedded
    library and both firmware binaries with warnings denied, the release
    build, and staging of the checked firmware.
  - "Renode simulation test": simulation Clippy, rustdoc for `bt2usb-sim`
    with warnings denied, the simulation build,
    `scripts/install-renode.sh` (Renode 1.16.1 by default), and `renode-test
    --results-dir "$RUNNER_TEMP/renode-results" renode/bt2usb-sim.robot`, with
    results uploaded even on failure.
- [build.rs](../../build.rs) stops a combined build with
  "features `embedded` and `sim` are mutually exclusive; build each
  separately" and selects `memory_sd.x` or `memory_sim.x`.
- [memory_sd.x](../../memory_sd.x) asserts that `.data` starts at the RAM
  origin and the stack ends at the top of RAM, with the message "stack must sit
  at the top of RAM and .data at ORIGIN(RAM): the SoftDevice uses __sdata as
  APP_RAM_BASE (is flip-link in use?)".
- Renode: [bt2usb-sim.robot](../../renode/bt2usb-sim.robot) waits for
  `bt2usb-sim starting`, `buttons ready`, and `entering sim UI loop`, then
  presses SELECT, DOWN, UP, and SELECT on pins 24, 12, 11, and 24 and asserts
  the resulting screens, commands, and coordinator actions through to
  `scenario: active_count=0`.
- Self-test: [selftest.rs](../../src/selftest.rs) reports the stages
  `softdevice`, `flash`, `usb enumeration`, `usb hid report`, `oled i2c`,
  `oled render`, the three buttons, `ble scan`, `stack`, and `stack guard`,
  each as `[PASS]`, `[FAIL]`, or `[SKIP]`, ends with
  `==== self-test done: N passed, N failed, N skipped ====`, and then offers an
  optional deliberate stack overflow
  ([ADR 0026](0026-mpu-stack-guard.md)). The only HID report
  it sends is an all-zero mouse report.
- Acceptance: [first-flash.md](../first-flash.md), recorded through the
  `.github/ISSUE_TEMPLATE/hardware-result.md` template.
- Records: dated validation records and known gaps in
  [testing](../testing.md#known-verification-gaps).

### Verification Status

- **Implemented:** all five layers. Layers 1 to 3 run in CI; layers 4 and 5
  exist as the self-test image, the first-flash checklist, and the
  hardware-result issue template.
- **Software-verified:** the
  [2026-09-28 validation record](../testing.md#validation-record--2026-09-28)
  reports every software layer passing locally. On GitHub-hosted runners, the
  push run for `2479c79` (36441384244) failed at the actionlint installation
  step, which `8a04b25` replaced. Push runs 36441995385 (`8a04b25`,
  2026-09-28) and 37932436721 (`7fc99d6`, 2026-10-09) and scheduled run
  37338711407 (2026-10-05) passed all five check jobs. The tag-only
  `release-package` and `release` jobs have never run, because no `v*` tag
  exists.
- **Hardware-verified:** not yet. No layer 4 or layer 5 result is recorded in
  the repository.

## Related

- [Testing guide](../testing.md)
- [Code quality](../code-quality.md)
- [First flash](../first-flash.md)
- [Deployment: release gates](../deployment.md#release-gates)
- [ADR 0003: Hardware-free decision modules](0003-pure-core-and-task-shell.md)
- [ADR 0008: Attested draft releases](0008-attested-draft-releases.md)
- [ADR 0013: Pinned toolchain and mask tasks](0013-pinned-toolchain-and-mask-tasks.md)
- [ADR 0014: Renode GPIO models](0014-renode-gpio-models.md)
- [ADR 0023: Host coverage floor](0023-host-coverage-floor.md)
