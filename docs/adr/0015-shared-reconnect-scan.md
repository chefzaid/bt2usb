# ADR 0015: Reconnect Saved Devices At Power-Up With One Shared Scan

- Status: Accepted
- Date: 2026-10-09
- Supersedes: the boot-reconnect part of
  [ADR 0005](0005-two-slots-and-independent-endpoints.md) ("At boot, select up
  to two most recently stored peers, scan once, ...")

## Context

The bridge exists so that a BLE keyboard and mouse behave like wired USB ones
behind a monitor's hub ([TODO.md](../../TODO.md#product-extensions)). A monitor
that powers its hub together with the PC starts the bridge at the same moment
as the PC, and a wired keyboard can press a firmware setup key such as F2 or
Del as soon as the PC's firmware looks for one. The bridge must therefore have
the keyboard connected, secured, and subscribed within that window, and a
keyboard that wakes from sleep must reconnect as quickly as it can.

Until this decision the code did three things that stood in the way, all in
the 2026-10-09 tree at `8cb5cf8`:

- **A boot scan before any reconnect.** `ble_task` in
  [multi_conn.rs](../../src/ble/multi_conn.rs) ran a full user-style scan of
  `BLE_SCAN_DURATION_SECS` (8 s) before it reserved any slot, then matched each
  stored peer to one scan result with `reconnect::resolve_reconnect_targets`.
  No keyboard could type for at least 8 seconds after power-up. The scan also
  sent `ScanStarted`, `DeviceFound`, and `ScanComplete` to the UI, which showed
  Scanning and, when nothing advertised, a "No devices found" error at every
  power-up.
- **One resolution scan per slot, in turn.** Every silent attempt of a bonded
  peer first ran `scanner::resolve_bonded_peer`, a scan of up to
  `BLE_CONNECT_TIMEOUT_SECS` (6 s) that matched only that slot's identity key.
  Scans and connection setup share the `GAP_PROCEDURE` lock, because the
  SoftDevice runs one locally initiated scan or connection at a time. A mouse
  that was asleep therefore held the radio for 6 seconds at a time while the
  keyboard, already advertising, waited for the lock; at boot a bonded peer
  was resolved twice, once by the boot scan and again by its worker.
- **A low scan duty cycle.** Both scans used the vendored
  `central::ScanConfig::default()`: a 312.5 ms window every 1707.5 ms, about
  18 % of the time. A peer that advertises only briefly after a key press can
  be missed for whole intervals.

Two constraints shape any fix. Resolving a resolvable private address against
a peer's identity key (`IdentityKey::is_match`) calls the SoftDevice's AES
block (`sd_ecb_block_encrypt`, a supervisor call), so it must not run inside a
critical section. And the decision logic should stay hardware-free and
host-tested ([ADR 0003](0003-pure-core-and-task-shell.md)).

## Decision

**Start reconnecting at power-up.** `ble_task` reserves slot *i* for the *i*-th
of the two most recently added saved devices and sends it
`SlotCommand::Reconnect` at once, without scanning. Since 2026-10-11 only
devices with a bond are chosen: a background reconnect never pairs, so a
device without keys could never connect this way
([attempt numbers and retry takeover](../architecture.md#attempt-numbers-and-retry-takeover)). Boot sends no scan events
to the UI, which stays on Home until a device connects.

**Register every background target in one shared table.**
`reconnect::ReconnectTable<T, A>`, a pure module, holds one entry per slot:
the target the slot is reconnecting to, when its outage started, and at most
one pending sighting. The firmware instantiates it as `RECONNECTS` in
[scanner.rs](../../src/ble/scanner.rs) with `T = SavedPeer`, an alias of the
pure `reconnect::SavedPeer<Address, IdentityKey>` (the stored address and,
for a bonded peer, its identity key from the `Bonder`), and `A = Address`. A worker registers its target before each silent attempt and
clears it when it connects (just before `SlotEvent::Connected`), when any
command other than `Reconnect` reaches it, and when an attempt ends in an error
that stops retries. Since 2026-10-11 the worker first checks that the
`Bonder` still holds the device's keys and, without them, clears the target
and frees the slot, so every registered target carries an identity key; the
table still handles a record without one.

**Let one passive scan look for every slot's device.**
`scanner::find_saved_peer(sd, slot)` takes the GAP lock and:

1. returns a sighting that the other slot's scan recorded for this slot in the
   last `BLE_RECONNECT_SIGHTING_TTL_MS` (2 s), without scanning; a sighting is used once and
   a stale one is discarded;
2. otherwise runs one passive scan, bounded by `BLE_CONNECT_TIMEOUT_SECS`,
   whose callback skips non-connectable advertising reports (added on
   2026-10-10: a device that also advertises a non-connectable set, possibly
   from another private address, would otherwise end the scan and hand its
   slot an address that a connection attempt cannot use), copies the
   registered targets out of the table, matches the
   advertiser against them outside the critical section with
   `reconnect::owner_of` (identity key, or the stored address for a record
   without a bond; the lower slot wins a tie), and records the sighting for the
   owning slot with `ReconnectTable::record_sighting`;
3. stops at the first match. Its own device (`Recorded::Own`) is returned for
   an immediate connection. Another slot's device (`Recorded::HandedOver`) is
   left for that slot, which is woken through `RECONNECT_WAKE[owner]` if it is
   between attempts, and the scanning slot returns to its 500 ms backoff. If the
   owner stopped reconnecting after the targets were copied
   (`Recorded::NotRegistered`), nothing is recorded and the scan goes on.

The table also decides wakes. `ReconnectTable::wake_pending(slot)` is true
while the slot holds a sighting another slot handed over and has not taken;
taking it (at the start of the slot's next `find_saved_peer`, fresh or
stale), a failed attempt, clearing the slot, and registering a different
device all end it. Every change to `RECONNECTS` goes through one helper,
`scanner::update(slot, f)`, which applies `f` and then sets or resets
`RECONNECT_WAKE[slot]` to match, so the signal cannot outlive its sighting and
cut a later backoff short. Until 2026-10-10 the shell reset the signal by hand
at three call sites and signalled the owner without checking that its entry
still existed; moving the rule into the table made it host-testable.

**Hide a device from the other slot's scans after a failed attempt.** When a
silent attempt fails with `ConnectFailed`, the worker calls
`scanner::reconnect_attempt_failed`, and `ReconnectTable::attempt_failed`
drops the slot's pending sighting and holds its target back from the other
slot's scans for `BLE_FAILED_RECONNECT_HOLDOFF_MS`: the 500 ms backoff plus
one `BLE_CONNECT_TIMEOUT_SECS` scan, 6.5 s. The scan callback gets its targets
from `targets(scanning_slot, now)`, which always includes the scanning slot's
own target. Re-registering the same device keeps the holdoff; a different
device starts without one. A device that advertises but never completes a
connection, for example a mouse paired again with a laptop and advertising
with a filter accept list, or one that deleted its bond, would otherwise stop
every scan of the other slot at its first advertisement and wake its own slot
for another failing attempt, so the other slot's device would be heard only
when it happened to advertise first.

**Scan fast only while it pays.** `ReconnectTable::duty` returns `Fast` while
any target was registered less than `BLE_FAST_RECONNECT_SECS` (30 s) ago, and
the scan then uses `BLE_FAST_SCAN_INTERVAL` / `BLE_FAST_SCAN_WINDOW`, a 50 ms
window every 100 ms; otherwise it uses the vendored default. Re-registering
the same device keeps the time its outage started; `SavedPeer` equality
compares identity keys when both records have one and stored addresses when
neither does, so a private-address change does not restart the window or end
a holdoff. The window restarts after power-up and after each lost link.

**Connect at the fast duty cycle.** Every connection attempt, silent or chosen
by the user, scans for its whitelisted address with the fast interval and
window, because the device was heard advertising moments earlier.

## Alternatives Considered

- **The SoftDevice whitelist and device identity list.** `set_whitelist` and
  `set_device_identities_list` in the vendored `gap.rs` let the controller
  resolve private addresses without an AES call per advertisement, and a
  whitelisted scan reports only saved devices. But under the Bluetooth Core's
  network privacy mode a listed peer that advertises with its identity address
  is ignored, which many keyboards do; which mode S140 applies to each listed
  peer under the bridge's privacy settings has to be confirmed on a board; and
  the list must be kept in step with every bond change. The
  application-side match handles both address kinds today and can be replaced
  later behind the same `find_saved_peer` call if measurements ask for it.
- **Keep the boot scan but shorten it.** A shorter scan still delays every
  boot by its length, and the per-slot resolution scans after it still let a
  sleeping device hold the radio.
- **Connect straight to the stored address.** A whitelist connection to the
  address saved at pairing never matches a device that has rotated its
  resolvable private address, which is why the resolution scan existed.
- **One scan per slot, run in parallel.** The SoftDevice allows one locally
  initiated scan or connection at a time (`GAP_PROCEDURE`); a second fails with
  `NRF_ERROR_INVALID_STATE`.
- **Scan fast all the time.** Fast scanning finds a device soonest but keeps
  the radio receiving half the time for as long as a saved device is absent,
  for example a mouse left at home, and the bridge's supply current is not
  measured yet ([ADR 0012](0012-bus-powered-no-system-off.md)). Bounding it to
  30 seconds after power-up or a lost link covers the cases where a device is
  expected back soon.
- **Active scanning for reconnects.** Scan requests and responses add nothing
  here: only the advertiser's address is needed, not its name. Passive scanning
  keeps the peer's radio and the bridge's quieter.
- **Keep scanning after seeing another slot's device.** Recording the
  sighting and scanning on for the slot's own device would stop a failing
  device from cutting the scan short, but the other slot could not use the
  sighting until the scan ended, up to 6 seconds later, so an awake mouse would
  again wait behind a sleeping keyboard's scan. The holdoff costs the handover
  only for a device whose last attempt failed.
- **Count failures and give up on a device.** Stopping retries after some
  number of failures would also end the loop, but a device that fails because
  it is paired with a laptop that is switched off becomes connectable again
  when that laptop's bond is gone or it is re-paired, and a wired keyboard does
  not stop working after a few minutes on the wrong computer. Retries stay
  unlimited ([ADR 0005](0005-two-slots-and-independent-endpoints.md)).
- **Run the address match inside the table lock.** One lock round trip per
  advertisement would be simpler, but `is_match` makes a supervisor call, which
  must not run inside the `CriticalSectionRawMutex` critical section. The
  table therefore hands out a copy of its targets (`targets(scanning_slot,
  now)`), and the callback records the sighting in a second, short lock.

## Rationale

Starting at once and looking for both devices in one scan means the device
that advertises first connects first, whatever the other one is doing. The
worst case for an awake keyboard at power-up drops from at least 8 seconds of
boot scan, plus a possible 6-second wait behind the other slot, to one fast
scan window plus connection setup. Handing a sighting to its owner instead of
discarding it means no advertisement heard by either slot is wasted.

The holdoff restores the fairness the per-slot scans had when one device
fails: one failing attempt, then one full scan for the other device, in turn.
It is tied to failures, so the handover keeps working for every device that
connects when it is seen.

The table is the only new shared state, and it is small: two entries, each with
two times and a sighting. Keeping it pure lets host tests cover registration,
handover, expiry, and the duty window with plain integers, while the scanner
and the workers stay thin. The 2-second sighting lifetime bounds how old an
address can be when a connection starts, so a rotated private address is not
used long after it was heard.

## Consequences

Positive:

- A keyboard can connect as soon as it advertises after power-up or a sleep,
  instead of after an 8-second boot scan.
- A saved device that is asleep no longer holds the radio while the other
  slot's device is advertising.
- Power-up no longer shows Scanning or a "No devices found" error.
- Each bonded peer is resolved once per attempt, not twice at boot.

Negative:

- For 30 seconds after power-up or a lost link, the radio scans half the time.
  The extra current is not measured; the
  [power budget](../../TODO.md#ui-display-and-power) item covers it.
- The scan callback, which runs in the SoftDevice event task, now does up to
  two AES operations per advertisement while two targets are registered, where
  it did one before.
- At power-up the UI no longer lists nearby devices on its own; the user starts
  a scan to see them.
- A sighting that waits more than 2 seconds, for example behind a user scan
  holding the lock, is discarded and costs that slot another scan.
- For 6.5 seconds after any failed attempt, including one that failed only
  because the device stopped advertising, the other slot's scans do not hand
  that device over; its own slot's scan still finds it.
- If both slots ever registered the same device, only the lower slot would
  connect it. The coordinator never assigns one device to two slots, so this
  is a tie-break, not a supported case.
- None of this is verified on hardware (see
  [Verification Status](#verification-status)).

Follow-up obligations, tracked in [TODO.md](../../TODO.md):

- "Keyboard ready in time for firmware setup keys": a cold-start check on named
  keyboards, monitors, and PCs, and published times from power-up and from a
  key press on a sleeping keyboard to the first delivered keystroke.
- "Power budget and USB suspend current": include the fast reconnect duty
  cycle.
- "Soak and latency measurements": reconnect time under a long workload.

## Implementation

| Concern | Where |
| --- | --- |
| Boot assignment without a scan | `ble_task` in [multi_conn.rs](../../src/ble/multi_conn.rs) |
| Shared table, sighting lifetime, duty window, failure holdoff, tie-break, wakes | `ReconnectTable` (`register`, `clear`, `attempt_failed`, `targets`, `record_sighting`, `take_sighting`, `wake_pending`, `duty`), `Recorded`, `ScanDuty`, and `owner_of` in [reconnect.rs](../../src/ble/reconnect.rs) |
| Saved-device identity and matching | `reconnect::SavedPeer` (equality by identity key, otherwise by address; `matches` with a key resolver) in `reconnect.rs`; the `SavedPeer` alias and the `IdentityKey::is_match` resolver in [scanner.rs](../../src/ble/scanner.rs) |
| Shared reconnect scan and handover | `RECONNECTS`, `RECONNECT_WAKE`, `update`, `register_reconnect`, `clear_reconnect`, `reconnect_attempt_failed`, `reconnect_sighted`, and `find_saved_peer` in `scanner.rs`; the log line `slot {} scan found slot {}'s device` |
| Registration lifecycle | `connection_slot_task` (including the `attempt_failed` call on a silent `ConnectFailed`) and `connect_and_run_secure` in [slot_worker.rs](../../src/ble/slot_worker.rs) |
| Timing constants | `BLE_FAST_SCAN_INTERVAL`, `BLE_FAST_SCAN_WINDOW`, `BLE_FAST_RECONNECT_SECS`, `BLE_RECONNECT_SIGHTING_TTL_MS` (in `reconnect.rs` as `SIGHTING_TTL_MS` until 2026-10-10), `BLE_FAILED_RECONNECT_HOLDOFF_MS`, `BLE_CONNECT_TIMEOUT_SECS`, `BLE_RECONNECT_BACKOFF_MS` in [config.rs](../../src/config.rs) |
| UI at power-up | `UiState::connection_status` in [ui_logic.rs](../../src/ui/ui_logic.rs) no longer special-cases a boot scan |

### Verification Status

- **Implemented:** everything in the table above.
- **Software-verified:** 31 host tests in `reconnect_tests.rs` cover handing a
  sighting to the slot that owns the device, ignoring unregistered devices and
  slots, single use, replacement by a newer sighting, expiry at and after 2
  seconds, clearing, keeping the outage start across re-registration of the
  same target, dropping a still-fresh sighting and restarting the window for a
  different one, the fast window across two targets, the holdoff after a failed
  attempt (the other slot's scan keeps looking for its own device, the failing
  slot's own scan still sees it, the holdoff ends on time, is kept across
  re-registration and extended by a new failure, ends for a new target, and
  drops the pending sighting), the lower-slot tie-break, out-of-range slots,
  and a clock that goes backwards. Since 2026-10-10 they also cover wakes (a
  handover wakes only the owner; taking the sighting, fresh or stale, a failed
  attempt, clearing, and a new target end the wake; re-registering keeps it; a
  sighting for a slot cleared after the copy wakes nobody) and saved-device
  identity (equality by identity key or by address, matching by a resolved or
  stored address, and a retry at a new private address keeping the holdoff and
  window). The scanner and worker shells pass embedded Clippy with warnings
  denied but have no host tests, and the Renode scenario does not include the
  SoftDevice.
- **Hardware-verified:** not yet. No board record covers a cold start, the
  handover between slots, a device that advertises but will not connect, or
  the fast duty cycle's current.

## Related

- [Architecture: background reconnect](../architecture.md#background-reconnect)
- [Architecture: boot and initialization](../architecture.md#boot-and-initialization)
- [Features: boot and reconnect](../features.md#boot-and-reconnect)
- [First flash: in the monitor](../first-flash.md#5-in-the-monitor)
- [ADR 0003: Hardware-free decision modules](0003-pure-core-and-task-shell.md)
- [ADR 0005: Two slots and independent endpoints](0005-two-slots-and-independent-endpoints.md)
- [ADR 0012: Bus-powered, no System-OFF](0012-bus-powered-no-system-off.md)
