# Development

## Toolchain

Use Rust through rustup. [rust-toolchain.toml](../rust-toolchain.toml) pins the
compiler used by the project, and [Cargo.lock](../Cargo.lock) pins dependency
resolution. Build and test with `--locked`; update dependencies intentionally in
a reviewed change. The BLE stack is based on a pinned upstream revision with a
small vendored offset-read patch; see
[vendor notes](../vendor/nrf-softdevice/README.bt2usb.md) before updating it.

```sh
rustup target add thumbv7em-none-eabihf
rustup component add rustfmt clippy llvm-tools-preview
cargo install --locked probe-rs-tools mask cargo-llvm-cov cargo-binutils cargo-bloat
```

Only the compiler is needed for host tests. ARM builds need the target; flashing
needs `probe-rs` and a probe; coverage and size analysis need the corresponding
optional Cargo tools. Tool installation may require system libraries on the host.

## Build and check

Commands run from the repository root. The project deliberately has no global
Cargo target, so host tests use the native platform. Firmware commands select ARM
explicitly.

```sh
cargo fmt --package bt2usb -- --check
cargo clippy --locked --lib --tests -- -D warnings
cargo test --locked --lib --tests
cargo clippy --locked --features embedded --target thumbv7em-none-eabihf -- -D warnings
cargo build --locked --features embedded --target thumbv7em-none-eabihf --release
cargo clippy --locked --features sim --target thumbv7em-none-eabihf -- -D warnings
cargo build --locked --features sim --target thumbv7em-none-eabihf
python -B -m unittest discover -s scripts -p release_test.py -v
```

Build `embedded` and `sim` separately. The simulation supplies a different
critical-section implementation and excludes SoftDevice, USB, and flash drivers;
`--all-features` is not a supported firmware configuration.

Release-helper tests require Python 3.11 or newer. When changing GitHub Actions,
run `actionlint` as well; CI uses actionlint 1.7.12. The
[release guide](RELEASING.md) explains version and provenance checks.

Format only the application package. The vendored SoftDevice source carries a
small reviewed patch and should retain upstream formatting; do not run
`cargo fmt --all` over it.

[maskfile.md](../maskfile.md) wraps these tasks. Its recipes are Bash scripts.
On Windows, use WSL/Bash for mask tasks or run Cargo directly in PowerShell.

| Command | Purpose |
| --- | --- |
| `mask build --release` | Build firmware and self-test |
| `mask run --release` | Flash and run bridge with RTT logs |
| `mask selftest` | Flash board bring-up image |
| `mask test` | Host unit and integration tests |
| `mask coverage` | Coverage for host-testable modules |
| `mask check` / `mask clippy` | Embedded type check / lint |
| `mask ci` | Local software checks; inspect recipe for exact gates |
| `mask sim-test` | Build simulation and run headless Renode test |
| `mask probe-list` | Discover available probes |
| `mask size` / `mask bloat` | Inspect release size |
| `mask doc` | Generate embedded API documentation |

Flashing the bridge requires S140 to have been installed. Complete
[First flash](FIRST_FLASH.md) before treating a successful download as a working
device. `mask run --release` updates the application; a full-chip erase also
removes SoftDevice and stored bonds.

## Devcontainer and WSL2

The VS Code [.devcontainer](../.devcontainer/) installs Rust embedded tools and
uses a privileged container for probe access. It does not require a
`/dev/bus/usb` mount at startup. Review that privilege when using it on a shared
development host.

1. Attach the probe from Windows to WSL with `usbipd-win`.
2. Reopen the repository in the VS Code devcontainer.
3. Run `mask probe-list` and the host tests.

The probe and the board's native USB HID connection are separate. Keep the native
USB port attached to the PC whose enumeration/input behavior you are testing.

## Making a change

Keep hardware-free decisions in the shared pure modules and asynchronous I/O in
the task layer. Add regression tests for behavior changes, particularly malformed
reports, bounded-buffer handling, reconnect transitions, and releases of held
inputs. Do not introduce dynamic allocation into firmware paths without an
explicit design review.

Run the relevant commands above and record which passed. Changes to pins, timing,
BLE security, flash, USB descriptors, or power also need the applicable hardware
checks. Include limitations and skipped checks in the review. Preserve the
separation between open and completed work in [TODO.md](../TODO.md), and update
the specific guide when behavior or commands change.

Do not include private bond keys, raw memory dumps, or unsanitized input capture
in a public issue. See [SECURITY.md](../SECURITY.md).
