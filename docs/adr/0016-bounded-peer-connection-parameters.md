# ADR 0016: Bound A Peripheral's Connection Parameter Requests In The Application

- Status: Accepted
- Date: 2026-10-09
- Amends: [ADR 0007](0007-vendored-softdevice-patch.md), whose patch is no
  longer confined to `gatt_client.rs`

## Context

The bridge opens every link with the parameters it needs as a wired-keyboard
replacement: a 7.5 to 15 ms connection interval (`BLE_CONN_INTERVAL_MIN`,
`BLE_CONN_INTERVAL_MAX`), no peripheral latency (`BLE_SLAVE_LATENCY`), and a
4-second supervision timeout (`BLE_SUP_TIMEOUT`), all in
[config.rs](../../src/config.rs). The supervision timeout matters most: when a
link fails without a disconnect, for example because the keyboard's battery is
pulled while a key is down, the bridge learns of it only when the timeout
expires, and only then releases the keys that link was holding
([ADR 0005](0005-two-slots-and-independent-endpoints.md)).

A peripheral may ask to change these parameters at any time, with an L2CAP
Connection Parameter Update Request or the Link Layer Connection Parameters
Request procedure. HID peripherals commonly do so to save power, asking for a
longer interval, some peripheral latency, or a longer supervision timeout. The
S140 SoftDevice reports either request as
`BLE_GAP_EVT_CONN_PARAM_UPDATE_REQUEST`, and the central answers with
`sd_ble_gap_conn_param_update`, passing the requested parameters, different
ones, or `NULL` to reject the request.

The pinned `nrf-softdevice` crate handles that event itself, in
`vendor/nrf-softdevice/src/ble/gap.rs`, and granted every request unchanged.
The Bluetooth Core allows an interval up to 4 s, latency up to 499 events, and
a supervision timeout up to 32 s, so a peripheral could make every report wait
for a long interval, or keep a key held on the PC for up to 32 seconds after its
link silently failed. The application never saw the request: the crate's
event handler is private, and no existing callback reached the application.

## Decision

**Answer each request in the application, through one vendored hook.** The
vendored `SecurityHandler` trait gains `conn_param_update_request(&self, conn,
requested) -> ble_gap_conn_params_t`, whose default returns the request
unchanged, as upstream did. The `CONN_PARAM_UPDATE_REQUEST` handler in
`gap.rs` looks up the connection by handle (`Connection::from_handle`), and,
when the link has a security handler, replies with whatever the hook returns;
links without one keep upstream behavior. Every bt2usb link is created by
`central::connect_with_security` with the shared `Bonder`, so every link has
the hook.

**Grant the nearest values inside fixed bounds.** `Bonder` converts the request
and calls the pure `conn_params::bound_request` with `PEER_CONN_PARAM_LIMITS`:

| Parameter | Bound | Source |
| --- | --- | --- |
| Interval | Overlap of the requested range with 7.5–15 ms; for a request entirely slower than 15 ms, its own fastest interval, capped at 30 ms, as a single value; 7.5 ms for a request entirely faster; reversed bounds read as a range | `BLE_CONN_INTERVAL_MIN`, `BLE_CONN_INTERVAL_MAX`, `BLE_PEER_MAX_CONN_INTERVAL` |
| Peripheral latency | At most 20 connection events, lowered further only if 4 s could not otherwise satisfy the Core rule | `BLE_MAX_PERIPHERAL_LATENCY` |
| Supervision timeout | The requested value, kept within 1–4 s and raised if needed so that it exceeds `(1 + latency) × max interval × 2` | `BLE_MIN_SUP_TIMEOUT`, `BLE_SUP_TIMEOUT` |

A request inside the bounds is granted as asked and logged as
`peer connection parameters granted: {}`; any other is logged with both values
as `peer asked for connection parameters {}; granting {}`. When the granted
interval lies outside the requested range (`conn_params::interval_within_request`
is false), the line is a warning and ends in `, outside its interval range`.
That happens in two cases only: the peripheral's fastest requested interval is
slower than 30 ms, so it is granted 30 ms, or its whole range is below 7.5 ms,
the shortest interval the SoftDevice supports, so it is granted 7.5 ms. A
peripheral whose fastest requested interval is 30 ms exactly is granted it
inside its range. The warning lets the hardware compatibility baseline name
the peripheral if it then disconnects.

## Alternatives Considered

- **Keep granting requests unchanged.** This keeps the vendored crate as it
  was, but lets any peripheral, including a misconfigured one, stretch the
  supervision timeout to 32 seconds and with it the time a key can stay stuck.
- **Reject every request (`NULL`).** The link would keep its opening
  parameters, but a peripheral that cannot get any of its requested values may
  keep asking or disconnect, and a reasonable request, such as the latency a
  mouse uses to save battery, would be refused too.
- **Change the parameters afterwards from the application.** Letting the crate
  grant the request and then calling `Connection::set_conn_params` would leave
  a window with the peripheral's values, run two update procedures per request,
  and invite the peripheral to ask again.
- **A new callback type or a global function in the vendored crate.**
  `SecurityHandler` is already the per-connection application object that the
  crate stores for central links, and `gap.rs` already reaches it the same way
  for the Security Request event. A new trait would add more vendored code for
  the same result.
- **Hold every interval to 15 ms.** A request for 20 to 40 ms would then get
  15 ms. Some peripherals check the interval they get against the range they
  asked for and disconnect once their update attempts run out; Nordic's nRF5
  SDK `ble_conn_params` module does so when `disconnect_on_fail` is set, after
  its configured number of attempts (about 95 seconds with a 5-second first
  delay, 30 seconds between attempts, and three attempts). Such a peripheral
  would lose its link every minute or two, releasing held keys and dropping
  input each time, which is worse than a 20 ms interval. The 30 ms cap still
  refuses a peripheral that would slow every report to tens or hundreds of
  milliseconds.
- **Clamp each field separately.** Capping latency and the supervision timeout
  independently can produce a combination that breaks the Core rule, for
  example latency 20 at 15 ms with a 600 ms timeout, which the SoftDevice
  rejects with `NRF_ERROR_INVALID_PARAM`. `bound_request` derives the timeout
  from the granted latency and interval instead.

## Rationale

The bounds keep the guarantees the opening parameters were chosen for. Input
still leaves a peripheral at its next connection event, at most 15 ms away,
or 30 ms for a peripheral that refuses anything faster, because latency only
lets a peripheral skip events when it has nothing to send. What latency
delays is traffic to the peripheral: with latency 20 at 15 ms, an LED write
can wait up to 21 events, about 315 ms, which is still prompt for a Caps Lock
light (about 630 ms at 30 ms). The supervision timeout never exceeds the
4 seconds the bridge opens with, so a key held through a silent link failure
is released within 4 seconds; that bound holds whatever interval is granted.
The 1-second floor keeps a peripheral from making the link drop on a brief
burst of interference.

The policy is a pure function of numbers, so it is host-tested with a sweep
over every boundary of the policy and over values outside the Core's legal
ranges ([ADR 0003](0003-pure-core-and-task-shell.md)).
The vendored change is a default trait method and one call site, which keeps
the patch small and leaves every other SecurityHandler user on upstream
behavior.

## Consequences

Positive:

- No peripheral can make the bridge keep a stuck key for longer than 4 seconds
  after a silent link failure, or stretch the connection interval past 15 ms
  unless it accepts nothing faster, and then not past 30 ms.
- A peripheral that asks for a range slower than 15 ms gets an interval inside
  that range, so a peripheral that checks its interval keeps its link.
- Every answer the bridge gives satisfies the Core rule and the SoftDevice's
  parameter limits.
- The log shows what each peripheral asked for and what it got, which feeds
  the hardware compatibility baseline.

Negative:

- The vendored patch now touches `security.rs` and `gap.rs` as well as
  `gatt_client.rs`, so an upgrade has three files to re-apply.
- A peripheral whose requested range includes 15 ms or less gets at most
  15 ms and may use more power than it planned for.
- A peripheral that refuses anything faster than 20 ms gets 20 ms, so its
  input waits up to 20 ms rather than 15 ms; one whose fastest requested
  interval is slower than 30 ms gets 30 ms, outside its range, and may still
  disconnect. So may one whose whole range is below 7.5 ms, which gets 7.5 ms.
  The warning log line names either.
- A peripheral that insists on its own values may ask again after each answer;
  the bridge answers each request the same way and does not rate-limit them.
- The hook and the event handler have no host tests, and no peripheral's
  negotiated parameters are recorded yet (see
  [Verification Status](#verification-status)).

Follow-up obligations, tracked in [TODO.md](../../TODO.md):

- "Hardware compatibility baseline": record the parameters each named
  peripheral asks for and is granted, and any that disconnects after a
  warning that its interval is outside its range.
- "Soak and latency measurements": confirm that latency 20 does not delay
  input, only LED writes.

## Implementation

| Concern | Where |
| --- | --- |
| Hook | `SecurityHandler::conn_param_update_request` in `vendor/nrf-softdevice/src/ble/security.rs`; called from the `BLE_GAP_EVTS_BLE_GAP_EVT_CONN_PARAM_UPDATE_REQUEST` arm in `vendor/nrf-softdevice/src/ble/gap.rs`; both recorded in [README.bt2usb.md](../../vendor/nrf-softdevice/README.bt2usb.md) |
| Policy | `ConnParams`, `ConnParamLimits`, `bound_request`, `interval_within_request`, and `min_supervision_timeout` in [conn_params.rs](../../src/ble/conn_params.rs) |
| Limits and logging | `PEER_CONN_PARAM_LIMITS` and `Bonder::conn_param_update_request` in [multi_conn.rs](../../src/ble/multi_conn.rs) |
| Constants | `BLE_CONN_INTERVAL_MIN`, `BLE_CONN_INTERVAL_MAX`, `BLE_PEER_MAX_CONN_INTERVAL`, `BLE_MAX_PERIPHERAL_LATENCY`, `BLE_MIN_SUP_TIMEOUT`, `BLE_SUP_TIMEOUT` in [config.rs](../../src/config.rs) |

### Verification Status

- **Implemented:** everything in the table above.
- **Software-verified:** 14 host tests in `conn_params.rs` cover a request
  granted unchanged, a 20–40 ms request granted 20 ms, a 50–100 ms request
  granted 30 ms and flagged as outside its range, a request whose fastest
  interval is 30 ms granted it inside its range, an overlapping range
  narrowed to the overlap, a 32-second timeout capped at 4 seconds, a short
  timeout raised to 1 second, latency capped at 20, reversed interval bounds
  (also in `interval_within_request`), a request entirely below 7.5 ms
  granted 7.5 ms and flagged, a request below a raised floor, latency
  lowered when the timeout cap cannot cover it, the latency limit checked as
  the largest the timeout covers, and the timeout raised to meet
  the Core rule. A sweep of 18,000 requests, over every boundary of the policy
  and over values outside the Core's legal ranges (interval 0 and 0xFFFF,
  latency 500 and 0xFFFF, timeout 0 and 0xFFFF), checks that every answer
  stays inside the bounds and meets the Core rule, that a peripheral accepting
  15 ms is never slowed, and that any request reaching into the grantable
  range gets an interval it asked for. The vendored hook and
  the `Bonder` conversion compile under embedded Clippy with warnings denied;
  they are not host-tested.
- **Hardware-verified:** not yet. No board record shows a peripheral's request
  or the parameters it was granted.

## Related

- [Architecture: peripheral connection parameter requests](../architecture.md#peripheral-connection-parameter-requests)
- [Features: connection and security](../features.md#connection-and-security)
- [Security: threat model](../security.md#threat-model)
- [ADR 0003: Hardware-free decision modules](0003-pure-core-and-task-shell.md)
- [ADR 0005: Two slots and independent endpoints](0005-two-slots-and-independent-endpoints.md)
- [ADR 0007: Vendored SoftDevice patch](0007-vendored-softdevice-patch.md)
