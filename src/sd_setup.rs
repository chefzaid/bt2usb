//! SoftDevice setup shared by the firmware (`main.rs`) and the on-board
//! self-test (`selftest.rs`), so both bring the BLE stack up identically.

use crate::config;
use defmt::{info, warn};

/// The link count as the SoftDevice's `u8` connection and role counts.
const LINKS: u8 = {
    assert!(config::BLE_MAX_CONNECTIONS <= u8::MAX as usize);
    config::BLE_MAX_CONNECTIONS as u8
};

/// The ATT MTU each link offers in the MTU exchange.
const ATT_MTU: u16 = 64;

/// Bytes the vendored `nrf-softdevice` gives `sd_ble_evt_get` for one event,
/// set by its `evt-max-size-256` feature in `Cargo.toml`.
const BLE_EVT_BUFFER: usize = 256;

// Nordic's `BLE_EVT_LEN_MAX(ATT_MTU)` (S140 `ble.h`): the longest event at this
// MTU, a primary service discovery response carrying one service for every 4
// bytes of an MTU-sized ATT PDU (132 bytes at 64). A peer chooses that count,
// and the vendored event loop panics on an event longer than its buffer, so
// the build fails instead if the MTU outgrows the buffer.
const _: () = {
    use core::mem::{offset_of, size_of};
    use nrf_softdevice::raw;
    // Bindgen lays each C union out as a struct whose members all sit at
    // offset 0, so the event's offset is the sum of the enclosing structs'.
    let services = offset_of!(raw::ble_evt_t, evt)
        + offset_of!(raw::ble_gattc_evt_t, params)
        + offset_of!(raw::ble_gattc_evt_prim_srvc_disc_rsp_t, services);
    let longest = services + (ATT_MTU as usize - 1) / 4 * size_of::<raw::ble_gattc_service_t>();
    assert!(longest <= BLE_EVT_BUFFER);
};

/// SoftDevice configuration: [`config::BLE_MAX_CONNECTIONS`] central links
/// (keyboard + mouse), no advertising or peripheral role. The RAM this needs is logged by
/// `Softdevice::enable` ("softdevice RAM: N bytes") and must fit below
/// `ORIGIN(RAM)` in `memory_sd.x`.
pub fn softdevice_config() -> nrf_softdevice::Config {
    nrf_softdevice::Config {
        clock: Some(nrf_softdevice::raw::nrf_clock_lf_cfg_t {
            source: nrf_softdevice::raw::NRF_CLOCK_LF_SRC_RC as u8,
            rc_ctiv: 16,
            rc_temp_ctiv: 2,
            accuracy: nrf_softdevice::raw::NRF_CLOCK_LF_ACCURACY_500_PPM as u8,
        }),
        conn_gap: Some(nrf_softdevice::raw::ble_gap_conn_cfg_t {
            conn_count: LINKS,
            event_length: config::BLE_CONN_EVENT_LENGTH,
        }),
        conn_gatt: Some(nrf_softdevice::raw::ble_gatt_conn_cfg_t { att_mtu: ATT_MTU }),
        gap_role_count: Some(nrf_softdevice::raw::ble_gap_cfg_role_count_t {
            adv_set_count: 0,          // we don't advertise
            periph_role_count: 0,      // we don't act as peripheral
            central_role_count: LINKS, // one central role per link
            central_sec_count: LINKS,
            _bitfield_1: nrf_softdevice::raw::ble_gap_cfg_role_count_t::new_bitfield_1(0),
        }),
        ..Default::default()
    }
}

/// Scratch buffer for `sequential-storage` on the SoftDevice flash driver.
///
/// The map writes item data to flash straight from the buffer it is given,
/// including when garbage collection moves items, and `nrf_softdevice::Flash`
/// refuses a source that is not word aligned in RAM
/// (`FlashError::BufferMisaligned`). A plain `[u8; N]` has alignment 1, so
/// whether a save succeeded would depend on where the compiler placed it.
#[repr(C, align(4))]
pub struct FlashBuffer<const N: usize>(pub [u8; N]);

impl<const N: usize> FlashBuffer<N> {
    pub const fn new() -> Self {
        Self([0; N])
    }
}

/// Turn on the SoftDevice's USB power SoC events and read the current USB
/// regulator state.
///
/// The SoftDevice owns the POWER peripheral, and it only reports
/// `PowerUsbDetected` / `PowerUsbPowerReady` / `PowerUsbRemoved` once each is
/// explicitly enabled; without this, `softdevice_task` never sees them and the
/// software VBUS detector stays frozen at its boot value, so an unplug/replug
/// is never noticed. Returns `(vbus_detected, power_ready)` for seeding the
/// detector. Must run after `Softdevice::enable`.
pub fn enable_usb_power_events() -> (bool, bool) {
    use nrf_softdevice::raw;
    // SAFETY: plain SoftDevice SVCs, valid once the SoftDevice is enabled.
    unsafe {
        let ok = raw::sd_power_usbdetected_enable(1) == raw::NRF_SUCCESS
            && raw::sd_power_usbremoved_enable(1) == raw::NRF_SUCCESS
            && raw::sd_power_usbpwrrdy_enable(1) == raw::NRF_SUCCESS;
        if !ok {
            warn!("failed to enable USB power events; assuming VBUS present");
            return (true, true);
        }
        let mut status: u32 = 0;
        if raw::sd_power_usbregstatus_get(&mut status) != raw::NRF_SUCCESS {
            return (true, true);
        }
        // USBREGSTATUS: bit 0 = VBUSDETECT, bit 1 = OUTPUTRDY.
        let detected = status & 0b01 != 0;
        let ready = status & 0b10 != 0;
        info!("USB power: vbus={} ready={}", detected, ready);
        (detected, ready)
    }
}
