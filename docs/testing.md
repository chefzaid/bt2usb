# Testing Guide

Software checks and board checks cover different failure modes. Passing host
tests or Renode does not establish radio interoperability, USB compliance,
flash durability, security certification, or production readiness.

This guide lists every test location and what it proves, the commands that run
each layer locally and in CI, the rules for adding tests, and the dated record
of what has actually been run. The reasons for the layering are in
[ADR 0004](adr/0004-layered-verification.md); the board procedure is the
[first-flash checklist](first-flash.md).

## Test Layers

Each layer owns a failure class the previous one cannot see
([ADR 0004](adr/0004-layered-verification.md)).

| Layer | What it exercises | What it cannot establish |
| --- | --- | --- |
| Host unit/integration tests | Shared HID parsing/serialization, reducers, advert parsing, power/UI rules, report coalescing, storage framing and record validation | Async driver timing and peripheral behavior |
| Embedded checks | Firmware/self-test compile and lint for the ARM target, linker reservations | Runtime correctness on hardware |
| Renode | SoftDevice-free ARM boot, GPIO/timer paths, UI/coordinator scenario | Actual BLE, USB, SoftDevice, or flash persistence |
| Board self-test | SoftDevice enable, flash access, USB enumeration/report, OLED, buttons, scan | Long-term reliability or the full peripheral matrix |
| Hardware acceptance | Pairing, reconnect, held-input release, monitor hub, sleep/wake, pre-OS operation | Untested host/peripheral combinations |

Where each layer runs and what it needs:

| Layer | Runs in | Needs |
| --- | --- | --- |
| Host unit/integration tests | Local, CI on Linux and Windows, devcontainer setup smoke check | Pinned Rust toolchain only |
| Embedded checks | Local, CI | The `thumbv7em-none-eabihf` target |
| Release helper tests | Local, CI on Linux and Windows | Python 3.11 or newer |
| Renode | Local Linux/WSL, CI on Linux | Renode 1.16.1 and the `renode-test` Python dependencies |
| Board self-test | Local only | nRF52840 board with the DK pin map, debug probe, SoftDevice S140 v7.3.0 |
| Hardware acceptance | Local only | Board, peripherals, host, monitor hub, and a person |

A change is verified only as deep as the deepest layer that actually ran.
Record which layers ran, and report a skipped layer as skipped, not passed.

## Quick Commands

Run from the repository root. Mask recipes call Cargo through
[run-tool.sh](../scripts/run-tool.sh) and are Bash scripts; in Windows
PowerShell, use the direct command.

| Goal | Mask | Direct command |
| --- | --- | --- |
| Host unit and integration tests | `mask test` | `cargo test --locked --lib --tests` |
| Host tests with output | `mask test-verbose` | `cargo test --locked --lib --tests -- --nocapture` |
| One module or test name | — | `cargo test --locked --lib coordinator` |
| Integration tests only | — | `cargo test --locked --test integration` |
| Coverage summary | `mask coverage` | `cargo llvm-cov --locked --lib --tests` |
| Coverage HTML / JSON | `mask coverage --html` / `--json` | `cargo llvm-cov --locked --lib --tests --html --output-dir coverage-html` |
| Host lint | — | `cargo clippy --locked --lib --tests -- -D warnings` |
| Embedded lint | `mask clippy` | `cargo clippy --locked --features embedded --target thumbv7em-none-eabihf -- -D warnings` |
| Simulation lint | — | `cargo clippy --locked --features sim --target thumbv7em-none-eabihf -- -D warnings` |
| Firmware and self-test build | `mask build --release` | `cargo build --locked --features embedded --target thumbv7em-none-eabihf --release` |
| Simulation build | `mask sim-build` | `cargo build --locked --features sim --target thumbv7em-none-eabihf` |
| Headless Renode test | `mask sim-test` | `renode-test renode/bt2usb-sim.robot` |
| Interactive Renode | `mask sim` | `renode renode/bt2usb-sim.resc` |
| Release helper tests | — | `python -m unittest discover -s scripts -p "release_test.py" -v` |
| Format check | `mask fmt-check` | `cargo fmt --package bt2usb -- --check` |
| Host API documentation | — | `cargo doc --locked --no-deps --lib` with `RUSTDOCFLAGS=-D warnings` |
| Workflow lint | — | `actionlint` |
| Dependency audit | — | `cargo audit` |
| Board self-test | `mask selftest` | `cargo run --locked --features embedded --target thumbv7em-none-eabihf --release --bin bt2usb-selftest` |
| Local software gate | `mask ci` | See [what `mask ci` covers](#local-and-ci-coverage-compared) |

`mask sim-setup` installs Renode and the `renode-test` dependencies, and
`mask coverage-install` installs `cargo-llvm-cov`. `actionlint` and
`cargo audit` are not installed by any mask recipe; CI pins actionlint 1.7.12
and cargo-audit 0.22.2. [ADR 0013](adr/0013-pinned-toolchain-and-mask-tasks.md)
explains the pinned toolchain, `--locked`, and the mask wrappers.

## Host Tests And Coverage

```sh
cargo test --locked --lib --tests
cargo test --locked --lib coordinator
cargo test --locked --lib ui_logic
mask coverage
mask coverage --html
```

The library exposes the same hardware-free code used by firmware, not a separate
implementation. Some hardware-coupled modules are absent from the host crate;
the coverage percentage measures only the instrumented selection. Report the
commit, toolchain, command, and excluded modules with coverage results. Avoid
putting an undated coverage percentage or test count in the README.

`--lib --tests` selects the library's unit tests and `tests/integration.rs`.
The three binaries are skipped because their `embedded` or `sim` feature is not
enabled. Doctests are not selected; the library's fenced doc blocks are all
`text` blocks, which rustdoc would not run anyway. Host tests build for the native
platform because [.cargo/config.toml](../.cargo/config.toml) deliberately sets
no global build target; do not pass `--target` or a firmware feature to
`cargo test`.

A filter after `--lib` matches test paths by substring, so `coordinator` runs
the coordinator reducer tests and `ui_logic` the UI state-machine tests. Use a
full test name to run one test, and `-- --nocapture` to see its output.

### Host Library Composition

[lib.rs](../src/lib.rs) is `no_std` except under `cfg(test)`. It declares
`hid` as an ordinary module and pulls the other pure modules in by `#[path]`,
re-exporting them through its own inline `ble`, `ui`, and `power_logic`
modules, so firmware and tests compile the same files:

| Compiled into the host library | Not compiled into the host library |
| --- | --- |
| `src/hid/` (all submodules), `src/ble/adv_parser.rs`, `conn_params.rs`, `coordinator.rs` (with `coordinator_tests.rs`), `reconnect.rs`, `long_read.rs`, `management.rs`, `src/power_logic.rs`, `src/ui/display_logic.rs`, `input_logic.rs`, `ui_logic.rs` (with `ui_logic_tests.rs`), `src/config.rs`, and, under `cfg(test)` only, `src/storage/framing.rs` and `record.rs` | `src/ble/mod.rs`, `multi_conn.rs`, `hid_client.rs`, `scanner.rs`; `src/storage.rs`, `src/storage/codec.rs`; `src/usb/`; `src/ui/mod.rs`, `display.rs`, `buttons.rs`; `src/power.rs`, `src/stack.rs`, `src/sd_setup.rs`; the `main.rs`, `selftest.rs`, and `sim.rs` entry points |

Apart from `ui/mod.rs`, which only declares modules, everything in the
right-hand column depends on
SoftDevice, Embassy, or peripheral types. It is
checked by the embedded build and Clippy, partly by
Renode (`ui/buttons.rs`), by the board self-test, and by hardware acceptance;
see [Modules Without Host Tests](#modules-without-host-tests).

### Coverage

`mask coverage` uses `cargo-llvm-cov` when it is installed and falls back to
`cargo-tarpaulin` (Linux only). Output locations:

| Tool | Summary | HTML | JSON |
| --- | --- | --- | --- |
| cargo-llvm-cov | Terminal | `coverage-html/html/index.html` | `coverage.json` |
| cargo-tarpaulin | Terminal | `coverage/tarpaulin-report.html` | `coverage/coverage.json` |

Both tools run with `--lib --tests`, so both include `tests/integration.rs`;
their percentages still differ because they instrument differently (see
[code quality](code-quality.md#coverage)). These output paths are ignored by
Git. CI does not run coverage. The latest local llvm-cov figure is 97.48% host
line coverage on 2026-10-10.

## Test Map

Counts below were taken with `grep -c '#\[test\]' <file>` on each file on
2026-10-10, in the commit that adds the scan-screen link-change tests. The
tree holds 285 `#[test]` functions: 282 in files compiled into the host library
and 3 in
`tests/integration.rs`, and every one of them runs under
`cargo test --locked --lib --tests` (see
[Tests That Do Not Run](#tests-that-do-not-run)). The
[2026-10-09 validation record](#validation-record--2026-10-09) ran 260 unit
tests, before four advertisement tests moved into the host library and
fourteen UI tests (the management deadline, the saved-device list, scans, and
`UiState` link updates) and four keyboard-report tests were added; the 282 passed
with `cargo test` on 2026-10-10. There are no `#[ignore]` or
`#[should_panic]` tests.

### HID Reports, Descriptors, And Delivery

| Location | Tests | Behavior covered |
| --- | --- | --- |
| [lib_tests.rs](../src/lib_tests.rs) | 53 | Keyboard, mouse, and consumer report parsing from BLE bytes and serialization to USB: empty, short, exact, and longer inputs; too-small output buffers; all modifiers and buttons; six-key arrays; negative motion and wheel; 5-byte mouse reports with horizontal pan and back/forward buttons; consumer volume, media, browser, and launcher usages. `classify_report` and `classify_notification` routing by report ID or length, rejecting a keyboard report with a nonzero reserved byte when its kind is only inferred, invalid 2-byte consumer payloads, unknown lengths, and empty or single-byte input. |
| [hid_descriptor_tests.rs](../src/hid_descriptor_tests.rs) | 34 | Report-descriptor parsing in `hid/report_protocol.rs`: usage pages, keyboard/mouse/consumer detection, Push/Pop, long items, bounded nesting, overflow-safe report dimensions, constant padding, unsupported applications, extended usages, and every truncated prefix of the firmware's own USB descriptors failing closed. Descriptor-guided routing (`classify_notification_with_hint`, `classify_known`) that rejects unknown or mixed-kind report IDs instead of falling back to another kind. GATT Report Reference parsing, consumer usage range, three-button boot mouse serialization, and GATT values whose first byte resembles a report ID. |
| [hid_keyboard_report_tests.rs](../src/hid_keyboard_report_tests.rs) | 10 | Which report is the keyboard's (`HidDescriptor::is_unnumbered_keyboard_only` and `is_keyboard_report`, the rule `subscribe_all` uses for the LED output report): an unnumbered keyboard-only map owns every report, a numbered map only the keyboard ID, and an unnumbered map with another input, or an ID shared by two kinds, owns none. The reserved keyboard byte: a keyboard report carrying OEM data there is accepted, with the reserved byte cleared, when `classify_known` is given the keyboard kind, when a Report Reference resolves through a numbered Report Map to the keyboard report as `subscribe_all` resolves it, and when an unnumbered map describes only a keyboard; the check stays when an unnumbered map also has other kinds and on the length- and ID-inferred paths; a declared keyboard report must still be 8 bytes. |
| [lib_logic_tests.rs](../src/lib_logic_tests.rs) | 14 | `HidReport` serialization, equality, and kind helpers; HID UUID detection, name extraction, malformed lengths, and name truncation through the public `ble::adv_parser` API; scan-dot cycling; `power_logic::screen_should_be_on` auto-off policy. |
| [hid/aggregate.rs](../src/hid/aggregate.rs) | 5 | Two-source union: a key held by both sources survives one release or disconnect, rollover recovers when a source disconnects, mouse buttons union without replaying the other source's motion, consumer lowest-slot priority with fallback, and out-of-range sources cannot change state or wake the host. |
| [hid/coalesce.rs](../src/hid/coalesce.rs) | 8 | Per-endpoint coalescing: latest keyboard and consumer state wins but a release survives, mouse motion accumulates with saturation while the latest buttons win, round-robin pop across endpoints, and endpoint independence. |
| [hid/delivery.rs](../src/hid/delivery.rs) | 6 | `EndpointDelivery` state: short taps keep press/release order, a failed press recovers the latest release rather than the failed packet, relative motion is never replayed after failure, resume, or reset, stale completions cannot erase post-reset input, queue overflow keeps the final release, and a blocked consumer endpoint does not block keyboard or mouse state. |
| [hid/delivery_tests.rs](../src/hid/delivery_tests.rs) | 4 | The production `run_endpoint` worker, polled by hand with fake queues, sinks, and a fake clock: an unpolled consumer endpoint does not stop keyboard and mouse writes, a press that times out (100 ms write deadline) is replaced by the latest release after a 20 ms backoff, a bus change cancels stale motion, and repeated errors back off 20, 40, 80, 160, 320, 640, then 1000 ms (capped) and recover. |
| [hid/consumer.rs](../src/hid/consumer.rs) | 4 | Consumer report defaults, a volume-up usage, serialization, and parsing from bytes. |
| [hid/host_leds.rs](../src/hid/host_leds.rs) | 5 | `forward_host_leds`, polled by hand with a fake host: a new link gets the host's current lock-key state first, a reconnecting keyboard gets a state the slot already forwarded to its previous link, nothing is written before the host sends a state, later changes follow in order, and the host's first state is forwarded when it arrives during the link. |
| [hid/keyboard.rs](../src/hid/keyboard.rs) | 4 | Host keyboard LED byte decoding: individual LEDs, Caps Lock with Num Lock, masking of undefined upper bits with round trip, and the all-off default. |
| [hid/wake.rs](../src/hid/wake.rs) | 3 | Remote-wake eligibility: only a new key or modifier wakes (the rollover error code does not); mouse motion, wheel, pan, and releases do not, a new button does; consumer input needs a new nonzero usage. |

### BLE Coordination And Discovery

| Location | Tests | Behavior covered |
| --- | --- | --- |
| [ble/coordinator_tests.rs](../src/ble/coordinator_tests.rs) | 26 | `ConnManager` slot state machine (reserve, connect, disconnect, ignored out-of-range slots, second slot when the first is busy, summary text) and the reducers: `plan_start_scan`, `plan_connect` (out of range, success, already connected acknowledges without a duplicate connect, already connecting waits, no free slot), `plan_disconnect`, `on_slot_connected` (persist and summary), `on_slot_disconnected`, `on_slot_error`, `on_slot_link_lost` keeping the slot reserved, reconnection, and disconnect during retry. `merge_advertisement` lets a name-only scan response update a known HID peer even when the list is full, and never enrolls a device without the HID UUID. |
| [ble/adv_parser.rs](../src/ble/adv_parser.rs) | 7 | Advertised names keep valid UTF-8 and truncate at a character boundary; a complete name beats a shortened one, and a shortened one is used when it is the only name; a missing, empty, or invalid name does not replace a known one. The HID UUID is found among other 16-bit UUIDs and in an incomplete UUID list, and an empty advertisement has neither the UUID nor a name. |
| [ble/long_read.rs](../src/ble/long_read.rs) | 4 | Bounded ATT Read/Read Blob assembly: no value until a short final fragment, an exact-MTU value needs an end response, a 512-byte value completes while an oversized one fails, and malformed termination never exposes a partial value. |
| [ble/management.rs](../src/ble/management.rs) | 5 | `commit` publishes only persisted state: a failed write keeps the store and bonds, and cancelled persistence never publishes the candidate. The `Quiescence` barrier suppresses reconnect events until the matching token is acknowledged, waits for both sources on reset, and ignores invalid slots. |
| [ble/reconnect.rs](../src/ble/reconnect.rs) | 23 | The shared background-reconnect table ([ADR 0015](adr/0015-shared-reconnect-scan.md)): a sighting goes to the slot that owns the device, unregistered devices and slots are ignored, a sighting is used once, replaced by a newer one, fresh at 2 s and discarded after, and dropped with its slot or when the slot changes target; re-registering the same target keeps the outage start, a different one restarts the fast window; the duty cycle is fast while any target is inside its window; after a failed attempt the other slot's scans ignore that device while its own still see it, the holdoff ends on time, survives re-registration, is extended by a new failure, ends for a new target, drops the pending sighting, and ignores unregistered slots; the lower slot wins a tie; out-of-range slots and a clock going backwards are harmless. |
| [ble/conn_params.rs](../src/ble/conn_params.rs) | 13 | Bounding a peripheral's connection parameter request ([ADR 0016](adr/0016-bounded-peer-connection-parameters.md)): a request inside the limits is granted unchanged, a peripheral asking only for 20–40 ms gets 20 ms, one asking for 50–100 ms gets 30 ms and is flagged as outside its range, an overlapping range is narrowed to 7.5–15 ms, a 32 s supervision timeout is capped at 4 s and a short one raised to 1 s, latency is capped at 20, reversed bounds read as a range (also by `interval_within_request`), a faster request gets 7.5 ms, latency is lowered when 4 s cannot cover it, and the timeout is raised to meet the Core rule. A sweep over every boundary of the policy and over out-of-range values (interval 0 and 0xFFFF, latency 500, timeout 0) checks that every answer stays inside the limits and the Core rule, that a peripheral accepting 15 ms is never slowed, and that any request reaching into the grantable range gets an interval it asked for. |

### Pairing Storage

| Location | Tests | Behavior covered |
| --- | --- | --- |
| [storage/framing.rs](../src/storage/framing.rs) | 8 | Versioned blob framing: empty and multi-record round trips in order, non-versioned data yields no records, the writer truncates cleanly when full, the reader stops on truncated or zero-length records, a complete frame needs the exact record count and length, and future or truncated versioned headers are never read as legacy data. |
| [storage/record.rs](../src/storage/record.rs) | 3 | Record metadata validation: name encoding, capacity, and base length; agreement between the bond flag and the record size; UTF-8 name lengths counted in bytes. |

The [data model](data-model.md#pairing-store) describes the layout these tests
protect.

### UI And Power Policy

| Location | Tests | Behavior covered |
| --- | --- | --- |
| [ui/ui_logic_tests.rs](../src/ui/ui_logic_tests.rs), for [ui_logic.rs](../src/ui/ui_logic.rs) | 33 | `on_button` transitions: SELECT scans from Home and Error, list navigation clamps, SELECT connects the highlighted entry, an empty list cannot connect, stale selections are clamped, SELECT on Connected rescans and DOWN disconnects, ignored combinations are no-ops. `on_scan_complete`. Management confirmations default to Cancel, one request runs at a time, stale replies are rejected, request IDs stay unique across wraparound, an unanswered request expires at its deadline and its late reply is ignored, an answered request never expires, a timeout shows **No reply** without claiming an outcome, drops the saved list, survives link updates, reopens saved devices with UP and is acknowledged with SELECT, and keeps an error already showing, an empty store still offers Factory reset, errors survive later status, and background status or scans do not dismiss a confirmation. Saved-device navigation reaches every entry and backs out, `UiState` counts saved devices on management screens, a completed change shows its notice and drops the saved list (an error stays), a scan lists results, reports none, or ignores a stray completion, a button scan clears old results, a new link or a drop on Home, Connecting, or Connected clears the list, a background connect or drop leaves a running scan or its picker and list on screen, and long messages are cut to 32 bytes. |
| [ui/display_logic.rs](../src/ui/display_logic.rs) | 2 | OLED retry backoff of 1, 2, 4, 8, 16, then 30 s (capped) without blocking new frames, reset on recovery, and saturating deadlines. |
| [ui/input_logic.rs](../src/ui/input_logic.rs) | 3 | The device-list window keeps the selection visible, handles an empty list and a stale selection, and the scan spinner recovers from an out-of-range state. |
| [power_logic.rs](../src/power_logic.rs) | 5 | Active, Idle, and LowPower decisions: USB suspend forces LowPower at once, idle beyond twice the timeout without a BLE link is LowPower while a link keeps Idle, and very large timeouts do not overflow. |

### Integration Tests

| Location | Tests | Behavior covered |
| --- | --- | --- |
| [tests/integration.rs](../tests/integration.rs) | 3 | Keyboard (report ID 1), mouse (ID 2), and consumer (ID 3) notifications classified and serialized through the crate's public `bt2usb::hid` API, as an external crate sees it. |

### Tests That Do Not Run

None. Every `#[test]` in `src/` and `tests/` is compiled by
`cargo test --locked --lib --tests`.

Until 2026-10-10, [scanner.rs](../src/ble/scanner.rs) held a `#[cfg(test)]`
module with 10 tests of HID UUID detection and name extraction that never ran:
the file is declared only in the firmware's [ble/mod.rs](../src/ble/mod.rs),
behind the `embedded` feature, [lib.rs](../src/lib.rs) does not include it, and
the firmware binaries cannot be built for the host test harness. Four of them
covered cases no host test did (a HID UUID among other UUIDs, an incomplete
UUID list, an empty advertisement, and a shortened name on its own) and now
live in [ble/adv_parser.rs](../src/ble/adv_parser.rs); the other six repeated
tests in `lib_logic_tests.rs` and were deleted. A test placed in a module that
only the firmware compiles will never run; put it beside the pure module it
exercises.

### Modules Without Host Tests

| Module | What checks it today |
| --- | --- |
| `ble/multi_conn.rs`, `ble/hid_client.rs`, `ble/scanner.rs` | Embedded build and Clippy; pure decisions they call are host-tested; hardware acceptance. The self-test scan stage checks the radio with its own scan loop and `ble/adv_parser.rs`; it does not run these modules |
| `storage.rs`, `storage/codec.rs` | Embedded build and Clippy; framing and record validation are host-tested; the self-test flash stage exercises the same region and `sequential-storage` map, not this code; hardware acceptance |
| `usb/hid_device.rs` | Embedded build and Clippy; delivery, aggregation, and wake policy are host-tested; self-test USB stages; hardware acceptance |
| `ui/buttons.rs` | Embedded and simulation builds and Clippy; Renode scenario (real GPIO edges through this module); hardware acceptance. The self-test button stages check wiring with their own `Input` code, not this module |
| `ui/display.rs` | Embedded build and Clippy; recovery policy is host-tested; self-test OLED stages |
| `stack.rs`, `sd_setup.rs` | Embedded build and Clippy; self-test SoftDevice and stack stages |
| `power.rs` | Embedded build and Clippy; its policy (`power_logic.rs`) is host-tested; hardware acceptance (sleep and wake) |

## Renode Simulation

The simulation runs the real `ble::coordinator` and `ui::ui_logic` modules on an
emulated nRF52840. Its BLE events are a scripted scenario. UART0 carries logs,
so no probe or defmt decoder is required.

Build from the repository root:

```sh
mask sim-build
# target/thumbv7em-none-eabihf/debug/bt2usb-sim
```

Install portable Renode and the Python test dependencies in Linux/WSL:

```sh
mask sim-setup
renode --version
```

The installer puts commands in `~/.local/bin`; ensure that directory is on PATH.
Run it inside WSL on Windows, from the repository directory. A separately
installed Renode distribution can also be used; record the version with results.

For interactive execution:

```sh
mask sim
```

In the Renode monitor, an active-low SELECT press and release is:

```text
gpio0 OnGPIO 24 false
gpio0 OnGPIO 24 true
```

Leave the press active longer than the configured debounce interval; UP is pin
11, DOWN is pin 12. The custom models in
[nrf52840_sense_gpio.cs](../renode/nrf52840_sense_gpio.cs) and the corresponding
REPL implement the SENSE/LATCH/GPIOTE PORT-event path used by Embassy. GPIO edges
therefore go through the actual button task and `BUTTON_CHANNEL` to the UI.

For repeatable headless execution:

```sh
mask sim-test
# Direct command after building:
renode-test renode/bt2usb-sim.robot
```

The Robot test asserts boot and coordinator output and uses GPIO presses to
check screen transitions. Run this when changing the simulation, GPIO path, or
shared reducers. A simulated scan completes immediately with scenario devices;
this is not a test of scan timing or advertisement interoperability.

### What The Simulation Build Contains

`--features sim` builds [sim.rs](../src/sim.rs) without SoftDevice, USB, flash
storage, or the OLED task, links it with [memory_sim.x](../memory_sim.x) from
address 0 (no SoftDevice reservation), and supplies the single-core
`cortex-m` critical section that the SoftDevice provides in firmware builds.
[build.rs](../build.rs) selects the memory map by feature and refuses to build
`embedded` and `sim` together. The debounce interval is `BUTTON_DEBOUNCE_MS`
(50 ms) from [config.rs](../src/config.rs).

UART0 output is written by the `slog!` macro in `sim.rs` (TX P0.06, RX P0.08 in
the code; Renode's UART model emits the bytes regardless of pin routing).
`defmt` messages from shared modules, such as the button driver's
`Button: …` line, go to the defmt RTT logger and do not appear on UART0.

The platform script loads Renode's stock `platforms/cpus/nrf52840.repl`,
unregisters its `gpiote`, `gpio0`, and `gpio1`, and loads
[nrf52840-sense-gpio.repl](../renode/nrf52840-sense-gpio.repl), which places the
custom models at the same addresses and IRQ. The stock models do not implement
LATCH or DETECTMODE, so edge waits never complete with them. See
[ADR 0014](adr/0014-renode-gpio-models.md).

## Renode Scenario Map

### Scripted BLE Scenario

The sim's main loop waits for either a button event or a 2-second timer. A
button event runs `ui_logic::on_button`; a timer tick runs one step of a
four-step scenario through the real coordinator reducers, using two scenario
devices: `Keyboard` (address `0xA1`, RSSI −42) and `Mouse` (`0xB2`, −55). The
address type is a `u32` stand-in for the SoftDevice `Address`, which is why the
coordinator is generic over it. The timer restarts after every button event,
so a tick is "2 s without a button press", not a fixed period.

| Step (tick mod 4) | UART header | Reducer calls | UI event logged | `active_count` |
| --- | --- | --- | --- | --- |
| 0 | `scenario: connect device 0 (Keyboard)` | `plan_connect`, then `on_slot_connected` for slot 0 | `action: UI Connected 'Keyboard'` | 1 |
| 1 | `scenario: connect device 1 (Mouse)` | `plan_connect`, then `on_slot_connected` for slot 1 | `action: UI Connected '2 devices'` | 2 |
| 2 | `scenario: slot 0 link lost` | `on_slot_disconnected` for slot 0 | `action: UI Connected 'Mouse'` | 1 |
| 3 | `scenario: disconnect all` | `plan_disconnect`, then `on_slot_disconnected` per slot | `action: UI Disconnected` | 0 |

Connect steps also log `action: ConnectSlot …` and `action: PersistDevice …`;
every step ends with `scenario: active_count=N`. The cycle then repeats.

Step 2 is labelled "link lost" but calls `on_slot_disconnected`, which frees the
slot. The link-loss path that keeps a slot reserved for reconnection
(`on_slot_link_lost`) is covered only by host tests.

Button commands are handled as follows:

| UI command | Simulation behavior |
| --- | --- |
| `StartScan` | Logged, then the scan completes at once through `ui_logic::on_scan_complete` with the two scenario devices |
| `ListPaired` | Logged, then the screen moves to `SavedDevices` |
| `Connect`, `Disconnect`, `Forget`, `FactoryReset`, `Dismiss` | Logged only; the coordinator is not called |

The sim calls `on_button` directly rather than through the firmware's
`UiState`, so connection-status merging, management request IDs, and retained
notices are covered by host tests, not by Renode.

### Robot Test Case

[bt2usb-sim.robot](../renode/bt2usb-sim.robot) has one test case,
`Sim Boots And Runs Coordinator And UI Logic`. It loads the GPIO models, creates
the machine, swaps the GPIO peripherals, loads the ELF, attaches a terminal
tester to `sysbus.uart0` with `timeout=20`, and starts emulation.
`Wait For Line On Uart` consumes output in order, so each expected line must
follow the previous one. The `Press Button` keyword drives the pin low, waits
for the expected line with emulation paused, then releases the pin, so presses
land at deterministic points and always outlast the debounce interval.

| Order | Stimulus | Expected UART text | What it proves |
| --- | --- | --- | --- |
| 1 | Boot | `bt2usb-sim starting` | Reset, memory map, and Embassy executor reach `main` |
| 2 | — | `buttons ready` | Three button tasks spawned on P0.11, P0.12, P0.24 |
| 3 | — | `entering sim UI loop` | The UI loop is running |
| 4 | Timer | `action: UI Connected 'Keyboard'` (pauses emulation) | RTC time driver and coordinator step 0 |
| 5 | SELECT (pin 24) | `button Select -> screen Scanning (selected 0)` | GPIO edge reaches the button task and `on_button` |
| 6 | — | `cmd: StartScan`, then `scan: 2 devices -> screen DeviceList` | Scan command and scan-complete reducer |
| 7 | DOWN (pin 12) | `button Down -> screen DeviceList (selected 1)`, then `redraw: DeviceList` | List navigation |
| 8 | UP (pin 11) | `button Up -> screen DeviceList (selected 0)`, then `redraw: DeviceList` | List navigation in the other direction |
| 9 | SELECT (pin 24) | `button Select -> screen Connecting (selected 0)`, then `cmd: Connect(0)` | Connect command for the highlighted entry |
| 10 | Timer | `action: UI Connected '2 devices'`, then `scenario: active_count=2` | Coordinator step 1, two active links |
| 11 | Timer | `action: UI Disconnected`, then `scenario: active_count=0` | Coordinator step 3, all links dropped |

The test does not assert step 2's output, any management screen, or anything
about the OLED. Override inputs with `--variable ELF:/abs/path` or
`--variable PLATFORM:@/abs/nrf52840.repl`; the interactive script takes
`renode -e "$bin=@/abs/path" renode/bt2usb-sim.resc`.

## Embedded And Linker Checks

The ARM builds catch failures the host crate cannot:

| Check | Source | Failure it catches |
| --- | --- | --- |
| Clippy with `-D warnings` for `embedded` and `sim` | [ci.yml](../.github/workflows/ci.yml), `mask ci` | Lints in task, driver, and entry-point code that the host crate never compiles |
| Release build of `bt2usb` and `bt2usb-selftest` | `cargo build --features embedded --release` builds both binaries | Type, linker, and size failures in the release profile (`opt-level = "s"`, fat LTO) |
| `__sdata == ORIGIN(RAM)` and stack-at-top assertion | [memory_sd.x](../memory_sd.x) | A linker such as flip-link moving `.data`, which would make the SoftDevice claim the stack |
| FLASH ends at `0xF0000` | [memory_sd.x](../memory_sd.x) | Code or read-only data placed on the pairing pages (240–243) that storage erases |
| FLASH ends at `STORAGE_FLASH_START`; storage ends within flash | [memory_sd.x](../memory_sd.x), symbols from [build.rs](../build.rs) | The page constants in `config.rs` and the `FLASH` length disagreeing, or storage past `0x00100000` |
| Feature guard | [build.rs](../build.rs) | `embedded` and `sim` enabled together, which would link firmware with the wrong memory map |

## Release Helper Tests

[release_test.py](../scripts/release_test.py) tests
[release.py](../scripts/release.py), the helper CI uses to validate tags, stage
the embedded build, and package a release. The tests build a fixture repository
and fake firmware in a temporary directory; they use no network, publish
nothing, and build no firmware. `release.py` needs Python 3.11 or newer for
`tomllib`.

```sh
python -m unittest discover -s scripts -p "release_test.py" -v
```

| Test | What it asserts |
| --- | --- |
| `test_exact_stable_prerelease_and_build_metadata_tags` | `v` plus the exact Cargo version validates, and prerelease detection is correct with and without build metadata |
| `test_prerelease_package_keeps_exact_version_identity` | A prerelease with build metadata packages under its full version name |
| `test_wrong_tag_or_nonsemantic_version_fails` | Missing `v`, mismatched versions, trailing newlines, path segments, and non-semantic versions are rejected |
| `test_package_preserves_exact_built_payload_and_checksums` | Packaged ELF/HEX files are byte-identical to staged inputs, `SHA256SUMS` matches, and `BUILD-INFO.json` is unchanged |
| `test_artifact_tampering_is_rejected` | A modified staged ELF fails with a checksum mismatch and leaves no output |
| `test_metadata_identity_and_build_policy_must_match` | Wrong commit, ref, repository, run ID, version, target, profile, features, `DEFMT_LOG` level, schema, or compiler fails packaging |
| `test_changed_lockfile_is_rejected` | A `Cargo.lock` differing from the recorded build input fails |
| `test_rehashed_artifact_must_still_match_build_metadata` | Regenerating checksums for a changed HEX still fails against the build metadata digest |
| `test_missing_extra_duplicate_and_unsafe_checksum_entries_fail` | Missing, duplicate, path-escaping, and unexpected entries or files fail |
| `test_existing_destination_is_not_overwritten` | Packaging refuses an existing output directory and keeps its contents |
| `test_staging_records_exact_compiled_bytes_and_build_identity` | Staging records the compiled bytes' digests and build identity, and its output packages successfully |
| `test_staging_refuses_dirty_source_or_wrong_checkout` | Modified tracked files, or a checkout whose `HEAD` differs from `GITHUB_SHA`, stop staging |

These tests cannot issue GitHub OIDC credentials or exercise the hosted
attestation and release APIs; see
[deployment validation limits](deployment.md#validation-limits).

## Continuous Integration

[ci.yml](../.github/workflows/ci.yml) runs on pushes to `main` or `master`,
pushes of tags matching `v*`, pull requests, manual dispatch, and a weekly
schedule (cron `23 7 * * 1`, Mondays 07:23 UTC). A newer run for the same ref
cancels an in-progress one, except for tag refs. The default token permission is
`contents: read`, and builds use `DEFMT_LOG=info`.

| Job | Runner and limit | Checks, in order |
| --- | --- | --- |
| Host tests (`ubuntu-latest`, `windows-latest`) | Both, 20 min, `fail-fast: false` | `cargo fmt --package bt2usb -- --check`; release-helper tests; on Linux, install actionlint 1.7.12 (SHA-256 verified) and run it; on `v*` tags, `release.py validate-tag`; `cargo test --locked --lib --tests`; host Clippy with `-D warnings`; `cargo doc --locked --no-deps --lib` with `RUSTDOCFLAGS=-D warnings` |
| Dependency security audit | Ubuntu, 10 min | `cargo audit` with cargo-audit 0.22.2 |
| Embedded build & clippy | Ubuntu, 25 min | Embedded Clippy with `-D warnings`; release build (firmware and self-test); `release.py stage` with `llvm-objcopy` into the runner's temporary directory; upload |
| Renode simulation test | Ubuntu, 20 min | Simulation Clippy with `-D warnings`; simulation build; `scripts/install-renode.sh`; `renode-test --results-dir` on the Robot file; upload results even on failure |
| Verify and attest release package | Ubuntu, 10 min, `v*` tag pushes only, after all four jobs | `validate-tag`; download this run's embedded artifact by ID with digest checking; `release.py package` against the expected commit, repository, and run ID; GitHub provenance attestation; add `provenance.sigstore.json`; upload |
| Prepare draft firmware release | Ubuntu, 10 min, after packaging | Download the attested package; refuse if the tag's release is already published; create or update a draft release |

Artifacts:

| Artifact | Producer | Contents |
| --- | --- | --- |
| `bt2usb-checked-firmware-<attempt>` | Embedded job | Staged ELF/HEX, self-test ELF, build inputs, `BUILD-INFO.json`, `SHA256SUMS` |
| `renode-results-<attempt>` | Simulation job | Robot/Renode results directory (ignored if empty) |
| `bt2usb-attested-release-<attempt>` | Packaging job | Versioned release files and the provenance bundle |

CI installs the toolchain pinned in `rust-toolchain.toml`, so a local run with
the same toolchain reproduces its results. `mask ci` runs the local subset.

### Hosted CI Runs

Runs of [ci.yml](../.github/workflows/ci.yml) on `main`, read with
`gh run list` and `gh run view` on 2026-10-09. The five check jobs are the two
host-test jobs, the dependency audit, the embedded build, and the Renode test.
Dependabot pull-request runs are not listed.

| Run ID | Trigger | Commit | Date (UTC) | Result | Jobs |
| --- | --- | --- | --- | --- | --- |
| 36441384244 | Push | `2479c79` | 2026-09-28 | Failed | Host tests (ubuntu-latest) failed; the other four check jobs passed |
| 36441995385 | Push | `8a04b25` | 2026-09-28 | Passed | All five check jobs passed |
| 37338711407 | Weekly schedule | `8a04b25` | 2026-10-05 | Passed | All five check jobs passed |
| 37932436721 | Push | `7fc99d6` | 2026-10-09 | Passed | All five check jobs passed |
| 37967375873 | Push | `802bbf1` | 2026-10-09 | Passed | All five check jobs passed |

The `2479c79` failure was the earlier actionlint installation step, which
`8a04b25` replaced (see its commit message). In every run, "Verify and attest
release package" and "Prepare draft firmware release" were skipped, because
they run only on `v*` tag pushes. No `v*` tag or GitHub release exists, so the
tag, attestation, and release path has never run on hosted runners;
[Hosted provenance and release recovery acceptance](../TODO.md#release-provenance-and-supply-chain)
tracks it.

### Local And CI Coverage Compared

| Check | `mask ci` | CI |
| --- | --- | --- |
| Format check | Yes (`cargo fmt -- --check`) | Yes (`--package bt2usb`) |
| Host, embedded, and simulation Clippy | Yes | Yes |
| Host unit and integration tests | Yes | Yes, Linux and Windows |
| Release firmware and simulation builds | Yes | Yes |
| Release-helper tests | No | Yes, Linux and Windows |
| actionlint | No | Yes, Linux |
| rustdoc with warnings denied | No | Yes |
| Dependency audit | No | Yes |
| Headless Renode test | No (`mask sim-test`) | Yes |
| Staging, packaging, attestation | No | Tag pushes only |

The [deployment guide](deployment.md#artifact-flow) explains how the release
jobs reuse the embedded job's bytes.

## Hardware Acceptance Evidence

Complete [first-flash.md](first-flash.md) for a new board and after changes to
the BLE/USB/storage boundary. Save a separate result record so the template
remains reusable: file it with the GitHub "Hardware acceptance result" issue
template, [hardware-result.md](../.github/ISSUE_TEMPLATE/hardware-result.md),
labelled `hardware-evidence`. The build, setup, and measurement fields, the
per-section result tables, and the sanitization checks it asks for are listed
in [Recording The Result](first-flash.md#recording-the-result).

An unchecked or skipped item is unverified, not a pass. For deployment, also run
the stress, corruption, and security cases in [TODO.md](../TODO.md). Release
review uses the [release gates](deployment.md#release-gates). A hardware item
in TODO.md stays unchecked until a record like this covers it, and the record
names the commit and artifact hash it covers
([Updating This Checklist](../TODO.md#updating-this-checklist)).

## Test Design Rules

### Start At The Narrowest Layer

Put a decision in a pure module and test it on the host first: a reducer for
UI or coordinator state, a parser for wire data, a policy function for power,
wake, or retry. Test async ordering with the delivery worker style of fake
queue, sink, and clock. Use Renode for boot, the executor, and GPIO paths, the
self-test for peripheral bring-up, and the first-flash checklist for anything
involving a real radio, USB host, or flash.

### Feed Malformed And Truncated Input

Every parser of peer- or flash-supplied bytes must fail closed: no panic, no
partial result, no fallback to another report kind. Follow the existing
patterns: every truncated prefix of each USB descriptor
(`parses_actual_usb_descriptors_without_cross_classifying_pan`),
`reader_stops_on_truncated_record`,
`malformed_termination_never_exposes_partial_value`, and
`malformed_known_id_never_falls_back_to_another_kind`.

### Prove Held Input Is Released

Any change to the input path needs a test showing the final release still
reaches the host after coalescing, overflow, endpoint failure, or disconnect,
as in `keyboard_latest_state_wins_but_release_survives`,
`queue_overflow_preserves_final_release`,
`timed_out_press_retries_latest_release_after_backoff`, and
`same_key_held_by_two_sources_survives_release_and_disconnect`. Relative
motion must never be replayed.

### Test Buffers At Their Bounds

Firmware buffers are fixed-capacity `heapless` types. Test at and beyond
capacity: `writer_truncates_cleanly_when_full`,
`maximum_value_completes_and_oversized_map_fails`,
`targets_are_capped_to_connection_slots`, `parser_nesting_is_bounded`, and
`mouse_accumulation_saturates`. Arithmetic on untrusted sizes and time must
saturate (`untrusted_report_dimensions_cannot_overflow_routing_parser`,
`large_timeout_does_not_overflow_into_low_power`).

### Keep Hardware Types Out Of Pure Modules

A module listed in `lib.rs` must compile for the host without SoftDevice,
Embassy, or peripheral types. Make the core generic instead, as the coordinator
is over its address type, and gate `defmt::Format` derives behind
`cfg_attr(feature = "defmt", …)`. The [architecture guide](architecture.md) and
[ADR 0003](adr/0003-pure-core-and-task-shell.md) describe the split.

### Make Time Deterministic

Pass time in rather than reading a clock: `next_power_state` takes elapsed
seconds and `Recovery::failed` takes `now_ms`. Async tests poll futures by hand
with `Waker::noop()` and advance a fake `RetryClock`, so no test sleeps or
depends on scheduler timing. In Robot, pause emulation at the line you expect
before driving the next input.

### Assert Host-Visible Contracts

Assert what the USB host or the user would observe: report bytes, held state,
wake decisions, the next screen, and the actions a reducer returns. Avoid
asserting only that an internal function ran.

### Keep Fixtures Small And Real

Use the firmware's own descriptors and minimal byte arrays. Name tests by the
behavior they guarantee, as the existing names do. Do not put bond keys or real
captured keystrokes in fixtures or logs.

## Known Verification Gaps

Not yet first-class:

- fuzzing and property tests for descriptors, advertisements, reports, and storage
- deterministic async fault tests (cancellation, full channels, contention)
  beyond the endpoint delivery worker tests
- power-loss and flash fault injection
- USB conformance captures and a peripheral/host/hub compatibility matrix
- soak, latency, and reconnect-time measurements
- automated documentation link and constant checks
- reproducible-build comparison across clean environments

Specific to the current workflow and test tree:

- No CI job flashes a board; the self-test and first-flash layers are manual.
- CI does not measure coverage, and no coverage threshold exists.
- The Renode job runs one scripted scenario on Linux; it does not exercise the
  OLED task, management screens, or the link-loss reservation path.
- actionlint runs only in the Linux host job.
- rustdoc is checked only for the host library (`--no-deps --lib`), not for the
  firmware build.
- `cargo audit` runs without a deny option, so unmaintained-crate warnings do
  not fail the job; see the [validation record](#validation-record--2026-09-28).
- Connection workers, the GATT HID client, the storage shell and codec, the USB
  device, and the display driver have no host tests
  ([details](#modules-without-host-tests)).
- `DeviceStore` in `storage.rs` and the address and bond-record encoding in
  `storage/codec.rs` have no host tests, so the legacy-format parser, the
  identity merge in `DeviceStore::add`, and bond-record round trips are checked
  only by the embedded build and on hardware; tracked as
  [Host tests for the device store](../TODO.md#verification-and-code-quality).
- The packaging and draft-release jobs run only on `v*` tag pushes. No `v*` tag
  exists, so they have never run; the check jobs have passed on hosted
  runners ([Hosted CI Runs](#hosted-ci-runs)).

Track these in [TODO.md](../TODO.md), not as existing coverage.

## Troubleshooting

### `renode-test` Or `renode` Not Found

`mask sim-test` prints `renode-test not found on PATH` and exits with status
127; `mask sim` prints `Renode not found on PATH`. Run `mask sim-setup`, then
add `~/.local/bin` to PATH (the installer prints the exact line when it is
missing) and open a new shell. CI does the same with
`export PATH="$HOME/.local/bin:$PATH"`. A different Renode version is set with
`RENODE_VERSION`; record it with results.

### Robot Framework Dependencies

`renode-test` is a Robot Framework harness. The installer bootstraps a user
`pip` when the system Python has none, then installs `psutil>=5.9.8` (binary
wheel only), `robotframework==6.1`, `pyyaml==6.0.*`,
`robotframework-retryfailed==0.2.0`, and `telnetlib3==2.0.*` with
`--user --break-system-packages`. If a run fails with a Python import error,
re-run `mask sim-setup` with the same `python3` that `renode-test` uses. Robot
may write `log.html`, `report.html`, and `output.xml` to the current directory;
these are ignored by Git, and `--results-dir` (as in CI) keeps them elsewhere.

### Renode Waits Time Out

Read the last matched line, then check the next expected one. A missing
`button …` line usually means the custom GPIO models were not loaded: the
`.cs` file must be included before the platform is created, and the stock
`gpio0`, `gpio1`, and `gpiote` unregistered before the overlay REPL loads. A
stale ELF also causes mismatches: `mask sim-test` rebuilds first, but a direct
`renode-test` uses whatever is at
`target/thumbv7em-none-eabihf/debug/bt2usb-sim`.

### Simulation Or Firmware Build Feature Errors

- ``features `embedded` and `sim` are mutually exclusive; build each
  separately`` comes from `build.rs`. Do not use `--all-features`; build each
  feature in its own command.
- Firmware, self-test, and simulation builds need
  `--target thumbv7em-none-eabihf`. There is no default target, so omitting it
  targets the host, which these `no_std`, `no_main` ARM binaries do not support.
- Do not add a `memory.x` at the crate root. `build.rs` writes `memory_sd.x` or
  `memory_sim.x` to `OUT_DIR/memory.x`, and the linker resolves a root
  `memory.x` first, so it would silently replace the selected layout. The
  `build.rs` comment records that this once linked the simulation at the
  SoftDevice offset.
- Do not `cargo run` the simulation binary. The runner configured for the ARM
  target in `.cargo/config.toml` is `probe-rs run`, so with a board attached it
  would try to flash an image linked from address 0, where a real board keeps
  the SoftDevice. (Inferred from the configuration; not tried.)

### Host Tests Fail To Build

Run `cargo test --locked --lib --tests` from the repository root with no
`--features` or `--target`. The ARM target has no `std` or test harness, which
is why [.cargo/config.toml](../.cargo/config.toml) sets no global build target;
do not add one. `--locked` fails if `Cargo.lock` would change; update
dependencies in a separate, reviewed change. If release-helper tests fail with
`No module named 'tomllib'`, the Python interpreter is older than 3.11.

### Windows And WSL Paths

- Mask recipes are Bash. Run them in WSL or Git Bash, or run the direct commands
  in PowerShell.
- When a tool is not on PATH, [run-tool.sh](../scripts/run-tool.sh) tries
  `$CARGO_HOME/bin`, `~/.cargo/bin`, and then the current Windows user's
  `.cargo/bin`: from `USERPROFILE` (only where `cygpath` exists, as in Git
  Bash), from `USERNAME` under `/c/Users` or `/mnt/c/Users`, and finally, in
  WSL, by asking `cmd.exe` for `%USERPROFILE%`. If every location fails it
  prints
  `Error: '<tool>' was not found in PATH or the current user's Rust install locations.`
- `scripts/install-renode.sh` refuses to run outside Linux with
  `ERROR: this installer targets Linux/WSL2.` and suggests
  `wsl -d Ubuntu -- ./scripts/install-renode.sh`.
- A shell script or mask recipe failing with `bash\r` or `$'\r'` errors has CRLF
  line endings. [.gitattributes](../.gitattributes) forces LF for scripts, Renode
  files, `maskfile.md`, and sources; re-checkout the affected files if the
  working tree was created without it.
- A mask recipe failing with `./scripts/run-tool.sh: Permission denied` has
  lost the executable bit. Both scripts in `scripts/` are committed as mode
  `100755` (`git ls-files -s scripts`); a copy that drops the mode, such as an
  archive extracted on Windows, needs `chmod +x scripts/*.sh`.

### Coverage Tool Missing

`mask coverage` prints `No coverage tool found.` when neither `cargo-llvm-cov`
nor `cargo-tarpaulin` is installed. Run `mask coverage-install`, which installs
`cargo-llvm-cov` and the `llvm-tools-preview` component.

## Validation Record — 2026-10-09

This record covers the commit that adds the shared reconnect scan
([ADR 0015](adr/0015-shared-reconnect-scan.md)), host-LED forwarding on every
new keyboard link, bounded peripheral connection parameters
([ADR 0016](adr/0016-bounded-peer-connection-parameters.md)), and the reserved
keyboard byte handling. The checks ran locally on Linux in a container, on the
working tree just before that commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 260 unit tests and 3 integration tests |
| Clippy with warnings denied | Host tests, embedded, simulation | Passed |
| Formatting and host API documentation | `cargo fmt --package bt2usb -- --check`; rustdoc with warnings denied | Passed |
| Release bridge and self-test builds | Rust 1.95.0, ARM target | Passed; `.text` 111,616 bytes, `.bss` 23,476 bytes, read from the release ELF's section headers |
| Simulation build | Rust 1.95.0, ARM target | Passed |
| Release helper policy/integrity tests | Python 3.13, Linux | Passed: 12 tests |
| Local Markdown links and anchors | Script over every tracked `.md` file | Passed |
| Headless Renode scenario | — | Not run: Renode is not installed in this environment; the scenario does not include the SoftDevice, so it would not exercise these changes |
| Dependency audit, actionlint | — | Not run: neither tool is installed in this environment; `Cargo.lock` and the workflows did not change |
| Hosted CI | GitHub Actions | Runs on the push; not recorded here |
| Board/radio/USB acceptance | Physical hardware | Not performed; the changes need the board checks in the [first-flash checklist](first-flash.md) |

## Validation Record — 2026-09-28

This record covers the current uncommitted hardening changes, not a published
release or a completed hardware qualification. Associate the final commit and
artifact hash with this record when preparing a release.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Windows | Passed: 219 unit tests and 3 integration tests |
| Headless Renode scenario | Renode 1.16.1 through WSL | Passed: 1 scenario |
| Dependency audit | Current locked dependency graph | No vulnerability errors; 2 unmaintained dependency warnings below |
| Release helper policy/integrity tests | Windows Python 3.14 and WSL Python 3.12 | Passed: 12 tests on each platform |
| Workflow syntax and expressions | actionlint 1.7.12 (checksum-verified) with ShellCheck 0.11.0 | Passed locally; the published-release guard's `gh --jq` filter was not executed locally |
| Clippy with warnings denied | Host tests, embedded, simulation | Passed |
| Release bridge and self-test builds | Rust 1.95.0, ARM target | Passed |
| Simulation build | Rust 1.95.0, ARM target | Passed |
| Formatting and host API documentation | Rustfmt; rustdoc with warnings denied | Passed |
| Invalid combined feature selection | Cargo check with `--keep-going` | Rejected by the explicit firmware/simulation build guard |
| Tooling and documentation | Workflow YAML, action pins, shell scripts, 31 mask recipes, local links | Validated; every action SHA resolves to its commented upstream tag (`git ls-remote`); WSL-to-Windows Cargo fallback smoke check passed |
| GitHub Actions workflow | Remote CI | Not run in this session; later hosted runs passed (see below) |
| Board/radio/USB acceptance | Physical hardware | Not performed in this validation session |

The dependency audit reports `bare-metal 0.2.5` (`RUSTSEC-2026-0110`) and
`proc-macro-error 1.0.4` (`RUSTSEC-2024-0370`) as unmaintained transitive
dependencies. These warnings remain open maintenance work in
[TODO.md](../TODO.md); the audit result is not a warning-free dependency bill.

Later history: the record was written before the hardening changes were
committed as `2479c79` and last edited in `8a04b25` (both 2026-09-28). Since
`2479c79` and until the 2026-10-09 fixes, Rust sources, tests, Renode files,
and scripts changed only in comments that name renamed documents, so the
counts above described the tree until then; the checks were not re-run locally
for those changes. The fixes added tests, which the
[2026-10-09 record](#validation-record--2026-10-09) and the
[Test Map](#test-map) count. The 219 unit tests did not include the ten
`scanner.rs` tests, which were never compiled
([Tests That Do Not Run](#tests-that-do-not-run)). The hosted run on `2479c79` failed at the earlier actionlint installation step,
which `8a04b25` replaced. Hosted runs later passed all five check jobs on
`8a04b25` (run 36441995385, 2026-09-28) and on later commits; see
[Hosted CI Runs](#hosted-ci-runs). The tag-only packaging and release jobs
have still never run.

## Related Guides

- [Development](development.md)
- [Code quality](code-quality.md)
- [First flash](first-flash.md)
- [Architecture and ADRs](architecture.md)
- [ADR 0004: layered verification](adr/0004-layered-verification.md)
- [ADR 0013: pinned toolchain and mask tasks](adr/0013-pinned-toolchain-and-mask-tasks.md)
- [ADR 0014: Renode GPIO models](adr/0014-renode-gpio-models.md)
- [Deployment](deployment.md)
- [Operations](operations.md)
- [Task reference](../maskfile.md)
