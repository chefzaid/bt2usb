//! User interface subsystem - OLED display + physical buttons.
//!
//! The UI task maintains a state machine that reacts to button presses
//! and BLE events, rendering the current view on the SSD1306 OLED.
//!
//! ## Components
//!
//! - **Display**: SSD1306 128×64 OLED via I²C
//! - **Buttons**: 3 tactile switches with debouncing (UP, DOWN, SELECT)

pub mod buttons;
pub mod display;
pub mod input_logic;

use defmt::Format;

/// Screens (views) the UI can be in.
#[derive(Clone, Copy, PartialEq, Eq, Format)]
pub enum Screen {
    /// Idle / home - shows connection status.
    Home,
    /// Scanning for BLE devices - shows spinner/progress.
    Scanning,
    /// Device list - user picks one to connect.
    DeviceList,
    /// Connected - shows active device info.
    Connected,
    /// Error - shows a transient message.
    Error,
}

/// Physical button events (after debouncing).
///
/// Simplified to 3 buttons for a dongle use case:
///   - UP/DOWN: Navigate device list
///   - SELECT: Context-dependent action (scan, connect, disconnect)
#[derive(Clone, Copy, PartialEq, Eq, Format)]
pub enum ButtonEvent {
    Up,
    Down,
    Select,
}
