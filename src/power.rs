//! Power management module - low-power modes for battery operation.
//!
//! Implements:
//! - BLE idle mode (reduced advertising)
//! - USB suspend handling
//! - System sleep when inactive
//!
//! nRF52840 power modes:
//! - System ON: Normal operation (~3.5 mA with BLE active)
//! - System ON Idle: CPU sleeping, peripherals active (~1.5 mA)
//! - System OFF: Deep sleep, wake on GPIO/RTC (~0.3 µA)

use crate::{config, power_logic};
use defmt::info;
use embassy_time::Instant;

/// Inactivity timeout before entering low-power mode.
const IDLE_TIMEOUT_SECS: u64 = 60;

/// Power state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, defmt::Format)]
pub enum PowerState {
    /// Normal operation - BLE active, USB active.
    Active,
    /// Idle - no HID activity, but connections maintained.
    Idle,
    /// Low power - BLE idle, display off.
    LowPower,
}

/// Power manager tracks activity and manages sleep modes.
pub struct PowerManager {
    state: PowerState,
    last_activity: Instant,
    ble_connected: bool,
    usb_suspended: bool,
}

impl PowerManager {
    /// Create a new power manager.
    pub fn new() -> Self {
        Self {
            state: PowerState::Active,
            last_activity: Instant::now(),
            ble_connected: false,
            usb_suspended: false,
        }
    }

    /// Record activity (keypress, mouse move, button press).
    pub fn activity(&mut self) {
        self.last_activity = Instant::now();
        if self.state != PowerState::Active {
            info!("Power: waking from {:?}", self.state);
            self.state = PowerState::Active;
        }
    }

    /// Update BLE connection state.
    pub fn set_ble_connected(&mut self, connected: bool) {
        self.ble_connected = connected;
        if connected {
            self.activity();
        }
    }

    /// Update USB suspend state from USB bus events.
    pub fn set_usb_suspended(&mut self, suspended: bool) {
        if self.usb_suspended == suspended {
            return;
        }

        self.usb_suspended = suspended;
        info!("Power: usb_suspended={}", suspended);

        if suspended {
            let elapsed = self.last_activity.elapsed().as_secs();
            self.update_state_with_elapsed(elapsed);
        } else {
            self.activity();
        }
    }

    /// Check if display should be on.
    pub fn display_on(&self) -> bool {
        let base_display_on = matches!(self.state, PowerState::Active | PowerState::Idle);
        let idle_secs = self.last_activity.elapsed().as_secs();
        power_logic::screen_should_be_on(
            base_display_on,
            config::SCREEN_AUTO_OFF_ENABLED,
            idle_secs,
            config::SCREEN_AUTO_OFF_TIMEOUT_SECS,
        )
    }

    /// Check if we should reduce BLE activity.
    pub fn ble_low_power(&self) -> bool {
        matches!(self.state, PowerState::LowPower)
    }

    /// Get current power state.
    pub fn state(&self) -> PowerState {
        self.state
    }

    /// Periodic tick - call every ~1 second.
    pub fn tick(&mut self) {
        let elapsed = self.last_activity.elapsed().as_secs();
        self.update_state_with_elapsed(elapsed);
    }

    fn update_state_with_elapsed(&mut self, elapsed_secs: u64) {
        let new_state = if self.usb_suspended
            || (elapsed_secs > IDLE_TIMEOUT_SECS * 2 && !self.ble_connected)
        {
            PowerState::LowPower
        } else if elapsed_secs > IDLE_TIMEOUT_SECS {
            PowerState::Idle
        } else {
            PowerState::Active
        };

        if new_state != self.state {
            info!("Power: {:?} -> {:?}", self.state, new_state);
            self.state = new_state;
        }
    }
}
