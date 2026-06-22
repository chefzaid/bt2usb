//! # bt2usb-sim — SoftDevice-free simulation entry point (Layer 3 / Renode)
//!
//! The real firmware can't be emulated end-to-end: the Nordic SoftDevice is a
//! closed BLE blob tied to the radio, and the nRF USBD peripheral isn't modeled
//! by emulators. This binary is the SoftDevice/USB-free variant that **does**
//! run on a simulated nRF52840 ([Renode](https://renode.io)), so we can exercise,
//! without hardware:
//!
//! - boot + the Embassy executor + the RTC time driver + the memory map,
//! - the GPIO button driver (real `ui::buttons` code, on P0.11/12/24),
//! - the **real** host-tested logic — `ui::ui_logic` (screen transitions) and
//!   `ble::coordinator` (connection-slot state machine + reducers) — driven by a
//!   synthetic BLE scenario, with `Address` substituted by a `u32` stand-in.
//!
//! Output is written to **UART0** (Renode's `uart0`), which Renode shows on its
//! console / analyzer with no probe or decoder. See the README "Renode
//! simulation" guide.

#![no_std]
#![no_main]
// This binary reuses shared modules (e.g. the SSD1306 display driver) that it
// does not fully exercise; don't warn about the unused parts.
#![allow(dead_code)]

mod ble;
mod config;
mod ui;

use core::fmt::Write as _;

use defmt_rtt as _; // defmt global logger required by shared modules (e.g. buttons)
use panic_probe as _; // panic handler → defmt

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_nrf::gpio::AnyPin;
use embassy_nrf::peripherals::UARTE0;
use embassy_nrf::uarte::{self, Uarte};
use embassy_nrf::{bind_interrupts, peripherals};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Timer};
use heapless::String;

use crate::ble::coordinator::{self, Action, ConnManager, DeviceInfo, UiEvent};
use crate::ui::ui_logic::{self, Redraw, UiCommand};
use crate::ui::{ButtonEvent, Screen};

bind_interrupts!(struct Irqs {
    UARTE0 => uarte::InterruptHandler<peripherals::UARTE0>;
});

/// Stand-in for the SoftDevice `Address` type, which is unavailable without the
/// radio stack. The coordinator is generic over the address type precisely so
/// the same logic runs here and in the real firmware.
type SimAddr = u32;

static BUTTON_CHANNEL: Channel<CriticalSectionRawMutex, ButtonEvent, 4> = Channel::new();

#[embassy_executor::task(pool_size = 3)]
async fn button_task(pin: AnyPin, event: ButtonEvent) -> ! {
    ui::buttons::button_task(pin, event, &BUTTON_CHANNEL.sender()).await
}

/// Synthetic button stimulus.
///
/// The real `button_task`s above are spawned (so the GPIO driver's setup runs),
/// but Renode's GPIOTE model doesn't drive embassy-nrf's edge waits from
/// injected pin changes, so physical presses don't reach the firmware. To still
/// exercise the UI reducer (`ui_logic::on_button`) on the simulated MCU, this
/// task feeds a rotating sequence of button events into the same channel the
/// real buttons use.
#[embassy_executor::task]
async fn ui_stimulus() -> ! {
    let sequence = [
        ButtonEvent::Select, // Home  -> Scanning (StartScan)
        ButtonEvent::Down,   // DeviceList navigation (once populated)
        ButtonEvent::Select, // connect highlighted
        ButtonEvent::Down,   // Connected -> Home (disconnect)
    ];
    let mut i = 0usize;
    loop {
        Timer::after(Duration::from_secs(3)).await;
        BUTTON_CHANNEL.send(sequence[i % sequence.len()]).await;
        i += 1;
    }
}

/// Format a line and write it to UART0 (Renode console). EasyDMA needs the
/// source buffer in RAM, which a stack `String` satisfies.
macro_rules! slog {
    ($uart:expr, $($arg:tt)*) => {{
        let mut line: String<160> = String::new();
        let _ = write!(line, $($arg)*);
        let _ = line.push_str("\r\n");
        let _ = $uart.blocking_write(line.as_bytes());
    }};
}

fn name32(s: &str) -> String<32> {
    let mut n = String::new();
    let _ = n.push_str(s);
    n
}

fn log_action(uart: &mut Uarte<'static, UARTE0>, action: &Action<SimAddr>) {
    match action {
        Action::DisconnectSlot(slot) => slog!(uart, "  action: DisconnectSlot({})", slot),
        Action::ConnectSlot { slot, device } => {
            slog!(
                uart,
                "  action: ConnectSlot slot={} addr={:#x}",
                slot,
                device.address
            )
        }
        Action::PersistDevice(device) => {
            slog!(uart, "  action: PersistDevice addr={:#x}", device.address)
        }
        Action::Emit(UiEvent::Connected(name)) => {
            slog!(uart, "  action: UI Connected '{}'", name.as_str())
        }
        Action::Emit(UiEvent::Disconnected) => slog!(uart, "  action: UI Disconnected"),
        Action::Emit(UiEvent::Error(_)) => slog!(uart, "  action: UI Error"),
    }
}

/// Drive one step of a synthetic BLE scenario through the real coordinator
/// reducers, logging the decisions. This runs the exact host-tested logic on the
/// simulated MCU (connect kbd → connect mouse → drop one → disconnect all).
fn scenario_step(
    uart: &mut Uarte<'static, UARTE0>,
    step: u32,
    manager: &mut ConnManager<SimAddr>,
    devices: &[DeviceInfo<SimAddr>],
) {
    match step % 4 {
        0 => {
            slog!(uart, "scenario: connect device 0 (Keyboard)");
            for a in coordinator::plan_connect(manager, devices, 0) {
                log_action(uart, &a);
                if let Action::ConnectSlot { slot, device } = a {
                    for b in coordinator::on_slot_connected(manager, slot, &device) {
                        log_action(uart, &b);
                    }
                }
            }
        }
        1 => {
            slog!(uart, "scenario: connect device 1 (Mouse)");
            for a in coordinator::plan_connect(manager, devices, 1) {
                log_action(uart, &a);
                if let Action::ConnectSlot { slot, device } = a {
                    for b in coordinator::on_slot_connected(manager, slot, &device) {
                        log_action(uart, &b);
                    }
                }
            }
        }
        2 => {
            slog!(uart, "scenario: slot 0 link lost");
            for a in coordinator::on_slot_disconnected(manager, 0) {
                log_action(uart, &a);
            }
        }
        _ => {
            slog!(uart, "scenario: disconnect all");
            for a in coordinator::plan_disconnect(manager) {
                log_action(uart, &a);
                if let Action::DisconnectSlot(slot) = a {
                    for b in coordinator::on_slot_disconnected(manager, slot) {
                        log_action(uart, &b);
                    }
                }
            }
        }
    }
    slog!(uart, "scenario: active_count={}", manager.active_count());
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_nrf::init(Default::default());

    // UART0 for human-readable output (TX=P0.06, RX=P0.08). Renode's uart0
    // model emits the TX bytes regardless of physical pin routing.
    let mut uart = Uarte::new(p.UARTE0, Irqs, p.P0_08, p.P0_06, uarte::Config::default());

    slog!(
        &mut uart,
        "bt2usb-sim starting (SoftDevice-free Renode build)"
    );

    spawner.must_spawn(button_task(p.P0_11.into(), ButtonEvent::Up));
    spawner.must_spawn(button_task(p.P0_12.into(), ButtonEvent::Down));
    spawner.must_spawn(button_task(p.P0_24.into(), ButtonEvent::Select));
    spawner.must_spawn(ui_stimulus());
    slog!(
        &mut uart,
        "buttons ready (UP=P0.11 DOWN=P0.12 SELECT=P0.24)"
    );

    let devices = [
        DeviceInfo {
            address: 0xA1,
            name: name32("Keyboard"),
            rssi: -42,
        },
        DeviceInfo {
            address: 0xB2,
            name: name32("Mouse"),
            rssi: -55,
        },
    ];
    let mut manager: ConnManager<SimAddr> = ConnManager::new();

    let mut screen = Screen::Home;
    let mut selected: usize = 0;
    let device_count: usize = devices.len();
    let mut step: u32 = 0;

    slog!(&mut uart, "entering sim UI loop (screen=Home)");
    loop {
        // A button press exercises the UI reducer; the 2 s tick advances the
        // synthetic BLE scenario through the coordinator reducers.
        match select(
            BUTTON_CHANNEL.receive(),
            Timer::after(Duration::from_secs(2)),
        )
        .await
        {
            Either::First(btn) => {
                let outcome = ui_logic::on_button(screen, btn, selected, device_count);
                screen = outcome.screen;
                selected = outcome.selected;
                slog!(
                    &mut uart,
                    "button {:?} -> screen {:?} (selected {})",
                    btn,
                    screen,
                    selected
                );
                match outcome.redraw {
                    Redraw::Scanning => slog!(&mut uart, "  redraw: Scanning"),
                    Redraw::DeviceList => slog!(&mut uart, "  redraw: DeviceList"),
                    Redraw::Home => slog!(&mut uart, "  redraw: Home"),
                    Redraw::None => {}
                }
                if let Some(cmd) = outcome.command {
                    match cmd {
                        UiCommand::StartScan => slog!(&mut uart, "  cmd: StartScan"),
                        UiCommand::Connect(i) => slog!(&mut uart, "  cmd: Connect({})", i),
                        UiCommand::Disconnect => slog!(&mut uart, "  cmd: Disconnect"),
                    }
                }
            }
            Either::Second(_) => {
                scenario_step(&mut uart, step, &mut manager, &devices);
                step = step.wrapping_add(1);
            }
        }
    }
}
