# ADR 0003: Keep Decisions In Hardware-Free Modules And I/O In Thin Tasks

- Status: Accepted
- Date: 2026-10-09

## Context

Most bt2usb defects that matter to a user (stuck keys, a wrong screen, a lost
bond, a malformed report accepted) come from decision logic rather than from
register access. That logic is hard to exercise on a board and cannot be
exercised in CI without one.

## Decision

Split each subsystem into:

- pure, synchronous modules with no hardware types in their decisions: HID
  parsing and serialization, input aggregation and coalescing, delivery and
  wake policy, BLE coordinator and slot reducers, UI transitions, power policy,
  storage framing and record validation
- async task code in the firmware binaries that performs I/O and feeds events
  into those modules

`src/lib.rs` exports the pure modules for host tests. The firmware, the
self-test, and the Renode simulation call the same modules; there is no second
implementation for tests.

## Rationale

Host tests run in seconds on any OS and can cover malformed input, every state
transition, and timing edge cases deterministically. Keeping the task layer
thin limits what only a board can reveal to driver behavior, timing, and
interoperability.

## Consequences

- New behavior goes into a pure module with tests first; task code wires it up.
- Some hardware-coupled modules are outside the host crate, so coverage
  percentages describe only the instrumented selection.
- Passing host tests does not verify drivers. That remains the job of the
  later layers in [ADR 0004](0004-layered-verification.md).
