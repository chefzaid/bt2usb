//! bt2usb patch: whether a link may pair when its peer asks.
//!
//! A peripheral's Security Request makes upstream start pairing whenever no
//! keys are found for the peer. The central application decides which links
//! may pair; every new link starts without that permission, so a Security
//! Request on a link the application did not open for pairing is left
//! unanswered. Kept free of SoftDevice types so the host tests in bt2usb's
//! `tests/vendor_pairing_gate.rs` can compile this file.

/// What the central does with a peer's Security Request when it holds no
/// keys for the peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecurityRequestAnswer {
    /// Start pairing, as upstream always did.
    Pair,
    /// Leave the link as it is; the peer may disconnect.
    Ignore,
}

/// Whether the application allowed pairing on one link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairingPermission(bool);

impl PairingPermission {
    /// Every new link's permission: a peer's request cannot start pairing.
    /// Its bit pattern is zero, as the connection state's `.bss` placement
    /// requires.
    pub const NEW_LINK: Self = Self(false);

    /// The permission the application grants with `Connection::allow_pairing`.
    pub const GRANTED: Self = Self(true);

    /// Whether pairing may run on the link.
    pub const fn allows_pairing(self) -> bool {
        self.0
    }

    /// The answer to a Security Request from a peer whose keys are not held.
    pub const fn on_security_request_without_keys(self) -> SecurityRequestAnswer {
        if self.0 {
            SecurityRequestAnswer::Pair
        } else {
            SecurityRequestAnswer::Ignore
        }
    }
}
