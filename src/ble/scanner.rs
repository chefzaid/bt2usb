//! BLE GAP scanner - discovers nearby peripherals and finds saved ones.
//!
//! Uses the SoftDevice Central-role scanning API. A user scan keeps devices
//! that advertise the HID Service UUID (0x1812) and pushes them into the UI
//! event channel. A reconnect scan ([`find_saved_peer`]) looks for the saved
//! devices the connection slots are waiting for.

use core::cell::RefCell;

use crate::ble::coordinator::{merge_advertisement, MAX_CONNECTIONS};
use crate::ble::reconnect::{owner_of, ReconnectTable, ScanDuty};
use crate::ble::{BleErrorTag, BleEvent, DiscoveredDevice};
use crate::config::{
    BLE_FAILED_RECONNECT_HOLDOFF_MS, BLE_FAST_RECONNECT_SECS, BLE_FAST_SCAN_INTERVAL,
    BLE_FAST_SCAN_WINDOW, BLE_MAX_DISCOVERED, BLE_SCAN_DURATION_SECS,
};
use defmt::info;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex as BlockingMutex;
use embassy_sync::channel::Sender;
use embassy_sync::signal::Signal;
use embassy_time::{with_timeout, Duration};
use heapless::Vec;
use nrf_softdevice::ble::{central, Address, IdentityKey};
use nrf_softdevice::Softdevice;

/// A saved device a connection slot is reconnecting to in the background.
#[derive(Clone, Copy)]
pub struct SavedPeer {
    /// The address stored when the device was saved, or its last live address.
    pub address: Address,
    /// The bonded peer's identity key, which resolves its rotating private
    /// addresses; `None` for a record saved without a bond.
    pub identity: Option<IdentityKey>,
}

impl SavedPeer {
    /// Whether an advertisement from `seen` comes from this device. Resolving
    /// a private address calls into the SoftDevice.
    fn matches(&self, seen: Address) -> bool {
        self.identity.is_some_and(|id| id.is_match(seen)) || self.address == seen
    }
}

impl PartialEq for SavedPeer {
    /// The same device: a bonded peer by its identity key, whatever private
    /// address it last used, and any other record by its stored address.
    fn eq(&self, other: &Self) -> bool {
        match (self.identity, other.identity) {
            (Some(a), Some(b)) => a == b,
            (None, None) => self.address == other.address,
            _ => false,
        }
    }
}

/// Background reconnect targets and sightings shared by both slots; see
/// [`reconnect`].
static RECONNECTS: BlockingMutex<
    CriticalSectionRawMutex,
    RefCell<ReconnectTable<SavedPeer, Address>>,
> = BlockingMutex::new(RefCell::new(ReconnectTable::new(
    BLE_FAST_RECONNECT_SECS * 1000,
    BLE_FAILED_RECONNECT_HOLDOFF_MS,
)));

/// Wakes a slot waiting between reconnect attempts when another slot's scan
/// has seen its device. A slot resets its own signal when it starts a
/// reconnect scan, when an attempt fails, and when it stops reconnecting, so a
/// wake always refers to a sighting still waiting in [`RECONNECTS`].
static RECONNECT_WAKE: [Signal<CriticalSectionRawMutex, ()>; MAX_CONNECTIONS] =
    [const { Signal::new() }; MAX_CONNECTIONS];

fn now_ms() -> u64 {
    embassy_time::Instant::now().as_millis()
}

/// Record that `slot` is reconnecting to `peer` in the background.
pub fn register_reconnect(slot: usize, peer: SavedPeer) {
    RECONNECTS.lock(|table| table.borrow_mut().register(slot, peer, now_ms()));
}

/// `slot` stopped reconnecting in the background.
pub fn clear_reconnect(slot: usize) {
    RECONNECTS.lock(|table| table.borrow_mut().clear(slot));
    if let Some(signal) = RECONNECT_WAKE.get(slot) {
        signal.reset();
    }
}

/// `slot`'s background attempt to connect to its device failed. The other
/// slot's reconnect scans ignore that device for
/// `BLE_FAILED_RECONNECT_HOLDOFF_MS`, so a device that advertises but will not
/// connect cannot keep cutting them short.
pub fn reconnect_attempt_failed(slot: usize) {
    RECONNECTS.lock(|table| table.borrow_mut().attempt_failed(slot, now_ms()));
    // The failed attempt dropped any pending sighting; its wake goes with it.
    if let Some(signal) = RECONNECT_WAKE.get(slot) {
        signal.reset();
    }
}

/// Wait until another slot's scan sees `slot`'s device.
pub async fn reconnect_sighted(slot: usize) {
    if let Some(signal) = RECONNECT_WAKE.get(slot) {
        signal.wait().await;
    } else {
        core::future::pending::<()>().await;
    }
}

/// Find the current address of `slot`'s registered device.
///
/// One scan looks for every slot's registered device at once, so a device
/// that is asleep cannot keep the radio from finding the other slot's. When
/// it sees another slot's device it records the sighting, wakes that slot and
/// stops, so that slot can connect at once; it then returns `None`, as it does
/// when nothing is seen within `BLE_CONNECT_TIMEOUT_SECS`. Another slot's
/// device whose last attempt failed is ignored for a while (see
/// [`reconnect_attempt_failed`]). A sighting recorded earlier by the other
/// slot's scan is used without scanning. The scan is passive, accepts
/// advertisements without the HID UUID, and does not change the UI's scan
/// results.
pub async fn find_saved_peer(sd: &Softdevice, slot: usize) -> Option<Address> {
    let _gap = crate::ble::GAP_PROCEDURE.lock().await;
    // No other reconnect scan can run while this slot holds the radio, so any
    // wake already pending refers to the sighting taken below, or to one that
    // has expired.
    if let Some(signal) = RECONNECT_WAKE.get(slot) {
        signal.reset();
    }
    let (sighting, duty) = RECONNECTS.lock(|table| {
        let mut table = table.borrow_mut();
        let now = now_ms();
        (table.take_sighting(slot, now), table.duty(now))
    });
    if let Some(address) = sighting {
        return Some(address);
    }

    let (interval, window) = match duty {
        ScanDuty::Fast => (BLE_FAST_SCAN_INTERVAL, BLE_FAST_SCAN_WINDOW),
        ScanDuty::Slow => {
            let default = central::ScanConfig::default();
            (default.interval, default.window)
        }
    };
    let config = central::ScanConfig {
        active: false,
        interval,
        window,
        ..Default::default()
    };
    let scan = central::scan(sd, &config, |params| {
        let address = Address::from_raw(params.peer_addr);
        // Match against a copy: resolving a private address calls into the
        // SoftDevice, which must not happen inside the critical section.
        let targets = RECONNECTS.lock(|table| table.borrow().targets(slot, now_ms()));
        let owner = owner_of(&targets, address, SavedPeer::matches)?;
        RECONNECTS.lock(|table| table.borrow_mut().record_sighting(owner, address, now_ms()));
        if owner != slot {
            RECONNECT_WAKE[owner].signal(());
        }
        Some(owner)
    });
    let owner = with_timeout(
        Duration::from_secs(crate::config::BLE_CONNECT_TIMEOUT_SECS as u64),
        scan,
    )
    .await
    .ok()?
    .ok()?;
    if owner != slot {
        info!("slot {} scan found slot {}'s device", slot, owner);
        return None;
    }
    RECONNECTS.lock(|table| table.borrow_mut().take_sighting(slot, now_ms()))
}

/// Result of a single scan pass.
pub struct ScanResult {
    pub devices: Vec<DiscoveredDevice, BLE_MAX_DISCOVERED>,
}

/// Run a BLE scan for `BLE_SCAN_DURATION_SECS` seconds.
///
/// Discovered HID peripherals are buffered during the scan (the SoftDevice scan
/// callback cannot `.await`), then emitted to `event_tx` as a burst of
/// `BleEvent::DeviceFound` followed by `BleEvent::ScanComplete` once the scan
/// window closes.
///
/// Returns the accumulated list so the connection manager can index into it.
pub async fn scan(
    sd: &Softdevice,
    event_tx: &Sender<'_, CriticalSectionRawMutex, BleEvent, 8>,
) -> Result<ScanResult, BleErrorTag> {
    // Publish UI state before acquiring the radio lock: a full UI channel
    // must not prevent another slot's radio procedure from making progress.
    event_tx.send(BleEvent::ScanStarted).await;
    // Wait for in-flight scan/connection procedures from other slots.
    let _gap = crate::ble::GAP_PROCEDURE.lock().await;

    info!("BLE scan starting ({} s window)", BLE_SCAN_DURATION_SECS);

    let mut found: Vec<DiscoveredDevice, BLE_MAX_DISCOVERED> = Vec::new();

    let config = central::ScanConfig {
        // Active scan to retrieve scan-response data (device names).
        active: true,
        ..Default::default()
    };

    // We set up a deadline so the scan doesn't run forever.
    let deadline = embassy_time::Instant::now() + Duration::from_secs(BLE_SCAN_DURATION_SECS);

    // The SoftDevice scan callback receives each advertisement.
    // We use a closure that captures our state.
    let scan_fut = central::scan(sd, &config, |params| {
        // SAFETY: the SoftDevice guarantees `p_data`/`len` describe the report,
        // valid for the duration of this callback; `data` does not outlive it
        // (`merge_advertisement` copies what it keeps).
        let data =
            unsafe { core::slice::from_raw_parts(params.data.p_data, params.data.len as usize) };

        // Check if we've exceeded our time budget.
        if embassy_time::Instant::now() > deadline {
            return Some(()); // Signal scan to stop
        }

        let address = nrf_softdevice::ble::Address::from_raw(params.peer_addr);
        // Merge name-only scan responses by peer address; buffer events because
        // this synchronous SoftDevice callback cannot await UI backpressure.
        merge_advertisement(&mut found, address, params.rssi, data);

        // Continue through the window even when full so scan responses can
        // still supply names for the devices already in the bounded list.
        None
    });

    // Hard backstop: the deadline above is only evaluated when an advertisement
    // arrives, so in a quiet RF environment the scan callback might never fire
    // and `central::scan` would otherwise run forever. Cap the whole scan with a
    // wall-clock timeout (slightly beyond the window) so the UI can never hang.
    let timeout = Duration::from_secs(BLE_SCAN_DURATION_SECS) + Duration::from_secs(2);
    let result = with_timeout(timeout, scan_fut).await;
    // Release on both success and failure before awaiting any UI event send.
    drop(_gap);
    match result {
        // Scan stopped itself after the deadline.
        Ok(Ok(())) => {}
        // SoftDevice reported a scan error.
        Ok(Err(_e)) => {
            defmt::warn!("BLE scan ended with error");
            event_tx
                .send(BleEvent::Error(BleErrorTag::ScanFailed))
                .await;
            return Err(BleErrorTag::ScanFailed);
        }
        // Backstop fired — proceed with whatever was discovered so far.
        Err(_timeout) => {
            info!("BLE scan hit hard timeout backstop");
        }
    }

    // Now send all found devices to the UI.
    for device in found.iter() {
        event_tx.send(BleEvent::DeviceFound(device.clone())).await;
    }
    event_tx.send(BleEvent::ScanComplete).await;

    info!("BLE scan complete - {} devices found", found.len());

    Ok(ScanResult { devices: found })
}

// ═══════════════════════════════════════════════════════════════════════════
// Unit Tests (run on host, not embedded)
// ═══════════════════════════════════════════════════════════════════════════
