//! Power management — tracks activity and drives the device power state.
//!
//! This board is **bus-powered** through the monitor's USB hub, not battery
//! powered, so the aggressive low-power modes are intentionally *not* used:
//! - We keep the fast 7.5 ms BLE connection interval for low HID latency rather
//!   than relaxing it to save a few mA that wall power makes irrelevant.
//! - We never enter System-OFF: it would drop USB enumeration and the BLE links,
//!   which must stay up for the monitor hub. The Embassy executor already idles
//!   the CPU (WFE / System-ON-Idle) automatically between events.
//!
//! What this module *does* do is keep the power state — and thus the OLED
//! auto-off — accurate to real activity, **including live HID traffic**, not
//! just button presses and connect/disconnect events (see [`note_hid_activity`]).
//! The state-transition policy is the pure, host-tested [`crate::power_logic`].

use crate::config;
use crate::power_logic::{self, next_power_state};
use core::sync::atomic::{AtomicBool, Ordering};
use defmt::info;
use embassy_time::Instant;

pub use crate::power_logic::PowerState;

/// Inactivity timeout before entering low-power mode.
const IDLE_TIMEOUT_SECS: u64 = 60;

/// Set whenever a HID report flows from a BLE device to the USB host, so the
/// power manager counts real typing/mousing as activity (and keeps the OLED on)
/// even though those reports never pass through the UI loop. Drained once per
/// [`PowerManager::tick`].
static HID_ACTIVITY: AtomicBool = AtomicBool::new(false);

/// Record HID traffic as activity. Called from the BLE→USB report path
/// ([`crate::usb::hid_device::hid_writer_task`]); cheap and lock-free so it's
/// safe on the hot path.
pub fn note_hid_activity() {
    HID_ACTIVITY.store(true, Ordering::Relaxed);
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

    /// Periodic tick - call every ~1 second.
    pub fn tick(&mut self) {
        // Fold in any HID traffic since the last tick so live typing/mousing
        // counts as activity (keeps the screen on, holds the Active state).
        if HID_ACTIVITY.swap(false, Ordering::Relaxed) {
            self.activity();
        }
        let elapsed = self.last_activity.elapsed().as_secs();
        self.update_state_with_elapsed(elapsed);
    }

    fn update_state_with_elapsed(&mut self, elapsed_secs: u64) {
        let new_state = next_power_state(
            elapsed_secs,
            self.usb_suspended,
            self.ble_connected,
            IDLE_TIMEOUT_SECS,
        );

        if new_state != self.state {
            info!("Power: {:?} -> {:?}", self.state, new_state);
            self.state = new_state;
        }
    }
}
