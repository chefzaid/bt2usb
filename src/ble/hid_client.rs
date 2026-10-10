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

use super::long_read::{EndOfValue, LongRead, ReadFailure, MAX_ATTRIBUTE_LEN};
use crate::ble::BleErrorTag;
use crate::hid;
use crate::hid::coalesce::ReportCoalescer;
use crate::hid::delivery::HidEvent;
use crate::hid::host_leds::forward_host_leds;
use crate::hid::keyboard::KeyboardLeds;
use crate::hid::report_protocol::{HidDescriptor, ReportKind, ReportReference, ReportType};
use crate::usb::host_requests::LedReceiver;
use core::cell::RefCell;
use defmt::{info, warn};
use embassy_futures::select::{select, select3};
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

/// Largest notification payload we copy out of the GATT event (boot reports are
/// ≤8 B; report-protocol notifications are capped by `att_mtu`).
const MAX_REPORT_LEN: usize = 32;

/// A discovered HID Report characteristic and the descriptor handles needed to
/// subscribe to and classify it.
#[derive(Clone, Copy)]
struct ReportCharacteristic {
    value_handle: u16,
    cccd_handle: Option<u16>,
    report_ref_handle: Option<u16>,
}

/// An active subscription: a notifying value handle and the report kind resolved
/// for it (`None` → defer to the heuristic classifier).
#[derive(Clone, Copy)]
struct Subscription {
    value_handle: u16,
    kind: Option<ReportKind>,
}

/// Notification event surfaced by the GATT run loop.
pub struct ReportNotification {
    kind: Option<ReportKind>,
    data: Vec<u8, MAX_REPORT_LEN>,
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
        // Only surface notifications from handles we actually subscribed to.
        let sub = self
            .subscriptions
            .iter()
            .find(|s| s.value_handle == handle)?;

        // Truncating an unsupported report can turn it into a different valid
        // input (for example the prefix of a long keyboard report). Reject it.
        let mut buf: Vec<u8, MAX_REPORT_LEN> = Vec::new();
        buf.extend_from_slice(data).ok()?;
        Some(ReportNotification {
            kind: sub.kind,
            data: buf,
        })
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

            // No CCCD → not a notifiable input. If it's an Output report, it's the
            // keyboard LED sink we write host Caps/Num/Scroll state to.
            let Some(cccd) = report.cccd_handle else {
                let is_output = matches!(report_ref, Some(r)
                if r.report_type == ReportType::Output
                    && descriptor.is_some_and(|d| d.is_keyboard_report(r.report_id)));
                if is_output && self.keyboard_led_handle.is_none() {
                    self.keyboard_led_handle = Some(report.value_handle);
                    info!("Found keyboard LED output report");
                }
                continue;
            };

            // A CCCD does not override an explicit Output/Feature direction.
            // Never turn an arbitrary notifiable output into host input.
            if report_ref.is_some_and(|r| !r.is_input()) {
                continue;
            }

            // Input report: resolve its kind from the Report Reference mapped
            // through the Report Map's report-ID table. Anything we can't resolve
            // is subscribed with `kind = None` and classified by the heuristic
            // fallback at notification time.
            let kind = report_ref
                .filter(ReportReference::is_input)
                .and_then(|r| descriptor.and_then(|d| d.report_kind_for_id(r.report_id)));
            if descriptor.is_some_and(HidDescriptor::has_report_ids) && kind.is_none() {
                warn!("Skipping unknown or ambiguous HID report reference");
                continue;
            }

            // Enable notifications (write 0x0001 to the CCCD).
            match gatt_client::write(conn, cccd, &[0x01, 0x00]).await {
                Ok(_) => {
                    let _ = self.subscriptions.push(Subscription {
                        value_handle: report.value_handle,
                        kind,
                    });
                }
                Err(_) => warn!("Could not enable notifications on a report characteristic"),
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
    let mut fragment = [0u8; MAX_ATTRIBUTE_LEN];
    loop {
        let result = gatt_client::read_by_offset(conn, handle, read.offset(), &mut fragment).await;
        let completed = match result {
            Ok(n) => fragment
                .get(..n)
                .ok_or(ReadFailure::InvalidFragment)
                .and_then(|bytes| read.append(bytes)),
            Err(gatt_client::ReadError::Gatt(GattError::ATTERR_INVALID_OFFSET)) => {
                read.finish_at_end(EndOfValue::InvalidOffset).map(|()| true)
            }
            Err(gatt_client::ReadError::Gatt(GattError::ATTERR_ATTRIBUTE_NOT_LONG)) => read
                .finish_at_end(EndOfValue::AttributeNotLong)
                .map(|()| true),
            Err(gatt_client::ReadError::Truncated) => return Err(BleErrorTag::ReportMapTooLarge),
            Err(_) => return Err(BleErrorTag::ReportMapReadFailed),
        }
        .map_err(|failure| match failure {
            ReadFailure::TooLarge => BleErrorTag::ReportMapTooLarge,
            _ => BleErrorTag::ReportMapReadFailed,
        })?;
        if completed {
            break;
        }
    }
    let bytes = read.value().ok_or(BleErrorTag::ReportMapReadFailed)?;
    let descriptor = HidDescriptor::parse(bytes).ok_or(BleErrorTag::ReportMapInvalid)?;
    info!(
        "Complete report map ({} bytes): keyboard={} mouse={} consumer={}",
        bytes.len(),
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
            coalescer.borrow_mut().push(report);
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

    // If this slot holds an LED receiver, also forward the host's LED state to
    // the peer: the current state first, because a keyboard that reconnects
    // starts with its LEDs off, then every change. `write_leds` is a no-op for
    // a peer without a keyboard LED output report. Otherwise just run
    // producer+consumer.
    match led_rx {
        Some(rx) => {
            let led_fut = forward_host_leds(rx, move |leds| client.write_leds(conn, leds));
            let _ = select3(gatt_fut, drain_fut, led_fut).await;
        }
        None => {
            let _ = select(gatt_fut, drain_fut).await;
        }
    }

    info!("HID notification loop ended (connection closed)");
}
