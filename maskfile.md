# bt2usb Task Runner

Common development tasks for the bt2usb Bluetooth-to-USB HID bridge.

> Requires [mask](https://github.com/jacobdeichert/mask) (`cargo install mask`)

## build

> Build the firmware for nRF52840

**OPTIONS**
* release
    * flags: --release
    * desc: Build release firmware (optimized for size)

```bash
if [[ "${release}" == "true" ]]; then
    ./scripts/run-tool.sh cargo build --features embedded --target thumbv7em-none-eabihf --release
else
    ./scripts/run-tool.sh cargo build --features embedded --target thumbv7em-none-eabihf
fi
```

## build-release

> Build the firmware for nRF52840 (release mode, optimized for size)

```bash
./scripts/run-tool.sh cargo build --features embedded --target thumbv7em-none-eabihf --release
```

## run

> Build, flash, and run firmware on the connected nRF52840 board

**OPTIONS**
* release
    * flags: --release
    * desc: Build and flash release firmware (optimized for size)

```bash
if [[ "${release}" == "true" ]]; then
    ./scripts/run-tool.sh cargo run --features embedded --target thumbv7em-none-eabihf --release
else
    ./scripts/run-tool.sh cargo run --features embedded --target thumbv7em-none-eabihf
fi
```

## flash

> Build and flash firmware to the connected nRF52840 board

```bash
./scripts/run-tool.sh cargo run --features embedded --target thumbv7em-none-eabihf --release
```

## flash-debug

> Flash debug build with RTT logging enabled

```bash
./scripts/run-tool.sh cargo run --features embedded --target thumbv7em-none-eabihf
```

## test

> Run unit + integration tests on host (Windows/Linux/macOS)

Runs the library unit tests AND the `tests/` integration tests, building for the
native host. The embedded binary target is skipped automatically (it is gated
behind `required-features = ["embedded"]`). This works on Windows, Linux, macOS,
and inside the WSL2 devcontainer because `.cargo/config.toml` no longer pins a
global `build.target` (embedded tasks pass `--target` explicitly instead).

```bash
./scripts/run-tool.sh cargo test --lib --tests
```

## test-verbose

> Run unit + integration tests with output shown

```bash
./scripts/run-tool.sh cargo test --lib --tests -- --nocapture
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
- `--html` - Generate HTML report and open in browser
- `--json` - Output JSON format for CI integration

```bash
# Try cargo-llvm-cov first (cross-platform), fallback to tarpaulin
if ./scripts/run-tool.sh cargo llvm-cov --version >/dev/null 2>&1; then
    if [[ "${html:-false}" == "true" ]]; then
        ./scripts/run-tool.sh cargo llvm-cov --lib --tests --html --output-dir coverage-html
        echo "Coverage report: coverage-html/html/index.html"
    elif [[ "${json:-false}" == "true" ]]; then
        ./scripts/run-tool.sh cargo llvm-cov --lib --tests --json --output-path coverage.json
        echo "Coverage report: coverage.json"
    else
        ./scripts/run-tool.sh cargo llvm-cov --lib --tests
    fi
elif ./scripts/run-tool.sh cargo tarpaulin --version >/dev/null 2>&1; then
    if [[ "${html:-false}" == "true" ]]; then
        ./scripts/run-tool.sh cargo tarpaulin --lib --out Html --output-dir coverage
        echo "Coverage report: coverage/tarpaulin-report.html"
    elif [[ "${json:-false}" == "true" ]]; then
        ./scripts/run-tool.sh cargo tarpaulin --lib --out Json --output-dir coverage
        echo "Coverage report: coverage/coverage.json"
    else
        ./scripts/run-tool.sh cargo tarpaulin --lib --out Stdout
    fi
else
    echo "No coverage tool found. Install one of:"
    echo "  cargo install cargo-llvm-cov"
    echo "  cargo install cargo-tarpaulin (Linux only)"
    exit 1
fi
```

## coverage-html

> Generate HTML coverage report

```bash
if ./scripts/run-tool.sh cargo llvm-cov --version >/dev/null 2>&1; then
    ./scripts/run-tool.sh cargo llvm-cov --lib --tests --html --output-dir coverage-html
    echo "Coverage report: coverage-html/html/index.html"
elif ./scripts/run-tool.sh cargo tarpaulin --version >/dev/null 2>&1; then
    ./scripts/run-tool.sh cargo tarpaulin --lib --out Html --output-dir coverage
    echo "Coverage report: coverage/tarpaulin-report.html"
else
    echo "No coverage tool found. Install one of:"
    echo "  cargo install cargo-llvm-cov"
    echo "  cargo install cargo-tarpaulin (Linux only)"
    exit 1
fi
```

## coverage-json

> Generate JSON coverage report

```bash
if ./scripts/run-tool.sh cargo llvm-cov --version >/dev/null 2>&1; then
    ./scripts/run-tool.sh cargo llvm-cov --lib --tests --json --output-path coverage.json
    echo "Coverage report: coverage.json"
elif ./scripts/run-tool.sh cargo tarpaulin --version >/dev/null 2>&1; then
    ./scripts/run-tool.sh cargo tarpaulin --lib --out Json --output-dir coverage
    echo "Coverage report: coverage/coverage.json"
else
    echo "No coverage tool found. Install one of:"
    echo "  cargo install cargo-llvm-cov"
    echo "  cargo install cargo-tarpaulin (Linux only)"
    exit 1
fi
```

## coverage-install

> Install code coverage tools

```bash
echo "Installing cargo-llvm-cov (recommended, cross-platform)..."
./scripts/run-tool.sh cargo install cargo-llvm-cov
./scripts/run-tool.sh rustup component add llvm-tools-preview
echo "Done! Run 'mask coverage' to generate reports."
```

## check

> Type-check the embedded build without compiling

```bash
./scripts/run-tool.sh cargo check --features embedded --target thumbv7em-none-eabihf
```

## clippy

> Run clippy lints on embedded build

```bash
./scripts/run-tool.sh cargo clippy --features embedded --target thumbv7em-none-eabihf -- -D warnings
```

## fmt

> Format all Rust code

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
./scripts/run-tool.sh cargo size --features embedded --target thumbv7em-none-eabihf --release -- -A
```

## bloat

> Analyze what's contributing to binary size (requires cargo-bloat)

```bash
./scripts/run-tool.sh cargo bloat --features embedded --target thumbv7em-none-eabihf --release -n 30
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
`mask sim-test`. (On Windows, run it from inside WSL — see README.)

```bash
bash scripts/install-renode.sh
```

## sim-build

> Build the SoftDevice-free simulation firmware for Renode (Layer 3, no hardware)

```bash
./scripts/run-tool.sh cargo build --features sim --target thumbv7em-none-eabihf
echo "Sim ELF: target/thumbv7em-none-eabihf/debug/bt2usb-sim"
```

## sim

> Build + run the simulation firmware in Renode, GUI (requires `renode` on PATH)

Boots the SoftDevice-free firmware on a simulated nRF52840; UART0 output (the
coordinator + UI logic running on the target) appears in the Renode terminal
window. No probe or board needed. See README "Renode simulation".

```bash
./scripts/run-tool.sh cargo build --features sim --target thumbv7em-none-eabihf
if command -v renode >/dev/null 2>&1; then
    renode renode/bt2usb-sim.resc
else
    echo "Renode not found on PATH. Install it from https://renode.io"
    echo "then run:  renode renode/bt2usb-sim.resc"
    exit 127
fi
```

## sim-test

> Build + run the headless Renode robot test (asserts the sim's UART output)

Boots the sim in Renode (no GUI) and asserts that both pure cores
(`ble::coordinator` and `ui::ui_logic`) run on the simulated MCU. Suitable for
CI. Requires `renode-test` on PATH (ships with Renode).

```bash
./scripts/run-tool.sh cargo build --features sim --target thumbv7em-none-eabihf
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

Downloads and flashes the SoftDevice if not present.

```bash
SD_URL="https://nsscprodmedia.blob.core.windows.net/prod/software-and-other-downloads/softdevices/s140/s140_nrf52_7.3.0.zip"
SD_HEX="s140_nrf52_7.3.0_softdevice.hex"

if [ ! -f "$SD_HEX" ]; then
    echo "Downloading SoftDevice S140 v7.3.0..."
    curl -L "$SD_URL" -o softdevice.zip
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
docker build -t bt2usb-dev -f .devcontainer/Dockerfile .devcontainer
```

## probe-list

> List connected debug probes

```bash
./scripts/run-tool.sh probe-rs list
```

## doc

> Generate and open documentation

```bash
./scripts/run-tool.sh cargo doc --features embedded --target thumbv7em-none-eabihf --open
```

## ci

> Run all CI checks (fmt, clippy, test, build)

```bash
set -e
echo "=== Checking format ==="
./scripts/run-tool.sh cargo fmt -- --check
echo "=== Running clippy ==="
./scripts/run-tool.sh cargo clippy --features embedded --target thumbv7em-none-eabihf -- -D warnings
echo "=== Running tests ==="
./scripts/run-tool.sh cargo test --lib --tests
echo "=== Building release ==="
./scripts/run-tool.sh cargo build --features embedded --target thumbv7em-none-eabihf --release
echo "=== All checks passed! ==="
```

## deps

> Install all required tools for development

```bash
set -e
echo "Installing Rust target..."
./scripts/run-tool.sh rustup target add thumbv7em-none-eabihf

echo "Installing embedded tools..."
./scripts/run-tool.sh cargo install probe-rs-tools flip-link cargo-binutils cargo-bloat mask

echo "Installing coverage tools..."
./scripts/run-tool.sh cargo install cargo-llvm-cov
./scripts/run-tool.sh rustup component add llvm-tools-preview

echo "Installing LLVM tools..."
./scripts/run-tool.sh rustup component add llvm-tools

echo "Done! All dependencies installed."
echo ""
echo "Quick start:"
echo "  mask test      - Run unit tests"
echo "  mask coverage  - Run tests with coverage"
echo "  mask flash     - Build and flash to device"
```
