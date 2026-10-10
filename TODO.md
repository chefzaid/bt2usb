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
| [FIXME](#fixme) | 19 | 1 | 0 |
| [BLE Central And Pairing](#ble-central-and-pairing) | 14 | 6 | 3 |
| [HID Report Parsing And Translation](#hid-report-parsing-and-translation) | 4 | 2 | 0 |
| [USB HID Device](#usb-hid-device) | 4 | 4 | 2 |
| [Input Aggregation And Delivery](#input-aggregation-and-delivery) | 3 | 2 | 2 |
| [Pairing Storage](#pairing-storage) | 4 | 5 | 3 |
| [UI, Display And Power](#ui-display-and-power) | 9 | 2 | 1 |
| [Platform, Memory And Recovery](#platform-memory-and-recovery) | 7 | 5 | 3 |
| [Device Security And Provisioning](#device-security-and-provisioning) | 1 | 3 | 2 |
| [Board Bring-Up And Hardware Acceptance](#board-bring-up-and-hardware-acceptance) | 2 | 5 | 3 |
| [Verification And Code Quality](#verification-and-code-quality) | 7 | 9 | 0 |
| [Release, Provenance And Supply Chain](#release-provenance-and-supply-chain) | 7 | 9 | 3 |
| [Developer Experience](#developer-experience) | 8 | 1 | 0 |
| [Documentation](#documentation) | 6 | 1 | 0 |
| [Product Extensions](#product-extensions) | 0 | 28 | 0 |
| **Total** | **95** | **83** | **22** |

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
- [ ] **P2** **Source files over 500 lines grew.** `4faf99f` added lines to
  `src/ble/multi_conn.rs` (845 now), `src/usb/hid_device.rs` (536), and pushed
  `src/hid_descriptor_tests.rs` past the limit (551, split to 444 since);
  `src/lib_tests.rs` is 503. Close with the split that
  [Keep source files within a size limit](#verification-and-code-quality)
  asks for.
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

## Needs Your Input

Decisions and actions only the project owner can take. The loop skips each
item listed here and moves on; answer in the item's row (or in the thread) and
it becomes workable again.

| Item | What is needed | Options (recommendation first) |
| --- | --- | --- |
| [Security maintenance ownership](#release-provenance-and-supply-chain) (P0) | A private reporting channel, which versions get fixes, who triages, and how fast reporters hear back. SECURITY.md cannot name a channel until one exists | 1. Enable GitHub private vulnerability reporting as the only channel; fix only the latest release tag and `main`; you triage; acknowledge within 7 days and give a fix or plan within 30 days. 2. Publish a security email address instead, with the same policy |
| [Hardware-evidence label and private reporting](#release-provenance-and-supply-chain) (P2) | Two repository settings no tool here can change: create the `hardware-evidence` label (Issues, Labels, New label) that the hardware-result template applies, and turn on private vulnerability reporting (Settings, Code security) | 1. Do both; the loop then updates SECURITY.md and checks a new hardware-result issue. 2. Create only the label and choose an email channel in the row above |

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
  behind a reconnect (`src/ble/mod.rs` `GAP_PROCEDURE`, `src/ble/multi_conn.rs`,
  `src/config.rs`). *(hardware evidence pending)*
- [x] Stored pairing/bond records, boot reconnect, identity-key matching, and
  retries after link loss; a lost or not-yet-seen paired device is retried,
  with a `BLE_RECONNECT_BACKOFF_MS` pause between attempts, while its slot
  stays reserved (`src/storage.rs`, `src/ble/reconnect.rs`,
  `src/ble/multi_conn.rs`).
  *(hardware evidence pending)*
- [x] Peer-identity-scoped bond replacement and key lookup, stable identity
  persistence, and private-address resolution on background reconnect
  (`src/ble/multi_conn.rs`, `src/ble/scanner.rs`, `src/storage.rs`).
  *(hardware evidence pending)*
- [x] Require encrypted links before HID discovery, restrict
  application-initiated fresh pairing to explicit user connection attempts, and
  handle cancellation during owned-link security/discovery
  (`src/ble/multi_conn.rs`). *(hardware evidence pending)*
- [x] Preserve UTF-8 advertising names, merge scan-response names, and release
  the radio procedure lock before delivering UI scan results (`src/ble/`).
  *(hardware evidence pending)*
- [x] Discover a BLE keyboard's LED output report during HID discovery and write
  each host LED change to it from the slot that owns the link
  (`src/ble/hid_client.rs` `write_leds`, `src/ble/multi_conn.rs`; the USB side
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
  `src/ble/scanner.rs`, `src/ble/multi_conn.rs`, `src/config.rs`,
  `src/ui/ui_logic.rs`; [ADR 0015](docs/adr/0015-shared-reconnect-scan.md)).
  Host tests cover the table: handover, single use, expiry, clearing, the
  fast window across retries, the tie-break, the holdoff after a failed
  attempt, wakes, and saved-device identity. *(hardware evidence pending)*
- [x] Write the host's current lock-key state to a keyboard as soon as its link
  starts, then every change. A keyboard that wakes and reconnects, or connects
  to a slot that already passed the last change to an earlier link, now shows
  the host's Caps Lock and Num Lock state at once, as a wired keyboard does
  when plugged in (`forward_host_leds` in `src/hid/host_leds.rs`, the
  `HostLeds` implementation for `LedReceiver` in `src/usb/hid_device.rs`,
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
  `Bonder::conn_param_update_request` in `src/ble/multi_conn.rs`, and the
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
- [ ] **P0** **Authenticated pairing and enrollment policy.** *(hardware)*
  Implement the ADR above: define whether each supported device uses
  authenticated pairing, how user presence is checked, and whether weaker
  devices are rejected; add a bounded pairing window and visible state.
  Require LE Secure Connections and a 16-byte minimum encryption key size.
  Today `Bonder` in `src/ble/multi_conn.rs` does not override
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
  peripherals with long maps and different MTUs. Decide whether the absent-map
  compatibility fallback remains allowed for deployment. Accept when captures
  demonstrate full reads and error handling, and same-length incompatible
  layouts are rejected by the supported descriptor-driven translation policy
  ([architecture](docs/architecture.md#hid-path-and-limits)).
- [ ] **P1** **Scan list under crowding.** The scan keeps the first
  `BLE_MAX_DISCOVERED` (8) HID advertisers it hears, so a crowded room, or
  deliberate fake advertisers, can hide the intended device. Decide how the
  bounded list chooses entries (for example by signal strength or by letting
  the user rescan with a name filter). Accept when host tests show the intended
  device is listed with more than eight HID advertisers present
  ([security](docs/security.md#threat-model)).
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
  (`src/usb/hid_device.rs`). *(hardware evidence pending)*
- [x] USB boot/report protocol negotiation, three-byte boot mouse
  serialization, keyboard LED control-request validation, reset state cleanup,
  and stable factory-derived per-unit USB serials (`src/usb/hid_device.rs`,
  `src/hid/mouse.rs`). *(hardware evidence pending)*
- [x] Publish the host's keyboard LED output report through a `Watch` that both
  BLE slots observe, and clear it on USB reset (`src/usb/hid_device.rs`
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
  a key repeating on the host (`src/ble/multi_conn.rs`, `src/hid/aggregate.rs`).
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
  supported upgrade/downgrade paths. The legacy parser and the identity merge in
  `DeviceStore` live in `src/storage.rs`, which the host crate does not compile,
  so they have no host tests today (see "Host tests for the device store" under
  [Verification And Code Quality](#verification-and-code-quality)). Accept
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
  action (`src/ui/ui_logic.rs` `ManagementRequests`, `src/main.rs`,
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
  `Connect failed` (`ble_error_message` in `src/main.rs`). Three cases do not
  reach the user at all. A newly paired device whose identity address matches
  a stored peer replaces that peer's record and bond (`DeviceStore::add` in
  `src/storage.rs`, `Bonder::on_bonded` in `src/ble/multi_conn.rs`), leaving
  only the `Updated existing paired device` log line. A full store evicts its
  oldest peer with only the
  `Paired device store full - evicting oldest entry` log line. A background
  reconnect whose link cannot be secured, because the peer lost its keys or
  the store has none for it (a legacy record carries no bond), fails with
  `ConnectFailed`, which a silent reconnect treats as "try again"
  (`connection_slot_task` in `src/ble/multi_conn.rs`); the slot retries after
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
- [ ] **P1** **Vendored debug and trace logs.** Local builds log at `debug`
  (`.cargo/config.toml`), where the vendored `nrf-softdevice` records each peer
  address (`connected role={:?} peer_addr={:?}` in
  `vendor/nrf-softdevice/src/ble/central.rs`), and at `trace` it logs raw
  notification bytes, which are keystrokes
  (`GATT_HVX write handle={:?} type={:?} data={:?}` in
  `vendor/nrf-softdevice/src/ble/gatt_client.rs`).
  Accept when the default local build records no peer addresses, notification
  data can be logged only through an explicit, documented opt-in, and the debug
  output of `embassy-usb`, `embassy-nrf`, and `sequential-storage` has been
  reviewed ([security](docs/security.md#logging-and-privacy)).

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
  `src/storage/codec.rs`; commit `e3bc620`). Four files are over 500 lines
  again; see "Keep source files within a size limit" below.
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
- [ ] **P1** **Host tests for the I/O shells.** The connection workers, GATT
  HID client, USB device, and display driver (`src/ble/multi_conn.rs`,
  `src/ble/hid_client.rs`, `src/usb/hid_device.rs`, `src/ui/display.rs`) have
  no host tests; only the pure modules they call do. The storage shell is the
  next item. Move remaining decisions into hardware-free modules or test the
  shells against fakes. Accept when each has host tests for its error paths, or
  the testing guide records why it cannot
  ([testing](docs/testing.md#modules-without-host-tests)).
- [ ] **P1** **Host tests for the device store.** `DeviceStore` in
  `src/storage.rs` loads, merges by identity address, evicts the oldest record
  when full, forgets, and factory-resets the pairing store, and it holds the
  legacy-format parser (`deserialize_legacy`); the byte codec is in
  `src/storage/codec.rs`. The host crate compiles only `storage/framing.rs`
  and `storage/record.rs`, so none of this has a host test. Move the in-memory
  logic and the codec into host-compiled modules, or test them against a fake
  flash. Accept when host tests cover load of valid, legacy, malformed, and
  unreadable stores, identity merge and bond replacement, eviction at
  `MAX_PAIRED_DEVICES`, forget, factory reset, and a codec round trip
  ([testing](docs/testing.md#modules-without-host-tests)).
- [ ] **P1** **Broaden the Renode scenarios.** The Robot test runs one scripted
  scenario; it does not exercise the OLED task or the saved-device management
  screens. It also does not exercise link loss: step 2 of the scenario logs
  `scenario: slot 0 link lost` but calls `coordinator::on_slot_disconnected`,
  which frees the slot, instead of `coordinator::on_slot_link_lost`, which keeps
  it reserved for reconnection (`src/sim.rs` `scenario_step`). Make step 2 call
  `on_slot_link_lost` and add scenarios for the other two paths. Accept when
  `mask sim-test` and the CI simulation job assert the reserved slot after a
  link loss, the OLED task, and the management screens through UART output
  ([testing](docs/testing.md#renode-scenario-map)).
- [ ] **P1** **Coverage and firmware documentation in CI.** CI measures no
  coverage and builds rustdoc only for the host library. Publish the
  `cargo llvm-cov` report as a CI artifact, set a threshold once a baseline is
  recorded, and build firmware rustdoc with warnings denied. Accept when a
  coverage drop below the threshold or a firmware rustdoc warning fails CI
  ([code quality](docs/code-quality.md)).
- [ ] **P2** **Inventory panic sites in firmware paths.** The
  [panic table](docs/code-quality.md#panics-allocation-and-arithmetic) lists
  the application's `unwrap!`, `expect`, and `unreachable!` sites, but not slice
  indexing, `RefCell` borrows, or `StaticCell` initialization, and not the
  vendored crate. The compiled vendored modules panic on an unexpected
  SoftDevice event (`panic!("unexpected event {}", e)`, four times in
  `vendor/nrf-softdevice/src/ble/gatt_client.rs` and once in `central.rs`).
  Accept when every `unwrap!`, `expect`, `unreachable!`, and `panic!` reachable
  from the bridge, application and vendored, is listed with the reason it
  cannot fire, or is replaced by an error path
  ([code quality](docs/code-quality.md#panics-allocation-and-arithmetic)).
- [ ] **P2** **Lint the release helper and shell scripts.** CI runs no Python
  or shell linter: CI runs `scripts/release.py` and its unit tests in
  `scripts/release_test.py` but does not lint them, and
  `scripts/install-renode.sh`,
  `scripts/run-tool.sh`, `.devcontainer/post-create.sh`, and the 31 Bash
  recipes in `maskfile.md` are not checked at all. Add Ruff (or an equivalent)
  for the Python files and ShellCheck for the scripts and the extracted
  `maskfile.md` recipes. Accept when a lint finding in any of them fails CI
  ([code quality](docs/code-quality.md#other-files)).
- [ ] **P2** **Keep source files within a size limit.** Three files are over
  500 lines again after the split in commit `e3bc620` (`wc -l` on 2026-10-10:
  `src/ble/multi_conn.rs` 844, `src/usb/hid_device.rs` 536,
  `src/lib_tests.rs` 503; `src/ui/ui_logic.rs` dropped to 446 when its tests
  moved to `ui_logic_tests.rs`, and `src/hid_descriptor_tests.rs` from 551 to
  444 when its keyboard-report tests moved out), and no tool limits file
  length. Accept when each is split below the limit, or a recorded limit with
  named exceptions is checked in CI
  ([code quality](docs/code-quality.md#known-gaps)).

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
  draft. Action pin comments name the exact upstream tag each SHA resolves to,
  except `Swatinem/rust-cache` and `taiki-e/install-action`, whose comments name
  only the moving major tag `v2` (open under CI runtime maintenance below)
  (`.github/workflows/ci.yml`,
  [deployment](docs/deployment.md#re-running-a-tag-workflow)).
- [ ] **P0** **Hosted provenance and release recovery acceptance.**
  *(hardware)* Push the first release tag (none exists yet), run the configured
  tag/attestation workflow, verify its downloaded artifacts against the approved
  commit from a clean machine, and document storage migrations, rollback
  constraints, and service flashing. Accept when provenance verification and
  failed-update recovery have evidence; local helper tests and workflow lint do
  not exercise GitHub signing or device recovery
  ([deployment](docs/deployment.md#verify-before-flashing)).
- [ ] **P0** **Release notes for deployment releases.** The release job uses
  generated notes only. Add a release-notes template and fill it for each
  deployment draft. Accept when a draft's notes list supported versions,
  compatibility limits, migrations, rollback constraints, checksums, and the
  exact SoftDevice prerequisite
  ([release gates](docs/deployment.md#release-gates)).
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
  and adopt maintained replacements through dependency upgrades; CI runs
  `cargo audit` without a deny option, so these warnings do not fail the job
  today. Accept when the lockfile no longer selects these affected versions,
  the audit is clean without advisory suppression, the CI audit fails on
  unmaintained-crate warnings, and host/firmware/simulation regression checks
  pass ([testing](docs/testing.md#validation-record--2026-09-28)).
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
- [ ] **P1** **CI runtime maintenance.** The 2026-10-09 run reports that the
  pinned `actions/checkout` v4.4.0 and `actions/upload-artifact` v4.6.2 target
  the deprecated Node.js 20 runtime, and that `ubuntu-latest` moves to Ubuntu 26
  from 2026-10-19. Move to maintained action releases (Dependabot pull
  requests #4 and #5 were closed unmerged) and pin or validate the runner
  image. Replace the `# v2` comments on the `Swatinem/rust-cache` and
  `taiki-e/install-action` pins with the exact release each SHA resolves to.
  On 2026-10-09 `git ls-remote` showed the `install-action` SHA is tag
  `v2.87.21`, and the `rust-cache` SHA is the annotated `v2` tag object, whose
  commit `6323deb` is tag `v2.9.2`; pin that commit instead of the tag object.
  Accept when a run shows no deprecation annotations and every pin is a commit
  SHA whose comment names its exact tag
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
  SoftDevice, and probe tasks as 31 Bash `mask` recipes; `mask ci` runs the
  local formatting, lint, test, and build subset (`maskfile.md`).
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
- [ ] **P1** **Automated documentation checks.** Validate local links, command
  examples, and configuration/memory-map consistency in CI. Accept when a broken
  link or stale documented constant produces a targeted failure
  ([testing](docs/testing.md#known-verification-gaps)).

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
  (`set_report` in `src/usb/hid_device.rs`), so this item follows "HID/USB
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
| Tests | Host tests for the new behavior; host-library line coverage above 85% (`cargo llvm-cov --locked --lib --tests`; 97.5% on 2026-10-10); the Renode scenario stands in for browser end-to-end tests |
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
