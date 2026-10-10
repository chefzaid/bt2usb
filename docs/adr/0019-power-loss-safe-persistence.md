# ADR 0019: Commit Pairing-Store Writes With A Generation Anchor And Migrate Formats Only By Rewriting

- Status: Proposed
- Date: 2026-10-10
- Would amend: [ADR 0006](0006-fail-closed-pairing-store.md) when Accepted
  (frame version 2, a second map key, and unreadable-store recovery)

## Context

The bridge is bus-powered with no battery or hold-up capacitor
([ADR 0012](0012-bus-powered-no-system-off.md),
[hardware](../hardware.md#power)), so removing VBUS stops it within the
unmeasured decay time of the board's decoupling. Power cuts are routine: a hub
switched off with the monitor panel, a KVM that drops VBUS when it switches
(general knowledge; some do), a cable pulled right after pairing. A keyboard
works in BIOS setup only if its bond survives every cut; lost bonds force
re-pairing, and a rollback can restore a peer the user forgot.

**The store today** (paths in `src/`, as of 2026-10-10)
is ADR 0006's single item: key `0x01` (`KEY_PAIRED_DEVICES` in `storage.rs`), at most 512
bytes (`MAX_RECORD_SIZE` and its compile-time assertion in
`storage/devices.rs`), `NoCache`, in pages 240 to 243
(`STORAGE_FLASH_PAGE_START` to `STORAGE_FLASH_END` in `config.rs`; the
`ASSERT` on the `FLASH` end in `memory_sd.x`), holding the W1 frame
`B2 01 count records` (`MAGIC` and `VERSION` in `storage/framing.rs`) without
a checksum of its own. The host-tested `DeviceList` decides: load fails closed
(`load`, `decode`, and `decode_versioned` in `devices.rs`, legacy parser
`decode_legacy` included), and a reset of an unreadable store asks for an
erase (`erase_first` in `DeviceList::reset`). The
`storage.rs` shell does the I/O: a save rewrites the frame, three attempts
20 ms apart (`save_to_flash`), from a word-aligned 512-byte `FlashBuffer`
(a plain `[u8; 512]` until this review; see below); the reset erases the four
pages, then writes (`factory_reset`). `ble_task` is
the only writer (`ble/multi_conn.rs`: load at the start of `ble_task`, Forget
and reset in `manage_devices`, enrollment in the `Action::PersistDevice` arm
of `execute_action`); the self-test image also writes and
removes key `0xFE` (`SELFTEST_KEY` and `check_flash` in `selftest.rs`).

**The flash path.** `vendor/nrf-softdevice/src/flash.rs` issues
`sd_flash_write` (`Flash::write`) or one `sd_flash_page_erase` per page
(`Flash::erase`), awaits the SoftDevice event (the flash arms of
`on_soc_evt` in `events.rs`), refuses an unaligned source buffer
(`FlashError::BufferMisaligned` in `write`), implements `MultiwriteNorFlash`,
and panics if the future is dropped (the `DropBomb` in `write` and
`erase`). The pinned bindings say exactly one event follows, an error event means
"the command could not be started", and all interrupts are blocked during the
NVMC operation. `embedded-storage-async` 0.4.2 states that a cut erase leaves
the page undefined and a cut write leaves the written words undefined and the
rest of the page unchanged (`nor_flash.rs` lines 36, 47 to 48, 88 to 92).
From general knowledge, to confirm against the revisions in use: the S140
SoftDevice Specification ("Flash memory API") schedules these operations
around radio activity, hence failures while links are busy, and the nRF52840
Product Specification (NVMC) allows two writes per word between erases, about
41 µs per word, about 85 ms per page erase, and 10,000 erase cycles.

**What `sequential-storage` 7.2.0 guarantees** (the `sequential-storage` entry
in `Cargo.toml` asks for `"7"`; its `Cargo.lock` entry pins 7.2.0). Its README
calls it "Power-fail
safe" ("the system is always fine or fully recoverable") with automatic repair,
and warns that a cancelled operation "might or might not have fully happened".
An item is a data CRC-32, a length, and a length CRC-16, written header first
(`item.rs` lines 5 to 21, 273 to 334); iteration skips corrupted and erased
items (lines 584 to 586) and a torn header word by word (lines 641 to 643);
`fetch_item` returns the newest valid copy, looking back into closed pages
(`map.rs` lines 296 to 331), so a torn write yields the previous value.
Collection copies the oldest page's live items before erasing it (lines 503
to 509, 782 to 843); an interrupted migration is redone from the intact source
(`try_repair`, lines 855 to 881) inside the next fetch or store (lines 172 to
175), so even a load can erase a page. It does **not** promise:

- to report decay: a decayed newest copy is skipped and an older one returned,
  and no public API lists skipped items, so after a Forget that older copy
  still holds the forgotten bond until its page is collected;
- anything about cells left weak by an interrupted erase;
- that items fit: every item a fetch walks, whatever its key, must fit the
  caller's buffer, or the fetch fails (`BufferTooSmall`, `item.rs` lines 138 to
  140);
- a stable layout: a layout change is a major release, and a minor release may
  write data older minors cannot read. Majors 3.0 to 7.0 each state disk
  compatibility with their predecessor; upstream 8.0.0 (2026-07-08) says
  "This release is 'disk'-compatible with 7.0", and the 7.2.0 and 8.0.2
  sources share the item header, CRC functions, and page markers. 8.0.1 fixed
  "a canceled map remove item, after a canceled map item store" that could "in
  fringe cases allow a map item fetch to return old data": 7.2.0 takes page 0
  as the newest when no page is partial-open (`map.rs` lines 595 to 599),
  8.0.1 the last closed page. Its power-cut mock flash is behind the private
  `_test` feature.

So a decayed newest frame loads an older store silently, an interrupted
recovery erase can leave older pages that load as valid, no rule covers a new
frame version or container upgrade, no test drives the flash shell, a cut
write, collection, or recovery. The crate also writes item bodies straight
from `buf` (`item.rs` lines 294 to 304); this review found the shell passing a
plain byte array there, whose alignment the type leaves open, and the
`FlashBuffer` fix landed on 2026-10-10 in `0428914`.

## Decision

**Keep `sequential-storage` and its atomic item append as the commit
mechanism; add a frame CRC and a generation, and confirm each generation in a
second key.**

```text
Key 0x01, frame version 2 (W2), at most 383 bytes
[0]      magic         0xB2
[1]      version       0x02
[2..6]   generation    u32 little-endian, 1 to 0xFFFF_FFFE
[6]      record count  0..=MAX_PAIRED_DEVICES
[7..n]   records       [len: u8][record], record layout unchanged
[n..n+4] frame CRC     CRC-32C of bytes 0..n

Key 0x02, generation anchor, 13 bytes
[0]      version       0x01
[1..5]   generation    of the frame it confirms
[5..9]   frame CRC     copied from that frame
[9..13]  anchor CRC    CRC-32C of bytes 0..9
```

CRC-32C is the reflected Castagnoli CRC with initial and final value
`0xFFFF_FFFF` (check value `0xE306_9283` for ASCII `123456789`), computed
bitwise in a pure module. It repeats the crate's protection at rest, binds the
anchor to one exact frame, and keeps a frame self-checking outside the
container (golden images, a future staged migration).

**Commit protocol.** Let *g* be the durable generation (0 after loading an
empty, legacy, or W1 store).

1. Serialize the candidate as a W2 frame with generation *g* + 1; abort before
   touching flash if it does not fit, as today.
2. If the anchor is pending, write it first, so frame *g* + 2 is never written
   while the anchor says *g*; if it cannot be written, the save fails with
   `StoreError::Flash` before the frame is touched.
3. Write the frame under `0x01` with the existing three attempts, reusing the
   same bytes, then read it back with `fetch_item` and compare; a mismatch is a
   failed attempt. On flash the frame commits when its last data word is
   written: from then on a boot loads *g* + 1. **A verified frame is the
   publication point:** *g* becomes *g* + 1 in RAM and the anchor becomes
   pending.
4. Write the anchor under `0x02` with the same retries; if that fails, the
   operation has still committed and the anchor follows at the next save or
   boot. Only then does `management::commit` publish the candidate and the UI
   report success. A failed step 3 is `StoreError::Flash`. A failed write or a
   mismatch leaves the previous frame as the newest valid copy; a read-back
   that fails with a crate error after a complete write leaves the outcome to
   the next boot (see Consequences).

**Load.** One pure function decides from the two fetched keys:

| Key `0x01` | Key `0x02` | Outcome |
| --- | --- | --- |
| Absent | Absent | Empty, writable, *g* = 0 |
| Absent | Valid | Unreadable: a confirmed store is missing |
| Legacy or W1, valid | Absent | Loaded, writable, *g* = 0; W2 from the first save |
| Legacy or W1, valid | Valid | Unreadable: rolled back past a W2 commit |
| W2 *g*, CRC *c* | Absent with *g* = 1, or generation *g* − 1 | Loaded; anchor written at once |
| W2 *g*, *c* | *g*, *c* | Loaded |
| W2 *g*, *c* | Absent with *g* ≥ 2, *g* with another CRC, above *g*, or below *g* − 1 | Unreadable: rolled back or out of protocol |
| Malformed, future version, generation 0 or `u32::MAX`, count above `MAX_PAIRED_DEVICES` | Any | Unreadable ([ADR 0006](0006-fail-closed-pairing-store.md)) |
| Any | Malformed, or anchor version other than 1 | Unreadable |

Step 2 means every frame from *g* = 2 on follows a durable anchor, so only
generation 1 may lack one. A crate `Error::Storage` at boot (a SoftDevice
operation failing inside the crate's repair) is retried three times 20 ms
apart; other crate errors are Unreadable at once. Unreadable keeps ADR 0006's
behavior (no ordinary write, `Storage failed`, Factory reset), and the log
line names the row. Bond keys thus load only from a frame whose structure,
CRC, and anchor all check; no frame is repaired or partly loaded.

**Garbage collection stays in the crate.** Both keys are rewritten on every
commit, so their live copies sit in the newest pages and a migration normally
copies nothing; the fault sweep covers the cases where it does.

**Recovery of an unreadable store** (confirmed Factory reset). If the map can
be walked (only its contents were rejected), call `remove_all_items`, which
marks items erased oldest page first and erases no page itself. A cut leaves
the store Unreadable: the newest copies go last, and a missing frame beside a
surviving anchor is itself Unreadable. If the crate still reports an error
after its repair, erase the pages one at a time, oldest first, ordered by the
page markers the crate's README documents (the partial-open page is the
newest), or in index order if the markers are inconsistent. Then commit an
empty W2 frame with generation 1; a cut between the last removal or erase and
that commit leaves no live item, which loads as empty, the reset the user
asked for.

**Wire versions.**

| Version | Written by | Loaded by this design | Loaded by older firmware |
| --- | --- | --- | --- |
| W0 (legacy, no magic) and W1 (`B2 01`) | before `c37d568`, and from it until this change | Yes; W2 from the first save | Not applicable |
| W2, `B2 02` plus anchor | this change | Yes | From `2479c79`: future version, fails closed; reflash W2 firmware or Factory reset, which erases the region. Before `2479c79`: unsupported, may overwrite |
| W3 and later | future | Fails closed | Not applicable |

Every layout change bumps the version byte, keeps the magic `0xB2`, and keeps
readers for older versions still in use; no firmware writes an older version,
so a downgrade means reflashing or a reset. The anchor format changes only
with a new key. Raising `MAX_PAIRED_DEVICES` keeps older stores loadable;
lowering it makes a larger store Unreadable instead of truncating it.

**Container versions.** Pin the crate exactly (`=8.0.2`) so `cargo update`
cannot change the on-flash writer; any version change is a storage change
under the [ADR trigger](../architecture.md#adr-process), adopted only if its
changelog states disk compatibility with the pinned one and the cross-version
golden-image tests pass both ways. No version that changes the layout reads
the old pages in place (question 3); a staged migration to spare pages
(`memory_sd.x` leaves `0xF4000` to `0x100000` unused, as the comment on its
`FLASH` region says) needs its own
ADR. Moving to 8.0.2 brings the 8.0.1 fix that recovery through
`remove_all_items` relies on (`NoCache` becomes `Cache::new_uncached()`);
8.0.2 (2026-10-01) leaves 8.0.1's map code unchanged and is what Dependabot's
pull request #9 proposes. Every writer of pages 240 to 243 keeps each item,
key byte included, within `MAX_RECORD_SIZE` (asserted at compile time for the
frame and the anchor), so the persistent-settings ADR picks another key under
the same limit; passes 4-byte aligned buffers (`FlashBuffer`, as the load,
the save, and the self-test already do); and
never cancels a storage future.

### Open Questions For The Owner

1. **Commit protocol:** `anchor` (recommended), `crate-only` (frame CRC and
   generation, no anchor), or `dual-copy` (raw A/B pages without the crate).
2. **Container version for this change:** `8.0.2` (recommended) or `7.2.0`.
3. **A future container release that changes the layout:** `hold` (stay on the
   last compatible release until a staging area exists; recommended), `reset`
   (adopt it with a notice and a confirmed Factory reset), or `migrate` (read
   old, erase, write new in place).
4. **Rewriting a W0 or W1 store as W2:** `lazy` (at the first save a change
   triggers; recommended) or `eager` (at the first boot of the new firmware).

## Alternatives Considered

- **Crate only.** A frame CRC and generation without an anchor survive torn
  writes, but a decayed newest frame still loads an older store silently, one
  that after a Forget holds the forgotten bond.
- **Explicit A/B copies in raw pages.** New code replaces a fuzzed, deployed
  container and needs its own repair and proof; it erases a page per commit
  (about ten times today's rate, each erase blocking the CPU) and still rolls
  back silently when the newest bank decays, unless every commit also erases
  the older bank.
- **A write-ahead journal.** The crate is already an append-only log with
  atomic appends; an intent record doubles the writes to add only rollback
  detection, which the 13-byte anchor gives.
- **Alternating keys retired with `remove_item`.** Also detects rollback, but
  every commit walks the region and rewrites headers, and it relies on the
  interruption behavior 8.0.1 had to fix.
- **Power-fail warning or hold-up energy.** `sd_power_pof_enable` or
  `PowerUsbRemoved` (`softdevice_task` in `main.rs`) could stop new operations
  from starting but cannot finish one in progress; a capacitor that covers a
  page erase changes every board. Worth adding only if the bench shows margin.
- **In-place migration of a changed container layout.** Between the erase and
  the rewrite the bonds exist only in RAM; a cut there leaves a blank region
  that loads as an empty store, which is silent loss.

## Rationale

The crate already makes each append atomic and repairs interrupted collection;
it cannot report a skipped newer copy, which the anchor supplies through the
same atomic append for 24 bytes per commit. The commit point stays one append,
so ADR 0006's contract (publish after the write, fail closed on doubt) holds
and every interrupted operation ends in the old or the new state. The version
byte already makes older firmware fail closed, and recovery follows the
crate's own oldest-first order.

## Consequences

Positive:

- A cut anywhere in a save, Forget, reset, or collection loads the last
  committed store or the new one, and a cut in recovery leaves it Unreadable
  or empty (the sweep below is the evidence for the pinned crate version); a
  forgotten peer cannot return through decay of the newest frame alone; a
  dependency update cannot change the writer unreviewed.
- The KVM, BIOS, and no-host-software promise is served directly: bonds
  survive hub and KVM power cuts, and every recovery stays on the bridge.

Negative:

- Decay that today loads an older store silently will show `Storage failed`
  and need a Factory reset and re-pairing. Once a store is W2, rolling back to
  earlier firmware costs a Factory reset or a reflash of W2 firmware.
- Rollback by one step stays possible if the new frame decays during the
  anchor write that follows it (a window of milliseconds), or if the newest
  frame and the newest anchor both decay.
- A read-back that fails with a crate error after a complete frame write
  reports `Storage failed`, yet the next boot loads the new frame: a Forget
  reported as failed can take effect after a reboot.
- Recovery of a region the crate cannot walk, with inconsistent page markers,
  may expose an older frame after a cut, rejected only if a newer anchor
  survives. Ordering by page markers couples recovery to the crate's layout;
  golden images guard it.
- Estimates: 1.5 to 2.5 KiB of code, about 12 bytes of static RAM, no new stack
  buffers (anchor and read-back reuse the two 512-byte save buffers), and for a
  full store nine commits per page instead of ten, so each page is erased once
  per 36 commits instead of 40 and 10,000 cycles last about 360,000 commits.
  The second fetch adds well under 5 ms at boot.
- Logical deletion is still not physical erasure
  ([security](../security.md#key-storage-and-deletion)); ADR 0021's proposed
  scrub after every Factory reset works oldest page first too, so it composes
  with this recovery.

Follow-up: ADR 0020 (proposed) treats a reset during a flash operation as a
power cut and sizes its storage grace from the worst measured operation; the
persistent-settings ADR keeps the key and size rules; "Visible
storage/security errors" separates a rollback from an unsupported version.

## Implementation

| Concern | Where |
| --- | --- |
| CRC-32C | New pure `src/storage/crc.rs` |
| W2 writer and reader, version dispatch | `src/storage/framing.rs`, and `DeviceList::encode` and `decode` in `src/storage/devices.rs` |
| Anchor | New pure `src/storage/anchor.rs` |
| Load table, commit steps, recovery order | New pure `src/storage/commit.rs` |
| Generation and pending anchor | `DeviceList` in `src/storage/devices.rs` |
| Map access over any `NorFlash + MultiwriteNorFlash`, borrowing the flash (`embedded-storage-async` 0.4.2 implements `MultiwriteNorFlash` for `&mut T`, `nor_flash.rs` line 94) | New `src/storage/region.rs`, free of SoftDevice types |
| SoftDevice conversion, logging, calls into `region.rs` | `DeviceStore` in [storage.rs](../../src/storage.rs) |
| Crate pin and API | `Cargo.toml`, `Cargo.lock`, `check_flash` in [selftest.rs](../../src/selftest.rs) |
| Host build | The `storage` module in [lib.rs](../../src/lib.rs) gains `crc`, `anchor`, `commit`, and `region`; dev-dependencies `sequential-storage =8.0.2`, `embedded-storage-async`, and a renamed `=7.2.0` |
| Board fault hook | Firmware feature `storage-fault`: the fault adapter over the real flash and a GPIO pulse at each commit |

Host tests cover the CRC check value; W2 round trips, every truncation and
single-bit flip, generations 0 and `u32::MAX`, oversized counts, future
versions, and W2 never read as legacy; every load-table row and all generation
pairs from 0 to 5; and W0, W1, and W2 golden images (synthetic peers only,
since images hold keys) read by both crate versions. A fault mock with the
nRF52840 geometry (4-byte words, four 4 KiB pages) fails on a third write to a
word between erases or an unaligned buffer. For each scenario (enrollment,
Forget, readable reset, both recovery paths, anchor completion, W0 and W1
migration), at every fill level that moves a save across a page boundary, it
cuts power before every word write and erase step (clean, a random subset of a
word's 1-to-0 bits, or a page partly reset to `0xFF`) and reboots. From a
readable store it asserts the exact old or new store (new only once the
frame's last word is written), never a mix or Unreadable, and a working next
save; the recovery scenarios must end Unreadable or empty, never with a bond
loaded. It adds a second cut in the next boot's repair, transient errors at
every step, and every single-bit flip of a committed image's newest frame and
anchor, which must load the committed store or Unreadable, never an older one.
By estimate that is 10⁵ to 10⁶ simulated boots; the run time is unmeasured, so
the full sweep may need its own CI job. Renode adds nothing: the simulation
build has no storage stack.

Board evidence ([hardware-result record](../testing.md#hardware-acceptance-evidence)):
a DK powered only from the nRF USB port through a switchable hub port or relay,
cut at a swept delay after the `storage-fault` pulse, about 1,000 cycles per
scenario, each boot logging generation and outcome over RTT; unplugs during
Forget and both resets, giving the
[first-flash](../first-flash.md#6-device-management-and-degraded-display)
"Failure reporting" check its fixture; and scope measurements of VBUS removal
to reset and of erase time with two links.

This unblocks, in [TODO.md](../../TODO.md#pairing-storage), "ADR:
power-loss-safe persistence and storage migration" (once Accepted),
"Power-loss-safe persistence", "Versioned storage migration", and
"Forget/reset hardware and interruption acceptance"; and the
`sequential-storage` part of "Major dependency upgrades"
([TODO.md](../../TODO.md#release-provenance-and-supply-chain)), whose 8.0.2
pull request #9 fails embedded Clippy today.

### Verification Status

- **Implemented:** nothing of this proposal. Today: the W1 frame under key
  `0x01`, fail-closed load, the legacy parser, three write attempts, and
  erase-then-write recovery, on `sequential-storage` 7.2.0 with `NoCache`.
- **Software-verified:** nothing of this proposal. Today 8 framing, 3 record,
  and 27 device-list host tests (`storage/devices_tests.rs` and
  `devices_format_tests.rs`: valid, legacy,
  malformed, and unreadable loads, merge, eviction, Forget and reset
  candidates, codec) and the `commit` tests in `ble/management.rs`; nothing
  tests the flash shell, a cut write, collection, or recovery
  ([testing](../testing.md#modules-without-host-tests)).
- **Hardware-verified:** not yet. No board record covers persistence across a
  reboot, a power cut during a write, or recovery.

## Related

- [Data model: pairing store](../data-model.md#pairing-store), [load rules](../data-model.md#load-rules), [write rules](../data-model.md#write-rules), [schema change rules](../data-model.md#schema-change-rules)
- [Security: key storage and deletion](../security.md#key-storage-and-deletion); [deployment: storage compatibility](../deployment.md#storage-compatibility); [operations: storage unreadable](../operations.md#storage-unreadable-and-writes-disabled); [hardware: power](../hardware.md#power), [memory layout](../hardware.md#memory-layout)
- [Architecture: pairing and storage](../architecture.md#pairing-and-storage), [Forget and factory reset](../architecture.md#forget-and-factory-reset); [testing: hardware acceptance evidence](../testing.md#hardware-acceptance-evidence)
- TODO.md: [pairing storage](../../TODO.md#pairing-storage), [platform, memory and recovery](../../TODO.md#platform-memory-and-recovery) (watchdog), [release, provenance and supply chain](../../TODO.md#release-provenance-and-supply-chain) (dependency upgrades), [shared decisions](../../TODO.md#shared-decisions) (settings)
- [ADR 0003](0003-pure-core-and-task-shell.md), [ADR 0004](0004-layered-verification.md), [ADR 0006](0006-fail-closed-pairing-store.md), [ADR 0010](0010-static-memory-layout.md), [ADR 0012](0012-bus-powered-no-system-off.md); [ADR 0020](0020-watchdog-and-progress-based-recovery.md) and [ADR 0021](0021-provisioning-debug-access-and-readout-protection.md) (both proposed)
