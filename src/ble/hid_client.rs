//! BLE GATT HID Client - discovers and subscribes to HID Report
//! characteristics on a connected peripheral.
//!
//! After GAP connection is established, this module:
//! 1. Discovers the HID Service (UUID 0x1812).
//! 2. Finds all HID Report characteristics (UUID 0x2A4D).
//! 3. Reads the Report Reference descriptor to classify each report
//!    (input/output, report ID).
//! 4. Enables CCCD notifications on input report characteristics.
//! 5. Forwards received HID reports to the USB task via a channel.

use crate::ble::BleErrorTag;
use crate::hid;
use crate::hid::report_protocol::HidDescriptor;
use crate::hid::HidReport;
use defmt::{info, warn};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Sender;
use nrf_softdevice::ble::{gatt_client, Connection};

/// nrf-softdevice GATT client struct for the HID-over-GATT service.
///
/// The `#[nrf_softdevice::gatt_client]` macro generates discovery and
/// read/write/notify helpers for the listed characteristics.
#[nrf_softdevice::gatt_client(uuid = "1812")]
pub struct HidServiceClient {
    /// HID Report (Input) - notifications carry live keystrokes / mouse data.
    #[characteristic(uuid = "2a4d", read, notify)]
    pub hid_report: [u8; 20],

    /// HID Report Map - describes the report descriptor (for advanced parsing).
    #[characteristic(uuid = "2a4b", read)]
    pub report_map: [u8; 128],

    /// Protocol Mode - 0 = Boot Protocol, 1 = Report Protocol.
    #[characteristic(uuid = "2a4e", read, write)]
    pub protocol_mode: u8,
}

/// Discover the HID service on the connected peripheral and subscribe
/// to HID Report notifications.
///
/// Returns the `HidServiceClient` on success so the caller can manage
/// the subscription lifetime.
pub async fn discover_and_subscribe(
    conn: &Connection,
) -> Result<(HidServiceClient, Option<HidDescriptor>), BleErrorTag> {
    info!("Discovering HID service...");

    // GATT service/characteristic discovery.
    let client: HidServiceClient = gatt_client::discover(conn)
        .await
        .map_err(|_| BleErrorTag::HidNotFound)?;

    info!("HID service discovered");

    // Optionally switch to Boot Protocol mode for simpler reports.
    // Many devices default to Report Protocol; boot mode gives us
    // a fixed 8-byte keyboard / 4-byte mouse layout.
    match client.protocol_mode_write(&0u8).await {
        Ok(_) => info!("Set HID protocol to Boot mode"),
        Err(_) => warn!("Could not set boot protocol (device may not support it)"),
    }

    let mut descriptor_info: Option<HidDescriptor> = None;
    match client.report_map_read().await {
        Ok(map) => {
            descriptor_info = HidDescriptor::parse(&map);
            if let Some(desc) = descriptor_info {
                let k_id = desc.keyboard_report_id.unwrap_or(0);
                let m_id = desc.mouse_report_id.unwrap_or(0);
                let c_id = desc.consumer_report_id.unwrap_or(0);
                info!(
                    "Report map parsed: keyboard={} mouse={} consumer={} (ids: k={} m={} c={})",
                    desc.has_keyboard, desc.has_mouse, desc.has_consumer, k_id, m_id, c_id
                );
            } else {
                warn!("Report map parsing returned no recognized report types");
            }
        }
        Err(_) => warn!("Could not read HID report map"),
    }

    // Enable CCCD notifications on the HID Report characteristic.
    client
        .hid_report_cccd_write(true)
        .await
        .map_err(|_| BleErrorTag::NotifyFailed)?;

    info!("Subscribed to HID report notifications");
    Ok((client, descriptor_info))
}

/// Run the notification listener loop.
///
/// Blocks until the connection drops.  Each received HID report is
/// classified and sent to `report_tx` for the USB task to consume.
pub async fn run_notification_loop(
    conn: &Connection,
    client: &HidServiceClient,
    descriptor: Option<HidDescriptor>,
    report_tx: &Sender<'_, CriticalSectionRawMutex, HidReport, 16>,
) {
    info!("HID notification loop started");

    // The nrf-softdevice gatt_client event callback.
    // We use `run()` which processes GATT events and calls our closure
    // for each notification.
    let _result = gatt_client::run(conn, client, |event| match event {
        HidServiceClientEvent::HidReportNotification(data) => {
            let parsed = hid::classify_notification_with_hint(&data, descriptor.as_ref());

            if let Some(report) = parsed {
                // try_send avoids blocking; if USB task is behind, we drop.
                if report_tx.try_send(report).is_err() {
                    warn!("HID report channel full - dropping report");
                }
            }
        }
    })
    .await;

    info!("HID notification loop ended (connection closed)");
}
