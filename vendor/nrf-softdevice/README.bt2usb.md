# bt2usb local patch

This crate is copied from embassy-rs/nrf-softdevice commit
`47d6121c6e823120e8b883a7ac75f44ce7daa3aa` under its original MIT / Apache-2.0
licenses. Sibling crates remain git dependencies at the same commit.

The GATT client changes are all in `src/ble/gatt_client.rs`. Three further
changes are described at the end: a connection-parameter hook in
`src/ble/security.rs` and `src/ble/gap.rs`, a `log-sensitive-data` feature
that keeps peer addresses, passkeys, and notification bytes out of the logs by
default, and fixes for panics a peer could reach in `src/ble/gap.rs` and
`src/ble/connection.rs`. The connection-parameter hook and the panic fixes
are marked `bt2usb patch:` in the source, as is the event portal fix in
`src/util/portal.rs` described last. The first GATT change adds
`gatt_client::read_by_offset` and makes `read`
delegate to it with offset zero. It exposes the SoftDevice's ATT Read Blob
support through the existing response portal; the response handle and offset
are checked before copying data. The application assembles and bounds fragments
in `src/ble/long_read.rs`, including exact-MTU endings and oversized values.
ATT timeouts return `ReadError::Timeout` so an unresponsive peer cannot leave a
read future waiting after the SoftDevice has abandoned the request.
Service/characteristic/descriptor discovery and MTU exchange likewise return
their respective `Timeout` errors instead of panicking on the timeout event,
and since 2026-10-10 they skip any other event they did not expect and keep
waiting, as `read_by_offset` and `write` do, instead of panicking with
`unexpected event {}`. Also since 2026-10-10, the MTU exchange stores the ATT
MTU the SoftDevice actually uses: the smaller of the requested MTU and the
server's offer, and never less than 23 (the S140 documentation of
`sd_ble_gattc_exchange_mtu_request`). Upstream stored the server's offer, so
with a peripheral offering more than bt2usb's 64, `Connection::att_mtu`
overstated the link's MTU, and bt2usb's Report Map reader took the first
63-byte fragment for the last one. And `central::connect_inner` no longer
fails the connection when the peripheral answers the Exchange MTU Request
with an ATT error, such as Request Not Supported: it logs `att mtu exchange
refused: {:?}; keeping the default mtu` and keeps the link at the default
ATT MTU of 23. A timeout, a disconnect, or a SoftDevice error still fails the
connect.

GATT server events without the GATT server feature (2026-10-10): bt2usb
leaves `ble-gatt-server` off, and upstream then dropped every GATT server
event in `ble::on_evt`, including the two the SoftDevice waits on the
application for. A peripheral that also acts as a GATT client and sends its
own Exchange MTU Request, or reads or writes the Service Changed CCCD that
S140 includes by default, got no answer; its ATT transaction timed out after
30 s, after which it may send no more ATT PDUs on the link, notifications
included. `on_gatts_evt_without_server` in `src/ble/mod.rs` now answers the
MTU request with the configured MTU as Server RX MTU and records the smaller
of the two RX MTUs (never below 23) through `try_with_state_by_conn_handle`,
a non-panicking lookup added to `src/ble/connection.rs`, and answers
`BLE_GATTS_EVT_SYS_ATTR_MISSING` with default system attributes. Both are
compiled only without `ble-gatt-server`, whose module answers them itself.

Discovery no longer panics on peer-controlled counts or handles. With the
configured 64-byte ATT MTU one response can carry eight characteristic
declarations; `discover` keeps the first six and resumes after the last kept
handle, so the SoftDevice reports the remainder in the next request. A
characteristic with more descriptors than the fixed buffer returns
`DiscoverError::TooManyAttributes`. An empty response, or a declaration outside
the requested range, out of order or not advancing, returns
`DiscoverError::InvalidResponse`, and handle arithmetic saturates. Before this,
a peer could panic the firmware, or make the loop wrap at handle `0xFFFF` and
never finish.

Connection parameter requests: upstream answers a peripheral's
`BLE_GAP_EVT_CONN_PARAM_UPDATE_REQUEST` by granting the requested parameters
unchanged. The patch adds `SecurityHandler::conn_param_update_request`, whose
default returns the request unchanged, and the event handler in `gap.rs` now
looks the connection up by handle and replies with whatever the connection's
security handler returns. bt2usb's `Bonder` uses it to keep the interval,
latency and supervision timeout inside fixed bounds
(`src/ble/conn_params.rs` in the application; ADR 0016).

Sensitive log values: upstream logs the peer address of every connection at
debug (`connected role={:?} peer_addr={:?}` in `src/ble/central.rs`), and at
trace each notification's bytes (`GATT_HVX ... data={:?}` in
`src/ble/gatt_client.rs`), which on a keyboard are keystrokes, and any
displayed passkey (`on_passkey_display passkey={}` in `src/ble/gap.rs`). The
patch adds a `log-sensitive-data` feature to `Cargo.toml`, off by default.
Without it the three lines log the role, the notification length
(`len={}`), and the bare event name instead. bt2usb forwards its own
`log-sensitive-data` feature to it; `docs/security.md` ("Logging And Privacy")
lists every dependency log line and when the opt-in is acceptable.

Panics a peer could reach (2026-10-10, bt2usb ADR 0025): `gap::on_evt`
panicked on a GAP timeout source other than connect and scan; S140 also
reports the authenticated payload timeout (source 3) when an encrypted link
carries no packet with a valid MIC for 480 s, which a peer can cause. The arm
now logs `unhandled timeout src {:?}` and leaves the link up.
`ConnectionState::disconnect_with_reason` unwrapped any SoftDevice error other
than `NRF_ERROR_INVALID_STATE`, and `Connection::drop` unwrapped the
`DisconnectedError` returned for that one, so dropping a connection whose link
had just ended (for example after a failed MTU exchange in `connect_inner`)
halted the chip. The first now logs `sd_ble_gap_disconnect err {:?}` and
returns `DisconnectedError`, and the drop accepts it. The third fix is in
bt2usb's `Cargo.toml`, not in this source: it enables `evt-max-size-256`,
because at the configured 64-byte ATT MTU a primary-service discovery
response can be a 132-byte event, over the default 128-byte buffer, and
`events::run_ble` panics on a too-small buffer. bt2usb's `sd_setup.rs` checks
the buffer against the MTU at compile time. The panic paths left in the
compiled modules are listed in bt2usb's `docs/code-quality.md` ("Vendored
nrf-softdevice"), each with why it does not fire. One of them,
`Address::address_type`, unwraps the address type and panics on a reserved
one; bt2usb never calls it on the identity address a peer sends during
pairing, and decodes that type itself in `src/storage.rs`.

Security handler calls outside the connection state (2026-10-10): upstream
calls `SecurityHandler` methods inside `Connection::with_state`, which holds
a `&mut ConnectionState` for the closure's duration: `on_bonded` in the
`AUTH_STATUS` arm of `gap::on_evt`, `on_security_update` in
`CONN_SEC_UPDATE`, `display_passkey`, `enter_passkey`, and
`recv_out_of_band` (whose `Connection::from_handle` also re-entered the state)
in `PASSKEY_DISPLAY` and `AUTH_KEY_REQUEST`, `get_peripheral_key` in
`Connection::encrypt`, and `security_params` in `Connection::request_pairing`.
A handler that reads the connection, as bt2usb's `Bonder` does with
`Connection::peer_address` in `on_bonded`, `get_key`, and
`get_peripheral_key`, then took a second `&mut` to the same state through
its `UnsafeCell` while the first was live, which is undefined behavior even
when both only read. Each site now copies what the handler needs (the handler
reference, the keys, the identity, the security mode) out of `with_state` and
calls the handler after it returns. Three such sites remain in code bt2usb
does not compile: `security_params` in the peripheral `SEC_PARAMS_REQUEST`
arm and `can_bond` and `request_mitm_protection` in
`Connection::request_security` (feature `ble-peripheral`), and
`save_sys_attrs` in `ConnectionState::on_disconnected` (feature
`ble-gatt-server`).

Event portal registrations (2026-10-11): `Portal::wait_once` and
`wait_many` arm a drop guard that upstream let reset the portal to
`State(None)` whenever the wait ended, completed or cancelled, without
checking whose closure the portal held. A wait whose closure had already run
and cleared the portal, such as a GATT wait that `on_disconnected` failed with
`DISCONNECTED`, then erased any closure registered after it. When a peer
drops a link and the SoftDevice gives the freed connection handle to another
link in the same event drain, embassy-executor can poll the new link's task
first: its MTU exchange registers on the handle's portal, and the old task's
guard then erases it, so the new connect never sees its response, timeout, or
disconnect and holds bt2usb's GAP procedure lock until reset. The guard now
calls `Portal::clear_if_registered`, which resets the portal only while it
holds the closure at the waiter's own address. Two live waiters never share
an address, and a waiter's closure outlives its guard. bt2usb's
`tests/vendor_portal.rs` compiles this file and `src/util/on_drop.rs` on the
host and tests the guard; three of its five tests fail on the upstream code.

Remove this patch only when the pinned upstream provides equivalent offset
reads, timeout errors, bounded discovery, a way for the application to
answer connection parameter requests, a way to keep peer addresses,
passkeys, and notification bytes out of debug and trace logs, no panic
on an unexpected timeout source or a disconnect error, an ATT MTU that
matches the one the SoftDevice uses, a connect that survives a refused
MTU exchange, answers to a peer's Exchange MTU Request and system
attribute access without the GATT server feature, security handler
calls made outside the connection state, and a portal whose ended wait
clears only its own registration. Do not edit the Cargo checkout to deploy this change; the root Cargo
patch and committed vendor sources make builds reproducible.
