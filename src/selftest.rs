//! # bt2usb-selftest — on-board bring-up check
//!
//! A separate firmware image to flash **before** the real one on a new board
//! (see `docs/first-flash.md`). It brings up each piece of hardware the way
//! the real firmware does — same SoftDevice config, same USB composite device,
//! same pins — and reports one PASS / FAIL / SKIP line per stage over RTT:
//!
//! 1. SoftDevice enable (its RAM requirement is logged just above)
//! 2. Flash storage round-trip in the pairing region, without touching the
//!    saved pairings
//! 3. USB enumeration by the PC, then one idle mouse report on the IN endpoint
//! 4. OLED ACK on I²C, then the Home screen
//! 5. Buttons: idle level, then one press of each within a time limit
//! 6. BLE radio: an 8 s scan, listing HID devices heard
//! 7. Stack high-water mark
//!
//! Run it with `mask selftest`. It never types anything on the PC: the only
//! HID report it sends is an all-zero mouse report.

#![no_std]
#![no_main]
// Reuses the firmware's modules without exercising all of them (including
// some of their re-exports).
#![allow(dead_code, unused_imports)]

mod config;
mod hid;
mod power;
mod power_logic;
mod sd_setup;
mod stack;
mod ui;
mod usb;

// The pure BLE files: the advertisement parser for the scan stage, and the
// coordinator and message types the shared `ui` controller speaks. The
// SoftDevice-coupled BLE tasks are not part of this image.
mod ble {
    pub mod adv_parser;
    pub mod coordinator;
    pub mod messages;
}

use defmt::{info, unwrap, warn};
use defmt_rtt as _;
use panic_probe as _;

use embassy_executor::Spawner;
use embassy_nrf::gpio::{AnyPin, Input, Pull};
use embassy_nrf::interrupt::{InterruptExt, Priority};
use embassy_nrf::usb::vbus_detect::SoftwareVbusDetect;
use embassy_nrf::{bind_interrupts, interrupt, peripherals, twim, Peri};
use embassy_time::{with_timeout, Duration, Instant, Timer};
use nrf_softdevice::ble::central;
use nrf_softdevice::{SocEvent, Softdevice};
use sequential_storage::cache::NoCache;
use sequential_storage::map::{MapConfig, MapStorage};

use crate::usb::hid_device;

bind_interrupts!(struct TwimIrqs {
    TWISPI0 => twim::InterruptHandler<peripherals::TWISPI0>;
});

/// SSD1306 7-bit I²C address (0x3C on nearly every 128×64 module; 0x3D if its
/// address jumper is moved).
const OLED_ADDR: u8 = 0x3C;
/// Map key for the flash round-trip record. The real firmware only uses key
/// 0x01 (paired devices), and this record is removed again afterwards.
const SELFTEST_KEY: u8 = 0xFE;
/// How long to wait for the PC to enumerate the device.
const USB_ENUM_TIMEOUT: Duration = Duration::from_secs(10);
/// How long to wait for each button press.
const BUTTON_TIMEOUT: Duration = Duration::from_secs(20);
/// BLE scan window.
const SCAN_TIME: Duration = Duration::from_secs(8);

#[derive(Default)]
struct Tally {
    passed: u8,
    failed: u8,
    skipped: u8,
}

impl Tally {
    fn pass(&mut self, stage: &str, detail: &str) {
        self.passed += 1;
        info!("[PASS] {}: {}", stage, detail);
    }
    fn fail(&mut self, stage: &str, detail: &str) {
        self.failed += 1;
        warn!("[FAIL] {}: {}", stage, detail);
    }
    fn skip(&mut self, stage: &str, detail: &str) {
        self.skipped += 1;
        warn!("[SKIP] {}: {}", stage, detail);
    }
}

#[embassy_executor::task]
async fn softdevice_task(sd: &'static Softdevice, vbus: &'static SoftwareVbusDetect) -> ! {
    sd.run_with_callback(|event| match event {
        SocEvent::PowerUsbDetected => vbus.detected(true),
        SocEvent::PowerUsbRemoved => vbus.detected(false),
        SocEvent::PowerUsbPowerReady => vbus.ready(),
        _ => {}
    })
    .await
}

#[embassy_executor::task]
async fn usb_device_task(device: embassy_usb::UsbDevice<'static, hid_device::UsbDriver>) -> ! {
    hid_device::run_usb_device(device).await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("==== bt2usb self-test ====");

    // Same interrupt priorities as the firmware: the SoftDevice reserves 0, 1, 4.
    let mut nrf_config = embassy_nrf::config::Config::default();
    nrf_config.gpiote_interrupt_priority = Priority::P2;
    nrf_config.time_interrupt_priority = Priority::P2;
    let p = embassy_nrf::init(nrf_config);
    interrupt::USBD.set_priority(Priority::P2);
    interrupt::TWISPI0.set_priority(Priority::P2);

    let mut tally = Tally::default();

    // 1. SoftDevice. `enable` panics with the RAM start address it needs if
    //    `memory_sd.x` reserves too little, so reaching the next line is the pass.
    let sd = Softdevice::enable(&sd_setup::softdevice_config());
    tally.pass(
        "softdevice",
        "enabled (the 'softdevice RAM' line above is what memory_sd.x must reserve)",
    );

    let (vbus_detected, usb_power_ready) = sd_setup::enable_usb_power_events();
    let mut usb = hid_device::init(p.USBD, vbus_detected, usb_power_ready);
    spawner.spawn(unwrap!(softdevice_task(sd, usb.vbus)));

    // 2. Flash.
    check_flash(sd, &mut tally).await;

    // 3. USB.
    spawner.spawn(unwrap!(usb_device_task(usb.device)));
    if !vbus_detected {
        info!("USB: no VBUS yet; plug the nRF USB port (not the debugger port) into the PC");
    }
    let deadline = Instant::now() + USB_ENUM_TIMEOUT;
    while !hid_device::is_configured() && Instant::now() < deadline {
        Timer::after(Duration::from_millis(100)).await;
    }
    if hid_device::is_configured() {
        tally.pass("usb enumeration", "configured by the PC");
        // An all-zero mouse report: no buttons, no motion. Proves the HID IN
        // endpoint moves data without the PC seeing any input.
        let idle_mouse = [0u8; hid::mouse::MOUSE_REPORT_SIZE];
        match with_timeout(Duration::from_secs(1), usb.mouse_writer.write(&idle_mouse)).await {
            Ok(Ok(())) => tally.pass("usb hid report", "idle mouse report accepted"),
            Ok(Err(_)) => tally.fail("usb hid report", "endpoint write failed"),
            Err(_) => tally.fail("usb hid report", "host never polled the endpoint (1 s)"),
        }
    } else {
        tally.fail(
            "usb enumeration",
            "not configured within 10 s: check the nRF USB cable/port and the PC's device list",
        );
        tally.skip("usb hid report", "needs enumeration");
    }

    // 4. OLED. Probe the address, then check initialization and framebuffer I/O.
    let mut twi =
        ui::display::StopSafeI2c::new(ui::display::new_twim(p.TWISPI0, TwimIrqs, p.P0_26, p.P0_27));
    // Control byte 0x00 (command stream) + 0xAE (display off): harmless, and
    // the panel is re-initialised right after. A missing panel NACKs here.
    let probe = embedded_hal_async::i2c::I2c::write(&mut twi, OLED_ADDR, &[0x00, 0xAE]);
    match ui::display::finish_or_stop(probe).await {
        Ok(()) => {
            tally.pass("oled i2c", "SSD1306 acknowledged at 0x3C");
            match ui::display::finish_or_stop(ui::display::init(twi)).await {
                Ok(mut display) => {
                    if ui::display::finish_or_stop(ui::display::draw_home(&mut display, false, ""))
                        .await
                        .is_ok()
                    {
                        tally.pass("oled render", "initialization and Home framebuffer sent");
                    } else {
                        tally.fail("oled render", "framebuffer transfer failed");
                    }
                }
                Err(_) => tally.fail("oled render", "SSD1306 initialization failed"),
            }
        }
        Err(_) => tally.fail(
            "oled i2c",
            "no ACK at 0x3C: check SDA=P0.26, SCL=P0.27, VCC, GND (or a 0x3D module)",
        ),
    }

    // 5. Buttons.
    check_button("button UP (P0.11)", p.P0_11.into(), &mut tally).await;
    check_button("button DOWN (P0.12)", p.P0_12.into(), &mut tally).await;
    check_button("button SELECT (P0.24)", p.P0_24.into(), &mut tally).await;

    // 6. BLE radio.
    check_ble_scan(sd, &mut tally).await;

    // 7. Stack.
    let (used, total) = stack::high_water();
    info!("stack high-water: {} of {} bytes", used, total);
    if used * 2 < total {
        tally.pass("stack", "under half the stack region used");
    } else {
        tally.fail("stack", "over half the stack region used");
    }

    info!(
        "==== self-test done: {} passed, {} failed, {} skipped ====",
        tally.passed, tally.failed, tally.skipped
    );
    loop {
        Timer::after(Duration::from_secs(3600)).await;
    }
}

/// Store, read back and remove a scratch record in the pairing region through
/// the same `sequential-storage` map the firmware uses. Saved pairings (key
/// 0x01) are only read.
async fn check_flash(sd: &Softdevice, tally: &mut Tally) {
    // Own the flash driver: MultiwriteNorFlash is not implemented for &mut
    // Flash, and record removal requires that trait. The range check in
    // `MapConfig::new` runs at compile time.
    let flash = nrf_softdevice::Flash::take(sd);
    let map_config =
        const { MapConfig::new(config::STORAGE_FLASH_START..config::STORAGE_FLASH_END) };
    let mut map = MapStorage::<u8, _, _>::new(flash, map_config, NoCache);
    // The scratch buffer must also fit the existing pairing blob, not just
    // our 16-byte test record, and be word aligned for the flash driver.
    let mut flash_buffer = sd_setup::FlashBuffer::<1024>::new();
    let buf = &mut flash_buffer.0;

    match map.fetch_item::<&[u8]>(buf, &0x01).await {
        Ok(Some(saved)) => info!(
            "flash: saved pairing record present ({} bytes)",
            saved.len()
        ),
        Ok(None) => info!("flash: no saved pairings yet"),
        Err(_) => {
            tally.fail("flash", "reading the pairing region failed");
            return;
        }
    }

    let mut pattern = [0u8; 16];
    for (i, b) in pattern.iter_mut().enumerate() {
        *b = 0xA5 ^ (i as u8).wrapping_mul(37);
    }
    if map
        .store_item::<&[u8]>(buf, &SELFTEST_KEY, &&pattern[..])
        .await
        .is_err()
    {
        tally.fail("flash", "write failed");
        return;
    }
    let matches = matches!(
        map.fetch_item::<&[u8]>(buf, &SELFTEST_KEY).await,
        Ok(Some(read)) if read == pattern
    );
    let removed = map.remove_item(buf, &SELFTEST_KEY).await.is_ok();
    match (matches, removed) {
        (true, true) => tally.pass("flash", "write, read-back and remove OK"),
        (false, _) => tally.fail("flash", "read-back didn't match what was written"),
        (true, false) => tally.fail("flash", "remove failed"),
    }
}

/// Buttons are active-low with the internal pull-up: at rest the pin must read
/// high, then a press must pull it low.
async fn check_button(name: &str, pin: Peri<'static, AnyPin>, tally: &mut Tally) {
    let mut btn = Input::new(pin, Pull::Up);
    // Let the pull-up settle before sampling.
    Timer::after(Duration::from_millis(5)).await;
    if btn.is_low() {
        tally.fail(name, "reads pressed at rest: shorted to GND or wrong pin");
        return;
    }
    info!(">>> press {} now ({} s)", name, BUTTON_TIMEOUT.as_secs());
    match with_timeout(BUTTON_TIMEOUT, btn.wait_for_low()).await {
        Ok(()) => {
            tally.pass(name, "press detected");
            // Wait for release so the next prompt isn't satisfied by this press.
            let _ = with_timeout(Duration::from_secs(5), btn.wait_for_high()).await;
            Timer::after(Duration::from_millis(config::BUTTON_DEBOUNCE_MS)).await;
        }
        Err(_) => tally.skip(name, "no press seen: check wiring to GND"),
    }
}

/// Scan like the firmware does and report what the radio hears.
async fn check_ble_scan(sd: &Softdevice, tally: &mut Tally) {
    info!(
        "BLE: scanning {} s; put a keyboard or mouse in pairing mode to see it listed",
        SCAN_TIME.as_secs()
    );
    let mut adverts: u32 = 0;
    let mut hid_seen: heapless::Vec<nrf_softdevice::ble::Address, 8> = heapless::Vec::new();
    let scan_config = central::ScanConfig {
        active: true,
        ..Default::default()
    };
    let scan = central::scan(sd, &scan_config, |params| {
        adverts += 1;
        // SAFETY: the SoftDevice guarantees `p_data`/`len` describe the report.
        let data =
            unsafe { core::slice::from_raw_parts(params.data.p_data, params.data.len as usize) };
        if ble::adv_parser::contains_hid_service_uuid(data) {
            let addr = nrf_softdevice::ble::Address::from_raw(params.peer_addr);
            if !hid_seen.contains(&addr) && hid_seen.push(addr).is_ok() {
                let name = ble::adv_parser::extract_device_name(data);
                info!("BLE: HID device '{}' (RSSI {})", name.as_str(), params.rssi);
            }
        }
        None::<()>
    });
    match with_timeout(SCAN_TIME, scan).await {
        // Timed out = scanned the whole window.
        Err(_) => {}
        Ok(Ok(())) => {}
        Ok(Err(_)) => {
            tally.fail("ble scan", "SoftDevice refused to scan");
            return;
        }
    }
    info!(
        "BLE: {} advertisements, {} HID devices",
        adverts,
        hid_seen.len()
    );
    if adverts > 0 {
        tally.pass("ble scan", "radio receives advertisements");
    } else {
        tally.fail(
            "ble scan",
            "heard nothing in 8 s: check the antenna, or test near any BLE device",
        );
    }
}
