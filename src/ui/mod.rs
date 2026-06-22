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
pub mod ui_logic;

/// `Screen` and `ButtonEvent` live in the pure `ui_logic` core (shared with the
/// host-tested logic) and are re-exported here for the embedded tasks.
pub use ui_logic::{ButtonEvent, Screen};
