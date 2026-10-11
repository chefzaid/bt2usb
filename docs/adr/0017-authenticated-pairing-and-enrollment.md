# ADR 0017: Require LE Secure Connections And Authenticate Keyboards In A Bounded Pairing Window

- Status: Proposed
- Date: 2026-10-10
- Would supersede: [ADR 0011](0011-interim-just-works-pairing.md) when Accepted
- Would amend: [ADR 0007](0007-vendored-softdevice-patch.md), adding security
  hooks to the vendored `gap.rs`, `security.rs`, `connection.rs`, `replies.rs`

## Context

Whoever pairs with the bridge can type on whichever computer the KVM selects,
including in firmware setup, where no host-side protection applies.
[ADR 0011](0011-interim-just-works-pairing.md) accepted Just Works for now;
three P0 items in [TODO.md](../../TODO.md#ble-central-and-pairing) replace
it. In the 2026-10-10 tree (items in the vendored `gap.rs` are named by
match arm or function):

- **Parameters.** `Bonder` ([bonder.rs](../../src/ble/bonder.rs),
  `io_capabilities` and `can_bond`) returns `IoCapabilities::None` and
  `can_bond = true` and overrides neither `request_mitm_protection` nor
  `security_params`. The vendored `security_params`
  (the `SecurityHandler` default in
  `vendor/nrf-softdevice/src/ble/security.rs`) starts from
  `default_security_params` in `gap.rs`: key size 7 to 16, no I/O
  capability, MITM clear, LESC never set. Every pairing is LE legacy Just
  Works.
- **Who starts security.** `connect_and_run_secure`
  ([slot_worker.rs](../../src/ble/slot_worker.rs), through `secure_and_discover`)
  calls `encrypt()` and, on `PeerKeysNotFound`, `request_pairing()` only when
  `allow_pairing` (`!silent` in `connection_slot_task`) is set, that is for
  `SlotCommand::Connect`. `wait_for_secure_link` polls 25 times at 200 ms
  for `JustWorks`, `Mitm`, or `LescMitm`; the vendored
  `SecurityMode::try_from_raw` (`types.rs`) calls every Level 2
  link, legacy or LESC, `JustWorks`; the `CONN_SEC_UPDATE` arm only traces
  the key size.
- **Security Request.** The `SEC_REQUEST` arm calls `encrypt()`, or
  `request_pairing()` when no keys are found, whatever the slot allows. It
  can beat the slot's own call, which waits for the MTU exchange (the
  `central::connect_with_security` call in `connect_and_run_secure`): the S140 header text in the
  `nrf-softdevice-s140` 0.1.2 bindings gives `NRF_ERROR_BUSY` from
  `sd_ble_gap_encrypt` for a procedure in progress, and
  `NRF_ERROR_INVALID_STATE` from `sd_ble_gap_authenticate` while an
  encryption is queued; the slot fails on any error (the `PeerKeysNotFound`
  and `Err(_)` arms in `secure_and_discover`). This race is derived, not observed.
- **Bonds.** The `AUTH_STATUS` arm calls `on_bonded` for any successful
  bonded pairing on any link. `on_bonded` (`bonder.rs`) replaces a bond
  whose identity address equals the new one or whose IRK resolves the
  connection address, else appends and evicts the oldest at
  `MAX_PAIRED_DEVICES` (4). The stored flags byte
  ([codec.rs](../../src/storage/codec.rs), `encode_bond` and `decode_bond`),
  which the [data model](../data-model.md#bond) says the application does
  not interpret, is the SoftDevice's `ble_gap_enc_info_t` bitfield: `lesc`
  bit 0, `auth` bit 1, `ltk_len` bits 2 to 7. Every bond so far has
  `lesc = 0` and `auth = 0`, and recording a bond's assurance needs no
  format change.
- **Binding gaps for LE Secure Connections.** S140 leaves the P-256
  Diffie-Hellman step to the application (`BLE_GAP_EVT_LESC_DHKEY_REQUEST`,
  answered by `sd_ble_gap_lesc_dhkey_reply`); `gap.rs` lists it as unhandled
  in a comment after the `SEC_REQUEST` arm. `keyset()` (`connection.rs`,
  lines 357–388) passes no public-key buffers and always passes
  `keys_peer.p_enc_key`, which the header says must be NULL for LESC. The
  header puts a LESC LTK in the local key, but the central branch of
  `AUTH_STATUS` passes `peer_enc_key`, zeroed for each new connection, so a
  LESC bond would get a zero LTK (derived, not observed).
- **No numeric comparison.** The `PASSKEY_DISPLAY` arm calls
  `display_passkey` without the connection and debug-asserts
  `match_request == 0`, so the handler cannot tell a comparison from a
  passkey to show. `PasskeyReply` (its `Drop` and `finalize` in `replies.rs`)
  replies `BLE_GAP_AUTH_KEY_TYPE_PASSKEY` with NULL when dropped, which the
  header defines as confirming a numeric comparison. `display_passkey` and
  `enter_passkey` panic unless implemented (their `SecurityHandler` defaults
  in `security.rs`).
- **No P-256 code.** `Cargo.lock` has none; `embassy-nrf` 0.7.0 lists the
  `CRYPTOCELL` interrupt but has no CryptoCell driver. The RNG belongs to the
  SoftDevice; the vendored `random_bytes` wraps
  `sd_rand_application_vector_get`.

From the Bluetooth Core, Vol 3, Part H unless noted:

- **Method selection (Section 2.3.5.1).** If neither side sets MITM, Just
  Works is used. Otherwise, with the bridge as initiator declaring
  DisplayYesNo, a KeyboardOnly peer gets Passkey Entry with the bridge
  displaying (S140 chart `BLE_GAP_CENTRAL_LESC_BONDING_PKE_CD_MSC`), a
  DisplayYesNo or KeyboardDisplay peer gets Numeric Comparison under LESC, and
  a NoInputNoOutput or DisplayOnly peer gets Just Works. The bridge cannot
  declare KeyboardOnly or KeyboardDisplay: it has three buttons, no keyboard.
- **Levels (Vol 3, Part C, Section 10.2.1).** Mode 1 Level 4 is authenticated
  LESC with a 128-bit key; Level 2 is unauthenticated encryption.
- **SMP timeout (Section 3.4).** 30 seconds without SMP progress fails the
  procedure, which bounds the time to type a passkey.
- **Rejecting a Security Request.** A central passes NULL parameters to
  `sd_ble_gap_authenticate` to reject it with Pairing Failed (S140 header).
- **General knowledge, sections not cited.** A recorded legacy pairing lets a
  listener brute-force the six-digit temporary key ("crackle", 2013). LESC
  Just Works resists passive recording, not an active man in the middle. The
  LESC DHKey check covers AuthReq, OOB flag, and I/O capability, not the
  maximum key size. CVE-2018-5383 requires validating the peer's public key;
  CVE-2020-26558 is mitigated by refusing a peer key equal to the local one.
  In LE only the central sends a Pairing Request.
- **Unconfirmed.** In the central role the header calls the
  `SEC_PARAMS_REQUEST` `peer_params` the "Initiator Security Parameters";
  whether they hold the peer's Pairing Response must be checked on a board.

## Decision

Pair only with LE Secure Connections, authenticate every device that can
authenticate, confine unauthenticated devices to pointer input, and create a
bond only inside a bounded window that a person opens and watches on the
bridge. A new pure module, `src/ble/pairing_policy.rs`, holds every rule
([ADR 0003](0003-pure-core-and-task-shell.md)); `Bonder` and the slot worker
execute its answers, and host tests plus sniffer captures verify it
([Implementation](#implementation)).

**Request.** Bond, MITM, and Secure Connections set; Keypress and OOB clear;
I/O capability DisplayYesNo; minimum and maximum key size 16. `Bonder`
overrides `io_capabilities`, `request_mitm_protection`, and
`security_params`; key distribution is unchanged.

**Acceptance.** Only these outcomes complete:

| Peer declares | Method | Assurance | OLED shows | User action | Input accepted |
| --- | --- | --- | --- | --- | --- |
| KeyboardOnly | LESC Passkey Entry, bridge displays | Authenticated, Level 4 | `Type on keyboard:`, six digits, seconds left | Types the code on the keyboard, then Enter if the keyboard needs it | Keyboard, mouse, consumer |
| DisplayYesNo, KeyboardDisplay | LESC Numeric Comparison | Authenticated, Level 4 | `Same code on device?`, six digits | SELECT if equal, DOWN if not | Keyboard, mouse, consumer |
| NoInputNoOutput, DisplayOnly | LESC Just Works | Unauthenticated, Level 2 | `No code: mouse only` | SELECT to accept | Mouse only |

The policy identifies the method from the events it sees (a passkey display,
a match request, or neither), not from the peer's declared capability.
`pairing_policy::verify` refuses everything else: legacy pairing (rejected
with `BLE_GAP_SEC_STATUS_AUTH_REQ` when the parameters show it, and by the
`AUTH_STATUS` `lesc` bit in any case), an unbonded result, a key size other
than 16, a level that does not match the method, or a passkey event without
an authenticated result and the reverse. `pairing_policy::permits` drops
keyboard and consumer reports on an unauthenticated link, since consumer
application-launch usages can start programs. Application code logs neither
the passkey nor the comparison value.

**Pairing window.** `Enrollment`, a state machine with an injected clock like
`ReconnectTable` ([ADR 0015](0015-shared-reconnect-scan.md)), opens when
SELECT picks a listed device without a bond, fewer than four bonds are held,
and no window is open. It is bound to one slot and connection handle for
`BLE_PAIRING_WINDOW_SECS` (60 s) and moves through `Connecting`,
`AwaitingMethod`, `ShowingPasskey` or `ConfirmingComparison` while SMP runs,
`Verifying` on `AUTH_STATUS`, `ConfirmingJustWorks` when the result is
unauthenticated, `Discovering`, and `Committed` or `Failed(reason)`. DOWN,
the deadline, link loss, any new slot command, or USB suspend closes it. The
OLED stays on and counts down. The new bond waits as the window's pending
bond, outside `Bonder`'s list, until HID discovery and any required SELECT
succeed; only then is it committed and persisted. For this link the 5-second
secure wait becomes the window deadline.

**No pairing outside the window.** Only the slot worker starts encryption or
pairing. A new `SecurityHandler::on_security_request` hook answers `Defer`
when a bond exists or the window belongs to the link, and otherwise
`Reject`, which calls `sd_ble_gap_authenticate` with NULL; the vendored
handler never encrypts or pairs itself. As defence in depth (a
`SEC_PARAMS_REQUEST` follows only the bridge's own pairing request), one on a
link without a window gets `BLE_GAP_SEC_STATUS_PAIRING_NOT_SUPP`, and a bond
reported for such a link is discarded and the link closed. Background
reconnects target only records with a bond; others stay under Saved devices
marked `pair again`.

**Downgrade and re-pairing protection.** A connection to an address that
resolves to a bond only encrypts; refused keys show `Keys rejected`, and the
device must be forgotten (default-Cancel confirmation) before it pairs again.
A new bond whose identity address or IRK matches a held bond is refused
(`Saved: forget first`). With four bonds the window does not open
(`Saved list full`), so nothing is evicted. On every reconnect the level and
key size reached with stored keys must match the assurance decoded from the
stored flags, or the link closes, and `permits` applies to it. The level
likely echoes the flags passed to `sd_ble_gap_encrypt` (general knowledge, to
confirm on a board), so this catches a corrupt record; what protects the
reconnect itself is that only the bonded peer holds the LTK.

**Binding.** The vendored patch adds the Security Request and parameter-check
hooks; a passkey hook with the connection and the `match_request` flag; a
comparison reply whose `Drop` rejects with `BLE_GAP_AUTH_KEY_TYPE_NONE`,
which `enter_passkey` also uses; the DH-key event with public-key buffers in
`keyset()`; `keys_peer.p_enc_key` NULL for LESC; the local LTK for a LESC
bond; the key size in `on_security_update`; and a callback for every
`AUTH_STATUS`. The event handler applies the Security Request answer; the
other hooks record the event in `Enrollment` and signal the slot worker,
which sends the comparison and DHKey replies. A firmware-only
`src/ble/lesc.rs` makes a fresh P-256 key pair per window and computes the
DHKey with the RustCrypto `p256` crate (`ecdh`, default features off), seeded
through `random_bytes`. It refuses a peer key off the curve, equal to its
own, or equal to the Core's debug key (Vol 3, Part H, Section 2.3.5.6.1).

**Display-less variant.** The I/O capability is a policy input. A dongle
([TODO.md](../../TODO.md#more-peripherals-and-form-factors)) would declare
DisplayYesNo, blink the passkey on its LED, use its button as Yes, and keep
the Just Works tier, once that board shows a blink code can be read and typed
within the 30-second SMP timeout. No variant uses a static passkey.

### Open Questions For The Owner

1. Devices that can do only Just Works, such as most mice: **Allow** (mouse
   reports only, after SELECT; recommended) or **Reject**.
2. Devices that support only LE legacy pairing: **Reject** (recommended) or
   **Allow** (in the Just Works tier, with keys a recording exposes).
3. Bonds made under ADR 0011: **Re-pair** (refused and shown as `pair again`;
   recommended) or **Grandfather** (full input until forgotten).
4. Window length in seconds: **60** (recommended), **30**, or **120**.

## Alternatives Considered

- **Keep Just Works (ADR 0011).** No protection against an active attacker,
  and a recorded legacy exchange yields the keys.
- **LESC Just Works for every device, confirmed with SELECT.** It defeats
  passive recording, but a look-alike keyboard chosen by its peer-supplied
  name could still type.
- **Authenticated pairing for every device.** Mice without input or display
  could not pair; this is the **Reject** answer to question 1.
- **Legacy passkey entry as a fallback.** A passive recording still recovers
  the key, so it adds a code path without a guarantee (question 2, **Allow**).
- **DisplayOnly instead of DisplayYesNo.** DisplayYesNo peers would get Just
  Works instead of Numeric Comparison, for one screen less.
- **A static passkey on a label.** LESC Passkey Entry reveals the passkey one
  bit per round to a peer that runs a failed pairing (general knowledge), so a
  fixed one leaks.
- **NFC out-of-band pairing or a name allowlist.** Common BLE keyboards and
  mice offer no NFC OOB, and names are peer-chosen before pairing.
- **Keep the vendored Security Request handler, discard bonds in
  `on_bonded`.** The pairing would still run and the peer would keep a bond.
- **CryptoCell CC310 for the DHKey.** It needs Nordic's closed library, FFI,
  and a licence review; it stays the fallback if software ECDH is too slow.

## Rationale

Passkey Entry suits keyboards: the keyboard, the input the user must trust,
proves its presence by typing the code the bridge shows. LESC removes passive
recording for every class. Limiting unauthenticated bonds to mouse reports
caps what a Just Works impostor gains: it can click but cannot type, launch
programs, or press setup keys. The application checks the outcome itself
because the SoftDevice reports LESC and legacy Just Works as one level and the
DHKey check does not protect the key size.

The window makes "a person is at the bridge" a bounded, visible state; the
deferred commit leaves no key behind a refused pairing; and one rule, that
only the slot worker starts security, closes the Security Request path and
the double-encryption race. A pure module makes all of it host-testable.

## Consequences

Positive:

- Recorded pairings no longer yield keys, and impersonating a keyboard during
  pairing requires the user to type the code on the impostor.
- A background reconnect, a Security Request, or a pairing on another link
  cannot create, replace, or evict a bond, and a reconnect cannot run below
  its bond's assurance.
- Pairing involves only the peripheral and the bridge, so the KVM,
  firmware-setup, and no-host-software promises hold. Reconnects reuse stored
  keys without ECDH, leaving the ADR 0015 boot path unchanged.

Negative:

- Keyboard pairing takes longer: six digits to read and type.
- A keyboard that declares NoInputNoOutput or supports only legacy pairing can
  no longer type through the bridge, and one without a display that declares
  KeyboardDisplay is offered a Numeric Comparison it cannot show and must be
  refused. Which named devices declare what is unknown until the hardware
  compatibility baseline records each one's Pairing Response.
- Mouse buttons that send keyboard or consumer reports do nothing on a Just
  Works bond, and a Just Works impostor accepted by mistake can still click.
- ADR 0011 bonds must be paired again under the recommended answer.
- Anyone with the buttons can still pair a keyboard; there is no local PIN.
- The vendored patch gains functional changes in `connection.rs` and
  `replies.rs` besides `gap.rs`, `security.rs`, and `gatt_client.rs`, five
  files to re-apply on an upgrade, and `p256` joins the audited dependencies.
- Two P-256 scalar multiplications per pairing block the single thread-mode
  executor ([architecture](../architecture.md#what-may-block)): the other
  slot's input, `softdevice_task`, and the USB endpoint workers (100 ms
  deadline) wait. Their time on the nRF52840's 64 MHz Cortex-M4F (Nordic
  product specification) is unmeasured.

Cost estimates, none measured: about 15 to 30 KiB of flash for `p256` with
ECDH and 4 to 8 KiB for the policy, window, screens, and hooks, against
804 KiB of application flash of which `.text` used 109,584 bytes in the
[2026-10-10 validation record](../testing.md#validation-record--2026-10-10);
about 300 bytes of static RAM (a secret, a local public key, a 64-byte peer
key per link, a DHKey) and 1 to 3 KiB more peak stack, which the stack
high-water log would confirm.

## Implementation

| Concern | Where |
| --- | --- |
| Request parameters, method table, `verify`, Security Request answer, flag decoding, `permits`, bond admission, `Enrollment` | New `src/ble/pairing_policy.rs` and `pairing_policy_tests.rs`, compiled into the host library ([lib.rs](../../src/lib.rs)) like `conn_params`; it takes plain integers (I/O capability, flags byte, key size, mode and level), no SoftDevice types and no `unsafe` |
| Handler | `Bonder` in [bonder.rs](../../src/ble/bonder.rs): overrides, passkey and comparison hooks, `enter_passkey` rejecting, pending bond |
| ECDH | New firmware-only `src/ble/lesc.rs`; `p256` in `Cargo.toml` and `Cargo.lock` |
| Window, waits, report filter | `connection_slot_task`, `connect_and_run_secure` in [slot_worker.rs](../../src/ble/slot_worker.rs); notification path in [hid_client.rs](../../src/ble/hid_client.rs) |
| Refusals | New `ErrorTag` values in [coordinator.rs](../../src/ble/coordinator.rs); `ble_error_message` in [main.rs](../../src/main.rs) |
| Screens | New `Screen::Pairing` in [ui_logic.rs](../../src/ui/ui_logic.rs), drawn in [display.rs](../../src/ui/display.rs); display kept on in [power_logic.rs](../../src/power_logic.rs) |
| Binding | `vendor/nrf-softdevice/src/ble/{gap,security,connection,replies}.rs`, [README.bt2usb.md](../../vendor/nrf-softdevice/README.bt2usb.md) |
| Constants | `BLE_PAIRING_WINDOW_SECS`, `BLE_PAIRING_KEY_SIZE` in [config.rs](../../src/config.rs) |
| Guides | The pairing lifecycle in [architecture](../architecture.md#user-scan-connect-pairing-and-hid-discovery), [features](../features.md#pair-and-connect), [OLED messages](../operations.md#oled-messages), [security](../security.md#pairing-and-authentication), [first flash](../first-flash.md#4-pairing-and-daily-use) |

Host tests: the method table for all 25 I/O pairs, MITM set and clear, LESC
and legacy; a sweep over `lesc`, `auth`, key sizes 0 to 255, bonding, and
method that accepts only the table's rows; the Security Request answer for
every window, bond, and slot state; admission of a duplicate identity, a
matching IRK, and a full list; `Enrollment` at and after the deadline, on
cancel, link loss, suspend, a second window, a backwards clock, and commit
only after discovery and confirmation; flag decoding; `permits`; and every
pairing screen. Renode would add a scripted pairing step to
[sim.rs](../../src/sim.rs) that feeds `Enrollment` synthetic SMP events while
SELECT and DOWN arrive as GPIO edges
([ADR 0014](0014-renode-gpio-models.md)); the simulation has no SoftDevice,
so it checks the window and the screens, not SMP.

On-air evidence, from a BLE sniffer, filed with a
[first-flash record](../first-flash.md#4-pairing-and-daily-use): a named
keyboard pairing by passkey (Pairing Request with Secure Connections, MITM,
Bond, DisplayYesNo, maximum key size 16; log showing Level 4, 16 bytes); a
named mouse with Just Works, its keyboard reports dropped; and, from a second
nRF52840 running a configurable HID peripheral, a legacy-only and a 7-byte
peer refused, a Security Request on a background reconnect after the peer
deleted its bond answered with Pairing Failed and no bond in RAM or flash,
an expired window, the time of each P-256 operation, and a reconnect timed
against ADR 0015.

Unblocks, in [TODO.md](../../TODO.md#ble-central-and-pairing), "ADR:
authenticated pairing and enrollment" (once Accepted), "Authenticated pairing
and enrollment policy", and "Refuse peer-initiated pairing on background
reconnects"; turns the replacement and eviction cases of "Visible
storage/security errors" ([TODO.md](../../TODO.md#ui-display-and-power)) into
refusals; and gives "Display-less plug-in dongle" its pairing flow.

### Verification Status

- **Implemented:** nothing of this proposal. Today: Just Works with the
  vendored defaults, encryption before HID discovery, pairing only on an
  explicit connection, and identity-scoped bonds
  ([ADR 0011](0011-interim-just-works-pairing.md)).
- **Software-verified:** nothing of this proposal. The security flow in
  `bonder.rs` and `slot_worker.rs` has no host tests
  ([testing](../testing.md#modules-without-host-tests)), and the binding gaps
  and the race in [Context](#context) come from reading the source.
- **Hardware-verified:** not yet; no board record covers any pairing.

## Related

- [Security: pairing and authentication](../security.md#pairing-and-authentication)
- [Security: threat model](../security.md#threat-model)
- [Security: key storage and deletion](../security.md#key-storage-and-deletion)
- [Architecture: user scan, connect, pairing and HID discovery](../architecture.md#user-scan-connect-pairing-and-hid-discovery)
- [Architecture: decisions needed for roadmap work](../architecture.md#decisions-needed-for-roadmap-work)
- [Features: pair and connect](../features.md#pair-and-connect)
- [TODO: BLE central and pairing](../../TODO.md#ble-central-and-pairing)
- [TODO: hardware compatibility baseline](../../TODO.md#board-bring-up-and-hardware-acceptance)
- [ADR 0003: Hardware-free decision modules](0003-pure-core-and-task-shell.md)
- [ADR 0006: Fail-closed pairing store](0006-fail-closed-pairing-store.md)
- [ADR 0007: Vendored nrf-softdevice patch](0007-vendored-softdevice-patch.md)
- [ADR 0011: Interim Just Works pairing](0011-interim-just-works-pairing.md)
- [ADR 0015: Shared reconnect scan](0015-shared-reconnect-scan.md)
- [ADR 0016: Bounded peer connection parameters](0016-bounded-peer-connection-parameters.md)
