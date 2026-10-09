# ADR 0006: Persist Pairings In A Versioned, Fail-Closed Flash Store

- Status: Accepted
- Date: 2026-10-09

## Context

Stored records hold long-term BLE encryption and identity keys. Overwriting a
store that failed to parse would silently destroy every bond; accepting a
partially valid store could load corrupted key material. Flash writes can fail
transiently while BLE links are active.

## Decision

- Reserve four flash pages (`0xF0000–0xF4000`) for pairing data, excluded from
  application flash by the linker script.
- Store all peers as one `sequential-storage` map item, framed with a magic
  byte, version, record count, and per-record lengths; the
  [data model](../data-model.md#pairing-store) defines the layout.
- Validate the whole frame and every record before loading. If the store is
  malformed, unsupported, or unreadable, load nothing and disable ordinary
  writes until an explicit, confirmed Factory reset recovers the region.
- Write first, then update the in-memory store and SoftDevice bonder only after
  the write succeeds. Retry transient write failures a bounded number of times.

## Rationale

Fail-closed loading turns corruption into a visible, recoverable state rather
than silent bond loss. Commit-then-cache keeps the UI from reporting a deletion
or enrollment that did not persist.

## Consequences

- A user can see "storage error" and keep a disconnected-but-saved device until
  they act.
- Logical deletion is not physical key erasure; older flash records can remain
  until garbage collection.
- Every format change needs a new version, fixture tests, and a stated upgrade
  and downgrade path. Power-loss behavior is an open release gate.
