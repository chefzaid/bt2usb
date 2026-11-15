//! The status LED task: drives P0.13 (active low) with the pattern the main
//! loop publishes. What each pattern means is in the pure
//! [`status_led_logic`](super::status_led_logic).

use super::status_led_logic::{lit, next_change_ms, LedPattern};
use embassy_futures::select::{select, Either};
use embassy_nrf::gpio::{AnyPin, Level, Output, OutputDrive};
use embassy_nrf::Peri;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Instant, Timer};

static PATTERN: Signal<CriticalSectionRawMutex, LedPattern> = Signal::new();

/// Hand the LED task the pattern to show. Cheap; the task ignores a repeat.
pub fn publish(pattern: LedPattern) {
    PATTERN.signal(pattern);
}

/// The LED task: the sole owner of the LED pin. It starts with the heartbeat,
/// so a board shows it runs before the main loop publishes anything.
#[embassy_executor::task]
pub async fn task(pin: Peri<'static, AnyPin>) -> ! {
    // Active low: a high pin is a dark LED.
    let mut led = Output::new(pin, Level::High, OutputDrive::Standard);
    let mut pattern = LedPattern::Heartbeat;
    loop {
        let now = Instant::now().as_millis();
        led.set_level(if lit(pattern, now) {
            Level::Low
        } else {
            Level::High
        });
        pattern = match next_change_ms(pattern, now) {
            Some(edge) => match select(PATTERN.wait(), Timer::at(Instant::from_millis(edge))).await
            {
                Either::First(new) => new,
                Either::Second(()) => pattern,
            },
            None => PATTERN.wait().await,
        };
    }
}
