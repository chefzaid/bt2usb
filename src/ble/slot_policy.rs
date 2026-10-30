//! A connection slot's decisions, apart from the SoftDevice procedures and
//! channels that carry them out in `slot_link.rs` and `slot_worker.rs`: the
//! error a link that would not encrypt fails with, and what the slot does
//! once an attempt ends: reconnect in the background, report an error, or
//! make the user's connection that took the attempt over.
//!
//! The coordinator's reducers decide what the reports mean for the slot table
//! ([`coordinator`](super::coordinator)); these decide what the worker
//! reports and whether it keeps trying.

use super::coordinator::ErrorTag;

/// How securing a new link ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Securing {
    /// The link encrypted with the keys the bridge holds for the peer.
    Encrypted,
    /// The stored keys were sent but the link stayed unencrypted. `refused`
    /// when the peer's security update showed it rejected them; otherwise the
    /// link dropped or the wait for encryption ran out.
    KeysNotAccepted { refused: bool },
    /// The bridge held no keys for the peer and the user's connection paired:
    /// whether the new pairing encrypted the link.
    Paired(bool),
    /// The bridge holds no keys for the peer, and a background reconnect
    /// never pairs.
    NoKeys,
    /// Encryption could not start for another reason.
    Failed,
}

/// The error a link that did not encrypt fails with, naming what the user
/// must do about it, or `None` once the link is encrypted.
pub fn securing_error(securing: Securing) -> Option<ErrorTag> {
    match securing {
        Securing::Encrypted | Securing::Paired(true) => None,
        Securing::KeysNotAccepted { refused: true } => Some(ErrorTag::KeysRejected),
        Securing::Paired(false) => Some(ErrorTag::PairingFailed),
        Securing::NoKeys => Some(ErrorTag::KeysMissing),
        // Nothing says the keys are wrong, so a background reconnect retries.
        Securing::KeysNotAccepted { refused: false } | Securing::Failed => {
            Some(ErrorTag::ConnectFailed)
        }
    }
}

/// How a connection attempt ended, unless a command superseded it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttemptEnd<D> {
    /// A working link closed.
    Closed,
    /// The attempt failed before the link was usable.
    Failed(ErrorTag),
    /// The user selected the device being reached, which took the attempt
    /// over, and the attempt then failed. Holds the device the user selected.
    TakenOver(D),
}

/// What a slot does once an attempt has ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextStep<D> {
    /// Report the link lost and reconnect to the device in the background.
    LinkLost(D),
    /// Try the device again in the background without reporting anything:
    /// a background reconnect did not connect.
    RetryQuietly(D),
    /// Make the user's connection to `device`, which may pair, and go back to
    /// the background retry `resume` should it fail.
    Connect { device: D, resume: D },
    /// Report the error, and go back to `resume` if there is one (reported
    /// as `retrying`); otherwise the slot is free.
    Report { tag: ErrorTag, resume: Option<D> },
}

/// Decide what a slot does after an attempt to `device` ended. `silent` marks
/// a background reconnect. `replaced` is the background retry that the
/// user's connection replaced, if it did. `has_keys` says whether the bridge
/// still holds keys for a device: without them a background reconnect, which
/// never pairs, cannot secure a link, so the slot reports `KeysMissing`
/// instead of holding on to the device.
pub fn after_attempt<D>(
    end: AttemptEnd<D>,
    device: D,
    silent: bool,
    replaced: Option<D>,
    has_keys: impl Fn(&D) -> bool,
) -> NextStep<D> {
    match end {
        AttemptEnd::Closed if has_keys(&device) => NextStep::LinkLost(device),
        AttemptEnd::Closed => NextStep::Report {
            tag: ErrorTag::KeysMissing,
            resume: None,
        },
        AttemptEnd::TakenOver(next) => NextStep::Connect {
            device: next,
            resume: device,
        },
        AttemptEnd::Failed(ErrorTag::ConnectFailed) if silent => NextStep::RetryQuietly(device),
        // A failed user connection goes back to the retry it replaced while
        // the keys for that device remain, unless they were found wanting.
        AttemptEnd::Failed(tag) => NextStep::Report {
            tag,
            resume: replaced.filter(|target| !tag.needs_new_pairing() && has_keys(target)),
        },
    }
}

#[cfg(test)]
#[path = "slot_policy_tests.rs"]
mod tests;
