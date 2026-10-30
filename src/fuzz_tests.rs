//! Fuzz targets for the parsers that read what a peripheral sends: Report
//! Maps, report notifications, advertising data, and long attribute reads.
//! Each check states what any accepted input must satisfy; [`crate::fuzz`]
//! drives it over a seed corpus and mutated inputs. The pairing store's
//! targets are in `storage_fuzz_tests.rs`.

use std::sync::OnceLock;

use crate::ble::adv_parser::{advertised_name, contains_hid_service_uuid, extract_device_name};
use crate::ble::hid_gatt::MAX_REPORT_LEN;
use crate::ble::long_read::{EndOfValue, LongRead, ReadFailure, MAX_ATTRIBUTE_LEN};
use crate::fuzz::{parse_corpus, run, Target};
use crate::hid::consumer::{CONSUMER_REPORT_DESCRIPTOR, MAX_CONSUMER_USAGE};
use crate::hid::keyboard::KEYBOARD_REPORT_DESCRIPTOR;
use crate::hid::mouse::MOUSE_REPORT_DESCRIPTOR;
use crate::hid::report_protocol::{HidDescriptor, ReportKind};
use crate::hid::{
    classify_gatt_notification, classify_known, classify_notification,
    classify_notification_with_hint, classify_report, HidReport,
};

const REPORT_MAPS: &str = include_str!("../tests/fuzz-corpus/report-map.txt");
const KINDS: [ReportKind; 3] = [
    ReportKind::Keyboard,
    ReportKind::Mouse,
    ReportKind::Consumer,
];
/// The firmware's own USB descriptors, which a peripheral could equally send.
const USB_DESCRIPTORS: [&[u8]; 3] = [
    KEYBOARD_REPORT_DESCRIPTOR,
    MOUSE_REPORT_DESCRIPTOR,
    CONSUMER_REPORT_DESCRIPTOR,
];

// ── Report Maps ──────────────────────────────────────────────────────────────

/// Whether the map has an input of `kind`, and the report ID it keeps for it.
fn declared(desc: &HidDescriptor, kind: ReportKind) -> (bool, Option<u8>) {
    match kind {
        ReportKind::Keyboard => (desc.has_keyboard, desc.keyboard_report_id),
        ReportKind::Mouse => (desc.has_mouse, desc.mouse_report_id),
        ReportKind::Consumer => (desc.has_consumer, desc.consumer_report_id),
    }
}

fn check_report_map(data: &[u8]) {
    let Some(desc) = HidDescriptor::parse(data) else {
        return;
    };
    assert!(
        KINDS.iter().any(|&kind| declared(&desc, kind).0),
        "accepted a map without a supported input"
    );
    for kind in KINDS {
        let (present, id) = declared(&desc, kind);
        assert!(
            present || id.is_none(),
            "{kind:?} report ID without a {kind:?} input"
        );
        assert_ne!(id, Some(0), "kept the reserved report ID 0");
    }
    // A report ID routes to a kind only when that kind alone holds it.
    for id in 0..=u8::MAX {
        let holders: Vec<_> = KINDS
            .into_iter()
            .filter(|&kind| declared(&desc, kind).1 == Some(id))
            .collect();
        let expected = match *holders {
            [kind] => Some(kind),
            _ => None,
        };
        assert_eq!(desc.report_kind_for_id(id), expected, "report ID {id}");
    }
    assert_eq!(
        desc.has_report_ids(),
        KINDS.iter().any(|&kind| declared(&desc, kind).1.is_some())
    );
    let again = HidDescriptor::parse(data).expect("parsing is deterministic");
    assert!(KINDS
        .iter()
        .all(|&kind| declared(&desc, kind) == declared(&again, kind)));
}

#[test]
fn fuzz_report_maps() {
    let target = Target {
        name: "report-map",
        corpus: REPORT_MAPS,
        // A Report Map is one attribute value.
        max_len: MAX_ATTRIBUTE_LEN,
    };
    run(&target, &USB_DESCRIPTORS, check_report_map);
}

// ── Report notifications ─────────────────────────────────────────────────────

/// The maps of the Report Map corpus and the USB descriptors that parse,
/// to classify notifications against.
fn descriptors() -> &'static [HidDescriptor] {
    static DESCRIPTORS: OnceLock<Vec<HidDescriptor>> = OnceLock::new();
    DESCRIPTORS.get_or_init(|| {
        let mut maps = parse_corpus(REPORT_MAPS);
        maps.extend(USB_DESCRIPTORS.iter().map(|map| map.to_vec()));
        maps.iter()
            .filter_map(|map| HidDescriptor::parse(map))
            .collect()
    })
}

fn kind_of(report: &HidReport) -> ReportKind {
    match report {
        HidReport::Keyboard(_) => ReportKind::Keyboard,
        HidReport::Mouse(_) => ReportKind::Mouse,
        HidReport::Consumer(_) => ReportKind::Consumer,
    }
}

/// Whether `report` carries exactly what `payload` says, in the fixed layout
/// its kind's decoder reads, and serializes to its USB report's length.
fn faithful(report: &HidReport, payload: &[u8]) -> bool {
    let mut usb = [0u8; 8];
    let length = report.serialize(&mut usb);
    match (report, payload) {
        (HidReport::Keyboard(keys), &[modifier, _, ref codes @ ..]) => {
            length == 8
                && payload.len() == 8
                && keys.modifier == modifier
                && keys.reserved == 0
                && keys.keycodes[..] == codes[..]
                && usb[..] == [&[modifier, 0][..], codes].concat()[..]
        }
        (HidReport::Mouse(mouse), &[buttons, x, y, ref rest @ ..]) => {
            let extra = |at: usize| rest.get(at).map_or(0, |&byte| byte as i8);
            length == 5
                && rest.len() <= 2
                && mouse.buttons == buttons & 0x1F
                && (mouse.x, mouse.y) == (x as i8, y as i8)
                && (mouse.wheel, mouse.pan) == (extra(0), extra(1))
        }
        (HidReport::Consumer(consumer), &[low, high]) => {
            length == 2
                && consumer.usage == u16::from_le_bytes([low, high])
                && consumer.usage <= MAX_CONSUMER_USAGE
                && usb[..2] == [low, high]
        }
        _ => false,
    }
}

/// The kind a conventional report ID prefix (1, 2, 3) names.
fn conventional(report_id: u8) -> Option<ReportKind> {
    KINDS.get(usize::from(report_id).checked_sub(1)?).copied()
}

fn check_report(data: &[u8]) {
    let first = data.first().copied().unwrap_or(0);
    for report_id in [0, 1, 2, 3, first] {
        if let Some(report) = classify_report(report_id, data) {
            assert!(faithful(&report, data), "report ID {report_id}: {report:?}");
            if let Some(kind) = conventional(report_id) {
                assert_eq!(kind_of(&report), kind);
            }
        }
    }
    if let Some(report) = classify_notification(data) {
        let prefixed = data.split_first().is_some_and(|(&id, payload)| {
            conventional(id) == Some(kind_of(&report)) && faithful(&report, payload)
        });
        assert!(prefixed || faithful(&report, data), "{report:?}");
    }
    for kind in KINDS {
        for report in [
            classify_known(kind, data),
            classify_gatt_notification(data, Some(kind), None),
        ]
        .into_iter()
        .flatten()
        {
            assert_eq!(kind_of(&report), kind);
            assert!(faithful(&report, data), "{kind:?}: {report:?}");
        }
    }
    assert_eq!(
        classify_gatt_notification(data, None, None),
        classify_report(0, data)
    );
    for desc in descriptors() {
        let advertised = |report: &HidReport| declared(desc, kind_of(report)).0;
        if let Some(report) = classify_notification_with_hint(data, Some(desc)) {
            if desc.has_report_ids() {
                // Numbered maps route only by the ID prefix the map declares.
                let (&id, payload) = data.split_first().expect("a numbered report has an ID");
                assert_eq!(desc.report_kind_for_id(id), Some(kind_of(&report)));
                assert!(faithful(&report, payload), "{report:?}");
            } else {
                assert!(advertised(&report), "{report:?} is not in the map");
                assert!(faithful(&report, data), "{report:?}");
            }
        }
        let gatt = classify_gatt_notification(data, None, Some(desc));
        if desc.has_report_ids() {
            assert_eq!(gatt, None, "a numbered map needs the characteristic's kind");
        } else if let Some(report) = gatt {
            assert!(advertised(&report) && faithful(&report, data), "{report:?}");
        }
    }
}

#[test]
fn fuzz_report_notifications() {
    let target = Target {
        name: "report",
        corpus: include_str!("../tests/fuzz-corpus/report.txt"),
        // The longest notification the GATT client passes on.
        max_len: MAX_REPORT_LEN,
    };
    run(&target, &[], check_report);
}

// ── Advertising data ─────────────────────────────────────────────────────────

/// The length-type-value structures up to the first malformed one, walked
/// independently of the parser.
fn structures(data: &[u8]) -> Vec<(u8, &[u8])> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(&len) = data.get(at) {
        let end = at + 1 + usize::from(len);
        if len == 0 || end > data.len() {
            break;
        }
        found.push((data[at + 1], &data[at + 2..end]));
        at = end;
    }
    found
}

/// The longest prefix of `text` that fits in 32 bytes without splitting a
/// character.
fn cut_to_32_bytes(text: &str) -> &str {
    (0..=text.len().min(32))
        .rev()
        .find_map(|end| text.get(..end))
        .unwrap_or_default()
}

fn check_advertisement(data: &[u8]) {
    let found = structures(data);
    let hid = found.iter().any(|&(ad_type, value)| {
        matches!(ad_type, 0x02 | 0x03) && value.chunks_exact(2).any(|uuid| uuid == [0x12, 0x18])
    });
    assert_eq!(contains_hid_service_uuid(data), hid);

    let names = |wanted: u8| {
        found
            .iter()
            .filter(move |&&(ad_type, _)| ad_type == wanted)
            .filter_map(|&(_, value)| core::str::from_utf8(value).ok())
            .filter(|name| !name.is_empty())
            .map(cut_to_32_bytes)
    };
    // The first complete name wins; otherwise the last shortened one.
    let expected = names(0x09).next().or_else(|| names(0x08).next_back());
    let name = advertised_name(data);
    assert_eq!(name.as_deref(), expected);
    assert_eq!(
        extract_device_name(data).as_str(),
        expected.unwrap_or("Unknown")
    );
}

#[test]
fn fuzz_advertisements() {
    let target = Target {
        name: "advertisement",
        corpus: include_str!("../tests/fuzz-corpus/advertisement.txt"),
        // Extended advertising data reaches 255 bytes per report; legacy data
        // is at most 31.
        max_len: 255,
    };
    run(&target, &[], check_advertisement);
}

// ── Long attribute reads ─────────────────────────────────────────────────────

/// Replay the steps `data` encodes (see `tests/fuzz-corpus/long-read.txt`)
/// against [`LongRead`] and a model of what a peer may legitimately send.
fn check_long_read(data: &[u8]) {
    let Some((&mtu, steps)) = data.split_first_chunk::<2>() else {
        return;
    };
    let mtu = 20 + u16::from_le_bytes(mtu) % 500;
    let mut read = match LongRead::new(mtu) {
        Ok(read) => read,
        Err(failure) => {
            assert!(!(23..=517).contains(&mtu), "refused MTU {mtu}");
            assert_eq!(failure, ReadFailure::InvalidFragment);
            return;
        }
    };
    assert!((23..=517).contains(&mtu), "accepted MTU {mtu}");
    let payload = usize::from(mtu) - 1;
    let mut value = Vec::new();
    let mut complete = false;
    for &step in steps {
        let amount = usize::from(step & 0x3F);
        if step >> 6 == 3 {
            let reason = if step & 1 == 1 {
                EndOfValue::InvalidOffset
            } else {
                EndOfValue::AttributeNotLong
            };
            // Only a value of whole fragments can end by an error, and
            // Attribute Not Long only after the first one.
            let ends = !complete
                && !value.is_empty()
                && value.len().is_multiple_of(payload)
                && (reason == EndOfValue::InvalidOffset || value.len() == payload);
            let result = read.finish_at_end(reason);
            assert_eq!(
                result,
                if ends {
                    Ok(())
                } else {
                    Err(ReadFailure::UnexpectedEnd)
                }
            );
            complete |= ends;
        } else {
            let length = match step >> 6 {
                0 => payload,
                1 => amount.min(payload - 1),
                _ => payload + 1 + amount,
            };
            let fragment: Vec<u8> = (value.len()..value.len() + length)
                .map(|at| at as u8)
                .collect();
            let result = read.append(&fragment);
            if complete || length > payload {
                assert_eq!(result, Err(ReadFailure::InvalidFragment));
            } else if value.len() + length > MAX_ATTRIBUTE_LEN {
                assert_eq!(result, Err(ReadFailure::TooLarge));
                return;
            } else {
                value.extend_from_slice(&fragment);
                complete = length < payload;
                assert_eq!(result, Ok(complete));
            }
        }
        assert_eq!(usize::from(read.offset()), value.len());
        assert_eq!(read.value(), complete.then_some(value.as_slice()));
    }
}

#[test]
fn fuzz_long_reads() {
    let target = Target {
        name: "long-read",
        corpus: include_str!("../tests/fuzz-corpus/long-read.txt"),
        max_len: 48,
    };
    run(&target, &[], check_long_read);
}
