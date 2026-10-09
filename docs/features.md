# Features

This guide lists what the bt2usb firmware implements today and how to use it.
Planned work lives in [TODO.md](../TODO.md). An implemented feature is supported
by source and software tests; it is hardware-verified only when a recorded
[first-flash](first-flash.md) result covers it.

## What It Does

bt2usb lets a Bluetooth LE keyboard and mouse be used through a monitor's USB
hub. The nRF52840 connects to BLE HID peripherals as a central and presents a
standard USB keyboard, mouse, and consumer-control device to the PC, so the
host needs no Bluetooth stack or driver. An SSD1306 OLED and three buttons
handle pairing and status.

Bluetooth Classic devices and arbitrary HID report layouts are not supported.
Behavior with a particular peripheral, host, hub, sleep mode, or BIOS is a
hardware acceptance question, not a guarantee.

## Using The Bridge

### Pair and connect

1. Put the BLE keyboard or mouse in pairing mode.
2. Press SELECT to scan, choose a device with UP/DOWN, and press SELECT to connect.
3. From the connected screen, press SELECT to scan for a second device. DOWN
   disconnects the current connections; it does not erase stored pairing keys.
4. Previously stored devices are selected for reconnect on the next boot.

### Manage saved devices

Press UP from Home, Connected, or an error screen to manage saved devices. Choose
a device to forget, or the final **Factory reset** entry to clear all saved peers.
SELECT opens a confirmation with **Cancel** selected; choose the action with DOWN
and press SELECT again to confirm. UP selects Cancel. These actions change
pairing records, not the installed firmware. Successful completion stays visible
until acknowledged; a storage error means the change did not complete.

### Wake and display

USB wake requires a new keyboard key/modifier, consumer key, or mouse button
press. Mouse movement, scroll, releases, and already-held input do not request a
wake. Host permission and hub power still determine whether the PC wakes.

The OLED turns off after 120 seconds without activity. While USB is active, a
button press wakes it and is consumed so waking does not trigger a menu action.
During USB suspend it stays off until the host resumes. Timing and USB settings
are in [src/config.rs](../src/config.rs) and listed in
[hardware](hardware.md#configuration-defaults).

## BLE Central

- Scanning with HID advertisement filtering, UTF-8 names, and merged
  scan-response names.
- GATT HID-over-GATT discovery, report-reference classification, and Report Map
  reads up to 512 bytes across MTU boundaries.
- Bonding with encrypted links required before HID discovery. New pairing is
  started only by an explicit user connection.
- Two simultaneous connection slots and up to four stored peers.
- Automatic reconnect after boot and after link loss, including peers whose
  private address rotates, with a 6-second connect timeout and backoff.
- Keyboard LED state (Caps Lock and others) forwarded to BLE keyboards.

## USB HID Device

- Composite keyboard, mouse, and consumer-control interfaces with boot
  subclass support and boot/report protocol switching.
- Six-key keyboard reports, five-button mouse with vertical and horizontal
  scroll, and one consumer usage at a time.
- USB remote wakeup on new presses, suspend/resume handling, and SoftDevice USB
  power (VBUS) events.
- A stable per-unit USB serial derived from the chip's factory device ID.
- Development VID/PID `0x1209`/`0x0001`; a production identity is open work.

## Input Delivery

- Per-source tracking: keys, modifiers, and buttons from two devices are
  unioned, and releasing or disconnecting one keeps the other's held input.
- Held input is released when a BLE link ends.
- Independent, bounded keyboard, mouse, and consumer endpoint workers; an
  unpolled endpoint cannot block the others.
- Current held state is replayed after USB reset, configuration, resume, or
  protocol change; mouse motion is never replayed.

The delivery and loss policy is described in
[architecture](architecture.md#hid-path-and-limits).

## Pairing Storage

- Versioned, validated pairing records in four reserved flash pages.
- Fail-closed loading: an unreadable or unsupported store is preserved and
  writes are disabled until an explicit Factory reset.
- Forget and Factory reset with default-Cancel confirmations, worker shutdown
  before mutation, and commit-then-cache updates.

See the [data model](data-model.md#pairing-store) for the format.

## Local UI And Power

- Async OLED rendering isolated in its own task, with retries and a bounded
  operation deadline; a failed display does not stop the bridge.
- Three debounced buttons with a persistent UI ticker.
- Errors and completion notices retained until acknowledged.
- Activity-driven display power-off; the bridge stays connected rather than
  entering System-OFF.

## Bring-Up And Diagnostics

- `bt2usb-selftest`, a staged board self-test for SoftDevice, flash, USB, OLED,
  buttons, and BLE scanning (`mask selftest`).
- `defmt` RTT logs, including the SoftDevice RAM requirement and painted-stack
  high-water marks.
- A [first-flash checklist](first-flash.md) for hardware acceptance.

## Development And Release Tooling

- Host unit and integration tests for all hardware-free logic, with coverage tasks.
- A SoftDevice-free Renode simulation with custom GPIO/GPIOTE models and a
  headless Robot test.
- `mask` task recipes, a VS Code devcontainer, and WSL-aware tool resolution.
- GitHub Actions for host, embedded, simulation, and dependency-audit checks,
  and tag-based draft releases with checksums and signed provenance.

## Current Technical Boundaries

Do not describe these as implemented:

- authenticated pairing (passkey or numeric comparison) or an enrollment policy
- watchdog recovery or recorded reset causes
- power-loss-safe persistence beyond the current fail-closed loading
- descriptor-driven translation for NKRO, 16-bit motion, or vendor layouts
- `GET_REPORT`/`SET_IDLE` conformance work
- a production USB VID/PID
- signed firmware, secure boot, or USB/BLE DFU
- readout protection or a production provisioning procedure
- multiple BLE profile sets, monitor-input-aware switching, or a companion app
- other MCUs or boards

These are tracked in [TODO.md](../TODO.md).

## Related Guides

- [Architecture and ADRs](architecture.md)
- [Hardware](hardware.md)
- [First flash](first-flash.md)
- [Operations](operations.md)
- [Security](security.md)
