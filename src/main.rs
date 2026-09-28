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
//! | `display_task`      | OLED rendering, initialization and fault recovery    |
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

use defmt::{info, unwrap};
use defmt_rtt as _; // global logger
use panic_probe as _; // panic handler → defmt

use embassy_executor::Spawner;
use embassy_nrf::gpio::AnyPin;
use embassy_nrf::interrupt::{InterruptExt, Priority};
use embassy_nrf::usb::vbus_detect::SoftwareVbusDetect;
use embassy_nrf::Peri;
use embassy_nrf::{self, bind_interrupts, interrupt, peripherals, twim};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use nrf_softdevice::SocEvent;

use crate::ble::multi_conn::{self, SlotCommand, SlotEvent};
use crate::ble::{BleCommand, BleEvent};
use crate::hid::delivery::HidEvent;
use crate::power::PowerManager;
use crate::ui::{ButtonEvent, Screen};
use crate::usb::hid_device;
use embassy_time::{Duration, Ticker};
use heapless::Vec;

/// BLE HID reports → USB HID writer.
static HID_REPORT_CHANNEL: Channel<CriticalSectionRawMutex, HidEvent, 16> = Channel::new();

/// UI → BLE commands (scan, connect, disconnect).
static BLE_CMD_CHANNEL: Channel<CriticalSectionRawMutex, BleCommand, 4> = Channel::new();

/// BLE → UI events (device found, connected, error).
static BLE_EVENT_CHANNEL: Channel<CriticalSectionRawMutex, BleEvent, 8> = Channel::new();

/// Coordinator -> BLE slot 0 command channel.
static BLE_SLOT0_CMD_CHANNEL: Channel<CriticalSectionRawMutex, SlotCommand, 2> = Channel::new();

/// Coordinator -> BLE slot 1 command channel.
static BLE_SLOT1_CMD_CHANNEL: Channel<CriticalSectionRawMutex, SlotCommand, 2> = Channel::new();

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
        &BLE_SLOT0_CMD_CHANNEL.sender(),
        &BLE_SLOT1_CMD_CHANNEL.sender(),
        &BLE_SLOT_EVENT_CHANNEL.receiver(),
    )
    .await
}

#[embassy_executor::task]
async fn ble_slot0_task(sd: &'static nrf_softdevice::Softdevice) -> ! {
    multi_conn::connection_slot_task(
        0,
        sd,
        &BLE_SLOT0_CMD_CHANNEL.receiver(),
        &BLE_SLOT_EVENT_CHANNEL.sender(),
        &HID_REPORT_CHANNEL.sender(),
    )
    .await
}

#[embassy_executor::task]
async fn ble_slot1_task(sd: &'static nrf_softdevice::Softdevice) -> ! {
    multi_conn::connection_slot_task(
        1,
        sd,
        &BLE_SLOT1_CMD_CHANNEL.receiver(),
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

#[embassy_executor::task]
async fn display_task(twi: twim::Twim<'static, peripherals::TWISPI0>) -> ! {
    ui::display::run(ui::display::StopSafeI2c::new(twi)).await
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

    spawner.spawn(unwrap!(ble_slot0_task(sd)));
    spawner.spawn(unwrap!(ble_slot1_task(sd)));
    spawner.spawn(unwrap!(ble_task(sd)));
    info!("BLE task started");

    let mut twi_config = twim::Config::default();
    // Most SSD1306 modules carry their own I2C pull-ups, but a bare panel or
    // a module without them would leave the bus floating. The internal
    // pull-ups (~13 kΩ) are harmless in parallel with external ones.
    twi_config.sda_pullup = true;
    twi_config.scl_pullup = true;
    // embassy-nrf 0.7's Twim requires a RAM scratch buffer for writes whose
    // source isn't in RAM (e.g. flash-resident SSD1306 command sequences); the
    // framebuffer flush is already RAM-backed. This lives for the program.
    static TWI_TX_BUF: static_cell::StaticCell<[u8; 64]> = static_cell::StaticCell::new();
    let twi_tx_buf = TWI_TX_BUF.init([0u8; 64]);
    let twi = twim::Twim::new(
        p.TWISPI0, TwimIrqs, p.P0_26, p.P0_27, twi_config, twi_tx_buf,
    );

    spawner.spawn(unwrap!(display_task(twi)));
    spawner.spawn(unwrap!(button_up_task(p.P0_11.into())));
    spawner.spawn(unwrap!(button_down_task(p.P0_12.into())));
    spawner.spawn(unwrap!(button_select_task(p.P0_24.into())));
    info!("UI and isolated OLED tasks started");

    let mut state = ui::ui_logic::UiState::new();
    let mut paired: Vec<ble::DiscoveredDevice, 4> = Vec::new();
    let mut management = ui::ui_logic::ManagementRequests::default();
    let mut power = PowerManager::new();
    let mut stack_reported = 0;
    let mut housekeeping = Ticker::every(Duration::from_secs(1));
    ui::display::publish(&state, power.display_on());

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
                if state.screen == Screen::Scanning && power.display_on() {
                    state.scan_dots = ui::input_logic::next_scan_dots(state.scan_dots);
                }
            }
            embassy_futures::select::Either4::Third(button) => {
                let was_off = !power.display_on();
                power.activity();
                // First press only wakes the screen. USB suspend still wins.
                if !was_off && !management.is_pending() {
                    if let Some(command) = state.button(button) {
                        use ui::ui_logic::UiCommand;
                        let request_id = if matches!(
                            command,
                            UiCommand::ListPaired | UiCommand::Forget(_) | UiCommand::FactoryReset
                        ) {
                            management.begin(command)
                        } else {
                            None
                        };
                        let ble_command = match command {
                            UiCommand::StartScan => Some(BleCommand::StartScan),
                            UiCommand::Connect(index) => Some(BleCommand::Connect(index)),
                            UiCommand::Disconnect => Some(BleCommand::Disconnect),
                            UiCommand::ListPaired => {
                                request_id.map(|id| BleCommand::ListPaired { id })
                            }
                            UiCommand::Forget(index) => paired.get(index).and_then(|peer| {
                                request_id.map(|id| BleCommand::Forget {
                                    id,
                                    address: peer.address,
                                })
                            }),
                            UiCommand::FactoryReset => {
                                request_id.map(|id| BleCommand::FactoryReset { id })
                            }
                            UiCommand::Dismiss => None,
                        };
                        if let Some(ble_command) = ble_command {
                            // Never deadlock UI and BLE by awaiting a full command
                            // channel while BLE is awaiting a full event channel.
                            if BLE_CMD_CHANNEL.try_send(ble_command).is_err() {
                                state.error("Busy; try again");
                                if let Some(id) = request_id {
                                    management.complete(id);
                                }
                            }
                        } else if command != UiCommand::Dismiss {
                            if let Some(id) = request_id {
                                management.complete(id);
                            }
                            state.error("Device changed; retry");
                        }
                    }
                }
            }
            embassy_futures::select::Either4::Fourth(event) => match event {
                BleEvent::ScanStarted => state.scan_started(),
                BleEvent::DeviceFound(device) => {
                    if state.screen == Screen::Scanning {
                        let _ = state.devices.push(device.name);
                    }
                }
                BleEvent::ScanComplete => state.scan_complete(),
                BleEvent::Connected(name) => {
                    power.set_ble_connected(true);
                    state.connection_status(Some(name));
                }
                BleEvent::Disconnected => {
                    power.set_ble_connected(false);
                    state.connection_status(None);
                }
                BleEvent::Error(tag) => {
                    state.error(ble_error_message(tag));
                }
                BleEvent::PairedDevices { id, devices } => {
                    if management.complete(id) == Some(ui::ui_logic::UiCommand::ListPaired) {
                        paired = devices;
                        state.paired_names.clear();
                        for peer in &paired {
                            let _ = state.paired_names.push(peer.name.clone());
                        }
                        if state.screen != Screen::Error {
                            state.screen = Screen::SavedDevices;
                            state.selected = 0;
                        }
                    }
                }
                BleEvent::ManagementResult { id, result } => {
                    if let Some(command) = management.complete(id) {
                        match result {
                            Ok(()) => {
                                paired.clear();
                                state.management_completed(command);
                            }
                            Err(tag) => state.error(ble_error_message(tag)),
                        }
                    }
                }
            },
        }
        ui::display::publish(&state, power.display_on());
    }
}

fn ble_error_message(tag: ble::BleErrorTag) -> &'static str {
    match tag {
        ble::BleErrorTag::ScanFailed => "Scan failed",
        ble::BleErrorTag::ConnectFailed => "Connect failed",
        ble::BleErrorTag::HidNotFound => "No HID service",
        ble::BleErrorTag::NotifyFailed => "Notify failed",
        ble::BleErrorTag::StorageFailed => "Storage failed",
        ble::BleErrorTag::ManagementFailed => "Action failed; retry",
        ble::BleErrorTag::ReportMapReadFailed => "HID map read failed",
        ble::BleErrorTag::ReportMapTooLarge => "HID map too large",
        ble::BleErrorTag::ReportMapInvalid => "Unsupported HID map",
    }
}
