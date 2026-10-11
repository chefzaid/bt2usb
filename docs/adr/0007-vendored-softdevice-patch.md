# ADR 0007: Vendor A Minimal nrf-softdevice Patch At A Pinned Revision

- Status: Accepted; amended by
  [ADR 0016](0016-bounded-peer-connection-parameters.md) (2026-10-09), which adds
  one hook outside `gatt_client.rs`, and on 2026-10-10 by a log-only change that
  keeps peer addresses, passkeys, and notification bytes out of default logs
  ([logging and privacy](../security.md#logging-and-privacy)), and on
  2026-10-10 by [ADR 0025](0025-panic-lints-and-inventory.md), which removes
  four panics a peer could reach in `events.rs`, `gap.rs`, and
  `connection.rs`, and on 2026-10-11 by a fix to the event portal's drop
  guard in `util/portal.rs`, the first patch with host tests
- Date: 2026-09-28

## Context

bt2usb reaches the SoftDevice through the `nrf-softdevice` crates
([ADR 0002](0002-nrf52840-softdevice-embassy.md)). Until 2026-09-28 they were
git dependencies on `https://github.com/embassy-rs/nrf-softdevice` with no
`rev`, and `Cargo.lock` was listed in `.gitignore`, so every fresh build could
resolve a different upstream commit.

Three problems in the GATT client made the pinned upstream unsuitable as is:

- **No offset reads.** A HID Report Map can be longer than one ATT payload.
  Reading it whole needs ATT Read Blob requests at increasing offsets, and the
  crate exposed only `gatt_client::read` at offset zero. The firmware of
  2026-09-26 read the map once into a 128-byte buffer, so a longer map was
  truncated, and a truncated map can classify reports wrongly.
- **Timeouts.** A GATT timeout event during discovery or MTU exchange
  panicked instead of returning an error, and a read could keep waiting after
  the SoftDevice had abandoned the request, so an unresponsive peer could halt
  the bridge or hang a connection worker.
- **Peer-controlled discovery.** Discovery could panic on counts or handles a
  peer chose, or loop forever when handle arithmetic wrapped at `0xFFFF`.

Anything in radio range can act as a peripheral, so none of these may be able to
crash or hang the bridge. The application cannot work around them on its own:
the crate routes every GATT client response through its own event portal
(`gatt_client::on_evt` and `util::portal`, both `pub(crate)`), so an
application that issued `sd_ble_gattc_read` itself would never see the reply.

## Decision

- Pin `nrf-softdevice` and `nrf-softdevice-s140` to upstream commit
  `47d6121c6e823120e8b883a7ac75f44ce7daa3aa` in `Cargo.toml`, and commit
  `Cargo.lock`.
- Copy the `nrf-softdevice` crate at that commit into `vendor/nrf-softdevice`,
  under its original MIT and Apache-2.0 licenses, and point the git dependency
  at the copy with a root `[patch.'https://github.com/embassy-rs/nrf-softdevice']`
  entry. The sibling crates (`nrf-softdevice-s140`, `nrf-softdevice-macro`)
  stay git dependencies at the same commit.
- Keep every functional change in `src/ble/gatt_client.rs` of the vendored
  crate, and keep it minimal (since
  [ADR 0016](0016-bounded-peer-connection-parameters.md), one more change adds
  `SecurityHandler::conn_param_update_request` in `security.rs` and calls it
  from `gap.rs`; since [ADR 0025](0025-panic-lints-and-inventory.md), the
  panic fixes below touch `gap.rs` and `connection.rs`, and since 2026-10-10
  the security handler calls move out of the connection state there):
  - add `read_by_offset`, which issues one ATT Read or Read Blob at a given
    offset, checks that the response handle and offset match the request
    (`ReadError::InvalidResponse` otherwise), and keeps upstream's
    `ReadError::Truncated` when the value does not fit the buffer; `read`
    delegates to it with offset zero
  - return `Timeout` errors from discovery and MTU exchange instead of
    panicking on the timeout event, and from reads instead of waiting forever
  - bound discovery: keep the first six characteristic declarations of a
    response and resume after the last kept handle, return
    `DiscoverError::TooManyAttributes` when descriptors overflow the fixed
    buffer, return `DiscoverError::InvalidResponse` for an empty, out-of-range,
    out-of-order, or non-advancing response, and saturate handle arithmetic
  - since 2026-10-10, let the discovery and MTU-exchange waiters skip an
    event they did not expect and keep waiting, as `read_by_offset` and
    `write` already did, instead of panicking with `unexpected event {}`
  - since 2026-10-10, store the ATT MTU the SoftDevice uses after an MTU
    exchange, the smaller of the requested MTU and the server's offer and
    never below 23, instead of the server's offer; the Report Map reader
    sizes its fragments from it
  - since 2026-10-10, keep a connection whose peripheral refuses the MTU
    exchange with an ATT error, at the default MTU of 23, instead of failing
    the connect (`central::connect_inner`, outside `gatt_client.rs`)
  - since 2026-10-10, answer a peer's Exchange MTU Request and a
    `BLE_GATTS_EVT_SYS_ATTR_MISSING` event without the `ble-gatt-server`
    feature, which bt2usb leaves off and upstream then dropped both
    (`on_gatts_evt_without_server` in `ble/mod.rs`, with a non-panicking
    state lookup in `connection.rs`)
- Since 2026-10-10 ([ADR 0025](0025-panic-lints-and-inventory.md)), remove the
  panics a peer could reach outside the GATT client: bt2usb enables the
  crate's `evt-max-size-256` feature, so the event buffer holds the largest
  discovery response the configured ATT MTU allows (a compile-time check in
  `sd_setup.rs` keeps the two in step); `gap::on_evt` logs a GAP timeout from
  an unexpected source, such as the authenticated payload timeout, instead of
  panicking; and `ConnectionState::disconnect_with_reason` returns
  `DisconnectedError` for any SoftDevice error, which `Connection::drop`
  accepts.
- Since 2026-10-10, call every `SecurityHandler` method in the compiled
  modules after `Connection::with_state` returns, with the handler reference
  and the data it needs copied out of the state, so a handler that reads the
  connection (bt2usb's `Bonder` calls `Connection::peer_address`) never takes
  a second `&mut ConnectionState` while the first is live (`gap::on_evt`,
  `Connection::encrypt`, and `Connection::request_pairing`).
- Since 2026-10-11, let a portal wait that ends, completed or cancelled,
  clear the portal only while the portal still holds that wait's own closure
  (`Portal::clear_if_registered` in `util/portal.rs`). Upstream's drop guard
  reset the portal unconditionally, so a GATT wait that a peer's disconnect
  had failed, and whose task ran again only after the SoftDevice gave the
  freed connection handle to the other slot's link, erased that link's MTU
  exchange; its connect then never returned and held the GAP procedure lock
  until reset ([architecture](../architecture.md#attempt-numbers-and-retry-takeover)).
  `tests/vendor_portal.rs` compiles the vendored portal source on the host
  and tests the guard.
- Assemble and bound long values in bt2usb, not in the vendored crate.
- Since 2026-10-10, print a peer address, a displayed passkey, or notification
  bytes only with the vendored crate's `log-sensitive-data` feature, which
  bt2usb's feature of the same name forwards. Without it the three upstream
  log lines that printed them (`central.rs`, `gap.rs`, `gatt_client.rs`) log
  the role, the bare event name, and the notification length. This changes
  log output only, never behavior.
- Record the base commit, every change, and the removal condition in
  [vendor/nrf-softdevice/README.bt2usb.md](../../vendor/nrf-softdevice/README.bt2usb.md).
- Remove the patch only when the pinned upstream provides equivalent offset
  reads, timeout errors, bounded discovery, the panic fixes above, the
  negotiated ATT MTU, a connect that survives a refused MTU exchange,
  answers to the GATT server events a central-only build still receives,
  security handler calls made outside the connection state, and a portal
  whose ended wait clears only its own registration.
  Never
  deploy a change by editing Cargo's git checkout.

## Alternatives Considered

- **Wait for upstream.** The panics were reachable by any peripheral in range,
  so the fix could not wait for an upstream release. Offering the changes
  upstream remains worthwhile; no upstream pull request is recorded in this
  repository.
- **A fork on GitHub, pinned by `rev`.** This works with Cargo, but the patch
  would live in a second repository with its own history and access control,
  and reviewing a bt2usb change would mean reviewing a commit somewhere else.
  Vendoring keeps the patch in the same history as the code that relies on it.
- **Edit the Cargo git checkout.** It is quick, but the change exists only on
  one machine and disappears on the next fetch. The vendor notes forbid it.
- **Issue raw SoftDevice calls from the application.** The response events are
  consumed by the crate's private portal, so the application cannot receive
  them without changing the crate anyway.
- **Read only the first fragment of the Report Map.** That was the 2026-09-26
  behavior. A truncated map can make a report look like a supported layout when
  it is not.
- **Replace the BLE stack.** Moving to Nordic's SoftDevice Controller with a
  pure-Rust host would remove this patch, but it would also replace the
  scanner, connection workers, GATT HID client, and bonder; see
  [ADR 0002](0002-nrf52840-softdevice-embassy.md#alternatives-considered).
- **Vendor every dependency with `cargo vendor`.** Only one crate needs
  changes, and `Cargo.lock` with the pinned revision already makes the rest
  reproducible.

## Rationale

A committed patch is reproducible and reviewable: anyone building the same
commit gets the same BLE code, and every changed line went through the same
review as the rest of the firmware. Keeping the patch to one file and to API
additions and error paths keeps rebasing on a newer upstream cheap and makes it
easy to compare with upstream later.

Splitting the work between the crate and the application keeps the crate change
small. The crate only exposes one ATT fragment per call; bt2usb's
`LongRead` decides how fragments add up, when a value is complete, and when it
is too large. That policy is pure and host-tested
([ADR 0003](0003-pure-core-and-task-shell.md)), while the vendored code is not.

## Consequences

Positive:

- A peripheral can no longer panic the bridge or hang discovery through these
  paths, and Report Maps up to 512 bytes are read whole.
- Builds resolve the same BLE code everywhere, and release packages identify it
  through the source commit ([ADR 0008](0008-attested-draft-releases.md)).

Negative:

- bt2usb carries a copy of an upstream crate. It keeps upstream formatting and
  is outside `cargo fmt --package bt2usb` (the portal test pulls its two
  modules in under `#[rustfmt::skip]`). Only the event portal has host tests,
  because it is the only patched module that builds without the SoftDevice
  (with `util/on_drop.rs`, which the test compiles alongside it).
- The vendored crate depends on `heapless` 0.9 and `embassy-sync` 0.8, while
  bt2usb itself uses 0.8 and 0.7. `embassy-usb` pulls in the same newer
  versions, so `Cargo.lock` carries two versions of each either way; the patch
  does not add to that.
- Upgrading `nrf-softdevice` means re-applying or dropping the patch and
  re-reading the upstream changes in between.
- The patched discovery and read paths have no host tests and no
  real-peripheral evidence yet (see
  [Verification Status](#verification-status)).

Follow-up obligations, tracked in [TODO.md](../../TODO.md):

- "Report Map interoperability and legacy policy": capture long Report Map reads
  and error handling from real peripherals at different MTUs, and decide whether
  the absent-map fallback stays allowed.
- "Parser fuzzing and property tests": fuzz the descriptor and long-read paths.
- "Supply-chain and tooling maintenance": include the vendored crate in license
  and SBOM checks.
- Offer the changes upstream, and drop the patch once an upstream release
  provides the same guarantees.

## Implementation

| Concern | Where |
| --- | --- |
| Pin and patch | `nrf-softdevice` and `nrf-softdevice-s140` with `rev = "47d6121c6e823120e8b883a7ac75f44ce7daa3aa"` and the `[patch]` entry in [Cargo.toml](../../Cargo.toml); the resolved graph in `Cargo.lock` |
| Vendored crate | `vendor/nrf-softdevice/` with `LICENSE-MIT`, `LICENSE-APACHE`, and its own `Cargo.toml`, which differs from upstream only in turning the sibling crates' `path` dependencies into git dependencies at the same `rev` and in adding the `log-sensitive-data` feature |
| Patched functions | `read_by_offset` (new), `read`, the new variants `ReadError::{Timeout, InvalidResponse}`, `DiscoverError::{Timeout, InvalidResponse, TooManyAttributes}`, and `MtuExchangeError::Timeout` (`ReadError::Truncated` is upstream's), and the stored MTU in `att_mtu_exchange` (since 2026-10-10), in `vendor/nrf-softdevice/src/ble/gatt_client.rs`; the refused-exchange arm in `central::connect_inner`, and `on_gatts_evt_without_server` in `ble/mod.rs` with `try_with_state_by_conn_handle` in `connection.rs` (both since 2026-10-10) |
| Fragment assembly | `LongRead` in [long_read.rs](../../src/ble/long_read.rs): `MAX_ATTRIBUTE_LEN = 512`, MTU accepted only in `23..=517`, completion only on a short final fragment or a valid end-of-value response |
| Report Map read | `read_report_map` in [hid_client.rs](../../src/ble/hid_client.rs) maps failures to `BleErrorTag::ReportMapReadFailed`, `ReportMapTooLarge`, or `ReportMapInvalid`, shown as "HID map read failed", "HID map too large", and "Unsupported HID map" |
| Absent map | Only a missing Report Map characteristic allows legacy classification, logged as `HID report map absent; using legacy report classification` |
| Discovery failures | `HID discovery failed: {:?}` in `hid_client.rs` |
| Security handler calls | The `PASSKEY_DISPLAY`, `AUTH_KEY_REQUEST`, `CONN_SEC_UPDATE`, and `AUTH_STATUS` arms of `gap::on_evt`, and `Connection::encrypt` and `Connection::request_pairing` in `connection.rs` (since 2026-10-10), each marked `bt2usb patch:` |
| Connection parameter hook ([ADR 0016](0016-bounded-peer-connection-parameters.md)) | `SecurityHandler::conn_param_update_request` (default: grant unchanged) in `vendor/nrf-softdevice/src/ble/security.rs`, called from the `CONN_PARAM_UPDATE_REQUEST` arm in `vendor/nrf-softdevice/src/ble/gap.rs` |
| Sensitive log gate | `#[cfg(feature = "log-sensitive-data")]` pairs on the connect line in `vendor/nrf-softdevice/src/ble/central.rs`, the passkey-display line in `gap.rs`, and the notification line in `gatt_client.rs`; the root feature in [Cargo.toml](../../Cargo.toml); a Clippy run with the feature in the CI "Embedded build & clippy" job |
| ATT MTU | `ATT_MTU = 64` in [sd_setup.rs](../../src/sd_setup.rs); the vendor notes explain why one discovery response can then carry eight declarations |
| Panic fixes ([ADR 0025](0025-panic-lints-and-inventory.md)) | `evt-max-size-256` in [Cargo.toml](../../Cargo.toml) and the event-size `const` assertion in [sd_setup.rs](../../src/sd_setup.rs); the timeout arm of `gap::on_evt`; `ConnectionState::disconnect_with_reason` and `Connection::drop` in `connection.rs`; the waiters in `discover_service`, `discover_characteristics`, `discover_descriptors`, and `att_mtu_exchange`. Each source change is marked `bt2usb patch:` |
| Event portal guard (since 2026-10-11) | `Portal::closure_address` and `Portal::clear_if_registered`, called from the drop guards of `wait_once` and `wait_many`, in `vendor/nrf-softdevice/src/util/portal.rs`, marked `bt2usb patch:`; five host tests in [tests/vendor_portal.rs](../../tests/vendor_portal.rs), which compiles that file and `util/on_drop.rs` with `embassy-sync` 0.8 and a std `critical-section` as dev-dependencies |
| Formatting | CI checks formatting of the application package only: `cargo fmt --package bt2usb -- --check` in [ci.yml](../../.github/workflows/ci.yml) |

### Verification Status

- **Implemented:** the pin, the vendored crate, and every change listed above.
- **Software-verified:** host tests cover `LongRead`, including exact-MTU
  endings and oversized values. The vendored crate compiles into every
  embedded build; the CI "Embedded build & clippy" job, which builds it,
  passed on GitHub-hosted runners in push runs 36441995385 (`8a04b25`,
  2026-09-28) and 37932436721 (`7fc99d6`, 2026-10-09) and scheduled run
  37338711407 (2026-10-05). The CI format check covers only the application
  package. Until 2026-10-11 no test exercised a patched function; the
  GATT client, GAP, and connection changes are still reviewed, not tested.
  The event portal guard has five host tests in `tests/vendor_portal.rs`,
  three of which fail on the unpatched source
  ([validation record](../testing.md#validation-record--2026-10-11-portal-clears-only-its-own-wait)). The log gate was checked on 2026-10-10 by listing the defmt
  format strings of `debug`, `trace`, and `info` builds with and without the
  feature ([logging and privacy](../security.md#logging-and-privacy)). The
  event-size check was tried against a 131-byte buffer, which fails the build,
  and a 132-byte one, which passes; the other panic fixes were reviewed, not
  tested ([ADR 0025](0025-panic-lints-and-inventory.md#verification-status)).
- **Hardware-verified:** not yet. The vendored functions run only on the
  board, and the repository holds no record of long Report Map reads,
  discovery, or timeouts against real peripherals.

## Related

- [Vendor notes](../../vendor/nrf-softdevice/README.bt2usb.md)
- [Architecture: HID path and limits](../architecture.md#hid-path-and-limits)
- [Security: supply chain](../security.md#supply-chain)
- [Development: toolchain](../development.md#toolchain)
- [ADR 0002: nRF52840, SoftDevice S140, and Embassy](0002-nrf52840-softdevice-embassy.md)
- [ADR 0003: Hardware-free decision modules](0003-pure-core-and-task-shell.md)
- [ADR 0008: Attested draft releases](0008-attested-draft-releases.md)
- [ADR 0013: Pinned toolchain and mask tasks](0013-pinned-toolchain-and-mask-tasks.md)
- [ADR 0025: Panic lints and inventory](0025-panic-lints-and-inventory.md)
