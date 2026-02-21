//! GPIO button input with async debouncing.
//!
//! Three physical buttons (active-low with internal pull-up):
//!   - UP     - navigate up in device list
//!   - DOWN   - navigate down in device list
//!   - SELECT - context-dependent: scan / connect / disconnect
//!
//! Each button is handled by an async task that waits for a GPIO edge,
//! debounces it, and sends a `ButtonEvent` to the UI channel.

use crate::config::BUTTON_DEBOUNCE_MS;
use crate::ui::ButtonEvent;
use defmt::info;
use embassy_nrf::gpio::{AnyPin, Input, Pull};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Sender;
use embassy_time::{Duration, Timer};

/// Run a single button polling loop.
///
/// Waits for the pin to go low (pressed), debounces, sends the event,
/// then waits for release before repeating.
pub async fn button_task(
    pin: AnyPin,
    event: ButtonEvent,
    tx: &Sender<'static, CriticalSectionRawMutex, ButtonEvent, 4>,
) -> ! {
    let mut btn = Input::new(pin, Pull::Up);

    loop {
        // Wait for falling edge (button press, active-low).
        btn.wait_for_falling_edge().await;

        // Debounce: wait and re-check.
        Timer::after(Duration::from_millis(BUTTON_DEBOUNCE_MS)).await;

        if btn.is_low() {
            info!("Button: {}", event);
            tx.send(event).await;

            // Wait for release to avoid repeat triggers.
            btn.wait_for_rising_edge().await;
            Timer::after(Duration::from_millis(BUTTON_DEBOUNCE_MS)).await;
        }
    }
}
