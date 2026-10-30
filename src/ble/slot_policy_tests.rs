//! Host tests for a connection slot's decisions (`ble/slot_policy.rs`): the
//! error a link that would not encrypt fails with, and what the slot does
//! after each way an attempt can end.

use super::*;

const KEYBOARD: &str = "keyboard";
const MOUSE: &str = "mouse";

/// The bridge holds keys for the keyboard only.
fn keys(device: &&str) -> bool {
    *device == KEYBOARD
}

// ── Securing a link ──────────────────────────────────────────────────────────

#[test]
fn an_encrypted_link_has_no_error() {
    assert_eq!(securing_error(Securing::Encrypted), None);
    assert_eq!(securing_error(Securing::Paired(true)), None);
}

#[test]
fn a_link_that_stays_unencrypted_fails_with_what_the_user_must_do() {
    for (securing, tag) in [
        // Pair the device again: it no longer has the keys, or the bridge
        // never had them.
        (
            Securing::KeysNotAccepted { refused: true },
            ErrorTag::KeysRejected,
        ),
        (Securing::NoKeys, ErrorTag::KeysMissing),
        // Try the pairing again.
        (Securing::Paired(false), ErrorTag::PairingFailed),
        // Nothing to do: a background reconnect retries.
        (
            Securing::KeysNotAccepted { refused: false },
            ErrorTag::ConnectFailed,
        ),
        (Securing::Failed, ErrorTag::ConnectFailed),
    ] {
        assert_eq!(securing_error(securing), Some(tag), "{securing:?}");
    }
}

// ── After an attempt ─────────────────────────────────────────────────────────

#[test]
fn a_closed_link_reconnects_in_the_background_while_its_keys_remain() {
    for silent in [false, true] {
        assert_eq!(
            after_attempt(AttemptEnd::Closed, KEYBOARD, silent, None, keys),
            NextStep::LinkLost(KEYBOARD)
        );
        // Its pairing was refused or evicted, or it paired without bonding.
        assert_eq!(
            after_attempt(AttemptEnd::Closed, MOUSE, silent, None, keys),
            NextStep::Report {
                tag: ErrorTag::KeysMissing,
                resume: None
            }
        );
    }
}

#[test]
fn a_background_reconnect_retries_quietly_only_when_it_did_not_connect() {
    let failed = |tag| after_attempt(AttemptEnd::Failed(tag), KEYBOARD, true, None, keys);
    assert_eq!(
        failed(ErrorTag::ConnectFailed),
        NextStep::RetryQuietly(KEYBOARD)
    );
    // Any other failure ends it with the error, such as a peer that refused
    // its keys or no longer has a HID service.
    for tag in ErrorTag::ALL {
        if tag != ErrorTag::ConnectFailed {
            assert_eq!(failed(tag), NextStep::Report { tag, resume: None });
        }
    }
}

#[test]
fn a_failed_user_connection_is_reported_and_frees_a_slot_it_did_not_take_over() {
    for tag in ErrorTag::ALL {
        assert_eq!(
            after_attempt(AttemptEnd::Failed(tag), MOUSE, false, None, keys),
            NextStep::Report { tag, resume: None }
        );
    }
}

#[test]
fn a_failed_user_connection_resumes_the_retry_it_replaced_while_that_can_succeed() {
    for tag in ErrorTag::ALL {
        let resume = (!tag.needs_new_pairing()).then_some(KEYBOARD);
        assert_eq!(
            after_attempt(
                AttemptEnd::Failed(tag),
                KEYBOARD,
                false,
                Some(KEYBOARD),
                keys
            ),
            NextStep::Report { tag, resume },
            "{tag:?}"
        );
        // Without keys for the replaced retry's device, it cannot reconnect.
        assert_eq!(
            after_attempt(AttemptEnd::Failed(tag), KEYBOARD, false, Some(MOUSE), keys),
            NextStep::Report { tag, resume: None }
        );
    }
}

#[test]
fn an_attempt_taken_over_by_the_user_makes_their_connection_and_keeps_the_retry() {
    // The same keyboard as the user selected it from a scan, perhaps at a new
    // private address.
    let selected = "keyboard, selected";
    for silent in [false, true] {
        assert_eq!(
            after_attempt(
                AttemptEnd::TakenOver(selected),
                KEYBOARD,
                silent,
                None,
                keys
            ),
            NextStep::Connect {
                device: selected,
                resume: KEYBOARD
            }
        );
    }
}
