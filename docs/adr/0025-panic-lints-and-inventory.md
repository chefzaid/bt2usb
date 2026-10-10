# ADR 0025: Deny Panic-Prone Constructs With Clippy And List The Ones It Cannot See

- Status: Accepted
- Date: 2026-10-10

## Context

All three firmware binaries link `panic-probe`: a panic prints its message over
RTT and stops the core, and nothing resets the chip
([what is fatal](../architecture.md#what-is-fatal)). Watchdog recovery is still
a proposal ([ADR 0020](0020-watchdog-and-progress-based-recovery.md)). A panic
that outside data can reach is therefore a defect, and its cost depends on the
source:

- **A BLE peer** (advertising data, GATT responses, Report Maps, reports, SMP
  and GAP events) could stop the bridge whenever it likes.
- **The USB host** (control requests, output reports) could stop it from the
  computer side.
- **Flash** (the pairing store) is read at every boot, so a record that panics
  the decoder would stop the bridge on every boot, and the UI that offers
  Factory reset never starts.

Until 2026-10-10 the panic table in the
[code quality guide](../code-quality.md#panics-allocation-and-arithmetic)
listed only the panic macros (`unwrap!`, `expect`, `unreachable!`), found by a
script that stopped at each file's test module. Slice indexing, `RefCell`
borrows, `StaticCell` initialization, and the vendored `nrf-softdevice` were not
listed; the P2 item "Inventory panic sites in firmware paths" in
[TODO.md](../../TODO.md) asked for all of them, application and vendored.

Clippy has restriction lints for most of these constructs: `indexing_slicing`,
`string_slice`, `unwrap_used`, `expect_used`, `panic`, `unreachable`, `todo`,
and `unimplemented`. Run over the four CI builds (host, firmware, firmware with
`log-sensitive-data`, simulation), they flagged 112 sites outside tests in 20
files: 80 indexing, 29 slicing, 2 `unreachable!`, and 1 `expect`. Many read
outside data: the advertising-data walk (`adv_parser.rs`), the Report Map
parser and report decoders (`report_protocol.rs`, `keyboard.rs`, `mouse.rs`,
`consumer.rs`, `hid/mod.rs`), the host's LED report (`host_requests.rs`), and
the flash record codecs (`codec.rs`, `framing.rs`, `record.rs`, `devices.rs`).
Each had a bound check before it, so none was a live defect, but the checks
were separate from the indexing they protected, and nothing stopped the next
change from separating them further.

No lint sees defmt's `unwrap!`, `assert!`, `RefCell` borrows, `StaticCell`
initialization, panics inside dependency calls, or division by zero. The
vendored crate is a dependency, so bt2usb's lints do not apply to it. A
read-only inventory of both, each claim checked by a second reviewer, found
five vendored panics a peer can reach. Four are removed here (see Decision);
the fifth, in `Address::address_type`, comes with a storage defect on the same
path and is a follow-up.

## Decision

- **Turn the lints on for every target.** `[lints.clippy]` in `Cargo.toml`
  sets the eight lints above to `warn`, and CI's Clippy runs deny warnings.
  `clippy.toml` allows `unwrap_used`, `expect_used`, `indexing_slicing`, and
  `panic` inside `#[test]` functions and `#[cfg(test)]` modules, where a panic
  is a failed test. The integration test `tests/oled_font.rs` and `build.rs`
  allow the lints they use at crate level, with a reason, for the same cause.
- **Remove a flagged panic path where the code can; state the bound where it
  cannot.** A flagged site is rewritten with the same behavior (`get` and
  `get_mut` returning the function's existing `None` or error path,
  `first_chunk`, `split_first_chunk_mut`, `split_at_checked`, slice patterns,
  iterators, or fixed-size array types), or it keeps the construct under
  `#[expect(clippy::<lint>, reason = "<the bound>")]` on the narrowest item.
  `#[expect]`, not `#[allow]`, so an attribute whose lint no longer fires fails
  the build. Data from a peer, the host, or flash takes the rewrite unless its
  type alone bounds the access: the one such kept site,
  `InputAggregator::keyboard`, indexes a `[bool; 256]` with a peer's `u8`
  keycode.
- **List what no lint sees.** The
  [code quality guide](../code-quality.md#panic-paths-no-lint-flags) lists
  every unlinted panic construct in the application and every panic path in
  the compiled vendored modules, each with the reason it cannot fire or, for
  `Address::address_type`, the open FIXME that tracks it. A change
  that adds one updates the list ([review checklist](../code-quality.md#review-checklist)).
- **Check configuration at compile time where Rust allows it.** The link-count
  conversion and the record-size and event-size checks are `const`
  assertions, and `sequential_storage::map::MapConfig::new`, a `const fn` that
  panics on a bad range, runs in `const` blocks in `storage.rs` and
  `selftest.rs`.
- **Remove the peer-reachable vendored panics**, as an amendment to the
  patch of [ADR 0007](0007-vendored-softdevice-patch.md):
  - size the BLE event buffer for the configured MTU: bt2usb enables the
    crate's `evt-max-size-256` feature, and a `const` assertion in
    `sd_setup.rs` computes Nordic's `BLE_EVT_LEN_MAX(ATT_MTU)` from the S140
    bindings' layout and fails the build if it outgrows the buffer;
  - log a GAP timeout from an unexpected source instead of panicking, and keep
    the link (every report that arrives on it is still authenticated);
  - return the existing `DisconnectedError` from `disconnect_with_reason` for
    any SoftDevice error, and let `Connection::drop` accept it (two panics).
- **Harden the discovery and MTU-exchange waiters** in the same amendment: on
  an event other than their response, a timeout, or a disconnect they now log
  it at trace and keep waiting; `read_by_offset` and `write` already kept
  waiting, without logging. No
  peer can produce such an event today, because the SoftDevice sends nothing
  else while one procedure is outstanding and bt2usb runs one procedure per
  link at a time; the change matters only if either stops being true.

## Alternatives Considered

- **Keep the hand-maintained table.** It covered only macros, and a table of
  line numbers drifts with every change
  (the citation FIXME in [TODO.md](../../TODO.md#fixme) found the same drift in
  the ADRs). The lints make the compiler keep the list for the constructs they
  cover.
- **`#[allow]` the lints per module and document the sites.** No enforcement:
  a new index in an allowed module passes silently.
- **Enable the lints only in modules that read outside data.** The boundary is
  not module-shaped: the record codec serves flash, the report decoders serve
  peers and tests, and data moves between modules. A crate-wide rule is easier
  to state and to keep.
- **Also enable `integer_division_remainder_used` or
  `arithmetic_side_effects`.** Tried: the division lint flagged eight sites,
  seven of them with constant divisors, and the arithmetic lint flags every
  addition. Only division and remainder by zero panic in the release build,
  which keeps `overflow-checks = false`; the one division by a runtime value
  (`max_latency_for` in `conn_params.rs`) is listed with its guard, and debug
  builds and host tests still panic on overflow
  ([arithmetic](../code-quality.md#arithmetic)).
- **Prove panic freedom at link time** (for example by failing the build when
  the release ELF references the panic handler). Not possible here: the
  firmware keeps deliberate boot-time panics (`Softdevice::enable` with too
  little RAM), dependencies panic on their own contract violations, and
  `panic-probe` is linked on purpose.
- **Reset on panic instead.** A reset hides the defect and, for a flash record
  that panics at boot, loops. Recovery after an unexpected stop belongs to the
  watchdog decision ([ADR 0020](0020-watchdog-and-progress-based-recovery.md));
  it complements this ADR and does not replace it.
- **Leave the vendored panics to upstream.** Upstream `nrf-softdevice` at the
  pinned commit has them, and bt2usb already carries a patch for the same
  class of defect ([ADR 0007](0007-vendored-softdevice-patch.md)).

## Rationale

The lints turn "every index is checked" from a review habit into a build
failure, at no runtime cost where the rewrite only moves an existing check
into the access. Rewriting rather than annotating also put each bound next to
the access it protects, so a later edit cannot move one without the other. The
three sites that keep a reason hold constant or type-guaranteed bounds, where a
rewrite would only add a fallback no input can reach. For what the lints cannot
see, a reasoned list is the best evidence available short of a proof, and the
inventory showed it is worth having: it found five vendored panics a peer can
reach and four defects that are not panics (see Follow-ups).

## Consequences

Positive:

- No index, slice, `unwrap`, `expect`, `panic!`, or `unreachable!` can enter
  the application without passing CI with a stated reason.
- A peer can no longer stop the bridge through an oversized discovery event, an
  authenticated-payload timeout, or a link that ends while the bridge drops it
  (for example after a failed MTU exchange).
- An unexpected event in a discovery or MTU-exchange waiter is logged instead
  of halting the chip.
- Host line coverage rose from 98.16% to 98.88%, with tests for the new error
  branches (for example zero-length and overrunning advertising structures,
  descriptor items of the wrong size, and buffers too small for a record).

Negative:

- Some code is longer: chains of `split_first_chunk_mut` instead of constant
  ranges, and `Option` returns from formerly infallible helpers.
- A reason can become untrue while its lint still fires; the review checklist,
  not the compiler, catches that.
- The unlinted list is maintained by hand and can drift like the old table.
- The vendored patch grew, so an `nrf-softdevice` upgrade has more to re-apply.
- The event-size check reads the layout of the pinned S140 bindings; a
  SoftDevice upgrade must keep it computing Nordic's macro.
- `softdevice_task` needs 128 more bytes of stack for the event buffer.

Follow-ups, tracked in [TODO.md](../../TODO.md#fixme) (the last one under
[Verification and code quality](../../TODO.md#verification-and-code-quality)):

- "Report Maps are cut short when a peripheral offers an MTU above 64": the
  vendored MTU exchange stores the peer's receive MTU, not the negotiated one.
  Fixed on 2026-10-10 ([ADR 0007](0007-vendored-softdevice-patch.md)).
- "A bond with a private or reserved identity address breaks the store": the
  vendored `Address::address_type` unwraps the identity address type a peer
  sends during pairing, and a private identity is saved but refused on reload.
  Fixed on 2026-10-10: the store decodes the type itself and keeps no bond a
  reload would refuse ([ADR 0006](0006-fail-closed-pairing-store.md)).
- "A peripheral that refuses the MTU exchange cannot connect": the vendored
  connect fails when the peer answers the exchange with an error, though the
  Core specification lets the link continue at the default MTU. Fixed on
  2026-10-10 ([ADR 0007](0007-vendored-softdevice-patch.md)).
- "A peripheral's own MTU exchange or CCCD access is never answered": with the
  GATT server feature off, the vendored crate drops the GATT server events
  that need a reply. Fixed on 2026-10-10
  ([ADR 0007](0007-vendored-softdevice-patch.md)).
- "Bonder callbacks re-enter the vendored connection state": `on_bonded` and
  `get_peripheral_key` call `Connection::peer_address` while the vendored
  crate holds a mutable reference to the same state, which is undefined
  behavior.
- "Parser fuzzing and property tests" (P1) would exercise the rewritten
  parsers with generated input.

## Implementation

| Concern | Where |
| --- | --- |
| Lint levels | `[lints.clippy]` in [Cargo.toml](../../Cargo.toml) |
| Test allowances | [clippy.toml](../../clippy.toml); crate-level `#![allow]` in [tests/oled_font.rs](../../tests/oled_font.rs) and [build.rs](../../build.rs) |
| Kept sites | `InputAggregator::keyboard` ([aggregate.rs](../../src/hid/aggregate.rs)), the serial `write!` in `init` ([hid_device.rs](../../src/usb/hid_device.rs)), `SimBle::scenario_step` ([sim_ble.rs](../../src/sim_ble.rs)) |
| Rewritten parsers | `ad_structures` in [adv_parser.rs](../../src/ble/adv_parser.rs); `HidDescriptor::parse` and `ReportReference::parse` in [report_protocol.rs](../../src/hid/report_protocol.rs); the report decoders in [hid/](../../src/hid); the codecs in [storage/](../../src/storage); `set_report` in [host_requests.rs](../../src/usb/host_requests.rs) |
| Compile-time checks | `LINKS` and the event-size assertion in [sd_setup.rs](../../src/sd_setup.rs); the record-size assertion in [devices.rs](../../src/storage/devices.rs); `const` blocks around `MapConfig::new` in [storage.rs](../../src/storage.rs) and [selftest.rs](../../src/selftest.rs) |
| Vendored changes | `gap::on_evt` (timeout arm), `ConnectionState::disconnect_with_reason` and `Connection::drop`, and the waiters in `discover_service`, `discover_characteristics`, `discover_descriptors`, and `att_mtu_exchange`, under `vendor/nrf-softdevice/src/ble/`; `evt-max-size-256` in [Cargo.toml](../../Cargo.toml) |
| Inventory | [Panic lints](../code-quality.md#panic-lints), [panic paths no lint flags](../code-quality.md#panic-paths-no-lint-flags), and [vendored nrf-softdevice](../code-quality.md#vendored-nrf-softdevice) in the code quality guide |
| USB handler budget | The comment at `builder.handler` in `hid_device::init`: four handlers fill embassy-usb's default `MAX_HANDLER_COUNT` |

### Verification Status

- **Implemented:** everything above, on 2026-10-10.
- **Software-verified:** the four Clippy builds pass with warnings denied; 361
  library tests, 3 integration tests, and 3 glyph-table tests pass; rustdoc
  builds pass; the Renode scenario and model checks pass. The compile-time
  checks were tried against bad values: a 131-byte event buffer fails the
  build and 132 bytes passes (so the computed worst case at MTU 64 is exactly
  132), and a misaligned flash range fails the self-test build. Two new
  descriptor tests were checked to fail when the size checks they cover are
  removed. Each inventory claim was reviewed by a second agent told to refute
  it.
- **Hardware-verified:** not yet. The vendored paths run only on the board,
  and exercising them needs a peer that misbehaves on purpose.

## Related

- [Code quality: panics, allocation, and arithmetic](../code-quality.md#panics-allocation-and-arithmetic)
- [Architecture: what is fatal](../architecture.md#what-is-fatal)
- [Security: input validation boundaries](../security.md#input-validation-boundaries)
- [Vendor notes](../../vendor/nrf-softdevice/README.bt2usb.md)
- [ADR 0003: Hardware-free decision modules](0003-pure-core-and-task-shell.md)
- [ADR 0007: Vendored nrf-softdevice patch](0007-vendored-softdevice-patch.md)
- [ADR 0020: Watchdog and progress-based recovery](0020-watchdog-and-progress-based-recovery.md)
