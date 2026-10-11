//! # bt2usb-sim — SoftDevice-free simulation entry point (Layer 3 / Renode)
//!
//! The real firmware can't be emulated end-to-end: the Nordic SoftDevice is a
//! closed BLE blob tied to the radio, and the nRF USBD peripheral isn't modeled
//! by emulators. This binary is the SoftDevice/USB-free variant that **does**
//! run on a simulated nRF52840 ([Renode](https://renode.io)), so we can exercise,
//! without hardware:
//!
//! - boot + the Embassy executor + the RTC time driver + the memory map,
//! - the GPIO button driver (real `ui::buttons` code, on P0.11/12/24), driven
//!   by edges injected on the simulated pins (`gpio0 OnGPIO <pin> <level>`;
//!   buttons are active-low, so `false` = pressed, `true` = released),
//! - the firmware's UI loop decisions (`ui::controller`, with `ui::ui_logic`
//!   underneath), fed by the buttons and by the simulated BLE side in
//!   [`sim_ble`], which runs the **real** host-tested coordinator reducers, scan
//!   merging, management barrier and commit, and paired-device list with its
//!   flash record codec, with `Address` substituted by a `u32` stand-in,
//! - the firmware's display task (`ui::display`) on TWIM0 (SDA=P0.26,
//!   SCL=P0.27), which renders every published view on the SSD1306 at 0x3C.
//!   Renode has neither an EasyDMA TWIM nor an SSD1306, so
//!   `renode/nrf52840_twim.cs` and `renode/ssd1306.cs` model them, and the
//!   panel model reads the screen's text back for the Robot test.
//!
//! Output is written to **UART0** (Renode's `uart0`), which Renode shows on its
//! console / analyzer with no probe or decoder. See docs/testing.md.

#![no_std]
#![no_main]
// This binary reuses shared modules (e.g. the self-test's display helpers)
// that it does not fully exercise; don't warn about the unused parts.
#![allow(dead_code)]

mod ble;
mod config;
mod diagnostics;
mod stack;
mod stack_logic;
mod ui;

// The pure parts of the paired-device store, mounted as the host library mounts
// them: inside this inline module the files resolve to `src/storage/`. The
// flash shell (`storage.rs`) needs the SoftDevice and is not part of this build.
mod storage {
    pub mod codec;
    pub mod devices;
    pub mod framing;
    pub mod record;
}

use core::fmt::Write as _;

use defmt::unwrap;
use defmt_rtt as _; // defmt global logger required by shared modules (e.g. buttons)
use panic_probe as _; // panic handler → defmt

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_nrf::gpio::AnyPin;
use embassy_nrf::peripherals::UARTE0;
use embassy_nrf::twim;
use embassy_nrf::uarte::{self, Uarte};
use embassy_nrf::{bind_interrupts, peripherals, Peri};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Timer};
use heapless::String;

use crate::ble::messages::{Command, Event};
use crate::ui::controller::UiController;
use crate::ui::ButtonEvent;

bind_interrupts!(struct Irqs {
    UARTE0 => uarte::InterruptHandler<peripherals::UARTE0>;
    TWISPI0 => twim::InterruptHandler<peripherals::TWISPI0>;
});

static BUTTON_CHANNEL: Channel<CriticalSectionRawMutex, ButtonEvent, 4> = Channel::new();

/// The real button driver. Under Renode, presses come from edges injected on
/// the simulated pins; they reach embassy-nrf's async edge wait through the
/// pin SENSE → LATCH → GPIOTE PORT chain, which the stock Renode nRF52840 GPIO
/// model lacks — hence the custom GPIO/GPIOTE models in
/// `renode/nrf52840_sense_gpio.cs`.
#[embassy_executor::task(pool_size = 3)]
async fn button_task(pin: Peri<'static, AnyPin>, event: ButtonEvent) -> ! {
    ui::buttons::button_task(pin, event, &BUTTON_CHANNEL.sender()).await
}

/// UART0, which Renode shows as the simulation's console.
pub struct Console(Uarte<'static, UARTE0>);

impl Console {
    /// Write one line. EasyDMA needs the source buffer in RAM, which a stack
    /// `String` satisfies; a line longer than the buffer is cut short.
    pub fn line(&mut self, args: core::fmt::Arguments<'_>) {
        let mut line: String<200> = String::new();
        let _ = line.write_fmt(args);
        let _ = line.push_str("\r\n");
        let _ = self.0.blocking_write(line.as_bytes());
    }
}

/// Write `bytes` (up to 128) to UART0 without the driver and wait until they
/// are out: the HardFault handler's overflow report, written after the
/// driver's task can no longer run. EasyDMA reads only RAM, so the bytes are
/// copied to the stack first; the console must already be set up.
pub fn uart_write_blocking(bytes: &[u8]) {
    let mut buffer = [0u8; 128];
    let len = bytes.len().min(buffer.len());
    let (Some(target), Some(source)) = (buffer.get_mut(..len), bytes.get(..len)) else {
        return;
    };
    target.copy_from_slice(source);
    let uarte = embassy_nrf::pac::UARTE0;
    uarte.events_endtx().write_value(0);
    uarte.txd().ptr().write_value(buffer.as_ptr() as u32);
    uarte.txd().maxcnt().write(|w| w.set_maxcnt(len as u16));
    uarte.tasks_starttx().write_value(1);
    // Bounded, so a UART that never finishes cannot hang the report forever.
    for _ in 0..1_000_000 {
        if uarte.events_endtx().read() != 0 {
            break;
        }
    }
}

/// Format a line and write it to the console.
macro_rules! slog {
    ($console:expr, $($arg:tt)*) => {
        $console.line(format_args!($($arg)*))
    };
}

// Declared after `slog!` so the macro is in scope there.
mod sim_ble;

use crate::sim_ble::{SimAddr, SimBle};

fn log_command(console: &mut Console, command: &Command<SimAddr>) {
    match command {
        Command::StartScan => slog!(console, "  cmd: StartScan"),
        Command::Connect(index) => slog!(console, "  cmd: Connect({})", index),
        Command::Disconnect => slog!(console, "  cmd: Disconnect"),
        Command::ListPaired { id } => slog!(console, "  cmd: ListPaired id={}", id),
        Command::Forget { id, address } => {
            slog!(console, "  cmd: Forget id={} addr={:#x}", id, address)
        }
        Command::FactoryReset { id } => slog!(console, "  cmd: FactoryReset id={}", id),
    }
}

fn describe(out: &mut impl core::fmt::Write, event: &Event<SimAddr>) -> core::fmt::Result {
    match event {
        Event::ScanStarted => write!(out, "ScanStarted"),
        Event::DeviceFound(device) => write!(
            out,
            "DeviceFound '{}' addr={:#x} rssi={}",
            device.name, device.address, device.rssi
        ),
        Event::ScanComplete => write!(out, "ScanComplete"),
        Event::Connected(summary) => write!(out, "Connected '{}'", summary),
        Event::Disconnected => write!(out, "Disconnected"),
        Event::Error(tag) => write!(out, "Error {:?}", tag),
        Event::PairedDevices { id, devices } => {
            write!(out, "PairedDevices id={} [", id)?;
            for (index, device) in devices.iter().enumerate() {
                let separator = if index == 0 { "" } else { ", " };
                write!(out, "{}'{}'", separator, device.name)?;
            }
            write!(out, "]")
        }
        Event::ManagementResult { id, result } => {
            write!(out, "ManagementResult id={} {:?}", id, result)
        }
    }
}

/// Hand each event to the UI controller, as the firmware's loop does when it
/// receives one, and log the screen it leads to.
fn apply(
    console: &mut Console,
    ui: &mut UiController<SimAddr>,
    events: impl IntoIterator<Item = Event<SimAddr>>,
) {
    for event in events {
        let mut text: String<120> = String::new();
        let _ = describe(&mut text, &event);
        ui.event(event);
        slog!(
            console,
            "  event: {} -> screen {:?} (selected {})",
            text,
            ui.state.screen,
            ui.state.selected
        );
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_nrf::init(Default::default());

    // UART0 for human-readable output (TX=P0.06, RX=P0.08). Renode's uart0
    // model emits the TX bytes regardless of physical pin routing.
    // embassy-nrf 0.7 reordered Uarte::new args to (uarte, rxd, txd, irq, config).
    let mut console = Console(Uarte::new(
        p.UARTE0,
        p.P0_08,
        p.P0_06,
        Irqs,
        uarte::Config::default(),
    ));

    slog!(
        console,
        "bt2usb-sim starting (SoftDevice-free Renode build)"
    );
    slog!(
        console,
        "version {}, commit {}, {} build",
        diagnostics::FIRMWARE_VERSION,
        diagnostics::SOURCE_COMMIT,
        diagnostics::BUILD_PROFILE
    );
    match stack::enable_guard() {
        Ok(guard) => slog!(
            console,
            "stack guard: {} bytes at {:#010x}..{:#010x}",
            guard.size(),
            guard.base(),
            guard.top()
        ),
        Err(err) => slog!(console, "stack guard off: {:?}", err),
    }

    spawner.spawn(unwrap!(button_task(p.P0_11.into(), ButtonEvent::Up)));
    spawner.spawn(unwrap!(button_task(p.P0_12.into(), ButtonEvent::Down)));
    spawner.spawn(unwrap!(button_task(p.P0_24.into(), ButtonEvent::Select)));
    slog!(console, "buttons ready (UP=P0.11 DOWN=P0.12 SELECT=P0.24)");
    let twi = ui::display::new_twim(p.TWISPI0, Irqs, p.P0_26, p.P0_27);
    spawner.spawn(unwrap!(ui::display::task(twi)));
    slog!(console, "display task started (TWIM0 SDA=P0.26 SCL=P0.27)");

    let mut ui = UiController::<SimAddr>::new();
    let mut ble = SimBle::new();

    slog!(
        console,
        "entering sim UI loop (screen={:?})",
        ui.state.screen
    );
    // As in the firmware, the display task gets the latest view after every
    // change and never holds up this loop; the panel stays on, because the
    // simulation has no USB power state to blank it for.
    ui::display::publish(&ui.state, true);
    loop {
        // A button press runs the UI controller, and the command it sends, if
        // any, runs through the simulated BLE side at once. Two seconds without
        // a press advance the scripted peripheral scenario, so a tick is
        // "2 s since the last press or tick", not a fixed period.
        match select(
            BUTTON_CHANNEL.receive(),
            Timer::after(Duration::from_secs(2)),
        )
        .await
        {
            Either::First(button) => {
                let command = ui.button(button, Instant::now().as_millis());
                slog!(
                    console,
                    "button {:?} -> screen {:?} (selected {})",
                    button,
                    ui.state.screen,
                    ui.state.selected
                );
                if let Some(command) = command {
                    log_command(&mut console, &command);
                    let events = ble.command(&mut console, command).await;
                    apply(&mut console, &mut ui, events);
                }
            }
            Either::Second(()) => {
                let events = ble.scenario_step(&mut console);
                apply(&mut console, &mut ui, events);
                if ui.tick(Instant::now().as_millis(), true) {
                    slog!(console, "ui: management request got no reply");
                }
            }
        }
        ui::display::publish(&ui.state, true);
    }
}
