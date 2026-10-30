//! BLE GATT HID Client - discovers and subscribes to **all** HID Report
//! characteristics on a connected peripheral.
//!
//! After GAP connection is established, this module:
//! 1. Discovers the HID Service (UUID 0x1812).
//! 2. Collects *every* HID Report characteristic (UUID 0x2A4D) — not just the
//!    first — so multi-report devices (e.g. keyboard + consumer keys) aren't
//!    truncated to their first report.
//! 3. Reads each one's Report Reference descriptor (0x2908) to classify it
//!    (report ID + direction), cross-referenced with the Report Map.
//! 4. Enables CCCD notifications on each input report characteristic.
//! 5. Forwards received HID reports to the USB task via a channel.
//!
//! The `#[gatt_client]` macro can only bind a single characteristic per UUID,
//! so this uses a hand-rolled [`gatt_client::Client`] implementation instead.
//! What each step decides lives in the host-tested [`super::hid_gatt`]; this
//! file runs the SoftDevice procedures and logs.

use super::hid_gatt::{
    notification, report_map, report_map_step, report_use, Fragment, ReportNotification, ReportUse,
    Subscription,
};
use super::long_read::{LongRead, MAX_ATTRIBUTE_LEN};
use crate::ble::{device_info, BleErrorTag};
use crate::diagnostics::{Counter, COUNTERS};
use crate::hid;
use crate::hid::coalesce::ReportCoalescer;
use crate::hid::delivery::HidEvent;
use crate::hid::host_leds::forward_host_leds;
use crate::hid::keyboard::KeyboardLeds;
use crate::hid::report_protocol::{HidDescriptor, ReportReference};
use crate::usb::host_requests::LedReceiver;
use core::cell::RefCell;
use defmt::{info, warn};
use embassy_futures::select::select3;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Sender;
use embassy_sync::signal::Signal;
use heapless::Vec;
use nrf_softdevice::ble::gatt_client::{
    self, Characteristic, Client, Descriptor, DiscoverError, HvxType,
};
use nrf_softdevice::ble::{Connection, GattError, Uuid};

// HID-over-GATT 16-bit UUIDs.
const UUID_HID_SERVICE: u16 = 0x1812;
const UUID_REPORT: u16 = 0x2A4D;
const UUID_REPORT_MAP: u16 = 0x2A4B;
const UUID_PROTOCOL_MODE: u16 = 0x2A4E;
const UUID_REPORT_REFERENCE: u16 = 0x2908;
const UUID_CCCD: u16 = 0x2902;

/// Maximum number of HID Report (0x2A4D) characteristics tracked per device.
/// A composite HID peripheral rarely exposes more than a handful of reports.
const MAX_REPORTS: usize = 8;

/// A discovered HID Report characteristic and the descriptor handles needed to
/// subscribe to and classify it.
#[derive(Clone, Copy)]
struct ReportCharacteristic {
    value_handle: u16,
    cccd_handle: Option<u16>,
    report_ref_handle: Option<u16>,
}

/// Hand-rolled HID-over-GATT client.
///
/// Captures every HID Report characteristic plus the Report Map and Protocol
/// Mode handles. Handles are gathered during discovery; the notification
/// subscriptions are resolved afterwards in [`HidServiceClient::subscribe_all`].
pub struct HidServiceClient {
    report_map_handle: Option<u16>,
    protocol_mode_handle: Option<u16>,
    reports: Vec<ReportCharacteristic, MAX_REPORTS>,
    subscriptions: Vec<Subscription, MAX_REPORTS>,
    /// Handle of the keyboard's LED **output** report characteristic, if any —
    /// where host LED (Caps/Num/Scroll) state is written back to the BLE keyboard.
    keyboard_led_handle: Option<u16>,
}

fn descriptor_handle(descriptors: &[Descriptor], uuid_16: u16) -> Option<u16> {
    let want = Uuid::new_16(uuid_16);
    descriptors
        .iter()
        .find(|d| d.uuid == Some(want))
        .map(|d| d.handle)
}

impl Client for HidServiceClient {
    type Event = ReportNotification;

    fn uuid() -> Uuid {
        Uuid::new_16(UUID_HID_SERVICE)
    }

    fn new_undiscovered(_conn: Connection) -> Self {
        Self {
            report_map_handle: None,
            protocol_mode_handle: None,
            reports: Vec::new(),
            subscriptions: Vec::new(),
            keyboard_led_handle: None,
        }
    }

    fn discovered_characteristic(
        &mut self,
        characteristic: &Characteristic,
        descriptors: &[Descriptor],
    ) {
        let Some(uuid) = characteristic.uuid else {
            return;
        };

        if uuid == Uuid::new_16(UUID_REPORT_MAP) {
            self.report_map_handle = Some(characteristic.handle_value);
        } else if uuid == Uuid::new_16(UUID_PROTOCOL_MODE) {
            self.protocol_mode_handle = Some(characteristic.handle_value);
        } else if uuid == Uuid::new_16(UUID_REPORT) {
            // A Report characteristic with a CCCD is a notifiable *input* report;
            // its Report Reference descriptor tells us the report ID + direction.
            let _ = self.reports.push(ReportCharacteristic {
                value_handle: characteristic.handle_value,
                cccd_handle: descriptor_handle(descriptors, UUID_CCCD),
                report_ref_handle: descriptor_handle(descriptors, UUID_REPORT_REFERENCE),
            });
        }
    }

    fn discovery_complete(&mut self) -> Result<(), DiscoverError> {
        if self.reports.is_empty() {
            return Err(DiscoverError::ServiceIncomplete);
        }
        Ok(())
    }

    fn on_hvx(
        &self,
        _conn: &Connection,
        type_: HvxType,
        handle: u16,
        data: &[u8],
    ) -> Option<Self::Event> {
        if type_ != HvxType::Notification {
            return None;
        }
        notification(&self.subscriptions, handle, data)
    }
}

impl HidServiceClient {
    /// Subscribe to every discovered input report characteristic, resolving each
    /// one's kind from its Report Reference descriptor and the Report Map.
    async fn subscribe_all(
        &mut self,
        conn: &Connection,
        descriptor: Option<&HidDescriptor>,
    ) -> Result<(), BleErrorTag> {
        // Snapshot the (Copy) report list so we can fill `subscriptions` while
        // iterating without aliasing `&mut self`.
        let reports = self.reports.clone();

        for report in reports.iter() {
            // Read this characteristic's Report Reference (0x2908) once: it gives
            // the report ID + direction (Input/Output/Feature).
            let report_ref = match report.report_ref_handle {
                Some(ref_handle) => {
                    let mut buf = [0u8; 2];
                    match gatt_client::read(conn, ref_handle, &mut buf).await {
                        Ok(n) => buf.get(..n).and_then(ReportReference::parse),
                        Err(_) => None,
                    }
                }
                None => None,
            };

            let usage = report_use(report.cccd_handle.is_some(), report_ref, descriptor);
            match (usage, report.cccd_handle) {
                (ReportUse::Subscribe(kind), Some(cccd)) => {
                    // Enable notifications (write 0x0001 to the CCCD).
                    match gatt_client::write(conn, cccd, &[0x01, 0x00]).await {
                        Ok(_) => {
                            let _ = self.subscriptions.push(Subscription {
                                value_handle: report.value_handle,
                                kind,
                            });
                        }
                        Err(_) => {
                            warn!("Could not enable notifications on a report characteristic")
                        }
                    }
                }
                // The first one found is where host LED state is written.
                (ReportUse::KeyboardLeds, _) if self.keyboard_led_handle.is_none() => {
                    self.keyboard_led_handle = Some(report.value_handle);
                    info!("Found keyboard LED output report");
                }
                (ReportUse::Unresolved, _) => {
                    warn!("Skipping unknown or ambiguous HID report reference")
                }
                _ => {}
            }
        }

        if self.subscriptions.is_empty() {
            warn!("No HID report characteristics could be subscribed");
            return Err(BleErrorTag::NotifyFailed);
        }

        info!(
            "Subscribed to {} of {} HID report characteristics",
            self.subscriptions.len(),
            self.reports.len()
        );
        Ok(())
    }

    /// Write host LED (Caps/Num/Scroll) state to the BLE keyboard's output
    /// report, if this device exposes one. No-op for non-keyboard peers.
    async fn write_leds(&self, conn: &Connection, leds: KeyboardLeds) {
        if let Some(handle) = self.keyboard_led_handle {
            if gatt_client::write(conn, handle, &[leds.byte()])
                .await
                .is_err()
            {
                COUNTERS.bump(Counter::LedWriteFailures);
                warn!("Failed to write LED state to BLE keyboard");
            }
        }
    }
}

/// Read and parse the Report Map (0x2A4B) so report IDs can be mapped to kinds.
async fn read_report_map(
    conn: &Connection,
    client: &HidServiceClient,
) -> Result<Option<HidDescriptor>, BleErrorTag> {
    // A missing characteristic is the sole legacy-fallback case. A present but
    // unreadable/malformed map must never fall through to payload heuristics.
    let Some(handle) = client.report_map_handle else {
        warn!("HID report map absent; using legacy report classification");
        return Ok(None);
    };
    let mut read = LongRead::new(conn.att_mtu()).map_err(|_| BleErrorTag::ReportMapReadFailed)?;
    let mut buffer = [0u8; MAX_ATTRIBUTE_LEN];
    loop {
        let result = gatt_client::read_by_offset(conn, handle, read.offset(), &mut buffer).await;
        let fragment = match result {
            Ok(n) => buffer.get(..n).map_or(Fragment::Failed, Fragment::Data),
            Err(gatt_client::ReadError::Gatt(GattError::ATTERR_INVALID_OFFSET)) => {
                Fragment::InvalidOffset
            }
            Err(gatt_client::ReadError::Gatt(GattError::ATTERR_ATTRIBUTE_NOT_LONG)) => {
                Fragment::AttributeNotLong
            }
            Err(gatt_client::ReadError::Truncated) => Fragment::Truncated,
            Err(_) => Fragment::Failed,
        };
        if report_map_step(&mut read, fragment)? {
            break;
        }
    }
    let descriptor = report_map(&read)?;
    info!(
        "Complete report map ({} bytes): keyboard={} mouse={} consumer={}",
        read.offset(),
        descriptor.has_keyboard,
        descriptor.has_mouse,
        descriptor.has_consumer
    );
    Ok(Some(descriptor))
}

/// Discover the HID service and subscribe to all HID Report notifications.
///
/// Returns the client (which owns the subscription handles) and the parsed
/// Report Map descriptor for notification-time classification.
pub async fn discover_and_subscribe(
    conn: &Connection,
) -> Result<(HidServiceClient, Option<HidDescriptor>), BleErrorTag> {
    info!("Discovering HID service...");

    let mut client: HidServiceClient = gatt_client::discover(conn).await.map_err(|err| {
        warn!("HID discovery failed: {:?}", err);
        BleErrorTag::HidNotFound
    })?;

    info!(
        "HID service discovered ({} report characteristics)",
        client.reports.len()
    );

    // Force Report Protocol mode (1). Boot Protocol would route input to the
    // Boot Keyboard/Mouse Input Report characteristics, which we don't track.
    // Report Protocol is also the GATT default, so this is mostly defensive.
    if let Some(handle) = client.protocol_mode_handle {
        match gatt_client::write(conn, handle, &[1u8]).await {
            Ok(_) => info!("Set HID protocol to Report mode"),
            Err(_) => warn!("Could not set report protocol (using device default)"),
        }
    }

    let descriptor = read_report_map(conn, &client).await?;

    client.subscribe_all(conn, descriptor.as_ref()).await?;

    Ok((client, descriptor))
}

/// Run the notification listener loop.
///
/// Blocks until the connection drops.  Each received HID report is classified
/// and forwarded to `report_tx` for the USB task to consume.
///
/// The GATT callback (`gatt_client::run`) is *synchronous*, so it cannot await
/// channel backpressure. Instead of the old `try_send`-and-drop — which could
/// silently drop a key-*up* and leave a key stuck on the host — it pushes into a
/// [`ReportCoalescer`]. A concurrent drain future `pop`s from the coalescer and
/// `send().await`s with backpressure, so reports are never dropped on a full
/// channel; the coalescer keeps memory bounded by merging per-endpoint state
/// while preserving release reports and accumulating relative mouse motion.
pub async fn run_notification_loop(
    conn: &Connection,
    client: &HidServiceClient,
    descriptor: Option<HidDescriptor>,
    report_tx: &Sender<'_, CriticalSectionRawMutex, HidEvent, 16>,
    led_rx: Option<&mut LedReceiver>,
    source: usize,
) {
    info!("HID notification loop started");

    // Single-producer (sync GATT callback) / single-consumer (async drain)
    // hand-off on the cooperative executor. The `RefCell` is only ever borrowed
    // synchronously — never across an `.await` — so it cannot double-borrow.
    let coalescer: RefCell<ReportCoalescer> = RefCell::new(ReportCoalescer::new());
    let wake: Signal<CriticalSectionRawMutex, ()> = Signal::new();

    // Producer: classify each notification and enqueue it (never blocks). A
    // report whose characteristic resolved to a known kind is classified
    // directly (its payload carries no report-ID prefix); otherwise we fall back
    // to the descriptor-guided heuristic.
    let gatt_fut = gatt_client::run(conn, client, |event: ReportNotification| {
        let parsed = hid::classify_gatt_notification(&event.data, event.kind, descriptor.as_ref());
        if let Some(report) = parsed {
            if coalescer.borrow_mut().push(report) {
                COUNTERS.bump(Counter::ReportsCoalesced);
            }
            wake.signal(());
        }
    });

    // Consumer: drain pending reports into the USB channel, applying
    // backpressure (`send().await`) so nothing is ever dropped.
    let drain_fut = async {
        loop {
            // Pop without holding the borrow across the await below.
            let next = coalescer.borrow_mut().pop();
            match next {
                Some(report) => report_tx.send(HidEvent::Report { source, report }).await,
                None => wake.wait().await,
            }
        }
    };

    // The link's GATT client procedures, one at a time: log the peer's PnP ID
    // now that input flows, then, if this slot holds an LED receiver, forward
    // the host's LED state to the peer: the current state first, because a
    // keyboard that reconnects starts with its LEDs off, then every change.
    // `write_leds` is a no-op for a peer without a keyboard LED output report.
    let gatt_procedures_fut = async {
        device_info::log_pnp_id(conn, source).await;
        match led_rx {
            Some(rx) => forward_host_leds(rx, move |leds| client.write_leds(conn, leds)).await,
            None => core::future::pending().await,
        }
    };
    let _ = select3(gatt_fut, drain_fut, gatt_procedures_fut).await;

    info!("HID notification loop ended (connection closed)");
}
