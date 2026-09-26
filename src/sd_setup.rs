//! SoftDevice setup shared by the firmware (`main.rs`) and the on-board
//! self-test (`selftest.rs`), so both bring the BLE stack up identically.

use crate::config;
use defmt::{info, warn};

/// SoftDevice configuration: two central links (keyboard + mouse), no
/// advertising or peripheral role. The RAM this needs is logged by
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
            conn_count: 2,
            event_length: config::BLE_CONN_EVENT_LENGTH,
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
