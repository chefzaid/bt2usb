//! USB HID composite device - keyboard + mouse.
//!
//! Initialises the Embassy USB stack on the nRF52840 hardware USB
//! peripheral and exposes two HID endpoints.

use crate::config;
use crate::hid::consumer::CONSUMER_REPORT_DESCRIPTOR;
use crate::hid::keyboard::KEYBOARD_REPORT_DESCRIPTOR;
use crate::hid::mouse::MOUSE_REPORT_DESCRIPTOR;
use crate::hid::HidReport;
use defmt::{info, warn};
use embassy_nrf::usb::vbus_detect::HardwareVbusDetect;
use embassy_nrf::usb::Driver;
use embassy_nrf::{self, bind_interrupts, peripherals};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Receiver;
use embassy_sync::signal::Signal;
use embassy_usb::class::hid::{Config as HidConfig, HidWriter, State};
use embassy_usb::{Builder, Config, UsbDevice};
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    USBD => embassy_nrf::usb::InterruptHandler<peripherals::USBD>;
    CLOCK_POWER => embassy_nrf::usb::vbus_detect::InterruptHandler;
});

static KB_STATE: StaticCell<State> = StaticCell::new();
static MOUSE_STATE: StaticCell<State> = StaticCell::new();
static CONSUMER_STATE: StaticCell<State> = StaticCell::new();
static USB_CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
static USB_BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
static USB_MSOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
static USB_CTRL_BUF: StaticCell<[u8; 128]> = StaticCell::new();
static USB_POWER_HANDLER: StaticCell<UsbPowerHandler> = StaticCell::new();
static USB_SUSPEND_SIGNAL: Signal<CriticalSectionRawMutex, bool> = Signal::new();

struct UsbPowerHandler;

impl embassy_usb::Handler for UsbPowerHandler {
    fn suspended(&mut self, suspended: bool) {
        USB_SUSPEND_SIGNAL.signal(suspended);
    }
}

/// USB bus suspend/resume signal.
///
/// Emits `true` when the host suspends the bus and `false` when resumed.
pub fn suspend_signal() -> &'static Signal<CriticalSectionRawMutex, bool> {
    &USB_SUSPEND_SIGNAL
}

/// Build result containing the USB device runner and the two HID writers.
pub struct UsbHidDevice {
    pub device: UsbDevice<'static, Driver<'static, peripherals::USBD, HardwareVbusDetect>>,
    pub keyboard_writer:
        HidWriter<'static, Driver<'static, peripherals::USBD, HardwareVbusDetect>, 8>,
    pub mouse_writer: HidWriter<'static, Driver<'static, peripherals::USBD, HardwareVbusDetect>, 8>,
    pub consumer_writer:
        HidWriter<'static, Driver<'static, peripherals::USBD, HardwareVbusDetect>, 8>,
}

/// Initialise the USB stack and create the composite HID device.
///
/// Must be called exactly once.  All static buffers are consumed here.
pub fn init(usbd: peripherals::USBD) -> UsbHidDevice {
    // Create the low-level USB driver with hardware VBUS detection.
    let driver = Driver::new(usbd, Irqs, HardwareVbusDetect::new(Irqs));

    // USB device-level configuration.
    let mut usb_config = Config::new(config::USB_VID, config::USB_PID);
    usb_config.manufacturer = Some(config::USB_MANUFACTURER);
    usb_config.product = Some(config::USB_PRODUCT);
    usb_config.serial_number = Some(config::USB_SERIAL_NUMBER);
    usb_config.max_power = 100; // mA
    usb_config.max_packet_size_0 = 64;

    // Allocate static descriptor buffers.
    let config_desc = USB_CONFIG_DESC.init([0u8; 256]);
    let bos_desc = USB_BOS_DESC.init([0u8; 256]);
    let msos_desc = USB_MSOS_DESC.init([0u8; 256]);
    let ctrl_buf = USB_CTRL_BUF.init([0u8; 128]);

    // Build the USB device.
    let mut builder = Builder::new(
        driver,
        usb_config,
        config_desc,
        bos_desc,
        msos_desc,
        ctrl_buf,
    );

    let usb_handler = USB_POWER_HANDLER.init(UsbPowerHandler);
    builder.handler(usb_handler);

    let kb_state = KB_STATE.init(State::new());
    let kb_config = HidConfig {
        report_descriptor: KEYBOARD_REPORT_DESCRIPTOR,
        request_handler: None,
        poll_ms: config::USB_HID_POLL_MS,
        max_packet_size: 8,
    };
    let keyboard_writer = HidWriter::new(&mut builder, kb_state, kb_config);

    let mouse_state = MOUSE_STATE.init(State::new());
    let mouse_config = HidConfig {
        report_descriptor: MOUSE_REPORT_DESCRIPTOR,
        request_handler: None,
        poll_ms: config::USB_HID_POLL_MS,
        max_packet_size: 8,
    };
    let mouse_writer = HidWriter::new(&mut builder, mouse_state, mouse_config);

    let consumer_state = CONSUMER_STATE.init(State::new());
    let consumer_config = HidConfig {
        report_descriptor: CONSUMER_REPORT_DESCRIPTOR,
        request_handler: None,
        poll_ms: config::USB_HID_POLL_MS,
        max_packet_size: 8,
    };
    let consumer_writer = HidWriter::new(&mut builder, consumer_state, consumer_config);

    let device = builder.build();

    info!("USB HID composite device initialised (keyboard + mouse)");

    UsbHidDevice {
        device,
        keyboard_writer,
        mouse_writer,
        consumer_writer,
    }
}

/// Run the USB device stack - must be spawned as a dedicated Embassy task.
///
/// This handles USB enumeration, suspend/resume, and endpoint servicing.
/// It runs forever (or until the USB cable is disconnected).
pub async fn run_usb_device(
    mut device: UsbDevice<'static, Driver<'static, peripherals::USBD, HardwareVbusDetect>>,
) -> ! {
    info!("USB device task started");
    device.run().await
}

/// HID report forwarding task - reads from the BLE→USB channel and
/// writes to the appropriate USB HID endpoint.
pub async fn hid_writer_task(
    mut keyboard: HidWriter<'static, Driver<'static, peripherals::USBD, HardwareVbusDetect>, 8>,
    mut mouse: HidWriter<'static, Driver<'static, peripherals::USBD, HardwareVbusDetect>, 8>,
    mut consumer: HidWriter<'static, Driver<'static, peripherals::USBD, HardwareVbusDetect>, 8>,
    report_rx: &Receiver<'static, CriticalSectionRawMutex, HidReport, 16>,
) -> ! {
    info!("HID writer task started - waiting for reports");

    let mut buf = [0u8; 8];

    loop {
        let report = report_rx.receive().await;

        match &report {
            HidReport::Keyboard(kb) => {
                let n = kb.serialize(&mut buf);
                if let Err(_e) = keyboard.write(&buf[..n]).await {
                    warn!("USB keyboard write failed");
                }
            }
            HidReport::Mouse(m) => {
                let n = m.serialize(&mut buf);
                if let Err(_e) = mouse.write(&buf[..n]).await {
                    warn!("USB mouse write failed");
                }
            }
            HidReport::Consumer(c) => {
                let n = c.serialize(&mut buf);
                if let Err(_e) = consumer.write(&buf[..n]).await {
                    warn!("USB consumer write failed");
                }
            }
        }
    }
}
