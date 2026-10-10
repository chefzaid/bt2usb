# ADR 0022: Translate BLE HID Reports Through A Bounded Field Table Built From The Report Map

- Status: Proposed
- Date: 2026-10-10
- Would amend: [ADR 0005](0005-two-slots-and-independent-endpoints.md) when
  Accepted: mouse motion becomes 16-bit inside the bridge, and the mouse
  endpoint writes motion beyond the signed 8-bit range as consecutive reports

## Context

A wired keyboard tells an operating system how to read it through its own
report descriptor, and offers the fixed boot layout to firmware that parses no
descriptor. The bridge's USB interfaces stay fixed instead, so firmware setup
screens, KVMs, and every host behind the hub see the same boot-capable devices
with no software ([TODO.md](../../TODO.md#product-extensions)), and every BLE
layout must be translated on the bridge. The 2026-10-10 tree handles only fixed
byte layouts:

- **The Report Map is parsed for routing only.** `HidDescriptor` in
  [report_protocol.rs](../../src/hid/report_protocol.rs) (lines 146–161) keeps
  three booleans and one report ID per kind; `note_input` (358–368) keeps the
  first ID seen, and `report_kind_for_id` (185–202) refuses an ID carrying two
  kinds. `parse` (207–356) bounds collection and Push/Pop stacks at 16
  (220–221), skips long items with checked arithmetic, rejects Report ID 0 and
  Delimiter (313–318, 338), and never multiplies Report Size by Report Count
  (217–218), so it computes no bit offsets.
- **Payloads are decoded by length.** `parse_by_kind` in
  [hid/mod.rs](../../src/hid/mod.rs) (150–169) accepts an 8-byte keyboard report
  (`KeyboardReport::from_identified_bytes`, keyboard.rs 58–67: bytes 0 and 2–7
  copied, OEM byte 1 zeroed), a 3- to 5-byte mouse report read as buttons and
  signed 8-bit X, Y, wheel, and pan (`MouseReport::from_ble_bytes`, mouse.rs
  54–65), and a 2-byte consumer usage up to `MAX_CONSUMER_USAGE` (consumer.rs
  15, 108–117). Without a map, `infer_from_length` (191–210) routes by length;
  an unnumbered map with several kinds routes by length among its kinds
  (`classify_notification_with_hint`, 79–116).
- **What that drops or misreads.** An NKRO bitmap report, usually longer than
  8 bytes, is dropped; a keyboard that sends only those connects but types
  nothing, silently
  ([operations](../operations.md#connect-fails-with-an-hid-error)). A hybrid
  keyboard's second keyboard report ID is not even subscribed:
  `report_kind_for_id` knows only the first, so `subscribe_all` skips it with
  `Skipping unknown or ambiguous HID report reference` (hid_client.rs
  227–230). Eight buttons, 12-bit X and Y, and an 8-bit wheel fill exactly five
  bytes, a layout many BLE mice use (general knowledge, no fixture yet), so
  `from_ble_bytes` accepts it and reads X, Y, and wheel from the wrong bits. An
  8-byte keyboard report with seven key bytes loses the key in byte 1. A report
  ID with two kinds, and the one input report of an unnumbered map with two
  kinds, are never decoded as declared.
- **Notification context.** `on_hvx` in
  [hid_client.rs](../../src/ble/hid_client.rs) rejects payloads over
  `MAX_REPORT_LEN` (32 bytes, line 52). [sd_setup.rs](../../src/sd_setup.rs)
  sets `att_mtu: 64` (line 29), and a notification carries at most ATT_MTU − 3
  octets (Bluetooth Core, Vol 3, Part F, 3.4.7.1): 61 bytes, or 20 with a peer
  that keeps the 23-byte default. Classification runs in the
  `gatt_client::run` callback (hid_client.rs 385–391), which the vendored
  crate's `on_evt` invokes through the HVX portal inside `softdevice_task`
  (`vendor/nrf-softdevice/src/ble/gatt_client.rs` lines 573 and 669), so it
  must not await. The parsed descriptor lives in the `connect_and_run_secure`
  future ([slot_worker.rs](../../src/ble/slot_worker.rs) 265–297), inside
  `ble_slot_task`, which embassy-executor 0.10 keeps in a static pool of
  `MAX_CONNECTIONS` (2) tasks (main.rs 131); there is no heap
  ([ADR 0010](0010-static-memory-layout.md)). Report Maps are at most 512
  bytes (`MAX_ATTRIBUTE_LEN` in `src/ble/long_read.rs`; Core, Vol 3, Part F,
  3.2.9).
- **Fixed USB side and downstream rules.** USB reports are the 8-byte boot
  keyboard, a 5-byte mouse with signed 8-bit axes, and one consumer usage up to
  `0x0FFF`, on 8-byte endpoints with the boot subclass for keyboard and mouse
  ([hid_device.rs](../../src/usb/hid_device.rs) 314–350); the boot mouse is
  three bytes (`serialize_boot`, mouse.rs 83–91). The aggregator unions every
  source's six keys and sends `ErrorRollOver` when the union exceeds six or a
  source reports codes 1–3
  ([aggregate.rs](../../src/hid/aggregate.rs) 101–132); mouse motion merges
  with 8-bit saturation (mouse.rs 100–108); each endpoint FIFO holds 16 reports
  and is cleared when full (delivery.rs 22, 54).

The specifications fix the rest (HID 1.11 section numbers from general
knowledge, not rechecked): fields are packed from the least significant bit,
little-endian (5.8); no field spans more than four bytes (8.4); a Main item's
usages apply in order, the last repeating (6.2.2.8); an array element indexes
its usage range, out-of-range meaning no control (6.2.2.5); Report ID 0 is
reserved (6.2.2.7); a keyboard reports `ErrorRollOver` in every array position
when more keys are down than the array holds, in no significant order
(Appendix C). GATT Report values omit the report ID, which the Report
Reference supplies (HID Service 1.0). HID has no signed flag: the Global item
remarks (6.2.2.7) make a field two's complement unless both logical extents
are non-negative, and Linux's hid-core sign-extends a field exactly when its
Logical Minimum is negative (general knowledge). Nothing in the dependency
tree parses descriptors: `usbd-hid` 0.10.2 with its `usbd-hid-descriptors`
and `usbd-hid-macros` companions, pulled in by embassy-usb 0.6.0, only builds
them (Cargo.lock).

## Decision

**Build one bounded field table per link, once, from the Report Map.** A new
pure parser runs in `read_report_map` and produces a `ReportTable` that
replaces `HidDescriptor`; `report_kind_for_id`, `is_keyboard_report`,
`is_unnumbered_keyboard_only`, and `has_report_ids` move to it, so routing and
translation cannot disagree. It tracks every global item with Push/Pop,
buffers up to 32 local usages per Main item, and keeps one input bit counter
per report ID; constant items only advance it, Output and Feature items do
not touch it. Logical Minimum is sign-extended from its item size, and Logical
Maximum too when the minimum is negative, as Linux's hid-core does (general
knowledge), so `0x15 0x80` is −128 and `0x26 0xFF 0x00` is 255. Each data
item becomes fields, split wherever role or usage run changes:

| Field | Type | Meaning |
| --- | --- | --- |
| `role` | `u8` enum | What it feeds (below), from application collection, usage, and flags |
| `flags` | `u8` | Array or variable, relative, signed (Logical Minimum below 0), null state |
| `bit_offset` | `u16` | Bit of element 0 in the GATT value, which has no report ID byte |
| `bit_size` | `u8` | 1 to 32, spanning at most four bytes |
| `count` | `u16` | Elements; a 256-key bitmap is one field |
| `usage_min`, `usage_max` | `u16` | Usage of element 0 (variable) or of Logical Minimum (array) |
| `logical_min`, `logical_max` | `i32` | Range; outside it means no control or no motion |

Each input report records its ID (0 without IDs), byte length, kinds, and run
of fields. Fields with no role (vendor pages, LED or battery inputs, System
Control, gamepads, digitizers) are skipped by position and not stored.

| Role | Recognized when | Translated to |
| --- | --- | --- |
| Keys | Keyboard page `0x07` in a Generic Desktop Keyboard (`0x06`) or Keypad (`0x07`) application; bits or array | The link's 256-bit key set, modifiers `0xE0`–`0xE7` included; usages above `0xFF` ignored |
| Buttons | Button page `0x09`, 1-bit variable, in a Generic Desktop Mouse (`0x02`) application | Buttons 1–16 held; 1–5 sent |
| X, Y, Wheel | Generic Desktop `0x30`, `0x31`, `0x38`, relative, signed, in a Mouse application | Sign-extended and saturated to `i16` |
| Pan | Consumer AC Pan `0x0238`, relative, signed, in a Mouse application | Sign-extended and saturated to `i16` |
| Consumer | Consumer page in a Consumer Control (`0x0C`, `0x01`) application; array or bits | Held set of up to eight usages; usages above `MAX_CONSUMER_USAGE` ignored |

**Static capacities.** `MAX_INPUT_REPORTS` is 8, as `MAX_REPORTS` in
hid_client.rs; `MAX_FIELDS` is 32 per link; `MAX_REPORT_BYTES` is 61 (ATT MTU
64 less 3), replacing `MAX_REPORT_LEN`; a translated array holds at most 16
elements, four arrays per link; nesting stays at 16. Common shapes need few
fields (a boot or bitmap keyboard two, a hybrid six-key and bitmap keyboard
three, a five-button mouse with wheel and pan five); the fixture corpus must
confirm the limits before acceptance, and each is one constant.

**Held state is last writer per usage.** A report updates exactly the usages
its fields cover: a variable field sets each to its bit; an array releases what
its previous value asserted and asserts its new value. A PC's HID stack does
the same (Linux's hid-input handles variable fields value by value and arrays
by difference; general knowledge), so a keyboard that sends a six-key report in
one mode and a bitmap in another, or splits keys across report IDs, behaves as
on a PC.

**Emit the fixed reports.** For each kind a notification feeds, the
translator emits a `KeyboardReport` (modifiers; up to six other usages from
`0x04`, ascending; six `0x01` bytes when more than six are held or any of
`0x01`–`0x03` is asserted, which passes on a peripheral's own rollover or
phantom report), a `MouseReport` (buttons 1–5, this notification's motion), or
a `ConsumerReport` (lowest held usage, or 0). A kind is emitted only when its state changed or it
carries motion, so a combined keyboard and mouse report does not resend keys
at mouse rate. Up to three reports go to the `ReportCoalescer`.

**Carry motion in 16 bits and split it at the USB edge.** `MouseReport` axes
become `i16` and `merged_with` saturates at the `i16` range.
`EndpointDelivery::take` gives the mouse worker at most ±127 per axis, wheel
and pan included, and leaves the remainder at the queue head, so one BLE report becomes as many USB reports as its motion needs,
one per `USB_HID_POLL_MS` (1 ms) poll. A failed write replays held buttons and
drops the remainder; motion is never replayed. Chunks never reach −128, so the
three-byte boot report carries them unchanged.

**Keep the fixed decoders for fixed-layout reports.** A report whose fields are
exactly a fixed layout (8-byte keyboard: modifier bits 0–7, nothing translated
in bits 8–15, six 8-bit key elements with usage equal to value; 3- to 5-byte
mouse: up to eight buttons from bit 0, then 8-bit signed X, Y, optional Wheel
and AC Pan; 2-byte consumer array) uses today's decoder plus the table's range
rule; a differential test holds both paths byte-identical. Any other layout
uses extraction. With a map, length never selects a decoder.

**Fail explicitly, at the narrowest level.**

| Level | Example | Result | Log |
| --- | --- | --- | --- |
| Map | Truncated item, unbalanced collection, nesting over 16, Report ID 0, an input item before the first Report ID of a map that uses IDs | Connection fails with `ReportMapInvalid` (`Unsupported HID map`), as today | As today |
| Map | No input report feeds any kind | Same tag; the slot worker reports it and stops retrying, as for every failure but a background reconnect's `ConnectFailed` (slot_worker.rs 154–160) | `HID map has no translatable report` |
| Report | Over 61 bytes, table full, a field over 32 bits or spanning more than four bytes, Delimiter in its items (today Delimiter fails the whole map) | Not subscribed; other reports work | `HID report {} not translated: {}` |
| Kind | Absolute X or Y, two X fields, unsigned relative axis, array over 16 elements | Kind dropped from that report; other kinds translate | Same line |
| Element | Key above `0xFF`, consumer above `0x0FFF`, button 6, out-of-range array value | Ignored, as released | None |
| Notification | Length differs from the table's | Dropped | `HID report {} is {} bytes; map says {}`, once per report per link |

A report longer than the link's `att_mtu() − 3` is logged at subscription. An
absent Report Map keeps today's length rules until "Report Map
interoperability and legacy policy" decides on them.

**Leave the USB side unchanged**: descriptors, 8-byte endpoints, boot
subclass, `SET_PROTOCOL`, and the boot mouse. Out of scope: System Control,
quirks, and passthrough ([input fidelity](../../TODO.md#input-fidelity)); NKRO
and 16-bit USB reports ("ADR: USB interface and report extensions",
[shared decisions](../../TODO.md#shared-decisions)); LED output layouts other
than the boot byte, which `write_leds` keeps writing.

### Open Questions For The Owner

1. **Motion beyond ±127 per report:** `split` (recommended), `clamp`.
2. **Length that differs from the map:** `strict` (recommended), `pad`
   (zero-fill short reports and ignore extra bytes, as Linux's hid-core does;
   general knowledge).
3. **Fixed decoders for fixed-layout reports:** `keep` (recommended), `remove`.
4. **A map with no translatable report:** `fail` (recommended), `connect`.

## Alternatives Considered

- **Add more fixed layouts.** Matching known NKRO and mouse layouts by length
  repeats today's flaw: same-length layouts differ and packed fields sit at
  any bit offset. Every new device would need code.
- **Store every control, or re-parse the map per notification.** A 256-key
  bitmap alone is 256 usages per link, and keeping the 512-byte map costs more
  than the table and repeats validation in `softdevice_task` per report.
- **Translate each report into a whole kind state**, today's model. Keys split
  across two report IDs, or a switch between six-key and bitmap reports, make
  each report overwrite the other: keys flicker, or a key released in one
  report stays held from the other.
- **BLE boot protocol.** Protocol Mode 0 with the Boot Keyboard and Mouse
  Input Reports (`0x2A22`, `0x2A33`) loses consumer keys, more than six keys,
  and wheels, needs boot support in the peripheral, and switches the whole
  device. It may become a per-device fallback.
- **Mirror the Report Map on USB.** Passthrough bypasses aggregation and
  release on disconnect and cannot be a boot device ("ADR: HID passthrough for
  other device classes", [input fidelity](../../TODO.md#input-fidelity)).
- **A third-party parser crate.** None is in the tree; one would be a new
  peer-facing parser and a material dependency to vet
  ([dependency hygiene](../code-quality.md#dependency-hygiene),
  [ADR 0013](0013-pinned-toolchain-and-mask-tasks.md)). No survey of
  crates.io was made for this proposal.
- **Clamp motion to ±127** (question 1). Simpler and today's behavior, but a
  12- or 16-bit mouse at speed reports more counts per connection interval, so
  the pointer would move less than the hand.
- **Extraction only, removing the fixed decoders** (question 3). Less code,
  but every device that works today would move to new code at once, and the
  differential test would lose its reference.

## Rationale

A table of roles is the smallest structure that says which bits feed which
USB field. Built once and bounded by constants, it makes per-notification work
in `softdevice_task` a walk over at most 488 bits. Failing per report and kind
keeps a keyboard typing beside an unsupported vendor report, and a device with
nothing translatable gets a visible error, not a silent connection. Last-writer
state copies what a PC does, so a peripheral that works paired directly works
through the bridge. Splitting motion uses the 1 ms poll already advertised, so
travel is kept without a new descriptor, in boot protocol too. The fixed
decoders keep today's devices on today's code and give the new path an oracle.
All of it is pure and host-testable ([ADR 0003](0003-pure-core-and-task-shell.md)).

## Consequences

Positive:

- NKRO keyboards type, with six-key rollover over USB; 12- and 16-bit mice,
  packed buttons, combined reports, and split or mode-switching keyboards
  translate as declared. No same-length layout is misread; every drop is logged.
- A consumer usage above `0x0FFF` releases the previous usage instead of
  leaving it held, because the report is no longer dropped whole.
- The promise holds: the host sees the same interfaces and boot devices, so
  firmware setup, KVMs, and hosts without Bluetooth or software are unaffected,
  and a fast flick in firmware setup moves the full distance.

Negative and risks:

- More parser code faces peer data; checked arithmetic, the capacities, and
  fuzzing are mandatory, and the
  [input validation](../security.md#input-validation-boundaries) rows change.
- A peripheral whose map misdescribes its reports, which may work today when
  its length happens to fit, stops working until a quirk covers it.
- Static RAM grows by about 1.8 KiB for two links (estimate: table 0.7 KiB and
  held state 0.2 KiB per link), plus under 0.2 KiB for wider mouse reports in
  the channel and queues; parsing briefly uses about 0.5 KiB of stack, more if
  the table is returned by value through `discover_and_subscribe`; code grows
  by an estimated 4 to 8 KiB. No size figure exists, so the change records
  `mask size` before and after and the stack high-water mark after driving the
  largest fixture's map
  ([budgets](../code-quality.md#binary-size-and-memory-budgets)).
- Translation adds an estimated few thousand cycles per notification in
  `softdevice_task`, to be measured; the power effect is negligible next to
  the radio ([ADR 0012](0012-bus-powered-no-system-off.md)). `MouseReport`,
  `ReportCoalescer`, and `EndpointDelivery` and their tests change, amending
  ADR 0005.
- A split move takes one USB poll per chunk. A compliant host polls a
  full-speed interrupt endpoint at least once per `bInterval` (USB 2.0, 5.7.4;
  general knowledge), but a KVM that emulates the mouse and polls it at its
  own rate (general knowledge) stretches the move over more time, and a mouse
  FIFO that fills is still cleared as ADR 0005 states, losing the remainder.

## Implementation

| Concern | Where |
| --- | --- |
| Item walker, globals, local usages, bit counters | New `src/hid/report_map.rs`, replacing `HidDescriptor::parse`; [report_protocol.rs](../../src/hid/report_protocol.rs) keeps `ReportReference`, `ReportType`, `ReportKind`, and the usage enums |
| Roles, capacities, fixed-layout detection | New `src/hid/report_table.rs` (`ReportTable`) |
| Held state, extraction, emission | New `src/hid/translator.rs`: `LinkTranslator::translate(report_id, payload)` returns up to three `HidReport`s or a drop reason |
| Legacy path | `classify_report` and `infer_from_length` in [hid/mod.rs](../../src/hid/mod.rs), only without a map |
| Wide motion and split | [mouse.rs](../../src/hid/mouse.rs), [coalesce.rs](../../src/hid/coalesce.rs), `EndpointDelivery::take` in [delivery.rs](../../src/hid/delivery.rs) |
| Shell | [hid_client.rs](../../src/ble/hid_client.rs): `read_report_map` builds the table; `subscribe_all` subscribes translatable reports and checks `att_mtu() − 3`; `on_hvx` bounds at 61 bytes; `run_notification_loop` owns the `LinkTranslator`. The MTU becomes a [config.rs](../../src/config.rs) constant shared with [sd_setup.rs](../../src/sd_setup.rs) |

The new modules sit in `src/hid/`, which `lib.rs` exports whole, so they are
pure and host-compiled; each stays under 500 lines
([file length](../code-quality.md#file-length)).
Host tests: items, Push/Pop of every global, extended usages, the 6.2.2.8 rule,
and every truncated prefix of every fixture failing closed (new
`hid_report_map_tests.rs`); extraction at offsets 0–31 and widths 1–32 with
sign extension, each role, last-writer switching, rollover, ignored elements,
length mismatch, and combined reports (new `hid_translate_tests.rs`); 300 split
into 127, 127, 46, and a failure mid-split never replaying motion. A corpus
(new `src/hid_fixtures/`) pairs Report Maps with expected tables and payload
cases: the firmware's three descriptors; synthetic boot, six-key, NKRO, hybrid,
explicit-usage consumer, 12- and 16-bit mouse, combined, absolute-pointer, and
gamepad maps; and maps captured from named peripherals with device, firmware,
capture method, and hardware-result issue, never keystrokes
([fixtures](../testing.md#keep-fixtures-small-and-real)). A differential test
runs both paths on every fixed-layout fixture; a seeded mutation test in
`cargo test` asserts no panic, bounded work, table invariants, and USB
contracts on mutated maps and payloads; a `cargo-fuzz` target, which needs
nightly outside the pinned toolchain (general knowledge), joins "Parser
fuzzing and property tests". Renode is not cycle-accurate, so it cannot time
the callback; a new `sim.rs` step, which adds the `hid` module to the
simulation build (today it has only `ble`, `config`, and `ui`), can run the
corpus on the emulated Cortex-M4 with its 32-bit `usize` and report through
the [Robot test](../testing.md#robot-test-case). Hardware
([ADR 0004](0004-layered-verification.md)): DWT cycles for the worst fixture; a
named NKRO keyboard rolling over at seven keys and a named 12- or 16-bit mouse
moving fast, in firmware setup and through a KVM, in the compatibility baseline.

Acceptance, once listed in the architecture index, completes "ADR:
descriptor-driven HID report translation". It unblocks "Descriptor-driven
report translation", the acceptance of "Report Map interoperability and legacy
policy", the BLE side of "More than six keys on the USB keyboard" and
"High-resolution mouse movement and scrolling", a role for "Power, sleep, and
wake keys", "Per-peripheral quirks" (which would patch the table), and the
unsupported-report case of "Visible storage/security errors".

### Verification Status

- **Implemented:** nothing of this proposal. Today `HidDescriptor::parse`
  extracts routing metadata (three kinds, one ID each, bounded nesting,
  Push/Pop, long items, no offsets) and `parse_by_kind` decodes 8-, 3- to 5-,
  and 2-byte layouts with the reserved-byte rule.
- **Software-verified:** today's path only, over the firmware's own
  descriptors and synthetic arrays: 34 tests in `hid_descriptor_tests.rs`, 17
  in `hid_classify_tests.rs`, 10 in `hid_keyboard_report_tests.rs`, 36 report
  parsing and serialization tests in `lib_tests.rs`, and 3 notification round
  trips in `tests/integration.rs`. No real Report Map, no fuzzing, and no
  Renode scenario touches HID.
- **Hardware-verified:** not yet; no real peripheral's map or report recorded.

## Related

- [Architecture: HID path and limits](../architecture.md#hid-path-and-limits), [one input report](../architecture.md#one-input-report-from-ble-to-usb),
  [decisions needed](../architecture.md#decisions-needed-for-roadmap-work)
- [Data model: USB HID report contracts](../data-model.md#usb-hid-report-contracts), [accepted BLE payloads](../data-model.md#accepted-ble-payloads),
  [HidEvent and HidReport](../data-model.md#hidevent-and-hidreport); [features: translation](../features.md#translation),
  [HID discovery and Report Maps](../features.md#hid-discovery-and-report-maps), [current technical boundaries](../features.md#current-technical-boundaries)
- [Security: input validation boundaries](../security.md#input-validation-boundaries), [threat model](../security.md#threat-model);
  [testing: feed malformed and truncated input](../testing.md#feed-malformed-and-truncated-input), [keep fixtures small and real](../testing.md#keep-fixtures-small-and-real),
  [known verification gaps](../testing.md#known-verification-gaps)
- [Code quality: binary size and memory budgets](../code-quality.md#binary-size-and-memory-budgets), [dependency hygiene](../code-quality.md#dependency-hygiene);
  [operations: connect fails with an HID error](../operations.md#connect-fails-with-an-hid-error); [first flash: in the monitor](../first-flash.md#5-in-the-monitor)
- [ADR 0003](0003-pure-core-and-task-shell.md), [ADR 0004](0004-layered-verification.md), [ADR 0005](0005-two-slots-and-independent-endpoints.md),
  [ADR 0007](0007-vendored-softdevice-patch.md), [ADR 0010](0010-static-memory-layout.md), [ADR 0012](0012-bus-powered-no-system-off.md),
  [ADR 0013](0013-pinned-toolchain-and-mask-tasks.md)
- TODO.md: [HID report parsing and translation](../../TODO.md#hid-report-parsing-and-translation), [BLE central and pairing](../../TODO.md#ble-central-and-pairing),
  [input fidelity](../../TODO.md#input-fidelity), [shared decisions](../../TODO.md#shared-decisions),
  [verification and code quality](../../TODO.md#verification-and-code-quality), [UI, display and power](../../TODO.md#ui-display-and-power),
  [board bring-up and hardware acceptance](../../TODO.md#board-bring-up-and-hardware-acceptance)
