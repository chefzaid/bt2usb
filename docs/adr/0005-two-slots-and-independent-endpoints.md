# ADR 0005: Aggregate Two BLE Sources Into Independent USB Endpoint Workers

- Status: Accepted; the boot-reconnect bullet is superseded by
  [ADR 0015](0015-shared-reconnect-scan.md) (2026-10-09)
- Date: 2026-09-28

## Context

A typical desk has one keyboard and one mouse, sometimes two keyboards. The
bridge has supported two simultaneous BLE links since the embedded
implementation landed on 2026-02-21 (`8e6dd17`): `MAX_CONNECTIONS` was 2 in
`multi_conn.rs` (it now lives in the coordinator), and the SoftDevice was
configured for two central links. The
interesting question is what happens between those two links and the three USB
interfaces the host sees.

USB HID keyboard and mouse-button reports carry absolute state: the host keeps a
key pressed until a report says otherwise. That makes three failures possible:

- **Stuck input.** If a link drops while a key is held, its release never
  arrives over BLE, and the host auto-repeats the key.
- **One source overwriting another.** Two peripherals that share a USB
  interface each send only their own state, so the latest report replaces the
  other device's held keys.
- **Head-of-line blocking.** The host may stop polling one interface while
  another is still active, and reset, suspend, resume, or unplug can happen at
  any moment.

The design before this decision had all three weaknesses. In the 2026-09-26
code (`f477d4c`), the BLE workers sent untagged `HidReport` values through
`HID_REPORT_CHANNEL`, and one `hid_writer_task` awaited each endpoint write in
turn (`keyboard.write(bytes).await`, then the next report). A host that stopped
polling the consumer interface therefore stalled the keyboard and mouse as
well. `src/hid/held.rs` released a dropped link's input by sending all-released
reports for each kind that link held, which also cleared keys held through the
other slot when both links held the same kind. Any report that arrived while the
bus was suspended requested a remote wakeup, including mouse motion.

Two lower-level facts shape the solution:

- The GATT notification callback in `hid_client.rs` is synchronous and cannot
  await channel space. Since 2026-06-23 (`dc11b4a`) it pushes into a
  `ReportCoalescer`, and a drain future forwards to the channel with
  backpressure.
- The SoftDevice runs one locally initiated scan or connection establishment at
  a time; a second one fails with `NRF_ERROR_INVALID_STATE` (comment on
  `GAP_PROCEDURE` in [ble/mod.rs](../../src/ble/mod.rs)).

## Decision

**Two connection slots, each owned by one worker.**

- Run two connection slots (`MAX_CONNECTIONS = 2`) with up to four stored peers
  (`MAX_PAIRED_DEVICES = 4`). Each slot has its own task
  (`connection_slot_task`) that alone owns its link, its security and discovery
  phase, its notification loop, and its retry target.
- Serialize scan and connection establishment with the shared `GAP_PROCEDURE`
  mutex, held only for establishment, never for a link's lifetime. Bound the
  whitelist scan of each connection attempt (the `ScanConfig` timeout) and each
  reconnect scan (`with_timeout`) by `BLE_CONNECT_TIMEOUT_SECS`
  (6 s), so an absent peer cannot hold the lock indefinitely, and pause
  `BLE_RECONNECT_BACKOFF_MS` (500 ms) between silent retries so a user scan can
  get the lock. A user scan holds the lock for its `BLE_SCAN_DURATION_SECS`
  (8 s) window, with a 2 s hard backstop.
- At boot, select up to two most recently stored peers, scan once, match each
  to a live address (resolving private addresses by IRK), and give each a slot;
  a peer not seen in that scan keeps its stored address. Every later silent
  retry of a bonded peer resolves its current address again. *Superseded by
  [ADR 0015](0015-shared-reconnect-scan.md) on 2026-10-09: there is no boot
  scan, and one shared reconnect scan looks for both slots' devices.*

**Source-tagged input and a per-source union.**

- Tag every input with its source slot: `HidEvent::Report { source, report }`.
  When a link ends, the worker sends `HidEvent::Disconnected { source }` after
  its last report.
- Keep each source's absolute state separately. Union keyboard keys, modifiers,
  and mouse buttons across sources. Give the single consumer usage to the
  lowest-numbered active slot, falling back to the other slot on release or
  disconnect. Treat relative mouse motion and scroll as belonging only to the
  current report, never as held state.
- Send the keyboard `ErrorRollOver` array when the union holds more than six
  keys, or when a source reports a rollover error itself.
- On `Disconnected`, clear only that source and publish the new union for all
  three endpoints.

**Independent, bounded endpoint workers.**

- A dispatcher applies each event to the aggregator and publishes the result to
  per-endpoint mailboxes without waiting for any USB write.
- Keyboard, mouse, and consumer each have their own worker, a FIFO of 16
  reports (`ENDPOINT_QUEUE_CAPACITY`), a 100 ms write deadline, and retry
  backoff that starts at 20 ms, doubles, and caps at 1000 ms.
- Normal traffic keeps FIFO order, so short press and release sequences
  survive. A full FIFO is cleared and keeps only the newest report. While the
  bus is unavailable or an endpoint is recovering, the queue holds only the
  latest durable state: held keys and buttons, without motion.
- On USB reset, configuration change, suspend or resume, or a protocol change,
  advance an epoch that invalidates in-flight transfers, then replay current
  held state. Never replay relative motion. A completion from an older epoch
  cannot overwrite newer input.

**Wake only on a new press.** Request USB remote wakeup only for a newly pressed
key, modifier, consumer usage, or mouse button. Motion, scroll, releases,
repeated held state, disconnect cleanup, and recovery replay never wake the
host.

## Alternatives Considered

- **One writer that awaits each endpoint in turn.** This was the 2026-09-26
  design. It is simple, but one endpoint that the host stops polling blocks the
  others indefinitely.
- **Last report wins per endpoint.** Forwarding each source's report unchanged
  needs no aggregation, but two keyboards, or a keyboard and a mouse that both
  send keyboard reports, overwrite each other's held keys.
- **Release a dropped link by sending all-released reports.** This was
  `held.rs` (2026-09-26). It clears the stuck key, but it also clears keys held
  through the other slot on the same interface.
- **Backpressure all the way to BLE.** Letting the dispatcher wait for USB would
  push a stalled endpoint back into the BLE workers, and the GATT callback
  cannot wait at all. The coalescer applies backpressure up to the dispatcher;
  the endpoints absorb the rest with bounded, collapsing queues.
- **Larger or unbounded queues.** Memory is statically allocated
  ([ADR 0002](0002-nrf52840-softdevice-embassy.md)), and a longer queue only
  delays the moment it fills during a long stall.
- **More than two slots.** The SoftDevice can be configured for more links, but
  every slot adds a task, channels, SoftDevice RAM, and GAP contention, and the
  RAM reservation in `memory_sd.x` is sized for two links. Two covers the
  target desk.
- **An NKRO keyboard report.** A bitmap report would remove the six-key limit,
  but it changes the USB descriptor and is not the boot-protocol format that
  pre-OS hosts read. Six-key rollover with `ErrorRollOver` keeps one report
  format for both protocols.
- **Wake on any input while suspended.** That was the 2026-09-26 behavior. It is
  simpler, but motion and releases are not deliberate wake requests.

## Rationale

Per-source state makes "release one device, keep the other's keys" correct by
construction instead of a special case. A disconnect clears exactly one source,
and the union that remains is what the host should see.

Separating dispatch from delivery means an unpolled endpoint can only delay its
own reports. Bounded queues keep memory fixed; clearing a full queue down to
its newest report chooses to deliver the final state, including releases, over
every intermediate tap. For absolute-state reports that is the right trade: a
stuck key is worse than a lost tap.

Epochs make recovery deterministic: after a bus change the host receives
exactly the held state, and a transfer that finishes late cannot undo it. Never
replaying motion avoids a cursor jump after resume.

Waking only on a new press matches user intent. A mouse nudged by a desk
vibration, or a key that was already down when the host suspended, should not
wake a sleeping PC.

## Consequences

Positive:

- Releasing or disconnecting one device keeps the other device's held input.
- An endpoint the host stops polling cannot block the other two.
- After any bus change the host receives the current held state once, without
  stale transfers or motion.
- Memory use is fixed regardless of how long USB stalls.

Negative:

- Under sustained overload, intermediate taps and accumulated relative motion
  can be dropped; the final state always arrives.
- More than six unique keys produces the rollover error report instead of the
  keys.
- Only one consumer usage reaches the host at a time, so a second device's media
  key waits until the lower slot releases its own.
- The SoftDevice's one-procedure rule means a user scan can wait behind
  in-flight connection attempts or address resolutions, each bounded by
  `BLE_CONNECT_TIMEOUT_SECS`.
- Adding a third slot or a new endpoint means revisiting channel capacities,
  GAP contention, SoftDevice RAM, and this policy.
- None of this is verified on hardware yet (see
  [Verification Status](#verification-status)).

Follow-up obligations, tracked in [TODO.md](../../TODO.md):

- "Multi-device aggregation hardware acceptance": validate the unions and
  consumer priority with two real peripherals on one host.
- "Backpressure behavior and bounded recovery": specify and measure the
  acceptable loss and recovery bound under stalls, suspend, and unplug.
- "HID/USB conformance" and "Descriptor-driven report translation": settle
  `GET_REPORT` and `SET_IDLE` behavior and decode layouts beyond the supported
  ones.
- "Soak and latency measurements" and "Async task fault tests".

## Implementation

| Concern | Where |
| --- | --- |
| Slot count and reducers | `BLE_MAX_CONNECTIONS` in [config.rs](../../src/config.rs), which sizes every per-link array, pool, and SoftDevice count; `MAX_CONNECTIONS` and `ConnManager` in [coordinator.rs](../../src/ble/coordinator.rs) |
| Slot workers, boot reconnect, link end | `connection_slot_task` in [slot_worker.rs](../../src/ble/slot_worker.rs), `connect_and_run_secure` in [slot_link.rs](../../src/ble/slot_link.rs), and `ble_task` in [multi_conn.rs](../../src/ble/multi_conn.rs); the log lines `slot {} link lost; reconnecting` and, when `Bonder` holds no keys for the device and the slot is freed instead, `slot {} link lost; no keys to reconnect` |
| Shared reconnect scan and address resolution ([ADR 0015](0015-shared-reconnect-scan.md)) | `ReconnectTable` and `owner_of` in [reconnect.rs](../../src/ble/reconnect.rs); `find_saved_peer` in [scanner.rs](../../src/ble/scanner.rs) |
| GAP serialization | `GAP_PROCEDURE` in [ble/mod.rs](../../src/ble/mod.rs) |
| Timing constants | `BLE_CONNECT_TIMEOUT_SECS`, `BLE_RECONNECT_BACKOFF_MS`, `BLE_CONN_EVENT_LENGTH`, `BLE_FAST_SCAN_INTERVAL`, `BLE_FAST_SCAN_WINDOW`, `BLE_FAST_RECONNECT_SECS` in [config.rs](../../src/config.rs) |
| Synchronous-callback hand-off | `ReportCoalescer` in [coalesce.rs](../../src/hid/coalesce.rs), driven by `run_notification_loop` in [hid_client.rs](../../src/ble/hid_client.rs) |
| Source tags | `HidEvent` in [delivery.rs](../../src/hid/delivery.rs); `HID_REPORT_CHANNEL` (capacity 16) in [main.rs](../../src/main.rs) |
| Aggregation | `InputAggregator::apply` and `SOURCES` (the link count) in [aggregate.rs](../../src/hid/aggregate.rs); `MAX_CONSUMER_USAGE` (`0x0FFF`) in [consumer.rs](../../src/hid/consumer.rs) |
| Endpoint policy | `EndpointDelivery` (`publish`, `replay`, `failed`, `succeeded`, epochs) and `run_endpoint` in `delivery.rs` |
| USB side | `dispatch_reports`, `hid_writer_task` (one dispatcher and three workers joined), and `EndpointMailbox` in [hid_device.rs](../../src/usb/hid_device.rs); `UsbPowerHandler` calls `replay_endpoints` from its `reset`, `configured`, and `suspended` callbacks, and `BootRequestHandler::set_protocol` in [host_requests.rs](../../src/usb/host_requests.rs) replays the keyboard or mouse endpoint |
| Wake policy | `new_press` in [wake.rs](../../src/hid/wake.rs); `REMOTE_WAKE` and `run_usb_device` in `hid_device.rs` |
| Keyboard LEDs | A `Watch` with `LED_CONSUMERS` receivers (one per link) in `host_requests.rs`, so whichever slot holds a keyboard with an LED output report forwards host LED state; `forward_host_leds` in [host_leds.rs](../../src/hid/host_leds.rs) writes the current state when it starts on a link, after that link's PnP ID read, then every change |

### Verification Status

- **Implemented:** everything in the table above.
- **Software-verified:** host tests cover the policy with the production code:
  `same_key_held_by_two_sources_survives_release_and_disconnect`,
  `keyboard_rollover_recovers_when_one_source_disconnects`,
  `mouse_unions_buttons_without_replaying_other_sources_motion`, and
  `consumer_priority_falls_back_and_shared_usage_survives_disconnect` in
  `aggregate.rs`; `queue_overflow_preserves_final_release` and
  `stale_completion_cannot_erase_post_reset_input` in `delivery.rs`; and the
  actual worker against fake endpoints in
  [delivery_tests.rs](../../src/hid/delivery_tests.rs), for example
  `unpolled_consumer_allows_actual_keyboard_and_mouse_workers_to_write` and
  `repeated_usb_errors_use_capped_backoff_and_eventually_recover`. The
  coordinator and the reconnect table have host tests too, and the Renode
  scenario drives the coordinator reducers on the ARM target. These passed on
  GitHub-hosted runners in push runs 36441995385 (`8a04b25`, 2026-09-28) and
  37932436721 (`7fc99d6`, 2026-10-09) and scheduled run 37338711407
  (2026-10-05). The slot workers, GAP serialization, and the USB side in
  `hid_device.rs` and `host_requests.rs` are not host-tested.
- **Hardware-verified:** not yet. No layer 5 result covers two peripherals on
  one host, endpoint stalls, or suspend and resume.

## Related

- [Architecture: tasks and data flow](../architecture.md#tasks-and-data-flow)
- [Architecture: HID path and limits](../architecture.md#hid-path-and-limits)
- [Data model: USB HID report contracts](../data-model.md#usb-hid-report-contracts)
- [Features: input delivery](../features.md#input-delivery)
- [ADR 0003: Hardware-free decision modules](0003-pure-core-and-task-shell.md)
- [ADR 0006: Fail-closed pairing store](0006-fail-closed-pairing-store.md)
- [ADR 0010: Static memory layout](0010-static-memory-layout.md)
- [ADR 0012: Bus-powered, no System-OFF](0012-bus-powered-no-system-off.md)
