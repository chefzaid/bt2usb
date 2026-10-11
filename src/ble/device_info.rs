//! Reads a peripheral's PnP ID from its Device Information Service (service
//! `0x180A`, characteristic `0x2A50`) and logs it, once per link.
//!
//! The notification loop runs [`log_pnp_id`] after HID input is flowing, so a
//! slow or missing service never delays input. The vendored GATT client allows
//! one client procedure per link at a time, and the host LED writes are the
//! only other ones the loop makes, so it runs in the LED writer's future,
//! before that writer's first write
//! ([`run_notification_loop`](crate::ble::hid_client::run_notification_loop)).
//! Parsing is in the host-tested [`pnp_id`](crate::ble::pnp_id).

use crate::ble::pnp_id::PnpId;
use defmt::{info, warn};
use nrf_softdevice::ble::gatt_client::{
    self, Characteristic, Client, Descriptor, DiscoverError, HvxType,
};
use nrf_softdevice::ble::{Connection, Uuid};

const UUID_DEVICE_INFORMATION: u16 = 0x180A;
const UUID_PNP_ID: u16 = 0x2A50;

/// Finds the PnP ID characteristic; every other Device Information
/// characteristic (names, serial number, revisions) is ignored.
struct DeviceInfoClient {
    pnp_id_handle: Option<u16>,
}

impl Client for DeviceInfoClient {
    type Event = ();

    fn on_hvx(&self, _conn: &Connection, _type: HvxType, _handle: u16, _data: &[u8]) -> Option<()> {
        None
    }

    fn uuid() -> Uuid {
        Uuid::new_16(UUID_DEVICE_INFORMATION)
    }

    fn new_undiscovered(_conn: Connection) -> Self {
        Self {
            pnp_id_handle: None,
        }
    }

    fn discovered_characteristic(&mut self, characteristic: &Characteristic, _: &[Descriptor]) {
        if characteristic.uuid == Some(Uuid::new_16(UUID_PNP_ID)) {
            self.pnp_id_handle = Some(characteristic.handle_value);
        }
    }

    fn discovery_complete(&mut self) -> Result<(), DiscoverError> {
        // The service is optional and so is its PnP ID; `log_pnp_id` reports
        // a missing one.
        Ok(())
    }
}

/// Log the PnP ID of the peripheral on `slot`'s link, or why there is none.
/// Nothing depends on the outcome, so every failure is only logged; a link
/// that drops meanwhile logs nothing here, since the slot reports it.
pub async fn log_pnp_id(conn: &Connection, slot: usize) {
    let client = match gatt_client::discover::<DeviceInfoClient>(conn).await {
        Ok(client) => client,
        Err(DiscoverError::ServiceNotFound) => {
            info!("slot {} PnP ID: no Device Information Service", slot);
            return;
        }
        Err(DiscoverError::Disconnected) => return,
        Err(err) => {
            warn!("slot {} PnP ID: discovery failed: {:?}", slot, err);
            return;
        }
    };
    let Some(handle) = client.pnp_id_handle else {
        info!("slot {} PnP ID: not offered", slot);
        return;
    };
    // One byte more than a valid value, so a longer one is reported, as
    // malformed or truncated, instead of being cut to a valid-looking seven.
    let mut value = [0u8; PnpId::LEN + 1];
    match gatt_client::read(conn, handle, &mut value).await {
        Ok(len) => match value.get(..len).and_then(PnpId::parse) {
            Some(id) => info!("slot {} PnP ID: {}", slot, id),
            None => warn!("slot {} PnP ID: malformed, {} bytes", slot, len),
        },
        Err(gatt_client::ReadError::Disconnected) => {}
        Err(err) => warn!("slot {} PnP ID: read failed: {:?}", slot, err),
    }
}
