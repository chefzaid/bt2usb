# ADR 0011: Accept Just Works Bonding As An Interim Pairing Policy

- Status: Accepted
- Date: 2026-02-21

This policy is interim. It is expected to be superseded by the authenticated
pairing and enrollment decision that [TODO.md](../../TODO.md) lists as a P0
deployment gate; Proposed
[ADR 0017](0017-authenticated-pairing-and-enrollment.md) drafts that decision
and would supersede this record once Accepted. This record was written retroactively on 2026-10-09 from the
source and the commit history.

## Context

bt2usb pairs with BLE keyboards and mice and then forwards their input to a
USB host as if it were a wired keyboard and mouse. Whoever is paired can type
on that host, so the pairing method is the bridge's main security boundary.

The first embedded implementation (`8e6dd17`, 2026-02-21) bonded with
`IoCapabilities::None` and handled "pairing/encryption in normal connect flow
(no separate pairing screen required)", in the words of its README. That
commit also accepted any security mode other than `NoAccess` and `Open` before
HID discovery, and requested pairing whenever a connection found no stored
keys, including background reconnects. The hardening commit `2479c79`
(2026-09-28) kept Just Works but narrowed everything around it: encryption is
required before discovery, only an explicit user connection may start a new
pairing, and bonds are matched by peer identity. The same commit documented
the limitation in the security policy and opened the P0 item to replace it.

Constraints on the choice:

- **The local UI.** bt2usb has three buttons and a 128×64 OLED. It can show a
  six-digit passkey but has no keyboard of its own.
- **The peripherals.** A keyboard can type a passkey; a mouse generally has no
  input or display capability, so Just Works is the only method it can use.
  A policy that requires authenticated pairing must therefore also decide what
  happens to devices that cannot do it.
- **The pinned BLE binding.** The pinned `nrf-softdevice` supports passkey
  display and entry callbacks in its `SecurityHandler` trait, but it does not
  handle the SoftDevice's LE Secure Connections DH-key request event
  (`vendor/nrf-softdevice/src/ble/gap.rs` lists it among unhandled events), so
  LE Secure Connections pairing is not available without further binding work.
- **The default parameters.** The binding's `security_params` sets bonding,
  key distribution, the I/O capabilities, and MITM from the handler; LE Secure
  Connections and out-of-band data stay off, and `default_security_params`
  allows encryption keys of 7 to 16 bytes.

## Decision

Use Just Works bonding for now, and constrain when it can happen and what it
protects.

- **Security handler.** The single `Bonder` in
  [bonder.rs](../../src/ble/bonder.rs) declares
  `IoCapabilities::None`, accepts bonding (`can_bond` returns `true`), and does
  not request MITM protection. With the binding's parameters this is LE legacy
  pairing with the Just Works association model.
- **Encryption before HID discovery.** After connecting, a slot first calls
  `conn.encrypt()` with stored keys. `wait_for_secure_link` then polls up to
  25 times, 200 ms apart (about 5 s), for an encrypted mode: `JustWorks`,
  `Mitm`, or `LescMitm`. Signed-only modes are rejected because they do not
  encrypt notifications. Only an encrypted link proceeds to GATT discovery and
  HID subscription; otherwise the slot logs `slot {} failed to secure BLE link`
  and closes the link.
- **New pairing only on an explicit user connection.** A connection request
  carries `allow_pairing`. It is `true` only for `SlotCommand::Connect`, which
  the coordinator issues when the user selects a device from the scan list.
  Boot-time auto-reconnect and link-loss retries use `SlotCommand::Reconnect`,
  where it is `false`. When no keys are stored for the peer
  (`EncryptError::PeerKeysNotFound`), the slot calls `conn.request_pairing()`
  only if pairing is allowed; a background reconnect fails instead and retries
  later.
- **Identity-scoped bonds.** A stored bond holds the peer's encryption info,
  `MasterId`, and `IdentityKey`. Key lookup (`get_key`,
  `get_peripheral_key`) matches the connecting peer by identity, resolving a
  rotating private address with the stored identity key. `on_bonded` replaces
  only the keys of the same identity, because `MasterId` alone is not a peer
  identity: LE Secure Connections peers can share an all-zero EDIV and RAND.
  Reconnect scans resolve each bonded peer's current address the same way.
- **Persistence.** After a successful connection the coordinator persists the
  device with the bond the handler holds, using the fail-closed store from
  [ADR 0006](0006-fail-closed-pairing-store.md). The store keeps the bond's
  identity address rather than the advertised private address. The handler
  and the store each hold at most `MAX_PAIRED_DEVICES` (4) peers and evict
  their oldest entry when a new peer arrives while full; the store logs
  `Paired device store full - evicting oldest entry`.
- **Superseded by authenticated pairing.** No new feature may depend on Just
  Works being the pairing method. The replacement is a new ADR written before
  the P0 "Authenticated pairing and enrollment policy" item is implemented.

## Alternatives Considered

- **Passkey entry now.** bt2usb could show a passkey on the OLED for a
  keyboard to type, which gives MITM protection for keyboards. It needs a
  pairing screen, a timeout, and a policy for mice and other devices that
  cannot enter a passkey. That design is the P0 item, not yet done.
- **LE Secure Connections, Just Works or numeric comparison.** Secure
  Connections protects the pairing exchange against passive eavesdropping
  even without user interaction, and numeric comparison adds MITM protection
  for devices with a display and a yes/no input, which keyboards and mice
  rarely have. The pinned binding does not handle the DH-key request that
  Secure Connections needs, so this requires binding work first.
- **No bonding: pair on every connection.** Peripherals could not reconnect
  after sleeping or after a reboot without user action, which defeats
  auto-reconnect and use before the operating system starts.
- **Allow pairing on background reconnects** (the behavior before
  `2479c79`). A device advertising with a stored peer's address, or a peer that
  lost its keys, could be re-paired without anyone at the bridge, silently
  replacing a bond.
- **Accept any non-open security mode** (also the behavior before `2479c79`).
  Signed modes authenticate individual writes but leave notifications, which
  carry every keystroke, unencrypted.

## Rationale

Every BLE HID peripheral supports Just Works, and the decision to ship a
working bridge did not need to wait for a pairing UI and a policy on weaker
devices. Just Works cannot authenticate a peer, so the decision limits where
an attacker can use that gap:

- The application starts a new pairing only while a person is at the bridge,
  has started a scan, and has selected a device.
- Background reconnects never start a pairing from the application, and the
  coordinator persists a device only after its link is secured and connected.
  The binding's own Security Request path is the remaining gap (see
  Consequences).
- No HID data is read or forwarded over an unencrypted link.
- One peer's re-pairing cannot overwrite another peer's keys.

Recording the policy as interim keeps it from becoming the permanent design by
default.

## Consequences

Positive:

- Keyboards and mice pair with one selection and reconnect on their own after
  sleep, link loss, or a reboot of the bridge.
- A background reconnect cannot be turned into a silent new pairing by the
  application.
- HID input is only accepted over encrypted links.

Negative:

- There is no MITM protection. An active attacker in radio range during an
  explicit pairing can impersonate the selected device. The scan list shows
  advertised names only, and a name is whatever the advertiser chooses.
- LE legacy Just Works pairing does not protect the pairing exchange itself. A
  passive listener who records the exchange can derive the keys and decrypt
  later traffic on that bond.
- The application does not check the negotiated encryption key size; the
  binding's defaults accept 7 to 16 bytes.
- The application gates only the pairing it starts. The binding's own handler
  for a peripheral's Security Request
  (`BLE_GAP_EVTS_BLE_GAP_EVT_SEC_REQUEST` in `gap.rs`) calls
  `request_pairing()` when no keys are found for that connection, whatever
  the slot's `allow_pairing` says. On a background reconnect the slot's own
  `encrypt()` call fails with `PeerKeysNotFound`, pairing is not allowed, and
  the slot closes the link; but a pairing the binding has already started can
  run until the disconnect completes, and a completed one reaches `on_bonded`,
  which updates the in-memory bond cache. Since 2026-10-11 a background
  reconnect starts an attempt only while `Bonder` holds the device's keys
  (power-up skips a stored peer without a bond, and each attempt checks
  first), so this arises only when a pairing on the other slot drops those
  keys during an attempt or while the link it opened is up; on a link already
  up the slot does not close it. Whether such a pairing can complete has not
  been tested or observed; the TODO item below exists to close this path.
- The connect path tries stored keys first and requests pairing only when no
  keys are stored. A peripheral that has discarded its side of the bond is
  therefore expected to fail to connect until it is forgotten in the Saved
  devices menu and paired again. This is derived from the code path, not
  observed on hardware.
- There is no bounded pairing window and no enrollment allowlist. Pairing a
  fifth device evicts the oldest stored one with only a log message.
- Bond keys are stored unencrypted in internal flash; see the
  [security reference](../security.md#key-storage-and-deletion).

Follow-up obligations, tracked in [TODO.md](../../TODO.md):

- "ADR: authenticated pairing and enrollment" (P0): the decision record that
  supersedes this one, choosing between passkey entry and numeric comparison
  per device class.
- "Refuse peer-initiated pairing on background reconnects" (P0): make a
  Security Request on a link that does not allow pairing unable to create or
  replace a bond, shown by a test or on-air evidence.
- "Authenticated pairing and enrollment policy" (P0): decide per device class
  whether authenticated pairing is required, how user presence is checked, and
  whether weaker devices are rejected; add a bounded pairing window and visible
  state; test downgrade, unsolicited pairing, timeout, and reconnect cases, and
  collect representative real-device evidence.
- "Visible storage/security errors" (P1): show security failures with useful
  user actions.
- "Provisioning and physical key protection" (P0): protect stored keys on
  production units.

## Implementation

| Concern | Where |
| --- | --- |
| Security handler and bond cache | `Bonder` and its `SecurityHandler` implementation in [bonder.rs](../../src/ble/bonder.rs) |
| Encryption gate | `wait_for_secure_link` and `secure_and_discover`, which `connect_and_run_secure` races against commands, in [slot_link.rs](../../src/ble/slot_link.rs) |
| Pairing permission | `ConnectionRequest { allow_pairing: !silent }` in `connection_slot_task`; `silent` is true for `SlotCommand::Reconnect` |
| Identity resolution on reconnect | `SavedPeer` and the shared reconnect table in [reconnect.rs](../../src/ble/reconnect.rs) ([ADR 0015](0015-shared-reconnect-scan.md)); `find_saved_peer` and the `IdentityKey::is_match` resolver in [scanner.rs](../../src/ble/scanner.rs) |
| Persistence and eviction | `Action::PersistDevice` in `execute_action` ([multi_conn.rs](../../src/ble/multi_conn.rs)); `DeviceStore::add` in [storage.rs](../../src/storage.rs) |
| Binding behavior relied on | `security_params` in [security.rs](../../vendor/nrf-softdevice/src/ble/security.rs); `default_security_params` and the Security Request handler in [gap.rs](../../vendor/nrf-softdevice/src/ble/gap.rs) |
| SoftDevice security contexts | `central_sec_count`, one per link (`BLE_MAX_CONNECTIONS`), in `softdevice_config()` ([sd_setup.rs](../../src/sd_setup.rs)) |

Useful log lines: `Loaded {} BLE bonds into security handler` at startup,
`BLE security mode updated: {}` whenever a link's security mode changes, and
`slot {} failed to secure BLE link` when the encryption gate fails.

### Verification Status

- **Implemented:** everything above.
- **Software-verified:** the
  [2026-09-28 validation record](../testing.md#validation-record--2026-09-28)
  reports the firmware building and passing embedded Clippy locally, and the
  CI "Embedded build & clippy" job passed on GitHub-hosted runners in push runs
  36441995385 (`8a04b25`, 2026-09-28) and 37932436721 (`7fc99d6`, 2026-10-09)
  and scheduled run 37338711407 (2026-10-05). The security handler, the encryption gate, and the pairing permission live in
  SoftDevice-coupled code that is not part of the host test library, so no
  automated test covers them. The pure reconnect table (`ReconnectTable`,
  [ADR 0015](0015-shared-reconnect-scan.md)) and coordinator that surround them
  have host tests.
- **Hardware-verified:** not yet. The pairing, reconnect, and sleep-reconnect
  checks in [first flash](../first-flash.md#4-pairing-and-daily-use) have no
  board record in the repository.

## Related

- [Security reference: pairing and authentication](../security.md#pairing-and-authentication)
- [Features: pair and connect](../features.md#pair-and-connect)
- [Architecture: decisions needed for roadmap work](../architecture.md#decisions-needed-for-roadmap-work)
- [Security policy](../../SECURITY.md)
- [ADR 0006: Fail-closed pairing store](0006-fail-closed-pairing-store.md)
- [ADR 0007: Vendored nrf-softdevice patch](0007-vendored-softdevice-patch.md)
