//! What a defect report needs to name the firmware and the last reset, without
//! any input or key material.
//!
//! The firmware logs the build's identity and the decoded reset reason at
//! boot (`src/main.rs`), so a log interval says which code wrote it and why
//! the chip last restarted. The build script supplies the identity
//! (`build.rs`); the reset reason comes from the nRF52840's `POWER.RESETREAS`
//! register, which the firmware reads and clears before the SoftDevice takes
//! the POWER peripheral. Both are hardware-free here so the host tests cover
//! the decoding; `docs/operations.md` ("Reporting A Defect") says how a report
//! collects them.

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

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
