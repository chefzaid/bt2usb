//! Fault tests for the slot worker loop (`ble/slot_loop.rs`): the production
//! loop runs on the bench in `slot_loop_bench.rs`, a scripted radio whose
//! events go through the real coordinator reducers, so each test checks what
//! the worker does, what it reports, and what the UI ends up showing.
//! Covered: commands cancelling a scan or a link, a full event channel, the
//! other slot's scan waking a retry, repeated security failures, a device
//! that disappears during discovery, and keys that are gone.

use super::bench::*;
use super::*;
use crate::ble::coordinator::{self, UiEvent};
use core::pin::pin;
use core::task::{Context, Waker};

#[test]
fn a_lost_link_reconnects_at_the_new_address_the_scan_finds() {
    let mut bench = Bench::new(&[KEYBOARD]);
    let mut fake = bench.fake();
    let mut worker = pin!(run(&mut fake, SLOT));
    bench.power_up();
    bench.settle(&mut worker);
    // Scanning for the keyboard, which is asleep: nothing is shown yet.
    assert_eq!(bench.state.target.get(), Some(KEYBOARD));
    assert!(bench.ui.is_empty() && bench.attempts().is_empty());

    bench.state.scans.borrow_mut().push_back(Some(KEYBOARD_NOW));
    bench.link([Step::Up]);
    bench.settle(&mut worker);
    assert_eq!(bench.attempts(), [(KEYBOARD_NOW, false)]);
    assert_eq!(bench.ui, [shows_keyboard()]);

    // It sleeps again; the slot stays reserved and scans for it.
    bench.link([Step::End(SlotOutcome::Closed)]);
    bench.settle(&mut worker);
    assert_eq!(bench.ui.last(), Some(&UiEvent::Disconnected));
    assert!(bench.manager.is_slot_occupied(SLOT));
    let lost = Call::Note(Note::LinkLostReconnecting);
    assert!(bench.state.calls.borrow().contains(&lost));

    bench.state.backoffs.set(1);
    bench.state.scans.borrow_mut().push_back(Some(KEYBOARD_NOW));
    bench.link([Step::Up]);
    bench.settle(&mut worker);
    assert_eq!(bench.attempts().len(), 2);
    assert_eq!(bench.ui.last(), Some(&shows_keyboard()));
}

#[test]
fn a_device_gone_during_discovery_is_retried_quietly_in_the_background() {
    let mut bench = Bench::new(&[KEYBOARD]);
    let mut fake = bench.fake();
    let mut worker = pin!(run(&mut fake, SLOT));
    bench.power_up();
    for round in 1..=3 {
        bench.state.scans.borrow_mut().push_back(Some(KEYBOARD_NOW));
        bench.link([Step::End(SlotOutcome::Failed(ErrorTag::ConnectFailed))]);
        bench.state.backoffs.set(1);
        bench.settle(&mut worker);
        assert_eq!(bench.attempts().len(), round);
        assert_eq!(bench.state.failed_attempts.get(), round as u32);
    }
    // Nothing reaches the UI, and the slot keeps waiting for the keyboard,
    // now at the address it was last seen at.
    assert!(bench.ui.is_empty());
    assert!(bench.manager.is_slot_occupied(SLOT));
    assert_eq!(bench.state.target.get(), Some(KEYBOARD_NOW));
}

#[test]
fn a_user_connection_whose_device_disappears_reports_and_frees_the_slot() {
    let mut bench = Bench::new(&[]);
    let mut fake = bench.fake();
    let mut worker = pin!(run(&mut fake, SLOT));
    bench.select();
    bench.link([Step::End(SlotOutcome::Failed(ErrorTag::ConnectFailed))]);
    bench.settle(&mut worker);
    assert_eq!(bench.attempts(), [(KEYBOARD, true)]);
    assert_eq!(bench.ui, failed(ErrorTag::ConnectFailed));
    assert!(!bench.manager.is_slot_occupied(SLOT));
    assert_eq!(bench.discarded, [KEYBOARD]);
    // The slot does not retry on its own.
    bench.state.backoffs.set(1);
    bench.state.sighted.set(true);
    bench.settle(&mut worker);
    assert_eq!(bench.attempts().len(), 1);
}

#[test]
fn refused_keys_end_the_retry_and_a_failed_new_pairing_can_be_tried_again() {
    let mut bench = Bench::new(&[KEYBOARD]);
    let mut fake = bench.fake();
    let mut worker = pin!(run(&mut fake, SLOT));
    bench.power_up();
    bench.state.scans.borrow_mut().push_back(Some(KEYBOARD_NOW));
    bench.link([Step::End(SlotOutcome::Failed(ErrorTag::KeysRejected))]);
    bench.settle(&mut worker);
    assert_eq!(bench.ui, failed(ErrorTag::KeysRejected));
    assert!(!bench.manager.is_slot_occupied(SLOT));
    assert_eq!(bench.state.target.get(), None);
    bench.state.backoffs.set(1);
    bench.state.sighted.set(true);
    bench.settle(&mut worker);
    assert_eq!(bench.attempts(), [(KEYBOARD_NOW, false)]);

    // Pairing again fails twice, then succeeds; each failure is shown.
    for _ in 0..2 {
        bench.select();
        bench.link([Step::End(SlotOutcome::Failed(ErrorTag::PairingFailed))]);
        bench.settle(&mut worker);
        assert!(bench.ui.ends_with(&failed(ErrorTag::PairingFailed)));
    }
    bench.select();
    bench.link([Step::Up]);
    bench.settle(&mut worker);
    assert_eq!(bench.ui.last(), Some(&shows_keyboard()));
    let paired = [(KEYBOARD, true); 3];
    assert_eq!(bench.attempts()[1..], paired);
}

#[test]
fn a_selection_during_the_scan_takes_the_retry_over_and_resumes_it_on_failure() {
    let mut bench = Bench::new(&[KEYBOARD]);
    let mut fake = bench.fake();
    let mut worker = pin!(run(&mut fake, SLOT));
    bench.power_up();
    bench.settle(&mut worker);
    bench.select();
    bench.link([Step::End(SlotOutcome::Failed(ErrorTag::ConnectFailed))]);
    bench.settle(&mut worker);
    // The user's connection may pair, and its failure is shown, but the slot
    // goes back to waiting for the keyboard.
    assert_eq!(bench.attempts(), [(KEYBOARD, true)]);
    assert_eq!(bench.ui, failed(ErrorTag::ConnectFailed));
    assert!(bench.manager.is_slot_occupied(SLOT));
    // Any pairing the failed connection made is dropped; saved keys stay.
    assert_eq!(bench.discarded, [KEYBOARD]);

    bench.state.backoffs.set(1);
    bench.state.scans.borrow_mut().push_back(Some(KEYBOARD_NOW));
    bench.link([Step::Up]);
    bench.settle(&mut worker);
    assert_eq!(bench.attempts()[1], (KEYBOARD_NOW, false));
    assert_eq!(bench.ui.last(), Some(&shows_keyboard()));
}

#[test]
fn a_selection_during_a_background_attempt_is_confirmed_under_its_number() {
    let mut bench = Bench::new(&[KEYBOARD]);
    let mut fake = bench.fake();
    let mut worker = pin!(run(&mut fake, SLOT));
    bench.power_up();
    bench.state.scans.borrow_mut().push_back(Some(KEYBOARD_NOW));
    bench.settle(&mut worker);
    // `slot_link` takes the attempt over when the selection arrives.
    bench.select();
    let taken = match bench.state.commands.borrow_mut().pop_front() {
        Some(SlotCommand::Connect { attempt, .. }) => attempt,
        other => panic!("{other:?}"),
    };
    bench.link([Step::TakeOver(taken), Step::Up]);
    bench.settle(&mut worker);
    assert_eq!(bench.ui, [shows_keyboard()]);
    assert_eq!(bench.manager.slot_attempt(SLOT), taken);
}

#[test]
fn commands_cancel_a_scan_a_link_or_a_retry_and_are_acknowledged() {
    let mut bench = Bench::new(&[KEYBOARD]);
    let mut fake = bench.fake();
    let mut worker = pin!(run(&mut fake, SLOT));
    // A disconnect while the reconnect scan runs: no attempt, slot free.
    bench.power_up();
    bench.settle(&mut worker);
    let actions = coordinator::plan_disconnect(&bench.manager);
    bench.execute(actions);
    bench.settle(&mut worker);
    assert!(bench.attempts().is_empty());
    assert!(!bench.manager.is_slot_occupied(SLOT));
    assert_eq!(bench.state.target.get(), None);

    // A disconnect that supersedes a running link.
    bench.select();
    bench.link([Step::Up]);
    bench.settle(&mut worker);
    let ended = SlotOutcome::Superseded(SlotCommand::Disconnect);
    bench.link([Step::End(ended)]);
    bench.settle(&mut worker);
    assert!(!bench.manager.is_slot_occupied(SLOT));
    assert_eq!(bench.ui.last(), Some(&UiEvent::Disconnected));

    // A quiesce between retries is acknowledged and ends the retry.
    bench.power_up();
    bench.state.scans.borrow_mut().push_back(None);
    bench.settle(&mut worker);
    bench
        .state
        .commands
        .borrow_mut()
        .push_back(SlotCommand::Quiesce(7));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(worker.as_mut().poll(&mut cx).is_pending());
    let quiesced = bench.state.events.borrow_mut().pop_front();
    assert_eq!(
        quiesced,
        Some(SlotEvent::Quiesced {
            slot: SLOT,
            token: 7
        })
    );
    bench.state.backoffs.set(1);
    bench.state.sighted.set(true);
    bench.settle(&mut worker);
    assert_eq!(bench.attempts().len(), 1);
    assert_eq!(bench.state.target.get(), None);
}

#[test]
fn a_full_event_channel_holds_the_worker_without_losing_or_reordering_reports() {
    let mut bench = Bench::new(&[KEYBOARD]);
    let mut fake = bench.fake();
    let mut worker = pin!(run(&mut fake, SLOT));
    bench.select();
    bench.link([Step::Up]);
    bench.settle(&mut worker);
    bench.state.channel_full.set(true);
    bench.link([Step::End(SlotOutcome::Closed)]);
    bench.state.backoffs.set(1);
    bench.state.sighted.set(true);
    bench.settle(&mut worker);
    // The LinkLost report waits; the worker starts nothing meanwhile.
    assert!(bench.state.events.borrow().is_empty());
    assert_eq!(bench.state.backoffs.get(), 1);
    assert_eq!(bench.attempts().len(), 1);

    bench.state.channel_full.set(false);
    bench.state.scans.borrow_mut().push_back(Some(KEYBOARD_NOW));
    bench.link([Step::Up]);
    bench.settle(&mut worker);
    assert_eq!(
        bench.ui,
        [shows_keyboard(), UiEvent::Disconnected, shows_keyboard()]
    );
    assert_eq!(bench.attempts().len(), 2);
}

#[test]
fn the_other_slots_sighting_wakes_a_retry_before_its_pause_ends() {
    let mut bench = Bench::new(&[KEYBOARD]);
    let mut fake = bench.fake();
    let mut worker = pin!(run(&mut fake, SLOT));
    bench.power_up();
    bench.state.scans.borrow_mut().push_back(None);
    bench.settle(&mut worker);
    assert_eq!(bench.state.target.get(), Some(KEYBOARD));
    // Pausing between scans; the other slot's scan sees the keyboard.
    bench.state.sighted.set(true);
    bench.state.scans.borrow_mut().push_back(Some(KEYBOARD_NOW));
    bench.link([Step::Up]);
    bench.settle(&mut worker);
    assert_eq!(bench.state.backoffs.get(), 0);
    assert_eq!(bench.attempts(), [(KEYBOARD_NOW, false)]);
    assert_eq!(bench.ui, [shows_keyboard()]);
}

#[test]
fn a_device_whose_keys_are_gone_frees_its_slot_with_keys_missing() {
    // Saved without keys, or evicted by a newer pairing, before power-up.
    let mut bench = Bench::new(&[]);
    let mut fake = bench.fake();
    let mut worker = pin!(run(&mut fake, SLOT));
    bench.power_up();
    bench.settle(&mut worker);
    assert!(bench.attempts().is_empty());
    assert_eq!(bench.ui, failed(ErrorTag::KeysMissing));
    assert!(!bench.manager.is_slot_occupied(SLOT));

    // Evicted while the link was up: losing it frees the slot too.
    bench.state.keys.borrow_mut().push(KEYBOARD);
    bench.select();
    bench.link([Step::Up]);
    bench.settle(&mut worker);
    bench.state.keys.borrow_mut().clear();
    bench.link([Step::End(SlotOutcome::Closed)]);
    bench.settle(&mut worker);
    assert!(bench.ui.ends_with(&failed(ErrorTag::KeysMissing)));
    assert!(!bench.manager.is_slot_occupied(SLOT));
    let notes = [Note::NoKeysToReconnect, Note::LinkLostWithoutKeys].map(Call::Note);
    let calls = bench.state.calls.borrow();
    assert!(notes.iter().all(|note| calls.contains(note)));
}
