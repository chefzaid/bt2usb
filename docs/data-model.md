# Data Model Reference

bt2usb has one persisted data set, the pairing store, and a small set of fixed
contracts: the USB identity and HID reports it presents to the host, the BLE
HID payloads it accepts from peripherals, the messages its tasks exchange, and
the UI state the OLED renders. This guide defines their layout, ownership,
verification status, and the rules for changing them. The physical memory map
and pins are in [hardware](hardware.md#memory-layout); the runtime structure is
in [architecture](architecture.md).

Every value below is taken from the source files linked next to it. When a
behavior depends on a library default rather than on this repository's code,
the guide says so.

## Relationship Overview

```mermaid
erDiagram
    PAIRING_STORE ||--o{ PAIRED_DEVICE : "holds 0..4"
    PAIRED_DEVICE ||--o| BOND : "optional"

    PAIRING_STORE {
        u8 magic "0xB2"
        u8 version "0x01"
        u8 record_count "0..4"
    }
    PAIRED_DEVICE {
        bytes7 address "6 bytes + address type"
        i8 last_rssi
        u8 name_len "0..32"
        utf8 name
        u8 bond_flag "0 or 1"
    }
    BOND {
        u16 ediv
        bytes8 rand
        bytes16 ltk
        u8 key_flags
        bytes16 irk
        bytes7 identity_address
    }
```

A device record identifies a peer for the UI and for reconnect. Its bond, when
present, carries the keys that let the bridge reconnect without pairing again
and resolve the peer's rotating private address.

The data sets at a glance:

| Data set | Lifetime | Owner | Defined in |
| --- | --- | --- | --- |
| Pairing store | Persistent, internal flash pages 240–243 | BLE coordinator | [storage.rs](../src/storage.rs), [storage/](../src/storage/) |
| USB device identity and descriptors | Fixed at build time; serial per chip | USB task | [usb/hid_device.rs](../src/usb/hid_device.rs), [config.rs](../src/config.rs) |
| USB HID input and LED output reports | Transient, per USB transfer | Endpoint workers, USB control handler | [hid/](../src/hid/) |
| BLE HID payloads from peripherals | Transient, per GATT notification | Connection workers | [hid/mod.rs](../src/hid/mod.rs), [ble/hid_client.rs](../src/ble/hid_client.rs) |
| Internal messages | RAM, bounded channels and signals | Producing and consuming tasks | [main.rs](../src/main.rs), [ble/](../src/ble/), [hid/delivery.rs](../src/hid/delivery.rs) |
| UI state | RAM, main loop | Main UI loop; display task renders a copy | [ui/ui_logic.rs](../src/ui/ui_logic.rs) |

## Verification Status

"Host tests" counts `#[test]` functions with `grep -c '#\[test\]'` in the named
file. A module that is not compiled into the host library (`src/lib.rs`) is
not covered by `cargo test --lib --tests`, whatever tests it contains.

| Contract | Implementation | Software verification | Hardware verification |
| --- | --- | --- | --- |
| Store framing (magic, version, count, length prefixes) | [storage/framing.rs](../src/storage/framing.rs) | 8 host tests | None recorded |
| Record prefix and bond-flag validation | [storage/record.rs](../src/storage/record.rs) | 3 host tests | None recorded |
| Address and bond byte codec | [storage/codec.rs](../src/storage/codec.rs) | Firmware build and Clippy only; no tests | None recorded |
| Load, save, legacy parse, merge, eviction | [storage.rs](../src/storage.rs) | Firmware build and Clippy only; no tests | None recorded |
| Persist-then-publish commit and quiescence barrier | [ble/management.rs](../src/ble/management.rs) | 5 host tests | None recorded |
| USB report layouts and descriptors | [hid/](../src/hid/) | Host tests in [lib_tests.rs](../src/lib_tests.rs) and [hid_descriptor_tests.rs](../src/hid_descriptor_tests.rs), including `parses_actual_usb_descriptors_without_cross_classifying_pan` | None recorded |
| USB device identity and request handling | [usb/hid_device.rs](../src/usb/hid_device.rs) | Firmware build and Clippy only | Self-test enumeration stage exists; no recorded run |
| Coordinator reducers behind the BLE messages | [ble/coordinator.rs](../src/ble/coordinator.rs) | 26 host tests in [coordinator_tests.rs](../src/ble/coordinator_tests.rs); Renode scenario | None recorded |
| UI state model | [ui/ui_logic.rs](../src/ui/ui_logic.rs) | 19 host tests; Renode scenario | None recorded |

[Testing](testing.md#known-verification-gaps) lists the missing fuzzing, fault
injection, and hardware evidence. The `mask selftest` flash stage exercises the
pairing region with a scratch key, not this record format.

## Pairing Store

### Location

| Property | Value | Source |
| --- | --- | --- |
| Flash pages | 240–243 (`0x000F0000–0x000F4000`), 4 KiB each | `STORAGE_FLASH_PAGE_START` / `COUNT` in [config.rs](../src/config.rs); `FLASH_PAGE_SIZE` in [storage.rs](../src/storage.rs) |
| Container | One `sequential-storage` map item, key `0x01`, no cache (`NoCache`) | `KEY_PAIRED_DEVICES` in [storage.rs](../src/storage.rs) |
| Flash access | SoftDevice flash API (`nrf_softdevice::Flash`), so writes wait for radio-idle time | [ble/multi_conn.rs](../src/ble/multi_conn.rs) |
| Maximum item size | 512 bytes, checked at compile time against four bonded records with 32-byte names | `MAX_RECORD_SIZE` |
| Capacity | 4 peers; 2 can be connected at once | `MAX_PAIRED_DEVICES`, `MAX_CONNECTIONS` |
| Other keys | `0xFE` is written, read back and removed by the self-test image only | `SELFTEST_KEY` in [selftest.rs](../src/selftest.rs) |

The linker script excludes these pages from application flash, so firmware
growth cannot overwrite bonds ([ADR 0006](adr/0006-fail-closed-pairing-store.md),
[hardware](hardware.md#memory-layout)).

### Frame

Owned by [storage/framing.rs](../src/storage/framing.rs):

```text
[0]   magic        0xB2
[1]   version      0x01
[2]   record count 0..=4
[3..] repeated:    [len: u8][record bytes; len]
```

A record length of zero, a length that runs past the end of the item, or a
count that disagrees with the records present makes the frame incomplete. The
writer refuses a record that is empty, longer than 255 bytes, or does not fit
the buffer, so a full buffer never produces a corrupt frame.

### Device Record

Owned by [storage.rs](../src/storage.rs) and validated by
[storage/record.rs](../src/storage/record.rs):

| Offset | Size | Field | Rules |
| --- | --- | --- | --- |
| 0 | 6 | Address bytes | As reported by the SoftDevice |
| 6 | 1 | Address type | 0 public, 1 random static, 2 resolvable private, 3 non-resolvable private, 4 anonymous; anything else is invalid |
| 7 | 1 | Last RSSI | Signed dBm; see the note below |
| 8 | 1 | Name length | 0–32 bytes |
| 9 | n | Name | Valid UTF-8; truncated by characters to fit 32 bytes |
| 9 + n | 1 | Bond flag | 0 = no bond, record ends here; 1 = bond follows |
| 10 + n | 50 | Bond | Present only when the flag is 1; the record length must match exactly |

A bonded record is stored under the peer's identity address from the bond, not
the advertising address seen at connection time (`DeviceStore::add`). This
keeps a peer that rotates its resolvable private address from creating a new
record on every rotation.

The RSSI byte holds the cached value at the most recent write. An RSSI-only
change updates the in-memory record but does not mark the store dirty, so the
flash copy can be older than the cache. No current code sorts or displays
saved devices by RSSI.

### Bond

Owned by [storage/codec.rs](../src/storage/codec.rs):

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 2 | Master ID `ediv`, little-endian |
| 2 | 8 | Master ID `rand` |
| 10 | 16 | Long-term key (LTK) |
| 26 | 1 | Encryption key flags |
| 27 | 16 | Identity resolving key (IRK) |
| 43 | 7 | Identity address and type; must be public or random static |

The flags byte is copied verbatim from the SoftDevice `EncryptionInfo`, which
the vendored `nrf-softdevice` declares layout-compatible with the SoftDevice's
`ble_gap_enc_info_t` ([vendor/nrf-softdevice/src/ble/types.rs](../vendor/nrf-softdevice/src/ble/types.rs)).
The application does not interpret it. Validation rejects an identity address
type above 1 (byte 49 of the bond).

### Worked Example

One public-address device named `KB`, RSSI −60 dBm, without a bond:

```text
B2 01 01            magic, version 1, one record
0C                  record length 12
11 22 33 44 55 66   address bytes as stored by the SoftDevice
00                  address type: public
C4                  last RSSI: -60 as a signed byte
02                  name length
4B 42               "KB"
00                  bond flag: no bond
```

The item is 16 bytes. With a bond the record grows by 50 bytes and the flag is
`01`. After a successful Factory reset of a readable store the item is the
3-byte empty frame `B2 01 00`.

### Legacy Format

Firmware before the versioned format (introduced in commit c37d568) wrote an
unversioned blob. It is still accepted on load when the first byte is not
`0xB2`:

```text
[0]   record count 0..=4
[1..] repeated, no length prefix:
      [address 6][type 1][rssi 1][name_len 1][name; name_len]
```

Legacy records carry no bond. Each record passes the same address-type,
name-length and UTF-8 checks as a version 1 record, and the records must end
exactly at the end of the item; trailing or missing bytes reject the whole
blob. Because a legacy count is at most 4, a valid legacy blob never starts
with the `0xB2` magic. Legacy parsing exists only in `storage.rs` and has no
host test.

### Capacity And Compile-Time Checks

[storage.rs](../src/storage.rs) contains:

```rust
const _: () =
    assert!(3 + MAX_PAIRED_DEVICES * (1 + 9 + 32 + 1 + BOND_RECORD_SIZE) <= MAX_RECORD_SIZE);
```

| Term | Bytes | Meaning |
| --- | --- | --- |
| `3` | 3 | Magic, version, count |
| `1` | 1 | Per-record length prefix |
| `9` | 9 | Address (7), RSSI (1), name length (1) |
| `32` | 32 | Longest name |
| `1` | 1 | Bond flag |
| `BOND_RECORD_SIZE` | 50 | Bond |

With four peers the worst case is 3 + 4 × 93 = 375 bytes, leaving 137 bytes of
the 512-byte item unused. The largest record body is 92 bytes, well inside the
255-byte length prefix. Five peers (468 bytes) would still fit; six (561
bytes) fail the assertion, so raising `MAX_PAIRED_DEVICES` past five also
requires a larger `MAX_RECORD_SIZE`. Load uses one 512-byte buffer; save uses
two. New versions of the item are appended across the four 4 KiB pages, and
`sequential-storage` handles wear levelling and garbage collection.

### In-Memory Cache

`DEVICE_STORE` is an async `Mutex` around a `DeviceStore`:

| Field | Type | Meaning |
| --- | --- | --- |
| `devices` | `heapless::Vec<PairedDevice, 4>` | Cached records in insertion order, oldest first |
| `dirty` | `bool` | The cache differs from flash in a field that must persist |
| `writable` | `bool` | Ordinary saves are allowed |

`add` merges a new record into an existing one when the addresses are equal or
either side's IRK resolves the other's address. A merge marks the store dirty
only when the address, name, or bond changed. A new record appended to a full
store evicts the oldest entry (`"Paired device store full - evicting oldest entry"`).
Updating an existing record does not move it.

`iter_recent` yields records newest-first by insertion. Boot reconnect takes the
first two of that order, and the saved-devices list is shown in it. Because an
update does not move a record, "recent" means most recently added, not most
recently connected.

### Load Rules

1. No item in flash: the store is empty and writable (`"No paired devices in flash"`).
2. Flash read error: the store is empty and **not writable**
   (`"Flash read error: {:?}"`).
3. An item that exists but is empty is invalid.
4. First byte is the magic: the frame must be complete, the version supported,
   the count at most four, and every record valid. Any failure loads nothing and
   makes the store not writable. A truncated or future-version frame is never
   reinterpreted as the legacy format.
5. No magic: the blob is parsed as the [legacy format](#legacy-format) and is
   rewritten in the current format by the next save that a change triggers.
6. Records resolving to the same bonded identity are merged while loading, so
   two slots never chase the same peer. The merge is not written back until the
   next change.

An invalid item logs `"Invalid or unsupported device store; writes disabled"`
and the coordinator raises `BleEvent::Error(StorageFailed)` at boot. A store
that is not writable stays that way until the user confirms Factory reset,
which then erases pages 240–243 before writing an empty frame. Ordinary saves refuse with `StoreError::Unreadable`
(`"Device store is unreadable; refusing to overwrite stored bonds"`).

### Write Rules

- Saves happen only when the cache is dirty and the store is writable.
- Serialization that would exceed the item size aborts before touching flash
  (`"Device store exceeds serialization capacity; save aborted"`).
- A write is attempted up to three times in total (two retries), 20 ms apart,
  because SoftDevice flash operations wait for radio-idle timeslots
  (`FLASH_WRITE_ATTEMPTS`; `"Flash write busy (attempt {}), retrying"`, then
  `"Flash write failed after {} attempts: {:?}"`).
- Every successful connection adds or updates the record and saves if
  anything changed (`Action::PersistDevice`); a failure raises
  `BleEvent::Error(StorageFailed)` while the link stays up.
- Forget and Factory reset stop affected workers, build a candidate store,
  write it, and only then replace the in-memory cache and update the SoftDevice
  bonder (`management::commit`). A failed write leaves cached records and bonds
  unchanged and is shown to the user.
- Forget first looks the requested address up with `DeviceStore::find`, which
  also matches through a stored IRK, and then removes that record by its stored
  address with an exact comparison (`DeviceStore::forget`). Factory reset
  writes an empty frame through the same map append; it erases pages 240–243
  first only when the store was not writable.
- Removal is logical. Older copies of the item stay in flash until
  `sequential-storage` reclaims their page; see
  [security](security.md#key-storage-and-deletion).

### Store Errors

| `StoreError` | Raised when | Surfaces as |
| --- | --- | --- |
| `Unreadable` | Save or Forget on a store whose load failed | `StorageFailed` |
| `Serialization` | The frame does not fit `MAX_RECORD_SIZE` | `StorageFailed` |
| `Flash` | Three write attempts failed, or the recovery erase failed | `StorageFailed` |
| `NotFound` | Forget named an address with no exact match (the coordinator's earlier lookup normally prevents this) | `StorageFailed` |

A Forget whose identity is not in the cache fails earlier with
`ManagementFailed`, before any worker is stopped.

## USB Device Identity

The USB descriptors are assembled once by `hid_device::init` with
`embassy-usb`. Fields this repository does not set keep the `embassy-usb`
defaults; confirm the emitted descriptors with a descriptor dump on hardware.

| Field | Value | Source |
| --- | --- | --- |
| Controller | nRF52840 USBD, USB 2.0 full speed | [usb/mod.rs](../src/usb/mod.rs) |
| Vendor ID / Product ID | `0x1209` / `0x0001`: the pid.codes open-source VID, marked in `config.rs` as a test identity to replace for production | `USB_VID`, `USB_PID` in [config.rs](../src/config.rs) |
| Manufacturer string | `bt2usb` | `USB_MANUFACTURER` |
| Product string | `BT-to-USB HID Bridge` | `USB_PRODUCT` |
| Serial number | 16 uppercase hex characters: `FICR.DEVICEID[1]` then `FICR.DEVICEID[0]`, each `{:08X}` | `init` in [usb/hid_device.rs](../src/usb/hid_device.rs) |
| Control endpoint max packet | 64 bytes | `max_packet_size_0` |
| Max power | 100 mA, a declared budget, not a measurement | `max_power` |
| Remote wakeup | Advertised | `supports_remote_wakeup = true` |
| Self-powered, device class, device release | Not set by this repository | `embassy-usb` defaults |
| Descriptor buffers | 256 bytes each for configuration, BOS and MS OS descriptors; 128-byte control buffer | Static cells in `hid_device.rs` |

The serial is read from factory-programmed, read-only registers, so it is
stable across firmware updates and USB ports and differs between chips.
[TODO.md](../TODO.md) tracks a production VID/PID and unit-identity policy.

| Interface | Function | Subclass / protocol | Report descriptor | Endpoints | Class requests handled |
| --- | --- | --- | --- | --- | --- |
| 0 | Keyboard | Boot (1) / Keyboard (1) | 64 bytes, no report IDs | One interrupt IN, 8-byte max packet, 1 ms | `GET_PROTOCOL`, `SET_PROTOCOL`, `SET_REPORT` for the LED output report |
| 1 | Mouse | Boot (1) / Mouse (2) | 77 bytes, no report IDs | One interrupt IN, 8-byte max packet, 1 ms | `GET_PROTOCOL`, `SET_PROTOCOL` |
| 2 | Consumer control | None (0) / None (0) | 23 bytes, no report IDs | One interrupt IN, 8-byte max packet, 1 ms | None beyond the class defaults |

Interface numbers follow creation order in `init` and match the list in
[usb/mod.rs](../src/usb/mod.rs). Endpoint addresses are allocated by the
`embassy-usb` builder and `embassy-nrf` driver, not pinned in source. No
interface has an interrupt OUT endpoint: the host sends keyboard LED state with
`SET_REPORT` on the control endpoint. `GET_REPORT`, `SET_IDLE` and `GET_IDLE`
are left to the `embassy-usb` request-handler defaults; defining them is the
HID/USB conformance item in [TODO.md](../TODO.md). Descriptor lengths were
counted from the byte arrays in [hid/](../src/hid/).

### USB Lifecycle Effects

| Host event | Effect in [usb/hid_device.rs](../src/usb/hid_device.rs) |
| --- | --- |
| Bus reset or USB disabled | Configured and suspended flags cleared, both interfaces back to report protocol, pending wake cleared, LED state reset to all off, resume signalled to the UI loop, all endpoints replay held state |
| Configured or deconfigured | Configured flag updated, endpoints replay, `"USB configured by host: {}"` |
| Suspend or resume | Pending wake cleared, suspended flag updated, signal to the UI loop's power manager, endpoints replay |
| `SET_PROTOCOL` on keyboard or mouse | That interface's boot flag updated and only its endpoint replays |
| `SET_REPORT` (keyboard, Output, ID 0, 1 byte) | Byte masked to 5 LED bits, published to the LED `Watch`, `"Host LEDs: num={} caps={} scroll={}"`; any other `SET_REPORT` to the keyboard or mouse interface is rejected |
| New press while suspended | Remote wakeup requested; `"USB remote wakeup sent"` or `"USB remote wakeup not possible: {}"` |

An endpoint is available only while the device is configured and not
suspended. Replay invalidates in-flight transfers and requeues only held
state; relative motion is never replayed
([ADR 0005](adr/0005-two-slots-and-independent-endpoints.md)).

## USB HID Report Contracts

The composite device exposes three interrupt-IN interfaces. Report formats are
defined in [src/hid/](../src/hid/) and the descriptors in the same modules.

| Interface | Report mode | Boot mode | Notes |
| --- | --- | --- | --- |
| Keyboard | 8 bytes: modifiers, reserved, six key codes | Same 8 bytes | More than six keys sends the `ErrorRollOver` array; LED output report is forwarded to BLE keyboards |
| Mouse | 5 bytes: five buttons, X, Y, wheel, horizontal pan (signed 8-bit) | 3 bytes: three buttons, X, Y | Motion is relative and never replayed |
| Consumer control | 2 bytes: one usage, little-endian, at most `0x0FFF` | — | Lowest active BLE slot wins |

Incoming BLE reports are classified by the peripheral's Report Map and report
references into these internal types. Layouts that cannot be mapped are
rejected rather than guessed; see
[HID path and limits](architecture.md#hid-path-and-limits).

### Keyboard Interface

The descriptor is a Generic Desktop (`0x01`) Keyboard (`0x06`) application
collection in [hid/keyboard.rs](../src/hid/keyboard.rs).

| Input byte | Field | Usages |
| --- | --- | --- |
| 0 | Modifier bits, bit 0 Left Control to bit 7 Right GUI | Keyboard page `0x07`, `0xE0–0xE7` |
| 1 | Reserved, always 0 | Constant |
| 2–7 | Up to six key usages, 0 for none | Keyboard page `0x00–0xFF`, array |

The aggregator forces the reserved byte to 0. If the union of both sources has
more than six keys, or a source reports an error code `0x01–0x03`, all six key
bytes become `0x01` (`ErrorRollOver`) while modifiers are still sent.

| Output bit | LED usage (page `0x08`) |
| --- | --- |
| 0 | Num Lock |
| 1 | Caps Lock |
| 2 | Scroll Lock |
| 3 | Compose |
| 4 | Kana |
| 5–7 | Padding |

`KeyboardLeds` masks the byte to `0x1F`. Each connection worker that found a
keyboard LED output report on its peer writes the masked byte to that
characteristic whenever the host changes it.

### Mouse Interface

The descriptor is a Generic Desktop Mouse (`0x02`) application collection with
a Pointer physical collection in [hid/mouse.rs](../src/hid/mouse.rs).

| Input byte | Field | Range |
| --- | --- | --- |
| 0 | Buttons 1–5 in bits 0–4 (Button page `0x09`); bits 5–7 padding | — |
| 1 | X (`0x30`), relative | −128..127 |
| 2 | Y (`0x31`), relative | −128..127 |
| 3 | Wheel (`0x38`), relative | −128..127 |
| 4 | AC Pan (Consumer page `0x0238`), relative | −128..127 |

In boot protocol the interface sends 3 bytes: buttons masked to bits 0–2, then
X and Y with −128 clamped to −127 (`serialize_boot`). Wheel and pan are
dropped.

### Consumer Control Interface

The descriptor is a Consumer page (`0x0C`) Consumer Control (`0x01`)
application collection in [hid/consumer.rs](../src/hid/consumer.rs) with one
16-bit array field, usages and logical values `0x000–0xFFF`. The report is the
active usage, little-endian; `0x0000` is release. Only one usage can be active,
so when both sources hold a usage the lower-numbered slot's usage is sent.

### Accepted BLE Payloads

GATT values carry no report-ID prefix; the Report Reference descriptor and the
Report Map supply the kind ([hid/mod.rs](../src/hid/mod.rs)).

| Kind | Accepted payload | Rejected |
| --- | --- | --- |
| Keyboard | Exactly 8 bytes with byte 1 equal to 0 | Any other length, nonzero reserved byte |
| Mouse | 3–5 bytes; buttons masked to 5 bits; missing wheel or pan read as 0 | Fewer than 3 or more than 5 bytes |
| Consumer | Exactly 2 bytes, usage at most `0x0FFF` | Other lengths, larger usages |

A notification longer than 32 bytes (`MAX_REPORT_LEN` in
[hid_client.rs](../src/ble/hid_client.rs)) is dropped, not truncated. Without a
Report Map, length alone selects the kind (8 keyboard, 3–5 mouse, 2 consumer
below `0x1000`). A Report Map without report IDs restricts that fallback to the
kinds it declares; with report IDs, an input characteristic whose kind cannot
be resolved is not subscribed at all.

## Task Channel Contracts

Inter-task messages use bounded Embassy channels with `CriticalSectionRawMutex`;
the structure is drawn in [architecture](architecture.md#tasks-and-data-flow).
Management commands (`ListPaired`, `Forget`, `FactoryReset`) carry a request
ID, and the UI accepts only the reply that matches its single outstanding
request. Forget names a stable peer identity, never a list index.

| Channel | Message | Producer | Consumer | Capacity | Full-channel behavior |
| --- | --- | --- | --- | --- | --- |
| `BUTTON_CHANNEL` | `ButtonEvent` | Three button tasks | Main UI loop | 4 | Sender waits |
| `BLE_CMD_CHANNEL` | `BleCommand` | Main UI loop | BLE coordinator | 4 | `try_send`; the UI shows `"Busy; try again"` and abandons the request |
| `BLE_EVENT_CHANNEL` | `BleEvent` | BLE coordinator, including its scans | Main UI loop | 8 | Sender waits |
| `BLE_SLOT0_CMD_CHANNEL`, `BLE_SLOT1_CMD_CHANNEL` | `SlotCommand` | BLE coordinator | Connection worker 0 or 1 | 2 each | Sender waits |
| `BLE_SLOT_EVENT_CHANNEL` | `SlotEvent` | Both connection workers | BLE coordinator | 8 | Sender waits |
| `HID_REPORT_CHANNEL` | `HidEvent` | Both connection workers | HID dispatcher in `hid_writer_task` | 16 | Sender waits; a per-link coalescer absorbs bursts |

The UI never waits on `BLE_CMD_CHANNEL`, because the coordinator may be waiting
on a full `BLE_EVENT_CHANNEL` that only the UI drains (comment in
[main.rs](../src/main.rs)). The GATT notification callback cannot wait, so each
link pushes into a `ReportCoalescer` that keeps the latest keyboard and
consumer state and sums mouse motion, and a drain future sends with
backpressure ([hid/coalesce.rs](../src/hid/coalesce.rs)).

### Latest-Value Signals And Shared State

| Item | Type | Writer | Reader | Purpose |
| --- | --- | --- | --- | --- |
| `FRAMES` | `Signal<Frame>` | Main UI loop | Display task | Latest UI snapshot and display power; older frames are overwritten |
| `USB_SUSPEND_SIGNAL` | `Signal<bool>` | USB event handler | Main UI loop | Suspend or resume for the power manager |
| `REMOTE_WAKE` | `Signal<()>` | HID dispatcher | USB device task | Request remote wakeup |
| `KEYBOARD_LEDS` | `Watch<KeyboardLeds>`, 2 receivers | USB control handler | Connection workers | Latest host LED state |
| `KEYBOARD_DELIVERY`, `MOUSE_DELIVERY`, `CONSUMER_DELIVERY` | Endpoint mailboxes | HID dispatcher, USB event handler | Endpoint workers | 16-report FIFO, held state, transfer epoch |
| `USB_CONFIGURED`, `USB_SUSPENDED`, `KEYBOARD_BOOT_PROTOCOL`, `MOUSE_BOOT_PROTOCOL` | `AtomicBool` | USB handlers | USB tasks, self-test | Host-facing state |
| `HID_ACTIVITY` | `AtomicBool` | HID dispatcher | Power manager tick | Input counts as activity for display power |
| `DEVICE_STORE` | Async `Mutex<DeviceStore>` | BLE coordinator | BLE coordinator | Pairing cache; held across flash writes |
| `GAP_PROCEDURE` | Async `Mutex<()>` | Scanner, connection workers | Same | One SoftDevice scan or connection setup at a time |
| Bonder | `StaticCell` around a `RefCell` | SoftDevice security callbacks, coordinator | Same, plus connection workers | In-RAM bond table loaded from the store |

## Internal Message Contracts

### BleCommand

UI to coordinator, defined in [ble/mod.rs](../src/ble/mod.rs). The UI loop
builds it from a `UiCommand`.

| Variant | Fields | Meaning | Reply |
| --- | --- | --- | --- |
| `StartScan` | — | Scan for HID peripherals for `BLE_SCAN_DURATION_SECS`; if both slots are occupied, disconnect both first | `ScanStarted`, `DeviceFound`…, `ScanComplete`, or `Error(ScanFailed)` |
| `Connect(usize)` | Index into the coordinator's last scan result | Connect and allow pairing | `Connected` or `Error`; an out-of-range index or no free slot gives `Error(ConnectFailed)`; an already connected peer gives `Connected` at once |
| `Disconnect` | — | Disconnect every occupied slot; records are kept | `Disconnected` or `Connected(summary)` as slots free |
| `ListPaired { id }` | Request ID | Read the saved-device list | `PairedDevices { id, devices }` |
| `Forget { id, address }` | Request ID, stored identity address | Stop matching workers, remove the record, drop its bond | Link status event, then `ManagementResult { id, result }`; a failed lookup sends only `ManagementResult` with `ManagementFailed` |
| `FactoryReset { id }` | Request ID | Stop all workers, clear every record and bond; recover an unreadable store | Link status event, then `ManagementResult { id, result }` |

A Factory reset also discards the coordinator's last scan result, so a later
`Connect` needs a new scan.

### BleEvent

Coordinator to UI, defined in [ble/mod.rs](../src/ble/mod.rs).

| Variant | Fields | Produced when |
| --- | --- | --- |
| `ScanStarted` | — | Any scan begins, including the boot reconnect scan |
| `DeviceFound(DiscoveredDevice)` | Address, name (`"Unknown"` if none advertised), RSSI | Once per HID device at the end of a scan, up to 8 |
| `ScanComplete` | — | The scan window closed |
| `Connected(String<32>)` | One device's name, or `"2 devices"` | A slot connected, or a slot changed while another stays connected |
| `Disconnected` | — | No slot is connected |
| `Error(BleErrorTag)` | Error tag | A user-visible failure; see the table below |
| `PairedDevices { id, devices }` | Request ID, up to 4 `DiscoveredDevice`, newest first | Reply to `ListPaired` |
| `ManagementResult { id, result }` | Request ID, `Result<(), BleErrorTag>` | Reply to `Forget` or `FactoryReset`; `Ok` means the change reached flash |

`DiscoveredDevice` is `coordinator::DeviceInfo<Address>`: an address, a
`heapless::String<32>` name, and an `i8` RSSI.

### SlotCommand

Coordinator to one connection worker, defined in
[ble/multi_conn.rs](../src/ble/multi_conn.rs).

| Variant | Meaning |
| --- | --- |
| `Connect(DiscoveredDevice)` | User-selected connection; pairing may be initiated; a failure is reported |
| `Reconnect(DiscoveredDevice)` | Silent retry of a stored peer; never initiates pairing; for a bonded peer, resolves its current address before each attempt; waits `BLE_RECONNECT_BACKOFF_MS` between attempts until it connects or another command arrives |
| `Disconnect` | Close the link or stop retrying, then report `Disconnected` |
| `Quiesce(u32)` | Close the link, drop any retry target, then acknowledge with the same token |

### SlotEvent

Connection worker to coordinator, defined in
[ble/multi_conn.rs](../src/ble/multi_conn.rs).

| Variant | Fields | Meaning | Coordinator action |
| --- | --- | --- | --- |
| `Connected` | `slot`, `device` | Link encrypted, HID discovered and subscribed | Mark slot connected, persist the device and its bond, emit `Connected` |
| `Disconnected` | `slot` | The slot is free | Clear slot, emit link status |
| `LinkLost` | `slot`, `device` | An established link dropped; the worker is retrying | Keep the slot reserved, emit link status |
| `Error` | `slot`, `tag` | A user connection failed, or a silent attempt failed for a reason other than `ConnectFailed` | Clear slot, emit `Error(tag)` and link status |
| `Quiesced` | `slot`, `token` | Reply to `Quiesce` | Counted only by the management barrier |

### HidEvent And HidReport

Connection workers to the HID dispatcher, defined in
[hid/delivery.rs](../src/hid/delivery.rs) and [hid/mod.rs](../src/hid/mod.rs).

| Variant | Fields | Meaning |
| --- | --- | --- |
| `HidEvent::Report` | `source` (slot index), `report: HidReport` | One classified input report |
| `HidEvent::Disconnected` | `source` | The source's link ended after it was established; its held input must be released |

`HidReport` is `Keyboard(KeyboardReport)`, `Mouse(MouseReport)` or
`Consumer(ConsumerReport)`, with the fields shown in the
[report contracts](#usb-hid-report-contracts). A worker sends `Disconnected`
after its final report, so the release follows the last input in channel order.
The dispatcher marks activity, merges the event into the per-source aggregate,
requests remote wakeup for a new press while suspended, and publishes up to
three endpoint reports.

### ButtonEvent

`Up`, `Down`, `Select` ([ui/ui_logic.rs](../src/ui/ui_logic.rs)), one event per
debounced press. Electrical details are in
[hardware](hardware.md#buttons).

### Error Tags And UI Messages

`BleErrorTag` is the coordinator's `ErrorTag`
([ble/coordinator.rs](../src/ble/coordinator.rs)); the UI text comes from
`ble_error_message` in [main.rs](../src/main.rs).

| Tag | Raised by | OLED message |
| --- | --- | --- |
| `ScanFailed` | SoftDevice refused or ended a scan with an error | `Scan failed` |
| `ConnectFailed` | Connection setup failed or timed out, the link could not be encrypted, bad scan index, or no free slot | `Connect failed` |
| `HidNotFound` | GATT discovery of the HID service failed | `No HID service` |
| `NotifyFailed` | No input report characteristic could be subscribed | `Notify failed` |
| `StorageFailed` | Store unreadable at boot, a save failed, or a management write failed | `Storage failed` |
| `ManagementFailed` | Forget named an identity not in the store | `Action failed; retry` |
| `ReportMapReadFailed` | The Report Map could not be read | `HID map read failed` |
| `ReportMapTooLarge` | The Report Map exceeds 512 bytes | `HID map too large` |
| `ReportMapInvalid` | The Report Map does not parse | `Unsupported HID map` |

The UI also produces `Busy; try again` (command channel full),
`Device changed; retry` (Forget index no longer in the saved-device snapshot)
and `No devices found` (scan ended empty).

### Management Request Lifecycle

`ManagementRequests` in [ui/ui_logic.rs](../src/ui/ui_logic.rs) allows one
request at a time. IDs increase with wrapping arithmetic; a reply whose ID does
not match the pending request is ignored. While a request is pending the UI
ignores every button, so a coordinator that never replies leaves the UI on
**Please wait...** until power is cycled; a bounded wait is open in
[TODO.md](../TODO.md).

```mermaid
sequenceDiagram
    participant UI as Main UI loop
    participant C as BLE coordinator
    participant W as Targeted connection worker
    participant S as DEVICE_STORE and flash
    participant B as Bonder
    UI->>C: BleCommand::Forget { id, address }
    C->>S: find(address)
    C->>W: SlotCommand::Quiesce(token)
    W-->>C: SlotEvent::Disconnected if a link was open (discarded)
    W-->>C: SlotEvent::Quiesced { slot, token }
    C->>S: forget(stored address), write, then replace cache
    S-->>C: Ok or StoreError
    C->>B: forget(address), only after Ok
    C-->>UI: BleEvent::Connected(summary) or Disconnected
    C-->>UI: BleEvent::ManagementResult { id, result }
```

While the barrier is open, events from targeted slots are discarded, so a
queued `Connected` or `LinkLost` cannot re-save the peer being removed. Slots
that are not targeted keep running. Factory reset targets both slots and calls
`factory_reset` instead of `forget`. If `find` fails, the coordinator replies
`ManagementResult` with `ManagementFailed` immediately.

## UI State Model

The main loop owns a `UiState` and publishes a copy with the display power flag
to the display task after every event
([ADR 0009](adr/0009-isolated-display-task.md)). User-facing flows are in
[features](features.md#using-the-bridge).

| `UiState` field | Type | Meaning |
| --- | --- | --- |
| `screen` | `Screen` | Current view |
| `selected` | `usize` | Cursor in the current list or dialog |
| `devices` | `Vec<String<32>, 8>` | Names from the current scan, in scan order |
| `paired_names` | `Vec<String<32>, 4>` | Names from the last `PairedDevices` reply |
| `connected_name` | `String<32>` | Link summary; empty when nothing is connected |
| `message` | `String<32>` | Error or notice text, truncated by characters |
| `scan_dots` | `u8` | Scanning animation, advanced once per second while scanning with the display on |
| `interactive_scan` | `bool` (private) | Set by the first user-started scan and never cleared |

The loop also keeps the `DiscoveredDevice` list from the last `PairedDevices`
reply to turn a Forget index into an address, a `ManagementRequests` tracker,
and a `PowerManager`. RSSI and addresses are never rendered.

| Screen | OLED text ([ui/display.rs](../src/ui/display.rs)) | UP | DOWN | SELECT |
| --- | --- | --- | --- | --- |
| `Home` | `bt2usb / Idle`, `SELECT: scan`, `UP: saved devices` | List saved devices | — | Scan |
| `Scanning` | `Scanning` and 0–3 dots | — | — | — |
| `DeviceList` | `Select device` and a 4-row window | Cursor up | Cursor down | Connect to the highlighted device |
| `Connecting` | `Connecting...` | — | — | — |
| `Connected` | `Connected`, summary, `SEL:add DOWN:disc`, `UP:saved devices` | List saved devices | Disconnect all | Scan for another device |
| `Error` | `ERROR`, message, `SEL:retry DOWN:back`, `UP:saved devices` | List saved devices | Dismiss | Scan |
| `SavedDevices` | `Saved devices`, names, final `Factory reset` row, `UP at first: back` | Cursor up; on the first row, dismiss | Cursor down | Open `ConfirmForget(i)` or `ConfirmReset` with Cancel selected |
| `ConfirmForget(i)` | `Forget device?`, name, Cancel / Forget | Select Cancel | Select Forget | Cancel returns to the list; Forget sends the request |
| `ConfirmReset` | `Reset all pairings?`, `Disconnect all`, Cancel / Reset | Select Cancel | Select Reset | Cancel returns to the list; Reset sends the request |
| `Managing` | `Please wait...` | Ignored | Ignored | Ignored |
| `Notice` | `Complete`, message, `SELECT: back` | List saved devices | — | Dismiss |

Dismiss returns to `Connected` when a link summary exists, otherwise `Home`,
and clears the message. Events change screens as follows:

| Event | Effect |
| --- | --- |
| `ScanStarted` | From `Home` or `Scanning`: enter `Scanning` and clear the list; other screens are kept |
| `DeviceFound` | Name appended only while `Scanning` |
| `ScanComplete` | From `Scanning`: `DeviceList`, or `Error` with `No devices found` |
| `Connected` / `Disconnected` | Update `connected_name`; before the first user scan, a boot scan's `Scanning` or `DeviceList` returns to `Home` first; `Home`, `Connecting` and `Connected` then follow the link state; dialogs and errors are kept |
| `Error(tag)` | `Error` with the tag's message |
| `PairedDevices` (matching ID) | Store names; enter `SavedDevices` unless an error is showing |
| `ManagementResult` (matching ID) | `Ok`: `Notice` with `Device forgotten` or `Pairings reset` unless an error is showing; `Err`: `Error` |

When the display is off, the first button press only wakes it. The display
policy is in [hardware](hardware.md#power).

## Data Ownership Rules

- The BLE coordinator owns the pairing store and the SoftDevice bonder; the UI
  reads the saved-device list only through management commands.
- Connection workers own their slot's link and held-input source state.
- The input aggregator owns the per-source union; endpoint workers own their
  queue and retained state.
- The USB task owns host-facing state: configuration, suspend, protocol mode,
  and keyboard LEDs (published to workers through a `Watch`).
- The main UI loop owns `UiState`, the management request tracker and the
  power manager. The display task owns TWIM0 and the OLED and only reads
  published frames.
- The bonder and store change together and only after a successful write
  (`management::commit`); neither is edited directly by the UI.
- Flash pages 240–243 belong to the store. The self-test may add and remove its
  own key there; no other code writes them.

## Schema Change Rules

- Any change to the frame or record layout bumps the version byte.
- Add fixture tests for the new version, every supported older version, and
  malformed and truncated input before changing the writer.
- Never relax validation of key material to accept a damaged record.
- Document the upgrade and downgrade path; an older firmware that sees a newer
  version must fail closed, not overwrite it.
- Keep `MAX_RECORD_SIZE`, `MAX_PAIRED_DEVICES`, and the flash reservation
  consistent; the compile-time assertion guards the first two.
- Move record parsing that needs coverage into a host-compiled module, as
  `framing.rs` and `record.rs` already are; `storage.rs` and `codec.rs` are not
  compiled by the host tests.
- A USB descriptor or report change updates the descriptor, the serializer,
  boot-protocol output, `hid_descriptor_tests.rs`, and this guide together, and
  needs enumeration evidence on each supported host.
- A new message variant is handled at every match site (the compiler enforces
  this), keeps channel capacities bounded, and updates the Renode scenario when
  coordinator or UI reducers change.

Versioned migration beyond the legacy format is open work in
[TODO.md](../TODO.md).

## Privacy And Retention

Bond keys are stored unencrypted in internal flash. Device names are kept for
display; RSSI is stored but not used. The application's own log statements
print device names, counts, slot numbers and host LED state, but not
addresses, keys, or keystroke content; keep it that way. The vendored
`nrf-softdevice` is not as strict: at debug level it logs the peer address of
every new connection (`"connected role={:?} peer_addr={:?}"` in
[central.rs](../vendor/nrf-softdevice/src/ble/central.rs)).
[.cargo/config.toml](../.cargo/config.toml) sets `DEFMT_LOG = "debug"` for
any build whose environment does not already set it, so local RTT logs include
peer addresses; the CI workflow sets `DEFMT_LOG: info` for its artifacts, which
leaves that line out. Mask
addresses before sharing a debug log
([security](security.md#logging-and-privacy)). Logical deletion does not guarantee physical erasure; see
[security](security.md#key-storage-and-deletion). Factory reset of a readable
store appends an empty item rather than erasing pages.

## Related Guides

- [Architecture and ADRs](architecture.md)
- [Hardware and memory map](hardware.md)
- [Features](features.md)
- [Testing](testing.md)
- [Security](security.md)
- [Operations](operations.md)
