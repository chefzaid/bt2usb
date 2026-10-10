# bt2usb local patch

This crate is copied from embassy-rs/nrf-softdevice commit
`47d6121c6e823120e8b883a7ac75f44ce7daa3aa` under its original MIT / Apache-2.0
licenses. Sibling crates remain git dependencies at the same commit.

The GATT client changes are all in `src/ble/gatt_client.rs`. Two further
changes are described at the end: a connection-parameter hook in
`src/ble/security.rs` and `src/ble/gap.rs`, and a `log-sensitive-data` feature
that keeps peer addresses, passkeys, and notification bytes out of the logs by
default. The first GATT change adds
`gatt_client::read_by_offset` and makes `read`
delegate to it with offset zero. It exposes the SoftDevice's ATT Read Blob
support through the existing response portal; the response handle and offset
are checked before copying data. The application assembles and bounds fragments
in `src/ble/long_read.rs`, including exact-MTU endings and oversized values.
ATT timeouts return `ReadError::Timeout` so an unresponsive peer cannot leave a
read future waiting after the SoftDevice has abandoned the request.
Service/characteristic/descriptor discovery and MTU exchange likewise return
their respective `Timeout` errors instead of panicking on the timeout event.

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

Remove this patch only when the pinned upstream provides equivalent offset
reads, timeout errors, bounded discovery, a way for the application to
answer connection parameter requests, and a way to keep peer addresses,
passkeys, and notification bytes out of debug and trace logs. Do not edit the Cargo checkout to deploy this change; the root Cargo
patch and committed vendor sources make builds reproducible.
