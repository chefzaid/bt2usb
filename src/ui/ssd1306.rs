//! In-tree async driver for the 128×64 SSD1306 OLED on I²C (ADR 0030).
//!
//! It replaces the `ssd1306` crate, whose pinned `maybe-async-cfg =0.2.4`
//! pulled in the unmaintained `proc-macro-error` (RUSTSEC-2024-0370). The
//! driver covers exactly what the bridge uses: one panel geometry (128×64,
//! the module mounted with segment remap and the remapped COM scan), one
//! addressing mode (horizontal), a RAM framebuffer that `embedded-graphics`
//! draws into, a full-frame flush, and display on/off. The command values are
//! the ones `ssd1306` 0.10.0 sent for `DisplaySize128x64`, `Rotate0`, and the
//! default brightness, so the panel is configured as before; ADR 0030 lists
//! them byte for byte against the datasheet.
//!
//! I²C framing (SSD1306 datasheet, section 8.1.5.2): every write starts with
//! a control byte. `0x00` (Co=0, D/C#=0) makes every following byte of that
//! write a command byte; `0x40` (Co=0, D/C#=1) makes every following byte
//! display data. The driver therefore needs only three kinds of write: the
//! configuration as one command stream, the address window as another, and
//! the whole framebuffer as one data stream.
//!
//! Each write is a single I²C `write` operation (one START, the address, the
//! bytes, one STOP), so the display task's `StopSafeI2c` sees only
//! write-only `LASTTX_STOP` transfers, as its error handling assumes. The
//! framebuffer is stored with its data control byte in front, so a flush is
//! one contiguous RAM buffer that EasyDMA reads in place (1,025 bytes, well
//! under the nRF52840's 65,535-byte `MAXCNT`); the command streams are
//! constants short enough for the TWIM driver's 64-byte RAM copy buffer.
//!
//! The pixel layout and the byte sequences are pure and host-tested
//! (`ssd1306_tests.rs`), including a small model of the controller's RAM
//! addressing that checks a flush lands every pixel where the panel shows it.

/// 7-bit I²C address of the module (SA0 low). A module strapped to 0x3D
/// does not answer; the self-test reports it.
pub const ADDRESS: u8 = 0x3C;

/// Panel width in pixels (segments).
pub const WIDTH: u32 = 128;
/// Panel height in pixels (COM lines).
pub const HEIGHT: u32 = 64;
/// Bytes of display RAM: one byte per column per page, bit 0 at the top.
pub const FRAME_BYTES: usize = 1024;

/// Control byte: every following byte of the write is a command.
pub const CONTROL_COMMANDS: u8 = 0x00;
/// Control byte: every following byte of the write is display data.
pub const CONTROL_DATA: u8 = 0x40;

/// Display off (sleep). RAM and settings are kept.
pub const DISPLAY_OFF: u8 = 0xAE;
/// Display on.
pub const DISPLAY_ON: u8 = 0xAF;

/// The configuration, as one command stream, leaving the panel dark.
///
/// Same values, in the same order, as `ssd1306` 0.10.0's
/// `init_with_addr_mode(Horizontal)` for a 128×64 panel at `Rotate0` with
/// `Brightness::NORMAL`, except that it does not end with display on: the
/// caller clears RAM and lights the panel only with a whole frame in it.
#[rustfmt::skip]
pub const INIT: [u8; 26] = [
    CONTROL_COMMANDS,
    DISPLAY_OFF,
    0xD5, 0x80, // display clock: oscillator 8 (reset value), divide by 1
    0xA8, 0x3F, // multiplex ratio 64
    0xD3, 0x00, // display offset 0
    0x40,       // display start line 0
    0x8D, 0x14, // charge pump on (the module has no external VCC)
    0x20, 0x00, // horizontal addressing
    0xDA, 0x12, // COM pins: alternative configuration, no left/right remap
    0xA1,       // segment remap: column 127 drives SEG0
    0xC8,       // COM scan from COM63 to COM0
    0xD9, 0x21, // pre-charge: phase 1 = 1, phase 2 = 2 DCLK
    0x81, 0x5F, // contrast 0x5F
    0xDB, 0x40, // VCOMH deselect level (the value ssd1306 calls Auto)
    0xA4,       // show RAM content, not entire display on
    0xA6,       // normal, not inverse
    0x2E,       // scrolling off
];

/// The address window covering the whole panel, as one command stream:
/// columns 0..=127 and pages 0..=7. Sent before every flush, so the RAM
/// pointer starts at column 0, page 0 even if an earlier transfer was cut
/// short by STOP or a NACK.
pub const FULL_WINDOW: [u8; 7] = [CONTROL_COMMANDS, 0x21, 0x00, 0x7F, 0x22, 0x00, 0x07];

/// The command stream that turns the panel on or off.
pub const fn display_on_command(on: bool) -> [u8; 2] {
    [CONTROL_COMMANDS, if on { DISPLAY_ON } else { DISPLAY_OFF }]
}

/// The framebuffer, stored as the data write that sends it: the data control
/// byte, then the 1,024 bytes of display RAM in horizontal-addressing order
/// (page 0 columns 0..=127, then page 1, and so on).
#[derive(Clone)]
pub struct Framebuffer {
    transfer: [u8; 1 + FRAME_BYTES],
}

impl Default for Framebuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Framebuffer {
    /// A blank (all dark) frame.
    pub const fn new() -> Self {
        let mut transfer = [0; 1 + FRAME_BYTES];
        transfer[0] = CONTROL_DATA;
        Self { transfer }
    }

    /// Turn every pixel off.
    pub fn clear(&mut self) {
        if let Some(pixels) = self.transfer.get_mut(1..) {
            pixels.fill(0);
        }
    }

    /// Turn the pixel at (`x`, `y`) on or off, (0, 0) being the top left as
    /// the module is mounted. Coordinates off the panel are ignored, as
    /// `embedded-graphics` expects of a draw target.
    pub fn set_pixel(&mut self, x: u32, y: u32, on: bool) {
        if x >= WIDTH || y >= HEIGHT {
            return;
        }
        // Lossless: x < 128 and y < 64.
        let index = 1 + (y as usize / 8) * WIDTH as usize + x as usize;
        let mask = 1u8 << (y % 8);
        if let Some(byte) = self.transfer.get_mut(index) {
            if on {
                *byte |= mask;
            } else {
                *byte &= !mask;
            }
        }
    }

    /// Whether the pixel at (`x`, `y`) is on; false off the panel.
    #[cfg(test)]
    pub fn pixel(&self, x: u32, y: u32) -> bool {
        if x >= WIDTH || y >= HEIGHT {
            return false;
        }
        let index = 1 + (y as usize / 8) * WIDTH as usize + x as usize;
        self.transfer
            .get(index)
            .is_some_and(|byte| byte & (1 << (y % 8)) != 0)
    }

    /// The display RAM image, without the control byte.
    #[cfg(test)]
    pub fn ram(&self) -> &[u8] {
        self.transfer.get(1..).unwrap_or(&[])
    }

    /// The data write that sends the whole frame: the control byte and RAM.
    pub fn transfer(&self) -> &[u8] {
        &self.transfer
    }
}

#[cfg(any(test, feature = "embedded", feature = "sim"))]
mod graphics {
    use super::{Framebuffer, HEIGHT, WIDTH};
    use embedded_graphics::draw_target::DrawTarget;
    use embedded_graphics::pixelcolor::BinaryColor;
    use embedded_graphics::prelude::{OriginDimensions, Pixel, Size};

    impl OriginDimensions for Framebuffer {
        fn size(&self) -> Size {
            Size::new(WIDTH, HEIGHT)
        }
    }

    impl DrawTarget for Framebuffer {
        type Color = BinaryColor;
        type Error = core::convert::Infallible;

        fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Self::Color>>,
        {
            for Pixel(point, color) in pixels {
                // Negative coordinates are off the panel.
                if let (Ok(x), Ok(y)) = (u32::try_from(point.x), u32::try_from(point.y)) {
                    self.set_pixel(x, y, color.is_on());
                }
            }
            Ok(())
        }
    }
}

/// The panel on an async I²C bus, with its framebuffer.
///
/// Every method is one or more complete writes; an error is the bus error of
/// the first write that failed, after which nothing more is sent. The panel
/// state after an error is unknown, so the caller runs [`Ssd1306::init`]
/// again before the next frame (the display task's recovery does).
#[cfg(any(test, feature = "embedded", feature = "sim"))]
pub struct Ssd1306<I2C> {
    i2c: I2C,
    frame: Framebuffer,
}

#[cfg(any(test, feature = "embedded", feature = "sim"))]
impl<I2C: embedded_hal_async::i2c::I2c> Ssd1306<I2C> {
    /// Take the bus. Sends nothing.
    pub const fn new(i2c: I2C) -> Self {
        Self {
            i2c,
            frame: Framebuffer::new(),
        }
    }

    /// Configure the panel and clear its RAM, leaving it dark.
    ///
    /// A panel powers up with random RAM, and a panel that lost its bus
    /// mid-frame holds a torn one, so RAM is cleared before anything lights
    /// it; the caller turns it on with its first whole frame. The first
    /// command turns the panel off, so a re-initialization never shows the
    /// clear either. Clears the framebuffer too.
    pub async fn init(&mut self) -> Result<(), I2C::Error> {
        self.i2c.write(ADDRESS, &INIT).await?;
        self.frame.clear();
        self.flush().await
    }

    /// The framebuffer, to draw into; [`Ssd1306::flush`] sends it.
    pub fn frame(&mut self) -> &mut Framebuffer {
        &mut self.frame
    }

    /// Send the whole framebuffer: the full-panel window, then the frame as
    /// one data write (~93 ms at 100 kHz).
    pub async fn flush(&mut self) -> Result<(), I2C::Error> {
        self.i2c.write(ADDRESS, &FULL_WINDOW).await?;
        self.i2c.write(ADDRESS, self.frame.transfer()).await
    }

    /// Turn the panel on or off; RAM and settings are kept either way.
    pub async fn set_display_on(&mut self, on: bool) -> Result<(), I2C::Error> {
        self.i2c.write(ADDRESS, &display_on_command(on)).await
    }

    /// Give the bus back.
    #[cfg(test)]
    pub fn release(self) -> I2C {
        self.i2c
    }
}

#[cfg(test)]
#[path = "ssd1306_tests.rs"]
mod tests;
