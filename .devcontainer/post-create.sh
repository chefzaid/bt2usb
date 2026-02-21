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
echo "Target installed: $(rustup target list --installed | grep thumb | head -1)"
echo "flip-link:        $(flip-link --version 2>/dev/null || echo 'not found')"
echo "probe-rs:         $(probe-rs --version 2>/dev/null || echo 'not found')"
echo "mask:             $(mask --version 2>/dev/null || echo 'not found')"
echo "cargo-llvm-cov:   $(cargo llvm-cov --version 2>/dev/null || echo 'not found')"

echo ""
echo "══════════════════════════════════════════════════════════════════════════"
echo "  Setup complete! You can now build with:"
echo "    mask build --release"
echo ""
echo "  Run tests with:"
echo "    mask test"
echo ""
echo "  Flash to device with:"
echo "    mask run --release"
echo "══════════════════════════════════════════════════════════════════════════"
