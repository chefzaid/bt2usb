//! Host tests for the in-tree SSD1306 driver: the command bytes against the
//! datasheet and the previous `ssd1306` 0.10.0 driver, the I²C framing, the
//! pixel layout, and a controller RAM model that checks each flush lands
//! every pixel where the panel shows it (ADR 0030).

use super::*;
use embassy_futures::block_on;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use embedded_hal_async::i2c::{ErrorKind, ErrorType, I2c, Operation};

/// The TWIM driver's RAM copy buffer for writes from flash
/// (`ui::display::new_twim`): every constant command stream must fit it.
const TWIM_COPY_BUFFER: usize = 64;

/// I²C bus fake: records every write and can fail the `fail_at`-th one.
#[derive(Default)]
struct Bus {
    writes: Vec<Vec<u8>>,
    fail_at: Option<usize>,
}

impl ErrorType for Bus {
    type Error = ErrorKind;
}

impl I2c for Bus {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        assert_eq!(address, ADDRESS, "every transfer goes to the panel");
        assert_eq!(
            operations.len(),
            1,
            "one write per transaction (LASTTX_STOP only)"
        );
        let Operation::Write(bytes) = &operations[0] else {
            panic!("the driver never reads");
        };
        if self.fail_at == Some(self.writes.len()) {
            return Err(ErrorKind::NoAcknowledge(
                embedded_hal_async::i2c::NoAcknowledgeSource::Data,
            ));
        }
        self.writes.push(bytes.to_vec());
        Ok(())
    }
}

/// The SSD1306 controller as far as this driver drives it: the control
/// bytes, the command arguments, the charge pump, display on/off, and RAM
/// writes under horizontal addressing in the column/page window (datasheet
/// 8.1.5.2, 10.1.3, 10.1.4, 10.1.5). Power-on state: page addressing, full
/// window, RAM holding noise.
struct Panel {
    ram: [u8; FRAME_BYTES],
    on: bool,
    charge_pump: bool,
    horizontal: bool,
    columns: (usize, usize),
    pages: (usize, usize),
    column: usize,
    page: usize,
    segment_remap: bool,
    com_remap: bool,
}

impl Panel {
    fn power_on() -> Self {
        Self {
            ram: [0xA5; FRAME_BYTES],
            on: false,
            charge_pump: false,
            horizontal: false,
            columns: (0, 127),
            pages: (0, 7),
            column: 0,
            page: 0,
            segment_remap: false,
            com_remap: false,
        }
    }

    fn arguments(command: u8) -> usize {
        match command {
            0x20 | 0x81 | 0x8D | 0xA8 | 0xD3 | 0xD5 | 0xD9 | 0xDA | 0xDB => 1,
            0x21 | 0x22 => 2,
            _ => 0,
        }
    }

    /// One I²C write, as the controller takes it.
    fn write(&mut self, bytes: &[u8]) {
        let (&control, rest) = bytes
            .split_first()
            .expect("a write starts with a control byte");
        match control {
            CONTROL_COMMANDS => self.commands(rest),
            CONTROL_DATA => rest.iter().for_each(|&b| self.data(b)),
            other => panic!("control byte {other:#04x} is not a Co=0 stream"),
        }
    }

    fn commands(&mut self, mut bytes: &[u8]) {
        while let Some((&command, rest)) = bytes.split_first() {
            let count = Self::arguments(command);
            assert!(
                rest.len() >= count,
                "command {command:#04x} split across writes"
            );
            let (args, rest) = rest.split_at(count);
            bytes = rest;
            match command {
                0x20 => self.horizontal = args[0] == 0x00,
                0x21 => {
                    self.columns = (usize::from(args[0]), usize::from(args[1]));
                    self.column = self.columns.0;
                }
                0x22 => {
                    self.pages = (usize::from(args[0]), usize::from(args[1]));
                    self.page = self.pages.0;
                }
                0x8D => self.charge_pump = args[0] & 0x04 != 0,
                0xA0 | 0xA1 => self.segment_remap = command == 0xA1,
                0xAE | 0xAF => self.on = command == 0xAF,
                0xC0 | 0xC8 => self.com_remap = command == 0xC8,
                _ => {}
            }
        }
    }

    fn data(&mut self, byte: u8) {
        assert!(self.horizontal, "data before horizontal addressing");
        self.ram[self.page * 128 + self.column] = byte;
        self.column += 1;
        if self.column > self.columns.1 {
            self.column = self.columns.0;
            self.page = if self.page >= self.pages.1 {
                self.pages.0
            } else {
                self.page + 1
            };
        }
    }

    /// Whether the pixel the viewer sees at (`x`, `y`) is lit, for the module
    /// mounted as common 128x64 modules are (segment remap and remapped COM
    /// scan give an upright picture with RAM column 0, row 0 at the top left).
    fn shows(&self, x: usize, y: usize) -> bool {
        assert!(
            self.segment_remap && self.com_remap,
            "picture would be mirrored"
        );
        self.on && self.charge_pump && self.ram[(y / 8) * 128 + x] & (1 << (y % 8)) != 0
    }
}

#[test]
fn init_is_the_previous_drivers_configuration_without_lighting_the_panel() {
    // What ssd1306 0.10.0's `init()` sent for DisplaySize128x64, Rotate0, and
    // Brightness::NORMAL, one command per write, recorded from that crate
    // with a logging bus before it was removed (ADR 0030).
    let previous: [&[u8]; 17] = [
        &[0xAE],
        &[0xD5, 0x80],
        &[0xA8, 0x3F],
        &[0xD3, 0x00],
        &[0x40],
        &[0x8D, 0x14],
        &[0x20, 0x00],
        &[0xDA, 0x12],
        &[0xA1],
        &[0xC8],
        &[0xD9, 0x21],
        &[0x81, 0x5F],
        &[0xDB, 0x40],
        &[0xA4],
        &[0xA6],
        &[0x2E],
        &[0xAF],
    ];
    let mut expected = vec![CONTROL_COMMANDS];
    // The same commands as one stream, minus the final display on.
    for command in &previous[..previous.len() - 1] {
        expected.extend_from_slice(command);
    }
    assert_eq!(INIT.to_vec(), expected);
    assert!(!INIT[1..].contains(&DISPLAY_ON));
}

#[test]
fn init_configures_then_clears_ram_while_dark() {
    let mut driver = Ssd1306::new(Bus::default());
    driver.frame().set_pixel(3, 3, true);
    block_on(driver.init()).unwrap();
    let bus = driver.release();
    assert_eq!(bus.writes.len(), 3);
    assert_eq!(bus.writes[0], INIT);
    assert_eq!(bus.writes[1], FULL_WINDOW);
    assert_eq!(bus.writes[2].len(), 1 + FRAME_BYTES);
    assert_eq!(bus.writes[2][0], CONTROL_DATA);
    assert!(
        bus.writes[2][1..].iter().all(|&b| b == 0),
        "init clears the framebuffer too"
    );

    let mut panel = Panel::power_on();
    for write in &bus.writes {
        panel.write(write);
        assert!(!panel.on, "the panel never lights during init");
    }
    assert!(panel.charge_pump);
    assert!(panel.ram.iter().all(|&b| b == 0), "power-on noise cleared");
}

#[test]
fn reinit_turns_a_lit_panel_off_before_clearing() {
    let mut panel = Panel::power_on();
    panel.write(&INIT);
    panel.write(&display_on_command(true));
    assert!(panel.on);
    // The first command of INIT is display off, ahead of the clear.
    assert_eq!(INIT[1], DISPLAY_OFF);
    panel.write(&INIT);
    assert!(!panel.on);
}

#[test]
fn flush_lands_every_pixel_where_the_panel_shows_it() {
    let mut driver = Ssd1306::new(Bus::default());
    block_on(driver.init()).unwrap();
    let style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    Text::new("bt2usb / Idle", Point::new(0, 7), style)
        .draw(driver.frame())
        .unwrap();
    Text::new("UP: saved devices", Point::new(0, 61), style)
        .draw(driver.frame())
        .unwrap();
    driver.frame().set_pixel(127, 0, true);
    driver.frame().set_pixel(0, 63, true);
    let frame = driver.frame().clone();
    block_on(async {
        driver.flush().await?;
        driver.set_display_on(true).await
    })
    .unwrap();

    let mut panel = Panel::power_on();
    for write in &driver.release().writes {
        panel.write(write);
    }
    assert_eq!(&panel.ram[..], frame.ram());
    let mut lit = 0;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            assert_eq!(
                panel.shows(x as usize, y as usize),
                frame.pixel(x, y),
                "({x}, {y})"
            );
            lit += usize::from(frame.pixel(x, y));
        }
    }
    assert!(lit > 100, "the text drew pixels");
}

#[test]
fn flush_after_a_torn_transfer_starts_again_at_the_origin() {
    let mut frame = Framebuffer::new();
    for x in 0..WIDTH {
        frame.set_pixel(x, x % HEIGHT, true);
    }
    let mut panel = Panel::power_on();
    panel.write(&INIT);
    panel.write(&FULL_WINDOW);
    // A STOP or NACK cut the previous data write after 300 bytes, leaving the
    // RAM pointer mid-page.
    panel.write(&frame.transfer()[..301]);

    let mut driver = Ssd1306::new(Bus::default());
    *driver.frame() = frame.clone();
    block_on(driver.flush()).unwrap();
    for write in &driver.release().writes {
        panel.write(write);
    }
    assert_eq!(&panel.ram[..], frame.ram());
}

#[test]
fn pixels_map_to_page_column_and_bit() {
    let mut frame = Framebuffer::default();
    assert_eq!(
        frame.transfer(),
        Framebuffer::new().transfer(),
        "default is blank"
    );
    frame.set_pixel(0, 0, true);
    frame.set_pixel(5, 9, true);
    frame.set_pixel(127, 63, true);
    // Same bytes ssd1306 0.10.0's buffered mode produced for these pixels.
    assert_eq!(frame.ram()[0], 0x01);
    assert_eq!(frame.ram()[128 + 5], 0x02);
    assert_eq!(frame.ram()[1023], 0x80);
    assert_eq!(frame.ram().iter().filter(|&&b| b != 0).count(), 3);
    assert!(frame.pixel(5, 9) && !frame.pixel(5, 8));

    frame.set_pixel(5, 9, false);
    assert_eq!(frame.ram()[128 + 5], 0x00);
    // Off the panel: ignored, never wrapped onto another pixel.
    frame.set_pixel(WIDTH, 0, true);
    frame.set_pixel(0, HEIGHT, true);
    frame.set_pixel(u32::MAX, u32::MAX, true);
    Pixel(Point::new(-1, 3), BinaryColor::On)
        .draw(&mut frame)
        .unwrap();
    Pixel(Point::new(3, -1), BinaryColor::On)
        .draw(&mut frame)
        .unwrap();
    assert!(!frame.pixel(WIDTH, 0));
    assert_eq!(frame.ram().iter().filter(|&&b| b != 0).count(), 2);

    frame.clear();
    assert!(frame.ram().iter().all(|&b| b == 0));
    assert_eq!(
        frame.transfer()[0],
        CONTROL_DATA,
        "clear keeps the control byte"
    );
    assert_eq!(frame.size(), Size::new(128, 64));
}

#[test]
fn display_on_and_off_are_single_command_writes() {
    assert_eq!(display_on_command(true), [0x00, 0xAF]);
    assert_eq!(display_on_command(false), [0x00, 0xAE]);
    let mut driver = Ssd1306::new(Bus::default());
    block_on(async {
        driver.set_display_on(true).await?;
        driver.set_display_on(false).await
    })
    .unwrap();
    assert_eq!(
        driver.release().writes,
        [vec![0x00, 0xAF], vec![0x00, 0xAE]]
    );
}

#[test]
fn window_covers_the_whole_panel() {
    // Column address 0..=127, page address 0..=7 (datasheet 10.1.4, 10.1.5).
    assert_eq!(FULL_WINDOW, [0x00, 0x21, 0x00, 0x7F, 0x22, 0x00, 0x07]);
}

#[test]
fn command_streams_fit_the_twim_copy_buffer() {
    assert!(INIT.len() <= TWIM_COPY_BUFFER);
    assert!(FULL_WINDOW.len() <= TWIM_COPY_BUFFER);
    // The frame is sent from RAM in place, in one EasyDMA transfer.
    assert!(Framebuffer::new().transfer().len() <= 0xFFFF);
}

#[test]
fn a_failed_write_stops_the_sequence_and_returns_the_error() {
    for fail_at in 0..3 {
        let mut driver = Ssd1306::new(Bus {
            fail_at: Some(fail_at),
            ..Bus::default()
        });
        let result = block_on(driver.init());
        assert!(result.is_err(), "write {fail_at} failed");
        assert_eq!(
            driver.release().writes.len(),
            fail_at,
            "nothing sent after the failure"
        );
    }
    let mut driver = Ssd1306::new(Bus {
        fail_at: Some(0),
        ..Bus::default()
    });
    assert!(block_on(driver.flush()).is_err());
    assert!(
        driver.release().writes.is_empty(),
        "no frame after a failed window"
    );
}
