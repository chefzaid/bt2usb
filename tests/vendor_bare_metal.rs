//! Host tests for the in-tree copy of the archived `bare-metal` 0.2.5 crate
//! (`vendor/bare-metal`, ADR 0031), which `cortex-m` 0.7 re-exports as
//! `cortex_m::interrupt::{CriticalSection, Mutex, Nr}`.
//!
//! The copy is the release's source unchanged, so these tests pin down what
//! the firmware relies on: the token and the mutex cost nothing, the mutex
//! gives access only with a token, and the interrupt number trait and the
//! peripheral wrapper return what they were given.

use bare_metal::{CriticalSection, Mutex, Nr, Peripheral};
use std::cell::Cell;
use std::mem::size_of;

#[test]
fn token_and_mutex_add_no_data() {
    assert_eq!(size_of::<CriticalSection>(), 0);
    assert_eq!(size_of::<Mutex<u32>>(), size_of::<u32>());
    assert_eq!(size_of::<Peripheral<u32>>(), size_of::<usize>());
}

#[test]
fn mutex_lends_its_value_to_a_critical_section_token() {
    static COUNTER: Mutex<Cell<u32>> = Mutex::new(Cell::new(7));
    fn shared<T: Sync>(_: &T) {}
    // A mutex of a `Send` value may be shared between contexts.
    shared(&COUNTER);

    // SAFETY: the token only claims a critical section; this test is the
    // only code that touches `COUNTER`, so no access can race.
    let token = unsafe { CriticalSection::new() };
    let cell = COUNTER.borrow(&token);
    assert_eq!(cell.get(), 7);
    cell.set(cell.get() + 1);
    assert_eq!(COUNTER.borrow(&token).get(), 8);
}

#[test]
fn interrupt_number_trait_returns_its_number() {
    struct Timer;
    // SAFETY: `nr` returns the same number on every call, as the trait asks.
    unsafe impl Nr for Timer {
        fn nr(&self) -> u8 {
            8
        }
    }
    assert_eq!(Timer.nr(), 8);
}

#[test]
fn peripheral_points_at_its_register_block() {
    static REGISTER: u32 = 0x1234_5678;
    let address = &raw const REGISTER as usize;
    // SAFETY: `address` is the address of a live, immutable `u32` static.
    let peripheral = unsafe { Peripheral::<u32>::new(address) };
    assert_eq!(peripheral.get() as usize, address);
    // SAFETY: as above for the token; the static is never written.
    let token = unsafe { CriticalSection::new() };
    assert_eq!(*peripheral.borrow(&token), 0x1234_5678);
}
