//! What a defect report needs to name the firmware, the last reset, and how
//! often things went wrong since, without any input or key material.
//!
//! The firmware logs the build's identity and the decoded reset reason at
//! boot (`src/main.rs`), so a log interval says which code wrote it and why
//! the chip last restarted. The build script supplies the identity
//! (`build.rs`); the reset reason comes from the nRF52840's `POWER.RESETREAS`
//! register, which the firmware reads and clears before the SoftDevice takes
//! the POWER peripheral. The shells count lost links, background reconnects,
//! coalesced and dropped reports, and failed writes in [`COUNTERS`], and the
//! main loop logs them when they change ([`CounterReport`]). All of it is
//! hardware-free here so the host tests cover it; `docs/operations.md`
//! ("Reporting A Defect") says how a report collects it.

use crate::config::DIAGNOSTICS_REPORT_INTERVAL_SECS;
use core::sync::atomic::{AtomicU32, Ordering};

/// The crate version from `Cargo.toml`.
pub const FIRMWARE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The git commit the build came from: 40 hexadecimal digits, followed by
/// `-dirty` when tracked files differed from it when the build script last
/// ran. A build outside a git checkout reports `BT2USB_SOURCE_COMMIT` when
/// that is set to a full commit, and `unknown` otherwise (`build.rs`).
pub const SOURCE_COMMIT: &str = env!("BT2USB_SOURCE_COMMIT");

/// Cargo's profile for the build: `debug` or `release`.
pub const BUILD_PROFILE: &str = env!("BT2USB_BUILD_PROFILE");

/// The build's `DEFMT_LOG` filter, or `unset`; it decides which log lines the
/// firmware can print at all.
pub const LOG_FILTER: &str = env!("BT2USB_DEFMT_LOG");

/// One cause the nRF52840 records in `POWER.RESETREAS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetCause {
    /// No cause bit set: the on-chip reset generator restarted the chip, at
    /// power-on or after a brown-out.
    PowerOnOrBrownout,
    /// The nRESET pin, such as a board's reset button.
    ResetPin,
    /// The watchdog timer expired.
    Watchdog,
    /// A soft reset (`AIRCR.SYSRESETREQ`), which a debug probe also uses to
    /// restart the chip after flashing.
    SoftReset,
    /// The CPU locked up, for example after a fault inside the HardFault
    /// handler.
    Lockup,
    /// Woke from System OFF on a GPIO DETECT signal.
    WakeFromOffGpio,
    /// Woke from System OFF on the LPCOMP's ANADETECT signal.
    WakeFromOffLpcomp,
    /// Woke from System OFF when a debugger entered debug interface mode.
    WakeFromOffDebug,
    /// Woke from System OFF on an NFC field.
    WakeFromOffNfc,
    /// Woke from System OFF when VBUS rose into its valid range.
    WakeFromOffVbus,
}

impl ResetCause {
    /// Each cause bit of `POWER.RESETREAS` with its cause (nRF52840 Product
    /// Specification, POWER chapter).
    const BITS: [(u32, ResetCause); 9] = [
        (1 << 0, ResetCause::ResetPin),
        (1 << 1, ResetCause::Watchdog),
        (1 << 2, ResetCause::SoftReset),
        (1 << 3, ResetCause::Lockup),
        (1 << 16, ResetCause::WakeFromOffGpio),
        (1 << 17, ResetCause::WakeFromOffLpcomp),
        (1 << 18, ResetCause::WakeFromOffDebug),
        (1 << 19, ResetCause::WakeFromOffNfc),
        (1 << 20, ResetCause::WakeFromOffVbus),
    ];

    /// The cause as the boot log prints it.
    pub const fn name(self) -> &'static str {
        match self {
            ResetCause::PowerOnOrBrownout => "power-on or brown-out",
            ResetCause::ResetPin => "reset pin",
            ResetCause::Watchdog => "watchdog",
            ResetCause::SoftReset => "soft reset",
            ResetCause::Lockup => "CPU lock-up",
            ResetCause::WakeFromOffGpio => "wake from System OFF (GPIO)",
            ResetCause::WakeFromOffLpcomp => "wake from System OFF (LPCOMP)",
            ResetCause::WakeFromOffDebug => "wake from System OFF (debug interface)",
            ResetCause::WakeFromOffNfc => "wake from System OFF (NFC)",
            ResetCause::WakeFromOffVbus => "wake from System OFF (VBUS)",
        }
    }
}

/// The value of `POWER.RESETREAS` at boot. The register accumulates causes
/// until written back, so the firmware clears it after reading and each boot
/// reports only the resets since the previous one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResetReasons(u32);

impl ResetReasons {
    /// Every bit the nRF52840 defines in `POWER.RESETREAS`: bits 0 to 3 and
    /// 16 to 20, the bits of [`ResetCause::BITS`].
    const DEFINED: u32 = 0x001F_000F;

    pub const fn from_register(bits: u32) -> Self {
        Self(bits)
    }

    /// The register value, as the firmware writes it back to clear the
    /// causes it read.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// The causes recorded, in register bit order, or only
    /// [`ResetCause::PowerOnOrBrownout`] when no defined bit is set.
    pub fn causes(self) -> impl Iterator<Item = ResetCause> {
        let power_on = self.0 & Self::DEFINED == 0;
        let bits = self.0;
        power_on
            .then_some(ResetCause::PowerOnOrBrownout)
            .into_iter()
            .chain(
                ResetCause::BITS
                    .into_iter()
                    .filter(move |(bit, _)| bits & bit != 0)
                    .map(|(_, cause)| cause),
            )
    }

    /// Set bits the nRF52840 does not define, which a report should quote.
    pub const fn undefined_bits(self) -> u32 {
        self.0 & !Self::DEFINED
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for ResetReasons {
    fn format(&self, f: defmt::Formatter) {
        for (index, cause) in self.causes().enumerate() {
            if index > 0 {
                defmt::write!(f, " + ");
            }
            defmt::write!(f, "{=str}", cause.name());
        }
        if self.undefined_bits() != 0 {
            defmt::write!(f, " (undefined bits {=u32:#x})", self.undefined_bits());
        }
    }
}

/// One kind of event the firmware counts since boot. Each count is a number
/// of occurrences only: no address, name, or report content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Counter {
    /// An established link that the peer or the radio dropped.
    LinksLost,
    /// A background reconnect that found its saved device and started to
    /// connect.
    ReconnectAttempts,
    /// A background reconnect attempt that ended without a working link.
    ReconnectFailures,
    /// A report from a peripheral that replaced or merged into the previous
    /// one while that one still waited for the slot to hand it on towards
    /// USB. Notifications the radio delivers together, in one connection
    /// event or after the firmware was busy elsewhere, all reach the slot
    /// before it hands any on, and a full report channel holds reports back
    /// the same way. The latest keyboard and media state and the newer mouse
    /// buttons go out, so a state in between, such as a fast tap or a click
    /// whose press and release merged, is lost; mouse motion is added up,
    /// stopping at ±127 per axis (`hid::coalesce::ReportCoalescer`).
    ReportsCoalesced,
    /// A USB endpoint queue that was full and collapsed to its latest state,
    /// dropping intermediate presses or motion
    /// (`hid::delivery::EndpointDelivery`).
    EndpointOverflows,
    /// A USB endpoint whose report write failed or timed out after it last
    /// worked, or after the bus last reset, resumed, or was configured. The
    /// endpoint keeps retrying with the current state, backing off to one
    /// retry a second, and the retries are not counted again
    /// (`hid::delivery::run_endpoint`).
    UsbWriteFailures,
    /// A host LED state write to a BLE keyboard that failed.
    LedWriteFailures,
    /// A flash write of the device store that failed and was retried, which
    /// the SoftDevice does while the radio leaves it no time to write.
    FlashWriteRetries,
    /// A device store save that failed on every attempt, or the erase of an
    /// unreadable store before a factory reset that failed.
    FlashWriteFailures,
}

impl Counter {
    /// Every counter, in the order the log line lists them.
    pub const ALL: [Counter; 9] = [
        Counter::LinksLost,
        Counter::ReconnectAttempts,
        Counter::ReconnectFailures,
        Counter::ReportsCoalesced,
        Counter::EndpointOverflows,
        Counter::UsbWriteFailures,
        Counter::LedWriteFailures,
        Counter::FlashWriteRetries,
        Counter::FlashWriteFailures,
    ];

    /// The counter as the log line names it.
    pub const fn name(self) -> &'static str {
        match self {
            Counter::LinksLost => "links lost",
            Counter::ReconnectAttempts => "reconnect attempts",
            Counter::ReconnectFailures => "reconnect failures",
            Counter::ReportsCoalesced => "reports coalesced",
            Counter::EndpointOverflows => "endpoint overflows",
            Counter::UsbWriteFailures => "USB write failures",
            Counter::LedWriteFailures => "LED write failures",
            Counter::FlashWriteRetries => "flash write retries",
            Counter::FlashWriteFailures => "flash write failures",
        }
    }
}

/// Counts since boot, one per [`Counter`], each stopping at `u32::MAX`
/// instead of wrapping. Any task may bump them; the atomics need no lock.
pub struct Counters([AtomicU32; Counter::ALL.len()]);

impl Counters {
    pub const fn new() -> Self {
        Self([const { AtomicU32::new(0) }; Counter::ALL.len()])
    }

    /// Count one more `counter` event.
    pub fn bump(&self, counter: Counter) {
        if let Some(count) = self.0.get(counter as usize) {
            let _ = count.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1));
        }
    }

    /// The counts now.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot(Counter::ALL.map(|counter| {
            self.0
                .get(counter as usize)
                .map_or(0, |count| count.load(Ordering::Relaxed))
        }))
    }

    /// Set a count, so the tests can start one near its limit.
    #[cfg(test)]
    pub(crate) fn set(&self, counter: Counter, value: u32) {
        if let Some(count) = self.0.get(counter as usize) {
            count.store(value, Ordering::Relaxed);
        }
    }
}

impl Default for Counters {
    fn default() -> Self {
        Self::new()
    }
}

/// The firmware's counters, bumped by the shells that see each event.
pub static COUNTERS: Counters = Counters::new();

/// Every count at one moment, in [`Counter::ALL`] order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snapshot([u32; Counter::ALL.len()]);

#[cfg(feature = "defmt")]
impl defmt::Format for Snapshot {
    fn format(&self, f: defmt::Formatter) {
        for (index, (counter, count)) in Counter::ALL.iter().zip(self.0).enumerate() {
            if index > 0 {
                defmt::write!(f, ", ");
            }
            defmt::write!(f, "{=str} {=u32}", counter.name(), count);
        }
    }
}

/// When the main loop logs the counters: as soon as they change after a quiet
/// spell, then at most once per [`DIAGNOSTICS_REPORT_INTERVAL_SECS`] while
/// they keep changing, and never while they stay the same.
pub struct CounterReport {
    logged: Snapshot,
    next_ms: u64,
}

impl CounterReport {
    /// Starts from all zero counts, which are never logged on their own.
    pub const fn new() -> Self {
        Self {
            logged: Snapshot([0; Counter::ALL.len()]),
            next_ms: 0,
        }
    }

    /// The counts to log at `now_ms`, if any.
    pub fn poll(&mut self, now_ms: u64, counts: Snapshot) -> Option<Snapshot> {
        if counts == self.logged || now_ms < self.next_ms {
            return None;
        }
        self.logged = counts;
        self.next_ms = now_ms.saturating_add(DIAGNOSTICS_REPORT_INTERVAL_SECS * 1000);
        Some(counts)
    }
}

impl Default for CounterReport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
