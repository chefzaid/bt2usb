//! OLED rendering in an isolated task with latest-frame delivery and recovery.
//!
//! I2C errors never block the UI/USB tasks. Failed operations trigger a fresh
//! initialization with capped exponential backoff. A deadline requests STOP,
//! but retains the DMA future until it completes: the pinned TWIM driver is not
//! cancellation-safe. [`StopSafeI2c`] likewise holds a NACK/overrun error until
//! STOPPED before the retry. An electrically stuck bus can leave this task degraded;
//! the rest of the bridge continues and the latest requested frame is retained.

use super::display_logic::Recovery;
use super::ui_logic::{Screen, UiState};
use defmt::{info, warn};
use embassy_futures::select::{select, Either};
use embassy_nrf::peripherals::TWISPI0;
use embassy_nrf::twim;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use ssd1306::mode::BufferedGraphicsModeAsync;
use ssd1306::prelude::*;
use ssd1306::{I2CDisplayInterface, Ssd1306Async};

pub type Display<I2C> = Ssd1306Async<
    I2CInterface<I2C>,
    DisplaySize128x64,
    BufferedGraphicsModeAsync<DisplaySize128x64>,
>;

#[derive(Clone, Copy, Debug)]
pub struct DisplayFault;

#[derive(Clone, PartialEq, Eq)]
struct Frame {
    state: UiState,
    powered_on: bool,
}

static FRAMES: Signal<CriticalSectionRawMutex, Frame> = Signal::new();

/// Never waits for hardware or a full queue; the display consumes the latest view.
pub fn publish(state: &UiState, powered_on: bool) {
    FRAMES.signal(Frame {
        state: state.clone(),
        powered_on,
    });
}

fn new<I2C: embedded_hal_async::i2c::I2c>(i2c: I2C) -> Display<I2C> {
    Ssd1306Async::new(
        I2CDisplayInterface::new(i2c),
        DisplaySize128x64,
        DisplayRotation::Rotate0,
    )
    .into_buffered_graphics_mode()
}

/// Request that the exclusively owned display TWIM peripheral stop its DMA.
/// Nordic requires RESUME before STOP when a transfer is suspended. We retain
/// all transfer buffers until the driver observes completion; never cancel a
/// pending DMA future or disable the peripheral before STOPPED.
pub fn request_bus_stop() {
    let peripheral = embassy_nrf::pac::TWIM0;
    peripheral.tasks_resume().write_value(1);
    peripheral.tasks_stop().write_value(1);
}

/// TWIM0 wrapper whose error returns follow the end of DMA.
///
/// On NACK/overrun the pinned driver triggers STOP and returns before STOPPED.
/// display-interface sends from a buffer inside its own future, so an early
/// error would release that buffer during the stop sequence, and the next
/// transfer would clear the pending STOPPED event and could finish early.
pub struct StopSafeI2c<'d>(twim::Twim<'d, TWISPI0>);

impl<'d> StopSafeI2c<'d> {
    pub fn new(twim: twim::Twim<'d, TWISPI0>) -> Self {
        Self(twim)
    }
}

impl embedded_hal_async::i2c::ErrorType for StopSafeI2c<'_> {
    type Error = twim::Error;
}

impl embedded_hal_async::i2c::I2c for StopSafeI2c<'_> {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [embedded_hal_async::i2c::Operation<'_>],
    ) -> Result<(), Self::Error> {
        let result =
            embedded_hal_async::i2c::I2c::transaction(&mut self.0, address, operations).await;
        // Only these come from the driver's error branch, which returns right
        // after STOP. For write-only SSD1306 traffic (LASTTX_STOP, never a
        // suspended write-read), other errors occur before a start or after STOPPED.
        if let Err(twim::Error::AddressNack | twim::Error::DataNack | twim::Error::Overrun) = result
        {
            wait_stopped().await;
        }
        result
    }
}

/// Wait for EVENTS_STOPPED, which the driver leaves set on its error path and
/// clears only when the next transfer starts. A bus held low never returns;
/// this task then stays degraded while input and USB continue.
async fn wait_stopped() {
    let peripheral = embassy_nrf::pac::TWIM0;
    let mut polls: u32 = 0;
    while peripheral.events_stopped().read() == 0 {
        Timer::after(Duration::from_micros(100)).await;
        polls = polls.wrapping_add(1);
        if polls.is_multiple_of(5_000) {
            warn!("OLED I2C error: STOP not complete; requesting again");
            request_bus_stop();
        }
    }
}

/// Diagnose a stalled operation and request STOP without invalidating DMA buffers.
/// The caller must run in a dedicated task if an electrically stuck bus must not
/// block its own work. This does not promise a bounded electrical recovery time.
pub async fn finish_or_stop<F: core::future::Future>(operation: F) -> F::Output {
    let mut operation = core::pin::pin!(operation);
    match select(operation.as_mut(), Timer::after(Duration::from_millis(500))).await {
        Either::First(result) => result,
        Either::Second(()) => {
            warn!("OLED I2C stalled; requesting STOP, display task degraded until DMA completes");
            request_bus_stop();
            operation.await
        }
    }
}

/// Standalone initialization used by the on-board self-test. Errors are explicit.
#[allow(dead_code)]
pub async fn init<I2C: embedded_hal_async::i2c::I2c>(
    i2c: I2C,
) -> Result<Display<I2C>, DisplayFault> {
    let mut display = new(i2c);
    display.init().await.map_err(|_| DisplayFault)?;
    display.clear_buffer();
    display.flush().await.map_err(|_| DisplayFault)?;
    Ok(display)
}

fn text<I2C: embedded_hal_async::i2c::I2c>(display: &mut Display<I2C>, value: &str, y: i32) {
    // Framebuffer drawing cannot fail; I2C failures are handled at flush.
    let _ = Text::new(
        value,
        Point::new(0, y),
        MonoTextStyle::new(&FONT_6X10, BinaryColor::On),
    )
    .draw(display);
}

fn draw_list<I2C: embedded_hal_async::i2c::I2c>(
    display: &mut Display<I2C>,
    names: &[heapless::String<32>],
    selected: usize,
    saved: bool,
) {
    text(
        display,
        if saved {
            "Saved devices"
        } else {
            "Select device"
        },
        10,
    );
    let count = names.len() + usize::from(saved);
    for (row, index) in super::input_logic::device_list_window(count, selected, 4).enumerate() {
        let name = names
            .get(index)
            .map(|n| n.as_str())
            .unwrap_or("Factory reset");
        let mut line = heapless::String::<36>::new();
        let _ = line.push_str(if index == selected { "> " } else { "  " });
        let _ = line.push_str(name);
        text(display, &line, 23 + row as i32 * 10);
    }
    if saved {
        text(display, "UP at first: back", 63);
    }
}

fn draw_view<I2C: embedded_hal_async::i2c::I2c>(display: &mut Display<I2C>, state: &UiState) {
    display.clear_buffer();
    match state.screen {
        Screen::Home => {
            text(display, "bt2usb / Idle", 10);
            text(display, "SELECT: scan", 30);
            text(display, "UP: saved devices", 48);
        }
        Screen::Scanning => {
            text(display, "Scanning", 10);
            text(
                display,
                match state.scan_dots % 4 {
                    0 => "",
                    1 => ".",
                    2 => "..",
                    _ => "...",
                },
                30,
            );
        }
        Screen::Connecting => {
            text(display, "Connecting...", 20);
        }
        Screen::Managing => {
            text(display, "Please wait...", 20);
        }
        Screen::DeviceList => draw_list(display, &state.devices, state.selected, false),
        Screen::SavedDevices => draw_list(display, &state.paired_names, state.selected, true),
        Screen::ConfirmForget(index) => {
            text(display, "Forget device?", 10);
            text(
                display,
                state
                    .paired_names
                    .get(index)
                    .map(|n| n.as_str())
                    .unwrap_or("Device unavailable"),
                24,
            );
            text(
                display,
                if state.selected == 0 {
                    "> Cancel"
                } else {
                    "  Cancel"
                },
                40,
            );
            text(
                display,
                if state.selected == 1 {
                    "> Forget"
                } else {
                    "  Forget"
                },
                54,
            );
        }
        Screen::ConfirmReset => {
            text(display, "Reset all pairings?", 10);
            text(display, "Disconnect all", 24);
            text(
                display,
                if state.selected == 0 {
                    "> Cancel"
                } else {
                    "  Cancel"
                },
                40,
            );
            text(
                display,
                if state.selected == 1 {
                    "> Reset"
                } else {
                    "  Reset"
                },
                54,
            );
        }
        Screen::Connected => {
            text(display, "Connected", 10);
            text(display, &state.connected_name, 24);
            text(display, "SEL:add DOWN:disc", 40);
            text(display, "UP:saved devices", 54);
        }
        Screen::Error => {
            text(display, "ERROR", 10);
            text(display, &state.message, 26);
            text(display, "SEL:retry DOWN:back", 44);
            text(display, "UP:saved devices", 58);
        }
        Screen::Notice => {
            text(display, "Complete", 10);
            text(display, &state.message, 28);
            text(display, "SELECT: back", 48);
        }
        Screen::NoReply => {
            text(display, "No reply", 10);
            text(display, &state.message, 26);
            text(display, "SELECT: back", 44);
            text(display, "UP:saved devices", 58);
        }
    }
}

#[allow(dead_code)]
pub async fn draw_home<I2C: embedded_hal_async::i2c::I2c>(
    display: &mut Display<I2C>,
    connected: bool,
    name: &str,
) -> Result<(), DisplayFault> {
    let mut state = UiState::new();
    if connected {
        state.screen = Screen::Connected;
        let _ = state.connected_name.push_str(name);
    }
    draw_view(display, &state);
    display.flush().await.map_err(|_| DisplayFault)
}

async fn render<I2C: embedded_hal_async::i2c::I2c>(
    display: &mut Display<I2C>,
    frame: &Frame,
    initialize: bool,
) -> Result<(), DisplayFault> {
    if initialize {
        display.init().await.map_err(|_| DisplayFault)?;
    }
    if frame.powered_on {
        draw_view(display, &frame.state);
        display.flush().await.map_err(|_| DisplayFault)?;
    }
    display
        .set_display_on(frame.powered_on)
        .await
        .map_err(|_| DisplayFault)
}

/// Sole owner of TWIM0 and the OLED; main only publishes snapshots.
pub async fn run<I2C: embedded_hal_async::i2c::I2c>(i2c: I2C) -> ! {
    let mut display = new(i2c);
    let mut frame = FRAMES.wait().await;
    let mut applied: Option<Frame> = None;
    let mut initialized = false;
    let mut recovery = Recovery::default();
    loop {
        if recovery.ready(Instant::now().as_millis()) && applied.as_ref() != Some(&frame) {
            match finish_or_stop(render(&mut display, &frame, !initialized)).await {
                Ok(()) => {
                    if !initialized {
                        info!("OLED initialized/recovered");
                    }
                    initialized = true;
                    recovery.recovered();
                    applied = Some(frame.clone());
                }
                Err(_) => {
                    initialized = false;
                    applied = None;
                    let wait_ms = recovery.failed(Instant::now().as_millis());
                    warn!(
                        "OLED operation failed; retry in {} ms (bridge remains active)",
                        wait_ms
                    );
                }
            }
        }
        if initialized {
            frame = FRAMES.wait().await;
        } else {
            let wait = Duration::from_millis(recovery.wait_ms(Instant::now().as_millis()));
            if let Either::First(next) = select(FRAMES.wait(), Timer::after(wait)).await {
                frame = next;
            }
        }
    }
}
