//! Host tests for the boot diagnostics: the build identity the build script
//! supplies, and the decoding of `POWER.RESETREAS`.

use super::*;

fn causes(bits: u32) -> std::vec::Vec<ResetCause> {
    ResetReasons::from_register(bits).causes().collect()
}

#[test]
fn the_build_identity_names_a_commit_profile_and_log_filter() {
    assert_eq!(FIRMWARE_VERSION, env!("CARGO_PKG_VERSION"));
    let commit = SOURCE_COMMIT
        .strip_suffix("-dirty")
        .unwrap_or(SOURCE_COMMIT);
    let full = commit.len() == 40
        && commit
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    assert!(
        full || SOURCE_COMMIT == "unknown",
        "unexpected source commit {SOURCE_COMMIT:?}"
    );
    assert!(["debug", "release"].contains(&BUILD_PROFILE));
    assert!(!LOG_FILTER.is_empty());
}

#[test]
fn no_cause_bit_is_a_power_on_or_brown_out() {
    assert_eq!(causes(0), [ResetCause::PowerOnOrBrownout]);
    assert_eq!(ResetReasons::from_register(0).undefined_bits(), 0);
}

#[test]
fn each_defined_bit_decodes_to_its_cause() {
    let expected = [
        (0, ResetCause::ResetPin),
        (1, ResetCause::Watchdog),
        (2, ResetCause::SoftReset),
        (3, ResetCause::Lockup),
        (16, ResetCause::WakeFromOffGpio),
        (17, ResetCause::WakeFromOffLpcomp),
        (18, ResetCause::WakeFromOffDebug),
        (19, ResetCause::WakeFromOffNfc),
        (20, ResetCause::WakeFromOffVbus),
    ];
    for (bit, cause) in expected {
        assert_eq!(causes(1 << bit), [cause], "bit {bit}");
    }
}

#[test]
fn the_defined_mask_holds_exactly_the_cause_bits() {
    let bits = ResetCause::BITS.iter().fold(0, |mask, (bit, _)| mask | bit);
    assert_eq!(ResetReasons::DEFINED, bits);
}

#[test]
fn accumulated_causes_are_listed_in_bit_order() {
    // A probe's soft reset after a pin reset that was never cleared.
    assert_eq!(causes(0b101), [ResetCause::ResetPin, ResetCause::SoftReset]);
    assert_eq!(ResetReasons::from_register(0b101).bits(), 0b101);
}

#[test]
fn undefined_bits_are_kept_for_the_report() {
    let reasons = ResetReasons::from_register((1 << 8) | (1 << 2));
    assert_eq!(
        reasons.causes().collect::<std::vec::Vec<_>>(),
        [ResetCause::SoftReset]
    );
    assert_eq!(reasons.undefined_bits(), 1 << 8);
    // Only undefined bits: no defined cause, so it reads as a power-on reset.
    let odd = ResetReasons::from_register(1 << 31);
    assert_eq!(
        odd.causes().collect::<std::vec::Vec<_>>(),
        [ResetCause::PowerOnOrBrownout]
    );
    assert_eq!(odd.undefined_bits(), 1 << 31);
}

#[test]
fn every_cause_has_a_distinct_name() {
    let all = [
        ResetCause::PowerOnOrBrownout,
        ResetCause::ResetPin,
        ResetCause::Watchdog,
        ResetCause::SoftReset,
        ResetCause::Lockup,
        ResetCause::WakeFromOffGpio,
        ResetCause::WakeFromOffLpcomp,
        ResetCause::WakeFromOffDebug,
        ResetCause::WakeFromOffNfc,
        ResetCause::WakeFromOffVbus,
    ];
    for (i, a) in all.iter().enumerate() {
        assert!(!a.name().is_empty());
        for b in &all[i + 1..] {
            assert_ne!(a.name(), b.name());
        }
    }
}
