//! SSD1306 OLED display wrapper.
//!
//! Rendering uses **async** I2C (`ssd1306` 0.10's `Ssd1306Async`): a full
//! 128×64 flush is a ~1 KB transfer, and `.await`ing it lets the cooperative
//! executor run other tasks during the DMA instead of stalling. Drawing into the
//! framebuffer (`clear_buffer`, `Text::draw`) stays synchronous — only the I2C
//! flushes/commands are async. Redraws only happen on UI events
//! (connect/scan/button), never on the keystroke→USB hot path.

use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoTextStyleBuilder;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use ssd1306::mode::BufferedGraphicsModeAsync;
use ssd1306::prelude::*;
use ssd1306::{I2CDisplayInterface, Ssd1306Async};

/// Type alias for the concrete async display driver.
///
/// Generic over the I²C implementation so callers pass in their HAL's
/// async I²C peripheral.
pub type Display<I2C> = Ssd1306Async<
    I2CInterface<I2C>,
    DisplaySize128x64,
    BufferedGraphicsModeAsync<DisplaySize128x64>,
>;

/// Initialise the SSD1306 display and clear the screen.
pub async fn init<I2C>(i2c: I2C) -> Display<I2C>
where
    I2C: embedded_hal_async::i2c::I2c,
{
    let interface = I2CDisplayInterface::new(i2c);
    let mut display = Ssd1306Async::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();
    let _ = display.init().await;
    display.clear_buffer();
    let _ = display.flush().await;
    display
}

fn text_style() -> embedded_graphics::mono_font::MonoTextStyle<'static, BinaryColor> {
    MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(BinaryColor::On)
        .build()
}

/// Render the Home screen.
pub async fn draw_home<I2C>(display: &mut Display<I2C>, connected: bool, device_name: &str)
where
    I2C: embedded_hal_async::i2c::I2c,
{
    display.clear_buffer();

    let _ = Text::new("bt2usb", Point::new(0, 10), text_style()).draw(display);

    let status = if connected { "Connected" } else { "Idle" };
    let _ = Text::new(status, Point::new(0, 24), text_style()).draw(display);

    if connected && !device_name.is_empty() {
        let _ = Text::new(device_name, Point::new(0, 38), text_style()).draw(display);
    } else {
        let _ = Text::new("Press SELECT to scan", Point::new(0, 38), text_style()).draw(display);
    }

    let _ = display.flush().await;
}

/// Render the Scanning screen with a simple progress indicator.
pub async fn draw_scanning<I2C>(display: &mut Display<I2C>, dots: u8)
where
    I2C: embedded_hal_async::i2c::I2c,
{
    display.clear_buffer();

    let _ = Text::new("Scanning", Point::new(0, 10), text_style()).draw(display);

    // Animated dots: "." / ".." / "..."
    let dot_str = match dots % 4 {
        0 => "",
        1 => ".",
        2 => "..",
        _ => "...",
    };
    let _ = Text::new(dot_str, Point::new(54, 10), text_style()).draw(display);

    let _ = Text::new("Please wait...", Point::new(0, 30), text_style()).draw(display);

    let _ = display.flush().await;
}

/// Render the discovered-device list with current selection.
pub async fn draw_device_list<I2C>(
    display: &mut Display<I2C>,
    devices: &[heapless::String<32>],
    selected: usize,
) where
    I2C: embedded_hal_async::i2c::I2c,
{
    display.clear_buffer();

    let _ = Text::new("Select device", Point::new(0, 10), text_style()).draw(display);

    for (row, name) in devices.iter().take(4).enumerate() {
        let marker = if row == selected { ">" } else { " " };
        let mut line: heapless::String<36> = heapless::String::new();
        let _ = line.push_str(marker);
        let _ = line.push_str(" ");
        let _ = line.push_str(name.as_str());
        let y = 24 + (row as i32 * 10);
        let _ = Text::new(line.as_str(), Point::new(0, y), text_style()).draw(display);
    }

    let _ = display.flush().await;
}

pub async fn draw_connected<I2C>(display: &mut Display<I2C>, device_name: &str)
where
    I2C: embedded_hal_async::i2c::I2c,
{
    display.clear_buffer();

    let _ = Text::new("Connected", Point::new(0, 10), text_style()).draw(display);
    let _ = Text::new(device_name, Point::new(0, 24), text_style()).draw(display);
    let _ = Text::new("SEL:add  DOWN:disc", Point::new(0, 38), text_style()).draw(display);
    let _ = Text::new("HID active", Point::new(0, 52), text_style()).draw(display);

    let _ = display.flush().await;
}

/// Render a transient error message.
pub async fn draw_error<I2C>(display: &mut Display<I2C>, message: &str)
where
    I2C: embedded_hal_async::i2c::I2c,
{
    display.clear_buffer();

    let _ = Text::new("ERROR", Point::new(0, 10), text_style()).draw(display);
    let _ = Text::new(message, Point::new(0, 30), text_style()).draw(display);

    let _ = display.flush().await;
}

/// Turn the OLED panel on or off at the hardware level.
///
/// When off, the SSD1306 stops driving the OLED pixels, reducing power
/// consumption to near zero.  The display buffer is preserved so the
/// screen content reappears immediately when turned back on.
pub async fn set_power<I2C>(display: &mut Display<I2C>, on: bool)
where
    I2C: embedded_hal_async::i2c::I2c,
{
    let _ = display.set_display_on(on).await;
}
