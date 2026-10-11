//! USB HID composite device - keyboard + mouse + consumer control.
//!
//! Initialises the Embassy USB stack on the nRF52840 hardware USB
//! peripheral and exposes keyboard, mouse, and consumer-control HID endpoints.

use super::host_requests;
use crate::config;
use crate::diagnostics::{Counter, COUNTERS};
use crate::hid::aggregate::InputAggregator;
use crate::hid::consumer::{ConsumerReport, CONSUMER_REPORT_DESCRIPTOR};
use crate::hid::delivery::{
    run_endpoint, DeliveryQueue, EndpointDelivery, HidEvent, PendingReport, ReportSink, RetryClock,
};
use crate::hid::keyboard::{KeyboardReport, KEYBOARD_REPORT_DESCRIPTOR};
use crate::hid::mouse::{MouseReport, MOUSE_REPORT_DESCRIPTOR};
use crate::hid::HidReport;
use core::cell::RefCell;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};
use defmt::{info, warn};
use embassy_futures::join::join4;
use embassy_futures::select::{select, Either};
use embassy_nrf::usb::vbus_detect::SoftwareVbusDetect;
use embassy_nrf::usb::Driver;
use embassy_nrf::{self, bind_interrupts, peripherals, Peri};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::channel::Receiver;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};
use embassy_usb::class::hid::{
    Config as HidConfig, HidBootProtocol, HidSubclass, HidWriter, State,
};
use embassy_usb::{Builder, Config, UsbDevice};
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    USBD => embassy_nrf::usb::InterruptHandler<peripherals::USBD>;
});

/// VBUS detection source.
///
/// The Nordic SoftDevice owns the POWER peripheral and its `POWER_CLOCK`
/// interrupt, so the application may **not** use `HardwareVbusDetect` (which
/// would register a conflicting `CLOCK_POWER` handler and touch POWER
/// registers reserved by the SoftDevice). Instead we use a software detector
/// fed by SoftDevice SoC power events (see `SOFTWARE_VBUS` and [`init`] below,
/// and the `softdevice_task` callback in `main.rs`).
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
/// Whether the host has suspended the bus (PC asleep).
static USB_SUSPENDED: AtomicBool = AtomicBool::new(false);
/// Raised by the HID writer when input arrives while suspended; the USB device
/// task answers it with a remote wakeup so a key press wakes the PC.
static REMOTE_WAKE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static SOFTWARE_VBUS: StaticCell<SoftwareVbusDetect> = StaticCell::new();
static USB_SERIAL: StaticCell<heapless::String<16>> = StaticCell::new();

/// Each endpoint has its own bounded queue, current held state, and worker.
/// No USB write or timer is awaited while this short critical section is held.
pub(super) struct EndpointMailbox {
    state: Mutex<CriticalSectionRawMutex, RefCell<EndpointDelivery>>,
    pending: Signal<CriticalSectionRawMutex, ()>,
    lifecycle: Signal<CriticalSectionRawMutex, ()>,
}

impl EndpointMailbox {
    const fn new(initial: HidReport) -> Self {
        Self {
            state: Mutex::new(RefCell::new(EndpointDelivery::new(initial))),
            pending: Signal::new(),
            lifecycle: Signal::new(),
        }
    }

    fn publish(&self, report: HidReport) {
        let overflowed = self
            .state
            .lock(|state| state.borrow_mut().publish(report, usb_available()));
        if overflowed {
            COUNTERS.bump(Counter::EndpointOverflows);
        }
        self.pending.signal(());
    }

    pub(super) fn replay(&self) {
        self.state.lock(|state| state.borrow_mut().replay());
        self.lifecycle.signal(());
        self.pending.signal(());
    }
}

impl DeliveryQueue for EndpointMailbox {
    fn available(&self) -> bool {
        usb_available()
    }

    fn take(&self) -> Option<PendingReport> {
        self.state.lock(|state| state.borrow_mut().take())
    }

    fn start(&self, epoch: u32) -> bool {
        self.lifecycle.reset();
        self.state.lock(|state| state.borrow().is_current(epoch)) && usb_available()
    }

    fn failed(&self, epoch: u32, first_failure: bool) {
        self.state.lock(|state| state.borrow_mut().failed(epoch));
        if first_failure {
            COUNTERS.bump(Counter::UsbWriteFailures);
            warn!("USB HID endpoint unavailable; retaining current input state");
        }
    }

    fn succeeded(&self, epoch: u32) {
        self.state.lock(|state| state.borrow_mut().succeeded(epoch));
    }

    async fn pending(&self) {
        self.pending.wait().await;
    }
    async fn lifecycle(&self) {
        self.lifecycle.wait().await;
    }
}

struct UsbReportSink {
    writer: HidWriter<'static, UsbDriver, 8>,
    mouse: bool,
}

impl ReportSink for UsbReportSink {
    async fn write(&mut self, report: &HidReport) -> Result<(), ()> {
        let mut buf = [0u8; 8];
        let len = match report {
            HidReport::Mouse(mouse) if self.mouse && host_requests::mouse_boot_protocol() => {
                mouse.serialize_boot(&mut buf)
            }
            _ => report.serialize(&mut buf),
        };
        // `serialize` and `serialize_boot` never report more than `buf` holds;
        // a longer length would fail the write.
        let bytes = buf.get(..len).ok_or(())?;
        // Cancellation is safe for these single-packet reports with the pinned
        // embassy-nrf 0.7 HAL: EndpointIn::write only awaits wait_data_ready;
        // afterwards DMA copies synchronously and returns without another
        // suspension point. Recheck this invariant when upgrading the driver.
        self.writer.write(bytes).await.map_err(|_| ())
    }
}

struct UsbRetryClock;

impl RetryClock for UsbRetryClock {
    async fn after_ms(&self, milliseconds: u64) {
        Timer::after(Duration::from_millis(milliseconds)).await;
    }
}

pub(super) static KEYBOARD_DELIVERY: EndpointMailbox =
    EndpointMailbox::new(HidReport::Keyboard(KeyboardReport {
        modifier: 0,
        reserved: 0,
        keycodes: [0; 6],
    }));
pub(super) static MOUSE_DELIVERY: EndpointMailbox =
    EndpointMailbox::new(HidReport::Mouse(MouseReport {
        buttons: 0,
        x: 0,
        y: 0,
        wheel: 0,
        pan: 0,
    }));
static CONSUMER_DELIVERY: EndpointMailbox =
    EndpointMailbox::new(HidReport::Consumer(ConsumerReport { usage: 0 }));

fn usb_available() -> bool {
    USB_CONFIGURED.load(Ordering::Relaxed) && !USB_SUSPENDED.load(Ordering::Relaxed)
}

fn replay_endpoints() {
    KEYBOARD_DELIVERY.replay();
    MOUSE_DELIVERY.replay();
    CONSUMER_DELIVERY.replay();
}

struct UsbPowerHandler;

/// Whether the host has enumerated and configured the device.
static USB_CONFIGURED: AtomicBool = AtomicBool::new(false);

/// True once the host has configured the device (enumeration finished), until
/// it is reset or unplugged.
#[allow(dead_code)] // used by the self-test binary
pub fn is_configured() -> bool {
    USB_CONFIGURED.load(Ordering::Relaxed)
}

impl embassy_usb::Handler for UsbPowerHandler {
    fn reset(&mut self) {
        USB_CONFIGURED.store(false, Ordering::Relaxed);
        USB_SUSPENDED.store(false, Ordering::Relaxed);
        host_requests::reset();
        REMOTE_WAKE.reset();
        USB_SUSPEND_SIGNAL.signal(false);
        replay_endpoints();
    }

    fn enabled(&mut self, enabled: bool) {
        if !enabled {
            embassy_usb::Handler::reset(self);
        }
    }

    fn configured(&mut self, configured: bool) {
        USB_CONFIGURED.store(configured, Ordering::Relaxed);
        replay_endpoints();
        info!("USB configured by host: {}", configured);
    }

    fn suspended(&mut self, suspended: bool) {
        // Start each suspend interval with no stale request. Only new press
        // edges observed after USB_SUSPENDED becomes true can request wake.
        REMOTE_WAKE.reset();
        USB_SUSPENDED.store(suspended, Ordering::Relaxed);
        USB_SUSPEND_SIGNAL.signal(suspended);
        replay_endpoints();
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
/// `vbus_detected` / `power_ready` seed the software VBUS detector with the
/// USB regulator state read from the SoftDevice at boot (see
/// `enable_usb_power_events` in `sd_setup.rs`); the SoftDevice's USB power SoC
/// events keep it accurate afterwards, across unplug/replug.
///
/// Must be called exactly once.  All static buffers are consumed here.
pub fn init(
    usbd: Peri<'static, peripherals::USBD>,
    vbus_detected: bool,
    power_ready: bool,
) -> UsbHidDevice {
    let vbus: Vbus = SOFTWARE_VBUS.init(SoftwareVbusDetect::new(vbus_detected, power_ready));

    // Create the low-level USB driver with software VBUS detection (SoftDevice
    // owns the POWER peripheral, so HardwareVbusDetect cannot be used).
    let driver = Driver::new(usbd, Irqs, vbus);

    // USB device-level configuration.
    let mut usb_config = Config::new(config::USB_VID, config::USB_PID);
    usb_config.manufacturer = Some(config::USB_MANUFACTURER);
    usb_config.product = Some(config::USB_PRODUCT);
    // FICR is factory-programmed and read-only. Both words identify this
    // physical unit consistently across firmware updates and USB ports.
    let serial = USB_SERIAL.init_with(|| {
        let mut serial = heapless::String::<16>::new();
        #[expect(
            clippy::expect_used,
            reason = "`{:08X}` prints a u32 as exactly 8 hex digits, so two fill the 16 bytes of `serial`"
        )]
        write!(
            serial,
            "{:08X}{:08X}",
            embassy_nrf::pac::FICR.deviceid(1).read(),
            embassy_nrf::pac::FICR.deviceid(0).read()
        )
        .expect("two u32 hex words fit in 16 characters");
        serial
    });
    usb_config.serial_number = Some(serial.as_str());
    usb_config.max_power = 100; // mA
    usb_config.max_packet_size_0 = 64;
    // Advertise remote-wakeup capability so a host that has suspended the bus
    // (e.g. PC asleep) lets a key press wake it; see `run_usb_device`.
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

    // embassy-usb 0.6 holds at most four handlers (its default
    // `MAX_HANDLER_COUNT`) and panics at boot on a fifth. This one and the
    // control handler each `HidWriter::new` below adds fill all four, so a new
    // class or handler needs a `max-handler-count-*` feature on embassy-usb.
    let usb_handler = USB_POWER_HANDLER.init(UsbPowerHandler);
    builder.handler(usb_handler);

    let kb_state = KB_STATE.init(State::new());
    let kb_config = HidConfig {
        report_descriptor: KEYBOARD_REPORT_DESCRIPTOR,
        // Capture the host's LED (Caps/Num/Scroll) output report so we can mirror
        // it onto the BLE keyboard.
        request_handler: Some(host_requests::keyboard_handler()),
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
        request_handler: Some(host_requests::mouse_handler()),
        poll_ms: config::USB_HID_POLL_MS,
        max_packet_size: 8,
        // SET_PROTOCOL selects three-byte reports for a boot/BIOS host.
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
    loop {
        device.run_until_suspend().await;
        match select(device.wait_resume(), REMOTE_WAKE.wait()).await {
            Either::First(()) => {}
            Either::Second(()) => match device.remote_wakeup().await {
                Ok(()) => info!("USB remote wakeup sent"),
                // The host didn't enable remote wakeup for us (a per-device OS
                // setting); stay suspended until it resumes the bus itself.
                Err(e) => info!("USB remote wakeup not possible: {}", e),
            },
        }
    }
}

/// Dispatch never waits for an endpoint. The three concurrent workers can
/// each wait for host polling without holding up either of the other two.
pub async fn hid_writer_task(
    keyboard: HidWriter<'static, UsbDriver, 8>,
    mouse: HidWriter<'static, UsbDriver, 8>,
    consumer: HidWriter<'static, UsbDriver, 8>,
    report_rx: &Receiver<'static, CriticalSectionRawMutex, HidEvent, 16>,
) -> ! {
    info!("HID dispatcher and three endpoint workers started");
    // Every future is `-> !`, so the join never completes; its first output
    // has type `!` and serves as this function's result.
    let (never, ..) = join4(
        dispatch_reports(report_rx),
        endpoint_worker(keyboard, &KEYBOARD_DELIVERY, false),
        endpoint_worker(mouse, &MOUSE_DELIVERY, true),
        endpoint_worker(consumer, &CONSUMER_DELIVERY, false),
    )
    .await;
    never
}

async fn dispatch_reports(
    report_rx: &Receiver<'static, CriticalSectionRawMutex, HidEvent, 16>,
) -> ! {
    let mut aggregate = InputAggregator::default();
    loop {
        let event = report_rx.receive().await;
        if matches!(event, HidEvent::Report { .. }) {
            crate::power::note_hid_activity();
        }
        let update = aggregate.apply(event);
        if update.wake && USB_SUSPENDED.load(Ordering::Relaxed) {
            REMOTE_WAKE.signal(());
        }
        for report in update.reports {
            match &report {
                HidReport::Keyboard(_) => KEYBOARD_DELIVERY.publish(report),
                HidReport::Mouse(_) => MOUSE_DELIVERY.publish(report),
                HidReport::Consumer(_) => CONSUMER_DELIVERY.publish(report),
            }
        }
    }
}

async fn endpoint_worker(
    writer: HidWriter<'static, UsbDriver, 8>,
    mailbox: &'static EndpointMailbox,
    mouse: bool,
) -> ! {
    run_endpoint(
        mailbox,
        &mut UsbReportSink { writer, mouse },
        &UsbRetryClock,
    )
    .await
}
