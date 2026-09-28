# Firmware architecture

bt2usb uses `no_std` Rust, static allocation, and Embassy's cooperative async
executor. Nordic SoftDevice S140 supplies the BLE central stack. USB uses the
nRF52840 device peripheral through Embassy. The UI runs in `main`; a separate
display task renders its latest state over async I2C.

## Source map

| Area | Responsibility |
| --- | --- |
| [main.rs](../src/main.rs) | Hardware setup, task spawning, channels, UI loop |
| [sim.rs](../src/sim.rs) | SoftDevice-free Renode entry point and synthetic BLE scenario |
| [selftest.rs](../src/selftest.rs) | Staged board bring-up image |
| [sd_setup.rs](../src/sd_setup.rs) | Shared SoftDevice setup |
| [lib.rs](../src/lib.rs) | Host-test entry point for hardware-free logic |
| [ble/coordinator.rs](../src/ble/coordinator.rs) | Pure connection-slot and event reducers |
| [ble/multi_conn.rs](../src/ble/multi_conn.rs) | BLE coordinator and two connection workers |
| [ble/hid_client.rs](../src/ble/hid_client.rs) | GATT discovery, subscriptions, HID classification |
| [ble/long_read.rs](../src/ble/long_read.rs) | Bounded fragmented Report Map acquisition |
| [ble/management.rs](../src/ble/management.rs) | Peer-management quiescence and transactional commit primitives |
| [ble/scanner.rs](../src/ble/scanner.rs) | Scan and HID advertisement filtering |
| [hid/](../src/hid/) | Report types, descriptors, per-source aggregation, endpoint delivery, wake policy |
| [usb/hid_device.rs](../src/usb/hid_device.rs) | Composite device, LED output, VBUS, suspend, report writing |
| [storage.rs](../src/storage.rs) | Paired-device persistence with versioned framing and bond codec |
| [ui/](../src/ui/) | Display, button tasks, pure UI transitions |
| [power_logic.rs](../src/power_logic.rs) | Pure power/display policy |
| [stack.rs](../src/stack.rs) | Painted-stack high-water measurement |

## Tasks and data flow

```mermaid
flowchart TD
    SD[SoftDevice task] --> BLE[BLE coordinator]
    BLE --> SLOT[Two BLE connection workers]
    SLOT -->|Source-tagged input and disconnect events| DISPATCH[Input aggregator / dispatcher]
    DISPATCH --> KB[Keyboard endpoint worker]
    DISPATCH --> MOUSE[Mouse endpoint worker]
    DISPATCH --> MEDIA[Consumer endpoint worker]
    KB --> USB[USB device task]
    MOUSE --> USB
    MEDIA --> USB
    BUTTONS[Three GPIO button tasks] --> UI[Main UI loop]
    UI -->|Commands| BLE
    BLE -->|Events| UI
    UI -->|Latest display state| DISPLAY[OLED task]
    USB -->|Latest keyboard LED state| SLOT
    SD -->|USB power events| USB
```

Bounded Embassy channels use `CriticalSectionRawMutex`. The BLE coordinator sends
commands to per-slot workers; a shared GAP mutex serializes scan and connection
establishment because SoftDevice permits one such procedure at a time. A link
does not hold that mutex for its whole lifetime. A requested scan can therefore
wait for an in-flight connection attempt.

| Channel | Direction | Capacity |
| --- | --- | --- |
| `HID_REPORT_CHANNEL` | BLE workers → input aggregator/dispatcher | 16 |
| `BLE_CMD_CHANNEL` | UI → coordinator | 4 |
| `BLE_EVENT_CHANNEL` | Coordinator → UI | 8 |
| `BLE_SLOT0_CMD_CHANNEL` / `BLE_SLOT1_CMD_CHANNEL` | Coordinator → worker | 2 each |
| `BLE_SLOT_EVENT_CHANNEL` | Workers → coordinator | 8 |
| `BUTTON_CHANNEL` | GPIO tasks → UI | 4 |

The USB keyboard LED state uses a `Watch`, so both slots can observe the latest
state. USB suspend/resume and wake requests use signals and atomic state.

The dispatcher does not wait for USB writes. Keyboard, mouse, and consumer
workers run concurrently, each with a 16-report FIFO, a 100 ms write deadline,
and 20–1000 ms retry backoff. An unpolled endpoint cannot block the other two.
Reset, configuration, resume, and protocol transitions invalidate old transfers
and replay current held state; relative mouse motion is never replayed.

## HID path and limits

Each input travels through GATT notification decoding, descriptor/report-reference
classification, a fixed internal report type, and a USB endpoint. Supported USB
reports are an 8-byte, six-key keyboard report, mouse buttons with signed 8-bit
movement and scroll, and consumer control. Mouse translation includes five
buttons and horizontal scrolling for compatible layouts. The keyboard and mouse
interfaces advertise the USB boot subclass and handle boot/report protocol
selection; boot mouse output uses the three-byte layout. Actual pre-OS behavior
is a hardware acceptance item.

The descriptor parser is a classifier for supported report layouts, not a full
HID bit-field translator. NKRO, 16-bit movement, and arbitrary vendor layouts need
additional translation. Descriptor parsing bounds collection/global-state
nesting and supports Push/Pop, but retains at most one report ID per supported
kind. GATT discovery and notification buffers are bounded; see the current
constants in `hid_client.rs` when adding report families.

Report Maps are read by offset across negotiated MTU boundaries into a bounded
512-byte buffer using a small vendored SoftDevice API patch. Present but
unreadable, malformed, or oversized maps fail discovery with a specific error.
Only an absent Report Map characteristic permits legacy fixed-layout/length
classification. Neither that fallback nor report-kind classification proves
an arbitrary same-length layout is supported; field-level translation remains
open work.

The aggregator tracks each BLE slot independently. Keyboard keys/modifiers and
mouse buttons are unioned, so releasing or disconnecting one source preserves
the other's held input. More than six unique keys produces the keyboard
`ErrorRollOver` array. The one-usage consumer interface gives the lowest active
slot priority and falls back to the other slot when that input is released.
Mouse movement belongs only to the current event and is not unioned as held state.

Normal endpoint traffic retains FIFO ordering. While unavailable or recovering,
an endpoint keeps the latest absolute state. Saturated queues collapse to current
state, so intermediate taps or relative motion may be lost under sustained
overload. The policy prioritizes bounded memory and final releases. Host tests
include actual asynchronous workers with fake sinks; real-device timing and
recovery still need hardware acceptance.

## Pairing and storage

The store records BLE addresses, names, RSSI hints, and optional bonding keys in
four reserved flash pages through `sequential-storage`. Its codec and versioned
framing are separate from flash I/O. Complete records, names, address types, and
bond boundaries are validated before loading. An invalid, unsupported, or
unreadable store disables writes instead of being silently replaced.

Bonded records use stable peer identities. Background reconnects resolve a peer's
current advertising address on each attempt, including when its private address
rotates after boot. Up to two recent stored devices are selected at boot. New
pairing is initiated for explicit user connections; background reconnects use
existing keys. HID discovery waits for an encrypted link, and commands can cancel
security/discovery once the connection is owned.

Disconnecting closes connections while retaining their records. Separate
confirmation-based Forget and Factory reset operations stop affected workers and
wait for their source/retry cleanup before changing records. The in-memory store
and bonder change only after persistence succeeds. Factory reset can explicitly
erase an unreadable storage region to recover it; ordinary writes cannot.
Logical deletion is not a physical key-erasure guarantee. Power-loss behavior,
authenticated pairing, and physical flash protection remain release work in
[TODO.md](../TODO.md).

## Execution and power choices

Tasks yield on I/O; there are no per-task stacks or a dynamic allocator. Blocking
work in one task can still delay the whole executor. `defmt` supplies compact RTT
diagnostics. `probe-rs` flashes and runs firmware through a debug probe.

The bus-powered design keeps BLE links responsive rather than entering
System-OFF. Input activity informs display power policy; the OLED can turn off
without disconnecting devices. USB remote wakeup requires a newly pressed
key/modifier, consumer usage, or mouse button during suspend. Motion, scroll,
releases, repeated held state, and recovery replay do not wake the host.

UI errors and completion notices remain until acknowledged; incoming background
status changes do not silently replace them. Display updates use the latest
state. A failed OLED operation retries with bounded exponential backoff. A 500 ms
operation deadline requests TWIM STOP but retains the DMA future until completion;
cancelling that future is unsafe in the pinned HAL. If the hardware never reports
completion, the isolated display task stays degraded while the bridge and UI
continue. There is no general watchdog recovery guarantee yet.
