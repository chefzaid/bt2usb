# bt2usb

bt2usb is Rust firmware for an nRF52840 that lets a Bluetooth LE keyboard and
mouse work through a monitor's USB hub. It connects to BLE HID peripherals and
presents a standard USB keyboard, mouse, and consumer-control device to the PC,
with an SSD1306 OLED and three buttons for pairing and status.

[![CI](https://github.com/chefzaid/bt2usb/actions/workflows/ci.yml/badge.svg)](https://github.com/chefzaid/bt2usb/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/Rust-1.95.0%20no__std-orange.svg)](rust-toolchain.toml)
[![MCU](https://img.shields.io/badge/MCU-nRF52840-blue.svg)](docs/hardware.md)
[![SoftDevice](https://img.shields.io/badge/SoftDevice-S140%207.3.0-blue.svg)](docs/first-flash.md#1-softdevice-once-per-board)
[![License](https://img.shields.io/badge/License-GPL%203.0-green.svg)](LICENSE)

## Status

This is development firmware. Its behavior is implemented and covered by host
tests, embedded builds, and a Renode simulation, but it has not yet passed the
[first-flash hardware checklist](docs/first-flash.md) on a real board. What
works today is in [Features](docs/features.md); every task, done and open, is in
[TODO.md](TODO.md); known security limits are in [Security](docs/security.md).

## Documentation

- [Features and how to use it](docs/features.md)
- [Hardware, wiring and configuration](docs/hardware.md)
- [First flash: board bring-up checklist](docs/first-flash.md)
- [Architecture overview and ADR index](docs/architecture.md)
- [Data model reference](docs/data-model.md)
- [Development guide](docs/development.md)
- [Testing guide](docs/testing.md)
- [Code quality](docs/code-quality.md)
- [Deployment: releases, provenance and flashing](docs/deployment.md)
- [Operations runbook](docs/operations.md)
- [Security reference](docs/security.md) and [security policy](SECURITY.md)
- [Task reference](maskfile.md)

## Roadmap

[TODO.md](TODO.md) is the complete work plan: finished work is checked off with
its source references, and open work carries a priority and an acceptance
criterion. Hardware gates stay open until board evidence exists.

## Quick Start

You need an nRF52840-DK, an SSD1306 128×64 I2C display, and Rust through
rustup; see [hardware](docs/hardware.md) for wiring.

```sh
rustup target add thumbv7em-none-eabihf
cargo install --locked probe-rs-tools mask
mask test                # host tests, no board needed
mask build --release     # bridge and self-test firmware
```

On a new board, follow [first flash](docs/first-flash.md) to install SoftDevice
S140, run the self-test, and validate the bridge; then `mask run --release`.
The [development guide](docs/development.md) covers every task, Windows/WSL, and
the devcontainer.

## License

GNU GPL v3; see [LICENSE](LICENSE). Nordic SoftDevice is a separate component
obtained from Nordic.
