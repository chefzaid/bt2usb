//! User interface subsystem - OLED display + physical buttons.
//!
//! The main loop runs the [`controller`], whose state machine reacts to
//! button presses and BLE events; the display task renders the latest view on
//! the SSD1306 OLED.
//!
//! ## Components
//!
//! - **Controller and view model**: `controller`, `ui_logic`, and
//!   `input_logic` (pure, host-tested)
//! - **Display**: SSD1306 128×64 OLED via I²C (`display`, which draws the
//!   lines `layout` gives each screen, with the retry policy in
//!   `display_logic`)
//! - **Buttons**: 3 tactile switches with debouncing (UP, DOWN, SELECT)

pub mod buttons;
pub mod controller;
pub mod display;
pub mod display_logic;
pub mod input_logic;
pub mod layout;
pub mod ui_logic;

/// `ButtonEvent` lives in the pure `ui_logic` core (shared with the
/// host-tested logic) and is re-exported here for the button tasks.
pub use ui_logic::ButtonEvent;
