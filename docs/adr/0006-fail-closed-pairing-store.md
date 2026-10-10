# ADR 0006: Persist Pairings In A Versioned, Fail-Closed Flash Store

- Status: Accepted
- Date: 2026-09-28

## Context

The pairing store holds the only copy of each peer's long-term key (LTK),
identity resolving key (IRK), and identity address. Losing it means pairing
every device again; loading a damaged copy could hand the SoftDevice corrupted
key material. Flash writes can also fail transiently, because SoftDevice flash
operations wait for radio-idle timeslots while links are active.

The store reached its current form in steps:

| Date | Commit | Change |
| --- | --- | --- |
| 2026-02-21 | `8e6dd17` | Paired devices persisted with `sequential-storage` in four pages from page 240 (`STORAGE_FLASH_PAGE_START`, `STORAGE_FLASH_PAGE_COUNT`) |
| 2026-06-21 | `c37d568` | Bond keys added to records, with a magic byte (`0xB2`) and version (`0x01`) in front of the record list |
| 2026-06-22 | `e3bc620` | Flash writes retried (`FLASH_WRITE_ATTEMPTS`); the byte codec split into `storage/codec.rs` |
| 2026-06-24 | `22d208a` | Framing moved into a pure, host-tested `storage/framing.rs` |
| 2026-09-26 | `f477d4c` | The application `FLASH` region ends at `0xF0000`, so code can never be linked onto the pairing pages |
| 2026-09-28 | `2479c79` | This decision: whole-frame validation, a `writable` flag, explicit recovery, and commit-then-cache management |

Before 2026-09-28 the loader was fail-open. A flash read error logged
`Flash read error: {:?}` and continued with an empty store that the next save
would write over every saved bond. The versioned parser, whose behavior had
not changed since 2026-06-21, skipped records that failed to decode and kept
the rest, so a single
damaged record silently dropped out of the store at the next save. There was
also no way to remove one peer short of erasing the chip.

Deleting a device was also about to become a user action. The saved-device menu
added in the same commit lets a user Forget one peer or Factory reset all of
them, and those actions must not report success for a change that never reached
flash, or race a connection worker that is about to save the peer again.

## Decision

**Location and container.**

- Reserve flash pages 240 to 243 (`0x000F0000–0x000F4000`, 16 KiB) for pairing
  data, outside the linker's `FLASH` region in `memory_sd.x`
  ([ADR 0010](0010-static-memory-layout.md)).
- Store all peers as one `sequential-storage` map item under key `0x01`
  (`KEY_PAIRED_DEVICES`), at most 512 bytes (`MAX_RECORD_SIZE`), holding at most
  four peers (`MAX_PAIRED_DEVICES`).

**Format.** Frame the item as a magic byte (`0xB2`), a version byte (`0x01`), a
record count, and length-prefixed records. Each record holds the address and
type, last RSSI, a UTF-8 name of up to 32 bytes, a bond flag, and, when the
flag is set, a 50-byte bond. The [data model](../data-model.md#pairing-store)
owns the byte layout.

**Load fails closed.** Validate the whole frame and every record before loading
anything:

| What flash holds | Result |
| --- | --- |
| No item | Empty store, writable |
| A read error | Empty store, **not writable** |
| The magic byte, with a complete, supported, valid frame of at most four records | Loaded, writable |
| The magic byte, with anything else (truncated, future version, bad length, invalid address type, invalid UTF-8, wrong bond length, bad identity address type) | Nothing loaded, **not writable** |
| No magic byte | Parsed as the legacy unversioned format; any failure loads nothing and is not writable |

A store that is not writable stays that way until the user confirms Factory
reset, which erases the four pages and writes an empty store, or until a later
boot loads the item successfully (for example after a transient read error;
nothing is written in between, so the bytes are unchanged). Ordinary saves
refuse with `StoreError::Unreadable`, and the UI shows "Storage failed".

**Writes are bounded; removals are transactional; enrollment is not.**

- Serialize the whole store first; if it does not fit the item, abort before
  touching flash (`StoreError::Serialization`).
- Retry a failed write up to three times, 20 ms apart (`FLASH_WRITE_ATTEMPTS`,
  `FLASH_RETRY_BACKOFF_MS`), then report `StoreError::Flash`.
- Enrollment updates memory first and flash second. During pairing the
  SoftDevice hands the new keys to the bonder (`on_bonded`), which holds them in
  RAM. When the slot then reports a successful connection, the coordinator's
  `Action::PersistDevice` adds or updates the peer in the in-memory store
  (`store.add`) and only then calls `save_to_flash`. If the save fails, the UI
  shows "Storage failed" and the store stays dirty, so the save at the next
  connection writes it again. Until a save succeeds, the bond exists in RAM
  (bonder and cache) but not in flash.
- Forget and Factory reset are the only write-first paths. They build a
  candidate store, persist it, and publish it as the in-memory store only
  after the write succeeds; the SoftDevice bonder forgets the keys only after
  that. Before either changes anything, every affected connection worker is
  stopped and must acknowledge a fresh token, so no queued connection event
  can recreate a forgotten bond.

**Identity.** Persist a bonded peer under its stable identity address rather
than its current private address, merge records that resolve to the same
identity while loading, and skip a flash write when only the RSSI hint changed.

## Alternatives Considered

- **Fail open: discard and reinitialize a store that does not parse.** This was
  the behavior before 2026-09-28. It keeps the device usable, but a transient
  read error or a single damaged byte erases every bond without telling anyone.
- **Load the valid records and skip the bad ones.** This was the parser from
  2026-06-21 to 2026-09-28. It keeps more devices working, but the next save
  rewrites the store without the skipped records, and a partially valid frame
  is weak evidence that the "valid" records are intact.
- **One flash item per peer.** Updating one peer would rewrite less data, but
  Factory reset, merging duplicate identities, and evicting the oldest peer
  would each span several writes, and an interruption could leave a mix of old
  and new records.
- **A general-purpose bond database or key-value layer.** Nordic's C SDK has a
  peer manager for this, but `nrf-softdevice` leaves bond storage to the
  application through its `SecurityHandler` trait. A generic database would add
  code and a format bt2usb does not control, for at most four records whose
  validation rules are security-specific.
- **Write behind for removals: update the cache first and persist later.** The
  UI would report a deletion that might never reach flash, and the bonder would
  drop keys that flash still holds.
- **Encrypt records at rest.** The key would have to live in the same internal
  flash, so encryption adds little without readout protection. That belongs to
  the open provisioning work rather than to this format.

## Rationale

Fail-closed loading turns corruption into a visible, recoverable state instead
of silent bond loss. The user sees "Storage failed", the damaged bytes stay in
flash for diagnosis, and recovery is a deliberate, confirmed action. Validating
the whole frame before loading any record means a store is either fully trusted
or not used at all, so damaged key material never reaches the security handler.

For removals, commit-then-publish keeps three copies of the truth consistent:
flash, the in-memory cache, and the SoftDevice bonder. If the write fails,
nothing changes, and the UI can say so truthfully. The quiescence barrier
closes the remaining race, where a worker that is still connecting saves the
peer again after the user forgot it. A new pairing is different: the bonder
already holds the keys by the time the record is saved, so discarding the
record would not undo the bond. The record stays in the cache and is retried
rather than discarded, at the cost of a window in which the bond is in RAM but
not in flash.

A single item keeps every change, including Factory reset of a readable store,
to one `store_item` append, which `factory_reset` calls "the same atomic append
as ordinary changes". The data set is small enough (375 bytes at most for four
bonded peers with full names, checked at compile time against the 512-byte
item) that rewriting it whole costs little.

## Consequences

Positive:

- Corruption and read errors are visible and never silently destroy bonds.
- A failed Forget or reset leaves flash, cache, and bonder unchanged, and the UI
  reports the failure instead of a false success. This guarantee covers
  removals only, not enrollment.
- Firmware growth cannot overwrite the pairing pages; the linker fails first.

Negative:

- A failed save after a new pairing leaves the bond in the bonder and the
  in-memory store but not in flash. The peer keeps working until the next
  reset or power loss; if no later save succeeds before then, the bond is lost
  and the peer must be paired again. The UI shows only the generic
  "Storage failed", which does not say that the new peer was not saved
  ("Visible storage/security errors" in
  [TODO.md](../../TODO.md#ui-display-and-power)).
- A store that is not writable stops new pairings from persisting until the user
  runs Factory reset, and that reset discards every record in the region,
  including any that were intact.
- A failed management action can leave a saved device disconnected, because
  its worker was stopped before the write.
- Logical deletion is not physical key erasure; `sequential-storage` appends,
  so older records can remain in flash until garbage collection.
- A full store evicts the oldest peer with only a log line.
- The flash I/O, write retries, and SoftDevice type conversion in the
  `storage.rs` shell are not host-tested; the load, legacy parsing, merge, and
  eviction rules are, since 2026-10-10.
- Power loss during a write or a recovery erase has not been tested.

Follow-up obligations, tracked in [TODO.md](../../TODO.md):

- "Power-loss-safe persistence" and "Forget/reset hardware and interruption
  acceptance": fault-inject writes, garbage collection, and recovery erases, and
  validate Forget and reset on a board.
- "Versioned storage migration": every format change needs a new version
  byte, fixture tests for each supported version and for malformed input, and a
  stated upgrade and downgrade path; older firmware must fail closed on a newer
  version.
- ["Visible storage/security errors"](../../TODO.md#ui-display-and-power):
  surface full-store replacement and persistence failures, including a new
  pairing that was not saved, with useful actions.
- "Provisioning and physical key protection": readout protection and key
  lifetime for the keys this store holds.
- ["Host tests for the device store"](../../TODO.md#verification-and-code-quality)
  (done 2026-10-10): the load, legacy parsing, merge, and eviction rules
  moved into the host-tested `storage/devices.rs`.

## Implementation

| Concern | Where |
| --- | --- |
| Page reservation | `STORAGE_FLASH_PAGE_START = 240`, `STORAGE_FLASH_PAGE_COUNT = 4`, and the derived `STORAGE_FLASH_START`/`STORAGE_FLASH_END` in [config.rs](../../src/config.rs); `FLASH : ORIGIN = 0x00027000, LENGTH = 804K` and the assertion that it ends at `STORAGE_FLASH_START` in [memory_sd.x](../../memory_sd.x) |
| Container and limits | `KEY_PAIRED_DEVICES` in [storage.rs](../../src/storage.rs); `MAX_RECORD_SIZE` and the compile-time size assertion in [devices.rs](../../src/storage/devices.rs) |
| Frame | `MAGIC`, `VERSION`, `has_magic`, `is_versioned`, `is_complete`, `Writer::push` (rolls back a record that does not fit), and `records` in [framing.rs](../../src/storage/framing.rs) |
| Record validation | `ADDRESS_RECORD_SIZE = 7`, `BOND_RECORD_SIZE = 50`, `base`, and `bond` in [record.rs](../../src/storage/record.rs) |
| Byte codec | Device, address-type, and bond encoding in [codec.rs](../../src/storage/codec.rs) |
| Load, save, merge | `DeviceList::load`, `load_empty`, `load_unreadable`, `pending_item`, `add`, and the `writable` and `dirty` flags in [devices.rs](../../src/storage/devices.rs); `DeviceStore::load_from_flash`, `save_to_flash`, and `add` in `storage.rs` do the flash I/O and logging |
| Enrollment | `Bonder::on_bonded` in [bonder.rs](../../src/ble/bonder.rs) stores the keys in RAM during pairing; `execute_action` for `Action::PersistDevice` in [multi_conn.rs](../../src/ble/multi_conn.rs) calls `store.add`, then `save_to_flash`, and sends `BleErrorTag::StorageFailed` if the save fails. `store.add` stores no bond whose identity a reload would refuse (not public or random static, `AddressKind::is_identity`); it returns `BondRefused`, and `execute_action` then calls `Bonder::forget_bond` with exactly that bond and sends `BleErrorTag::BondRefused` |
| Forget and reset | `DeviceList::without` and `reset` (whose `erase_first` is set only when the store is not writable) in `devices.rs`; `DeviceStore::forget` and `factory_reset` in `storage.rs` erase `STORAGE_FLASH_START..STORAGE_FLASH_END` when told to and persist through `commit` |
| Commit and quiescence | `commit` and `Quiescence` in [management.rs](../../src/ble/management.rs); `manage_devices` in `multi_conn.rs` sends `SlotCommand::Quiesce(token)`, waits for `SlotEvent::Quiesced`, and calls `Bonder::forget` or `clear` only after the store write succeeds |
| Boot | `ble_task` loads the store, loads bonds into the bonder, and sends `BleEvent::Error(BleErrorTag::StorageFailed)` when the store is not writable |
| Self-test | `check_flash` in [selftest.rs](../../src/selftest.rs) only reads key `0x01`, and writes, reads back, and removes a separate scratch key |

The log lines that mark each path are `Loaded {} devices from flash`,
`No paired devices in flash`, `Flash read error: {:?}`,
`Invalid or unsupported device store; writes disabled`,
`Device store is unreadable; refusing to overwrite stored bonds`,
`Device store exceeds serialization capacity; save aborted`,
`Flash write busy (attempt {}), retrying`,
`Flash write failed after {} attempts: {:?}`,
`Paired device store full - evicting oldest entry`,
`Bond refused: identity address is not public or random static; stored without keys`,
and `Paired device address has a reserved type; not stored`.

### Verification Status

- **Implemented:** everything in the table above.
- **Software-verified:** host tests cover the pure parts: frame and record
  validation in `framing.rs` and `record.rs`, and
  `failed_deletion_preserves_current_store_and_bonds`,
  `cancelled_persistence_never_publishes_candidate`, and
  `reconnect_events_are_suppressed_until_matching_cancellation_ack` in
  `management.rs`. They passed on GitHub-hosted runners in push runs
  36441995385 (`8a04b25`, 2026-09-28) and 37932436721 (`7fc99d6`, 2026-10-09)
  and scheduled run 37338711407 (2026-10-05). Since 2026-10-10 the 31 tests
  in `storage/devices_tests.rs` and `devices_format_tests.rs` also cover loading valid, legacy, malformed,
  and unreadable stores (each invalid one refusing saves until reset),
  identity merge, bond replacement, eviction, Forget and reset candidates
  published only after a successful save, the record codec, and, for every
  address type, that a save keeps a bond only when a reload accepts it. The
  `storage.rs` shell, the enrollment save, and the bonder depend on SoftDevice
  types and are not in the host library
  ([ADR 0003](0003-pure-core-and-task-shell.md)), so no automated test covers
  the flash I/O, the type conversion, or the enrollment window.
- **Hardware-verified:** not yet. The repository holds no board record of the
  self-test flash stage, persistence across a reboot, Forget, Factory reset,
  or a failed write.

## Related

- [Data model: pairing store](../data-model.md#pairing-store) and
  [schema change rules](../data-model.md#schema-change-rules)
- [Architecture: pairing and storage](../architecture.md#pairing-and-storage)
- [Security: key storage and deletion](../security.md#key-storage-and-deletion)
- [Operations: saved devices and storage](../operations.md#saved-devices-and-storage)
- [Features: manage saved devices](../features.md#manage-saved-devices)
- [ADR 0003: Hardware-free decision modules](0003-pure-core-and-task-shell.md)
- [ADR 0005: Two slots and independent endpoints](0005-two-slots-and-independent-endpoints.md)
- [ADR 0010: Static memory layout](0010-static-memory-layout.md)
- [ADR 0011: Interim Just Works pairing](0011-interim-just-works-pairing.md)
