//! USB HID composite device - keyboard + mouse + consumer control.
//!
//! Initialises the Embassy USB stack on the nRF52840 hardware USB
//! peripheral and exposes keyboard, mouse, and consumer-control HID endpoints.

use crate::config;
use crate::hid::consumer::CONSUMER_REPORT_DESCRIPTOR;
use crate::hid::keyboard::{KeyboardLeds, KEYBOARD_REPORT_DESCRIPTOR};
use crate::hid::mouse::MOUSE_REPORT_DESCRIPTOR;
use crate::hid::HidReport;
use defmt::{info, warn};
use embassy_nrf::usb::vbus_detect::SoftwareVbusDetect;
use embassy_nrf::usb::Driver;
use embassy_nrf::{self, bind_interrupts, peripherals, Peri};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Receiver;
use embassy_sync::signal::Signal;
use embassy_sync::watch::{Receiver as WatchReceiver, Watch};
use embassy_usb::class::hid::{
    Config as HidConfig, HidBootProtocol, HidSubclass, HidWriter, ReportId, RequestHandler, State,
};
use embassy_usb::control::OutResponse;
use embassy_usb::{Builder, Config, UsbDevice};
use static_cell::StaticCell;

/// Number of BLE connection slots that may consume host LED updates. Must be ≥
/// the BLE `MAX_CONNECTIONS` (a keyboard occupies one slot; mice/consumer slots
/// simply ignore the updates).
pub const LED_CONSUMERS: usize = 2;

/// Latest host keyboard-LED (Caps/Num/Scroll) state, published by the USB
/// control handler and consumed by the BLE slot tasks to drive the BLE
/// keyboard's LEDs. `Watch` keeps only the newest value and wakes every slot.
static KEYBOARD_LEDS: Watch<CriticalSectionRawMutex, KeyboardLeds, LED_CONSUMERS> = Watch::new();

/// Receiver handle a BLE slot task uses to observe host LED changes.
pub type LedReceiver = WatchReceiver<'static, CriticalSectionRawMutex, KeyboardLeds, LED_CONSUMERS>;

/// Take one of the [`LED_CONSUMERS`] LED receivers (one per BLE slot).
pub fn keyboard_led_receiver() -> Option<LedReceiver> {
    KEYBOARD_LEDS.receiver()
}

/// USB control handler that captures the host's keyboard LED **output** report
/// (sent via SET_REPORT on the control pipe) and republishes it for the BLE
/// side. Installed only on the keyboard interface.
struct LedRequestHandler;

impl RequestHandler for LedRequestHandler {
    fn set_report(&mut self, _id: ReportId, data: &[u8]) -> OutResponse {
        // Our keyboard descriptor declares no report IDs, so the output report
        // payload is the single LED bitfield byte.
        if let Some(&byte) = data.first() {
            let leds = KeyboardLeds::from_byte(byte);
            info!(
                "Host LEDs: num={} caps={} scroll={}",
                leds.num_lock(),
                leds.caps_lock(),
                leds.scroll_lock()
            );
            KEYBOARD_LEDS.sender().send(leds);
        }
        OutResponse::Accepted
    }
}

static LED_HANDLER: StaticCell<LedRequestHandler> = StaticCell::new();

bind_interrupts!(struct Irqs {
    USBD => embassy_nrf::usb::InterruptHandler<peripherals::USBD>;
});

/// VBUS detection source.
///
/// The Nordic SoftDevice owns the POWER peripheral and its `POWER_CLOCK`
/// interrupt, so the application may **not** use `HardwareVbusDetect` (which
/// would register a conflicting `CLOCK_POWER` handler and touch POWER
/// registers reserved by the SoftDevice). Instead we use a software detector
/// fed by SoftDevice SoC power events (see `software_vbus()` and the
/// `softdevice_task` callback in `main.rs`).
pub type Vbus = &'static SoftwareVbusDetect;

/// Concrete USB driver type used throughout the firmware.
pub type UsbDriver = Driver<'static, peripherals::USBD, Vbus>;

static KB_STATE: StaticCell<State> = StaticCell::new();
static MOUSE_STATE: StaticCell<State> = StaticCell::new();
static CONSUMER_STATE: StaticCell<State> = StaticCell::new();
static USB_CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
static USB_BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
static USB_MSOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
static USB_CTRL_BUF: StaticCell<[u8; 128]> = StaticCell::new();
static USB_POWER_HANDLER: StaticCell<UsbPowerHandler> = StaticCell::new();
static USB_SUSPEND_SIGNAL: Signal<CriticalSectionRawMutex, bool> = Signal::new();
static SOFTWARE_VBUS: StaticCell<SoftwareVbusDetect> = StaticCell::new();

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

/// Build result containing the USB device runner, HID writers, and the
/// software VBUS detector that the SoftDevice task must feed with SoC events.
pub struct UsbHidDevice {
    pub device: UsbDevice<'static, UsbDriver>,
    pub keyboard_writer: HidWriter<'static, UsbDriver, 8>,
    pub mouse_writer: HidWriter<'static, UsbDriver, 8>,
    pub consumer_writer: HidWriter<'static, UsbDriver, 8>,
    /// Software VBUS detector — route SoftDevice `SocEvent` power events here.
    pub vbus: Vbus,
}

/// Initialise the USB stack and create the composite HID device.
///
/// Must be called exactly once.  All static buffers are consumed here.
pub fn init(usbd: Peri<'static, peripherals::USBD>) -> UsbHidDevice {
    // The device is bus-powered through the monitor hub, so VBUS is present at
    // boot. Initialise as detected+ready so enumeration can proceed even if the
    // first SoC events were emitted before `softdevice_task` started draining
    // them; subsequent PowerUsbDetected/Removed/PowerReady events keep it
    // accurate across unplug/replug.
    let vbus: Vbus = SOFTWARE_VBUS.init(SoftwareVbusDetect::new(true, true));

    // Create the low-level USB driver with software VBUS detection (SoftDevice
    // owns the POWER peripheral, so HardwareVbusDetect cannot be used).
    let driver = Driver::new(usbd, Irqs, vbus);

    // USB device-level configuration.
    let mut usb_config = Config::new(config::USB_VID, config::USB_PID);
    usb_config.manufacturer = Some(config::USB_MANUFACTURER);
    usb_config.product = Some(config::USB_PRODUCT);
    usb_config.serial_number = Some(config::USB_SERIAL_NUMBER);
    usb_config.max_power = 100; // mA
    usb_config.max_packet_size_0 = 64;
    // Advertise remote-wakeup capability so a host that has suspended the bus
    // (e.g. PC asleep) permits the device to request wake. Emitting the wake
    // signal on incoming HID activity is still unimplemented, but no longer
    // blocked: embassy-usb 0.6 exposes `UsbDevice::remote_wakeup()` for that —
    // it just needs routing a wake trigger to `usb_device_task`.
    usb_config.supports_remote_wakeup = true;

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
        // Capture the host's LED (Caps/Num/Scroll) output report so we can mirror
        // it onto the BLE keyboard.
        request_handler: Some(LED_HANDLER.init(LedRequestHandler)),
        poll_ms: config::USB_HID_POLL_MS,
        max_packet_size: 8,
        // Advertise the Boot Interface subclass so the keyboard works in BIOS /
        // pre-OS environments (before an OS HID driver loads). Our keyboard
        // report is already boot-protocol compatible (8-byte layout).
        hid_subclass: HidSubclass::Boot,
        hid_boot_protocol: HidBootProtocol::Keyboard,
    };
    let keyboard_writer = HidWriter::new(&mut builder, kb_state, kb_config);

    let mouse_state = MOUSE_STATE.init(State::new());
    let mouse_config = HidConfig {
        report_descriptor: MOUSE_REPORT_DESCRIPTOR,
        request_handler: None,
        poll_ms: config::USB_HID_POLL_MS,
        max_packet_size: 8,
        // Boot mouse subclass for pre-OS use; our 3-byte report is boot compatible.
        hid_subclass: HidSubclass::Boot,
        hid_boot_protocol: HidBootProtocol::Mouse,
    };
    let mouse_writer = HidWriter::new(&mut builder, mouse_state, mouse_config);

    let consumer_state = CONSUMER_STATE.init(State::new());
    let consumer_config = HidConfig {
        report_descriptor: CONSUMER_REPORT_DESCRIPTOR,
        request_handler: None,
        poll_ms: config::USB_HID_POLL_MS,
        max_packet_size: 8,
        // Consumer Control has no boot protocol — only keyboard/mouse do.
        hid_subclass: HidSubclass::No,
        hid_boot_protocol: HidBootProtocol::None,
    };
    let consumer_writer = HidWriter::new(&mut builder, consumer_state, consumer_config);

    let device = builder.build();

    info!("USB HID composite device initialised (keyboard + mouse + consumer)");

    UsbHidDevice {
        device,
        keyboard_writer,
        mouse_writer,
        consumer_writer,
        vbus,
    }
}

/// Run the USB device stack - must be spawned as a dedicated Embassy task.
///
/// This handles USB enumeration, suspend/resume, and endpoint servicing.
/// It runs forever (or until the USB cable is disconnected).
pub async fn run_usb_device(mut device: UsbDevice<'static, UsbDriver>) -> ! {
    info!("USB device task started");
    device.run().await
}

/// HID report forwarding task - reads from the BLE→USB channel and
/// writes to the appropriate USB HID endpoint.
pub async fn hid_writer_task(
    mut keyboard: HidWriter<'static, UsbDriver, 8>,
    mut mouse: HidWriter<'static, UsbDriver, 8>,
    mut consumer: HidWriter<'static, UsbDriver, 8>,
    report_rx: &Receiver<'static, CriticalSectionRawMutex, HidReport, 16>,
) -> ! {
    info!("HID writer task started - waiting for reports");

    let mut buf = [0u8; 8];

    loop {
        let report = report_rx.receive().await;
        let n = report.serialize(&mut buf);
        let bytes = &buf[..n];

        // The variant only selects which USB endpoint receives the report; the
        // wire bytes are produced once by `HidReport::serialize` above.
        let result = match &report {
            HidReport::Keyboard(_) => keyboard.write(bytes).await,
            HidReport::Mouse(_) => mouse.write(bytes).await,
            HidReport::Consumer(_) => consumer.write(bytes).await,
        };
        if result.is_err() {
            warn!("USB HID write failed");
        }
    }
}
