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
  list of `#[path]` includes of the firmware's own files and is now 103 lines.

The 2026-09-28 hardening (`2479c79`) followed the same pattern for everything
it added: input aggregation, endpoint delivery, wake policy, long reads,
management transactions, storage record validation, and display policy.

## Decision

Split each subsystem into a hardware-free core and a thin asynchronous shell:

| Core module (host-tested) | Decides | Shell (firmware only) |
| --- | --- | --- |
| [hid/](../../src/hid/): `report_protocol`, `keyboard`, `mouse`, `consumer`, `coalesce`, `aggregate`, `delivery`, `wake`, `host_leds` | Report Map parsing, report classification and serialization, per-source union, endpoint queue and replay policy, wake eligibility, host LED forwarding | [hid_client.rs](../../src/ble/hid_client.rs), [hid_device.rs](../../src/usb/hid_device.rs) |
| [coordinator.rs](../../src/ble/coordinator.rs) | Slot reservation and the `Action`s for each command and slot event | `execute_action` in [multi_conn.rs](../../src/ble/multi_conn.rs) |
| [reconnect.rs](../../src/ble/reconnect.rs) | Which slot an advertisement from a saved device belongs to, how long a sighting stays usable, and the reconnect scan's duty cycle ([ADR 0015](0015-shared-reconnect-scan.md)) | `find_saved_peer` in [scanner.rs](../../src/ble/scanner.rs), `connection_slot_task` in `multi_conn.rs` |
| [conn_params.rs](../../src/ble/conn_params.rs) | The connection parameters granted to a peripheral's request ([ADR 0016](0016-bounded-peer-connection-parameters.md)) | `Bonder::conn_param_update_request` in `multi_conn.rs` |
| [long_read.rs](../../src/ble/long_read.rs) | Assembly and bounds of a fragmented ATT read | `read_report_map` in `hid_client.rs` |
| [management.rs](../../src/ble/management.rs) | Worker quiescence barrier and commit-then-publish | `manage_devices` in `multi_conn.rs`, `DeviceStore` in [storage.rs](../../src/storage.rs) |
| [adv_parser.rs](../../src/ble/adv_parser.rs) | HID service detection and device names in advertisements | [scanner.rs](../../src/ble/scanner.rs) |
| [framing.rs](../../src/storage/framing.rs), [record.rs](../../src/storage/record.rs) | Store frame and record validation | `DeviceStore` in `storage.rs` |
| [ui_logic.rs](../../src/ui/ui_logic.rs), [input_logic.rs](../../src/ui/input_logic.rs), [display_logic.rs](../../src/ui/display_logic.rs) | Screen transitions, management request IDs, list windowing, display retry policy | UI loop in [main.rs](../../src/main.rs), [display.rs](../../src/ui/display.rs), [buttons.rs](../../src/ui/buttons.rs) |
| [power_logic.rs](../../src/power_logic.rs) | Display power state | [power.rs](../../src/power.rs) |

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
  `hid_client.rs`, `scanner.rs`, `usb/hid_device.rs`, `ui/display.rs`,
  `power.rs`, `storage/codec.rs`, and the `DeviceStore` logic in `storage.rs`
  that loads the legacy format, merges records for the same identity, evicts the
  oldest peer, and sets the writable flag.
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
  shells against fakes. The `DeviceStore` load, merge, and eviction rules have
  their own item,
  ["Host tests for the device store"](../../TODO.md#verification-and-code-quality).
- "Async task fault tests" in [TODO.md](../../TODO.md) covers the shell-level
  behavior (cancellation, full channels, contention) that reducer tests cannot.
- Passing host tests never closes a hardware gate; that needs the later layers
  in [ADR 0004](0004-layered-verification.md).

## Implementation

- [lib.rs](../../src/lib.rs) is `#![cfg_attr(not(test), no_std)]`. It exports
  `hid` verbatim, includes `ble/adv_parser.rs`, `ble/conn_params.rs`,
  `ble/coordinator.rs`, `ble/reconnect.rs`, `ble/long_read.rs`,
  `ble/management.rs`,
  `power_logic.rs`, and the three `ui` logic files through `#[path]`, and
  includes `storage/framing.rs` and `storage/record.rs` only under
  `#[cfg(test)]`.
- The self-test includes `ble/adv_parser.rs` through `#[path]`, and the
  simulation compiles `ble::coordinator` and `ui::ui_logic` for the ARM target.
- Test files: `src/lib_tests.rs`, `src/lib_logic_tests.rs`,
  `src/hid_descriptor_tests.rs`, `src/ble/coordinator_tests.rs`,
  `src/hid/delivery_tests.rs` (the production worker against fake endpoints,
  for example `unpolled_consumer_allows_actual_keyboard_and_mouse_workers_to_write`),
  in-module tests such as those in `aggregate.rs` and `management.rs`, and
  [tests/integration.rs](../../tests/integration.rs).
- CI runs `cargo test --locked --lib --tests`, `cargo clippy --locked --lib
  --tests -- -D warnings`, and `cargo doc --locked --no-deps --lib` with
  warnings denied on `ubuntu-latest` and `windows-latest`
  ([ci.yml](../../.github/workflows/ci.yml)). `mask coverage` measures the same
  library with `cargo-llvm-cov`, or `cargo-tarpaulin` as a fallback.

### Verification Status

- **Implemented:** the split in the table above, for every subsystem listed.
- **Software-verified:** counting with `grep -rh '#\[test\]' src tests | wc -l`
  finds 267 test attributes, all of them compiled by
  `cargo test --locked --lib --tests`: 264 unit and 3 integration tests, which
  passed on 2026-10-10 (the
  [2026-10-09 validation record](../testing.md#validation-record--2026-10-09)
  ran 260 unit tests, before four advertisement tests moved out of the
  firmware-only `scanner.rs`).
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
