//! Pure power-management policy (hardware-free, host-tested).
//!
//! The timing source and the activity-tracking glue live in [`crate::power`];
//! the *decisions* — what power state a given amount of inactivity implies, and
//! whether the screen should be on — live here so they can be unit-tested on the
//! host.

/// Coarse device power state, derived from inactivity and link state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum PowerState {
    /// Normal operation — BLE active, USB active.
    Active,
    /// Idle — no recent HID/UI activity, but connections are maintained.
    Idle,
    /// Low power — bus suspended (PC asleep) or long idle with no BLE link.
    LowPower,
}

/// Decide the power state from inactivity and link state.
///
/// - `LowPower` when the USB bus is suspended (the PC is asleep — the only
///   meaningful deep-idle signal for a bus-powered device), or after a long idle
///   with no BLE link to keep alive.
/// - `Idle` after `idle_timeout_secs` of inactivity.
/// - `Active` otherwise.
pub fn next_power_state(
    elapsed_secs: u64,
    usb_suspended: bool,
    ble_connected: bool,
    idle_timeout_secs: u64,
) -> PowerState {
    if usb_suspended || (elapsed_secs > idle_timeout_secs * 2 && !ble_connected) {
        PowerState::LowPower
    } else if elapsed_secs > idle_timeout_secs {
        PowerState::Idle
    } else {
        PowerState::Active
    }
}

/// Decide whether screen should be on based on base power state and inactivity policy.
pub fn screen_should_be_on(
    base_display_on: bool,
    auto_off_enabled: bool,
    idle_secs: u64,
    auto_off_timeout_secs: u64,
) -> bool {
    if !base_display_on {
        return false;
    }

    if auto_off_enabled && idle_secs >= auto_off_timeout_secs {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDLE: u64 = 60;

    #[test]
    fn active_while_recently_used() {
        assert_eq!(next_power_state(0, false, true, IDLE), PowerState::Active);
        assert_eq!(
            next_power_state(IDLE, false, true, IDLE),
            PowerState::Active
        );
    }

    #[test]
    fn idle_after_timeout() {
        assert_eq!(
            next_power_state(IDLE + 1, false, true, IDLE),
            PowerState::Idle
        );
    }

    #[test]
    fn usb_suspend_forces_low_power_immediately() {
        // PC asleep → deepest idle regardless of elapsed time or BLE link.
        assert_eq!(next_power_state(0, true, true, IDLE), PowerState::LowPower);
    }

    #[test]
    fn long_idle_without_ble_is_low_power() {
        // No link to keep alive and idle for >2× the timeout.
        assert_eq!(
            next_power_state(IDLE * 2 + 1, false, false, IDLE),
            PowerState::LowPower
        );
        // ...but with a BLE link we stay merely Idle (keep the link responsive).
        assert_eq!(
            next_power_state(IDLE * 2 + 1, false, true, IDLE),
            PowerState::Idle
        );
    }
}
