# ADR 0021: Lock The Debug Port In Production Images And Erase The Pairing Pages On Factory Reset

- Status: Proposed
- Date: 2026-10-10
- Would amend: [ADR 0006](0006-fail-closed-pairing-store.md) (Factory reset
  erases the pages) and [ADR 0008](0008-attested-draft-releases.md) (package
  contents and checks) when Accepted

## Context

The bridge sits within reach of anyone at the desk, and every nRF52840 board
exposes SWD. A bond's LTK lets its holder decrypt that link's recorded traffic
or impersonate either side ([security: assets](../security.md#assets)), for
example connecting to the user's keyboard as the bridge to receive its typing.
The release gate "key deletion and physical-access policy"
([deployment](../deployment.md#release-gates)) waits on this decision. Facts
are from the tree at `eeae4b8`.

**Debug access today.** The bridge ([main.rs](../../src/main.rs) lines 181 to
184) and the self-test ([selftest.rs](../../src/selftest.rs) lines 115 to 118)
pass `embassy_nrf::config::Config::default()` to `embassy_nrf::init` before
`Softdevice::enable`; no Nordic MDK `SystemInit` runs, so `init` alone touches
access port protection (APPROTECT). In the pinned `embassy-nrf` 0.7.0
(`src/lib.rs`), the default `Debug::Allowed` (line 581) reads the build-code
letter from FICR `INFO.VARIANT` (`0x1000_0104`, line 716) and, from `F` on
(`chips/nrf52840.rs` line 10), programs UICR `APPROTECT` (`0x1000_1208`) to
`0x5A` when it differs (line 727) and writes `0x5A` to `APPROTECT.DISABLE`
(`0x4000_0558`, line 730) at every boot. `Debug::Disallowed` writes only
`0x00` to UICR `APPROTECT` on the nRF52; its `FORCEPROTECT` write is compiled
only for `nrf9120-s` (lines 829 to 851). UICR is written only to clear bits
(line 655), followed by one `SCB::sys_reset` (line 928), a soft reset. Every
image keeps SWD open
([security](../security.md#physical-access-and-debug-port)).

**What the chip offers** (nRF52840 Product Specification, chapter "Debug and
trace", section "Access port protection", online edition read 2026-10-10):

- Build codes D and earlier (revisions 1 and 2, `Cx0` and `Dx0` in Nordic's
  nRF52840 compatibility matrix): off by default, on after any reset once UICR
  `APPROTECT` holds Enabled (`0x00`). Nordic's informational notice IN-133
  (v1.0) reports that fault injection can bypass APPROTECT on nRF52 Series
  devices and names preventing physical access, or detecting and responding to
  enclosure breach, as the mitigations.
- Build codes F and later (revision 3, `Fx0`; Nordic's blog "Working with the
  nRF52 Series' improved APPROTECT"): on by default. UICR HwDisabled (`0x5A`)
  turns the hardware part off from the next reset of any type, and firmware
  must also write SwDisable (`0x5A`) to `APPROTECT.DISABLE`. That register is
  cleared only by a pin, power-on, brownout, or watchdog reset or a wake from
  System OFF, not by a soft reset. "For additional security" the specification
  recommends Enabled in UICR plus a firmware write of Force to
  `APPROTECT.FORCEPROTECT` (`0x4000_0550`, Force `0x00` in `nrf-pac` 0.1.0),
  which any reset clears; the blog warns that a device without that firmware
  step "may appear locked but it will not be completely protected".
- CTRL-AP `ERASEALL` erases flash, UICR, and RAM; on revision 3 the port
  then stays open until one of those same resets (a watchdog reset only
  outside Debug Interface mode), so not after the soft reset `init` performs.
  FICR `ER` and `IR` (`0x1000_0080`, `0x1000_0090`) and UICR `CUSTOMER` are as
  readable as flash through an open port (general knowledge). The development
  boards' build codes are not recorded, and the matrix's SoftDevice table
  appears to list S140 7.3.0 for revisions 2 and 3 but groups its rows
  ambiguously, so revision 3 support is unconfirmed.

**Keys at rest.** A bond record holds EDIV, Rand, LTK, key flags, IRK, and
identity address in plain form ([codec.rs](../../src/storage/codec.rs)) in the
frame under key `0x01` (`KEY_PAIRED_DEVICES` in [storage.rs](../../src/storage.rs)), pages 240
to 243; RAM copies in `Bonder` ([bonder.rs](../../src/ble/bonder.rs) line 39)
and `DEVICE_STORE` in `storage.rs` are not zeroized. Each save appends a
frame (`store_item` in `save_to_flash`): `forget` one without the record,
`factory_reset` an empty one. The host-tested `DeviceList::reset`
([devices.rs](../../src/storage/devices.rs) lines 257 to 266) asks for the
pages to be erased first (`erase_first` in `factory_reset`) only when the store
is unreadable; `manage_devices` then clears the bonder
([multi_conn.rs](../../src/ble/multi_conn.rs) lines 292 to 303).
`sequential-storage` 7.2.0 only "logically" overwrites on `store_item`
(`map.rs` lines 368 to 372), `remove_item` only zeroes an item's CRC
(`erase_data`, `item.rs` lines 197 to 207), and its README (lines 136 to 139)
warns that "the data itself is still stored on the flash". Pages are erased
only by collection (`open_page`, `lib.rs` lines 207 to 225) or `erase_all`
(`map.rs` line 886), and `fetch_item` takes the lowest-index partial-open page
as the newest (`map.rs` lines 260 to 264). By estimate, a superseded copy
survives about three pages of saves: 30 for a full store (384-byte items: the
8-byte header of `item.rs` line 45, the key, a 375-byte frame), 1,000 for an
empty one (12 bytes). Saves happen only on changes, so the LTKs a Forget or a
readable-store Factory reset removed can stay readable for the life of the
unit.

**Flashing and release today.** `mask deps` installs `probe-rs-tools` 0.32.0,
which the Cargo runner and `mask softdevice` use. Its nRF52 sequence
(`probe-rs/src/vendor/nordicsemi/sequences/nrf52.rs` at `v0.32.0`) reads
CTRL-AP `APPROTECTSTATUS` and runs `ERASEALL` on a locked chip only with
erase-all permission (`--allow-erase-all`). `stage_build` in
[release.py](../../scripts/release.py) ships `bt2usb.elf`,
`bt2usb-selftest.elf`, and `bt2usb.hex` and records `profile` and `features`
as fixed strings; nothing shipped states a debug policy.
[ADR 0018](0018-production-usb-identity.md) (Proposed) adds the first such
record, for the USB identity.

## Decision

**Lock the debug port from the production image itself, holding the policy as
data** so that locked and open images share every instruction:

```text
Debug policy record, 16 bytes in .rodata, read once with read_volatile
[0..8]   magic   ASCII "BT2USBDP"
[8]      format  0x01
[9..12]  zero
[12..16] policy  u32 little-endian: 0x4E45_504F (ASCII "OPEN") opens the port;
                 production carries 0x4B43_4F4C ("LOCK"); anything else locks
```

`build.rs` reads `BT2USB_DEBUG_POLICY` (unset or `open`, or `locked`; else the
build fails) and emits the declared cfg `bt2usb_debug_locked`, which selects
only the policy word; local and `mask` builds stay open. Before
`init`, `main` reads the record (one documented `unsafe` volatile read) and
FICR `INFO.VARIANT` and applies the pure `provisioning::boot_plan`:

| Policy | Build code | `nrf_config.debug` | After `init`, before `Softdevice::enable` |
| --- | --- | --- | --- |
| Open | Any | `Debug::Allowed`, as today | Nothing |
| Locked | `F` to `Z` | `Debug::Disallowed` | Write Force (`0x00`) to `APPROTECT.FORCEPROTECT` |
| Locked | `A` to `E`, or not a letter | `Debug::Disallowed` | Nothing; legacy protection only |

The Force write precedes `Softdevice::enable`, which restricts the POWER and
CLOCK peripherals at the base address the APPROTECT registers share
(`0x4000_0000` in `nrf-pac`; S140 SoftDevice Specification, general
knowledge), so whether it would block the write does not matter. `main` logs
`debug port: {} (build code {})`. The self-test keeps `Config::default()` and
no record, so bring-up stays debuggable (a locked unit takes it only after
`ERASEALL`); the simulation is unchanged.

**Ship a locked production image and an open service twin.** The `embedded`
job builds the bridge unset and with `BT2USB_DEBUG_POLICY=locked`.
`release.py stage` requires one record in each bridge ELF's loadable segments
and none in the self-test, `LOCK` in production and `OPEN` in the twin,
loadable bytes that differ only there, and a HEX equal to the production
bytes, and records `debug_policy` in `BUILD-INFO.json`; `package` rereads the
files and refuses otherwise. The package gains `bt2usb-service-<tag>.elf` and
`.hex`; drafts are hardware-tested on the twin. With ADR 0018, one ELF reader
and one schema bump serve both records.

**Provision from a full erase, end with a power cycle, and prove the lock**,
as the deployment guide and a `provision` recipe in `maskfile.md` (which first
checks the HEX for `LOCK`) will do:

1. `probe-rs erase --chip nRF52840_xxAA --allow-erase-all` (`ERASEALL` when
   locked): firmware, SoftDevice, and bonds are gone. On an open chip it uses
   the flash algorithm; whether that clears UICR is unchecked but harmless,
   since the locked image's UICR write only clears bits.
2. Read FICR `INFO.VARIANT` with `probe-rs read`; stop if question 3 excludes
   the build code.
3. Download S140 7.3.0 (as `mask softdevice` does) and `bt2usb-<tag>.hex`, and
   check both with `probe-rs verify`.
4. Reset, wait for enumeration (the first boot writes UICR and resets once),
   then remove all power, since access that `ERASEALL` opened outlives the
   soft reset; the blog asks for such a reset before a device ships.
5. A `probe-rs read` without `--allow-erase-all` must fail for lack of
   erase-all permission. Record tag, HEX and SoftDevice SHA-256, build code,
   USB serial, date, and operator.

A unit that has only held its owner's bonds may take the production HEX over
its open image and keep them (steps 3 to 5); any other unit starts at step 1.

**Keep bond records unencrypted.** Locked, the port closes flash, FICR, UICR,
and RAM together; opened by fault injection, it opens them together, so any
on-chip key arrives with the ciphertext. Invasive attacks (fault injection,
decapsulation) are out of scope, as IN-133's advice implies.

**Erase the pairing pages on every Factory reset.** The pure
`DeviceList::reset` returns the steps: erase then commit for an unreadable
store, as today, which leaves only the empty frame; commit, invalidate, then
erase for a readable one. After the commit and `bonder().clear()`,
`DeviceStore::scrub` calls the crate's `remove_all_items`, which zeroes every
item's CRC, oldest page first and newest copy last (`remove_item_inner`,
`map.rs` lines 578 to 655; the vendored `Flash` is `MultiwriteNorFlash`,
`vendor/nrf-softdevice/src/flash.rs` line 189), then erases the four pages
with the region erase recovery already uses (one `sd_flash_page_erase` per
page, ascending, line 158). Invalidating first makes a cut safe: a cut erase
leaves the page undefined (`embedded-storage-async`, as
[ADR 0019](0019-power-loss-safe-persistence.md) cites), and an older page left
partial-open with intact frames at a lower index than the newest would be read
first and load a removed bond. With every CRC zeroed, a cut at any step loads
the empty store, or Unreadable for inconsistent markers (`lib.rs` lines 186
to 197), short of a CRC-32 collision; afterwards all 16 KiB read `0xFF`.
Success is reported after the last step; a later failure reports
`StorageFailed` with the empty store in effect and the residue left for the
next Factory reset. An all-erased region also loads as empty under ADR 0019.
Forget, eviction, and re-pairing stay logical; their residue waits behind the
lock.

**Key lifetime, disposal, and service.** A key lives from pairing until Forget,
eviction, re-pairing, Factory reset, or `ERASEALL`; no timer expires it (the
Bluetooth Core defines no LTK lifetime; general knowledge), and the peripheral
keeps its copy until its bond is removed there. Before a unit changes hands or
is serviced or discarded, the owner runs Factory reset and removes the bridge
from each peripheral; a unit that no longer boots stays locked until the bench
erases it. Service never reads a locked unit: it diagnoses from the OLED and
the host, runs step 1, installs the SoftDevice and the twin for RTT, and
provisions again after repair. Updating a locked unit loses the bonds the same
way until signed DFU exists, whose ADR must keep the lock and the bonds.

### Open Questions For The Owner

1. **Production debug port:** `lock` (recommended) or `open` (keys readable
   with any probe; updates keep bonds).
2. **Service image in each release:** `twin` (recommended) or `none` (only the
   locked image ships; drafts are tested on an unattested CI build).
3. **Chip revision for production units:** `rev3` (build code F or later;
   recommended) or `any` (older chips get legacy protection only).
4. **Bond encryption at rest:** `none` (recommended) or `ficr` (AES with a key
   derived from FICR `ER`, against leaked dumps of the pairing pages only).

## Alternatives Considered

- **Keep every image open.** Updates keep their bonds, but anyone with a probe
  and a minute copies every LTK, and the release gate stays open.
- **Lock with the probe only** (`probe-rs write`, or an untried UICR section
  in the HEX). Protection rests on a manual step the image does not show: a
  unit erased and reflashed with the same release comes back open, and the
  Force write still needs firmware.
- **A compile-time policy without a record** (a Cargo feature, or ADR 0018's
  channel). Features are easily set locally and `stage` records them as a
  fixed string; a constant policy makes the images differ in code, so tests on
  one do not cover the other, and the check could only disassemble.
- **Patch the production ELF into the twin.** Identical code by construction,
  but the twin is no compiler output of the tagged source, and one tool would
  write and check the only difference.
- **Encrypt bonds with a key from FICR or UICR `CUSTOMER`.** An open port
  yields key and ciphertext together; it protects only a dump of the pairing
  pages taken without FICR, already sensitive under the
  [logging rules](../security.md#logging-and-privacy), for a frame version, an
  AES mode on `sd_ecb_block_encrypt`, nonces, and migration (2 to 4 KiB,
  estimated). A CryptoCell key behind an ACL-protected page (the nRF Connect
  SDK's hardware-unique-key scheme; general knowledge, unchecked) hides the
  key from compromised code, not from a probe that halts the core before the
  ACL is set, and needs an early boot stage and a CC310 driver.
- **Erase the pages in the crate's age order without invalidating first.**
  Fewer flash writes, but a cut erase can leave an older page partial-open
  with intact frames and a lower index than the newest, which 7.2.0 then reads
  first, so a removed bond could load.
- **Scrub after every Forget** by rewriting the frame until every older page
  is collected: power-safe, but up to about 1,000 appends and an extra erase
  per page per Forget, for residue already behind the lock.

## Rationale

The image that holds the keys closes the port at every boot, so no tool,
operator, or reflash of the same release leaves a production unit open, and
Nordic's recommended Force write comes with it. A policy record keeps both
images identical in code, so hardware tests of a draft cover what ships and
`release.py` checks the shipped bytes, as
[ADR 0008](0008-attested-draft-releases.md) trusts digests over descriptions;
the byte comparison also feeds "Reproducible firmware evidence". Encryption at
rest would move the attack from flash to FICR without raising its cost.
Factory reset is the operation meant for disposal, so it pays for erasure;
committing first makes the deletion durable at the first write, and
invalidating before erasing keeps ADR 0006's commit-then-publish contract and
ADR 0019's interruption guarantee through any cut. Every decision is a pure,
host-tested function ([ADR 0003](0003-pure-core-and-task-shell.md)).

## Consequences

Positive: a production unit's keys and firmware are closed to a probe short of
an invasive attack, and replacing its firmware erases the bonds first, so a
swapped image shows as a bridge with no saved devices. Factory reset leaves no
key material in flash, each provisioning record ties a unit to a tag, a HEX
digest, and a chip revision, and packaging refuses a production image that
would open the port.

Negative:

- Every update or service visit of a locked unit erases the SoftDevice and the
  bonds, so every peripheral pairs again, many only after their old bond is
  removed (peripheral-specific), until signed DFU exists.
- Locked units give no RTT logs, so field diagnostics need the OLED and a
  probe-free method. A developer who flashes the production HEX to a DK locks
  it; the file names and the deployment guide must say so.
- Forget leaves residue behind the lock; legacy chips stay open to the attack
  IN-133 describes, and revision 3 chips are assumed to be; RAM copies stay
  unzeroized. CI builds the bridge twice with fat LTO, and a
  non-deterministic build fails packaging.
- Estimates: about 1.5 KiB of flash and no static RAM; about 150 lines of
  Python shared with ADR 0018's reader; per Factory reset, up to about 1,000
  item-header rewrites (a region full of empty frames) and four page erases
  (about 85 ms each by the Product Specification's NVMC timing, general
  knowledge), with both links already closed.

Product promise: no descriptor, interface, or host-visible behavior changes,
so KVM switching, firmware setup, and the no-host-software rule are untouched;
recovery and diagnostics stay on the bridge and the bench, never on a host.
The bridge stays bus-powered; provisioning only adds one power cycle.

## Implementation

| Concern | Where |
| --- | --- |
| Policy word, record, build-code classes, `boot_plan` | New pure `src/provisioning.rs`, in the host library through `#[path]` like `ble/conn_params.rs`; tests in `src/provisioning_tests.rs` |
| Policy selection | `BT2USB_DEBUG_POLICY` and `bt2usb_debug_locked` in [build.rs](../../build.rs); a second build in the `embedded` job of `.github/workflows/ci.yml` |
| Boot shell | A `debug_port` helper called by `main` ([main.rs](../../src/main.rs)): record and FICR reads, `nrf_config.debug`, the Force write through `embassy_nrf::pac` (`unstable-pac` is enabled), the log line |
| Reset steps | `DeviceList::reset` in [devices.rs](../../src/storage/devices.rs) returns the ordered steps in place of `erase_first`; tests beside the existing reset cases in `devices_format_tests.rs` |
| Scrub and reset | `DeviceStore::factory_reset` and a new `scrub` in [storage.rs](../../src/storage.rs) (`remove_all_items`, then the region erase), run by `manage_devices` ([multi_conn.rs](../../src/ble/multi_conn.rs)) after `bonder().clear()` |
| Release check | `stage_build` and `package_release` in [release.py](../../scripts/release.py) with an ELF and HEX reader; cases in `scripts/release_test.py` |
| Procedures | A `provision` recipe in `maskfile.md`; provisioning and service in [deployment](../deployment.md#erase-behavior) and [operations](../operations.md#recovery-and-diagnostics) |

Host tests cover every policy-word path (OPEN opens; LOCK, `0`,
`0xFFFF_FFFF`, all 32 single-bit flips of OPEN, and a bad magic or format
lock), every build-code byte, `boot_plan` for every pair (never `Allowed` when
locked), and the reset steps for readable and unreadable stores. A fault
sweep over a 4 × 4 KiB mock (ADR 0019's, when it exists) fills the region
through saves, Forgets, and evictions of bonded peers at every fill level,
cuts power before and partway through every header rewrite and page erase of
a Factory reset (a cut erase leaves the page partly reset to `0xFF`, as in ADR
0019's mock, including an older page left partial-open), and asserts that a
reload yields the empty store or Unreadable, never a removed bond, and a
finished scrub only `0xFF`. Release helper tests cover a missing, duplicated,
or malformed record, an open production image, a locked twin or self-test, a
difference outside the policy word, and a disagreeing HEX or metadata. Renode
adds nothing: the simulation has no policy shell or storage.

Board evidence ([hardware-result record](../testing.md#hardware-acceptance-evidence))
on a production-equivalent board with build code F or later: the procedure,
with the step 5 read failing after that power cycle, ten more, and a pin
reset; recovery with `--allow-erase-all`, then pages 240 to 243 reading `0xFF`
and the SoftDevice gone; an in-place lock whose peripherals reconnect; on a
twin, a dump showing a Forget's residue, then Factory reset and an all-`0xFF`
dump; and the locked bridge in a monitor hub and firmware setup
([first flash](../first-flash.md#5-in-the-monitor)).

This unblocks "ADR: provisioning, debug access, and readout protection" (once
Accepted) and "Provisioning and physical key protection"
([TODO.md](../../TODO.md#device-security-and-provisioning)), and with them the
release gate; gives "Hosted provenance and release recovery acceptance" its
service procedure; makes "Diagnostics without sensitive input" probe-free
([TODO.md](../../TODO.md#platform-memory-and-recovery)); adds the scrub to
"Forget/reset hardware and interruption acceptance"
([TODO.md](../../TODO.md#pairing-storage)); and constrains the signed DFU ADR
([TODO.md](../../TODO.md#updates-and-host-tools)).

### Verification Status

- **Implemented:** nothing of this proposal. Today every image uses
  `Debug::Allowed`, Factory reset erases only an unreadable store, and bonds
  are stored in plain form.
- **Software-verified:** nothing of this proposal. The host tests of
  `devices.rs` cover today's reset decision (erase first only when
  unreadable), but none covers the debug setting or the `storage.rs` flash
  shell ([testing](../testing.md#modules-without-host-tests)), and no release
  helper test inspects firmware contents.
- **Hardware-verified:** not yet; no board record shows a build code, a locked
  unit, a recovery, or the pairing pages after a reset.

## Related

- [Security: physical access and debug port](../security.md#physical-access-and-debug-port), [key storage and deletion](../security.md#key-storage-and-deletion), [threat model](../security.md#threat-model), [firmware integrity and updates](../security.md#firmware-integrity-and-updates)
- [Data model: pairing store](../data-model.md#pairing-store), [privacy and retention](../data-model.md#privacy-and-retention); [architecture: Forget and factory reset](../architecture.md#forget-and-factory-reset), [decisions needed](../architecture.md#decisions-needed-for-roadmap-work)
- [Deployment: erase behavior](../deployment.md#erase-behavior), [SoftDevice reinstall after a full erase](../deployment.md#softdevice-reinstall-after-a-full-erase), [release gates](../deployment.md#release-gates); [operations: saved devices and storage](../operations.md#saved-devices-and-storage); [hardware: pin and peripheral usage](../hardware.md#pin-and-peripheral-usage)
- [ADR 0003](0003-pure-core-and-task-shell.md), [ADR 0004](0004-layered-verification.md), [ADR 0006](0006-fail-closed-pairing-store.md), [ADR 0008](0008-attested-draft-releases.md), [ADR 0012](0012-bus-powered-no-system-off.md) (no System OFF, so no wake reset), [ADR 0013](0013-pinned-toolchain-and-mask-tasks.md), [ADR 0018](0018-production-usb-identity.md), [ADR 0019](0019-power-loss-safe-persistence.md), [ADR 0020](0020-watchdog-and-progress-based-recovery.md) (a watchdog reset also ends the open window after `ERASEALL`)
- TODO.md: [device security and provisioning](../../TODO.md#device-security-and-provisioning), [pairing storage](../../TODO.md#pairing-storage), [release, provenance and supply chain](../../TODO.md#release-provenance-and-supply-chain), [updates and host tools](../../TODO.md#updates-and-host-tools)
