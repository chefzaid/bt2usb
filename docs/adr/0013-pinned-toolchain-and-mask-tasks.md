# ADR 0013: Pin The Toolchain And Wrap Workflows In Mask Tasks

- Status: Accepted
- Date: 2026-09-28

This record was written retroactively on 2026-10-09 from the source and the
commit history. `maskfile.md`, `scripts/run-tool.sh`, and the devcontainer
arrived with the first embedded implementation (`8e6dd17`, 2026-02-21). The
toolchain pin, the tracked lockfile, and `--locked` everywhere landed in
`2479c79` on 2026-09-28.

## Context

bt2usb is built in several places: Windows with Rust installed natively, WSL2
(often with Cargo installed only on the Windows side), plain Linux, a VS Code
devcontainer, and GitHub Actions on Linux and Windows runners. The same
checks must give the same answer in all of them, and a released firmware
image must be traceable to an exact compiler and dependency graph.

Several problems showed up before the pin:

- **No pinned compiler.** Each machine used whatever stable Rust it had.
  Clippy runs with warnings denied, so a new lint in a newer toolchain fails
  one machine's build and not another's. The bring-up commit `f477d4c`
  changed `src/ble/adv_parser.rs` for "the clippy lint added in Rust 1.98",
  in the words of its message. The pin that followed chose 1.95.0, not the
  version that message names; the repository does not record why.
- **No lockfile.** `.gitignore` excluded `Cargo.lock` (the template comment
  for libraries), so two builds of the same commit could resolve different
  dependency versions.
- **Long, easy-to-get-wrong commands.** Firmware commands need
  `--features embedded` or `--features sim`, `--target thumbv7em-none-eabihf`,
  and a binary name, while host tests need none of them. An early
  `.cargo/config.toml` set a global `build.target`, which made plain
  `cargo test` build for the ARM target and fail; it was removed in `9c3568f`
  (2026-06-22).
- **Tools outside `PATH`.** In WSL, `cargo` and `probe-rs` are often only in
  the Windows user's `.cargo\bin`. The first version of `run-tool.sh` found
  them by scanning every profile under `/mnt/c/Users` and `/c/Users`, which
  could run a binary from another user's profile.
- **Line endings.** On a Windows checkout, CRLF line endings break shell
  scripts, and `mask` runs the Bash blocks it extracts from `maskfile.md`.

## Decision

Pin what determines the build, and run every routine workflow through named,
readable tasks.

- **Compiler.** [rust-toolchain.toml](../../rust-toolchain.toml) pins channel
  `1.95.0` with the `minimal` profile, `clippy` and `rustfmt`, and the
  `thumbv7em-none-eabihf` target. `rust-version = "1.95"` in
  [Cargo.toml](../../Cargo.toml) states the same minimum to Cargo. CI resolves
  the toolchain from the file (`rustup show active-toolchain`) instead of
  installing its own.
- **Dependencies.** `Cargo.lock` is tracked; `.gitignore` now says "Cargo.lock
  is tracked: firmware builds must use the reviewed dependency graph." Every
  Cargo build, run, check, test, Clippy, coverage, size, and doc invocation in
  `maskfile.md` and in CI passes `--locked`, so a build fails instead of
  silently changing the graph. The BLE crates are pinned to one git revision
  and patched in-tree ([ADR 0007](0007-vendored-softdevice-patch.md)).
  Dependabot proposes Cargo and GitHub Actions updates weekly as reviewable
  pull requests.
- **No global target.** [.cargo/config.toml](../../.cargo/config.toml) sets no
  `build.target`, so host tests build for the native platform. The runner
  (`probe-rs run --chip nRF52840_xxAA`) and link arguments apply only under
  `cfg(all(target_arch = "arm", target_os = "none"))`, and every firmware task
  passes `--target thumbv7em-none-eabihf` explicitly.
- **Mask tasks.** [maskfile.md](../../maskfile.md) is the executable task
  reference. Each recipe is a Bash block, and each Cargo or `probe-rs` call in
  it goes through `scripts/run-tool.sh`. The main tasks:

  | Task | Runs |
  | --- | --- |
  | `mask build --release`, `mask run --release`, `mask selftest` | Firmware build, flash and run with RTT, self-test image |
  | `mask test`, `mask coverage` | Host unit and integration tests, coverage with `cargo-llvm-cov` or `cargo-tarpaulin` |
  | `mask check`, `mask clippy`, `mask fmt-check` | Embedded type check, embedded Clippy with `-D warnings`, formatting check |
  | `mask sim-setup`, `mask sim-build`, `mask sim`, `mask sim-test` | Renode install, simulation build, GUI run, headless Robot test |
  | `mask softdevice`, `mask probe-list`, `mask rtt` | S140 download and flash, probe discovery, RTT attach |
  | `mask ci` | The local check set below |

- **`mask ci` as the local gate.** It runs, stopping at the first failure:
  `cargo fmt -- --check`; Clippy with `-D warnings` for the host library and
  tests, the `embedded` build, and the `sim` build; host tests; the `embedded`
  release build; and the `sim` build. CI runs the same checks plus the items
  listed under Consequences.
- **Tool resolution.** [run-tool.sh](../../scripts/run-tool.sh) runs the named
  tool from the first location that has it: `PATH`, `$CARGO_HOME/bin`,
  `$HOME/.cargo/bin`, the Windows `USERPROFILE` converted with `cygpath` (Git
  Bash), `/c/Users/$USERNAME` and `/mnt/c/Users/$USERNAME`, and finally, in WSL,
  the profile that `cmd.exe` reports for the current Windows user, converted
  with `wslpath`. It never scans other users' profiles. If nothing is found it
  exits with status 127 and
  `Error: '$tool' was not found in PATH or the current user's Rust install locations.`
- **Line endings.** [.gitattributes](../../.gitattributes) forces LF for shell
  scripts, `maskfile.md`, Renode files, Rust sources, TOML, linker scripts,
  Markdown, JSON, YAML, and `Cargo.lock`.
- **Devcontainer.** [.devcontainer](../../.devcontainer/devcontainer.json)
  starts from `mcr.microsoft.com/devcontainers/rust:1-bookworm` as user
  `vscode`, runs privileged so a probe forwarded with `usbipd-win` is reachable
  without a `/dev/bus/usb` bind mount (which would fail container creation
  when no probe is attached), and runs
  [post-create.sh](../../.devcontainer/post-create.sh). That script installs
  the ARM target and `probe-rs-tools` 0.32.0, `mask` 0.11.7, `cargo-llvm-cov`
  0.9.1, and `cargo-binutils` 0.4.0 with `cargo install --locked`, writes udev rules for
  J-Link, ST-Link, CMSIS-DAP, and Nordic development kits and dongles, and
  fails setup if
  `cargo test --locked --lib --tests` fails.
- **Releases are tied to the pin.** `scripts/release.py` records `rustc
  --version` in `BUILD-INFO.json` and refuses to package a build whose
  compiler does not start with the pinned channel:
  `build compiler does not match the pinned Rust toolchain`
  ([ADR 0008](0008-attested-draft-releases.md)).

## Alternatives Considered

- **Track the latest stable toolchain.** New lints and compiler changes would
  arrive unannounced, with warnings denied, and released firmware could not be
  rebuilt with the same compiler.
- **Leave `Cargo.lock` untracked.** That is the convention for libraries. For
  firmware it means an untested dependency graph on every fresh clone.
- **Set `build.target` globally** (the state before `9c3568f`). It saves a flag
  on firmware commands and breaks `cargo test` on the host.
- **Document raw commands without a task runner.** The commands are long and
  differ in features, target, binary, and `--locked`. Mask keeps each command
  in readable Markdown that both documents and runs it, so the documentation
  cannot drift from what runs.
- **Scan every Windows profile for Cargo** (the first `run-tool.sh`). It finds
  tools in more setups, at the risk of running another user's binary.
- **Bind-mount `/dev/bus/usb` instead of running privileged.** The comment in
  `devcontainer.json` records why not: creation fails when no probe is
  attached yet.

## Rationale

A pinned compiler and lockfile make "it passed" mean the same thing on every
machine and in CI, and let a release record name the exact inputs. Wrapping
the commands in mask tasks makes the correct invocation the easy one, and
`run-tool.sh` lets the same Bash recipes run in WSL against a Windows Rust
installation without asking contributors to install Rust twice. The
devcontainer gives Linux contributors a ready environment that proves itself
by running the host tests before it reports success.

## Consequences

Positive:

- Toolchain and dependency changes are explicit, reviewable commits, and
  released artifacts name their compiler and input hashes.
- Host tests, firmware builds, and the simulation use the same Cargo
  invocations everywhere: through mask tasks on Windows (through WSL or Git
  Bash), Linux, and the devcontainer, and as the same `cargo` commands, with
  the same features, target, and `--locked`, in the CI jobs, which do not use
  mask.
- A missing tool fails with a clear message and status 127 instead of running
  something unexpected.

Negative:

- Upgrading Rust means editing `rust-toolchain.toml` and `rust-version`, fixing
  any new Clippy findings, and rerunning host, firmware, and simulation checks.
- Updating a dependency means updating `Cargo.lock` in its own change; a stale
  lockfile fails every `--locked` command.
- Mask recipes are Bash. Native Windows contributors use WSL or Git Bash, or
  run Cargo directly in PowerShell.
- `mask ci` is a subset of CI. It includes rustdoc with warnings denied for
  every build since 2026-10-10. CI also runs the release-helper unit tests,
  actionlint (Linux), the tag and version check on tags, the host coverage
  floor ([ADR 0023](0023-host-coverage-floor.md)), `cargo audit`, the Renode
  simulation test, a Windows host job, and release staging. Run `mask sim-test` locally when touching the simulation or
  shared reducers.
- Local and released firmware differ in log level. `.cargo/config.toml` sets
  `DEFMT_LOG = "debug"`, while CI sets `DEFMT_LOG: info` in its environment,
  which takes precedence, and release packaging requires `info`. A local
  release build is therefore not byte-identical to a published artifact.
- Optional Cargo tools are pinned by hand since 2026-10-10. `mask deps`,
  `mask coverage-install`, and the devcontainer run `cargo install --locked`
  with an exact `--version`; the versions repeat in `maskfile.md`,
  `post-create.sh`, and the development guide, and cargo-llvm-cov's also in
  the CI coverage job, with no check that they agree.
  The devcontainer base image tag `1-bookworm` still moves, and
  `scripts/install-renode.sh` downloads Renode 1.16.1 without a checksum.
- The privileged devcontainer has broad access to the host. Review it before
  using it on a shared machine.

Follow-up obligations, tracked in [TODO.md](../../TODO.md):

- "Development environment hardening": pin the container inputs, and replace
  blanket container privilege with scoped probe access.
- "Supply-chain and tooling maintenance": add license checks, an SBOM, and
  verified digests for downloaded non-Cargo tools and the SoftDevice.
- "Reproducible firmware evidence": compare artifacts from two clean
  environments and document remaining nondeterminism.

## Implementation

| Concern | Where |
| --- | --- |
| Compiler pin | [rust-toolchain.toml](../../rust-toolchain.toml); `rust-version` in [Cargo.toml](../../Cargo.toml) |
| Lockfile | [Cargo.lock](../../Cargo.lock), tracked per [.gitignore](../../.gitignore) |
| Target-scoped Cargo settings | [.cargo/config.toml](../../.cargo/config.toml) |
| Task recipes | [maskfile.md](../../maskfile.md) |
| Tool resolution | [run-tool.sh](../../scripts/run-tool.sh) |
| Line endings | [.gitattributes](../../.gitattributes) |
| Devcontainer | [devcontainer.json](../../.devcontainer/devcontainer.json), [post-create.sh](../../.devcontainer/post-create.sh) |
| CI use of the pin and `--locked` | [ci.yml](../../.github/workflows/ci.yml) |
| Release compiler check | `stage_build` and `package_release` in [release.py](../../scripts/release.py) |
| Update automation | [dependabot.yml](../../.github/dependabot.yml) |

### Verification Status

- **Implemented:** everything above.
- **Software-verified:** the
  [2026-09-28 validation record](../testing.md#validation-record--2026-09-28)
  reports the host, embedded, and simulation checks passing locally with Rust
  1.95.0, 31 mask recipes validated, and the WSL-to-Windows Cargo fallback
  smoke check passing. That record does not include a hosted GitHub Actions
  run. The push run for `2479c79` (36441384244) failed at the earlier
  actionlint installation step, which `8a04b25` replaced. With the pinned
  toolchain, push runs 36441995385 (`8a04b25`, 2026-09-28) and 37932436721
  (`7fc99d6`, 2026-10-09) and scheduled run 37338711407 (2026-10-05) passed
  all five check jobs on GitHub-hosted runners. The tag-only release jobs
  have never run.
- **Hardware-verified:** not applicable, except that `mask run`,
  `mask selftest`, and `mask softdevice` need a board and probe; the
  repository holds no board record of them.

## Related

- [Development: toolchain](../development.md#toolchain)
- [Development: devcontainer and WSL2](../development.md#devcontainer-and-wsl2)
- [Testing: continuous integration](../testing.md#continuous-integration)
- [Code quality](../code-quality.md)
- [Deployment](../deployment.md)
- [Task reference](../../maskfile.md)
- [ADR 0004: Layered verification](0004-layered-verification.md)
- [ADR 0007: Vendored nrf-softdevice patch](0007-vendored-softdevice-patch.md)
- [ADR 0008: Attested draft releases](0008-attested-draft-releases.md)
