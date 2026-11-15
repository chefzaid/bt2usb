//! What the status LED shows, as a pure function of the bridge's state and the
//! time.
//!
//! The LED is on P0.13, active low: LED1 on the nRF52840-DK and D1 on the
//! rev A board (docs/hardware.md). It tells a user who cannot read the OLED
//! (no module fitted, unplugged, or on the wrong supply jumpers) that the
//! firmware runs and whether a peripheral is linked:
//!
//! | State | LED |
//! | --- | --- |
//! | USB bus suspended | Off, to stay within the suspend current |
//! | At least one BLE link up | On |
//! | No BLE link | A short flash once per period (heartbeat) |
//!
//! The firmware's `ui::status_led` task drives the pin from these answers.

use crate::config::{STATUS_LED_FLASH_MS, STATUS_LED_PERIOD_MS};

// A heartbeat needs a flash shorter than its period, and a dark spell.
const _: () = assert!(STATUS_LED_FLASH_MS > 0 && STATUS_LED_FLASH_MS < STATUS_LED_PERIOD_MS);

/// What the status LED shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "embedded", derive(defmt::Format))]
pub enum LedPattern {
    /// Dark: the USB bus is suspended.
    Off,
    /// One short flash per period: running, no BLE link.
    Heartbeat,
    /// Steadily lit: at least one BLE link is up.
    On,
}

/// The pattern for the bridge's state. A suspended bus wins over a link,
/// because the host has asked the device to draw almost nothing.
pub fn pattern(usb_suspended: bool, ble_connected: bool) -> LedPattern {
    match (usb_suspended, ble_connected) {
        (true, _) => LedPattern::Off,
        (false, true) => LedPattern::On,
        (false, false) => LedPattern::Heartbeat,
    }
}

/// Whether the LED is lit at `now_ms` (any monotonic millisecond clock).
/// A heartbeat is lit for the first [`STATUS_LED_FLASH_MS`] of each
/// [`STATUS_LED_PERIOD_MS`].
pub fn lit(pattern: LedPattern, now_ms: u64) -> bool {
    match pattern {
        LedPattern::Off => false,
        LedPattern::On => true,
        LedPattern::Heartbeat => now_ms % STATUS_LED_PERIOD_MS < STATUS_LED_FLASH_MS,
    }
}

/// The next time after `now_ms` at which [`lit`] changes for `pattern`, or
/// `None` for a steady pattern.
pub fn next_change_ms(pattern: LedPattern, now_ms: u64) -> Option<u64> {
    match pattern {
        LedPattern::Off | LedPattern::On => None,
        LedPattern::Heartbeat => {
            let start = now_ms - now_ms % STATUS_LED_PERIOD_MS;
            let edge = if now_ms - start < STATUS_LED_FLASH_MS {
                start + STATUS_LED_FLASH_MS
            } else {
                start + STATUS_LED_PERIOD_MS
            };
            Some(edge)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suspend_wins_over_a_link() {
        assert_eq!(pattern(true, true), LedPattern::Off);
        assert_eq!(pattern(true, false), LedPattern::Off);
        assert_eq!(pattern(false, true), LedPattern::On);
        assert_eq!(pattern(false, false), LedPattern::Heartbeat);
    }

    #[test]
    fn steady_patterns_never_change() {
        for now in [
            0,
            1,
            STATUS_LED_FLASH_MS,
            STATUS_LED_PERIOD_MS * 7 + 3,
            u64::MAX,
        ] {
            assert!(!lit(LedPattern::Off, now));
            assert!(lit(LedPattern::On, now));
            assert_eq!(next_change_ms(LedPattern::Off, now), None);
            assert_eq!(next_change_ms(LedPattern::On, now), None);
        }
    }

    #[test]
    fn heartbeat_flashes_once_per_period() {
        let p = LedPattern::Heartbeat;
        let lit_ms = (0..STATUS_LED_PERIOD_MS * 3).filter(|&t| lit(p, t)).count() as u64;
        assert_eq!(lit_ms, STATUS_LED_FLASH_MS * 3);
        assert!(lit(p, 0));
        assert!(lit(p, STATUS_LED_FLASH_MS - 1));
        assert!(!lit(p, STATUS_LED_FLASH_MS));
        assert!(!lit(p, STATUS_LED_PERIOD_MS - 1));
        assert!(lit(p, STATUS_LED_PERIOD_MS));
    }

    #[test]
    fn heartbeat_edges_are_where_lit_changes() {
        let p = LedPattern::Heartbeat;
        assert_eq!(next_change_ms(p, 0), Some(STATUS_LED_FLASH_MS));
        assert_eq!(
            next_change_ms(p, STATUS_LED_FLASH_MS),
            Some(STATUS_LED_PERIOD_MS)
        );
        assert_eq!(
            next_change_ms(p, STATUS_LED_PERIOD_MS - 1),
            Some(STATUS_LED_PERIOD_MS)
        );
        // Every reported edge is in the future and flips the LED, and nothing
        // changes before it.
        for now in (0..STATUS_LED_PERIOD_MS * 2).step_by(7) {
            let edge = next_change_ms(p, now).unwrap();
            assert!(edge > now);
            assert_ne!(lit(p, edge), lit(p, edge - 1));
            assert!((now..edge).all(|t| lit(p, t) == lit(p, now)));
        }
    }
}
