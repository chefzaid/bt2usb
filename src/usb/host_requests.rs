//! Requests the USB host sends to the keyboard and mouse interfaces: the boot
//! or report protocol (SET_PROTOCOL) and the keyboard's LED output report
//! (SET_REPORT).
//!
//! The protocol decides the report layout the endpoint workers in
//! [`hid_device`](super::hid_device) send, and a change replays the
//! interface's held state in the new layout. The LED state is published to the
//! BLE connection slots, which forward it to a BLE keyboard
//! ([`crate::hid::host_leds`]). A USB bus reset returns both interfaces to
//! report protocol and the LEDs to all off ([`reset`]).

use super::hid_device::{KEYBOARD_DELIVERY, MOUSE_DELIVERY};
use crate::hid::host_leds::HostLeds;
use crate::hid::keyboard::KeyboardLeds;
use core::future::Future;
use core::sync::atomic::{AtomicBool, Ordering};
use defmt::info;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::{Receiver as WatchReceiver, Watch};
use embassy_usb::class::hid::{HidProtocolMode, ReportId, RequestHandler};
use embassy_usb::control::OutResponse;
use static_cell::StaticCell;

/// Number of BLE connection slots that consume host LED updates, one per link
/// ([`crate::config::BLE_MAX_CONNECTIONS`]). A keyboard occupies one slot;
/// mouse and consumer-control slots ignore the updates.
pub const LED_CONSUMERS: usize = crate::config::BLE_MAX_CONNECTIONS;

/// Latest host keyboard-LED (Caps/Num/Scroll) state, published by the USB
/// control handler and consumed by the BLE slot tasks to drive the BLE
/// keyboard's LEDs. `Watch` keeps only the newest value and wakes every slot.
static KEYBOARD_LEDS: Watch<CriticalSectionRawMutex, KeyboardLeds, LED_CONSUMERS> = Watch::new();

/// Receiver handle a BLE slot task uses to observe host LED changes. A slot
/// keeps its receiver across links, so the receiver remembers which state the
/// slot last saw; [`HostLeds::current`] hands a new link the state anyway.
pub type LedReceiver = WatchReceiver<'static, CriticalSectionRawMutex, KeyboardLeds, LED_CONSUMERS>;

impl HostLeds for LedReceiver {
    fn current(&mut self) -> Option<KeyboardLeds> {
        self.try_get()
    }

    fn changed(&mut self) -> impl Future<Output = KeyboardLeds> {
        // Call the receiver's own method, not this trait's.
        (**self).changed()
    }
}

/// Take one of the [`LED_CONSUMERS`] LED receivers (one per BLE slot).
pub fn keyboard_led_receiver() -> Option<LedReceiver> {
    KEYBOARD_LEDS.receiver()
}

/// Boot-protocol negotiation for keyboard/mouse, plus keyboard LED output.
pub(super) struct BootRequestHandler {
    keyboard: bool,
}

impl BootRequestHandler {
    fn boot_state(&self) -> &'static AtomicBool {
        if self.keyboard {
            &KEYBOARD_BOOT_PROTOCOL
        } else {
            &MOUSE_BOOT_PROTOCOL
        }
    }
}

impl RequestHandler for BootRequestHandler {
    fn get_protocol(&self) -> HidProtocolMode {
        if self.boot_state().load(Ordering::Relaxed) {
            HidProtocolMode::Boot
        } else {
            HidProtocolMode::Report
        }
    }

    fn set_protocol(&mut self, protocol: HidProtocolMode) -> OutResponse {
        self.boot_state()
            .store(protocol == HidProtocolMode::Boot, Ordering::Relaxed);
        if self.keyboard {
            KEYBOARD_DELIVERY.replay();
        } else {
            MOUSE_DELIVERY.replay();
        }
        OutResponse::Accepted
    }

    fn set_report(&mut self, id: ReportId, data: &[u8]) -> OutResponse {
        // Our keyboard descriptor declares no report IDs, so the output report
        // payload is the single LED bitfield byte.
        if !self.keyboard || id != ReportId::Out(0) || data.len() != 1 {
            return OutResponse::Rejected;
        }
        let leds = KeyboardLeds::from_byte(data[0]);
        info!(
            "Host LEDs: num={} caps={} scroll={}",
            leds.num_lock(),
            leds.caps_lock(),
            leds.scroll_lock()
        );
        KEYBOARD_LEDS.sender().send(leds);
        OutResponse::Accepted
    }
}

static KEYBOARD_HANDLER: StaticCell<BootRequestHandler> = StaticCell::new();
static MOUSE_HANDLER: StaticCell<BootRequestHandler> = StaticCell::new();
static KEYBOARD_BOOT_PROTOCOL: AtomicBool = AtomicBool::new(false);
static MOUSE_BOOT_PROTOCOL: AtomicBool = AtomicBool::new(false);

/// The keyboard interface's request handler; call once, when building the
/// USB device.
pub(super) fn keyboard_handler() -> &'static mut BootRequestHandler {
    KEYBOARD_HANDLER.init(BootRequestHandler { keyboard: true })
}

/// The mouse interface's request handler; call once, when building the USB
/// device.
pub(super) fn mouse_handler() -> &'static mut BootRequestHandler {
    MOUSE_HANDLER.init(BootRequestHandler { keyboard: false })
}

/// Whether the host selected the boot protocol for the mouse interface, so
/// mouse reports go out in the three-byte boot layout.
pub(super) fn mouse_boot_protocol() -> bool {
    MOUSE_BOOT_PROTOCOL.load(Ordering::Relaxed)
}

/// A USB bus reset or a disabled bus: both interfaces return to report
/// protocol, and the LED state to all off until the host sends its own.
pub(super) fn reset() {
    KEYBOARD_BOOT_PROTOCOL.store(false, Ordering::Relaxed);
    MOUSE_BOOT_PROTOCOL.store(false, Ordering::Relaxed);
    KEYBOARD_LEDS.sender().send(KeyboardLeds::default());
}
