# Work Plan

This is bt2usb's complete work plan: every task the project has taken on or
still needs, grouped by theme. Checked items are implemented and name the files
that implement them. Unchecked items are open; each carries a priority and says
what evidence closes it.

A checked item records code or documentation present in the repository. It does
not certify hardware compatibility or a production deployment. No board has yet
passed the [first-flash checklist](docs/first-flash.md), so firmware behavior is
verified in software at most: pure modules by host tests, the task shells only
by embedded builds and Clippy, and the button, UI, and coordinator path by the
Renode scenario. Such items are marked *(hardware evidence pending)*.
Evidence belongs in the
[testing guide](docs/testing.md#hardware-acceptance-evidence) or a
[hardware-result issue](.github/ISSUE_TEMPLATE/hardware-result.md).

Priorities: **P0** blocks a managed production deployment; **P1** improves
reliability, interoperability, and maintainability; **P2** is a future feature.
*(hardware)* marks an open item that needs a physical board, peripheral, debug
probe, or USB host to close.

## Status At A Glance

| Section | Done | Open | Open P0 |
| --- | ---: | ---: | ---: |
| [FIXME](#fixme) | 42 | 1 | 0 |
| [BLE Central And Pairing](#ble-central-and-pairing) | 15 | 5 | 3 |
| [HID Report Parsing And Translation](#hid-report-parsing-and-translation) | 4 | 2 | 0 |
| [USB HID Device](#usb-hid-device) | 4 | 4 | 2 |
| [Input Aggregation And Delivery](#input-aggregation-and-delivery) | 3 | 2 | 2 |
| [Pairing Storage](#pairing-storage) | 4 | 5 | 3 |
| [UI, Display And Power](#ui-display-and-power) | 9 | 2 | 1 |
| [Platform, Memory And Recovery](#platform-memory-and-recovery) | 7 | 5 | 3 |
| [Device Security And Provisioning](#device-security-and-provisioning) | 2 | 2 | 2 |
| [Board Bring-Up And Hardware Acceptance](#board-bring-up-and-hardware-acceptance) | 2 | 5 | 3 |
| [Verification And Code Quality](#verification-and-code-quality) | 13 | 3 | 0 |
| [Release, Provenance And Supply Chain](#release-provenance-and-supply-chain) | 9 | 7 | 2 |
| [Developer Experience](#developer-experience) | 8 | 1 | 0 |
| [Documentation](#documentation) | 7 | 0 | 0 |
| [Product Extensions](#product-extensions) | 0 | 28 | 0 |
| **Total** | **129** | **72** | **21** |

**Most important next step:** the
[first board bring-up](#board-bring-up-and-hardware-acceptance). Install
SoftDevice S140 v7.3.0 on an nRF52840-DK, run `mask selftest`, then work
through [first-flash.md](docs/first-flash.md) and file the result. That single
record turns most *(hardware evidence pending)* marks into evidence or into
concrete defects, and it supplies the SoftDevice RAM and stack numbers the
memory budget needs.

## FIXME

Defects found in the current tree while working through this plan, including
code that breaks the [coding-agent checklist](#how-this-plan-is-worked). They
are fixed before any other open item. Each entry says what is wrong, how it
shows, and what closes it; a fixed entry is checked and names the change that
fixed it.

- [x] **P1** **Scripts lacked the executable bit.** Git recorded every file in
  the repository as mode `100644`, including `scripts/run-tool.sh` and
  `scripts/install-renode.sh`. Almost every `maskfile.md` recipe runs
  `./scripts/run-tool.sh …`, so on a fresh Linux, macOS, or WSL clone each one
  stopped with `./scripts/run-tool.sh: Permission denied`, and `mask coverage`
  reported "No coverage tool found" even with `cargo-llvm-cov` installed (seen
  on 2026-10-10). CI was unaffected because it calls Cargo directly. Fixed by
  committing both scripts as `100755`; `mask coverage` then ran and reported
  96.16% host line coverage ([testing](docs/testing.md#troubleshooting)).
- [x] **P3** **Stale notes after the link-count change.** Commit `63d458d`
  compiled `config.rs` into the host library, but the
  [host library composition](docs/testing.md#host-library-composition) table
  and the [coverage scope](docs/code-quality.md#coverage) still listed it as
  firmware-only, and `manage_devices` kept a `clippy::too_many_arguments`
  allowance it no longer needed (seven parameters once the slot senders became
  one array). Found while bounding the management wait; fixed in the same
  commit, which also removed the allowance from the
  [lint inventory](docs/code-quality.md#lint-allowances).

The entries below came from a four-part audit of commit `4faf99f` against the
checklist on 2026-10-10; each was confirmed by a second, independent check.

- [x] **P1** **The reconnect scan counts non-connectable advertisements.**
  `find_saved_peer` (`src/ble/scanner.rs`) records a sighting for any report
  from a saved device, including `ADV_NONCONN_IND` and `ADV_SCAN_IND`. A device
  that also advertises a non-connectable set ends the other slot's scan and
  hands its slot an address for a 6 s attempt that a connection cannot use,
  and never succeeds if that set uses another private address. Fixed by the
  commit "Count only connectable advertisements in the reconnect scan": the
  callback returns early when the report's `connectable` bit is clear, and
  [ADR 0015](docs/adr/0015-shared-reconnect-scan.md), the architecture, feature,
  hardware, and security guides say so. Not yet seen on a board.
- [x] **P2** **The keyboard-only Report Map rule is written twice.**
  `src/hid/mod.rs` (reserved-byte handling) and `src/ble/hid_client.rs` (LED
  output report) each test `!has_report_ids() && has_keyboard && !has_mouse &&
  !has_consumer` by hand, and only the first is host-tested. If they drift, the
  bridge writes LEDs to a report it does not treat as the keyboard, or the
  reverse. Fixed: both call `HidDescriptor::is_unnumbered_keyboard_only`
  (through `is_keyboard_report` for the LED output report), with four host
  tests in `src/hid_keyboard_report_tests.rs`, which also took the
  reserved-byte tests out of `hid_descriptor_tests.rs` (551 to 444 lines).
- [x] **P2** **`HostLeds::current` documented the wrong start state.** The
  trait doc (`src/hid/host_leds.rs`) said it returns `None` until the host sends
  an LED state after enumeration, but every USB bus reset stores all-off in
  `KEYBOARD_LEDS` (`UsbPowerHandler::reset`), so a new link gets `Some(all off)`
  before any SET_REPORT. Fixed: the trait doc and the
  [feature guide](docs/features.md#keyboard-leds) now say `None` lasts only until
  the first bus reset, and that a keyboard connecting before the host sends its
  state gets all off first. The behavior was right and is unchanged.
- [x] **P2** **New log strings were missing from the operations guide.** The
  [log reference](docs/operations.md#ble-scan-and-connection) lacked
  `slot {} scan found slot {}'s device` and the three connection-parameter
  lines added in `4faf99f`, and its `slot {} connecting to {}` row no longer
  matched when that line appears. Fixed: all four are listed with their
  meaning and action, the row says a background reconnect logs it only after a
  reconnect scan hears the device, and the two reconnect incidents now describe
  the shared scan, its duty cycle, and the connectable-only rule. Every
  `info!`, `warn!`, and `error!` string outside the self-test is now in the
  guide.
- [x] **P2** **Python bytecode was not ignored, and got committed.** Running
  the documented release helper tests (`python -m unittest discover -s scripts
  -p "release_test.py"`) writes `scripts/__pycache__/`, which `.gitignore` did
  not cover, so it showed as untracked and was committed by mistake in "Record
  firmware sizes with their log level" (`3061a1b`). Tracked bytecode is rewritten
  whenever the tests run with another Python, leaving a modified tracked file
  that `release.py` staging refuses as a dirty tree. Fixed: the bytecode is
  removed from the repository and `.gitignore` covers `__pycache__/` and
  `*.py[cod]`.
- [x] **P2** **Source files over 500 lines grew.** `4faf99f` added lines to
  `src/ble/multi_conn.rs` (845), `src/usb/hid_device.rs` (536), and pushed
  `src/hid_descriptor_tests.rs` past the limit (551); `src/lib_tests.rs` was
  503. Fixed on 2026-10-10: `hid_descriptor_tests.rs` lost its keyboard-report
  tests to `hid_keyboard_report_tests.rs`; `multi_conn.rs` (now 356 lines) gave
  the security handler to `src/ble/bonder.rs` and the slot worker to
  `src/ble/slot_worker.rs`; `hid_device.rs` (440) gave the host's
  SET_PROTOCOL and SET_REPORT handling and the LED state to
  `src/usb/host_requests.rs`; and `lib_tests.rs` gave its classification tests
  to `src/hid_classify_tests.rs`. CI now fails when any Rust file passes 500
  lines ([code quality](docs/code-quality.md#file-length)).
- [x] **P2** **A link change on the scan screens had no test.** Since
  `4faf99f`, `UiState::connection_status` leaves a running scan or its picker on
  screen with its list when a saved device connects or drops in the
  background, and clears the list only on the Home, Connecting, and Connected
  screens. `a_new_link_returns_home_screens_to_connected_and_clears_the_list`
  covered the second half; nothing covered the first, so a regression that sent
  the user's picker back to Home passed every test. Fixed:
  `a_background_link_change_keeps_a_user_scan_and_its_list` pins both screens,
  their lists, and the highlight across a connect and a drop, and
  `a_dropped_link_returns_connecting_to_home_and_clears_the_list` covers the
  drop on Connecting.
- [x] **P2** **The advertised-kind filter for unnumbered maps lost its test.**
  `unnumbered_descriptor_rejects_unadvertised_kind` fed a 3-byte report to a
  keyboard-only map, which now takes the keyboard-only shortcut in
  `classify_notification_with_hint` and never reaches the filter that drops a
  report of a kind the map does not declare. No test covered that filter for a
  mixed map, so removing it would have let a keyboard-and-consumer device's
  mouse-sized vendor report move the host's pointer. Fixed: the test now sends
  each undeclared kind to three mixed maps without report IDs and checks the
  declared kinds still pass; with the filter removed, it is the one test that
  fails.
- [x] **P2** **Reconnect decisions in the scan shell had no host tests.**
  `SavedPeer` equality (`src/ble/scanner.rs`) decides whether re-registering a
  device keeps its slot's failure holdoff and fast-scan window: the same
  identity key, or the same address when neither record has one. The
  `RECONNECT_WAKE` bookkeeping woke the owning slot on a handover and was reset
  on scan start, after a failed attempt, and on clear, with the same
  `if let Some(signal) = RECONNECT_WAKE.get(slot)` block pasted three times,
  and a handover to a slot cleared after the scan copied the targets still
  stopped the scan and signalled that slot. Both were hardware-free but
  compiled only into the firmware, so no test covered them. Fixed:
  `reconnect::SavedPeer<A, K>` holds the identity rule and `matches` (the
  firmware passes `IdentityKey::is_match`), `ReconnectTable::record_sighting`
  returns `Recorded::{Own, HandedOver, NotRegistered}` and
  `wake_pending` says when a slot is due a wake, and every table change in
  `scanner.rs` goes through `update`, which sets or resets the slot's signal to
  match. A handover to a cleared slot now records nothing and the scan goes on.
  Eight host tests cover the rules, and the table's tests moved to
  `src/ble/reconnect_tests.rs` (`reconnect.rs` 271 lines, tests 405).
  This also closes the earlier P3 entry for the pasted reset block.
- [x] **P3** **Firmware rustdoc warning and a stale banner in `scanner.rs`.**
  The `RECONNECTS` doc linked to [`reconnect`], which does not resolve
  (firmware rustdoc with private items warns), and a "Unit Tests" banner was
  left at the end of the file when its tests moved to `adv_parser.rs`. Fixed:
  the banner went with the connectable-only fix, the link now names
  `crate::ble::reconnect`, and three more links that broke only in some builds
  (`coordinator.rs` and `power_logic.rs` in the embedded library, `ble/mod.rs`
  in the `sim` build) are written as code. Rustdoc with private items and
  warnings denied is clean for the host library, the embedded library, and all
  three binaries; the commands and the rule are in
  [code quality](docs/code-quality.md#documentation-comments).
- [x] **P3** **Duplicated and dead logic in `conn_params.rs`.** Reversed
  interval bounds were normalized twice in two styles (`bound_request` and
  `interval_within_request`); the step-back loop in `max_latency_for` could
  never run for a nonzero timeout and its comment ("can overshoot by one
  event") was false; and the tests copied the Core-rule check three times.
  Fixed: `requested_range` normalizes the range for both functions and the
  sweep test, `max_latency_for` is the closed form `(4 × timeout − 1) /
  interval − 1` with its derivation, and the tests share `meets_core_rule`. A
  new test checks the closed form against the Core rule for every timeout up to
  33 s at twelve intervals; answers are unchanged.
- [x] **P3** **`SIGHTING_TTL_MS` sat outside `config.rs` unlisted.** The
  other `ReconnectTable` timings come from `config.rs`; the 2 s sighting
  lifetime was a constant in `src/ble/reconnect.rs` and was missing from
  [constants outside config.rs](docs/hardware.md#constants-outside-configrs).
  Fixed: it is `BLE_RECONNECT_SIGHTING_TTL_MS` in `config.rs`, passed to
  `ReconnectTable::new` with the other two timings and listed in the
  [configuration defaults](docs/hardware.md#configuration-defaults); the tests
  use their own value.
- [x] **P3** **A redundant `Disconnect` arm in `connection_slot_task`.** The
  arm for a `Disconnect` that arrives between reconnect attempts repeated what
  the fall-through path does (`clear_reconnect`, then `Disconnected`). Fixed:
  the arm is gone and its comment moved to the one `Disconnect` arm, which now
  handles both cases with the same calls in the same order.
- [x] **P3** **`BLE_CONNECT_TIMEOUT_SECS` cited a scan rate attempts no longer
  use.** Its doc justified 6 s with three default 1.7 s scan intervals, but
  connection attempts now always scan at the fast duty, and the constant also
  bounds reconnect scans, which the doc did not mention. Fixed: the doc names
  both uses, the fast duty for attempts (60 windows in 6 s), the default duty
  for a reconnect scan past its fast window (three windows), and the holdoff
  derived from it.
- [x] **P3** **Docs described removed code.**
  [data model](docs/data-model.md#ui-state-model) still listed the removed
  `UiState::interactive_scan`, and ADR 0011 and the architecture guide still
  spoke of a "reconnect planner" or "boot planner" that `4faf99f` replaced with
  the reconnect table and inline boot reconnect. Fixed: the row is gone, the
  architecture guide says `ble_task` sends `Reconnect` at power-up, ADR 0011
  and the multi-device item name the reconnect table, and only ADR 0015's
  account of the old tree still names `resolve_reconnect_targets`, as history.

- [x] **P3** **The test map misdescribed two `conn_params` tests.** The
  [testing guide](docs/testing.md#test-map) said "a faster request gets
  7.5 ms" and "latency is lowered when 4 s cannot cover it", but those tests
  raise the floor to 15 ms and cap the timeout at 1 s. Fixed with the next
  entry: the faster-request test now also checks the production 7.5 ms floor,
  and the row names the floors and the 1 s cap the tests use.
- [x] **P3** **ADR 0016 and the architecture guide misstated when the
  out-of-range warning fires.** They said only a peripheral that wants nothing
  faster than 30 ms is granted an interval outside its range. A request
  entirely below 7.5 ms, which the SoftDevice cannot grant, is granted 7.5 ms
  and gets the warning too, while one whose fastest interval is exactly 30 ms
  is granted it inside its range. Fixed: both name the two cases, and host
  tests now pin the 7.5 ms case as flagged and the 30 ms case as inside its
  range.
- [x] **P3** **The recorded firmware size omitted `DEFMT_LOG` and was stale.**
  The [2026-10-09 validation record](docs/testing.md#validation-record--2026-10-09) gave
  `.text` and `.bss` for the release build without the log level, which
  [code quality](docs/code-quality.md#measuring) requires, and the code has
  changed since. Fixed: that record now names `debug`, and the
  [2026-10-10 validation record](docs/testing.md#validation-record--2026-10-10)
  gives every loaded section at `6e1b8b4` for both `debug` and `info`
  (`.text` 110,812 and 109,584 bytes).
- [x] **P3** **Three modules had no module comment.** The
  [review rule](docs/code-quality.md#documentation-comments) says every module
  starts with a `//!` comment, but `src/ble/adv_parser.rs`,
  `src/ui/input_logic.rs` (a plain `//` note instead), and
  `src/ble/coordinator_tests.rs` had none, and nothing checked the rule.
  Fixed: each now says what it owns, and the Linux host job fails on any Rust
  file under `src/`, `tests/`, or `build.rs` without a `//!` line.
- [x] **P3** **The audit job installed its toolchain implicitly.** The hosted
  run of `c0b4b48` showed rustup's "missing active toolchain has been
  auto-installed" warning in the Dependency security audit job: `cargo audit`
  goes through the rustup proxy, and that job was the only Cargo job without
  the `rustup install` step. Fixed in `2622228`: it installs the pinned
  toolchain first, like the other jobs.
- [x] **P2** **The documentation checker exceeded the 500-line limit.**
  `scripts/check_docs.py` was 676 lines when it landed (775 once formatted),
  over the coding guide's 500-line limit, which CI enforced only for Rust.
  Fixed: it is now a 73-line driver over one module per check in
  `scripts/docs_checks/` (the largest, `config.py`, under 230 lines), and the
  Linux host job applies the 500-line limit to Python files under `scripts/`
  as well ([file length](docs/code-quality.md#file-length)).
- [x] **P3** **`scripts/release.py` imported `sys` without using it**, and
  `mask rustdoc-check` and `mask ci` built the rustdoc command in word-split
  strings. Both were found by the first Ruff and ShellCheck run below and fixed:
  the import is gone, and the recipes use Bash arrays.
- [x] **P3** **The operations runbook overstated what `trace` logs.** Its
  log-sharing advice said a `trace` build also logs "security-request details,
  including the bond's master identifier (EDIV and RAND) and peer addresses".
  That line (`ble evt sec info request` in `vendor/nrf-softdevice/src/ble/gap.rs`)
  is compiled only with the crate's `ble-peripheral` feature, which bt2usb does
  not enable, and the boot-sequence note still said dependency logs were not
  audited. Found while reviewing the vendored logs below; fixed in the same
  change, which rewrote both passages from the
  [dependency log review](docs/security.md#dependency-logs).
- [x] **P3** **Boot logged a line per stored device that the docs omitted.**
  `DeviceStore::load_from_flash` rebuilt the list through the runtime `add`, so
  every boot printed `Added paired device - now storing N` for each stored
  record (or `Updated existing paired device` when two merged) before
  `Loaded N devices from flash`, while the
  [boot sequence](docs/operations.md#boot-sequence) lists only that summary line. Found while
  moving the store into host code; fixed by the move: the pure loader merges
  without logging, and only runtime adds log.
- [x] **P3** **The data model still described the store before the move.**
  After `eeae4b8` the [data model](docs/data-model.md#in-memory-cache) still
  gave `DeviceStore` the fields `devices: Vec<PairedDevice, 4>`, `dirty`, and
  `writable`, which now live in `DeviceList` as `StoredDevice` records, and
  said legacy parsing "exists only in `storage.rs` and has no host test".
  Found while reading the schema rules for the storage migration item; fixed:
  the In-Memory Cache and Legacy Format sections describe `DeviceList`, its
  `resolve` function, the `AddOutcome` log lines, and the legacy tests.
- [x] **P1** **Pairing saves could fail on buffer alignment.** `save_to_flash`
  and `load_from_flash` in `src/storage.rs`, and the self-test flash stage in
  `src/selftest.rs`, gave `sequential-storage` a plain `[u8; N]` scratch
  buffer. The map writes item data to flash straight from that buffer
  (its `ItemHeader::write_raw`, also when garbage collection moves items),
  and `nrf_softdevice::Flash::write` returns `FlashError::BufferMisaligned`
  for a source that is not 4-byte aligned in RAM. A byte array has alignment
  1, so whether a save, and with it every pairing, succeeded depended on where
  the compiler placed the buffer; no board record shows a save yet. Found by
  the fact-check of ADR 0019. Fixed: all three use `FlashBuffer`, a
  `#[repr(align(4))]` wrapper in `src/sd_setup.rs`
  ([write rules](docs/data-model.md#write-rules)).
- [x] **P3** **The button reducer computed a redraw hint nothing read.**
  `ui_logic::on_button` returned `ButtonOutcome::redraw` (`Redraw::{None,
  Scanning, DeviceList, Home, Current}`) for every press, and seven host-test
  assertions checked it, but since the display task began rendering the whole
  latest `UiState` ([ADR 0009](docs/adr/0009-isolated-display-task.md)) no
  firmware path read it; only the Renode build logged it. Found while moving
  the UI loop into `ui::controller`; fixed by removing `Redraw` and the field.
- [x] **P3** **The architecture retry table said management requests wait
  forever.** Its "Management request" row read "None | UI waits for the reply",
  although the UI has given up after `UI_MANAGEMENT_TIMEOUT_SECS` (30 s) and
  shown **No reply** since the bounded management wait landed. Found while
  updating the guide for the controller; fixed: the row names the deadline,
  the No reply screen, and `controller.rs`
  ([retries](docs/architecture.md#retries-deadlines-and-backoff)).
- [x] **P3** **The OLED showed power-on noise while its first frame was
  drawn.** The `ssd1306` crate's `init` ends by turning the panel on while its
  RAM still holds whatever it powered up with, and `render` then sent the
  1,024-byte frame at the TWIM's default 100 kHz, so for about 0.1 s after
  boot, and after a retry that followed a power loss of the panel, it showed
  random pixels. Found by the Renode SSD1306 model, which counted 1,024 frame
  bytes written to a lit panel at boot. Turning the panel off right after
  `init` still left it lit with noise for the two bytes of that command, so
  the fix clears the panel's RAM before `init`, while power-up still holds
  the panel off, then turns it off after `init` until the first frame is in
  (`display::initialize` in `src/ui/display.rs`); a render that initializes
  now sends two full frames, about 0.22 s at 100 kHz, inside the 500 ms
  deadline. The Robot test asserts that the panel receives no byte while lit
  with noise, at boot and after it is plugged back in
  ([OLED checks](docs/testing.md#oled-checks)).
- [x] **P2** **The glyph-table test failed on Windows.** The Windows host job
  of CI run 38085444796 (`bbe5a83`) failed
  `renode_glyph_table_matches_the_firmware_font`: `.gitattributes` kept
  Renode scripts and models LF but not `renode/oled-font-6x10.txt`, so the
  Windows checkout converted the table to CRLF and the byte-for-byte
  comparison failed. Fixed: `.gitattributes` keeps `renode/*.txt` LF, and the
  test compares the table with CRLF read as LF (`tests/oled_font.rs`). Push
  run 38085612272 (`ed63657`) passed every job.
- [x] **P3** **Line-number citations into bt2usb's own sources drifted.**
  The proposed ADRs 0017 to 0022 cited lines of `src/` files, `Cargo.toml`,
  `Cargo.lock`, `memory_sd.x`, and the vendored `nrf-softdevice` by number,
  and the commits since they were written moved that code: for example ADR
  0020 placed the UI loop at `main.rs` lines 244 to 376 and ADR 0019 placed
  the load in `ble_task` at `multi_conn.rs` lines 76 to 79. Found while
  renaming the display task in ADR 0020. ADR 0003 also still counted 283 test
  attributes. Fixed: the six ADRs cite bt2usb's own code and the vendored
  crate by item name (`execute_action`, `manage_devices`, `Flash::erase`, and
  so on), and keep line numbers only for dependency sources that `Cargo.lock`
  pins.
  ADR 0003 counts 359 test attributes and lists the modules and test files
  added since (`messages.rs`, `controller.rs`, `layout.rs`, and their tests).
- [x] **P1** **Peers could halt the bridge through four vendored panics.**
  The panic inventory found them in the compiled `nrf-softdevice` modules.
  At the 64-byte ATT MTU a primary-service discovery response with 15 handle
  ranges is a 132-byte event, over the crate's default 128-byte buffer, so
  `events::run_ble` panicked (`BLE_EVT_MAX_SIZE is too low`) and halted the
  chip, and after each power cycle a bonded device sent the same response to
  the next discovery. `gap::on_evt` panicked on the
  authenticated payload timeout (`unknown timeout src`), which an encrypted
  peer that ignores LE Ping causes. `Connection::drop` unwrapped the
  `DisconnectedError` that dropping a link the peer had just ended returns,
  for example after a failed MTU exchange in `connect_inner`, and
  `disconnect_with_reason` unwrapped any other SoftDevice error. Fixed:
  bt2usb enables `evt-max-size-256` and checks the buffer against the MTU at
  compile time in `src/sd_setup.rs`; the timeout arm logs `unhandled timeout
  src {:?}`; the disconnect returns `DisconnectedError` and the drop accepts
  it ([ADR 0025](docs/adr/0025-panic-lints-and-inventory.md),
  [vendored list](docs/code-quality.md#vendored-nrf-softdevice)).
- [x] **P0** **Report Maps were cut short when a peripheral offered an MTU
  above 64.** The vendored `gatt_client::att_mtu_exchange` stored the peer's
  Server RX MTU from `BLE_GATTC_EVT_EXCHANGE_MTU_RSP` as the link's ATT MTU,
  but the SoftDevice uses the smaller of that and the 64 bt2usb asks for (and
  never less than 23), as the S140 documentation of
  `sd_ble_gattc_exchange_mtu_request` says. With a peripheral that offered 65
  to 517, as current BLE stacks commonly do (247 and 517 are typical),
  `conn.att_mtu()` reported the larger value, `read_report_map` built its
  `LongRead` for fragments of that size minus 1, and the first 63-byte
  fragment looked short, so the read ended after 63 bytes. A longer Report Map
  was truncated: report IDs declared after byte 63 were unknown, and their
  reports (often the mouse and media keys) were dropped, or the map failed to
  parse. With an offer above 517, `LongRead::new` rejected the stored MTU and
  every connect failed with `HID map read failed`. Found by the panic
  inventory; not reproduced on hardware. Fixed: the exchange stores
  `server_rx_mtu.min(requested).max(23)` (`vendor/nrf-softdevice/src/ble/gatt_client.rs`,
  marked `bt2usb patch:`;
  [vendor notes](vendor/nrf-softdevice/README.bt2usb.md),
  [ADR 0007](docs/adr/0007-vendored-softdevice-patch.md)). Checked by the
  embedded builds and Clippy; the vendored code has no tests, and the
  hardware check is part of "Report Map interoperability and legacy policy".
- [x] **P1** **A bond with a private or reserved identity address broke the
  store.** `Bonder::on_bonded` kept the identity address the peer sent
  during pairing without checking its type; when the peer sent none, it
  kept the connection's address with an all-zero IRK
  (`IdentityKey::from_addr`). `execute_action` stored the bond only when
  `bond_for_address` matched it to the connection's address: a public or
  static address had to equal the identity, a non-resolvable one never
  matched, and a resolvable one had to be resolved by the bond's IRK. A
  private or reserved identity therefore reached the store only from a peer
  that connected from a resolvable private address and either distributed an IRK
  that resolved it together with an identity type the SoftDevice passes
  through (its documentation does not say whether it can), or distributed no
  identity and built its address from an all-zero IRK, which a crafted peer
  can do. `DeviceStore::add` then converted the identity with the vendored
  `Address::address_type`, which `unwrap!`s and panics for the reserved raw
  types 4 to 126. A private type (resolvable or non-resolvable) was saved, but
  the reload refuses any identity type above 1, so the next boot logged
  `Invalid or unsupported device store; writes disabled`, loaded no devices,
  and refused saves until a Factory reset: every pairing was lost. Found by
  the panic inventory's reviewer. Fixed: the raw type is decoded with
  `AddressKind::from_gap_type`, which returns `None` for a reserved one, and
  no bond whose identity is not public or random static
  (`AddressKind::is_identity`) is kept. Since the follow-up FIXME "A refused
  pairing still stored the device", `Bonder::on_bonded` refuses it when it is
  made and nothing is stored for that pairing; `DeviceStore::add`,
  `DeviceList::add`, and the reload in `codec::decode_bond` refuse it too, and
  the OLED shows `Pairing not saved` (`BleErrorTag::BondRefused`). Host tests
  decode every SoftDevice address type and save and reload a bond with each
  identity type (`src/storage/devices_tests.rs`, `devices_format_tests.rs`;
  [data model](docs/data-model.md#write-rules),
  [ADR 0006](docs/adr/0006-fail-closed-pairing-store.md)). The shell and the
  `execute_action` path are checked by the embedded builds and Clippy; no peer
  that sends such an identity was tried.
- [x] **P1** **A peripheral that refused the MTU exchange could not
  connect.** `central::connect_with_security` runs `att_mtu_exchange` inside
  the vendored `connect_inner` and failed the whole connection when it
  returned an error. A GATT server may answer the Exchange MTU Request with an
  ATT error (Request Not Supported), after which both sides keep the default
  23-byte MTU; such a peripheral failed every connect. Found by the panic
  inventory's reviewer; not reproduced. Fixed: an ATT error response logs
  `att mtu exchange refused: {:?}; keeping the default mtu` and the connect
  continues at MTU 23, while a timeout, a disconnect, or a SoftDevice error
  still fails it (`vendor/nrf-softdevice/src/ble/central.rs`, marked
  `bt2usb patch:`; [vendor notes](vendor/nrf-softdevice/README.bt2usb.md),
  [ADR 0007](docs/adr/0007-vendored-softdevice-patch.md)). Checked by the
  embedded builds and Clippy; hardware evidence belongs to "Report Map
  interoperability and legacy policy".
- [x] **P1** **A peripheral's own MTU exchange or CCCD access was never
  answered.** With the `ble-gatt-server` feature off, the vendored
  `ble::on_evt` dropped every GATT server event, including
  `BLE_GATTS_EVT_EXCHANGE_MTU_REQUEST`, which needs
  `sd_ble_gatts_exchange_mtu_reply`, and `BLE_GATTS_EVT_SYS_ATTR_MISSING`,
  which needs `sd_ble_gatts_sys_attr_set` (S140 includes the Service Changed
  characteristic and its CCCD by default). A peripheral that also acts as a
  GATT client, sending its own MTU exchange or reading or writing the bridge's
  Service Changed CCCD, got no answer; its ATT transaction timed out after
  30 s, after which the Core specification lets it send no more ATT PDUs on
  that link, including the notifications that carry its reports. Found by the
  panic inventory's reviewer; not reproduced. Fixed: without the GATT server
  feature, `on_gatts_evt_without_server` answers the MTU request with Server
  RX MTU 64 and records `client_rx_mtu.min(64).max(23)`, and answers a
  missing system attribute with the defaults
  (`vendor/nrf-softdevice/src/ble/mod.rs` and a non-panicking state lookup in
  `connection.rs`, marked `bt2usb patch:`;
  [vendor notes](vendor/nrf-softdevice/README.bt2usb.md),
  [ADR 0007](docs/adr/0007-vendored-softdevice-patch.md)). Checked by the
  embedded builds and Clippy; hardware evidence needs a peripheral that sends
  its own MTU exchange.
- [x] **P2** **Bonder callbacks re-entered the vendored connection state.**
  The vendored crate called `SecurityHandler::on_bonded` (in `gap::on_evt`,
  `BLE_GAP_EVT_AUTH_STATUS`) and `get_peripheral_key` (in
  `Connection::encrypt`) inside `Connection::with_state`, which holds a
  `&mut ConnectionState`. bt2usb's `Bonder` calls `conn.peer_address()` in
  both, which took a second `&mut` to the same state through the
  `UnsafeCell`. Two live mutable references are undefined behavior even
  though both accesses only read, so the builds behaved as intended.
  Found by the panic inventory. Fixed: every security handler call in the
  compiled modules now runs after `with_state` returns, with the handler and
  its data copied out: `on_bonded`, `on_security_update`, `display_passkey`,
  `enter_passkey` and `recv_out_of_band` (whose `Connection::from_handle`
  also re-entered the state) in `gap::on_evt`, `get_peripheral_key` in
  `Connection::encrypt`, and `security_params` in
  `Connection::request_pairing`, each marked `bt2usb patch:`
  (`vendor/nrf-softdevice/src/ble/`;
  [vendor notes](vendor/nrf-softdevice/README.bt2usb.md),
  [ADR 0007](docs/adr/0007-vendored-softdevice-patch.md)). The
  [unsafe-code inventory](docs/code-quality.md#vendored-unsafe) states the
  rule for `with_state` closures and lists the three sites left in code
  bt2usb does not compile. Checked by the embedded builds and Clippy; the
  vendored code has no tests.
- [x] **P2** **An all-zero IRK resolved private addresses.** A peer that
  distributes no identity key during pairing gets the vendored
  `IdentityKey::from_addr`, whose IRK is all zeros, and its bond keeps that
  IRK. Every resolution in bt2usb treated it as a real key:
  `IdentityKey::is_match` in `Bonder` (`on_bonded`, `bond_for_address`,
  `get_key`, `get_peripheral_key`, `forget`), `StoredBond::matches` through
  `storage::resolve`, the reconnect scan's `SavedPeer::matches`, and the
  Forget targets in `manage_devices`. A device that builds a resolvable
  private address from the all-zero IRK, which anyone can do, therefore
  matched every bond made without an IRK, such as a keyboard on a public
  address. Paired, it replaced that keyboard's keys in `Bonder`
  (`on_bonded` replaces the first match), and its record merged into the
  keyboard's in the store, overwriting its address and name
  (`DeviceList::add`); merely advertising, it counted as the keyboard during
  a background reconnect scan, so the slot connected to it and failed to
  encrypt instead of reaching the keyboard. Nordic's nRF5 SDK peer manager
  treats an all-zero IRK as no IRK. Found by the review of the bond identity
  fix. Fixed: `irk_present` in `src/storage/devices.rs` decides whether an
  IRK is a key, `StoredBond::matches` resolves a private address only with
  one, and `bonder::key_matches`, which `Bonder`, the reconnect scan, and
  Forget's slot selection now use instead of `IdentityKey::is_match`, does
  the same ([security](docs/security.md#pairing-and-authentication)). Host
  tests cover `irk_present`, `StoredBond::matches`, `DeviceList::find`, and
  `DeviceList::add` with such a device. `SavedPeer::matches` is generic over
  the resolver it is given, so the rule is tested where it lives, and the
  firmware passes it `key_matches`; that shell code is checked by the
  embedded builds and Clippy.
- [x] **P2** **A refused pairing still stored the device.** After the bond
  identity fix, `DeviceStore::add` refused the bond but stored the device
  without keys, at the private address it connected from. A peer that names
  a private identity pairs again after each address rotation, so every
  pairing added a record; in a full store each one evicted the oldest bonded
  peer, whose keys were then lost at the next boot, and the keyless records
  took boot reconnect slots although only an address the peer had already
  left matched them. A peer that sent no identity while connecting from a
  non-resolvable private address was not refused at all:
  `IdentityKey::is_match` never matches such an address, so
  `bond_for_address` found no bond, the device was stored without keys, and
  no error was shown. Found by the behavior review of the bond identity fix.
  Fixed: `Bonder::on_bonded` checks the identity the peer sends
  (`storage::is_identity_address`, which decodes the type without
  `Address::address_type`), keeps no keys for one that is not public or
  random static, logs `Bond refused: identity address is not public or random
  static`, and records the link; `execute_action` then stores nothing for it
  and shows `Pairing not saved` (`Bonder::take_refused`). As a second line of
  defense `DeviceList::add` returns `AddOutcome::BondRefused` and stores
  nothing for such a bond, and `DeviceStore::add` logs `Device store refused a
  bond whose identity is not public or random static`; `execute_action` then
  drops exactly those keys from `Bonder` (`forget_bond`). A bond already
  stored for the same peer stays. Host tests show that a refused bond stores
  nothing and leaves nothing to save, leaves the same peer's stored bond, and
  that 256 refused pairings from rotating addresses leave every bond of a
  full store (`src/storage/devices_tests.rs`;
  [data model](docs/data-model.md#write-rules)). `Bonder` and
  `execute_action` are checked by the embedded builds and Clippy.
- [x] **P2** **A slot kept retrying a device it could not secure.** When the
  link of a device for which `Bonder` held no keys ended (a refused pairing,
  or a device whose keys a newer pairing evicted from the four in memory),
  the slot worker reported `LinkLost` and retried it in the background.
  Background reconnects never pair, so every attempt failed, and the slot
  stayed reserved until the user disconnected: with two slots, one such
  device halved what the bridge could connect. Found while fixing "A refused
  pairing still stored the device". Fixed: on link loss
  `connection_slot_task` asks `Bonder::bond_for_address` for the device's
  keys and, without them, sends `SlotEvent::Disconnected`, which frees the
  slot, and logs `slot {} link lost; no keys to reconnect`, instead of
  retrying (`src/ble/slot_worker.rs`;
  [architecture](docs/architecture.md#background-reconnect)). This is the
  part of the open P1 "Visible storage/security errors" that the bridge can
  decide alone, since it knows it holds no keys; that item still covers a
  peer that lost its keys and a legacy record without a bond. The slot
  worker is checked by the embedded builds and Clippy; the coordinator's
  handling of `Disconnected` is host-tested.
- [ ] **P2** **Selecting a device whose slot is retrying leaves the UI on
  Connecting.** `plan_connect` returns no action when the selected address
  is reserved by a slot that is not connected, expecting that attempt to
  report. A background retry that cannot succeed, such as one for a peripheral
  that lost its keys after pairing with another host, never reports, so
  `Connecting...` stays on screen and the device cannot be paired again
  without disconnecting everything first. Close by sending the selection to
  that slot as an explicit connect, which may pair, with a coordinator host
  test and a guard so the slot's `Disconnected` for the superseded retry does
  not free the reservation.

## Needs Your Input

Decisions and actions only the project owner can take. The loop skips each
item listed here and moves on; answer in the item's row (or in the thread) and
it becomes workable again.

| Item | What is needed | Options (recommendation first) |
| --- | --- | --- |
| [Security maintenance ownership](#release-provenance-and-supply-chain) (P0) | A private reporting channel, which versions get fixes, who triages, and how fast reporters hear back. SECURITY.md cannot name a channel until one exists | 1. Enable GitHub private vulnerability reporting as the only channel; fix only the latest release tag and `main`; you triage; acknowledge within 7 days and give a fix or plan within 30 days. 2. Publish a security email address instead, with the same policy |
| [Replace unmaintained transitive dependencies](#release-provenance-and-supply-chain) (P1) | How to drop `proc-macro-error` (via `ssd1306`), and whether documented audit ignores may close the item while no released `cortex-m` drops `bare-metal`. The audit already fails on any other unmaintained crate | 1. Keep both ignores until upstream releases remove the crates, and accept "dropped, or ignored by ID with its chain and removal trigger" as the closing criterion; both are low risk (compile-time only, or stable core types). 2. Replace `ssd1306` with a small in-tree async driver for the 128x64 panel, under a new ADR, with host tests for the command bytes and a Renode and board display check; drops `proc-macro-error`, `maybe-async-cfg`, and their `syn` 1 tree. 3. Port `ssd1306` to `maybe-async-cfg` 0.2.5 and offer it upstream from your GitHub account, keeping the ignore until a release carries it |
| [Hardware-evidence label and private reporting](#release-provenance-and-supply-chain) (P2) | Two repository settings no tool here can change: create the `hardware-evidence` label (Issues, Labels, New label) that the hardware-result template applies, and turn on private vulnerability reporting (Settings, Code security) | 1. Do both; the loop then updates SECURITY.md and checks a new hardware-result issue. 2. Create only the label and choose an email channel in the row above |
| [ADR 0017: authenticated pairing](docs/adr/0017-authenticated-pairing-and-enrollment.md) (P0) | Four answers before the ADR can be Accepted and implemented: how Just Works-only devices (most mice) are treated, whether LE legacy pairing is allowed, what happens to bonds made under ADR 0011, and the pairing-window length | 1. Allow Just Works-only devices with mouse reports only, after a SELECT; reject legacy pairing; mark ADR 0011 bonds "pair again"; 60 s window (the ADR's recommendation). 2. Reject Just Works-only devices outright. 3. Allow legacy pairing in the Just Works tier. 4. Keep ADR 0011 bonds with full input until forgotten. 5. A 30 s or 120 s window |
| [ADR 0018: USB identity](docs/adr/0018-production-usb-identity.md) (P0) | Where the production VID/PID comes from, what serial release builds report, whether the product string changes, and when to request the PID | 1. Request a pid.codes PID now; keep the FICR factory serial; rename the product to "BLE HID Bridge" (the recommendation). 2. Buy a USB-IF vendor ID instead. 3. Report no serial, or a resettable one. 4. Keep "BT-to-USB HID Bridge". 5. Request the PID after hardware acceptance |
| [ADR 0019: power-loss-safe persistence](docs/adr/0019-power-loss-safe-persistence.md) (P0) | The commit protocol, the `sequential-storage` version, what a future layout-changing container release may do, and when old frames are rewritten | 1. Generation anchor under a second key; pin `sequential-storage` 8.0.2; hold any layout-changing release; rewrite old frames lazily on the next save (the recommendation). 2. Frame CRC and generation without the anchor. 3. Raw A/B pages without the crate. 4. Stay on 7.2.0. 5. Adopt a layout change with a confirmed Factory reset, or migrate in place. 6. Rewrite at the first boot of the new firmware |
| [ADR 0020: watchdog](docs/adr/0020-watchdog-and-progress-based-recovery.md) (P0) | The WDT timeout, whether a stuck display counts toward resets, what repeated early watchdog resets do, and whether a restart is shown on the OLED | 1. 8 s; exclude the display; latch the watchdog off after three early resets until a power cycle; show a restart notice (the recommendation). 2. 4 s. 3. Include the display. 4. Keep resetting. 5. Log the restart only |
| [ADR 0021: provisioning and readout protection](docs/adr/0021-provisioning-debug-access-and-readout-protection.md) (P0) | The production debug-port policy, whether each release ships an open service twin, the chip revision production units need, and whether bonds are encrypted at rest | 1. Lock the debug port; ship the open twin; require revision 3 (build code F or later); keep bonds unencrypted under the lock (the recommendation). 2. Leave production units open. 3. No twin image. 4. Accept any revision. 5. Encrypt bonds with a FICR-derived key |
| [ADR 0022: report translation](docs/adr/0022-descriptor-driven-report-translation.md) (P1) | How motion beyond ±127 is delivered, what a notification of the wrong length does, whether the fixed decoders stay, and what happens to a map with no translatable input | 1. Split large motion into consecutive USB reports; reject a wrong length; keep the fixed decoders for maps that match them; fail the connection with `HID map has no translatable report` (the recommendation). 2. Clamp motion. 3. Pad or truncate. 4. Remove the fixed decoders. 5. Connect and translate nothing |

## Contribution Rules For This Plan

- Work one item at a time with the coding-agent loop described in
  [How This Plan Is Worked](#how-this-plan-is-worked).
- Keep this file the single plan. Add new work here before starting it, as an
  unchecked item with a priority and an acceptance criterion; do not track open
  work only in a guide, an issue, or a code comment.
- Close hardware and security gates before adding product features. A P2 item
  must not weaken a guarantee that a P0 or P1 item depends on.
- Put decisions in hardware-free modules with host tests and keep task code thin
  ([ADR 0003](docs/adr/0003-pure-core-and-task-shell.md)). Bound and validate
  every peer- or flash-controlled value before use.
- A change that crosses an [ADR trigger](docs/architecture.md#adr-process) adds
  or supersedes an ADR in the same change. The items titled "ADR:" below must
  be Accepted before the implementation item that follows them starts.
- Describe behavior as implemented, software-verified, or hardware-verified,
  and never present an open item as a feature in another guide.

## BLE Central And Pairing

Scanning, GATT HID discovery, bonding, and the two connection slots. Context:
[features](docs/features.md#ble-central),
[architecture](docs/architecture.md#pairing-and-storage), and
[security](docs/security.md#pairing-and-authentication).

- [x] BLE central scan, GATT HID discovery, report-reference classification,
  bonding/encryption, and two connection slots (`src/ble/`).
  *(hardware evidence pending)*
- [x] Keep connection-slot, scan-merge, and command/event decisions in a pure
  coordinator reducer that returns actions for the async shell to execute,
  with host tests (`src/ble/coordinator.rs`, `src/ble/coordinator_tests.rs`,
  `src/ble/multi_conn.rs`; [ADR 0003](docs/adr/0003-pure-core-and-task-shell.md)).
- [x] Serialize SoftDevice scan and connection setup with one shared GAP
  procedure lock, and bound each connection attempt by
  `BLE_CONNECT_TIMEOUT_SECS` (6 s) so a user scan does not wait indefinitely
  behind a reconnect (`src/ble/mod.rs` `GAP_PROCEDURE`, `src/ble/slot_worker.rs`,
  `src/config.rs`). *(hardware evidence pending)*
- [x] Stored pairing/bond records, boot reconnect, identity-key matching, and
  retries after link loss; a lost or not-yet-seen paired device is retried,
  with a `BLE_RECONNECT_BACKOFF_MS` pause between attempts, while its slot
  stays reserved (`src/storage.rs`, `src/ble/reconnect.rs`,
  `src/ble/multi_conn.rs`, `src/ble/slot_worker.rs`, `src/ble/bonder.rs`).
  *(hardware evidence pending)*
- [x] Peer-identity-scoped bond replacement and key lookup, stable identity
  persistence, and private-address resolution on background reconnect
  (`src/ble/multi_conn.rs`, `src/ble/bonder.rs`, `src/ble/scanner.rs`,
  `src/storage.rs`). *(hardware evidence pending)*
- [x] Require encrypted links before HID discovery, restrict
  application-initiated fresh pairing to explicit user connection attempts, and
  handle cancellation during owned-link security/discovery
  (`src/ble/slot_worker.rs`). *(hardware evidence pending)*
- [x] Preserve UTF-8 advertising names, merge scan-response names, and release
  the radio procedure lock before delivering UI scan results (`src/ble/`).
  *(hardware evidence pending)*
- [x] Discover a BLE keyboard's LED output report during HID discovery and write
  each host LED change to it from the slot that owns the link
  (`src/ble/hid_client.rs` `write_leds`, `src/ble/slot_worker.rs`; the USB side
  is under [USB HID Device](#usb-hid-device)). *(hardware evidence pending)*
- [x] Read complete GATT Report Maps by offset up to 512 bytes, distinguish
  missing maps from invalid/unreadable/oversized maps, and restrict legacy
  classification fallback to an absent characteristic (`src/ble/long_read.rs`,
  `src/ble/hid_client.rs`, `vendor/nrf-softdevice/README.bt2usb.md`).
  *(hardware evidence pending)*
- [x] Remove peer-triggerable panics and unbounded loops from vendored GATT
  discovery: resume after a truncated characteristic response, reject
  descriptor overflow and out-of-range, non-advancing, or empty responses, and
  saturate handle arithmetic. HID discovery failures now log the cause
  (`vendor/nrf-softdevice`, `src/ble/hid_client.rs`;
  [ADR 0007](docs/adr/0007-vendored-softdevice-patch.md)). Needs real-peripheral
  evidence under the Report Map interoperability item below.
- [x] Vendor `nrf-softdevice` at the pinned upstream commit through a root
  Cargo `[patch]`, adding `gatt_client::read_by_offset` (ATT Read Blob). ATT
  read timeouts return `ReadError::Timeout` instead of leaving the read waiting,
  and discovery and MTU-exchange timeouts return errors instead of panicking
  (`vendor/nrf-softdevice/`, `Cargo.toml`,
  `vendor/nrf-softdevice/README.bt2usb.md`).
- [x] Start reconnecting saved devices at power-up without the 8-second boot
  scan, and let one passive reconnect scan look for both slots' saved devices.
  A device heard for the other slot is handed to it with its live address,
  usable once and for 2 seconds, and wakes it, so a sleeping device no longer
  holds the radio while the other slot's device advertises. A device whose
  attempt failed is left out of the other slot's scans for 6.5 seconds, so one
  that advertises but will not connect cannot keep cutting them short.
  Reconnect scans listen 50 ms of every 100 ms for 30 seconds after power-up
  or a lost link, then fall back to the default duty cycle; connection
  attempts always use the fast one. Boot no longer shows Scanning or a "No devices found" error
  (`ReconnectTable` in `src/ble/reconnect.rs`, `find_saved_peer` in
  `src/ble/scanner.rs`, `src/ble/multi_conn.rs`, `src/ble/slot_worker.rs`,
  `src/config.rs`, `src/ui/ui_logic.rs`; [ADR 0015](docs/adr/0015-shared-reconnect-scan.md)).
  Host tests cover the table: handover, single use, expiry, clearing, the
  fast window across retries, the tie-break, the holdoff after a failed
  attempt, wakes, and saved-device identity. *(hardware evidence pending)*
- [x] Write the host's current lock-key state to a keyboard as soon as its link
  starts, then every change. A keyboard that wakes and reconnects, or connects
  to a slot that already passed the last change to an earlier link, now shows
  the host's Caps Lock and Num Lock state at once, as a wired keyboard does
  when plugged in (`forward_host_leds` in `src/hid/host_leds.rs`, the
  `HostLeds` implementation for `LedReceiver` in `src/usb/host_requests.rs`,
  `run_notification_loop` in `src/ble/hid_client.rs`). Host tests poll the real
  forwarding loop, including a reconnect after the state was already
  forwarded, which fails against the old changes-only loop.
  *(hardware evidence pending)*
- [x] Bound the connection parameters a peripheral may request: the interval
  stays within 7.5–15 ms (a peripheral that asks only for slower intervals
  gets its fastest, up to 30 ms, so it does not disconnect), latency at most
  20, and the supervision timeout within 1–4 s and always above
  `(1 + latency) × interval × 2`; a request outside the bounds gets the
  nearest values and is logged with both
  (`bound_request` in `src/ble/conn_params.rs`,
  `Bonder::conn_param_update_request` in `src/ble/bonder.rs`, and the
  vendored `SecurityHandler::conn_param_update_request` hook in
  `vendor/nrf-softdevice/src/ble/security.rs` and `gap.rs`;
  [ADR 0016](docs/adr/0016-bounded-peer-connection-parameters.md)). Host tests
  sweep every policy boundary and out-of-range values. *(hardware evidence pending)*
- [ ] **P0** **ADR: authenticated pairing and enrollment.** Decide, per
  supported device class, between passkey entry and numeric comparison, how user
  presence is confirmed with the OLED and three buttons, the pairing-window
  length, and whether devices limited to Just Works are rejected. Accept when
  the ADR is Accepted, supersedes
  [ADR 0011](docs/adr/0011-interim-just-works-pairing.md), and is listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
  Drafted on 2026-10-10 as Proposed [ADR 0017](docs/adr/0017-authenticated-pairing-and-enrollment.md); waiting
  on its questions in [Needs Your Input](#needs-your-input).
- [ ] **P0** **Authenticated pairing and enrollment policy.** *(hardware)*
  Implement the ADR above: define whether each supported device uses
  authenticated pairing, how user presence is checked, and whether weaker
  devices are rejected; add a bounded pairing window and visible state.
  Require LE Secure Connections and a 16-byte minimum encryption key size.
  Today `Bonder` in `src/ble/bonder.rs` does not override
  `security_params`, so pairing uses the vendored `default_security_params`
  (`vendor/nrf-softdevice/src/ble/gap.rs`), which sets `min_key_size = 7` and
  leaves the LE Secure Connections flag clear. Accept when downgrade, legacy
  pairing, short-key, unsolicited pairing, timeout, and reconnect cases have
  automated tests plus representative real-device evidence
  ([security](docs/security.md#pairing-and-authentication)).
- [ ] **P0** **Refuse peer-initiated pairing on background reconnects.**
  *(hardware)* The application starts pairing only for an explicit user
  connection, but the vendored crate answers a peer's Security Request by
  requesting pairing when no keys are found
  (`BLE_GAP_EVTS_BLE_GAP_EVT_SEC_REQUEST` in
  `vendor/nrf-softdevice/src/ble/gap.rs`). Background reconnects also target
  stored peers without a bond, so such a peer can start pairing. Accept when a
  Security Request on a link whose request does not allow pairing cannot create
  or replace a bond, shown by a test or by recorded on-air evidence
  ([security](docs/security.md#threat-model)).
- [ ] **P1** **Report Map interoperability and legacy policy.** *(hardware)*
  Validate the implemented 512-byte fragmented GATT reader against real
  peripherals with long maps and different MTUs, including one that offers an
  MTU above 64 and one that keeps 23. Decide whether the absent-map
  compatibility fallback remains allowed for deployment. Accept when captures
  demonstrate full reads and error handling, and same-length incompatible
  layouts are rejected by the supported descriptor-driven translation policy
  ([architecture](docs/architecture.md#hid-path-and-limits)).
- [x] **P1** **Scan list under crowding.** The scan kept the first
  `BLE_MAX_DISCOVERED` (8) HID advertisers it heard, so a crowded room, or
  deliberate fake advertisers, could hide the intended device. Decided on
  signal strength, the choice that needs no new UI: since 2026-10-10
  `merge_advertisement` lets a new HID advertiser replace the listed device
  with the weakest latest RSSI when it is received more strongly, so a scan
  ends with the eight strongest HID advertisers and a peripheral held next to
  the bridge is always listed. Ties keep the listed device, an unavailable RSSI
  (127) ranks last, and a replaced device can return only through an
  advertisement that carries the HID UUID. Six host tests in
  `src/ble/coordinator_tests.rs` cover it, including a keyboard at -40 dBm
  heard after twenty advertisers at -70 to -89 dBm. Advertisers that reach the
  bridge more strongly than the intended device can still crowd it out
  ([security](docs/security.md#threat-model); `src/ble/coordinator.rs`,
  `src/ble/scanner.rs`).
- [ ] **P1** **Keyboard ready in time for firmware setup keys.** *(hardware)*
  A monitor that powers its hub together with the PC boots the bridge at the
  same moment as the PC, and the keyboard must work before the PC's firmware
  stops waiting for a setup key such as F2 or Del. The bridge now starts
  reconnecting at power-up with a fast, shared reconnect scan (done above;
  [ADR 0015](docs/adr/0015-shared-reconnect-scan.md)), but no board has shown
  that it is fast enough, and the fast duty cycle's current is unmeasured.
  Accept when the
  [first-flash cold-start check](docs/first-flash.md#5-in-the-monitor), in
  which the monitor and the PC power on together and a setup key pressed
  repeatedly from power-on opens firmware setup, passes for the named
  keyboards, monitors, and PCs of the hardware compatibility baseline, also
  with the mouse switched off; the times from VBUS to the first delivered
  keystroke and from a key press on a sleeping keyboard to its first delivered
  keystroke are published in
  [features](docs/features.md#boot-and-reconnect); and the
  [power budget](#ui-display-and-power) measurement covers the fast reconnect
  duty cycle, USB suspend included. If the times fall short, the SoftDevice
  device identity list and whitelist are the next option
  ([ADR 0015](docs/adr/0015-shared-reconnect-scan.md#alternatives-considered)).

## HID Report Parsing And Translation

Turning peer descriptors and notifications into the fixed internal report
types. Context: [architecture](docs/architecture.md#hid-path-and-limits) and
[data model](docs/data-model.md#usb-hid-report-contracts).

- [x] Bounded HID collection/global-state parsing, Push/Pop handling, malformed
  descriptor rejection, overflow-safe report-size arithmetic, and strict
  known-report routing (`src/hid/report_protocol.rs`, `src/hid/mod.rs`, and
  descriptor and classification regression tests in
  `src/hid_descriptor_tests.rs`).
- [x] Consumer usage bounds, exact report-reference direction handling,
  rejection of oversized notifications, and distinct GATT payload classification
  (`src/hid/consumer.rs`, `src/ble/hid_client.rs`).
- [x] Five-button/scroll-capable mouse and consumer report types, alongside the
  six-key USB keyboard format: BLE mouse reports of 3 to 5 bytes map to five
  buttons, X/Y, wheel, and horizontal scroll (AC Pan), and consumer usages are
  limited to `MAX_CONSUMER_USAGE` (`0x0FFF`) (`src/hid/`).
  *(hardware evidence pending)*
- [x] Accept keyboards that use the reserved byte: when the characteristic's
  Report Reference resolves to the keyboard report of a Report Map with report
  IDs, or a Report Map without report IDs describes only a keyboard, the
  report's OEM-reserved second byte (HID 1.11) is ignored and sent to the PC
  as zero; the zero check stays on the legacy
  length-only and conventional-report-ID paths, where it guards
  classification (`KeyboardReport::from_identified_bytes` in
  `src/hid/keyboard.rs`, `KindSource` in `src/hid/mod.rs`; tests in
  `src/hid_descriptor_tests.rs`;
  [security](docs/security.md#input-validation-boundaries)).
- [ ] **P1** **ADR: descriptor-driven HID report translation.** Choose how
  fields are located by usage, bit offset, width, signedness, and report ID, what
  the bounded translation tables look like, and how unsupported layouts fail.
  Accept when the ADR is Accepted and listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
  Drafted on 2026-10-10 as Proposed [ADR 0022](docs/adr/0022-descriptor-driven-report-translation.md); waiting
  on its questions in [Needs Your Input](#needs-your-input).
- [ ] **P1** **Descriptor-driven report translation.** Decode fields by usage,
  bit offset, width, signedness, and report ID for NKRO, packed mouse buttons,
  and 16-bit movement. Accept when a fixture corpus covers supported layouts and
  unsupported layouts fail explicitly without misclassifying input
  ([architecture](docs/architecture.md#hid-path-and-limits)).

## USB HID Device

The composite keyboard, mouse, and consumer-control device the PC sees.
Context: [features](docs/features.md#usb-hid-device) and
[hardware](docs/hardware.md#configuration-defaults).

- [x] Composite USB keyboard, mouse, and consumer interfaces, keyboard LED
  forwarding, remote-wakeup requests, and software VBUS event handling
  (`src/usb/hid_device.rs`, `src/usb/host_requests.rs`).
  *(hardware evidence pending)*
- [x] USB boot/report protocol negotiation, three-byte boot mouse
  serialization, keyboard LED control-request validation, reset state cleanup,
  and stable factory-derived per-unit USB serials (`src/usb/hid_device.rs`,
  `src/usb/host_requests.rs`, `src/hid/mouse.rs`).
  *(hardware evidence pending)*
- [x] Publish the host's keyboard LED output report through a `Watch` that both
  BLE slots observe, and clear it on USB reset (`src/usb/host_requests.rs`
  `KEYBOARD_LEDS`, `src/hid/keyboard.rs` `KeyboardLeds`).
  *(hardware evidence pending)*
- [x] Enable the SoftDevice's USB detected, power-ready, and removed events and
  seed the software VBUS detector from `USBREGSTATUS`, so unplug and replug are
  noticed after boot (`src/sd_setup.rs` `enable_usb_power_events`,
  `src/main.rs`). *(hardware evidence pending)*
- [ ] **P0** **ADR: production USB identity and unit identity.** Decide the
  VID/PID source, manufacturer and product strings, revision numbering, and how
  the factory-derived serial is used for unit identity. Accept when the ADR is
  Accepted and listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
  Drafted on 2026-10-10 as Proposed [ADR 0018](docs/adr/0018-production-usb-identity.md); waiting
  on its questions in [Needs Your Input](#needs-your-input).
- [ ] **P0** **USB production identity.** *(hardware)* Obtain an assigned
  VID/PID and define product, manufacturer, revision, and unit-identity policy;
  `src/config.rs` still uses the development `0x1209`/`0x0001`. Accept when
  descriptors and host inventory distinguish two units and development
  identifiers are absent from production builds
  ([hardware](docs/hardware.md#configuration-defaults)).
- [ ] **P1** **HID/USB conformance.** *(hardware)* Define and implement
  supported `GET_REPORT` and `SET_IDLE` behavior, validate boot/report protocol
  transitions, and reject invalid control requests consistently. Accept when
  descriptor/request tests and USB captures from a real host demonstrate the
  supported behavior ([testing](docs/testing.md#known-verification-gaps)).
- [ ] **P1** **Confirm USB stability without an explicit HFXO request.**
  *(hardware)* The USB peripheral needs the high-frequency crystal oscillator,
  but nothing under `src/` calls `sd_clock_hfclk_request` or otherwise starts
  it, so the USB clock depends on whatever the SoftDevice and `embassy-nrf`
  start on their own. Accept when a bring-up log and a long enumeration and
  typing run on a board show USB stays stable with the radio idle and active,
  or the firmware requests the crystal explicitly and that change has the same
  evidence ([hardware](docs/hardware.md#clocks-and-radio)).

## Input Aggregation And Delivery

Merging two BLE sources and delivering them through independent USB endpoint
workers. Context: [architecture](docs/architecture.md#hid-path-and-limits),
[features](docs/features.md#input-delivery), and
[ADR 0005](docs/adr/0005-two-slots-and-independent-endpoints.md).

- [x] Per-source held-input aggregation and bounded coalescing/delivery policy
  with host-testable logic (`src/hid/aggregate.rs`, `src/hid/coalesce.rs`,
  `src/hid/delivery.rs`); residual load and hardware acceptance work is open
  below.
- [x] Aggregate source-tagged input across two BLE slots, preserve the
  surviving source on disconnect, implement six-key rollover and deterministic
  consumer priority, and run independent bounded USB endpoint workers with
  retained-state replay and new-press-only wake (`src/hid/aggregate.rs`,
  `src/hid/delivery.rs`, `src/hid/wake.rs`, `src/usb/hid_device.rs`). Host
  regressions include actual asynchronous workers with fake endpoint sinks.
  *(hardware evidence pending)*
- [x] Release every key, mouse button, and consumer usage a BLE link was
  holding when that link ends for any reason: the slot worker sends
  `HidEvent::Disconnected` after its run phase, so a lost keyboard cannot leave
  a key repeating on the host (`src/ble/slot_worker.rs`, `src/hid/aggregate.rs`).
  *(hardware evidence pending)*
- [ ] **P0** **Multi-device aggregation hardware acceptance.** *(hardware)*
  Validate the implemented per-source key/modifier/button unions and
  consumer-slot priority with two real peripherals sharing an endpoint. Accept
  when releasing/disconnecting one source preserves the other's held state and
  the six-key rollover/consumer-priority behavior matches the documented policy
  on the USB host
  ([first flash](docs/first-flash.md#6-device-management-and-degraded-display)).
- [ ] **P0** **Backpressure behavior and bounded recovery.** *(hardware)*
  Specify whether transient taps and accumulated motion may be lost while USB is
  stalled. Test sustained input, queue saturation, suspend, unplug, and a
  simultaneous BLE disconnect. Accept when final releases reach a recovered host
  within a measured bound, neither slot starves, and any intentional loss policy
  is documented ([architecture](docs/architecture.md#hid-path-and-limits)).

## Pairing Storage

The versioned, fail-closed paired-device store in four reserved flash pages.
Context: [data model](docs/data-model.md#pairing-store),
[ADR 0006](docs/adr/0006-fail-closed-pairing-store.md), and
[security](docs/security.md#key-storage-and-deletion).

- [x] Frame the paired-device blob with a magic byte, version, record count, and
  per-record length prefixes in a pure, host-tested module; an unversioned blob
  loads through the legacy parser and is written in the versioned format on the
  next save (`src/storage/framing.rs`, `src/storage.rs`).
- [x] Validate complete storage frames and record metadata, reject unknown
  formats/types and malformed bond lengths, abort partial serialization, and
  preserve unreadable stores by disabling writes (`src/storage/`,
  `src/storage.rs`).
- [x] Retry a flash write up to three times, 20 ms apart, because SoftDevice
  flash operations need radio-idle timeslots and can fail transiently while
  links are active, and skip the write when only the RSSI hint changed
  (`src/storage.rs` `FLASH_WRITE_ATTEMPTS`, `DeviceStore::add`).
  *(hardware evidence pending)*
- [x] Add saved-device management with default-Cancel confirmations, stable
  selected identities, worker quiescence before mutation, transactional cached
  bond updates, and explicit reset recovery of unreadable storage (`src/ui/`,
  `src/ble/management.rs`, `src/ble/multi_conn.rs`, `src/storage.rs`).
  *(hardware evidence pending)*
- [ ] **P0** **ADR: power-loss-safe persistence and storage migration.**
  Decide the commit protocol for interrupted writes and garbage collection, the
  version-upgrade and downgrade rules, and whether a major `sequential-storage`
  upgrade may change the on-flash layout. Accept when the ADR is Accepted and
  listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
  Drafted on 2026-10-10 as Proposed [ADR 0019](docs/adr/0019-power-loss-safe-persistence.md); waiting
  on its questions in [Needs Your Input](#needs-your-input).
- [ ] **P0** **Forget/reset hardware and interruption acceptance.**
  *(hardware)* Validate the implemented confirmation, worker shutdown,
  persistence, and cache-update paths on a board. Accept when forgotten peers
  cannot reconnect across reboot, failed writes retain the documented state,
  and power interruption during both logical deletion and unreadable-store
  recovery has a tested outcome
  ([first flash](docs/first-flash.md#6-device-management-and-degraded-display)).
- [ ] **P0** **Power-loss-safe persistence.** *(hardware)* Fault-inject flash
  writes, garbage collection, malformed records, and version changes. Accept
  when interrupted writes retain the last committed valid state or take a
  documented safe reset path, without accepting corrupted bond material or
  silently replacing it ([data model](docs/data-model.md#write-rules)).
- [ ] **P1** **Versioned storage migration.** Document each wire version and
  supported upgrade/downgrade paths. The legacy parser and the identity merge
  moved into the host-tested `src/storage/devices.rs` on 2026-10-10, whose tests
  already refuse future versions and cover a valid legacy conversion and
  malformed items; capacity changes and a written migration policy remain. Accept
  when fixture tests reject unknown versions and cover valid legacy
  conversion, malformed data, and capacity changes
  ([data model](docs/data-model.md#schema-change-rules)).
- [ ] **P1** **Pairing region survives application reflash.** *(hardware)*
  Normal application flashing is expected to preserve pages 240 to 243, but the
  erase behavior of `probe-rs` in `mask run --release` has not been checked.
  Accept when a board with saved peers is reflashed with the documented command
  and the peers reconnect without pairing again, with the probe-rs version and
  erase mode recorded ([deployment](docs/deployment.md#flash-a-development-unit)).

## UI, Display And Power

The OLED, three buttons, UI state machine, and display power policy. Context:
[features](docs/features.md#local-ui-and-power),
[architecture](docs/architecture.md#execution-and-power-choices), and
[ADR 0009](docs/adr/0009-isolated-display-task.md).

- [x] Async OLED rendering, three debounced buttons, and activity-driven
  display power policy (`src/ui/`, `src/power_logic.rs`).
  *(hardware evidence pending)*
- [x] Drive the SSD1306 through async I2C (`ssd1306` `async` feature with
  `embedded-hal-async`) so a display transfer yields to other tasks, and enable
  the TWIM internal pull-ups for modules without their own (`src/ui/display.rs`,
  `src/main.rs`, `Cargo.toml`). *(hardware evidence pending)*
- [x] Keep screen transitions and button handling in pure, host-tested
  reducers (`src/ui/ui_logic.rs`, `src/ui/input_logic.rs`,
  `src/ui/display_logic.rs`).
- [x] Scroll the discovered-device list to keep its selection visible, reject
  invalid selections, use a persistent UI ticker, and handle button levels and
  suspend-aware activity (`src/ui/`, `src/main.rs`, `src/power_logic.rs`).
- [x] Retain errors and completion notices until acknowledgment; isolate OLED
  rendering from input/UI tasks, retry completed I2C failures, and request STOP
  after the display deadline without unsafely dropping an active DMA future
  (`src/main.rs`, `src/ui/display.rs`, `src/ui/ui_logic.rs`;
  [ADR 0009](docs/adr/0009-isolated-display-task.md)).
  *(hardware evidence pending)*
- [x] Tag saved-device list, Forget, and reset requests with a unique ID and
  allow one at a time, so a delayed or duplicate reply cannot complete a newer
  action (`ManagementRequests`, in `src/ui/controller.rs` since 2026-10-10,
  `src/ble/multi_conn.rs`). Reducer tests cover stale IDs and ID wraparound.
- [x] Hold TWIM NACK/overrun errors until the peripheral reports STOPPED. The
  pinned driver returns before STOPPED, which could release display-interface's
  transfer buffer during the stop sequence and let the retry clear the pending
  event (`src/ui/display.rs` `StopSafeI2c`, also used by the self-test).
  *(hardware evidence pending)*
- [x] Count live HID traffic as activity so typing keeps the OLED on, and never
  enter System-OFF on bus power, so BLE links and USB enumeration stay up
  (`src/power.rs` `note_hid_activity`, called from `src/usb/hid_device.rs`;
  `src/power_logic.rs`;
  [ADR 0012](docs/adr/0012-bus-powered-no-system-off.md)).
  *(hardware evidence pending)*
- [x] Bounded management wait in the UI. Each saved-device list, Forget, or
  reset request carries a deadline `UI_MANAGEMENT_TIMEOUT_SECS` (30 s) away;
  the 1 s housekeeping tick expires it, the UI drops its saved-device snapshot
  and shows **No reply** with `Forget result unknown`, `Reset result unknown`,
  or `List not loaded` (an error already showing stays), UP reopens saved
  devices, and a late reply is rejected by its request ID. Reducer tests cover
  the lost reply, the late reply before and after a new request, and the
  screen; seven more `UiState` tests raised `ui_logic.rs` from 81% to 97% line
  coverage, and its tests moved to `ui_logic_tests.rs` (2026-10-10;
  [features](docs/features.md#manage-saved-devices)).
- [ ] **P0** **Power budget and USB suspend current.** *(hardware)* Measure
  supply current while idle, scanning (at the default duty cycle and at the
  fast reconnect duty cycle of a 50 ms window every 100 ms), with two links, with the OLED on and off,
  and during USB suspend, where the bridge keeps its BLE links. Compare with the
  100 mA the configuration descriptor declares (`src/usb/hid_device.rs`
  `max_power`) and with the USB suspend-current limit. Accept when the measured
  margins are recorded and either meet those limits or a reviewed exception
  updates [ADR 0012](docs/adr/0012-bus-powered-no-system-off.md)
  ([release gates](docs/deployment.md#release-gates)).
- [ ] **P1** **Visible storage/security errors.** Surface pairing persistence
  failure, bond replacement, full-store eviction, unsupported reports, and
  security failures with useful user actions. Today a failed save shows only
  the generic `Storage failed` error, and a link that cannot be secured shows
  `Connect failed` (`controller::error_message` in `src/ui/controller.rs`). Three cases do not
  reach the user at all. A newly paired device whose identity address matches
  a stored peer replaces that peer's record and bond (`DeviceStore::add` in
  `src/storage.rs`, `Bonder::on_bonded` in `src/ble/bonder.rs`), leaving
  only the `Updated existing paired device` log line. A full store evicts its
  oldest peer with only the
  `Paired device store full - evicting oldest entry` log line. A background
  reconnect whose link cannot be secured, because the peer lost its keys or
  the store has none for it (a legacy record carries no bond), fails with
  `ConnectFailed`, which a silent reconnect treats as "try again"
  (`connection_slot_task` in `src/ble/slot_worker.rs`); the slot retries after
  each `BLE_RECONNECT_BACKOFF_MS` pause, with no limit, and the UI shows
  nothing. Accept when UI tests cover every state; the user is told, before or
  when it happens, that a pairing replaced an existing peer's bond or evicted
  the oldest peer; a reconnect that fails for missing keys on either side ends
  in a visible state that tells the user to pair the device again instead of
  retrying silently; and a user can distinguish a temporary link failure from
  a peer that was not saved
  ([operations](docs/operations.md#recovery-and-diagnostics)).

## Platform, Memory And Recovery

The MCU platform, interrupt and memory layout, and recovery from stuck
subsystems. Context: [hardware](docs/hardware.md#memory-layout),
[ADR 0002](docs/adr/0002-nrf52840-softdevice-embassy.md), and
[ADR 0010](docs/adr/0010-static-memory-layout.md).

- [x] Build on the nRF52840 with Nordic SoftDevice S140 v7.3.0 and Embassy's
  async executor, with static allocation and no heap (`Cargo.toml`,
  `src/main.rs`; [ADR 0002](docs/adr/0002-nrf52840-softdevice-embassy.md)).
- [x] Share one SoftDevice configuration (two central links, 64-byte ATT MTU,
  no advertising or peripheral role) between the bridge and the self-test
  (`src/sd_setup.rs`).
- [x] Run the USBD, TWISPI0 (OLED I2C), GPIOTE, and time-driver interrupts at
  priority 2, outside the levels the SoftDevice reserves (0, 1, and 4), so they
  cannot preempt its critical sections (`src/main.rs`).
  *(hardware evidence pending)*
- [x] Separate application and pairing flash regions, linker assertions, and
  stack high-water instrumentation (`memory_sd.x`, `src/stack.rs`).
- [x] Link with plain rust-lld instead of flip-link, because nrf-softdevice
  passes `__sdata` to the SoftDevice as its application RAM base; `memory_sd.x`
  asserts that `.data` starts at `ORIGIN(RAM)` and the stack sits at the top of
  RAM (`.cargo/config.toml`, `memory_sd.x`;
  [ADR 0010](docs/adr/0010-static-memory-layout.md)).
  *(hardware evidence pending)*
- [x] Select `memory_sd.x` or `memory_sim.x` by feature in `build.rs` and copy
  it to `OUT_DIR/memory.x`. Neither source is named `memory.x`, so no layout in
  the crate root shadows the copied one (rust-lld searches the current directory
  first), and the script reruns when the `sim` feature toggles (`build.rs`).
- [x] Single source for the pairing flash range. `config.rs` derives
  `STORAGE_FLASH_START` and `STORAGE_FLASH_END` from the page constants and
  `FLASH_PAGE_SIZE`, which the store and the self-test now use. `build.rs`
  compiles `config.rs` and writes the range ahead of the linker script, and
  `memory_sd.x` asserts that `FLASH` ends at `STORAGE_FLASH_START` and that
  storage ends within flash. Changing the start page, the page count past the
  end of flash, or the `FLASH` length alone was shown to fail the link, and
  changing the start page and the length together links (2026-10-10;
  [hardware](docs/hardware.md#memory-layout)).
- [ ] **P0** **ADR: watchdog and progress-based recovery.** Decide which tasks
  must prove progress before the watchdog is fed, the timeout, what state
  survives a reset, and how reset causes are recorded. Accept when the ADR is
  Accepted and listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
  Drafted on 2026-10-10 as Proposed [ADR 0020](docs/adr/0020-watchdog-and-progress-based-recovery.md); waiting
  on its questions in [Needs Your Input](#needs-your-input).
- [ ] **P0** **Watchdog and recoverable failures.** *(hardware)* Define
  progress-based watchdog feeding and recovery for stuck I2C, stalled USB, flash
  errors, and BLE task failure; today no watchdog exists and a stuck I2C bus
  leaves the display task degraded. Stuck-bus cases need an I2C fault-injection
  fixture, which the first-flash OLED isolation check also lacks. Accept when
  injected faults cannot leave permanent held input or require an undocumented
  recovery sequence; record reset causes
  ([operations](docs/operations.md#recovery-and-diagnostics)).
- [ ] **P0** **Memory and endurance budget.** *(hardware)* Measure SoftDevice
  RAM at enable (`softdevice RAM: N bytes`) and worst-case stack high-water
  (`stack high-water: X of Y bytes`) with two links, scanning, display, and
  persistence, as the
  [first-flash checklist](docs/first-flash.md#2-self-test-image) asks. Verify
  flash size and storage wear assumptions. Accept when reviewed margins are
  recorded and `memory_sd.x` matches the measured requirement; do not reduce
  its current 24 KiB reservation based only on estimates.
- [ ] **P1** **Stack overflow detection.** Without flip-link a stack overflow
  grows into `.bss` and corrupts statics silently; no MPU guard region is
  configured, and the painted-stack high-water mark is evidence after the fact.
  Evaluate a guard compatible with the SoftDevice RAM layout. Accept when a
  deliberate overflow in a test build faults with a diagnosable message instead
  of corrupting memory ([ADR 0010](docs/adr/0010-static-memory-layout.md)).
- [ ] **P1** **Diagnostics without sensitive input.** Add firmware/build
  identification, reset reasons, bounded counters for reconnect/queue/write
  failures, and a documented collection method. Log each peripheral's Device
  Information Service PnP ID (characteristic `0x2A50`) after HID input is
  flowing, and have the hardware-result template ask for it. Accept when
  reports support reproduction without logging key material or keystroke
  content
  ([operations](docs/operations.md#reporting-a-defect)).

## Device Security And Provisioning

Physical access, key protection, and production provisioning. Context:
[security](docs/security.md#security-posture-summary) and
[data model](docs/data-model.md#privacy-and-retention).

- [x] Keep bond keys and HID report contents out of application logs: no
  `defmt` statement under `src/` formats bond keys, BLE input reports,
  notification bytes, or flash record bytes (the host's keyboard LED state is
  logged as `Host LEDs: num={} caps={} scroll={}`); CI and release-package
  builds log at `info` (`src/`, `.github/workflows/ci.yml`,
  `scripts/release.py`;
  [security](docs/security.md#logging-and-privacy)).
- [ ] **P0** **ADR: provisioning, debug access, and readout protection.**
  Decide the production debug-port policy, readout protection, key lifetime and
  disposal, service recovery, and whether bond records need protection beyond
  readout protection. Accept when the ADR is Accepted and listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
  Drafted on 2026-10-10 as Proposed [ADR 0021](docs/adr/0021-provisioning-debug-access-and-readout-protection.md); waiting
  on its questions in [Needs Your Input](#needs-your-input).
- [ ] **P0** **Provisioning and physical key protection.** *(hardware)* Define
  production debug access, readout protection, key lifetime, disposal, and
  service recovery. Today `src/main.rs` and `src/selftest.rs` pass
  `embassy_nrf::config::Config::default()` to `embassy_nrf::init`, changing
  only interrupt priorities. In the pinned embassy-nrf 0.7.0 that default sets
  `debug: Debug::Allowed`, and on nRF52 chips with the improved APPROTECT
  `init` then writes `UICR.APPROTECT` to its software-disabled value and
  disables APPROTECT at every boot. Readout protection therefore needs
  `nrf_config.debug` set to `Debug::Disallowed` in the production build, plus a
  documented UICR and recovery procedure: how a unit flashed with a
  debug-allowed build is protected, and how a protected unit is reopened for
  service, which erases the whole chip, pairing store included. Accept when the
  threat model and provisioning procedure are reviewed and readout/recovery
  behavior is demonstrated on a production-equivalent board
  ([security](docs/security.md#key-storage-and-deletion)).
- [x] **P1** **Vendored debug and trace logs.** Local builds log at `debug`
  (`.cargo/config.toml`), where the vendored `nrf-softdevice` recorded each peer
  address (`connected role={:?} peer_addr={:?}` in
  `vendor/nrf-softdevice/src/ble/central.rs`), and at `trace` it logged raw
  notification bytes, which are keystrokes
  (`GATT_HVX write handle={:?} type={:?} data={:?}` in
  `vendor/nrf-softdevice/src/ble/gatt_client.rs`).
  Accept when the default local build records no peer addresses, notification
  data can be logged only through an explicit, documented opt-in, and the debug
  output of `embassy-usb`, `embassy-nrf`, and `sequential-storage` has been
  reviewed ([security](docs/security.md#logging-and-privacy)).
  Done: the vendored crate has a `log-sensitive-data` feature, forwarded by
  bt2usb's feature of the same name and off by default. Without it the connect
  line logs only the role, the notification line logs `len={}` instead of the
  bytes, and the passkey-display line (unused by Just Works pairing) omits the
  passkey. Listing the defmt strings of built ELFs confirmed that the default
  `debug` build has no `peer_addr`, a `trace` build has no notification bytes
  or passkey, and an `info` build has none of the three lines. CI and
  `mask clippy` also run embedded Clippy with the feature. The review of every
  crate the firmware builds with `defmt` found no other line that prints an
  address, key, or input: `embassy-usb` traces control OUT data, which is the
  host's LED output report; `embassy-nrf` logs peripheral state and UICR
  warnings; `sequential-storage` has no log statements
  ([dependency logs](docs/security.md#dependency-logs),
  [vendored patch notes](vendor/nrf-softdevice/README.bt2usb.md),
  [ADR 0007](docs/adr/0007-vendored-softdevice-patch.md)). A `trace` build
  still records typing rhythm through one line per notification, so the
  guides keep `trace` off units used for real typing.

## Board Bring-Up And Hardware Acceptance

Getting firmware onto real boards and recording what works. Context:
[first flash](docs/first-flash.md),
[testing](docs/testing.md#hardware-acceptance-evidence), and
[ADR 0004](docs/adr/0004-layered-verification.md).

- [x] Board self-test and reusable first-flash checklist (`src/selftest.rs`,
  [first-flash.md](docs/first-flash.md)).
- [x] Fix self-test flash ownership and pairing-record buffer capacity; reject
  combined firmware/simulation features (`src/selftest.rs`, `build.rs`).
- [ ] **P0** **First board bring-up.** *(hardware)* Install S140 v7.3.0, run
  `mask selftest`, then run the bridge on an nRF52840-DK with the documented
  wiring, one BLE keyboard, one BLE mouse, a direct host connection, and then a
  monitor hub. Accept when a
  [hardware-result issue](.github/ISSUE_TEMPLATE/hardware-result.md) records the
  commit and ELF hash, a self-test summary line with zero failures
  (`==== self-test done: P passed, 0 failed, S skipped ====`), the SoftDevice
  RAM and stack values, and pass, fail, or skipped-with-reason for every
  [first-flash](docs/first-flash.md) step; each failure gets its own issue.
- [ ] **P0** **Hardware compatibility baseline.** *(hardware)* Run
  [first-flash.md](docs/first-flash.md) on declared keyboard/mouse models,
  Windows/Linux/macOS hosts, monitor hubs, and BIOS/UEFI or KVM targets. Accept
  when the matrix identifies exact versions, pass/fail results, known
  limitations, the connection parameters each peripheral asked for and was
  granted ([ADR 0016](docs/adr/0016-bounded-peer-connection-parameters.md)),
  and the artifact hash.
- [ ] **P0** **Production hardware definition.** *(hardware)* Choose and
  document the deployed board (the DK or a custom PCB), display module, buttons,
  supply arrangement, and enclosure; move any pin changes into the board setup
  in `src/main.rs`, `src/selftest.rs`, and `src/sim.rs`. Accept when
  [hardware](docs/hardware.md#parts-and-wiring) lists the production bill of
  materials and pin map and that board has a passing first-flash record
  ([release gates](docs/deployment.md#release-gates)).
- [ ] **P1** **Hardware-in-the-loop check.** *(hardware)* No CI job flashes a
  board; the self-test and first-flash layers are manual. Attach a board and
  probe to a dedicated runner that flashes `bt2usb-selftest` for changes to
  the BLE, USB, storage, or memory-map boundary, kept apart from the release
  jobs. Accept when that job fails on any `[FAIL]` self-test line and its RTT
  log is kept as an artifact
  ([testing](docs/testing.md#known-verification-gaps)).
- [ ] **P1** **Soak and latency measurements.** *(hardware)* Run a defined
  multi-day keyboard/mouse workload with disconnects, monitor power changes, and
  flash updates. Accept when latency percentiles, reconnect time,
  dropped-input policy, reset count, and stack/flash margins meet published
  limits ([testing](docs/testing.md#known-verification-gaps)).

## Verification And Code Quality

Host tests, simulation, and code-health work. Context:
[testing](docs/testing.md), [code quality](docs/code-quality.md), and
[ADR 0004](docs/adr/0004-layered-verification.md).

- [x] Pure shared host-test modules, unit/integration tests, and coverage tasks
  (`src/lib.rs`, `tests/`, `maskfile.md`).
- [x] Share one implementation between firmware and host tests: split the
  oversized `src/lib.rs`, `src/ble/coordinator.rs`, and `src/storage.rs` into
  sibling test and codec files, and drop the duplicate HID classifier from the
  host library (`src/lib.rs`, `src/lib_tests.rs`, `src/lib_logic_tests.rs`,
  `src/ble/coordinator_tests.rs`, `src/hid_descriptor_tests.rs`,
  `src/storage/codec.rs`; commit `e3bc620`). The files that grew past 500
  lines again have since been split; see "Keep source files within a size
  limit" below.
- [x] SoftDevice-free Renode build and a headless GPIO/UI/coordinator scenario
  (`src/sim.rs`, `memory_sim.x`, `renode/bt2usb-sim.resc`,
  `renode/bt2usb-sim.robot`).
- [x] Custom Renode GPIO/GPIOTE models of the pin SENSE, LATCH, DETECT, and
  GPIOTE PORT event chain that embassy-nrf uses for async edge waits, so the
  Robot test presses UP, DOWN, and SELECT through the real button tasks and
  `BUTTON_CHANNEL` (`renode/nrf52840_sense_gpio.cs`,
  `renode/nrf52840-sense-gpio.repl`;
  [ADR 0014](docs/adr/0014-renode-gpio-models.md)).
- [x] Enforce `// SAFETY:` comments on `unsafe` blocks: the `[lints.clippy]`
  table in `Cargo.toml` turns on `undocumented_unsafe_blocks` for every
  target, so the host, firmware, self-test, and simulation Clippy steps, which
  run with `-D warnings`, fail on an undocumented block; `#![forbid(unsafe_code)]`
  in `src/lib.rs` keeps every module the host library compiles free of
  `unsafe` (`Cargo.toml`, `src/lib.rs`;
  [code quality](docs/code-quality.md#unsafe-code-policy)). Checked by removing
  the comment in `src/stack.rs` (embedded Clippy failed with "unsafe block
  missing a safety comment") and adding an `unsafe` block to `src/hid/wake.rs`
  (host Clippy failed on `forbid(unsafe_code)`).
- [x] Run the scanner's advertisement tests on the host: of the ten
  `#[test]` functions in `src/ble/scanner.rs`, which never compiled, the four
  that covered new cases (a HID UUID among other UUIDs, an incomplete UUID
  list, an empty advertisement, a shortened name alone) moved to
  `src/ble/adv_parser.rs`, and the six that repeated `src/lib_logic_tests.rs`
  were deleted. Every `#[test]` in `src/` and `tests/` now runs under
  `cargo test --locked --lib --tests`
  ([testing](docs/testing.md#tests-that-do-not-run)).
- [x] Single source for the link count and UI capacities.
  `BLE_MAX_CONNECTIONS` in `src/config.rs` now sizes `MAX_CONNECTIONS`,
  `SOURCES`, `LED_CONSUMERS`, the SoftDevice connection and role counts, the
  `SlotSenders` array, and in `main.rs` the slot command channel array and a
  `ble_slot_task` pool spawned once per slot. The `UiState` lists and the
  `paired` snapshot take their capacities from `BLE_MAX_DISCOVERED` and
  `MAX_PAIRED_DEVICES`, and the host crate compiles `config.rs` so the pure
  modules can. The firmware and simulation were shown to build with one and
  three links; the host tests assume two links and stop compiling when the
  count changes (2026-10-10;
  [development](docs/development.md#add-a-configuration-constant)).
- [ ] **P1** **Parser fuzzing and property tests.** Add bounded fuzz targets for
  HID descriptors, advertisements, report classification, and persistence
  framing. Accept when CI runs a seed corpus and scheduled fuzzing records no
  panics, out-of-bounds access, excessive work, or invalid accepted output
  ([testing](docs/testing.md#known-verification-gaps)).
- [ ] **P1** **Async task fault tests.** Exercise command cancellation, full
  event channels, scan/reconnect contention, repeated security failures, and
  device disappearance during discovery. Accept when deterministic test cases
  assert completion, retry policy, and UI state without relying only on reducer
  tests ([testing](docs/testing.md#known-verification-gaps)).
- [ ] **P1** **Host tests for the I/O shells.** The connection workers,
  security handler, GATT HID client, USB device, and display driver
  (`src/ble/multi_conn.rs`, `src/ble/slot_worker.rs`, `src/ble/bonder.rs`,
  `src/ble/hid_client.rs`, `src/usb/hid_device.rs`, `src/usb/host_requests.rs`,
  `src/ui/display.rs`) have no host tests; only the pure modules they call do.
  The storage shell (`src/storage.rs`) is down to flash I/O and SoftDevice type
  conversion since the device-store move. Move remaining decisions into
  hardware-free modules or test the shells against fakes. Accept when each
  has host tests for its error paths, or the testing guide records why it
  cannot ([testing](docs/testing.md#modules-without-host-tests)).
- [x] **P1** **Host tests for the device store.** The in-memory store moved
  out of the `src/storage.rs` shell on 2026-10-10 into host-compiled modules
  under ADR 0003: `src/storage/devices.rs` (`DeviceList`: fail-closed load,
  the legacy parser, identity merge and bond replacement, eviction at
  `MAX_PAIRED_DEVICES`, and the Forget and factory-reset candidates) and a pure
  `src/storage/codec.rs`. Address resolution with an IRK stays in the shell and
  reaches the list as a `resolve` function, so tests pass a fake. 27 host tests
  in `src/storage/devices_tests.rs` and `src/storage/devices_format_tests.rs`
  cover load of valid, legacy, malformed, and unreadable stores, merge, bond
  replacement, eviction, lookup, Forget and reset published through
  `management::commit` only after a successful save, and codec round trips.
  The shell keeps flash I/O, the write retry, SoftDevice type conversion, and
  the log lines; host line coverage rose to 97.90%
  ([testing](docs/testing.md#pairing-storage); [data model](docs/data-model.md#pairing-store);
  `src/storage.rs`, `src/lib.rs`).
- [x] **P1** **Broaden the Renode scenarios.** Since 2026-10-10 the Robot
  test covers link loss, the saved-device management screens, and the OLED.
  Step 2 of the scenario now calls `coordinator::on_slot_link_lost`, and the
  test asserts `slot 0 kept reserved for 0xa1` with
  `active_count=1 occupied_count=2`; until then it called
  `on_slot_disconnected`, which frees the slot, under a "link lost" label. The
  simulation runs the firmware's UI loop decisions, moved out of `main.rs`
  into the host-tested `ui::controller` (16 tests), and a simulated
  coordinator (`src/sim_ble.rs`) that calls the real coordinator reducers,
  `merge_advertisement`, `management::{forget_targets, Quiescence, commit}`,
  and `DeviceList` with its codec, reading every saved item back. The test
  lists the saved devices, cancels and then confirms a Forget of the device
  whose slot is reserved, and confirms a Factory reset
  ([scenario map](docs/testing.md#renode-scenario-map)). The simulation also
  spawns the firmware's display task (`ui::display::task`, shared with the
  bridge through `display::new_twim`) on two C# models written for Renode,
  which has neither: an EasyDMA TWIM (`renode/nrf52840_twim.cs`) and an
  SSD1306 (`renode/ssd1306.cs`), attached by `renode/nrf52840-twim-oled.repl`.
  The panel model reads its text back with the firmware's font
  (`renode/oled-font-6x10.txt`, kept equal to `FONT_6X10` by
  `tests/oled_font.rs`), and the screens' lines moved into the host-tested
  `ui::layout` (12 tests in `src/ui/layout_tests.rs`). The test checks the
  panel's text on every screen it reaches, an OLED that stops answering and
  comes back, and that the panel is never lit while it shows power-on noise;
  a second test case checks the models against the Product Specification and
  the datasheet ([OLED checks](docs/testing.md#oled-checks),
  [ADR 0024](docs/adr/0024-renode-oled-models.md)). Both passed locally with
  Renode 1.16.1, and the CI simulation job runs the same Robot file
  (`renode/bt2usb-sim.robot`).
- [x] **P1** **Coverage and firmware documentation in CI.** Since 2026-10-10
  the Host coverage job runs `cargo llvm-cov` 0.9.1, uploads the summary,
  lcov, and HTML reports as `coverage-report-<attempt>`, and then fails below
  97% of lines, set from the 97.59% baseline as a ratchet that guards the pure
  core rather than serving as evidence
  ([ADR 0023](docs/adr/0023-host-coverage-floor.md),
  [coverage in CI](docs/code-quality.md#coverage-in-ci)). The embedded and
  simulation jobs document the embedded library, `bt2usb`, `bt2usb-selftest`,
  and `bt2usb-sim` with private items and warnings denied, the host job adds
  private items, and `mask rustdoc-check` and `mask ci` run the same four builds
  ([documentation comments](docs/code-quality.md#documentation-comments)).
  Checked locally: `--fail-under-lines 99` exits 1, and a broken intra-doc
  link in `src/ble/bonder.rs` fails the firmware rustdoc run
  (`.github/workflows/ci.yml`, `maskfile.md`).
- [x] **P2** **Inventory panic sites in firmware paths.** Since 2026-10-10
  Clippy's `indexing_slicing`, `string_slice`, `unwrap_used`, `expect_used`,
  `panic`, `unreachable`, `todo`, and `unimplemented` lints are on for every
  target and denied in CI; `clippy.toml` allows `unwrap_used`, `expect_used`,
  `indexing_slicing`, and `panic` in tests, and `build.rs` and
  `tests/oled_font.rs` allow the ones they use at crate level. Of the
  112 sites they flagged outside tests, 109 were rewritten without a panic
  path and 3 keep `#[expect]` with the bound as the reason. The constructs no
  lint flags (`unwrap!` on spawns, `StaticCell`, `RefCell`, heapless
  capacity, dependency calls, the one runtime divisor, flash futures) and
  every panic path in the compiled vendored modules are listed with the
  reason each cannot fire; a second reviewer checked each entry.
  `Address::address_type`, open at first, was closed by a FIXME above. Four vendored
  panics a peer could reach were fixed (FIXME above), the flash range and
  the BLE event buffer are checked at compile time, and the inventory added
  five FIXMEs (`Cargo.toml`, `clippy.toml`, `src/`,
  `vendor/nrf-softdevice/src/ble/`;
  [ADR 0025](docs/adr/0025-panic-lints-and-inventory.md),
  [code quality](docs/code-quality.md#panics-allocation-and-arithmetic)).
- [x] **P2** **Lint the release helper and shell scripts.** Since 2026-10-10
  `scripts/lint_scripts.py` runs `ruff check` and `ruff format --check` (Ruff
  0.16.9, settings in `ruff.toml`) over every tracked Python file, and
  ShellCheck 0.11.0 over `scripts/install-renode.sh`, `scripts/run-tool.sh`,
  `.devcontainer/post-create.sh`, and each of the 34 Bash recipes in
  `maskfile.md`, which it extracts with mask's option and argument variables
  declared and reports at their `maskfile.md` line. The Linux host job installs
  both tools from their releases with a SHA-256 check, runs the script's 7
  tests, then the script, then actionlint, which now uses the same ShellCheck
  for the `run:` scripts; `mask lint-scripts` runs it locally. The first run's
  19 Ruff findings, unformatted files, and 6 ShellCheck notes were fixed (see
  FIXME). Verified locally: an unquoted variable added to a script, an unused
  import added to `release.py`, and an unchecked `cd` added to a recipe each
  made the script exit 1 with the finding
  ([code quality](docs/code-quality.md#python-and-shell-checks);
  `scripts/lint_scripts.py`, `scripts/lint_scripts_test.py`, `ruff.toml`,
  `.github/workflows/ci.yml`, `maskfile.md`).
- [x] **P2** **Keep source files within a size limit.** Every Rust file under
  `src/`, `tests/`, and `build.rs`, and every Python file under `scripts/`, is
  at most 500 lines since the 2026-10-10
  split (largest after the device-store move: `src/hid_descriptor_tests.rs`
  486, `src/ble/coordinator_tests.rs` 465), and
  the host-tests job fails on any file over the limit
  ([code quality](docs/code-quality.md#file-length); `src/ble/bonder.rs`,
  `src/ble/slot_worker.rs`, `src/usb/host_requests.rs`,
  `src/hid_classify_tests.rs`, `.github/workflows/ci.yml`).

## Release, Provenance And Supply Chain

CI, tagged releases, provenance, and dependency maintenance. Context:
[deployment](docs/deployment.md),
[testing](docs/testing.md#continuous-integration),
[security](docs/security.md#supply-chain), and
[ADR 0008](docs/adr/0008-attested-draft-releases.md).

- [x] GitHub Actions build/test/simulation workflow and tag-based firmware
  artifact generation (`.github/workflows/ci.yml`).
- [x] Migrate to current embedded crate releases (commit `dc11b4a`, "Upgrade
  crates versions and fix latent issues"): `embassy-executor` 0.7 to 0.10
  (feature `arch-cortex-m` renamed `platform-cortex-m`), `embassy-nrf` 0.3 to
  0.7, `embassy-time` 0.4 to 0.5, `embassy-usb` 0.4 to 0.6, `ssd1306` 0.9 to
  0.10, `sequential-storage` 3 to 7, and `defmt`, `defmt-rtt`, and
  `panic-probe` to 1.x; `usbd-hid` and `cortex-m-semihosting` were removed
  (`Cargo.toml`). *(hardware evidence pending)*
- [x] Pin Rust/dependency resolution and BLE Git revision, align Cargo license
  metadata with the existing GPL license, and use locked build tasks
  (`rust-toolchain.toml`, `Cargo.lock`, `Cargo.toml`, `maskfile.md`;
  [ADR 0013](docs/adr/0013-pinned-toolchain-and-mask-tasks.md)).
- [x] Configure Linux/Windows host checks, embedded/simulation lint and build,
  scheduled dependency auditing, pinned action revisions, restricted default
  workflow permissions, and draft release artifacts with checksums/build inputs
  (`.github/workflows/ci.yml`, `.github/dependabot.yml`). GitHub Actions has
  confirmed the push and scheduled path: push runs 36441995385 (commit
  `8a04b25`, 2026-09-28) and 37932436721 (commit `7fc99d6`, 2026-10-09) and
  scheduled run 37338711407 (2026-10-05) passed all five check jobs. The
  tag-only release jobs have never run; see the hosted
  provenance item below.
- [x] Install actionlint 1.7.12 from its upstream release and verify its
  SHA-256 before use, replacing an install-action fallback that failed the Linux
  host job (`.github/workflows/ci.yml`).
- [x] Validate exact release tag/Cargo version equality, including
  prereleases; reuse the successful embedded build by immutable artifact ID;
  verify source, build-input and firmware hashes before packaging; configure
  SHA-pinned GitHub provenance attestations separately from draft publication
  permissions (`scripts/release.py`, `.github/workflows/ci.yml`). Twelve helper
  regression tests pass on Windows and WSL; actionlint 1.7.12 passes. Hosted
  issuance and downloaded-release verification remain open below.
- [x] Stage release inputs and Renode results in the runner's temporary
  directory instead of the Rust-cached `target/`. Fail the release job before
  upload when the tag's release is already published; reruns may update only a
  draft. Action pin comments name the exact upstream tag each SHA resolves to
  (`.github/workflows/ci.yml`,
  [deployment](docs/deployment.md#re-running-a-tag-workflow)).
- [ ] **P0** **Hosted provenance and release recovery acceptance.**
  *(hardware)* Push the first release tag (none exists yet), run the configured
  tag/attestation workflow, verify its downloaded artifacts against the approved
  commit from a clean machine, check that the draft's description is the filled
  [release notes](docs/deployment.md#release-notes) followed by GitHub's change
  list, and document storage migrations, rollback constraints, and service
  flashing. Accept when provenance verification and failed-update recovery have
  evidence; local helper tests and workflow lint do not exercise GitHub signing,
  the release API, or device recovery
  ([deployment](docs/deployment.md#verify-before-flashing)).
- [x] **P0** **Release notes for deployment releases.** Since 2026-10-10 each
  draft's description starts from `.github/release-notes.md`, which
  `release.py notes` fills in the `release-package` job from the verified
  package and the tagged source: the exact SoftDevice prerequisite (S140
  v7.3.0, its HEX, and the application origins, refused unless `maskfile.md`
  installs the version `memory_sd.x` links against), compiled connection,
  saved-device, and USB identity limits, the pairing storage pages and version,
  rollback constraints, and `SHA256SUMS`. A `REVIEW:` line asks for supported
  versions, tested hardware, the migration, and the oldest rollback target; the
  `release` job uses the text as the draft body, and GitHub appends its change
  list. Five new helper tests fill the real template and check its guide links;
  the first hosted draft is part of "Hosted provenance and release recovery
  acceptance" ([release notes](docs/deployment.md#release-notes);
  `scripts/release.py`, `scripts/release_test.py`, `.github/workflows/ci.yml`).
- [ ] **P0** **Security maintenance ownership.** Publish a private reporting
  contact, supported-version policy, triage ownership, and response/update
  expectations. Accept when dependency/advisory review, license inventory, and
  remediation tracking are part of a documented release procedure
  ([security policy](SECURITY.md)). Waiting on the owner's choice in
  [Needs Your Input](#needs-your-input).
- [ ] **P2** **Create the hardware-evidence issue label and enable private
  vulnerability reporting.** The
  [hardware-result template](.github/ISSUE_TEMPLATE/hardware-result.md) applies
  the `hardware-evidence` label, which does not exist in the repository, and
  GitHub private vulnerability reporting is disabled (both checked with the
  GitHub REST API on 2026-10-09), so [SECURITY.md](SECURITY.md) has no private
  channel to point to. Accept when a new hardware-result issue carries the
  label, the **Report a vulnerability** button opens a private advisory, and
  SECURITY.md names that channel. Waiting on the owner in
  [Needs Your Input](#needs-your-input).
- [ ] **P1** **Supply-chain and tooling maintenance.** Extend the dependency
  audit and update automation with license checks, an SBOM artifact, and
  verified digests for downloaded non-Cargo tooling/SoftDevice inputs; today
  `scripts/install-renode.sh` and `mask softdevice` download archives without a
  digest check. Accept when a license conflict or digest mismatch fails the
  relevant check with an actionable message and dependency alerts have an
  assigned review process ([security](docs/security.md#supply-chain)).
- [ ] **P1** **Replace unmaintained transitive dependencies.** The 2026-09-28
  audit reported `bare-metal 0.2.5` (`RUSTSEC-2026-0110`) and
  `proc-macro-error 1.0.4` (`RUSTSEC-2024-0370`) as unmaintained; both are still
  in `Cargo.lock`. Trace their dependency chains, track the upstream migration,
  and adopt maintained replacements through dependency upgrades; CI ran
  `cargo audit` without a deny option until 2026-10-10, so these warnings did
  not fail the job. Accept when the lockfile no longer selects these affected versions,
  the audit is clean without advisory suppression, the CI audit fails on
  unmaintained-crate warnings, and host/firmware/simulation regression checks
  pass ([testing](docs/testing.md#validation-record--2026-09-28)).
  Progress (2026-10-10): `.cargo/audit.toml` now makes `cargo audit` fail on
  unmaintained, unsound, and yanked crates, and ignores only these two
  advisories by ID, each with its chain and removal trigger, so any new
  advisory fails CI. Neither crate can be dropped by an upgrade today:
  `bare-metal` has no patched version and comes from `cortex-m` 0.7.9, the
  newest release, whose main branch still uses it; `ssd1306` 0.10.0 and its
  main branch pin `maybe-async-cfg =0.2.4`, and with 0.2.5 (which drops
  `proc-macro-error`) `ssd1306` fails to compile, so it needs porting or
  replacing. Both run no code that a peer can reach: `proc-macro-error` runs
  only at compile time, and `bare-metal` supplies the `Mutex` and
  `CriticalSection` types of `cortex-m` 0.7
  ([auditing](docs/code-quality.md#auditing)). The route for
  `proc-macro-error` waits on [Needs Your Input](#needs-your-input).
- [ ] **P1** **Major dependency upgrades.** *(hardware)* Dependabot proposed
  embassy-nrf 0.11, embassy-sync 0.8, and sequential-storage 8. Pull requests
  #7 and #8 (the first two) were closed unmerged, and the open sequential-storage
  8.0.2 pull request #9 fails the embedded Clippy step. Upgrade deliberately:
  recheck the driver assumptions documented in `src/usb/hid_device.rs`
  (`UsbReportSink::write` cancellation) and `src/ui/display.rs` (`StopSafeI2c`,
  `finish_or_stop`), and prove the pairing store still loads. Accept when CI
  passes, existing stores load after the upgrade, and the affected first-flash
  sections pass on a board
  ([security](docs/security.md#supply-chain)).
- [x] **P1** **CI runtime maintenance.** Since 2026-10-10 every action runs
  on Node 24 or is composite: `actions/checkout` v7.0.1 and
  `actions/upload-artifact` v7.0.1 replace the Node 20 v4.4.0 and v4.6.2, and
  `softprops/action-gh-release` v3.0.3 replaces the Node 20 v2.6.2 in the
  tag-only release job. `Swatinem/rust-cache` is pinned to commit `6323deb`
  (`v2.9.2`) instead of the `v2` tag object, `taiki-e/install-action` names
  `v2.87.21`, every job runs on `ubuntu-24.04` or `windows-2025` instead of the
  moving `-latest` labels (`ubuntu-latest` was to move to Ubuntu 26 from
  2026-10-19), and every job that runs Cargo, the audit job included, calls
  `rustup install` first, ending rustup's warning that implicit installation
  is deprecated
  ([pinning](docs/code-quality.md#pinning)). Accepted on hosted runs: the
  six check jobs of run 38068455349 (`c0b4b48`) logged no `##[warning]`
  annotation, run 38068870462 (`2622228`) removed the last rustup warning from
  the audit job, and run 38069780606 (`baa29ff`) passed clean
  ([hosted runs](docs/testing.md#hosted-ci-runs)). The release jobs' actions
  were checked from their `action.yml`; they first run with the first tag, under
  [Hosted provenance and release recovery acceptance](#release-provenance-and-supply-chain)
  ([testing](docs/testing.md#continuous-integration)).
- [ ] **P1** **Reproducible firmware evidence.** Compare artifacts from two
  clean environments, document remaining nondeterminism, and enforce release
  size budgets. Accept when the release record identifies compiler,
  dependency, external-tool, source, and artifact hashes
  ([testing](docs/testing.md#known-verification-gaps)).

## Developer Experience

Tasks, tooling, and environments for working on the firmware. Context:
[development](docs/development.md), the [task reference](maskfile.md), and
[ADR 0013](docs/adr/0013-pinned-toolchain-and-mask-tasks.md).

- [x] WSL-aware task tooling and a VS Code devcontainer
  (`scripts/run-tool.sh`, `.devcontainer/`).
- [x] Wrap build, flash, self-test, host tests, coverage, size, simulation,
  SoftDevice, and probe tasks as 34 Bash `mask` recipes; `mask ci` runs the
  local formatting, lint, test, rustdoc, Markdown, and build subset
  (`maskfile.md`).
- [x] Build host tests for the native platform by leaving `build.target` unset,
  and scope the probe-rs runner and ARM linker flags to
  `cfg(all(target_arch = "arm", target_os = "none"))` (`.cargo/config.toml`).
- [x] Install portable Renode and the `renode-test` Python dependencies into
  the user's home without root (`scripts/install-renode.sh`, `mask sim-setup`).
- [x] Resolve WSL tool fallbacks only from the current Windows user's profile;
  fix the devcontainer build recipe and stop SoftDevice download/flash tasks on
  download or extraction failure (`scripts/run-tool.sh`, `maskfile.md`).
- [x] Stop devcontainer setup when required tool installation or its host-test
  smoke check fails (`.devcontainer/post-create.sh`). Scoped probe permissions
  and optional-tool/container pinning remain open below.
- [x] Ignore the downloaded SoftDevice HEX and zip that `mask softdevice`
  writes to the repository root (`/s140_nrf52_7.3.0_softdevice.hex` and
  `/softdevice.zip` in `.gitignore`; 2026-10-09).
- [x] Fix maskfile coverage and install recipes. The tarpaulin fallback in
  `mask coverage`, `coverage-html`, and `coverage-json` now passes
  `--lib --tests`, so both coverage paths run the same unit and integration
  tests. `mask deps`, `mask coverage-install`, and the devcontainer run
  `cargo install --locked` with exact versions (probe-rs-tools 0.32.0,
  cargo-binutils 0.4.0, cargo-bloat 0.12.1, mask 0.11.7, cargo-llvm-cov
  0.9.1), and `mask deps` calls `$MASK coverage-install` unquoted, because
  mask sets `$MASK` to `mask --maskfile <path>` (2026-10-10;
  [development](docs/development.md#environment-setup)).
- [ ] **P1** **Development environment hardening.** *(hardware)* Pin the
  container inputs and replace blanket container privilege with scoped
  probe access where feasible. Today the devcontainer runs `--privileged` on the
  `mcr.microsoft.com/devcontainers/rust:1-bookworm` tag. The Cargo tools are
  pinned (done above). Accept when fresh Linux/WSL setups pass checks and
  a no-probe setup still works
  ([development](docs/development.md#devcontainer-and-wsl2)).

## Documentation

Guides, ADRs, and this plan. Context:
[ADR 0001](docs/adr/0001-documentation-structure.md) and the
[ADR process](docs/architecture.md#adr-process).

- [x] Reorganized setup/use, hardware, architecture, development, testing,
  operations, and security documentation; separated this backlog from completed
  work and removed unqualified compatibility/coverage claims (2026-09-28).
- [x] Restructured documentation by reader: lower-case guides in `docs/`, a
  short README, a security reference separate from the reporting policy, a
  data-model reference, eight ADRs in `docs/adr/`, and GitHub issue templates
  (2026-10-09; see [ADR 0001](docs/adr/0001-documentation-structure.md)).
- [x] Deepened every guide into a complete reference, added the
  [code quality](docs/code-quality.md) guide and ADRs 0009 to 0014 for
  decisions already in the code, simplified the README, and rebuilt this file as
  the complete work plan with done and open work side by side (2026-10-09).
- [x] Applied a second review pass across the guides: corrected facts against
  the source, configuration, and git history; added a "Verification Status"
  section to each ADR's Implementation; made the
  [data model](docs/data-model.md#error-tags-and-ui-messages) the one home for
  error tags and their causes, with the user-facing table in
  [features](docs/features.md#notices-and-errors) linking to it; and recorded
  the review's open findings in this plan (2026-10-09).
- [x] Stated the product's purpose and the rules every extension follows at the
  head of [Product Extensions](#product-extensions), grouped its open items by
  what they serve, added the open feature and decision items that follow from
  that purpose, and added the bridge behaviors that fall short of it (setup-key
  readiness, lock-key state on reconnect, peripheral-requested connection
  parameters, the reserved keyboard byte) to their sections; the user-visible
  limits they address are listed in
  [features](docs/features.md#current-technical-boundaries) (`TODO.md`,
  `docs/features.md`, `docs/architecture.md`, `docs/hardware.md`,
  `docs/security.md`; 2026-10-09).
- [x] Recorded the boot and reconnect, lock-key, connection-parameter, and
  reserved-byte fixes: [ADR 0015](docs/adr/0015-shared-reconnect-scan.md)
  supersedes the boot-reconnect part of ADR 0005,
  [ADR 0016](docs/adr/0016-bounded-peer-connection-parameters.md) amends
  ADR 0007, the lifecycles in [architecture](docs/architecture.md) and the
  behavior in [features](docs/features.md) follow the code, and
  [first flash](docs/first-flash.md) gained cold-start, lock-key, and
  connection-parameter checks (2026-10-09).
- [x] **P1** **Automated documentation checks.** Since 2026-10-10
  `scripts/check_docs.py` runs in the Linux host job and in `mask ci` and
  `mask docs-check`. It fails, with the file and line, on a broken link or
  anchor; a `config.rs` constant missing from, or disagreeing with, the
  [Configuration Defaults](docs/hardware.md#configuration-defaults) table; an
  inline `` `NAME` (value) `` mention that disagrees; a memory-map row, address
  range, or pairing page range that disagrees with the linker scripts; an
  unknown `mask` recipe, binary, or feature; and a code-formatted path that no
  longer exists. 27 unit tests cover each check, and a mutated copy of the
  repository (a changed table value, constant, flash length, page range,
  recipe, binary, feature, and path) failed on every change
  ([Markdown checks](docs/code-quality.md#markdown-checks);
  `scripts/check_docs.py`, `scripts/check_docs_test.py`,
  `.github/workflows/ci.yml`, `maskfile.md`).

## Product Extensions

bt2usb exists so that a Bluetooth LE keyboard and mouse behave like wired USB
ones. Plugged into a monitor's USB hub, the bridge is meant to move with the
monitor's built-in KVM from one computer to the next, work in BIOS/UEFI setup
screens and boot menus, and need no Bluetooth radio, driver, pairing, or
software on any host, which also suits work machines where Bluetooth is
disabled. None of this is hardware-verified yet. Every extension below serves
that purpose and follows these rules:

1. **The host sees standard USB HID.** Normal use needs no driver, app, or
   setting on any host, and a boot-protocol keyboard and mouse stay available
   for firmware setup screens and KVMs whatever else is added.
2. **The bridge holds the configuration.** Pairings, settings, and remaps are
   stored in the bridge, not on a host, so they follow the bridge from one host
   to the next behind the hub. The bridge's own controls can change every one
   of them. The keyboard may run bridge actions, but deleting data and
   approving a pairing stay on the bridge's own controls.
3. **No host changes the bridge unseen.** Several computers share the bridge
   through a KVM, so every change made from a host needs confirmation on the
   bridge itself. No host can read bond keys, peer addresses, or keystrokes,
   and no extension weakens the pairing, key-protection, or release gates
   above.
4. **Out of scope:** Bluetooth Classic peripherals (the nRF52840 radio is LE
   only), audio, and acting as a Bluetooth keyboard or mouse toward a host. Any
   role in which the bridge itself advertises or accepts connections, such as
   a firmware update over BLE, needs its own decision, because it adds attack
   surface the bridge does not have today.

None of these items is started. Each needs its decision first where an "ADR:"
item is listed. Context:
[features](docs/features.md#current-technical-boundaries).

### Shared Decisions

These decisions come first because several items below depend on them.

- [ ] **P2** **ADR: persistent device settings.** Decide where settings live:
  a second `sequential-storage` item beside `KEY_PAIRED_DEVICES` in the pairing
  pages, or separate flash pages, which changes the
  [memory layout](docs/hardware.md#memory-layout). A new record type inside
  the paired-device frame is not an option:
  [ADR 0006](docs/adr/0006-fail-closed-pairing-store.md) validates that whole
  frame and makes an unreadable pairing store refuse writes, so a corrupt
  setting would lock the pairing store. Cover versioning, defaults, and how a
  corrupt or unknown settings item falls back to defaults without touching bond
  records. The compatibility mode, settings menu, keyboard shortcuts, and key
  remapping items depend on this decision. Accept when the ADR is Accepted and
  listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
- [ ] **P2** **ADR: USB interface and report extensions.** Decide how System
  Control, an NKRO keyboard report, 16-bit mouse motion with a Resolution
  Multiplier, Battery Strength, the host management interface, and the
  compatibility configuration fit the composite device. Cover which interface
  carries each report and whether that adds report IDs to an interface that
  has none today; the endpoint packet sizes (each interrupt endpoint uses an
  8-byte maximum packet in `src/usb/hid_device.rs`, which a bitmap keyboard
  report or a 16-bit mouse report does not fit); the Feature `GET_REPORT` and
  `SET_REPORT` requests a Resolution Multiplier needs on the mouse interface,
  which rejects `SET_REPORT` today; whether a changed interface set needs its
  own product ID or device release number; and how the boot keyboard and
  mouse, whose report-protocol layouts start with the boot layout for hosts
  that skip `SET_PROTOCOL`, stay unchanged. Accept when the ADR is Accepted and
  listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).

### Hand-Off Between Hosts And KVMs

- [ ] **P2** **ADR: multiple BLE profile sets.** Decide profile selection,
  storage layout and migration, and which slots a profile owns. Accept when the
  ADR is Accepted and listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
- [ ] **P2** **Multiple BLE profile sets.** *(hardware)* Design selection,
  storage, migration, and connection ownership. Accept when switching profiles
  releases old inputs and only reconnects the selected profile's authorized
  peers.
- [ ] **P2** **Monitor-input-aware switching.** *(hardware)* Identify an
  explicit supported signal from the monitor/host, define fallback behavior, and
  prototype against named hardware. Accept when input reaches the intended PC
  without leaking held keys during a switch.
- [ ] **P2** **Connect any saved device that is present.** Only the two most
  recently added saved devices reconnect at boot
  (`store.iter_recent().take(MAX_CONNECTIONS)` in `ble_task`,
  `src/ble/multi_conn.rs`), and a slot whose link drops stays reserved for
  that device (`on_slot_link_lost` in `src/ble/coordinator.rs`). A third or
  fourth saved device, such as a second keyboard kept at another desk,
  therefore connects only through a scan and a selection on the bridge, and
  that scan first disconnects both slots when both are in use
  (`plan_start_scan`). Let a saved device that advertises take a free slot, or
  a slot whose device is still retrying, and decide when a retrying device
  gives up its slot. Accept when reconnect-table and coordinator tests cover
  four saved devices with any two present, a saved device that wakes while its
  slot is held by a retrying one, and an unchanged result when only the two
  most recent are present ([features](docs/features.md#boot-and-reconnect)).
- [ ] **P2** **KVM and firmware-setup compatibility mode.** *(hardware)* Some
  KVM switches emulate the keyboard and mouse instead of passing USB through,
  and some firmware setup screens may handle only simple boot devices; either
  can mishandle a composite device with a consumer-control interface. Add a
  stored setting (see "ADR: persistent device settings" above) that enumerates
  with only the boot keyboard and boot mouse interfaces, or with the boot
  keyboard alone, and restarts the bridge so it re-enumerates when the setting
  changes; `hid_device::init` builds the descriptors once at start-up. Start
  only when the
  [hardware compatibility baseline](#board-bring-up-and-hardware-acceptance)
  records a KVM or firmware setup screen that fails with the default
  configuration, and promote this item to P1 then. Accept when that target
  works with the compatibility configuration, both configurations are recorded
  on the baseline's KVMs and firmware setup screens, and a change of
  configuration releases every held input
  ([features](docs/features.md#usb-hid-device)).

### Control From The Bridge

The bridge usually sits behind a monitor, so every setting must be reachable
from its own controls, and frequent actions also from the keyboard.

- [ ] **P2** **Settings menu on the bridge.** *(hardware)* Add a Settings
  screen with the display timeout (today the build-time
  `SCREEN_AUTO_OFF_TIMEOUT_SECS`, 120 s), display rotation for a bridge
  mounted upside down (`src/ui/display.rs` fixes `DisplayRotation::Rotate0`),
  contrast, and the switches other items add, such as the compatibility mode
  and keyboard shortcuts. Which saved devices reconnect belongs to "Multiple
  BLE profile sets" and "Connect any saved device that is present". Accept when
  reducer tests cover each setting, a corrupt settings item restores defaults
  while saved devices still reconnect, and settings survive a reboot on a board
  ([features](docs/features.md#screens-and-buttons)).
- [ ] **P2** **Link status screen.** *(hardware)* The Connected screen shows one
  device name or `2 devices`. Add a status view that lists each slot's device,
  whether it delivers keyboard, mouse, or consumer input, and its live signal
  strength, so the bridge can be placed where a monitor stand does not block
  the radio; the stored `last_rssi` is a scan-time hint only. The view also
  shows the firmware version from "Diagnostics without sensitive input" and the
  state that "Visible storage/security errors" defines for a slot that keeps
  retrying. Accept when reducer tests cover connected, retrying, and empty
  slots and a board shows the signal strength change as a peripheral moves
  ([features](docs/features.md#screens-and-buttons)).
- [ ] **P2** **Peripheral battery level.** *(hardware)* Discover the Battery
  Service (`0x180F`) and subscribe to Battery Level (`0x2A19`) when a
  peripheral offers it, after HID input is flowing so the first keystroke is
  never delayed. Show each device's level in the link status view, with one
  low-battery notice that does not interrupt input. Today HID discovery ignores
  the Battery Service; its UUID appears in `src/` only in advertisement-parser
  test data. Accept when host tests cover level parsing and the low-battery
  threshold, a peripheral without the service connects as before, and a real
  keyboard's level is shown and updates
  ([features](docs/features.md#hid-discovery-and-report-maps)).
- [ ] **P2** **Keyboard shortcuts for bridge actions.** Reserve a configurable
  key chord, recognized per source before aggregation, that runs bridge
  actions from the keyboard: open the status view, switch profile set, and
  wake the display. The key that completes the chord never reaches the host,
  chord keys already sent are released before the action runs, and the same
  keys typed without the chord are unaffected. A shortcut never starts a scan
  that would disconnect the keyboard that typed it (`plan_start_scan` in
  `src/ble/coordinator.rs` disconnects both slots when both are in use), and
  never confirms a Forget, a reset, or a pairing, which stay on the bridge's
  own controls. Shortcuts stay off until enabled on the bridge, so no key
  combination is taken from the host unasked. Accept when host tests prove
  those properties for both slots
  ([features](docs/features.md#two-source-aggregation)).
- [ ] **P2** **Key remapping and input adjustments.** *(hardware)* Remap keys
  per saved device (for example Caps Lock to Control), invert scrolling, and
  scale pointer speed inside the bridge, so the change follows the bridge to
  every host behind the hub and into firmware setup screens with no host
  software. Apply remapping per source before aggregation, so six-key rollover,
  new-press-only wake, and release on disconnect see the remapped keys. A remap
  meant for one host only, such as Command and Option swapped for a PC but not
  for a Mac on the same KVM, needs the active host from "Monitor-input-aware
  switching" and is not part of this item. Accept when host tests cover
  remapped press and release pairs on both slots, modifiers, and a remap whose
  target key the other slot already holds, and a remap survives a reboot on a
  board ([features](docs/features.md#two-source-aggregation)).

### Input Fidelity

The host should get everything a keyboard or mouse would give it when plugged
in by cable or paired directly.

- [ ] **P2** **Power, sleep, and wake keys.** *(hardware)* Keyboards with power
  or sleep keys often report them in a Generic Desktop System Control
  collection (usage `0x80`: System Power Down `0x81`, System Sleep `0x82`,
  System Wake Up `0x83`). `src/hid/report_protocol.rs` classifies only
  keyboard, mouse, and consumer reports and the USB device has no System
  Control report, so these keys are dropped. Power and sleep keys that a
  keyboard sends on the Consumer page already reach the host through the
  consumer-control interface. Add the report kind, a bounded translation, and a
  USB System Control report on the interface that "ADR: USB interface and
  report extensions" chooses, with the same release-on-disconnect and
  new-press-only wake rules as the other endpoints. Accept when descriptor
  fixtures and aggregation tests cover press and release, and a real keyboard's
  sleep key suspends a named host through the bridge
  ([features](docs/features.md#translation)).
- [ ] **P2** **More than six keys on the USB keyboard.** *(hardware)* The USB
  keyboard report carries six key codes, so more than six distinct keys held
  across both keyboards become the rollover report of six `0x01` codes. Add a
  bitmap (NKRO) keyboard report for report protocol while keeping the six-key
  boot report. The USB extensions ADR decides between a separate NKRO
  interface and the boot keyboard's own report protocol, with the firmware
  setup screens and KVMs that skip `SET_PROTOCOL` in mind, and the larger
  endpoint packet a bitmap report needs. NKRO input from BLE keyboards comes
  from descriptor-driven translation
  ([HID Report Parsing And Translation](#hid-report-parsing-and-translation)).
  Accept when aggregation tests cover more than six held keys in report
  protocol and the rollover report in boot protocol, and a named host registers
  more than six simultaneous keys through the bridge
  ([data model](docs/data-model.md#usb-hid-report-contracts)).
- [ ] **P2** **High-resolution mouse movement and scrolling.** *(hardware)* USB
  mouse X, Y, wheel, and pan are signed 8-bit values and the descriptor has no
  Resolution Multiplier, so the host gets only standard wheel steps and
  coalesced motion saturates at the signed 8-bit range (-128 to 127) per
  report. Add 16-bit movement and high-resolution wheel and pan (a Resolution
  Multiplier feature report) for report protocol, leaving the three-byte boot
  report unchanged, on the interface and packet size the USB extensions ADR
  chooses. The Resolution Multiplier needs Feature `GET_REPORT` and
  `SET_REPORT` on the mouse interface, which rejects `SET_REPORT` today
  (`set_report` in `src/usb/host_requests.rs`), so this item follows "HID/USB
  conformance" and updates the
  [USB host interface](docs/security.md#usb-host-interface) table. This also
  carries the 12- and 16-bit motion that descriptor-driven translation will
  decode from BLE mice. Accept when host tests show motion beyond the signed
  8-bit range per report delivered without saturation, and a named host scrolls
  in high-resolution steps through the bridge
  ([data model](docs/data-model.md#usb-hid-report-contracts)).
- [ ] **P2** **Peripheral battery level reported to the host.** *(hardware)* A
  host paired over Bluetooth shows each keyboard's and mouse's battery level;
  through the bridge it sees none. Report the level from "Peripheral battery
  level" above as a HID Battery Strength usage (Generic Device Controls page
  `0x06`, usage `0x20`) on an interface that the USB extensions ADR chooses,
  never on the boot keyboard or mouse interface. Hosts that read HID battery
  reports, such as Linux, show it; others may not. Accept when descriptor tests
  cover the report and a Linux host shows a real peripheral's level
  ([features](docs/features.md#usb-hid-device)).
- [ ] **P2** **Per-peripheral quirks.** Key a bounded, data-driven quirk table
  on the Device Information Service PnP ID (characteristic `0x2A50`: vendor ID
  source, vendor ID, product ID, and version) for peripherals whose Report Map
  misdescribes their reports, so that descriptor-driven translation alone
  cannot handle them. Read the PnP ID after HID input is flowing, so the first
  keystroke is never delayed. Accept when host tests cover quirk lookup and
  default behavior for an unknown peripheral, and each quirk names the
  hardware-result issue that justifies it
  ([features](docs/features.md#translation)).
- [ ] **P2** **ADR: HID passthrough for other device classes.** Decide whether
  a BLE HID peripheral whose reports the fixed keyboard, mouse, and consumer
  translation cannot represent (for example a touchpad or a vendor-defined
  control) gets its own USB interface that mirrors its Report Map. A mirrored
  map hands a peer-chosen descriptor and peer-chosen reports to the host, so
  decide which usages a mirrored interface may carry (no keyboard, keypad,
  mouse, or system-control usages, which keep going through the bounded
  translation and release rules), and add the case to the
  [threat model](docs/security.md#threat-model). Cover how many such
  interfaces exist, how their descriptors stay fixed at enumeration while
  peripherals come and go, where Report Maps of up to 512 bytes are stored, how
  they are validated, and how the boot keyboard and mouse stay unaffected.
  Accept when the ADR is Accepted and listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
- [ ] **P2** **HID passthrough for other device classes.** *(hardware)*
  Implement the ADR above. Accept when a mirrored peripheral works on a named
  host, a malformed or oversized Report Map is rejected before enumeration, and
  the boot keyboard and mouse still pass the
  [first-flash](docs/first-flash.md#5-in-the-monitor) monitor and firmware setup
  checks.

### More Peripherals And Form Factors

- [ ] **P2** **ADR: more than two simultaneous peripherals.** Decide the link
  count and what it costs in SoftDevice RAM (`BLE_MAX_CONNECTIONS` in
  `src/config.rs` sizes every per-link array and the SoftDevice counts, so the
  code follows it; the host tests assume two links), aggregation sources and
  consumer priority, endpoint fairness, the saved-device capacity
  (`MAX_PAIRED_DEVICES` is 4, and all records share one item of at most 512
  bytes, `MAX_RECORD_SIZE`, which fits at most five full records), and the UI.
  The decision would supersede
  [ADR 0005](docs/adr/0005-two-slots-and-independent-endpoints.md) in part and
  builds on the single link-count constant (done under
  [Verification And Code Quality](#verification-and-code-quality)). Accept when
  the ADR is Accepted and listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
- [ ] **P2** **More simultaneous peripherals and saved devices.** *(hardware)*
  Support a third device at once, such as a numeric keypad or presenter remote
  beside the keyboard and mouse, and more saved devices. Accept when three
  links deliver input at once on a board with recorded SoftDevice RAM and stack
  margins, the [memory and endurance budget](#platform-memory-and-recovery)
  covers the new link count, and existing pairing stores migrate to the new
  capacity ([data model](docs/data-model.md#schema-change-rules)).
- [ ] **P2** **Display-less plug-in dongle.** *(hardware)* A bridge that plugs
  straight into the monitor's USB port needs no cable or case. Build a variant
  for a plug-in nRF52840 USB dongle, such as Nordic's nRF52840 Dongle, with an
  LED for status, one button for the actions that stay on the bridge's own
  controls, and keyboard shortcuts that a long press of that button enables.
  Nordic's dongle ships with a USB bootloader that starts at `0xE0000`, inside
  today's application range and below the pairing pages (`0xF0000` to
  `0xF3FFF`), so the variant needs its own flash map and a decision whether
  firmware is flashed through that bootloader or the board is erased over SWD,
  under "ADR: bootloader, flash partitioning, and signed DFU" below. If
  "Production hardware definition" chooses this variant, that item's
  acceptance applies to it. Accept when the authenticated pairing ADR covers a
  display-less pairing flow, the variant has its own build, pin map, and linker
  script with the same assertions as `memory_sd.x`, and it has a passing
  first-flash record adapted to it
  ([hardware](docs/hardware.md#porting-to-another-nrf52840-board)).
- [ ] **P2** **Additional MCU/board targets.** *(hardware)* Select a concrete
  target, isolate board configuration, and implement its radio/USB/storage
  integration. Accept when it has a maintained build and hardware acceptance
  record; alternatives listed in
  [hardware](docs/hardware.md#possible-future-ports) are not supported ports
  today.

### Updates And Host Tools

- [ ] **P2** **ADR: bootloader, flash partitioning, and signed DFU.** Choose a
  bootloader, the flash partition layout (no bootloader or DFU region is
  allocated today), image signing, secure boot, and anti-rollback. Accept when
  the ADR is Accepted and listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
- [ ] **P2** **Signed USB/BLE DFU.** *(hardware)* Choose a bootloader and flash
  partition layout; implement signed image validation, rollback policy,
  interrupted-update recovery, and physical recovery. Entering update mode
  needs a press on the bridge's own button, so no host behind a KVM can start
  an update unseen. An update over BLE would give the bridge an advertising,
  connectable role that it does not have today
  ([security](docs/security.md#security-posture-summary)), so the ADR above
  decides it separately. Accept only after power-cut and invalid-image tests
  ([security](docs/security.md#firmware-integrity-and-updates)).
- [ ] **P2** **ADR: host management interface.** Decide whether the bridge
  offers a management channel to the host, such as a vendor-defined HID
  interface, its versioned protocol, what it may read (status, diagnostics,
  settings) and change, and how each change is confirmed on the bridge itself.
  Every host behind a KVM shares the bridge, so no host may change pairings or
  settings unseen, and the protocol never exposes bond keys, peer addresses,
  or keystrokes. The interface is left out of the compatibility
  configuration, so a KVM or firmware setup screen sees only the boot keyboard
  and mouse. Today the bridge has no such interface, which the
  [threat model](docs/security.md#threat-model) relies on. Accept when the ADR
  is Accepted and listed in the
  [architecture index](docs/architecture.md#decisions-needed-for-roadmap-work).
- [ ] **P2** **Browser configuration page.** *(hardware)* Publish, with each
  release, a static web page that reads status and diagnostics and changes
  settings through WebHID, so nothing is installed on the host. WebHID blocks
  reports in keyboard and mouse top-level collections and is available only in
  Chromium-based browsers, so the page talks to the vendor-defined interface
  from the host management ADR. Accept when the page works in a named
  Chromium-based browser, every change waits for confirmation on the bridge,
  and each page release states the firmware versions it supports.
- [ ] **P2** **Windows/macOS companion app.** *(hardware)* The app is an
  optional client for hosts without a WebHID browser, and normal use never
  needs it. Build it on the host management interface above, after the browser
  configuration page, and define its installation and update policy before
  implementing the tray UI. Accept when it performs every operation the
  browser configuration page does against the same firmware versions, and
  every change waits for confirmation on the bridge.

## How This Plan Is Worked

Items are taken one at a time with the owner's coding-agent loop
([CODING_AGENT.md](https://github.com/chefzaid/agents/blob/main/CODING_AGENT.md)):

1. Pick an open [FIXME](#fixme) first; otherwise the simplest open item that
   needs neither a board nor a decision from the owner. Items tagged
   *(hardware)* wait for a board, and items listed under
   [Needs Your Input](#needs-your-input) wait for an answer.
2. Plan it, implement it, and pass it through the checklist below.
3. Check it here with the implementing paths, update the guide that owns the
   behavior, and commit it on its own with a short message.
4. Record any defect found on the way under [FIXME](#fixme), and any question
   for the owner under [Needs Your Input](#needs-your-input), then repeat.

The checklist, adapted to this firmware:

| Phase | What it requires here |
| --- | --- |
| Implementation | Fully implemented, no dead code, edge cases handled, no stand-in values. The OLED UI is English only, so localization does not apply; any user-visible state is reachable from the three buttons |
| Decomposition | No modified or new file over 500 lines; readable control flow; decisions in hardware-free modules ([ADR 0003](docs/adr/0003-pure-core-and-task-shell.md)) |
| Reuse | No duplicated logic; existing helpers and constants are reused |
| Static analysis | `cargo fmt`, and host, firmware, and simulation Clippy with `-D warnings`, rustdoc with warnings denied |
| Security | Every peer-, host-, or flash-controlled value bounded and validated; no key material or keystrokes logged; no unreviewed dependency |
| Robustness | No lost wakeups, unbounded waits, or panics reachable from outside input; bounded work in callbacks |
| Tests | Host tests for the new behavior; host-library line coverage above 85% (`cargo llvm-cov --locked --lib --tests`; 98.2% on 2026-10-10); the Renode scenario stands in for browser end-to-end tests |
| Documentation | The owning guide, ADRs, and this plan match the code; dependencies pinned and current |

## Updating This Checklist

- Check an item only when its acceptance criterion is met with evidence. Add
  the implementing paths to the item when you check it.
- Record the evidence in the [testing guide](docs/testing.md) validation record
  or in a [hardware-result issue](.github/ISSUE_TEMPLATE/hardware-result.md),
  and name the commit and artifact hash it covers. Record skipped tests as
  skipped, not passed.
- Describe a new capability in [features](docs/features.md), and update the
  guide that owns its details.
- Add an ADR, or supersede one, when a decision changed; see the
  [ADR process](docs/architecture.md#adr-process).
- A hardware gate stays unchecked until board evidence exists, even when the
  supporting code is merged. When part of an item is done, split it: check the
  finished part and leave the rest open with its own acceptance criterion.
- Remove *(hardware evidence pending)* from a done item only when a recorded
  first-flash or acceptance run covers it.
