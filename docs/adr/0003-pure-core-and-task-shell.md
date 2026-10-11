# ADR 0003: Keep Decisions In Hardware-Free Modules And I/O In Thin Tasks

- Status: Accepted
- Date: 2026-06-22

## Context

The defects that matter most to a bt2usb user come from decisions, not from
register access:

- a key or mouse button that stays pressed on the host after a link drops
- the wrong screen, or a button press that triggers the wrong action
- a bond lost, or a corrupted record accepted, when the store is loaded
- a malformed report or Report Map from a peripheral accepted as valid input
- a delayed reply completing a newer saved-device action

That logic is hard to exercise on a board, and CI has no board. The two ends of
the data path cannot be emulated either: the header of
[sim.rs](../../src/sim.rs) notes that the SoftDevice is a closed binary tied to
the radio and that emulators do not model the nRF USBD peripheral.

Host tests have existed since the embedded implementation landed on 2026-02-21
(`8e6dd17`), but at first only partly against shipped code. That first
`src/lib.rs` (764 lines, most of them tests) described itself as "a separate
entry point for host-based testing". It already included the firmware's
keyboard, mouse, consumer, advertisement-parser, power-logic, and input-logic
files through `#[path]`, but it re-declared the `HidReport` enum with the
comment "matches the embedded version" together with its own report
classification functions. Classification tests could therefore pass against
code that did not ship. By the time of the fix the file had grown to 815
lines.

The decision recorded here was implemented on 2026-06-22:

- `9c3568f` extracted the connection-slot state machine and its command and
  event reducers into `src/ble/coordinator.rs`, and the UI screen transitions
  into `src/ui/ui_logic.rs`. Both describe themselves as the "functional core"
  whose "imperative shell" is the task code.
- `e3bc620` removed the duplicate ("Unify HID classification on the real hid
  module (drop lib.rs duplicate + dead hid/tests.rs)"). `src/lib.rs` became a
  list of `#[path]` includes of the firmware's own files and is now 166 lines.

The 2026-09-28 hardening (`2479c79`) followed the same pattern for everything
it added: input aggregation, endpoint delivery, wake policy, long reads,
management transactions, storage record validation, and display policy.

## Decision

Split each subsystem into a hardware-free core and a thin asynchronous shell:

| Core module (host-tested) | Decides | Shell (firmware only) |
| --- | --- | --- |
| [hid/](../../src/hid/): `report_protocol`, `keyboard`, `mouse`, `consumer`, `coalesce`, `aggregate`, `delivery`, `wake`, `host_leds` | Report Map parsing, report classification and serialization, per-source union, endpoint queue and replay policy, wake eligibility, host LED forwarding | [hid_client.rs](../../src/ble/hid_client.rs), [hid_device.rs](../../src/usb/hid_device.rs), [host_requests.rs](../../src/usb/host_requests.rs) |
| [coordinator.rs](../../src/ble/coordinator.rs) | Slot reservation, the attempt numbers that let the coordinator ignore a replaced attempt's events, and the `Action`s for each command and slot event (the slot-event reducers in its child module [coordinator_events.rs](../../src/ble/coordinator_events.rs)) | `execute_action` in [multi_conn.rs](../../src/ble/multi_conn.rs) |
| [scan_list.rs](../../src/ble/scan_list.rs) | Which HID advertisers a user scan lists, and which listed device a newcomer replaces when the list is full | `scan` in [scanner.rs](../../src/ble/scanner.rs) |
| [reconnect.rs](../../src/ble/reconnect.rs) | Which slot an advertisement from a saved device belongs to, whether two saved-device records are the same device, how long a sighting stays usable, when a slot is due a wake, and the reconnect scan's duty cycle ([ADR 0015](0015-shared-reconnect-scan.md)) | `find_saved_peer` and `update` in [scanner.rs](../../src/ble/scanner.rs), `connection_slot_task` in [slot_worker.rs](../../src/ble/slot_worker.rs) |
| [conn_params.rs](../../src/ble/conn_params.rs) | The connection parameters granted to a peripheral's request ([ADR 0016](0016-bounded-peer-connection-parameters.md)) | `Bonder::conn_param_update_request` in [bonder.rs](../../src/ble/bonder.rs) |
| [long_read.rs](../../src/ble/long_read.rs) | Assembly and bounds of a fragmented ATT read | `read_report_map` in `hid_client.rs` |
| [management.rs](../../src/ble/management.rs) | Worker quiescence barrier and commit-then-publish | `manage_devices` in `multi_conn.rs`, `DeviceStore` in [storage.rs](../../src/storage.rs) |
| [messages.rs](../../src/ble/messages.rs) | The commands and events between the UI and the BLE side, and the per-request management IDs they carry | The channels in [main.rs](../../src/main.rs) and `ble_task` in `multi_conn.rs` |
| [pnp_id.rs](../../src/ble/pnp_id.rs) | What a Device Information Service PnP ID value says, and which values are malformed | `log_pnp_id` in [device_info.rs](../../src/ble/device_info.rs) |
| [adv_parser.rs](../../src/ble/adv_parser.rs) | HID service detection and device names in advertisements | [scanner.rs](../../src/ble/scanner.rs) |
| [bond_table.rs](../../src/ble/bond_table.rs) | Which bonding keys the security handler holds: saved devices' keys beside unsaved pairings, which a new pairing never displaces, and which an unsaved connection's end or a store eviction drops | `Bonder` in [bonder.rs](../../src/ble/bonder.rs), and `execute_action` in [multi_conn.rs](../../src/ble/multi_conn.rs) |
| [devices.rs](../../src/storage/devices.rs), [codec.rs](../../src/storage/codec.rs), [framing.rs](../../src/storage/framing.rs), [record.rs](../../src/storage/record.rs) | The paired-device list (fail-closed load, legacy format, identity merge, eviction, Forget and reset candidates), the record codec, and frame and record validation, on SoftDevice-free types; IRK resolution is passed in as a function | `DeviceStore` in `storage.rs`, which converts SoftDevice types and does the flash I/O |
| [ui_logic.rs](../../src/ui/ui_logic.rs), [controller.rs](../../src/ui/controller.rs), [input_logic.rs](../../src/ui/input_logic.rs), [layout.rs](../../src/ui/layout.rs), [display_logic.rs](../../src/ui/display_logic.rs) | Screen transitions; the UI loop's decisions (the command each button sends, management request tracking and its deadline); list windowing; each screen's text and where it sits; display retry policy | UI loop in [main.rs](../../src/main.rs) and [sim.rs](../../src/sim.rs), [display.rs](../../src/ui/display.rs), [buttons.rs](../../src/ui/buttons.rs) |
| [power_logic.rs](../../src/power_logic.rs) | Display power state | [power.rs](../../src/power.rs) |
| [diagnostics.rs](../../src/diagnostics.rs) | The build identity the boot line reports, which causes a `POWER.RESETREAS` value names, the event counters, and when the counts are logged | `main` in [main.rs](../../src/main.rs), which reads and clears the register and logs the counters; the shells that bump them; `selftest.rs` and `sim.rs`, which log the identity |

Rules for the core:

- It takes and returns plain data and never names a SoftDevice, Embassy, or USB
  type. Where a hardware type would leak in, the core is generic over it:
  `ConnManager<A>` is generic over the address type, so the firmware uses
  `nrf_softdevice::ble::Address` and the simulation a `u32`.
- Decisions are returned as data, such as coordinator `Action`s and UI
  commands, and the shell performs them.
- Where the asynchronous loop is itself the policy, the core defines narrow
  traits and the shell implements them. `run_endpoint` in
  [delivery.rs](../../src/hid/delivery.rs) is the production endpoint worker,
  written against `DeliveryQueue`, `ReportSink`, and `RetryClock`, so host
  tests run that exact function against fake endpoints and a fake clock.
- There is one implementation. `src/lib.rs` includes the firmware's own files
  through `#[path]`; the bridge, the self-test, and the Renode simulation use
  the same modules.

Rules for the shell:

- It performs I/O, translates hardware events into core inputs, and applies the
  core's outputs. `execute_action` in `multi_conn.rs` is "the only place the
  pure decisions touch hardware/channels".
- A decision that starts to grow in task code moves into a core module, with
  tests, before it grows further.

## Alternatives Considered

- **A separate host implementation.** The 2026-02-21 library was partly this:
  it shared the report types but kept its own `HidReport` and classification
  code. It is easy to start, but two copies diverge without any test noticing,
  and a passing host test no longer says anything about the firmware.
- **Mock the hardware and test the tasks on the host.** The SoftDevice bindings
  expose concrete types (`Softdevice`, `Connection`, `Flash`) and callback
  traits rather than an abstraction designed for substitution. Wrapping all of
  them would add a large layer that itself needs testing. bt2usb uses narrow
  traits only where the loop is the logic (endpoint delivery) and keeps other
  decisions out of the loops.
- **Run unit tests on the target.** On-target harnesses run through a probe
  need a board for every run, so CI could not run them, and they are slower to
  iterate. They could complement the self-test later; they do not replace host
  tests.
- **Emulate the whole firmware.** The SoftDevice and the USB peripheral cannot
  be emulated, so an emulator can only run a SoftDevice-free variant. That
  variant is the Renode layer, which reuses the core modules rather than
  replacing their tests ([ADR 0014](0014-renode-gpio-models.md)).

## Rationale

Host tests run in seconds on Linux and Windows without a board, and they can
cover what a board cannot reproduce on demand: malformed and truncated input,
every state transition, wraparound of request IDs, stale completions after a
USB reset, and cancellation in the middle of a persistence step. Results are
deterministic, so a failure is a defect, not a flaky radio.

Keeping the shell thin limits what only hardware can reveal to driver
behavior, timing, and interoperability, which the later verification layers own
([ADR 0004](0004-layered-verification.md)). Sharing the same files between
firmware, simulation, and tests means a passing test describes code that
ships.

## Consequences

Positive:

- Most behavior changes get fast, deterministic regression tests on both CI
  operating systems.
- The simulation and the self-test reuse tested logic instead of carrying
  their own.
- The boundary is visible in review: a change that adds a decision to a task
  stands out.

Negative:

- The shells are not host-tested. That includes `multi_conn.rs`,
  `slot_worker.rs`, `slot_link.rs`, `bonder.rs`, `hid_client.rs`, `scanner.rs`,
  `usb/hid_device.rs`, `usb/host_requests.rs`, `ui/display.rs`, `power.rs`,
  and the `storage.rs` shell (flash I/O, write retries, and the conversion to
  SoftDevice types). Since 2026-10-10 the store's load, merge, and eviction
  rules and its codec are in the host-tested `storage/devices.rs` and
  `storage/codec.rs`.
- Coverage percentages describe only the host library, not the firmware.
- A test placed in a firmware-only module, such as `src/ble/scanner.rs`, is
  never compiled, because that module depends on the SoftDevice and is not part
  of the host library; tests belong beside the pure module they exercise. Ten
  such tests sat in `scanner.rs` until 2026-10-10.
- The pattern costs some indirection: generic types, small traits, `#[path]`
  includes, and a `dead_code` allowance in the simulation binary.

Follow-up obligations:

- Put new behavior in a core module with tests first, then wire it into a task.
- "Host tests for the I/O shells" in [TODO.md](../../TODO.md): move the
  remaining decisions in the shells into hardware-free modules, or test the
  shells against fakes. The device store's rules moved out under
  ["Host tests for the device store"](../../TODO.md#verification-and-code-quality),
  done on 2026-10-10.
- "Async task fault tests" in [TODO.md](../../TODO.md) covers the shell-level
  behavior (cancellation, full channels, contention) that reducer tests cannot.
- Passing host tests never closes a hardware gate; that needs the later layers
  in [ADR 0004](0004-layered-verification.md).

## Implementation

- [lib.rs](../../src/lib.rs) is `#![cfg_attr(not(test), no_std)]`. It exports
  `hid` verbatim, includes `ble/adv_parser.rs`, `ble/bond_table.rs`,
  `ble/conn_params.rs`, `ble/coordinator.rs`, `ble/reconnect.rs`,
  `ble/scan_list.rs`, `ble/long_read.rs`, `ble/management.rs`,
  `ble/messages.rs`, `ble/pnp_id.rs`, `power_logic.rs`, and the five pure
  `ui` files (`controller.rs`, `display_logic.rs`, `input_logic.rs`,
  `layout.rs`, `ui_logic.rs`) through `#[path]`, and
  includes `storage/codec.rs`, `devices.rs`, `framing.rs`, and `record.rs` only
  under `#[cfg(test)]`, in an inline `storage` module. It declares `config`
  and, since 2026-10-11, `diagnostics` as ordinary modules.
- The self-test includes `ble/adv_parser.rs` through `#[path]`, and the
  simulation compiles the same pure modules and the four `storage` files for
  the ARM target and runs the coordinator, management, the device store,
  `ui::controller`, and `ui::layout` there
  ([ADR 0014](0014-renode-gpio-models.md),
  [ADR 0024](0024-renode-oled-models.md)).
- Test files: `src/lib_tests.rs`, `src/lib_logic_tests.rs`,
  `src/hid_descriptor_tests.rs`, `src/hid_keyboard_report_tests.rs`,
  `src/hid_classify_tests.rs`, `src/ble/coordinator_tests.rs`,
  `src/ble/coordinator_attempt_tests.rs`, `src/ble/scan_list_tests.rs`,
  `src/ble/bond_table_tests.rs`, `src/diagnostics_tests.rs`,
  `src/ble/reconnect_tests.rs`, `src/storage/devices_tests.rs`,
  `src/storage/devices_format_tests.rs`, `src/ui/ui_logic_tests.rs`,
  `src/ui/controller_tests.rs`, `src/ui/layout_tests.rs`,
  `src/hid/delivery_tests.rs` (the production
  worker against fake endpoints, for example
  `unpolled_consumer_allows_actual_keyboard_and_mouse_workers_to_write`),
  in-module tests such as those in `aggregate.rs` and `management.rs`,
  [tests/integration.rs](../../tests/integration.rs),
  [tests/oled_font.rs](../../tests/oled_font.rs), and
  [tests/vendor_portal.rs](../../tests/vendor_portal.rs), which compiles the
  vendored crate's event portal on the host.
- CI runs `cargo test --locked --lib --tests`, `cargo clippy --locked --lib
  --tests -- -D warnings`, and `cargo doc --locked --no-deps --lib` with
  warnings denied on `ubuntu-24.04` and `windows-2025`
  ([ci.yml](../../.github/workflows/ci.yml)). `mask coverage` measures the same
  library with `cargo-llvm-cov`, or `cargo-tarpaulin` as a fallback.

### Verification Status

- **Implemented:** the split in the table above, for every subsystem listed.
- **Software-verified:** counting with `grep -rh '#\[test\]' src tests | wc -l`
  finds 402 test attributes, all of them compiled by
  `cargo test --locked --lib --tests`: 391 unit tests, 3 integration tests,
  3 glyph-table tests, and 5 vendored-portal tests, which passed on
  2026-10-11 (the
  [boot diagnostics record](../testing.md#validation-record--2026-10-11-boot-identity-and-reset-reason);
  the [test map](../testing.md#test-map) lists what was added since the
  [2026-10-09 validation record](../testing.md#validation-record--2026-10-09),
  which ran 260 unit tests). Since 2026-10-10 the pure core also includes
  `ui::controller` and `ui::layout`, which took the UI loop's decisions and
  each screen's text out of the untested shells.
  The CI host-test jobs on Linux and Windows passed on GitHub-hosted runners
  in push runs 36441995385 (`8a04b25`, 2026-09-28) and 37932436721
  (`7fc99d6`, 2026-10-09) and scheduled run 37338711407 (2026-10-05). The
  shells listed under Consequences have no host tests.
- **Hardware-verified:** not applicable to the split itself. The shells it
  leaves untested are covered only by the board layers in
  [ADR 0004](0004-layered-verification.md), for which the repository holds no
  board record.

## Related

- [Testing: host tests and coverage](../testing.md#host-tests-and-coverage),
  [tests that do not run](../testing.md#tests-that-do-not-run), and
  [modules without host tests](../testing.md#modules-without-host-tests)
- [Code quality](../code-quality.md)
- [Architecture constraints](../architecture.md#architecture-constraints)
- [Development: making a change](../development.md#making-a-change)
- [ADR 0004: Verify in layers](0004-layered-verification.md)
- [ADR 0005: Two slots and independent endpoints](0005-two-slots-and-independent-endpoints.md)
- [ADR 0009: Isolated display task](0009-isolated-display-task.md)
- [ADR 0014: Renode GPIO models](0014-renode-gpio-models.md)
- [ADR 0024: Renode OLED models](0024-renode-oled-models.md)
