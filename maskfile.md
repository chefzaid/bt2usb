# bt2usb Task Runner

Common development tasks for the bt2usb Bluetooth-to-USB HID bridge.

> Requires [mask](https://github.com/jacobdeichert/mask) (`cargo install --locked mask --version 0.11.7`)

## build

> Build the firmware for nRF52840

**OPTIONS**
* release
    * flags: --release
    * desc: Build release firmware (optimized for size)

```bash
if [[ "${release}" == "true" ]]; then
    ./scripts/run-tool.sh cargo build --locked --features embedded --target thumbv7em-none-eabihf --release
else
    ./scripts/run-tool.sh cargo build --locked --features embedded --target thumbv7em-none-eabihf
fi
```

## build-release

> Build the firmware for nRF52840 (release mode, optimized for size)

```bash
./scripts/run-tool.sh cargo build --locked --features embedded --target thumbv7em-none-eabihf --release
```

## run

> Build, flash, and run firmware on the connected nRF52840 board

**OPTIONS**
* release
    * flags: --release
    * desc: Build and flash release firmware (optimized for size)

```bash
if [[ "${release}" == "true" ]]; then
    ./scripts/run-tool.sh cargo run --locked --features embedded --target thumbv7em-none-eabihf --release --bin bt2usb
else
    ./scripts/run-tool.sh cargo run --locked --features embedded --target thumbv7em-none-eabihf --bin bt2usb
fi
```

## flash

> Build and flash firmware to the connected nRF52840 board

```bash
./scripts/run-tool.sh cargo run --locked --features embedded --target thumbv7em-none-eabihf --release --bin bt2usb
```

## flash-debug

> Build and flash the dev-profile (debug) firmware; logs stream over RTT as in every profile

```bash
./scripts/run-tool.sh cargo run --locked --features embedded --target thumbv7em-none-eabihf --bin bt2usb
```

## selftest

> Flash and run the on-board self-test (new-board bring-up, see docs/first-flash.md)

Checks the SoftDevice, flash storage, USB enumeration, OLED, buttons and BLE
radio on the connected board and prints one PASS/FAIL/SKIP line per stage.
Needs the SoftDevice flashed once. Flash the real firmware afterwards with
`mask run --release`.

`--no-catch-hardfault` goes to `probe-rs run`: by default it halts the core
on entry to every HardFault, before the firmware's handler can log, so the
optional deliberate overflow would end in probe-rs's
`Firmware exited unexpectedly: Exception` instead of the self-test's
`stack overflow` line (docs/adr/0026-mpu-stack-guard.md). A panic still logs
its message; the session then stays attached until Ctrl-C.

```bash
./scripts/run-tool.sh cargo run --locked --features embedded --target thumbv7em-none-eabihf --release --bin bt2usb-selftest -- --no-catch-hardfault
```

## test

> Run unit + integration tests on host (Windows/Linux/macOS)

Runs the library unit tests AND the `tests/` integration tests, building for the
native host. The firmware binaries are skipped automatically: `bt2usb` and
`bt2usb-selftest` require the `embedded` feature and `bt2usb-sim` requires the
`sim` feature (`required-features` in `Cargo.toml`). This works on Windows,
Linux, macOS, and inside the WSL2 devcontainer because `.cargo/config.toml`
sets no global `build.target` (embedded tasks pass `--target` explicitly
instead).

```bash
./scripts/run-tool.sh cargo test --locked --lib --tests
```

## test-verbose

> Run unit + integration tests with output shown

```bash
./scripts/run-tool.sh cargo test --locked --lib --tests -- --nocapture
```

## coverage

> Run tests with code coverage analysis (requires cargo-tarpaulin on Linux or cargo-llvm-cov)

**OPTIONS**
* html
    * flags: --html
    * desc: Generate HTML report
* json
    * flags: --json
    * desc: Generate JSON report

**Options:**
- `--html` - Generate an HTML report and print its path (no browser is opened)
- `--json` - Output JSON format for CI integration

```bash
# Try cargo-llvm-cov first (cross-platform), fallback to tarpaulin
if ./scripts/run-tool.sh cargo llvm-cov --version >/dev/null 2>&1; then
    if [[ "${html:-false}" == "true" ]]; then
        ./scripts/run-tool.sh cargo llvm-cov --locked --lib --tests --html --output-dir coverage-html
        echo "Coverage report: coverage-html/html/index.html"
    elif [[ "${json:-false}" == "true" ]]; then
        ./scripts/run-tool.sh cargo llvm-cov --locked --lib --tests --json --output-path coverage.json
        echo "Coverage report: coverage.json"
    else
        ./scripts/run-tool.sh cargo llvm-cov --locked --lib --tests
    fi
elif ./scripts/run-tool.sh cargo tarpaulin --version >/dev/null 2>&1; then
    if [[ "${html:-false}" == "true" ]]; then
        ./scripts/run-tool.sh cargo tarpaulin --locked --lib --tests --out Html --output-dir coverage
        echo "Coverage report: coverage/tarpaulin-report.html"
    elif [[ "${json:-false}" == "true" ]]; then
        ./scripts/run-tool.sh cargo tarpaulin --locked --lib --tests --out Json --output-dir coverage
        echo "Coverage report: coverage/coverage.json"
    else
        ./scripts/run-tool.sh cargo tarpaulin --locked --lib --tests --out Stdout
    fi
else
    echo "No coverage tool found. Install one of:"
    echo "  mask coverage-install   (cargo-llvm-cov, recommended)"
    echo "  cargo install --locked cargo-tarpaulin --version 0.37.5   (Linux only)"
    exit 1
fi
```

## coverage-html

> Generate HTML coverage report

```bash
if ./scripts/run-tool.sh cargo llvm-cov --version >/dev/null 2>&1; then
    ./scripts/run-tool.sh cargo llvm-cov --locked --lib --tests --html --output-dir coverage-html
    echo "Coverage report: coverage-html/html/index.html"
elif ./scripts/run-tool.sh cargo tarpaulin --version >/dev/null 2>&1; then
    ./scripts/run-tool.sh cargo tarpaulin --locked --lib --tests --out Html --output-dir coverage
    echo "Coverage report: coverage/tarpaulin-report.html"
else
    echo "No coverage tool found. Install one of:"
    echo "  mask coverage-install   (cargo-llvm-cov, recommended)"
    echo "  cargo install --locked cargo-tarpaulin --version 0.37.5   (Linux only)"
    exit 1
fi
```

## coverage-json

> Generate JSON coverage report

```bash
if ./scripts/run-tool.sh cargo llvm-cov --version >/dev/null 2>&1; then
    ./scripts/run-tool.sh cargo llvm-cov --locked --lib --tests --json --output-path coverage.json
    echo "Coverage report: coverage.json"
elif ./scripts/run-tool.sh cargo tarpaulin --version >/dev/null 2>&1; then
    ./scripts/run-tool.sh cargo tarpaulin --locked --lib --tests --out Json --output-dir coverage
    echo "Coverage report: coverage/coverage.json"
else
    echo "No coverage tool found. Install one of:"
    echo "  mask coverage-install   (cargo-llvm-cov, recommended)"
    echo "  cargo install --locked cargo-tarpaulin --version 0.37.5   (Linux only)"
    exit 1
fi
```

## coverage-install

> Install code coverage tools

```bash
set -e
echo "Installing cargo-llvm-cov (recommended, cross-platform)..."
./scripts/run-tool.sh cargo install --locked cargo-llvm-cov --version 0.9.1
./scripts/run-tool.sh rustup component add llvm-tools-preview
echo "Done! Run 'mask coverage' to generate reports."
```

## check

> Type-check the embedded build without compiling

```bash
./scripts/run-tool.sh cargo check --locked --features embedded --target thumbv7em-none-eabihf
```

## clippy

> Run clippy lints on the embedded build, with and without the sensitive-logging opt-in

```bash
set -e
./scripts/run-tool.sh cargo clippy --locked --features embedded --target thumbv7em-none-eabihf -- -D warnings
./scripts/run-tool.sh cargo clippy --locked --features embedded,log-sensitive-data --target thumbv7em-none-eabihf -- -D warnings
```

## fmt

> Format the bt2usb package (vendored code is not touched)

```bash
./scripts/run-tool.sh cargo fmt
```

## fmt-check

> Check formatting without modifying files

```bash
./scripts/run-tool.sh cargo fmt -- --check
```

## clean

> Remove build artifacts

```bash
./scripts/run-tool.sh cargo clean
```

## size

> Show firmware binary size breakdown

```bash
./scripts/run-tool.sh cargo size --locked --features embedded --target thumbv7em-none-eabihf --release --bin bt2usb -- -A
```

## bloat

> Analyze what's contributing to binary size (requires cargo-bloat)

```bash
./scripts/run-tool.sh cargo bloat --locked --features embedded --target thumbv7em-none-eabihf --release --bin bt2usb -n 30
```

## rtt

> Attach to RTT logs from a running device (requires probe-rs)

```bash
./scripts/run-tool.sh probe-rs attach --chip nRF52840_xxAA target/thumbv7em-none-eabihf/release/bt2usb
```

## sim-setup

> Install Renode + renode-test deps for Layer-3 simulation (Linux/WSL2, no root)

Downloads portable Renode and the Python deps for `renode-test` into your home
(`~/.local`). Idempotent. Run this once inside WSL/Linux, then use `mask sim` /
`mask sim-test`. (On Windows, run it from inside WSL; see
docs/development.md#devcontainer-and-wsl2.)

```bash
bash scripts/install-renode.sh
```

## sim-build

> Build the SoftDevice-free simulation firmware for Renode (Layer 3, no hardware)

```bash
./scripts/run-tool.sh cargo build --locked --features sim --target thumbv7em-none-eabihf
echo "Sim ELF: target/thumbv7em-none-eabihf/debug/bt2usb-sim"
```

## sim

> Build + run the simulation firmware in Renode, GUI (requires `renode` on PATH)

Boots the SoftDevice-free firmware on a simulated nRF52840; UART0 output (the
coordinator + UI logic running on the target) appears in the Renode terminal
window, and `sysbus.twi0.oled Text` in the monitor prints what the simulated
OLED shows. No probe or board needed. See docs/testing.md.

```bash
./scripts/run-tool.sh cargo build --locked --features sim --target thumbv7em-none-eabihf
if command -v renode >/dev/null 2>&1; then
    renode renode/bt2usb-sim.resc
else
    echo "Renode not found on PATH. Install it from https://renode.io"
    echo "then run:  renode renode/bt2usb-sim.resc"
    exit 127
fi
```

## sim-test

> Build + run the headless Renode robot test (asserts the sim's UART output and OLED text)

Boots the sim in Renode (no GUI), presses the GPIO buttons, and asserts that
the UI controller, the coordinator, management, and the store run on the
simulated MCU and that the simulated OLED shows each screen's text. Suitable
for CI. Requires `renode-test` on PATH (ships with Renode).

```bash
./scripts/run-tool.sh cargo build --locked --features sim --target thumbv7em-none-eabihf
if command -v renode-test >/dev/null 2>&1; then
    renode-test renode/bt2usb-sim.robot
else
    echo "renode-test not found on PATH. Install Renode from https://renode.io"
    echo "then run:  renode-test renode/bt2usb-sim.robot"
    exit 127
fi
```

## softdevice

> Flash the Nordic SoftDevice S140 (required once per board)

Downloads Nordic's S140 archive only when `s140_nrf52_7.3.0_softdevice.hex` is
missing from the repository root, extracts the HEX, and always flashes it. No
digest is checked, of a fresh download or of an existing HEX; record the
archive's SHA-256 yourself. See docs/deployment.md#softdevice-installation
before release use.

```bash
set -euo pipefail
SD_URL="https://nsscprodmedia.blob.core.windows.net/prod/software-and-other-downloads/softdevices/s140/s140_nrf52_7.3.0.zip"
SD_HEX="s140_nrf52_7.3.0_softdevice.hex"

if [ ! -f "$SD_HEX" ]; then
    echo "Downloading SoftDevice S140 v7.3.0..."
    curl -fL --retry 3 "$SD_URL" -o softdevice.zip
    unzip -o softdevice.zip "$SD_HEX"
    rm softdevice.zip
fi

echo "Flashing SoftDevice..."
./scripts/run-tool.sh probe-rs download "$SD_HEX" --chip nRF52840_xxAA --format hex
echo "Done! SoftDevice is ready."
```

## devcontainer

> Open the project in VS Code devcontainer

```bash
code --folder-uri "vscode-remote://dev-container+$(printf '%s' "$PWD" | xxd -p -c 256)/workspaces/bt2usb"
```

## devcontainer-build

> Build the devcontainer image

```bash
devcontainer build --workspace-folder .
```

## probe-list

> List connected debug probes

```bash
./scripts/run-tool.sh probe-rs list
```

## doc

> Generate and open the firmware API documentation

Builds rustdoc for the `embedded` configuration, dependencies included, without
denying warnings. `mask rustdoc-check` runs the checks CI runs instead.

```bash
./scripts/run-tool.sh cargo doc --locked --features embedded --target thumbv7em-none-eabihf --open
```

## rustdoc-check

> Check every rustdoc build with private items and warnings denied, as CI does

Documents the host library, the embedded library, the `bt2usb` and
`bt2usb-selftest` binaries, and the `bt2usb-sim` binary. A broken intra-doc
link or any other rustdoc warning fails the task. The library and the firmware
binary share the name `bt2usb`, so they are documented in separate runs.

```bash
set -e
export RUSTDOCFLAGS="-D warnings"
doc=(./scripts/run-tool.sh cargo doc --locked --no-deps --document-private-items)
arm=(--target thumbv7em-none-eabihf)
"${doc[@]}" --lib
"${doc[@]}" --features embedded "${arm[@]}" --lib
"${doc[@]}" --features embedded "${arm[@]}" --bin bt2usb --bin bt2usb-selftest
"${doc[@]}" --features sim "${arm[@]}" --bin bt2usb-sim
echo "All documentation builds are free of warnings."
```

## docs-check

> Check the Markdown guides' links, documented constants, memory map, and commands

Runs [scripts/check_docs.py](scripts/check_docs.py) over every tracked
Markdown file and its unit tests. A broken link or anchor, a value that
disagrees with `src/config.rs` or the linker scripts, an unknown `mask` recipe,
binary, or feature, or a file path that no longer exists fails the task, with
the file and line of each finding.

```bash
set -e
py="$(command -v python3 || command -v python)"
"$py" -m unittest discover -s scripts -p "check_docs_test.py"
"$py" scripts/check_docs.py
```

## lint-scripts

> Lint the Python helpers, the shell scripts, and these recipes, as CI does

Runs [scripts/lint_scripts.py](scripts/lint_scripts.py) and its unit tests:
Ruff (`ruff check` and `ruff format --check`, settings in `ruff.toml`) over
every tracked Python file, and ShellCheck over every tracked `*.sh` file and
every bash or sh recipe in this file, with findings reported at their line
here. Needs Ruff and ShellCheck on `PATH`; CI pins Ruff 0.16.9 and ShellCheck
0.11.0.

```bash
set -e
py="$(command -v python3 || command -v python)"
"$py" -m unittest discover -s scripts -p "lint_scripts_test.py"
"$py" scripts/lint_scripts.py
```

## ci

> Run local formatting, lint, host tests, rustdoc and Markdown checks, and firmware builds

```bash
set -e
echo "=== Checking format ==="
./scripts/run-tool.sh cargo fmt -- --check
echo "=== Running clippy ==="
./scripts/run-tool.sh cargo clippy --locked --lib --tests -- -D warnings
./scripts/run-tool.sh cargo clippy --locked --features embedded --target thumbv7em-none-eabihf -- -D warnings
./scripts/run-tool.sh cargo clippy --locked --features embedded,log-sensitive-data --target thumbv7em-none-eabihf -- -D warnings
./scripts/run-tool.sh cargo clippy --locked --features sim --target thumbv7em-none-eabihf -- -D warnings
echo "=== Running tests ==="
./scripts/run-tool.sh cargo test --locked --lib --tests
echo "=== Checking documentation ==="
export RUSTDOCFLAGS="-D warnings"
doc=(./scripts/run-tool.sh cargo doc --locked --no-deps --document-private-items)
arm=(--target thumbv7em-none-eabihf)
"${doc[@]}" --lib
"${doc[@]}" --features embedded "${arm[@]}" --lib
"${doc[@]}" --features embedded "${arm[@]}" --bin bt2usb --bin bt2usb-selftest
"${doc[@]}" --features sim "${arm[@]}" --bin bt2usb-sim
unset RUSTDOCFLAGS
echo "=== Checking Markdown documentation ==="
"$(command -v python3 || command -v python)" scripts/check_docs.py
echo "=== Building release ==="
./scripts/run-tool.sh cargo build --locked --features embedded --target thumbv7em-none-eabihf --release
./scripts/run-tool.sh cargo build --locked --features sim --target thumbv7em-none-eabihf
echo "=== All checks passed! ==="
```

## deps

> Install all required tools for development

```bash
set -e
echo "Installing Rust target..."
./scripts/run-tool.sh rustup target add thumbv7em-none-eabihf

echo "Installing embedded tools..."
./scripts/run-tool.sh cargo install --locked probe-rs-tools --version 0.32.0
./scripts/run-tool.sh cargo install --locked cargo-binutils --version 0.4.0
./scripts/run-tool.sh cargo install --locked cargo-bloat --version 0.12.1
./scripts/run-tool.sh cargo install --locked mask --version 0.11.7

echo "Installing coverage tools..."
$MASK coverage-install

echo "Installing LLVM tools..."
./scripts/run-tool.sh rustup component add llvm-tools

echo "Done! All dependencies installed."
echo ""
echo "Quick start:"
echo "  mask test      - Run unit tests"
echo "  mask coverage  - Run tests with coverage"
echo "  mask flash     - Build and flash to device"
```
