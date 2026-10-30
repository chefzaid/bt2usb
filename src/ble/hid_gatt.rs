//! The GATT HID client's decisions, apart from the SoftDevice procedures that
//! carry them out in `hid_client.rs`: what each discovered HID Report
//! characteristic is used for, how each Report Map read request moves the long
//! read on, and which notifications reach the report classifier.

use super::coordinator::ErrorTag;
use super::long_read::{EndOfValue, LongRead, ReadFailure};
use crate::hid::report_protocol::{HidDescriptor, ReportKind, ReportReference, ReportType};
use heapless::Vec;

/// Largest notification payload the client copies out of a GATT event: boot
/// reports are at most 8 bytes, and the report-protocol layouts the bridge
/// translates are shorter than this.
pub const MAX_REPORT_LEN: usize = 32;

/// What the client does with one discovered HID Report characteristic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportUse {
    /// Enable notifications. The kind comes from the Report Reference through
    /// the Report Map; `None` leaves it to the classifier's heuristics.
    Subscribe(Option<ReportKind>),
    /// The keyboard's LED output report, where the host's LED state is
    /// written.
    KeyboardLeds,
    /// Not used: an output or feature report, or a characteristic that cannot
    /// notify.
    Ignore,
    /// An input report that a numbered Report Map does not resolve to exactly
    /// one kind; it is skipped rather than guessed.
    Unresolved,
}

/// Decide what a Report characteristic is for, from whether it has a CCCD
/// (`notifies`), its Report Reference if one was read, and the parsed Report
/// Map (`None` when the peer has none).
pub fn report_use(
    notifies: bool,
    reference: Option<ReportReference>,
    descriptor: Option<&HidDescriptor>,
) -> ReportUse {
    if !notifies {
        let keyboard_output = reference.is_some_and(|r| {
            r.report_type == ReportType::Output
                && descriptor.is_some_and(|d| d.is_keyboard_report(r.report_id))
        });
        return if keyboard_output {
            ReportUse::KeyboardLeds
        } else {
            ReportUse::Ignore
        };
    }
    // A CCCD does not override an explicit output or feature direction: never
    // turn a notifiable output into host input.
    if reference.is_some_and(|r| !r.is_input()) {
        return ReportUse::Ignore;
    }
    let kind = reference.and_then(|r| descriptor.and_then(|d| d.report_kind_for_id(r.report_id)));
    if descriptor.is_some_and(HidDescriptor::has_report_ids) && kind.is_none() {
        return ReportUse::Unresolved;
    }
    ReportUse::Subscribe(kind)
}

/// How one Read or Read Blob request for the Report Map ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fragment<'a> {
    /// The peer returned these bytes.
    Data(&'a [u8]),
    /// ATT Invalid Offset: the previous fragment was the value's last.
    InvalidOffset,
    /// ATT Attribute Not Long: the value fit in the first fragment.
    AttributeNotLong,
    /// The value did not fit in the read buffer.
    Truncated,
    /// Any other failure, including a length the buffer cannot hold.
    Failed,
}

/// Apply one read request's result to the long read: `Ok(true)` once the value
/// is complete, `Ok(false)` while more remains, and the error the slot reports
/// otherwise. A present but unreadable map fails here and never falls back to
/// payload heuristics.
pub fn report_map_step(read: &mut LongRead, fragment: Fragment<'_>) -> Result<bool, ErrorTag> {
    let applied = match fragment {
        Fragment::Data(bytes) => read.append(bytes),
        Fragment::InvalidOffset => read.finish_at_end(EndOfValue::InvalidOffset).map(|()| true),
        Fragment::AttributeNotLong => read
            .finish_at_end(EndOfValue::AttributeNotLong)
            .map(|()| true),
        Fragment::Truncated => return Err(ErrorTag::ReportMapTooLarge),
        Fragment::Failed => return Err(ErrorTag::ReportMapReadFailed),
    };
    applied.map_err(|failure| match failure {
        ReadFailure::TooLarge => ErrorTag::ReportMapTooLarge,
        ReadFailure::InvalidFragment | ReadFailure::UnexpectedEnd => ErrorTag::ReportMapReadFailed,
    })
}

/// Parse a complete Report Map read; a read that has not completed fails.
pub fn report_map(read: &LongRead) -> Result<HidDescriptor, ErrorTag> {
    let bytes = read.value().ok_or(ErrorTag::ReportMapReadFailed)?;
    HidDescriptor::parse(bytes).ok_or(ErrorTag::ReportMapInvalid)
}

/// A notifying value handle and the report kind resolved for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subscription {
    pub value_handle: u16,
    pub kind: Option<ReportKind>,
}

/// A notification passed on to the classifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportNotification {
    pub kind: Option<ReportKind>,
    pub data: Vec<u8, MAX_REPORT_LEN>,
}

/// The notification a peer's value on `handle` becomes: only from a handle
/// the client subscribed to, and only whole. Cutting an over-long report could
/// turn it into a different valid input, such as the prefix of a long keyboard
/// report, so it is dropped instead.
pub fn notification(
    subscriptions: &[Subscription],
    handle: u16,
    data: &[u8],
) -> Option<ReportNotification> {
    let subscription = subscriptions.iter().find(|s| s.value_handle == handle)?;
    let mut copy = Vec::new();
    copy.extend_from_slice(data).ok()?;
    Some(ReportNotification {
        kind: subscription.kind,
        data: copy,
    })
}

#[cfg(test)]
#[path = "hid_gatt_tests.rs"]
mod tests;
