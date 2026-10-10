//! Bluetooth Low Energy

mod connection;
mod gap;
mod gatt_traits;
mod replies;
mod types;

pub use connection::*;
pub use gap::*;
pub use gatt_traits::*;
pub use types::*;

mod common;

#[cfg(feature = "ble-sec")]
pub mod security;

#[cfg(feature = "ble-central")]
pub mod central;

#[cfg(feature = "ble-peripheral")]
pub mod advertisement_builder;
#[cfg(feature = "ble-peripheral")]
pub mod peripheral;

#[cfg(feature = "ble-gatt-client")]
pub mod gatt_client;

#[cfg(feature = "ble-gatt-server")]
pub mod gatt_server;

#[cfg(feature = "ble-l2cap")]
pub mod l2cap;

use core::mem;

#[cfg(any(feature = "ble-gatt-server", feature = "ble-sec"))]
pub use replies::*;

use crate::{raw, RawError, Softdevice};

pub(crate) unsafe fn on_evt(ble_evt: *const raw::ble_evt_t) {
    trace!("ble evt {:?}", (*ble_evt).header.evt_id as u32);
    match (*ble_evt).header.evt_id as u32 {
        raw::BLE_EVT_BASE..=raw::BLE_EVT_LAST => common::on_evt(ble_evt),
        raw::BLE_GAP_EVT_BASE..=raw::BLE_GAP_EVT_LAST => gap::on_evt(ble_evt),
        #[cfg(feature = "ble-gatt-client")]
        raw::BLE_GATTC_EVT_BASE..=raw::BLE_GATTC_EVT_LAST => gatt_client::on_evt(ble_evt),
        #[cfg(feature = "ble-gatt-server")]
        raw::BLE_GATTS_EVT_BASE..=raw::BLE_GATTS_EVT_LAST => gatt_server::on_evt(ble_evt),
        #[cfg(not(feature = "ble-gatt-server"))]
        raw::BLE_GATTS_EVT_BASE..=raw::BLE_GATTS_EVT_LAST => on_gatts_evt_without_server(ble_evt),
        #[cfg(feature = "ble-l2cap")]
        raw::BLE_L2CAP_EVT_BASE..=raw::BLE_L2CAP_EVT_LAST => l2cap::on_evt(ble_evt),
        _ => {}
    }
}

/// bt2usb patch: without the `ble-gatt-server` feature nothing answered the
/// two GATT server events that wait for the application: a peer's Exchange
/// MTU Request, and an access to a system attribute such as the Service
/// Changed CCCD that S140 includes by default. Unanswered, the peer's ATT
/// transaction times out after 30 s, after which it may send no more ATT PDUs
/// on the link, notifications included.
#[cfg(not(feature = "ble-gatt-server"))]
unsafe fn on_gatts_evt_without_server(ble_evt: *const raw::ble_evt_t) {
    let gatts_evt = crate::util::get_union_field(ble_evt, &(*ble_evt).evt.gatts_evt);
    let conn_handle = gatts_evt.conn_handle;
    match (*ble_evt).header.evt_id as u32 {
        raw::BLE_GATTS_EVTS_BLE_GATTS_EVT_EXCHANGE_MTU_REQUEST => {
            let params = crate::util::get_union_field(ble_evt, &gatts_evt.params.exchange_mtu_request);
            // The Server RX MTU is what this device can receive. A later client
            // exchange must request the same value, and `connect_inner` requests
            // the configured MTU too.
            let server_rx_mtu = Softdevice::steal().att_mtu;
            let ret = raw::sd_ble_gatts_exchange_mtu_reply(conn_handle, server_rx_mtu);
            if let Err(_err) = RawError::convert(ret) {
                warn!("sd_ble_gatts_exchange_mtu_reply err {:?}", _err);
                return;
            }
            // The SoftDevice uses the smaller RX MTU, never below the default.
            let att_mtu = params
                .client_rx_mtu
                .min(server_rx_mtu)
                .max(raw::BLE_GATT_ATT_MTU_DEFAULT as u16);
            debug!("att mtu exchange from peer: client offers {:?}, using {:?}", params.client_rx_mtu, att_mtu);
            connection::try_with_state_by_conn_handle(conn_handle, |state| state.att_mtu = att_mtu);
        }
        raw::BLE_GATTS_EVTS_BLE_GATTS_EVT_SYS_ATTR_MISSING => {
            // No stored CCCD values: start every system attribute at its default.
            let ret = raw::sd_ble_gatts_sys_attr_set(conn_handle, core::ptr::null(), 0, 0);
            if let Err(_err) = RawError::convert(ret) {
                warn!("sd_ble_gatts_sys_attr_set err {:?}", _err);
            }
        }
        _ => {}
    }
}

pub fn get_address(_sd: &Softdevice) -> Address {
    unsafe {
        let mut addr: raw::ble_gap_addr_t = mem::zeroed();
        let ret = raw::sd_ble_gap_addr_get(&mut addr);
        unwrap!(RawError::convert(ret), "sd_ble_gap_addr_get");
        Address::from_raw(addr)
    }
}

pub fn set_address(_sd: &Softdevice, addr: &Address) {
    unsafe {
        let ret = raw::sd_ble_gap_addr_set(addr.as_raw());
        unwrap!(RawError::convert(ret), "sd_ble_gap_addr_set");
    }
}
