//! Fault tests for the coordinator loop (`ble/coordinator_loop.rs`): the
//! production loop runs on the bench in `coordinator_loop_bench.rs`, a
//! scripted store, scanner, and channels, and each test plays the connection
//! workers. Covered: power-up with a store that cannot be written, user scans
//! and selections, a UI command racing a queued worker report, and the Forget
//! and factory-reset transaction with late, stale, and foreign reports and a
//! failed store write.

use super::bench::*;
use super::*;
use core::pin::pin;
use std::vec;

fn quiesced(slot: usize, token: u32) -> SlotEvent<u8> {
    SlotEvent::Quiesced { slot, token }
}

fn result(id: u32, result: Result<(), ErrorTag>) -> Call {
    Call::Emit(Event::ManagementResult { id, result })
}

#[test]
fn power_up_reports_an_unwritable_store_and_reconnects_the_newest_saved_devices() {
    let bench = Bench::new(&[MOUSE, KEYBOARD, REMOTE]);
    bench.state.writable.set(false);
    let mut fake = bench.fake();
    let mut coordinator = pin!(run(&mut fake));
    bench.settle(&mut coordinator);
    assert_eq!(
        bench.take(),
        [
            Call::Emit(Event::Error(ErrorTag::StorageFailed)),
            Call::Send(
                0,
                SlotCommand::Reconnect {
                    device: device(MOUSE),
                    attempt: 1
                }
            ),
            Call::Send(
                1,
                SlotCommand::Reconnect {
                    device: device(KEYBOARD),
                    attempt: 2
                }
            ),
        ]
    );

    // Nothing is shown until a link is up. The keyboard comes back at its new
    // address, which is saved as its live address.
    bench.connected(&mut coordinator, 1, KEYBOARD_NOW);
    assert_eq!(
        bench.take(),
        [
            Call::Persist(KEYBOARD_NOW),
            Call::Emit(shows(&["Keyboard"]))
        ]
    );
    // A report from an attempt the slot does not serve is ignored.
    let stale = SlotEvent::Error {
        slot: 0,
        attempt: 9,
        tag: ErrorTag::ConnectFailed,
        retrying: false,
    };
    bench.report(&mut coordinator, stale);
    assert!(bench.take().is_empty());

    bench.ui(&mut coordinator, Command::ListPaired { id: 5 });
    let devices = [MOUSE, KEYBOARD, REMOTE].map(device);
    let listed = Event::PairedDevices {
        id: 5,
        devices: Vec::from_slice(&devices).unwrap(),
    };
    assert_eq!(bench.take(), [Call::Emit(listed)]);
}

#[test]
fn a_scan_frees_both_busy_slots_and_selections_index_its_results_until_one_fails() {
    let bench = Bench::new(&[MOUSE, KEYBOARD]);
    let mut fake = bench.fake();
    let mut coordinator = pin!(run(&mut fake));
    bench.settle(&mut coordinator);
    bench.connected(&mut coordinator, 0, MOUSE);
    bench.connected(&mut coordinator, 1, KEYBOARD);
    bench.take();

    // Both slots are in use, so both are freed for the user to pick from
    // the scan, which runs meanwhile.
    bench.ui(&mut coordinator, Command::StartScan);
    let disconnect = |slot| Call::Send(slot, SlotCommand::Disconnect);
    assert_eq!(bench.take(), [disconnect(0), disconnect(1), Call::Scan]);
    // The workers' reports wait for the scan to end.
    let attempt = bench.attempt(0);
    bench.report(
        &mut coordinator,
        SlotEvent::Disconnected { slot: 0, attempt },
    );
    assert!(bench.take().is_empty());
    bench
        .state
        .scans
        .borrow_mut()
        .push_back(Some(vec![REMOTE, MOUSE]));
    bench.settle(&mut coordinator);
    let attempt = bench.attempt(1);
    bench.report(
        &mut coordinator,
        SlotEvent::Disconnected { slot: 1, attempt },
    );
    assert_eq!(
        bench.take(),
        [
            Call::Emit(shows(&["Keyboard"])),
            Call::Emit(Event::Disconnected)
        ]
    );

    bench.ui(&mut coordinator, Command::Connect(0));
    let remote = SlotCommand::Connect {
        device: device(REMOTE),
        attempt: bench.attempt(0),
    };
    assert_eq!(bench.take(), [Call::Send(0, remote)]);

    // One slot is free, so the next scan frees nothing. It fails, and
    // leaves nothing to select.
    bench.ui(&mut coordinator, Command::StartScan);
    bench.state.scans.borrow_mut().push_back(None);
    bench.settle(&mut coordinator);
    bench.ui(&mut coordinator, Command::Connect(0));
    assert_eq!(
        bench.take(),
        [
            Call::Scan,
            Call::Emit(Event::Error(ErrorTag::ConnectFailed))
        ]
    );
}

#[test]
fn a_selection_is_handled_before_a_queued_report_and_takes_the_retrying_slot() {
    let bench = Bench::new(&[KEYBOARD]);
    let mut fake = bench.fake();
    let mut coordinator = pin!(run(&mut fake));
    bench.settle(&mut coordinator);
    bench.ui(&mut coordinator, Command::StartScan);
    bench
        .state
        .scans
        .borrow_mut()
        .push_back(Some(vec![KEYBOARD_NOW]));
    bench.settle(&mut coordinator);
    bench.take();

    // The retry's link comes up just as the user selects the keyboard at its
    // new address. The selection goes first, to the slot retrying the
    // keyboard rather than the free one, and the retry's report is then
    // ignored: its attempt was replaced.
    let retry = bench.attempt(0);
    let up = SlotEvent::Connected {
        slot: 0,
        attempt: retry,
        device: device(KEYBOARD_NOW),
    };
    bench.state.events.borrow_mut().push_back(up);
    bench.ui(&mut coordinator, Command::Connect(0));
    let selected = SlotCommand::Connect {
        device: device(KEYBOARD_NOW),
        attempt: bench.attempt(0),
    };
    assert_ne!(bench.attempt(0), retry);
    assert_eq!(bench.take(), [Call::Send(0, selected)]);

    // The worker keeps the link and reports it under the new number.
    bench.connected(&mut coordinator, 0, KEYBOARD_NOW);
    assert_eq!(
        bench.take(),
        [
            Call::Persist(KEYBOARD_NOW),
            Call::Emit(shows(&["Keyboard"]))
        ]
    );
}

#[test]
fn forget_quiesces_only_the_slot_holding_the_device_and_drops_its_late_reports() {
    let bench = Bench::new(&[MOUSE, KEYBOARD]);
    let mut fake = bench.fake();
    let mut coordinator = pin!(run(&mut fake));
    bench.settle(&mut coordinator);
    bench.connected(&mut coordinator, 0, MOUSE);
    bench.connected(&mut coordinator, 1, KEYBOARD_NOW);
    bench.take();

    // The keyboard is up at a private address its identity key resolves.
    bench.ui(
        &mut coordinator,
        Command::Forget {
            id: 7,
            address: KEYBOARD,
        },
    );
    assert_eq!(bench.take(), [Call::Send(1, SlotCommand::Quiesce(1))]);

    // Until slot 1 acknowledges, its queued reports neither save the keyboard
    // again nor show it, UI commands wait, and the mouse's slot is served.
    let attempt = bench.attempt(1);
    let keyboard = device(KEYBOARD_NOW);
    let late = [
        SlotEvent::LinkLost {
            slot: 1,
            attempt,
            device: keyboard.clone(),
        },
        SlotEvent::Connected {
            slot: 1,
            attempt,
            device: keyboard,
        },
    ];
    for event in late {
        bench.report(&mut coordinator, event);
    }
    bench.ui(&mut coordinator, Command::Disconnect);
    assert!(bench.take().is_empty());
    let mouse_lost = SlotEvent::LinkLost {
        slot: 0,
        attempt: bench.attempt(0),
        device: device(MOUSE),
    };
    bench.report(&mut coordinator, mouse_lost);
    assert_eq!(bench.take(), [Call::Emit(shows(&["Keyboard"]))]);

    // Then the store changes, the UI hears the result, and the waiting
    // Disconnect reaches only the mouse's slot.
    bench.report(&mut coordinator, quiesced(1, 1));
    assert_eq!(
        bench.take(),
        [
            Call::Forget(Some(KEYBOARD)),
            Call::Emit(Event::Disconnected),
            result(7, Ok(())),
            Call::Send(0, SlotCommand::Disconnect),
        ]
    );
    assert_eq!(*bench.state.saved.borrow(), [MOUSE]);
}

#[test]
fn a_stale_or_unasked_acknowledgement_leaves_the_store_unchanged() {
    let bench = Bench::new(&[KEYBOARD]);
    let mut fake = bench.fake();
    let mut coordinator = pin!(run(&mut fake));
    bench.settle(&mut coordinator);
    bench.take();

    // Slot 0 is retrying the keyboard, so only it is quiesced.
    let forget = Command::Forget {
        id: 1,
        address: KEYBOARD,
    };
    bench.ui(&mut coordinator, forget);
    assert_eq!(bench.take(), [Call::Send(0, SlotCommand::Quiesce(1))]);
    // An earlier transaction's token, or a slot that was not asked, opens
    // nothing.
    bench.report(&mut coordinator, quiesced(0, 0));
    bench.report(&mut coordinator, quiesced(1, 1));
    assert!(bench.take().is_empty());

    // The retry had no link up, so a pairing it made was never saved and
    // goes with it.
    bench.report(&mut coordinator, quiesced(0, 1));
    assert_eq!(
        bench.take(),
        [
            Call::Discard(KEYBOARD),
            Call::Forget(Some(KEYBOARD)),
            Call::Emit(Event::Disconnected),
            result(1, Ok(())),
        ]
    );
}

#[test]
fn forgetting_a_device_that_is_not_saved_stops_no_link() {
    let bench = Bench::new(&[KEYBOARD, MOUSE]);
    let mut fake = bench.fake();
    let mut coordinator = pin!(run(&mut fake));
    bench.settle(&mut coordinator);
    bench.connected(&mut coordinator, 0, KEYBOARD);
    bench.connected(&mut coordinator, 1, MOUSE);
    bench.take();

    let forget = Command::Forget {
        id: 4,
        address: REMOTE,
    };
    bench.ui(&mut coordinator, forget);
    assert_eq!(bench.take(), [result(4, Err(ErrorTag::ManagementFailed))]);

    // The next transaction still gets a token of its own.
    bench.ui(&mut coordinator, Command::FactoryReset { id: 5 });
    let quiesce = |slot| Call::Send(slot, SlotCommand::Quiesce(2));
    assert_eq!(bench.take(), [quiesce(0), quiesce(1)]);
}

#[test]
fn a_factory_reset_quiesces_both_slots_and_forgets_the_scan_results() {
    let bench = Bench::new(&[KEYBOARD]);
    let mut fake = bench.fake();
    let mut coordinator = pin!(run(&mut fake));
    bench.settle(&mut coordinator);
    bench.connected(&mut coordinator, 0, KEYBOARD_NOW);
    bench.ui(&mut coordinator, Command::StartScan);
    bench.state.scans.borrow_mut().push_back(Some(vec![MOUSE]));
    bench.settle(&mut coordinator);
    bench.take();

    bench.ui(&mut coordinator, Command::FactoryReset { id: 9 });
    let quiesce = |slot| Call::Send(slot, SlotCommand::Quiesce(1));
    assert_eq!(bench.take(), [quiesce(0), quiesce(1)]);
    // The keyboard's keys were saved when its link came up, so the store
    // removes them, not the acknowledgement.
    bench.report(&mut coordinator, quiesced(0, 1));
    assert!(bench.take().is_empty());
    bench.report(&mut coordinator, quiesced(1, 1));
    assert_eq!(
        bench.take(),
        [
            Call::Forget(None),
            Call::Emit(Event::Disconnected),
            result(9, Ok(())),
        ]
    );
    assert!(bench.state.saved.borrow().is_empty());

    bench.ui(&mut coordinator, Command::Connect(0));
    let failed = Call::Emit(Event::Error(ErrorTag::ConnectFailed));
    assert_eq!(bench.take(), [failed]);
}

#[test]
fn a_failed_store_write_reports_storage_failed_and_keeps_the_device() {
    let bench = Bench::new(&[KEYBOARD, MOUSE]);
    let mut fake = bench.fake();
    let mut coordinator = pin!(run(&mut fake));
    bench.settle(&mut coordinator);
    bench.connected(&mut coordinator, 0, KEYBOARD);
    bench.connected(&mut coordinator, 1, MOUSE);
    bench.take();
    bench.state.writable.set(false);

    let forget = Command::Forget {
        id: 3,
        address: MOUSE,
    };
    bench.ui(&mut coordinator, forget);
    bench.report(&mut coordinator, quiesced(1, 1));
    // The mouse's link is gone and its slot free, but it stays saved with
    // its keys: selecting it again reconnects without pairing.
    assert_eq!(
        bench.take(),
        [
            Call::Send(1, SlotCommand::Quiesce(1)),
            Call::Forget(Some(MOUSE)),
            Call::Emit(shows(&["Keyboard"])),
            result(3, Err(ErrorTag::StorageFailed)),
        ]
    );
    assert_eq!(*bench.state.saved.borrow(), [KEYBOARD, MOUSE]);
}
