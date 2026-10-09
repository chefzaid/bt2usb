# ADR 0007: Vendor A Minimal nrf-softdevice Patch At A Pinned Revision

- Status: Accepted
- Date: 2026-10-09

## Context

HID Report Maps can be longer than one ATT MTU, which needs ATT Read Blob.
The pinned upstream `nrf-softdevice` exposed no offset read, panicked on some
GATT timeouts, and could panic or loop on peer-controlled discovery responses.
A peripheral in range must not be able to crash the bridge.

## Decision

Copy the `nrf-softdevice` crate at a pinned upstream commit into
`vendor/nrf-softdevice`, apply a small reviewed patch in `gatt_client.rs`
(offset reads, timeout errors, bounded discovery), and wire it in with a root
Cargo `[patch]`. Sibling crates stay git dependencies at the same commit. The
[vendor notes](../../vendor/nrf-softdevice/README.bt2usb.md) record the exact
changes.

## Rationale

A committed patch keeps builds reproducible and reviewable, unlike editing a
Cargo checkout. Keeping the patch minimal keeps rebasing on upstream cheap.

## Consequences

- The vendored crate keeps upstream formatting; run `cargo fmt` on the bt2usb
  package only.
- Upgrading `nrf-softdevice` means re-applying or dropping the patch; drop it
  only when upstream provides equivalent offset reads, timeout errors, and
  bounded discovery.
