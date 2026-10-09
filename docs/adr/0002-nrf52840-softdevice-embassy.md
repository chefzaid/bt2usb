# ADR 0002: Build On nRF52840, Nordic SoftDevice S140, And Embassy

- Status: Accepted
- Date: 2026-10-09

## Context

The bridge must be a BLE central with two simultaneous links and bonding, and a
full-speed USB HID device, on one low-cost part that is easy to obtain as a
development kit. It must keep links responsive while the USB host is idle or
suspended, and it should be written in memory-safe code that can share its
decision logic with host tests.

## Decision

- Target the nRF52840: integrated BLE radio, native USB device, 1 MiB flash,
  256 KiB RAM. The nRF52840-DK pin mapping is the reference board.
- Use Nordic SoftDevice S140 v7.3.0 as the BLE stack, installed separately from
  the application, through the `nrf-softdevice` Rust bindings.
- Write `no_std` Rust on Embassy's cooperative async executor, with static
  allocation only: bounded `heapless` collections and fixed-capacity channels,
  no global allocator.
- Use `defmt` over RTT for diagnostics and `probe-rs` for flashing.

## Rationale

The SoftDevice is Nordic's qualified BLE stack and supports the central,
bonding, and multi-link roles bt2usb needs; a pure-Rust stack would add
interoperability risk to the most hardware-dependent part of the device.

Embassy expresses many concurrent I/O waits (two BLE links, three USB
endpoints, buttons, display) without per-task stacks or an RTOS. Static
allocation makes worst-case memory visible at link time and removes allocator
failure modes from firmware paths.

## Consequences

- Flash and RAM below the application are reserved for the SoftDevice; see the
  [memory map](../hardware.md#memory-layout). The application RAM origin must
  match the SoftDevice requirement measured on hardware.
- A full-chip erase removes the SoftDevice, which must be reinstalled before
  the application can start.
- Flash writes contend with radio timeslots and need retries.
- Blocking work in any task delays the whole executor.
- Other MCUs are not supported ports; each would need its own BLE, USB,
  storage, and memory design.
