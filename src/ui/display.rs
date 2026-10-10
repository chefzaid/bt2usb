//! OLED rendering in an isolated task with latest-frame delivery and recovery.
//!
//! I2C errors never block the UI/USB tasks. Failed operations trigger a fresh
//! initialization with capped exponential backoff. A deadline requests STOP,
//! but retains the DMA future until it completes: the pinned TWIM driver is not
//! cancellation-safe. [`StopSafeI2c`] likewise holds a NACK/overrun error until
//! STOPPED before the retry. An electrically stuck bus can leave this task degraded;
//! the rest of the bridge continues and the latest requested frame is retained.

use super::display_logic::Recovery;
use super::layout;
use super::ui_logic::{Screen, UiState};
use defmt::{info, warn};
use embassy_futures::select::{select, Either};
use embassy_nrf::gpio::Pin as GpioPin;
use embassy_nrf::peripherals::TWISPI0;
use embassy_nrf::{interrupt, twim, Peri};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use ssd1306::command::AddrMode;
use ssd1306::mode::BufferedGraphicsModeAsync;
use ssd1306::prelude::*;
use ssd1306::{I2CDisplayInterface, Ssd1306Async};
use static_cell::StaticCell;

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

/// TWIM0 set up for the OLED on SDA and SCL, as the firmware, the self-test,
/// and the simulation all use it. Call it once: the copy buffer is a static.
pub fn new_twim(
    twim: Peri<'static, TWISPI0>,
    irq: impl interrupt::typelevel::Binding<
            interrupt::typelevel::TWISPI0,
            twim::InterruptHandler<TWISPI0>,
        > + 'static,
    sda: Peri<'static, impl GpioPin>,
    scl: Peri<'static, impl GpioPin>,
) -> twim::Twim<'static, TWISPI0> {
    let mut config = twim::Config::default();
    // Most SSD1306 modules carry their own I2C pull-ups, but a bare panel or
    // a module without them would leave the bus floating. The internal
    // pull-ups (~13 kΩ) are harmless in parallel with external ones.
    config.sda_pullup = true;
    config.scl_pullup = true;
    // embassy-nrf 0.7's Twim requires a RAM scratch buffer for writes whose
    // source isn't in RAM (e.g. flash-resident SSD1306 command sequences); the
    // framebuffer flush is already RAM-backed. This lives for the program.
    static TX_BUF: StaticCell<[u8; 64]> = StaticCell::new();
    twim::Twim::new(twim, irq, sda, scl, config, TX_BUF.init([0; 64]))
}

/// The display task: the sole owner of TWIM0 and the OLED, rendering the
/// latest frame [`publish`] hands it.
#[embassy_executor::task]
pub async fn task(twim: twim::Twim<'static, TWISPI0>) -> ! {
    run(StopSafeI2c::new(twim)).await
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
    initialize(&mut display).await?;
    display
        .set_display_on(true)
        .await
        .map_err(|_| DisplayFault)?;
    Ok(display)
}

/// Configure the panel and leave it dark with its RAM cleared. The driver's
/// `init` ends by turning the panel on, and a panel powers up with random RAM,
/// so the RAM is cleared first, while power-up still holds the panel off;
/// `init` then lights a blank panel, and turning it off again lets the caller
/// show its first frame whole. The clear costs one full frame, ~0.1 s at
/// 100 kHz, on every initialization.
async fn initialize<I2C: embedded_hal_async::i2c::I2c>(
    display: &mut Display<I2C>,
) -> Result<(), DisplayFault> {
    // Power-up selects page addressing, which ignores the draw area a flush
    // sets; `init` selects horizontal addressing again.
    display
        .set_addr_mode(AddrMode::Horizontal)
        .await
        .map_err(|_| DisplayFault)?;
    display.clear_buffer();
    display.flush().await.map_err(|_| DisplayFault)?;
    display.init().await.map_err(|_| DisplayFault)?;
    display
        .set_display_on(false)
        .await
        .map_err(|_| DisplayFault)
}

/// Draw `state`'s screen into the framebuffer: every line [`layout::lines`]
/// gives it, in the 6×10 font. Drawing cannot fail; I2C failures surface at
/// flush.
fn draw_view<I2C: embedded_hal_async::i2c::I2c>(display: &mut Display<I2C>, state: &UiState) {
    display.clear_buffer();
    for line in layout::lines(state) {
        let _ = Text::new(
            &line.text,
            Point::new(0, line.y),
            MonoTextStyle::new(&FONT_6X10, BinaryColor::On),
        )
        .draw(display);
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
        self::initialize(display).await?;
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
