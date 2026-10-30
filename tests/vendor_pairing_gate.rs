//! Host tests for the vendored nrf-softdevice pairing permission, compiled
//! from `vendor/nrf-softdevice/src/ble/pairing_gate.rs` itself.
//!
//! A peer's Security Request must not start pairing on a link the bridge did
//! not open for pairing, such as a background reconnect, so a peripheral
//! cannot create or replace a bond without the user choosing it (ADR 0007,
//! `vendor/nrf-softdevice/README.bt2usb.md`).

// The vendored sources keep their own formatting: `rustfmt::skip` stops
// `cargo fmt` from following this module into `vendor/`.
#[rustfmt::skip]
#[allow(unused, clippy::all, clippy::restriction)]
#[path = "../vendor/nrf-softdevice/src/ble/pairing_gate.rs"]
mod pairing_gate;

use pairing_gate::{PairingPermission, SecurityRequestAnswer};

#[test]
fn a_new_link_ignores_a_security_request_without_keys() {
    let permission = PairingPermission::NEW_LINK;
    assert!(!permission.allows_pairing());
    assert_eq!(
        permission.on_security_request_without_keys(),
        SecurityRequestAnswer::Ignore
    );
}

#[test]
fn a_link_allowed_to_pair_pairs_on_a_security_request() {
    let permission = PairingPermission::GRANTED;
    assert!(permission.allows_pairing());
    assert_eq!(
        permission.on_security_request_without_keys(),
        SecurityRequestAnswer::Pair
    );
}
