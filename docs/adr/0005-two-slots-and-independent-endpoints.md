# ADR 0005: Aggregate Two BLE Sources Into Independent USB Endpoint Workers

- Status: Accepted
- Date: 2026-10-09

## Context

A typical desk has one keyboard and one mouse, sometimes a second keyboard. USB
endpoints can stall independently: a host may stop polling one interface, and
suspend, reset, or unplug can happen while keys are held. The worst user-visible
failure is a key or button that stays pressed on the host.

## Decision

- Support two simultaneous BLE connection slots, each owned by its own worker,
  with up to four stored peers. A shared GAP lock serializes scan and connection
  establishment because the SoftDevice permits one such procedure at a time.
- Tag every input with its source slot. Union keyboard keys, modifiers, and
  mouse buttons across sources; give consumer control to the lowest active slot;
  never treat relative mouse motion as held state.
- Drive the keyboard, mouse, and consumer endpoints from independent workers,
  each with a bounded FIFO, a write deadline, and retry backoff, so an unpolled
  endpoint cannot block the others.
- On reset, configuration, resume, or protocol change, replay current held state
  and never replay motion. On link loss, release that source's held input.
- Request USB remote wakeup only for a newly pressed key, modifier, consumer
  usage, or mouse button.

## Rationale

Per-source state makes "release one device, keep the other's keys" correct by
construction. Bounded queues keep memory fixed; collapsing a saturated queue to
current state favors delivering final releases over every intermediate tap.

## Consequences

- Under sustained overload, intermediate taps or relative motion may be dropped.
  The acceptable bound is an open release gate.
- More than six unique keys produces the keyboard rollover error report.
- Adding a third slot or a new endpoint means revisiting channel capacities,
  GAP contention, and RAM.
