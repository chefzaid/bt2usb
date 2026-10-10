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
| Renode glyph table check, or rewrite after a font change | — | `cargo test --locked --test oled_font`, or with `UPDATE_OLED_FONT=1` |
| Coverage summary | `mask coverage` | `cargo llvm-cov --locked --lib --tests` |
| Coverage HTML / JSON | `mask coverage --html` / `--json` | `cargo llvm-cov --locked --lib --tests --html --output-dir coverage-html` |
| Coverage against the CI floor | — | `cargo llvm-cov --locked --lib --tests --summary-only --fail-under-lines 97` (the floor is `COVERAGE_MIN_LINES` in [ci.yml](../.github/workflows/ci.yml)) |
| Host lint | — | `cargo clippy --locked --lib --tests -- -D warnings` |
| Embedded lint | `mask clippy` | `cargo clippy --locked --features embedded --target thumbv7em-none-eabihf -- -D warnings`, then again with `--features embedded,log-sensitive-data` |
| Simulation lint | — | `cargo clippy --locked --features sim --target thumbv7em-none-eabihf -- -D warnings` |
| Firmware and self-test build | `mask build --release` | `cargo build --locked --features embedded --target thumbv7em-none-eabihf --release` |
| Simulation build | `mask sim-build` | `cargo build --locked --features sim --target thumbv7em-none-eabihf` |
| Headless Renode test | `mask sim-test` | `renode-test renode/bt2usb-sim.robot` |
| Interactive Renode | `mask sim` | `renode renode/bt2usb-sim.resc` |
| Release helper tests | — | `python -m unittest discover -s scripts -p "release_test.py" -v` |
| Markdown links, constants, memory map, and commands | `mask docs-check` | `python scripts/check_docs.py`, and its tests with `python -m unittest discover -s scripts -p "check_docs_test.py" -v` |
| Format check | `mask fmt-check` | `cargo fmt --package bt2usb -- --check` |
| API documentation, every build, warnings denied | `mask rustdoc-check` | `cargo doc --locked --no-deps --document-private-items` with `RUSTDOCFLAGS=-D warnings`, once per build ([commands](code-quality.md#documentation-comments)) |
| Workflow lint | — | `actionlint` |
| Python and shell lint | `mask lint-scripts` | `python scripts/lint_scripts.py` (Ruff and ShellCheck), and its tests with `python -m unittest discover -s scripts -p "lint_scripts_test.py" -v` |
| Dependency audit | — | `cargo audit` |
| Board self-test | `mask selftest` | `cargo run --locked --features embedded --target thumbv7em-none-eabihf --release --bin bt2usb-selftest` |
| Local software gate | `mask ci` | See [what `mask ci` covers](#local-and-ci-coverage-compared) |

`mask sim-setup` installs Renode and the `renode-test` dependencies, and
`mask coverage-install` installs `cargo-llvm-cov`. `actionlint`, Ruff,
ShellCheck, and `cargo audit` are not installed by any mask recipe; CI pins
actionlint 1.7.12, Ruff 0.16.9, ShellCheck 0.11.0, and cargo-audit 0.22.2. [ADR 0013](adr/0013-pinned-toolchain-and-mask-tasks.md)
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
| `src/hid/` (all submodules), `src/ble/adv_parser.rs`, `conn_params.rs`, `coordinator.rs` (with `coordinator_tests.rs`), `reconnect.rs` (with `reconnect_tests.rs`), `long_read.rs`, `management.rs`, `src/power_logic.rs`, `src/ui/display_logic.rs`, `input_logic.rs`, `ui_logic.rs` (with `ui_logic_tests.rs`), `src/config.rs`, and, under `cfg(test)` only, `src/storage/codec.rs`, `devices.rs` (with `devices_tests.rs` and `devices_format_tests.rs`), `framing.rs`, and `record.rs` | `src/ble/mod.rs`, `multi_conn.rs`, `slot_worker.rs`, `bonder.rs`, `hid_client.rs`, `scanner.rs`; `src/storage.rs`; `src/usb/`; `src/ui/mod.rs`, `display.rs`, `buttons.rs`; `src/power.rs`, `src/stack.rs`, `src/sd_setup.rs`; the `main.rs`, `selftest.rs`, and `sim.rs` entry points |

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
Git.

CI runs llvm-cov in the Host coverage job, uploads the summary, an lcov file,
and the HTML report as the `coverage-report-<attempt>` artifact, and then fails
the job when line coverage is below the floor, 97% since 2026-10-10. The
baseline it was set from is 97.59% of lines, recorded in the
[validation record](#validation-record--2026-10-10). The
[coverage policy](code-quality.md#coverage-in-ci) says how the floor changes.

## Test Map

Counts below were taken with `grep -c '#\[test\]' <file>` on each file on
2026-10-10, in the commit that moved the screen layout into `ui::layout` and
ran the OLED task in Renode. The tree holds 359 `#[test]` functions: 353 in
files compiled into the host library, 3 in `tests/integration.rs`, and 3 in
`tests/oled_font.rs`, and every one of them runs under
`cargo test --locked --lib --tests` (see
[Tests That Do Not Run](#tests-that-do-not-run)). The
[2026-10-09 validation record](#validation-record--2026-10-09) ran 260 unit
tests, before four advertisement tests moved into the host library and
fourteen UI tests (the management deadline, the saved-device list, scans, and
`UiState` link updates), four keyboard-report tests, one connection-parameter test, eight reconnect wake and identity tests, six crowded-scan tests, 27
device-store tests, 17 controller, message, and management-target tests, and
12 screen-layout tests were added; the 353 passed with `cargo test` on
2026-10-10, as did the 6 integration tests. There are no `#[ignore]` or
`#[should_panic]` tests.

### HID Reports, Descriptors, And Delivery

| Location | Tests | Behavior covered |
| --- | --- | --- |
| [lib_tests.rs](../src/lib_tests.rs) | 36 | Keyboard, mouse, and consumer report parsing from BLE bytes and serialization to USB: empty, short, exact, and longer inputs; too-small output buffers; all modifiers and buttons; six-key arrays; negative motion and wheel; 5-byte mouse reports with horizontal pan and back/forward buttons; consumer volume, media, browser, and launcher usages. |
| [hid_classify_tests.rs](../src/hid_classify_tests.rs) | 17 | `classify_report` and `classify_notification` routing by report ID or length, rejecting a keyboard report with a nonzero reserved byte when its kind is only inferred, invalid 2-byte consumer payloads, unknown lengths, and empty or single-byte input. |
| [hid_descriptor_tests.rs](../src/hid_descriptor_tests.rs) | 34 | Report-descriptor parsing in `hid/report_protocol.rs`: usage pages, keyboard/mouse/consumer detection, Push/Pop, long items, bounded nesting, overflow-safe report dimensions, constant padding, unsupported applications, extended usages, and every truncated prefix of the firmware's own USB descriptors failing closed. Descriptor-guided routing (`classify_notification_with_hint`, `classify_known`) that rejects unknown or mixed-kind report IDs instead of falling back to another kind, and drops a report of a kind a mixed map without report IDs does not declare. GATT Report Reference parsing, consumer usage range, three-button boot mouse serialization, and GATT values whose first byte resembles a report ID. |
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
| [ble/coordinator_tests.rs](../src/ble/coordinator_tests.rs) | 32 | `ConnManager` slot state machine (reserve, connect, disconnect, ignored out-of-range slots, second slot when the first is busy, summary text) and the reducers: `plan_start_scan`, `plan_connect` (out of range, success, already connected acknowledges without a duplicate connect, already connecting waits, no free slot), `plan_disconnect`, `on_slot_connected` (persist and summary), `on_slot_disconnected`, `on_slot_error`, `on_slot_link_lost` keeping the slot reserved, reconnection, and disconnect during retry. `merge_advertisement` lets a name-only scan response update a known HID peer even when the list is full, and never enrolls a device without the HID UUID. In a crowded scan it keeps the strongest HID advertisers: a keyboard heard at -40 dBm after twenty advertisers at -70 to -89 dBm filled the eight-entry list is listed and stays listed while they keep advertising; only a strictly stronger newcomer replaces the weakest entry, judged by each entry's latest RSSI; an unavailable RSSI (127) ranks below every measurement; a replaced device cannot return through a name-only response; and a zero-capacity list stays empty. |
| [ble/adv_parser.rs](../src/ble/adv_parser.rs) | 7 | Advertised names keep valid UTF-8 and truncate at a character boundary; a complete name beats a shortened one, and a shortened one is used when it is the only name; a missing, empty, or invalid name does not replace a known one. The HID UUID is found among other 16-bit UUIDs and in an incomplete UUID list, and an empty advertisement has neither the UUID nor a name. |
| [ble/long_read.rs](../src/ble/long_read.rs) | 4 | Bounded ATT Read/Read Blob assembly: no value until a short final fragment, an exact-MTU value needs an end response, a 512-byte value completes while an oversized one fails, and malformed termination never exposes a partial value. |
| [ble/management.rs](../src/ble/management.rs) | 6 | `commit` publishes only persisted state: a failed write keeps the store and bonds, and cancelled persistence never publishes the candidate. `forget_targets` picks the connected or reconnecting slots of the forgotten peer only. The `Quiescence` barrier suppresses reconnect events until the matching token is acknowledged, waits for both sources on reset, and ignores invalid slots. |
| [ble/messages.rs](../src/ble/messages.rs) | 3 | Only management commands carry a request ID, only `Connected` and `Disconnected` report link state, and each coordinator `UiEvent` converts to the matching UI event. |
| [ble/reconnect_tests.rs](../src/ble/reconnect_tests.rs), for [reconnect.rs](../src/ble/reconnect.rs) | 31 | The shared background-reconnect table ([ADR 0015](adr/0015-shared-reconnect-scan.md)): a sighting goes to the slot that owns the device, unregistered devices and slots are ignored, a sighting is used once, replaced by a newer one, fresh at 2 s and discarded after, and dropped with its slot or when the slot changes target; re-registering the same target keeps the outage start, a different one restarts the fast window; the duty cycle is fast while any target is inside its window; after a failed attempt the other slot's scans ignore that device while its own still see it, the holdoff ends on time, survives re-registration, is extended by a new failure, ends for a new target, drops the pending sighting, and ignores unregistered slots; the lower slot wins a tie; out-of-range slots and a clock going backwards are harmless. A handover wakes only the owner, and taking the sighting (fresh or stale), a failed attempt, clearing, or a new target ends the wake while re-registering keeps it; a sighting for a slot cleared after the targets were copied wakes nobody. `SavedPeer` is the same device by identity key or, without one, by address, matches a resolved or stored address, and a retry at a new private address keeps the holdoff and fast window. |
| [ble/conn_params.rs](../src/ble/conn_params.rs) | 14 | Bounding a peripheral's connection parameter request ([ADR 0016](adr/0016-bounded-peer-connection-parameters.md)): a request inside the limits is granted unchanged, a peripheral asking only for 20–40 ms gets 20 ms, one asking for 50–100 ms gets 30 ms and is flagged as outside its range while one whose fastest interval is 30 ms gets it inside its range, an overlapping range is narrowed to 7.5–15 ms, a 32 s supervision timeout is capped at 4 s and a short one raised to 1 s, latency is capped at 20, reversed bounds read as a range (also by `interval_within_request`), a request entirely below 7.5 ms gets 7.5 ms and is flagged, as is a request below a raised 15 ms floor given that floor, latency is lowered when a 1 s timeout cap cannot cover it, the latency limit is the largest the timeout covers for every timeout up to 33 s at twelve intervals, and the timeout is raised to meet the Core rule. A sweep over every boundary of the policy and over out-of-range values (interval 0 and 0xFFFF, latency 500, timeout 0) checks that every answer stays inside the limits and the Core rule, that a peripheral accepting 15 ms is never slowed, and that any request reaching into the grantable range gets an interval it asked for. |

### Pairing Storage

| Location | Tests | Behavior covered |
| --- | --- | --- |
| [storage/framing.rs](../src/storage/framing.rs) | 8 | Versioned blob framing: empty and multi-record round trips in order, non-versioned data yields no records, the writer truncates cleanly when full, the reader stops on truncated or zero-length records, a complete frame needs the exact record count and length, and future or truncated versioned headers are never read as legacy data. |
| [storage/record.rs](../src/storage/record.rs) | 3 | Record metadata validation: name encoding, capacity, and base length; agreement between the bond flag and the record size; UTF-8 name lengths counted in bytes. |
| [storage/devices_format_tests.rs](../src/storage/devices_format_tests.rs) | 15 | The flash format of the device list in `devices.rs` and `codec.rs`. Codec: device records with and without a bond round trip and need their whole buffer, every address kind round trips and kind 5 is rejected, bond fields sit at their offsets, a bond identity must be public or random static, names truncate at a character boundary. Load: a full store of four bonded devices with 32-byte names round trips through the flash item; an empty area is writable; a legacy store loads without bonds and is rewritten versioned on the next change, and a bad legacy count or length is refused; empty, truncated, future-version, over-capacity, and malformed items, like an unreadable area, leave the store empty and refusing saves until a factory reset, which erases first only for an unreadable store; a save reports an item that does not fit its buffer; loading merges records of one bonded peer. |
| [storage/devices_tests.rs](../src/storage/devices_tests.rs) | 12 | The device list in `devices.rs`. Merge: a bonded device is stored under its identity address; RSSI alone is not saved but a name change is; new keys replace the bond of the same identity; a device without keys never clears a bond; keys merge with an entry under the identity or a private address. Capacity and removal: a fifth device evicts the oldest; lookup follows the stored address or a resolvable private address with `IdentityKey::is_match` rules, and address equality ignores the resolved flag; Forget builds a candidate and leaves the list unchanged; bonds list oldest first and devices newest first; Forget and reset publish only after the save succeeds (through `management::commit`). Both files resolve private addresses with a fake in place of the SoftDevice's AES block. |

The [data model](data-model.md#pairing-store) describes the layout these tests
protect.

### UI And Power Policy

| Location | Tests | Behavior covered |
| --- | --- | --- |
| [ui/ui_logic_tests.rs](../src/ui/ui_logic_tests.rs), for [ui_logic.rs](../src/ui/ui_logic.rs) | 30 | `on_button` transitions: SELECT scans from Home and Error, list navigation clamps, SELECT connects the highlighted entry, an empty list cannot connect, stale selections are clamped, SELECT on Connected rescans and DOWN disconnects, ignored combinations are no-ops. `on_scan_complete`. Only the saved-device commands are management requests. Management confirmations default to Cancel, a timeout shows **No reply** without claiming an outcome, drops the saved list, survives link updates, reopens saved devices with UP and is acknowledged with SELECT, and keeps an error already showing, an empty store still offers Factory reset, errors survive later status, and background status or scans do not dismiss a confirmation. Saved-device navigation reaches every entry and backs out, `UiState` counts saved devices on management screens, a completed change shows its notice and drops the saved list (an error stays), a scan lists results, reports none, or ignores a stray completion, a button scan clears old results, a new link or a drop on Home, Connecting, or Connected clears the list, a background connect or drop leaves a running scan or its picker and list on screen, and long messages are cut to 32 bytes. |
| [ui/controller_tests.rs](../src/ui/controller_tests.rs), for [controller.rs](../src/ui/controller.rs) | 16 | Request tracking: one request runs at a time, stale replies are rejected, IDs stay unique across wraparound, an unanswered request expires at its deadline and its late reply is ignored, an answered request never expires. The commands presses send: scan results are listed and Connect sends the highlighted index, devices found outside a scan are not listed, buttons wait for the list reply while a stale reply is ignored, a list reply does not hide an error, Forget names the listed address and reports the stored result, Factory reset returns home without links, a failed change shows the coordinator's error, a Forget of an entry missing from the snapshot sends nothing, and a command that could not be queued reports Busy and frees its request. `tick` animates only a visible scan and abandons an unanswered request at its deadline. Every error tag has a distinct message that fits a 21-character display line. |
| [ui/display_logic.rs](../src/ui/display_logic.rs) | 2 | OLED retry backoff of 1, 2, 4, 8, 16, then 30 s (capped) without blocking new frames, reset on recovery, and saturating deadlines. |
| [ui/input_logic.rs](../src/ui/input_logic.rs) | 3 | The device-list window keeps the selection visible, handles an empty list and a stale selection, and the scan spinner recovers from an out-of-range state. |
| [ui/layout_tests.rs](../src/ui/layout_tests.rs), for [layout.rs](../src/ui/layout.rs) | 12 | Every screen's lines and baselines: Home's title and hints, the scan's dots cycling, the one-line waiting screens, a device list that marks the selection with `> ` and scrolls to keep it among four rows, an empty list, the saved list ending with Factory reset and its footer, the Forget confirmation naming the device (or "Device unavailable") with Cancel as the default, the reset confirmation, and the name or message under each status title. Every fixed label fits the 21 columns of the panel, lines stay on the panel and never overlap, and a name wider than the panel is kept whole for the panel to cut. |
| [power_logic.rs](../src/power_logic.rs) | 5 | Active, Idle, and LowPower decisions: USB suspend forces LowPower at once, idle beyond twice the timeout without a BLE link is LowPower while a link keeps Idle, and very large timeouts do not overflow. |

### Integration Tests

| Location | Tests | Behavior covered |
| --- | --- | --- |
| [tests/integration.rs](../tests/integration.rs) | 3 | Keyboard (report ID 1), mouse (ID 2), and consumer (ID 3) notifications classified and serialized through the crate's public `bt2usb::hid` API, as an external crate sees it. |
| [tests/oled_font.rs](../tests/oled_font.rs) | 3 | The glyph table the Renode panel model reads text with, [oled-font-6x10.txt](../renode/oled-font-6x10.txt), equals `FONT_6X10` as embedded-graphics draws it (rewrite it with `UPDATE_OLED_FONT=1 cargo test --locked --test oled_font`); every printable ASCII glyph is distinct, so text reads back unambiguously; and the font's 6×10 cell, zero spacing, and baseline 7 are what `ui::layout` and the text reader assume, and `display.rs` draws in no other font. It uses `embedded-graphics` as a dev-dependency. |

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
| `ble/multi_conn.rs`, `ble/slot_worker.rs`, `ble/bonder.rs`, `ble/hid_client.rs`, `ble/scanner.rs` | Embedded build and Clippy; pure decisions they call are host-tested; hardware acceptance. The self-test scan stage checks the radio with its own scan loop and `ble/adv_parser.rs`; it does not run these modules |
| `storage.rs` | Embedded build and Clippy; the decisions it calls (the device list, codec, framing, and record validation) are host-tested, but its conversions to and from SoftDevice types, IRK resolution through the SoftDevice, and flash writes with retries are not; the self-test flash stage exercises the same region and `sequential-storage` map, not this code; hardware acceptance |
| `usb/hid_device.rs`, `usb/host_requests.rs` | Embedded build and Clippy; delivery, aggregation, wake policy, and host LED decoding and forwarding are host-tested; self-test USB stages; hardware acceptance |
| `ui/buttons.rs` | Embedded and simulation builds and Clippy; Renode scenario (real GPIO edges through this module); hardware acceptance. The self-test button stages check wiring with their own `Input` code, not this module |
| `sim.rs`, `sim_ble.rs` | Simulation build and Clippy; the Renode scenario, which is their purpose. `sim_ble.rs` mirrors the order in which `ble/multi_conn.rs` calls the pure modules, but nothing checks that the two stay in step |
| `ui/display.rs` | Embedded and simulation builds and Clippy; the screen layout (`ui::layout`) and the recovery policy are host-tested; the Renode scenario runs the display task on modelled TWIM and SSD1306 peripherals and reads every screen it draws back ([OLED checks](#oled-checks)); self-test OLED stages |
| `stack.rs`, `sd_setup.rs` | Embedded build and Clippy; self-test SoftDevice and stack stages |
| `power.rs` | Embedded build and Clippy; its policy (`power_logic.rs`) is host-tested; hardware acceptance (sleep and wake) |

## Renode Simulation

The simulation runs the firmware's UI controller (`ui::controller` over
`ui::ui_logic`), its display task (`ui::display` with the `ui::layout`
screens), its button task, and the pure BLE and storage modules
(`ble::coordinator`, `ble::management`, `ble::messages`, `ble::adv_parser`,
and `storage::{devices, codec, framing, record}`) on an emulated nRF52840. The
radio, the connection workers, and flash are stand-ins that answer at once;
BLE events come from a scripted scenario and from the commands the buttons
send. UART0 carries logs, so no probe or defmt decoder is required, and the
SSD1306 model reads the panel's text back, so the test can check what the
user would see.

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

Read the OLED from the monitor, as text or as the whole panel in `#` and `.`:

```text
sysbus.twi0.oled Text
sysbus.twi0.oled Dump
```

For repeatable headless execution:

```sh
mask sim-test
# Direct command after building:
renode-test renode/bt2usb-sim.robot
```

The Robot test asserts boot, the scripted scenario, and the UI flows the GPIO
presses drive, including a link loss and saved-device management, and reads
the OLED after each screen change it checks. Run it when changing the
simulation, the GPIO path, the display task or a screen layout, the Renode
models, or any pure module the simulation runs. A simulated scan hears three fixed advertisements at once; this is not a
test of scan timing or advertisement interoperability.

### What The Simulation Build Contains

`--features sim` builds [sim.rs](../src/sim.rs) and
[sim_ble.rs](../src/sim_ble.rs) without SoftDevice, USB, or the flash shell,
links them with [memory_sim.x](../memory_sim.x) from address 0
(no SoftDevice reservation), and supplies the single-core `cortex-m` critical
section that the SoftDevice provides in firmware builds.
[build.rs](../build.rs) selects the memory map by feature and refuses to build
`embedded` and `sim` together. The debounce interval is `BUTTON_DEBOUNCE_MS`
(50 ms) from [config.rs](../src/config.rs).

| Part | Firmware code that runs | Stand-in |
| --- | --- | --- |
| Buttons | `ui::buttons::button_task` on P0.11, P0.12, P0.24, through the custom GPIO/GPIOTE models | Edges injected with `gpio0 OnGPIO` |
| UI loop | `UiController::{button, event, tick}`, `UiState`, `on_button`, and `display::publish` after every iteration | `sim.rs` applies each event as `main.rs` does, but without the power manager (the panel is always on) or the command and event channels: commands run to completion before the next button |
| Display | `ui::display::task` on TWIM0 (SDA P0.26, SCL P0.27) from `display::new_twim`: `run`, `render`, `initialize`, `StopSafeI2c`, `finish_or_stop`, the `ssd1306` driver, and the `ui::layout` lines in `FONT_6X10` | The TWIM model in [nrf52840_twim.cs](../renode/nrf52840_twim.cs) and the SSD1306 model in [ssd1306.cs](../renode/ssd1306.cs) at `0x3C`; see [OLED checks](#oled-checks) |
| Coordinator | `plan_start_scan`, `plan_connect`, `plan_disconnect`, `on_slot_connected`, `on_slot_disconnected`, `on_slot_link_lost`, `link_state`, and the `ConnManager` | `SimBle` in `sim_ble.rs` executes their `Action`s in the order `multi_conn::execute_action` does; a connection worker connects or disconnects as soon as it is told |
| Scan | `merge_advertisement` and `adv_parser` | Three fixed advertisements: `Keyboard` (address `0xA1`, RSSI −42, HID UUID), `Phone` (`0xC3`, −30, no HID UUID), `Mouse` (`0xB2`, −55, HID UUID) |
| Forget and Factory reset | `forget_targets`, `Quiescence`, `commit`, then the link status and `ManagementResult`, as `multi_conn::manage_devices` orders them | Targeted workers acknowledge the barrier at once |
| Pairing store | `DeviceList::{add, find, without, reset, pending_item, mark_saved, load, iter_recent}` with the record codec and framing | The item is written to RAM; each save is read back with `DeviceList::load` and compared with the list. Addresses are random static addresses whose low four bytes are the `u32` stand-in; peers never pair, so no record has a bond |

UART0 output is written by the `slog!` macro through `Console` in `sim.rs`
(TX P0.06, RX P0.08 in the code; Renode's UART model emits the bytes
regardless of pin routing). `defmt` messages from shared modules, such as the
button driver's `Button: …` line, go to the defmt RTT logger and do not appear
on UART0.

The platform script loads Renode's stock `platforms/cpus/nrf52840.repl`,
unregisters its `gpiote`, `gpio0`, and `gpio1`, and loads
[nrf52840-sense-gpio.repl](../renode/nrf52840-sense-gpio.repl), which places the
custom models at the same addresses and IRQ. The stock models do not implement
LATCH or DETECTMODE, so edge waits never complete with them. See
[ADR 0014](adr/0014-renode-gpio-models.md). It then unregisters `twi0`, whose
stock model is the legacy TWI without EasyDMA, loads
[nrf52840-twim-oled.repl](../renode/nrf52840-twim-oled.repl), which puts the
TWIM model at `0x40003000` on IRQ 3 with the SSD1306 model at `0x3C`, and
loads the glyph table into the panel model. See
[ADR 0024](adr/0024-renode-oled-models.md).

### OLED Checks

Renode 1.16.1 has no EasyDMA TWIM and no SSD1306, so two models written for
this project stand in for them:

- **TWIM** ([nrf52840_twim.cs](../renode/nrf52840_twim.cs)) moves whole
  buffers with EasyDMA from the pointer and count latched at the start task,
  takes the time the bytes take on the wire at the programmed frequency (the
  firmware's 100 kHz, about 0.1 s for a full frame), raises TXSTARTED,
  LASTTX, STOPPED, ERROR, and SUSPENDED with the shortcuts and interrupt
  enables the Product Specification describes, and NACKs an address no
  target answers. STOP takes effect after the byte on the wire and is
  ignored while suspended, STOPPED follows the STOP condition, a change of
  direction sends a repeated START with the address, and a buffer outside
  Data RAM moves no bytes, all as on the chip.
- **SSD1306** ([ssd1306.cs](../renode/ssd1306.cs)) decodes the I2C control
  bytes and the commands the `ssd1306` crate sends, keeps the 1 KiB display
  RAM with its addressing modes, applies segment remap to the data written
  after it, and shows a picture only while the display and the charge pump
  are on. A power-on reset leaves the RAM holding a fixed noise pattern, and
  `NoisyBytes` counts the bytes the panel receives while it is lit and still
  shows some of it.

`Text` reads the panel back with the firmware's font: it finds each row where
21 six-pixel cells all match glyphs from
[oled-font-6x10.txt](../renode/oled-font-6x10.txt), which
[tests/oled_font.rs](../tests/oled_font.rs) keeps equal to `FONT_6X10`, and
returns those lines top to bottom, keeping leading spaces. Lit pixels that no
line explains come back as `(unreadable pixels in rows A-B)`, a dark panel as
`(display off)` or `(display dark: charge pump off)`, and a multiplex ratio,
offset, start line, COM pins configuration, or scrolling other than the
128×64 defaults as `(picture not modelled: ...)`, so a check cannot pass
while something else is on the screen. The `Oled Should Show` keyword runs
the emulation in 50 ms steps, up to 0.5 s by default, until the text equals
the expected lines exactly.

| Robot step | Panel text checked | What it proves |
| --- | --- | --- |
| After boot | `bt2usb / Idle`, `SELECT: scan`, `UP: saved devices`; then `NoisyBytes` is 0 | The display task initializes the panel over the modelled TWIM and draws the published Home view, and the panel was never lit while its RAM held power-on noise |
| After each link change | `Connected` with `Keyboard`, `2 devices`, or `Mouse`, and both hints | Coordinator events reach the screen through the controller and `publish` |
| After the scan and a DOWN | `Select device`, `> Keyboard`, `  Mouse`, then the mark on `Mouse` | The device list and its selection mark |
| Saved devices | `Saved devices`, `> Mouse`, `  Keyboard`, `  Factory reset`, `UP at first: back` | The saved list, newest first, ends with Factory reset |
| Forget and reset confirmations | `Forget device?`, `Keyboard`, `> Cancel`, `  Forget` (then the mark on `Forget`); `Reset all pairings?`, `Disconnect all`, `  Cancel`, `> Reset` | Confirmations name their subject and open on Cancel |
| Notices | `Complete` with `Device forgotten` or `Pairings reset`, and `SELECT: back` | Management results reach the screen |
| Unplug, SELECT, plug back in | `(display off)` right after the panel powers up again, then Home within 2 s, and `NoisyBytes` 0 again | A frame that fails on an address NACK leaves the UI loop running; the 1 s retry initializes the panel again and draws the latest view without showing noise |

`Unplug The OLED` makes the TWIM NACK address `0x3C`
(`twi0 SetDevicePresent 0x3C false`); `Plug The OLED Back In` resets the panel
model, as a power cycle would, and makes it answer again.

A second test case, `TWIM And SSD1306 Models Follow Their Specifications`,
checks the models at register level on a halted CPU, for behavior the
firmware's display traffic does not reach:

| Check | Expected | Specification |
| --- | --- | --- |
| Empty write to an absent target, STARTTX then STOP at once | STOPPED only after the STOP condition, then ERROR, `ERRORSRC.ANACK`, and `TXD.AMOUNT` 0 | STOP takes effect after the byte on the wire, so the address byte is still NACKed |
| STOP while suspended by `LASTTX_SUSPEND` | No STOPPED until RESUME, then STOPPED | The TWI master cannot be stopped while suspended |
| SUSPEND of an empty buffer | SUSPENDED after the address byte | embassy-nrf suspends an empty write that another write follows |
| STOP 500 µs into a 17-byte write at 100 kHz | `TXD.AMOUNT` 5, no LASTTX, STOPPED | The byte on the wire finishes; later bytes are not sent |
| `TXD.MAXCNT` rewritten after STARTTX | `TXD.AMOUNT` 2 and LASTTX for the 2-byte transfer | `PTR` and `MAXCNT` are double-buffered |
| Panel lit before its RAM is cleared, then a NOP | `NoisyBytes` 2, and the panel reads as unreadable pixels | The panel powers up with undefined RAM |
| COM pins configuration `0x02`, then `0x12` | `(picture not modelled: COM pins 0x02)`, then the picture again | Only the 128×64 module's geometry is modelled |

## Renode Scenario Map

### Scripted BLE Scenario

The sim's main loop waits for either a button event or a 2-second timer. A
button event runs `UiController::button`; a command it returns runs through
`SimBle::command`, and the events that produces go to `UiController::event`,
each logged as `  event: <event> -> screen <screen> (selected <n>)`. A timer
tick runs one step of a four-step scenario through `SimBle::scenario_step`
and applies its events the same way. The timer restarts after every button
event, so a tick is "2 s without a button press", not a fixed period.

| Step (tick mod 4) | UART header | Reducer calls | UI event | Slots after (`active_count`, `occupied_count`) |
| --- | --- | --- | --- | --- |
| 0 | `scenario: connect device 0 (Keyboard)` | `plan_connect`, then `on_slot_connected`; the device is saved | `Connected 'Keyboard'`, or the current summary if it is already connected | One more active, unless already connected |
| 1 | `scenario: connect device 1 (Mouse)` | The same for the mouse | `Connected '2 devices'` when both are up | As step 0 |
| 2 | `scenario: slot 0 link lost` | `on_slot_link_lost` for the device in slot 0; then `scenario: slot 0 kept reserved for 0xa1` (or `released`, which would be a fault) | The remaining links, for example `Connected 'Mouse'` | One fewer active, the same occupied |
| 3 | `scenario: disconnect all` | `plan_disconnect`, then `on_slot_disconnected` per occupied slot | `Disconnected` when a slot was occupied | 0, 0 |

Step 2 logs `scenario: slot 0 has no link to lose` when slot 0 is empty. Every
step ends with `scenario: active_count=N occupied_count=M`, so a reserved slot
shows as an occupied one that is not active. Connect steps also log
`action: ConnectSlot …`, `action: PersistDevice …`, and, when the list changed,
`store: item of N bytes holds M device(s); reload matches`. The cycle then
repeats.

Button commands are handled as follows:

| Command | Simulation behavior | Log lines |
| --- | --- | --- |
| `StartScan` | `plan_start_scan`, then the three advertisements through `merge_advertisement` | `cmd: StartScan`, `scan: heard 3 advertisers, listed 2 HID devices`, then `ScanStarted`, one `DeviceFound` per HID device, and `ScanComplete` events |
| `Connect(index)` | `plan_connect` on the last scan's results; a new connection is saved | `cmd: Connect(n)`, the actions, and the link-status event |
| `Disconnect` | `plan_disconnect` | `cmd: Disconnect`, `action: DisconnectSlot(n)`, link status |
| `ListPaired { id }` | Replies with the saved devices, newest first | `cmd: ListPaired id=N`, `event: PairedDevices id=N ['Mouse', 'Keyboard'] …` |
| `Forget { id, address }` | Looks the device up, quiesces the slots `forget_targets` picks, commits the list without it, reports the links, then the result | `cmd: Forget id=N addr=0x…`, `quiesce: slot n released`, `quiesce: complete for token T`, `store: …`, `slots: active_count=… occupied_count=…`, link status, `ManagementResult id=N Ok(())` |
| `FactoryReset { id }` | Quiesces every slot, commits an empty list, clears the scan results | As Forget, with every slot released |

Item sizes follow the record layout in the
[data model](data-model.md#pairing-store): 22 bytes for `Keyboard` alone, 38
with `Mouse`, 19 for `Mouse` alone, and 3 for an empty list.

The simulation sends no command through a channel, so `try_send` on a full
channel ("Busy; try again") and the management timeout never happen there;
host tests cover both.

### Robot Test Case

[bt2usb-sim.robot](../renode/bt2usb-sim.robot) compiles the GPIO, TWIM, and
SSD1306 models once, in its suite setup, and has two test cases: the model
checks in [OLED checks](#oled-checks), and the scenario,
`Sim Runs The UI Controller, Display, Coordinator, Management, And Store`,
described here. The scenario creates the machine, swaps the GPIO
peripherals and `twi0`, loads the glyph table and the ELF, attaches a terminal
tester to `sysbus.uart0` with `timeout=20`, and starts
emulation. `Wait For Line On Uart` consumes output in order, so each expected
line must follow the previous one; it matches a substring of a line. The
`Press Button` keyword drives the pin low, waits for the expected line with
emulation paused, releases the pin, and runs the emulation for 100 ms, which
outlasts the debounce, so the same button can be pressed twice in a row. Each
press restarts the 2-second scenario timer, so a run of presses is never
interrupted by a scenario step.

| Order | Stimulus | Expected UART text (abridged) | What it proves |
| --- | --- | --- | --- |
| 1 | Boot | `bt2usb-sim starting`, `buttons ready`, `entering sim UI loop (screen=Home)` | Reset, memory map, executor, three button tasks, UI loop |
| 2 | Timer (step 0, pauses emulation) | `connect device 0 (Keyboard)`, `ConnectSlot slot=0 addr=0xa1`, `PersistDevice addr=0xa1`, `holds 1 device(s); reload matches`, `active_count=1 occupied_count=1`, `event: Connected 'Keyboard' -> screen Connected (selected 0)` | RTC time driver, `plan_connect` and `on_slot_connected`, the store's encode and load on the target, the controller's link update |
| 3 | SELECT | `button Select -> screen Scanning (selected 0)`, `cmd: StartScan`, `scan: heard 3 advertisers, listed 2 HID devices`, `ScanStarted`, `DeviceFound 'Keyboard' addr=0xa1 rssi=-42`, `DeviceFound 'Mouse' addr=0xb2 rssi=-55`, `event: ScanComplete -> screen DeviceList (selected 0)` | GPIO edge to `UiController::button`; scan merging drops the non-HID phone; scan events build the list |
| 4 | DOWN, UP, DOWN | `button Down -> screen DeviceList (selected 1)`, then `selected 0`, then `selected 1` | List navigation both ways |
| 5 | SELECT | `button Select -> screen Connecting (selected 1)`, `cmd: Connect(1)`, `ConnectSlot slot=1 addr=0xb2`, `holds 2 device(s); reload matches`, `event: Connected '2 devices' -> screen Connected (selected 0)` | A user connect reserves, connects, and saves the second device |
| 6 | Timer (steps 1 and 2, pauses emulation) | `connect device 1 (Mouse)`, `active_count=2 occupied_count=2`, `slot 0 link lost`, `slot 0 kept reserved for 0xa1`, `active_count=1 occupied_count=2`, `event: Connected 'Mouse' -> screen Connected (selected 0)` | An already connected device only re-reports the links; `on_slot_link_lost` keeps the slot reserved while the UI shows only the live link |
| 7 | UP, DOWN, SELECT, SELECT | `button Up -> screen Managing (selected 0)`, `cmd: ListPaired id=1`, `event: PairedDevices id=1 ['Mouse', 'Keyboard'] -> screen SavedDevices (selected 0)`, then `SavedDevices (selected 1)`, `ConfirmForget(1) (selected 0)`, `SavedDevices (selected 0)` | The saved list, newest first, under a request ID; a confirmation opens on Cancel and cancelling changes nothing |
| 8 | DOWN, SELECT, DOWN, SELECT | `ConfirmForget(1) (selected 1)`, `button Select -> screen Managing (selected 1)`, `cmd: Forget id=2 addr=0xa1`, `quiesce: slot 0 released`, `quiesce: complete for token 1`, `holds 1 device(s); reload matches`, `slots: active_count=1 occupied_count=1`, `event: Connected 'Mouse' -> screen Managing`, `event: ManagementResult id=2 Ok(()) -> screen Notice` | Forget of a device whose slot is reserved: the barrier releases that slot only, the shorter list is committed and reads back, the link status comes before the result, and the result shows the notice |
| 9 | SELECT | `button Select -> screen Connected (selected 0)` | Dismissing returns to the live link |
| 10 | UP, DOWN, SELECT, DOWN, SELECT | `cmd: ListPaired id=3`, `PairedDevices id=3 ['Mouse']`, `SavedDevices (selected 1)` (the Factory reset row), `ConfirmReset (selected 0)`, `ConfirmReset (selected 1)`, `cmd: FactoryReset id=4`, `quiesce: slot 1 released`, `quiesce: complete for token 2`, `holds 0 device(s); reload matches`, `slots: active_count=0 occupied_count=0`, `event: Disconnected -> screen Managing`, `event: ManagementResult id=4 Ok(()) -> screen Notice` | Factory reset releases every slot, commits an empty list, and reports no link |
| 11 | SELECT | `button Select -> screen Home (selected 0)` | Dismissing with no link returns home |
| 12 | Timer (step 3, then step 0) | `disconnect all`, `active_count=0 occupied_count=0`, `connect device 0 (Keyboard)`, `holds 1 device(s); reload matches`, `event: Connected 'Keyboard' -> screen Connected (selected 0)` | Nothing is left to close, and the reset store accepts writes again |

Between these steps it reads the OLED as listed in
[OLED checks](#oled-checks).

The test does not exercise the power manager's panel blanking, a full command
channel, a management timeout, a storage failure, bonds or IRK resolution, a
bus held low (the TWIM model has no stuck-bus fault), or anything about real
radio timing. Override inputs with `--variable ELF:/abs/path` or
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
the embedded build, package a release, and fill its notes. The tests build a fixture repository
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
| `test_notes_fill_every_field_from_the_package_and_source` | The repository's release notes template, filled from a fixture package, leaves no field unfilled, carries the commit, run, SoftDevice S140 v7.3.0 with its HEX and origins, the connection, saved-device, and USB identity limits, the storage pages, magic, and version, and the exact `SHA256SUMS`, and keeps its four `REVIEW:` lines |
| `test_notes_links_reach_existing_guide_headings` | Each guide link in the filled notes names a heading that exists in that guide |
| `test_prerelease_notes_say_so` | A prerelease tag fills the heading as a prerelease |
| `test_notes_refuse_a_package_that_does_not_match` | A checksum entry for a missing or path-escaping file, a wrong digest, an empty manifest, or build metadata with another version, a short commit, or no compiler stops the notes |
| `test_notes_require_the_softdevice_mask_installs` | The notes fail when the `softdevice` recipe installs a different SoftDevice version than `memory_sd.x` links against |

These tests cannot issue GitHub OIDC credentials or exercise the hosted
attestation and release APIs; see
[deployment validation limits](deployment.md#validation-limits).

## Continuous Integration

[ci.yml](../.github/workflows/ci.yml) runs on pushes to `main` or `master`,
pushes of tags matching `v*`, pull requests, manual dispatch, and a weekly
schedule (cron `23 7 * * 1`, Mondays 07:23 UTC). A newer run for the same ref
cancels an in-progress one, except for tag refs. The default token permission is
`contents: read`, and builds use `DEFMT_LOG=info`. Every job runs on a pinned
runner image, `ubuntu-24.04` or `windows-2025`, so a change of the `-latest`
labels cannot change the build environment unannounced, and every job that runs
Cargo, including the audit, installs the toolchain from `rust-toolchain.toml`
with `rustup install` before its first Cargo command. The release jobs run no
Cargo command and install no toolchain.

| Job | Runner and limit | Checks, in order |
| --- | --- | --- |
| Host tests (`ubuntu-24.04`, `windows-2025`) | Both, 20 min, `fail-fast: false` | `cargo fmt --package bt2usb -- --check`; on Linux, the 500-line limit for every `.rs` file under `src/`, `tests/`, and `build.rs` and every `.rs` or `.py` file under `scripts/`, and a `//!` module comment for every `.rs` file; release-helper tests; on Linux, the documentation checker's tests and `scripts/check_docs.py` ([Markdown checks](code-quality.md#markdown-checks)); on Linux, install actionlint 1.7.12, Ruff 0.16.9, and ShellCheck 0.11.0 (each SHA-256 verified), run the script linter's tests and `scripts/lint_scripts.py` ([Python and shell checks](code-quality.md#python-and-shell-checks)), and run actionlint; on `v*` tags, `release.py validate-tag`; `cargo test --locked --lib --tests`; host Clippy with `-D warnings`; host rustdoc with private items and `RUSTDOCFLAGS=-D warnings` |
| Host coverage | Ubuntu, 20 min | Add the `llvm-tools` component; install cargo-llvm-cov 0.9.1; `cargo llvm-cov --locked --lib --tests --no-report`; write the summary, lcov, and HTML reports; upload them; fail when line coverage is below `COVERAGE_MIN_LINES` (97) |
| Dependency security audit | Ubuntu, 10 min | `rustup install`; `cargo audit` with cargo-audit 0.22.2 and [.cargo/audit.toml](../.cargo/audit.toml), which denies warnings and ignores two advisories by ID |
| Embedded build & clippy | Ubuntu, 25 min | Embedded Clippy with `-D warnings`, without and with the `log-sensitive-data` opt-in; rustdoc with private items and warnings denied for the embedded library, then for `bt2usb` and `bt2usb-selftest`; release build (firmware and self-test); `release.py stage` with `llvm-objcopy` into the runner's temporary directory; upload |
| Renode simulation test | Ubuntu, 20 min | Simulation Clippy with `-D warnings`; rustdoc with private items and warnings denied for `bt2usb-sim`; simulation build; `scripts/install-renode.sh`; `renode-test --results-dir` on the Robot file; upload results even on failure |
| Verify and attest release package | Ubuntu, 10 min, `v*` tag pushes only, after every check job | `validate-tag`; download this run's embedded artifact by ID with digest checking; `release.py package` against the expected commit, repository, and run ID; GitHub provenance attestation; add `provenance.sigstore.json`; upload; `release.py notes` from the package; upload the notes separately |
| Prepare draft firmware release | Ubuntu, 10 min, after packaging | Download the attested package and the notes; refuse if the tag's release is already published; create or update a draft release with the notes as its description, followed by GitHub's generated notes |

Artifacts:

| Artifact | Producer | Contents |
| --- | --- | --- |
| `coverage-report-<attempt>` | Coverage job | `summary.txt` (the per-file table), `lcov.info`, and the HTML report under `html/`; uploaded before the floor check, so it exists when the floor fails |
| `bt2usb-checked-firmware-<attempt>` | Embedded job | Staged ELF/HEX, self-test ELF, build inputs, `BUILD-INFO.json`, `SHA256SUMS` |
| `renode-results-<attempt>` | Simulation job | Robot/Renode results directory (ignored if empty) |
| `bt2usb-attested-release-<attempt>` | Packaging job | Versioned release files and the provenance bundle |

CI installs the toolchain pinned in `rust-toolchain.toml`, so a local run with
the same toolchain reproduces its results. `mask ci` runs the local subset.

### Hosted CI Runs

Runs of [ci.yml](../.github/workflows/ci.yml) on `main`, read with
`gh run list` and `gh run view` on 2026-10-09 and through the GitHub Actions API
on 2026-10-10. The five check jobs in these runs are the two host-test jobs, the
dependency audit, the embedded build, and the Renode test; the Host coverage
job, added on 2026-10-10 in `27bd09e`, makes six from then on. The 2026-10-10
rows are the commits that changed CI; every other push run on `main` that day
from run number 37 to 64 also passed, except the ones a newer push cancelled.
Dependabot pull-request runs are not listed.

| Run ID | Trigger | Commit | Date (UTC) | Result | Jobs |
| --- | --- | --- | --- | --- | --- |
| 36441384244 | Push | `2479c79` | 2026-09-28 | Failed | Host tests (ubuntu-latest) failed; the other four check jobs passed |
| 36441995385 | Push | `8a04b25` | 2026-09-28 | Passed | All five check jobs passed |
| 37338711407 | Weekly schedule | `8a04b25` | 2026-10-05 | Passed | All five check jobs passed |
| 37932436721 | Push | `7fc99d6` | 2026-10-09 | Passed | All five check jobs passed |
| 37967375873 | Push | `802bbf1` | 2026-10-09 | Passed | All five check jobs passed |
| 38064576663 | Push | `6e1b8b4` | 2026-10-10 | Passed | All five check jobs passed; the commit of the [2026-10-10 validation record](#validation-record--2026-10-10) |
| 38066476307 | Push | `6fe4ca9` | 2026-10-10 | Passed | All five check jobs passed, with the new 500-line check |
| 38067332132 | Push | `27bd09e` | 2026-10-10 | Passed | All six check jobs passed; the first Host coverage run, and the firmware and simulation rustdoc steps |
| 38068103002 | Push | `c5fe413` | 2026-10-10 | Passed | All six check jobs passed, with the documentation checker |
| 38068455349 | Push | `c0b4b48` | 2026-10-10 | Passed | All six check jobs passed on Node 24 actions, `ubuntu-24.04`, and `windows-2025`; no job log has a `##[warning]` annotation, and only the audit job printed rustup's implicit-install warning |
| 38068870462 | Push | `2622228` | 2026-10-10 | Passed | All six check jobs passed; the audit job installs its toolchain first and no longer prints the rustup warning |
| 38069780606 | Push | `baa29ff` | 2026-10-10 | Passed | All six check jobs passed, with Ruff, ShellCheck, and the script linter's 7 tests in the Linux host job |

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
| Markdown checks (`scripts/check_docs.py`) | Yes (also `mask docs-check`, which adds its tests) | Yes, Linux, with its tests |
| actionlint | No | Yes, Linux |
| Ruff and ShellCheck (`scripts/lint_scripts.py`) | No (`mask lint-scripts`, which adds its tests) | Yes, Linux, with its tests |
| rustdoc with private items and warnings denied, every build | Yes (also `mask rustdoc-check`) | Yes |
| Coverage with the line floor | No (`mask coverage` reports without a floor) | Yes, Linux |
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
- documentation checks beyond links, the configuration table, named
  constants, the memory map, and commands: prose values without a constant's
  name, test counts, sizes, and coverage figures are checked by hand
  ([Markdown checks](code-quality.md#markdown-checks))
- reproducible-build comparison across clean environments

Specific to the current workflow and test tree:

- No CI job flashes a board; the self-test and first-flash layers are manual.
- The coverage floor is one line-coverage total over the 22 host-library source
  modules; a single module can lose coverage while the total stays above 97%,
  and region and function coverage have no floor.
- The Renode job runs one scripted scenario on Linux, plus register-level
  checks of its TWIM and SSD1306 models. The scenario covers the UI
  controller, link loss, saved-device management, and the OLED task with its
  recovery after an address NACK, but its connection workers, radio, and
  flash are stand-ins that answer at once
  ([Renode Scenario Map](#renode-scenario-map)), and the TWIM and SSD1306
  models are written from the Product Specification and the datasheet, not
  checked against silicon. They have no stuck-bus or data-NACK fault, so the
  STOP request after the 500 ms deadline and `wait_stopped`'s re-request are
  still unexercised, and `StopSafeI2c`'s wait for STOPPED runs without being
  proven: the 1 s retry backoff would hide its absence
  ([OLED checks](#oled-checks)).
- actionlint, Ruff, and ShellCheck run only in the Linux host job, and only
  that job installs them; no mask recipe installs them locally.
- rustdoc is checked with `--no-deps`, so the vendored `nrf-softdevice` crates'
  documentation is not built or checked.
- `cargo audit` ignores two unmaintained-crate advisories by ID, because no
  released dependency lets the graph drop them; any other warning fails the
  job ([auditing](code-quality.md#auditing)).
- Connection workers, the security handler, the GATT HID client, the storage
  shell, the USB device, and the display driver's I2C shell have no host tests
  ([details](#modules-without-host-tests)). For storage that leaves the
  conversion between SoftDevice and stored types, IRK resolution through the
  SoftDevice, and the flash retries, which only the embedded build and
  hardware check.
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

## Validation Record — 2026-10-10, Panic Lints And Inventory

This record covers the commit that turns on Clippy's panic lints, rewrites the
flagged sites, checks the flash range and the BLE event buffer at compile
time, removes four vendored panics a peer could reach, and lists the remaining
panic paths ([ADR 0025](adr/0025-panic-lints-and-inventory.md)). The checks
ran locally on Linux in a container, on the working tree just before that
commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 361 unit tests, 3 integration tests, and 3 glyph-table tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.88% of lines (from 98.16%), 98.79% of regions, 98.99% of functions |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation, each after `cargo clean -p bt2usb` | Passed. An added `.unwrap()` in `LongRead::offset` failed the host run with `unwrap_used`, so the lints are live |
| Compile-time checks | Embedded build with a changed constant | A 131-byte event buffer in `sd_setup.rs` fails the build and 132 bytes passes, so the computed worst case at ATT MTU 64 is 132 bytes; a misaligned flash range in `selftest.rs` fails the build |
| Mutation checks | Host tests | The two new descriptor tests fail when the size checks they cover are removed |
| Formatting | `cargo fmt --package bt2usb -- --check` | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five ([commands](code-quality.md#documentation-comments)) |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A` on the release ELF: with `DEFMT_LOG=debug` (the `.cargo/config.toml` default), `.text` 112,912 bytes (+64), `.rodata` 11,488 (−220), `.data` 1,640, `.bss` 23,252 (+16), `.uninit` 1,024; with `DEFMT_LOG=info`, the release setting, `.text` 112,180 bytes and the other sections unchanged. The 128 extra bytes of the event buffer are on `softdevice_task`'s stack, not in `.bss` |
| Release helper policy/integrity tests | Python 3.13, Linux | Passed: 17 tests |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 45 Markdown files |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed: the scenario in 18.00 s and the model checks in 1.30 s. The run used a local copy of `platforms/cpus/nrf52840.repl` without its `ApplySVD` line, because this container's proxy blocks the SVD download; hosted CI uses the stock platform |
| Dependency audit, actionlint | — | Not run locally; `Cargo.lock` and the workflows did not change (enabling a feature of an existing dependency does not change the lock file) |
| Vendored panic fixes | Review | Reviewed, not tested: no test reaches the vendored crate, and exercising the fixed paths needs a peer that misbehaves on purpose |
| Hosted CI | GitHub Actions | Push run 38093287153 for `5783f5b` passed every job |
| Board/radio/USB acceptance | Physical hardware | Not performed; the change needs the pairing, reconnect, and device-management checks in the first-flash checklist ([4. Pairing and daily use](first-flash.md#4-pairing-and-daily-use)) |

## Validation Record — 2026-10-10, OLED In Renode

This record covers the commit that moves each screen's text into
`ui::layout`, shares the TWIM0 setup and the display task between the bridge,
the self-test, and the simulation, clears the panel's RAM before the
`ssd1306` driver turns it on, and runs the display task in Renode on the TWIM
and SSD1306 models of [ADR 0024](adr/0024-renode-oled-models.md). The checks
ran locally on Linux in a container, on the working tree just before that
commit; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 353 unit tests, 3 integration tests, and 3 glyph-table tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.16% of lines, 98.53% of regions, 99.20% of functions; `ui/layout.rs` 100% of lines |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation | Passed |
| Formatting | `cargo fmt --all` | No changes left |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five ([commands](code-quality.md#documentation-comments)) |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A` on the release ELF: with `DEFMT_LOG=debug` (the `.cargo/config.toml` default), `.text` 112,848 bytes, `.rodata` 11,708, `.data` 1,640, `.bss` 23,236, `.uninit` 1,024; with `DEFMT_LOG=info`, the release setting, `.text` 111,868 bytes and the other sections unchanged |
| Release helper policy/integrity tests | Python 3.13, Linux | Passed: 17 tests |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 44 Markdown files |
| Headless Renode tests | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed: the scenario in 18.49 s and the model checks in 1.36 s. With the RAM clear removed from `display::initialize`, the scenario failed at boot with 2 noisy bytes. The run used a local copy of `platforms/cpus/nrf52840.repl` without its `ApplySVD` line, because this container's proxy blocks the SVD download; hosted CI uses the stock platform |
| Dependency audit, actionlint | — | Not run locally; `Cargo.lock` and the workflows did not change (`embedded-graphics`, already a dependency, is added as a dev-dependency) |
| Hosted CI | GitHub Actions | Push run 38085444796 for `bbe5a83` failed one job: the Windows host tests read the glyph table with CRLF line endings (FIXME "The glyph-table test failed on Windows" in [TODO.md](../TODO.md#fixme)); the fix in `ed63657` passed every job of push run 38085612272, including the Windows host tests and the Renode test on the stock platform |
| Board/radio/USB acceptance | Physical hardware | Not performed; the display change needs the OLED checks in the first-flash checklist, including a power-up without noise ([6. Device management and degraded display](first-flash.md#6-device-management-and-degraded-display)) |

## Validation Record — 2026-10-10, UI Controller And Renode Scenario

This record covers the commit that moves the UI loop's decisions into
`ui::controller` with the shared `ble::messages` types, removes the unused
redraw hint from the button reducer, and broadens the Renode scenario to link
loss and saved-device management (list, cancelled forget, forget, factory
reset) through [`src/sim_ble.rs`](../src/sim_ble.rs). The checks ran locally on
Linux in a container, on the working tree just before that commit; nothing ran
on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 341 unit tests and 3 integration tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 98.09% of lines, 98.47% of regions, 99.19% of functions |
| Clippy with warnings denied | Host tests, embedded, embedded with `log-sensitive-data`, simulation | Passed |
| Formatting and host API documentation | `cargo fmt --package bt2usb -- --check`; rustdoc with warnings denied | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five ([commands](code-quality.md#documentation-comments)) |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A` on the release ELF: with `DEFMT_LOG=debug` (the `.cargo/config.toml` default), `.text` 111,648 bytes, `.rodata` 11,796, `.data` 1,640, `.bss` 23,204, `.uninit` 1,024; with `DEFMT_LOG=info`, the release setting, `.text` 110,676 bytes and the other sections unchanged |
| Release helper policy/integrity tests | Python 3.13, Linux | Passed: 17 tests |
| Documentation checker | `python3 scripts/check_docs.py` | Passed: 43 Markdown files |
| Headless Renode scenario | Renode 1.16.1 portable, `renode-test renode/bt2usb-sim.robot` | Passed: 1 test in 18.55 s. The run used a local copy of `platforms/cpus/nrf52840.repl` without its `ApplySVD` line, because this container's proxy blocks the SVD download; hosted CI uses the stock platform |
| Dependency audit, actionlint | — | Not run locally; `Cargo.lock` and the workflows did not change |
| Hosted CI | GitHub Actions | Push run 38081477998 for `61e1963` passed every job, including the Renode simulation test with the stock platform |
| Board/radio/USB acceptance | Physical hardware | Not performed; the UI loop change needs the pairing and device-management checks in the first-flash checklist ([4. Pairing and daily use](first-flash.md#4-pairing-and-daily-use), [6. Device management and degraded display](first-flash.md#6-device-management-and-degraded-display)) |

## Validation Record — 2026-10-10

This record covers the FIXME fixes of 2026-10-10, from "Decide the keyboard
report in one place" (`e1447c7`) through "Set the sighting lifetime in
config.rs" (`6e1b8b4`): the keyboard-only Report Map rule in one place, the
connectable-only reconnect scan, the reconnect wakes and saved-device identity
in the pure table, the simplified connection-parameter helpers, the sighting
lifetime in `config.rs`, and the tests and documentation fixes listed under
[FIXME](../TODO.md#fixme). The checks ran locally on Linux in a container, on
the tree at `6e1b8b4`; nothing ran on a board.

| Check | Environment | Result |
| --- | --- | --- |
| Host unit/integration tests | Rust 1.95.0, Linux | Passed: 291 unit tests and 3 integration tests |
| Host coverage | `cargo llvm-cov --locked --lib --tests --summary-only` | 97.59% of lines, 98.17% of regions, 98.67% of functions |
| Clippy with warnings denied | Host tests, embedded, simulation | Passed |
| Formatting and host API documentation | `cargo fmt --package bt2usb -- --check`; rustdoc with warnings denied | Passed |
| Rustdoc with private items, warnings denied | Host library, embedded library, `bt2usb`, `bt2usb-selftest`, `bt2usb-sim` | Passed for all five ([commands](code-quality.md#documentation-comments)) |
| Release bridge, self-test, and simulation builds | Rust 1.95.0, ARM target | Passed. Bridge sections from `llvm-size -A` on the release ELF: with `DEFMT_LOG=debug` (the `.cargo/config.toml` default), `.text` 110,812 bytes, `.rodata` 11,688, `.data` 1,624, `.bss` 23,300, `.uninit` 1,024; with `DEFMT_LOG=info`, the release setting, `.text` 109,584 bytes and the other sections unchanged |
| Release helper policy/integrity tests | Python 3.13, Linux | Passed: 12 tests |
| Local Markdown links and anchors | Script over every tracked `.md` file | Passed: 1,968 links in 35 files |
| Headless Renode scenario | — | Not run locally: Renode is not installed in this environment; the scenario does not include the SoftDevice, so it would not exercise the reconnect changes |
| Dependency audit, actionlint | — | Not run locally; `Cargo.lock` and the workflows did not change |
| Hosted CI | GitHub Actions | Passed on `6e1b8b4` ([run 38064576663](https://github.com/chefzaid/bt2usb/actions/runs/38064576663)) and on every earlier commit in the range that was not superseded by a newer push |
| Board/radio/USB acceptance | Physical hardware | Not performed; the reconnect handover, the connectable-only scan, and the LED start state need the board checks in the [first-flash checklist](first-flash.md) |

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
| Release bridge and self-test builds | Rust 1.95.0, ARM target | Passed; `.text` 111,616 bytes, `.bss` 23,476 bytes, read from the release ELF's section headers, with the default `DEFMT_LOG=debug` (the level was not recorded at the time; a rebuild of that commit on 2026-10-10 reproduced these figures at `debug` and gave `.text` 110,400 bytes at `info`) |
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
Since 2026-10-10 the CI audit denies warnings and ignores exactly these two
advisories by ID ([auditing](code-quality.md#auditing)).

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
