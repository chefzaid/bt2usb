//! # bt2usb - Bluetooth-to-USB HID Bridge
//!
//! Firmware for the **nRF52840** that acts as a BLE Central, connecting to
//! Bluetooth HID peripherals (keyboards, mice) and re-transmitting their
//! reports over USB so the host PC sees a standard wired HID device.
//!
//! ## Architecture
//!
//! ```text
//! +-------------------+   BLE HID reports   +---------------------+   USB HID reports   +-----------+
//! | BT Keyboard/Mouse | ------------------> | nRF52840 (firmware) | ------------------> | Host PC   |
//! +-------------------+                     +---------------------+                     +-----------+
//!                                                   ^
//!                                                   |
//!                                         SSD1306 OLED + 3 buttons
//! ```
//!
//! ## Async tasks (Embassy)
//!
//! | Task              | Responsibility                                 |
//! |-------------------|------------------------------------------------|
//! | `softdevice_task` | Runs the Nordic SoftDevice event loop          |
//! | `ble_task`        | Scan / connect / receive BLE HID reports       |
//! | `usb_device_task` | USB enumeration and endpoint servicing          |
//! | `hid_writer_task` | Forwards BLE reports → USB HID endpoints       |
//! | `ui_task`         | Drives display + reacts to button presses      |
//! | `button_*_task`   | Per-button debounced GPIO watcher (×3)         |

#![no_std]
#![no_main]

mod ble;
mod config;
mod hid;
mod power;
mod power_logic;
mod storage;
mod ui;
mod usb;

use defmt::{info, unwrap};
use defmt_rtt as _; // global logger
use panic_probe as _; // panic handler → defmt

use embassy_executor::Spawner;
use embassy_nrf::gpio::AnyPin;
use embassy_nrf::{self, bind_interrupts, peripherals, twim};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;

use crate::ble::multi_conn::{self, SlotCommand, SlotEvent};
use crate::ble::{BleCommand, BleEvent};
use crate::hid::HidReport;
use crate::power::PowerManager;
use crate::ui::{ButtonEvent, Screen};
use crate::usb::hid_device;
use embassy_time::{Duration, Timer};
use heapless::Vec;

/// BLE HID reports → USB HID writer.
static HID_REPORT_CHANNEL: Channel<CriticalSectionRawMutex, HidReport, 16> = Channel::new();

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

fn softdevice_config() -> nrf_softdevice::Config {
    nrf_softdevice::Config {
        clock: Some(nrf_softdevice::raw::nrf_clock_lf_cfg_t {
            source: nrf_softdevice::raw::NRF_CLOCK_LF_SRC_RC as u8,
            rc_ctiv: 16,
            rc_temp_ctiv: 2,
            accuracy: nrf_softdevice::raw::NRF_CLOCK_LF_ACCURACY_500_PPM as u8,
        }),
        conn_gap: Some(nrf_softdevice::raw::ble_gap_conn_cfg_t {
            conn_count: 2,
            event_length: 24,
        }),
        conn_gatt: Some(nrf_softdevice::raw::ble_gatt_conn_cfg_t { att_mtu: 64 }),
        gap_role_count: Some(nrf_softdevice::raw::ble_gap_cfg_role_count_t {
            adv_set_count: 0,      // we don't advertise
            periph_role_count: 0,  // we don't act as peripheral
            central_role_count: 2, // up to two central connections
            central_sec_count: 2,
            _bitfield_1: nrf_softdevice::raw::ble_gap_cfg_role_count_t::new_bitfield_1(0),
        }),
        ..Default::default()
    }
}

#[embassy_executor::task]
async fn softdevice_task(sd: &'static nrf_softdevice::Softdevice) -> ! {
    sd.run().await
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
async fn usb_device_task(
    device: embassy_usb::UsbDevice<
        'static,
        embassy_nrf::usb::Driver<
            'static,
            peripherals::USBD,
            embassy_nrf::usb::vbus_detect::HardwareVbusDetect,
        >,
    >,
) -> ! {
    hid_device::run_usb_device(device).await
}

#[embassy_executor::task]
async fn hid_writer_task(
    keyboard: embassy_usb::class::hid::HidWriter<
        'static,
        embassy_nrf::usb::Driver<
            'static,
            peripherals::USBD,
            embassy_nrf::usb::vbus_detect::HardwareVbusDetect,
        >,
        8,
    >,
    mouse: embassy_usb::class::hid::HidWriter<
        'static,
        embassy_nrf::usb::Driver<
            'static,
            peripherals::USBD,
            embassy_nrf::usb::vbus_detect::HardwareVbusDetect,
        >,
        8,
    >,
    consumer: embassy_usb::class::hid::HidWriter<
        'static,
        embassy_nrf::usb::Driver<
            'static,
            peripherals::USBD,
            embassy_nrf::usb::vbus_detect::HardwareVbusDetect,
        >,
        8,
    >,
) -> ! {
    hid_device::hid_writer_task(keyboard, mouse, consumer, &HID_REPORT_CHANNEL.receiver()).await
}

#[embassy_executor::task]
async fn button_up_task(pin: AnyPin) -> ! {
    ui::buttons::button_task(pin, ButtonEvent::Up, &BUTTON_CHANNEL.sender()).await
}

#[embassy_executor::task]
async fn button_down_task(pin: AnyPin) -> ! {
    ui::buttons::button_task(pin, ButtonEvent::Down, &BUTTON_CHANNEL.sender()).await
}

#[embassy_executor::task]
async fn button_select_task(pin: AnyPin) -> ! {
    ui::buttons::button_task(pin, ButtonEvent::Select, &BUTTON_CHANNEL.sender()).await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("bt2usb firmware starting");

    let mut nrf_config = embassy_nrf::config::Config::default();
    nrf_config.gpiote_interrupt_priority = embassy_nrf::interrupt::Priority::P2;
    nrf_config.time_interrupt_priority = embassy_nrf::interrupt::Priority::P2;
    let p = embassy_nrf::init(nrf_config);

    let sd = nrf_softdevice::Softdevice::enable(&softdevice_config());
    unwrap!(spawner.spawn(softdevice_task(sd)));
    info!("SoftDevice started");

    let usb = hid_device::init(p.USBD);
    unwrap!(spawner.spawn(usb_device_task(usb.device)));
    unwrap!(spawner.spawn(hid_writer_task(
        usb.keyboard_writer,
        usb.mouse_writer,
        usb.consumer_writer,
    )));
    info!("USB HID device started");

    unwrap!(spawner.spawn(ble_slot0_task(sd)));
    unwrap!(spawner.spawn(ble_slot1_task(sd)));
    unwrap!(spawner.spawn(ble_task(sd)));
    info!("BLE task started");

    let twi_config = twim::Config::default();
    let twi = twim::Twim::new(p.TWISPI0, TwimIrqs, p.P0_26, p.P0_27, twi_config);

    let mut display = ui::display::init(twi);
    ui::display::draw_home(&mut display, false, "");
    info!("OLED display initialised");

    unwrap!(spawner.spawn(button_up_task(p.P0_11.into())));
    unwrap!(spawner.spawn(button_down_task(p.P0_12.into())));
    unwrap!(spawner.spawn(button_select_task(p.P0_24.into())));
    info!("Button handlers started (3 buttons)");

    info!("Entering UI main loop");
    let mut screen = Screen::Home;
    let mut selected: usize = 0;
    let mut device_count: usize = 0;
    let mut devices: Vec<heapless::String<32>, 8> = Vec::new();
    let mut connected_name: heapless::String<32> = heapless::String::new();
    let mut power = PowerManager::new();
    let mut sleep_banner_drawn = false;

    loop {
        let action = embassy_futures::select::select4(
            BUTTON_CHANNEL.receive(),
            BLE_EVENT_CHANNEL.receive(),
            Timer::after(Duration::from_secs(1)),
            hid_device::suspend_signal().wait(),
        )
        .await;

        match action {
            embassy_futures::select::Either4::First(btn) => {
                let was_display_off = !power.display_on();
                power.activity();
                sleep_banner_drawn = false;

                if was_display_off {
                    match screen {
                        Screen::Home => ui::display::draw_home(&mut display, !connected_name.is_empty(), connected_name.as_str()),
                        Screen::Scanning => ui::display::draw_scanning(&mut display, 0),
                        Screen::DeviceList => {
                            ui::display::draw_device_list(&mut display, &devices, selected)
                        }
                        Screen::Connected => {
                            ui::display::draw_connected(&mut display, connected_name.as_str())
                        }
                        Screen::Error => ui::display::draw_error(&mut display, "Ready"),
                    }
                    continue;
                }

                match (screen, btn) {
                    (Screen::Home, ButtonEvent::Select) | (Screen::Error, ButtonEvent::Select) => {
                        screen = Screen::Scanning;
                        selected = 0;
                        device_count = 0;
                        devices.clear();
                        ui::display::draw_scanning(&mut display, 0);
                        BLE_CMD_CHANNEL.send(BleCommand::StartScan).await;
                    }

                    (Screen::DeviceList, ButtonEvent::Up) => {
                        selected = ui::input_logic::select_prev(selected);
                        ui::display::draw_device_list(&mut display, &devices, selected);
                    }

                    (Screen::DeviceList, ButtonEvent::Down) => {
                        let next = ui::input_logic::select_next(selected, device_count);
                        if next != selected {
                            selected = next;
                            ui::display::draw_device_list(&mut display, &devices, selected);
                        }
                    }

                    (Screen::DeviceList, ButtonEvent::Select) => {
                        screen = Screen::Scanning;
                        ui::display::draw_scanning(&mut display, 0);
                        BLE_CMD_CHANNEL.send(BleCommand::Connect(selected)).await;
                    }

                    (Screen::Connected, ButtonEvent::Select) => {
                        screen = Screen::Scanning;
                        selected = 0;
                        device_count = 0;
                        devices.clear();
                        ui::display::draw_scanning(&mut display, 0);
                        BLE_CMD_CHANNEL.send(BleCommand::StartScan).await;
                    }

                    (Screen::Connected, ButtonEvent::Down) => {
                        BLE_CMD_CHANNEL.send(BleCommand::Disconnect).await;
                        screen = Screen::Home;
                        ui::display::draw_home(&mut display, false, "");
                    }

                    _ => {}
                }
            }

            embassy_futures::select::Either4::Second(event) => match event {
                BleEvent::ScanStarted => {
                    sleep_banner_drawn = false;
                    screen = Screen::Scanning;
                    selected = 0;
                    device_count = 0;
                    devices.clear();
                    ui::display::draw_scanning(&mut display, 1);
                }

                BleEvent::DeviceFound(dev) => {
                    sleep_banner_drawn = false;
                    if !devices.is_full() {
                        let _ = devices.push(dev.name.clone());
                    }
                    device_count = devices.len();
                    info!(
                        "UI: device #{} = {} (RSSI {})",
                        device_count,
                        dev.name.as_str(),
                        dev.rssi
                    );
                }

                BleEvent::ScanComplete => {
                    sleep_banner_drawn = false;
                    if device_count > 0 {
                        screen = Screen::DeviceList;
                        selected = selected.min(device_count.saturating_sub(1));
                        ui::display::draw_device_list(&mut display, &devices, selected);
                    } else {
                        screen = Screen::Error;
                        ui::display::draw_error(&mut display, "No devices found");
                    }
                }

                BleEvent::Connected(name) => {
                    sleep_banner_drawn = false;
                    screen = Screen::Connected;
                    devices.clear();
                    connected_name = name.clone();
                    power.set_ble_connected(true);
                    ui::display::draw_connected(&mut display, name.as_str());
                    info!("UI: connected to {}", name.as_str());
                }

                BleEvent::Disconnected => {
                    sleep_banner_drawn = false;
                    screen = Screen::Home;
                    devices.clear();
                    selected = 0;
                    device_count = 0;
                    connected_name.clear();
                    power.set_ble_connected(false);
                    ui::display::draw_home(&mut display, false, "");
                    info!("UI: disconnected");
                }

                BleEvent::Error(tag) => {
                    sleep_banner_drawn = false;
                    screen = Screen::Error;
                    let msg = match tag {
                        ble::BleErrorTag::ScanFailed => "Scan failed",
                        ble::BleErrorTag::ConnectFailed => "Connect failed",
                        ble::BleErrorTag::HidNotFound => "No HID service",
                        ble::BleErrorTag::NotifyFailed => "Notify failed",
                    };
                    ui::display::draw_error(&mut display, msg);
                }
            },

            embassy_futures::select::Either4::Third(_) => {
                power.tick();
                let _ = power.state();
                let _ = power.ble_low_power();
                if !power.display_on() && screen == Screen::Home {
                    if !sleep_banner_drawn {
                        ui::display::draw_home(&mut display, false, "Sleeping");
                        sleep_banner_drawn = true;
                    }
                } else {
                    sleep_banner_drawn = false;
                }
            }

            embassy_futures::select::Either4::Fourth(suspended) => {
                power.set_usb_suspended(suspended);
                if !suspended {
                    sleep_banner_drawn = false;
                }
            }
        }
    }
}
