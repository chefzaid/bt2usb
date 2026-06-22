#!/bin/bash
# Post-create script for bt2usb devcontainer
# Installs embedded Rust toolchain and probe-rs

set -euo pipefail

echo "══════════════════════════════════════════════════════════════════════════"
echo "  bt2usb Development Environment Setup"
echo "══════════════════════════════════════════════════════════════════════════"

# - Install ARM Cortex-M target -----------------------
echo "→ Installing thumbv7em-none-eabihf target..."
rustup target add thumbv7em-none-eabihf

# - Install cargo tools for embedded development ---------------
echo "→ Installing flip-link (stack overflow protection)..."
cargo install --locked flip-link || true

echo "→ Installing probe-rs (flashing & debugging)..."
cargo install --locked probe-rs-tools || true

echo "→ Installing mask task runner..."
cargo install --locked mask || true

echo "→ Installing cargo-llvm-cov (coverage)..."
cargo install --locked cargo-llvm-cov || true

echo "→ Installing cargo-binutils (objcopy, nm, size)..."
cargo install --locked cargo-binutils || true
rustup component add llvm-tools llvm-tools-preview

# - Setup udev rules for probe access (requires privileged container) ----
echo "→ Setting up probe-rs udev rules..."
if command -v probe-rs &> /dev/null; then
    # Generate udev rules for common debug probes
    if command -v sudo &> /dev/null; then
        sudo bash -c 'cat > /etc/udev/rules.d/69-probe-rs.rules << EOF
# J-Link
ATTRS{idVendor}=="1366", MODE="0666"
# ST-Link
ATTRS{idVendor}=="0483", ATTRS{idProduct}=="3748", MODE="0666"
ATTRS{idVendor}=="0483", ATTRS{idProduct}=="374b", MODE="0666"
ATTRS{idVendor}=="0483", ATTRS{idProduct}=="374e", MODE="0666"
ATTRS{idVendor}=="0483", ATTRS{idProduct}=="374f", MODE="0666"
ATTRS{idVendor}=="0483", ATTRS{idProduct}=="3752", MODE="0666"
ATTRS{idVendor}=="0483", ATTRS{idProduct}=="3753", MODE="0666"
# CMSIS-DAP / DAPLink
ATTRS{product}=="*CMSIS-DAP*", MODE="0666"
# nRF DK / Dongle
ATTRS{idVendor}=="1915", MODE="0666"
EOF'
        echo "   ✓ udev rules installed"
    else
        echo "   ⚠ sudo not available; skipping udev rules"
    fi
else
    echo "   ⚠ probe-rs not found, skipping udev rules"
fi

# - Verify installation ---------------------------
echo ""
echo "- Verification -----------------------------"
echo "Rust version:     $(rustc --version)"
echo "Cargo version:    $(cargo --version)"
echo "Host target:      $(rustc -vV | sed -n 's/^host: //p')   (host tests build for this)"
echo "Target installed: $(rustup target list --installed | grep thumb | head -1)"
echo "flip-link:        $(flip-link --version 2>/dev/null || echo 'not found')"
echo "probe-rs:         $(probe-rs --version 2>/dev/null || echo 'not found')"
echo "mask:             $(mask --version 2>/dev/null || echo 'not found')"
echo "cargo-llvm-cov:   $(cargo llvm-cov --version 2>/dev/null || echo 'not found')"

# - Smoke-test the host test pipeline (no hardware required) -----------
# This is the layer of testing that runs fully in the container/WSL without a
# probe. If this fails, the dev environment is broken regardless of hardware.
# `cargo test` builds for the host by default (no global build.target is set).
echo ""
echo "- Host test smoke check (no hardware needed) -----------------------------"
if cargo test --lib --tests >/dev/null 2>&1; then
    echo "   ✓ host unit + integration tests pass"
else
    echo "   ⚠ host tests did not pass (run 'mask test' to see details)"
fi

echo ""
echo "══════════════════════════════════════════════════════════════════════════"
echo "  Setup complete!"
echo ""
echo "  Software testing (NO hardware required, runs here in the container/WSL):"
echo "    mask test         # host unit + integration tests"
echo "    mask coverage     # coverage report (cargo-llvm-cov)"
echo "    mask check        # type-check the embedded build"
echo "    mask clippy       # lints on the embedded build"
echo "    mask build --release"
echo ""
echo "  On-target simulation in Renode (no hardware; one-time setup):"
echo "    mask sim-setup    # install Renode + renode-test deps into ~/.local"
echo "    mask sim-test     # headless Renode test of the SoftDevice-free build"
echo ""
echo "  Full end-to-end (BLE SoftDevice + USB) requires a real nRF52840-DK:"
echo "    mask run --release   # flash + RTT logs over a probe (WSL: usbipd-win)"
echo ""
echo "  See the README 'Testing Strategy' section for the layered approach."
echo "══════════════════════════════════════════════════════════════════════════"
