//! # bt2usb - Bluetooth-to-USB HID Bridge
//!
//! Firmware for the **nRF52840** that acts as a BLE Central, connecting to
//! Bluetooth HID peripherals (keyboards, mice) and re-transmitting their
//! reports over USB through a PC monitor hub so the PC sees a standard wired HID device.
//!
//! ## Architecture
//!
//! ```text
//! +-------------------+   BLE HID reports   +---------------------+   USB HID reports   +----------------+
//! | BT Keyboard/Mouse | ------------------> | nRF52840 (firmware) | ------------------> | PC Monitor Hub |
//! +-------------------+                     +---------------------+                     +----------------+
//!                                                                                              |
//!                                                                                              | USB upstream
//!                                                                                              v
//!                                                                                        +-----------+
//!                                                                                        |    PC     |
//!                                                                                        +-----------+
//!                                                   ^
//!                                                   |
//!                                         SSD1306 OLED + 3 buttons
//! ```
//!
//! ## Async tasks (Embassy)
//!
//! | Task                | Responsibility                                       |
//! |---------------------|------------------------------------------------------|
//! | `softdevice_task`   | Runs the SoftDevice event loop; forwards USB power events |
//! | `ble_task`          | BLE coordinator: scan, slot orchestration, flash persist |
//! | `ble_slot{0,1}_task`| Per-slot connect/secure + HID notification loop      |
//! | `usb_device_task`   | USB enumeration and endpoint servicing               |
//! | `hid_writer_task`   | Dispatches aggregate state to independent USB workers |
//! | `ui::display::task` | OLED rendering, initialization and fault recovery    |
//! | `button_*_task`     | Per-button debounced GPIO watcher (×3)               |
//!
//! The UI state machine runs in `main`, reacting to button and BLE events and
//! publishing the latest view to the display task without waiting for I2C.

#![no_std]
#![no_main]

mod ble;
mod config;
mod hid;
mod power;
mod power_logic;
mod sd_setup;
mod stack;
mod storage;
mod ui;
mod usb;

use defmt::{info, unwrap, warn};
use defmt_rtt as _; // global logger
use panic_probe as _; // panic handler → defmt

use embassy_executor::Spawner;
use embassy_nrf::gpio::AnyPin;
use embassy_nrf::interrupt::{InterruptExt, Priority};
use embassy_nrf::usb::vbus_detect::SoftwareVbusDetect;
use embassy_nrf::Peri;
use embassy_nrf::{self, bind_interrupts, interrupt, peripherals, twim};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, TrySendError};
use nrf_softdevice::SocEvent;

use crate::ble::coordinator::MAX_CONNECTIONS;
use crate::ble::multi_conn::{self, SlotCommand, SlotEvent};
use crate::ble::slot_worker;
use crate::ble::{BleCommand, BleEvent};
use crate::hid::delivery::HidEvent;
use crate::power::PowerManager;
use crate::ui::controller::UiController;
use crate::ui::ButtonEvent;
use crate::usb::hid_device;
use embassy_time::{Duration, Instant, Ticker};

/// BLE HID reports → USB HID writer.
static HID_REPORT_CHANNEL: Channel<CriticalSectionRawMutex, HidEvent, 16> = Channel::new();

/// UI → BLE commands (scan, connect, disconnect).
static BLE_CMD_CHANNEL: Channel<CriticalSectionRawMutex, BleCommand, 4> = Channel::new();

/// BLE → UI events (device found, connected, error).
static BLE_EVENT_CHANNEL: Channel<CriticalSectionRawMutex, BleEvent, 8> = Channel::new();

/// Coordinator -> BLE slot command channels, one per link, indexed by slot.
static BLE_SLOT_CMD_CHANNELS: [Channel<CriticalSectionRawMutex, SlotCommand, 2>; MAX_CONNECTIONS] =
    [const { Channel::new() }; MAX_CONNECTIONS];

/// BLE slot workers -> coordinator event channel.
static BLE_SLOT_EVENT_CHANNEL: Channel<CriticalSectionRawMutex, SlotEvent, 8> = Channel::new();

/// Button press events → UI.
static BUTTON_CHANNEL: Channel<CriticalSectionRawMutex, ButtonEvent, 4> = Channel::new();

bind_interrupts!(struct TwimIrqs {
    TWISPI0 => twim::InterruptHandler<peripherals::TWISPI0>;
});

#[embassy_executor::task]
async fn softdevice_task(
    sd: &'static nrf_softdevice::Softdevice,
    vbus: &'static SoftwareVbusDetect,
) -> ! {
    // Drive the software VBUS detector from SoftDevice SoC power events, since
    // the SoftDevice owns the POWER peripheral and the application cannot read
    // those events directly.
    sd.run_with_callback(|event| match event {
        SocEvent::PowerUsbDetected => vbus.detected(true),
        SocEvent::PowerUsbRemoved => vbus.detected(false),
        SocEvent::PowerUsbPowerReady => vbus.ready(),
        _ => {}
    })
    .await
}

#[embassy_executor::task]
async fn ble_task(sd: &'static nrf_softdevice::Softdevice) -> ! {
    multi_conn::ble_task(
        sd,
        &BLE_CMD_CHANNEL.receiver(),
        &BLE_EVENT_CHANNEL.sender(),
        &BLE_SLOT_CMD_CHANNELS.each_ref().map(Channel::sender),
        &BLE_SLOT_EVENT_CHANNEL.receiver(),
    )
    .await
}

/// One connection worker per link; `main` spawns one for each slot, with that
/// slot's command channel.
#[embassy_executor::task(pool_size = MAX_CONNECTIONS)]
async fn ble_slot_task(
    slot: usize,
    commands: &'static Channel<CriticalSectionRawMutex, SlotCommand, 2>,
    sd: &'static nrf_softdevice::Softdevice,
) -> ! {
    slot_worker::connection_slot_task(
        slot,
        sd,
        &commands.receiver(),
        &BLE_SLOT_EVENT_CHANNEL.sender(),
        &HID_REPORT_CHANNEL.sender(),
    )
    .await
}

#[embassy_executor::task]
async fn usb_device_task(device: embassy_usb::UsbDevice<'static, hid_device::UsbDriver>) -> ! {
    hid_device::run_usb_device(device).await
}

#[embassy_executor::task]
async fn hid_writer_task(
    keyboard: embassy_usb::class::hid::HidWriter<'static, hid_device::UsbDriver, 8>,
    mouse: embassy_usb::class::hid::HidWriter<'static, hid_device::UsbDriver, 8>,
    consumer: embassy_usb::class::hid::HidWriter<'static, hid_device::UsbDriver, 8>,
) -> ! {
    hid_device::hid_writer_task(keyboard, mouse, consumer, &HID_REPORT_CHANNEL.receiver()).await
}

#[embassy_executor::task]
async fn button_up_task(pin: Peri<'static, AnyPin>) -> ! {
    ui::buttons::button_task(pin, ButtonEvent::Up, &BUTTON_CHANNEL.sender()).await
}

#[embassy_executor::task]
async fn button_down_task(pin: Peri<'static, AnyPin>) -> ! {
    ui::buttons::button_task(pin, ButtonEvent::Down, &BUTTON_CHANNEL.sender()).await
}

#[embassy_executor::task]
async fn button_select_task(pin: Peri<'static, AnyPin>) -> ! {
    ui::buttons::button_task(pin, ButtonEvent::Select, &BUTTON_CHANNEL.sender()).await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("bt2usb firmware starting");

    let mut nrf_config = embassy_nrf::config::Config::default();
    nrf_config.gpiote_interrupt_priority = Priority::P2;
    nrf_config.time_interrupt_priority = Priority::P2;
    let p = embassy_nrf::init(nrf_config);

    // The SoftDevice reserves interrupt priorities 0, 1, and 4. Every
    // application peripheral interrupt must run at 2, 3, 5, 6, or 7 or it will
    // preempt SoftDevice critical sections and fault. embassy-nrf only lowers
    // the GPIOTE and time-driver interrupts for us, so set the rest explicitly.
    interrupt::USBD.set_priority(Priority::P2);
    interrupt::TWISPI0.set_priority(Priority::P2);

    let sd = nrf_softdevice::Softdevice::enable(&sd_setup::softdevice_config());

    let (vbus_detected, usb_power_ready) = sd_setup::enable_usb_power_events();
    let usb = hid_device::init(p.USBD, vbus_detected, usb_power_ready);
    // Spawn the SoftDevice task with the VBUS detector so it can forward USB
    // power SoC events to the USB stack.
    spawner.spawn(unwrap!(softdevice_task(sd, usb.vbus)));
    info!("SoftDevice started");
    spawner.spawn(unwrap!(usb_device_task(usb.device)));
    spawner.spawn(unwrap!(hid_writer_task(
        usb.keyboard_writer,
        usb.mouse_writer,
        usb.consumer_writer,
    )));
    info!("USB HID device started");

    for (slot, commands) in BLE_SLOT_CMD_CHANNELS.iter().enumerate() {
        spawner.spawn(unwrap!(ble_slot_task(slot, commands, sd)));
    }
    spawner.spawn(unwrap!(ble_task(sd)));
    info!("BLE task started");

    let twi = ui::display::new_twim(p.TWISPI0, TwimIrqs, p.P0_26, p.P0_27);
    spawner.spawn(unwrap!(ui::display::task(twi)));
    spawner.spawn(unwrap!(button_up_task(p.P0_11.into())));
    spawner.spawn(unwrap!(button_down_task(p.P0_12.into())));
    spawner.spawn(unwrap!(button_select_task(p.P0_24.into())));
    info!("UI and isolated OLED tasks started");

    let mut ui = UiController::<nrf_softdevice::ble::Address>::new();
    let mut power = PowerManager::new();
    let mut stack_reported = 0;
    let mut housekeeping = Ticker::every(Duration::from_secs(1));
    ui::display::publish(&ui.state, power.display_on());

    loop {
        // Prioritize bus power and maintenance; display work is in another
        // task and cannot prevent this loop from draining BLE events.
        let action = embassy_futures::select::select4(
            hid_device::suspend_signal().wait(),
            housekeeping.next(),
            BUTTON_CHANNEL.receive(),
            BLE_EVENT_CHANNEL.receive(),
        )
        .await;
        match action {
            embassy_futures::select::Either4::First(suspended) => {
                power.set_usb_suspended(suspended);
            }
            embassy_futures::select::Either4::Second(_) => {
                power.tick();
                let (used, total) = stack::high_water();
                if used > stack_reported {
                    stack_reported = used;
                    info!("stack high-water: {} of {} bytes", used, total);
                }
                if ui.tick(Instant::now().as_millis(), power.display_on()) {
                    warn!("management request got no reply; result unknown");
                }
            }
            embassy_futures::select::Either4::Third(button) => {
                let was_off = !power.display_on();
                power.activity();
                // First press only wakes the screen. USB suspend still wins.
                if !was_off {
                    if let Some(command) = ui.button(button, Instant::now().as_millis()) {
                        // Never deadlock UI and BLE by awaiting a full command
                        // channel while BLE is awaiting a full event channel.
                        if let Err(TrySendError::Full(command)) = BLE_CMD_CHANNEL.try_send(command)
                        {
                            ui.command_not_sent(&command);
                        }
                    }
                }
            }
            embassy_futures::select::Either4::Fourth(event) => {
                if let Some(connected) = event.link_up() {
                    power.set_ble_connected(connected);
                }
                ui.event(event);
            }
        }
        ui::display::publish(&ui.state, power.display_on());
    }
}
