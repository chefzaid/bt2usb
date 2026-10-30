//! Host tests for the GATT HID client's decisions (`ble/hid_gatt.rs`): what
//! each Report characteristic is used for, how a Report Map long read ends or
//! fails, and which notifications reach the classifier.

use super::*;
use crate::hid::keyboard::KEYBOARD_REPORT_DESCRIPTOR;

/// A composite peer that numbers its keyboard, mouse, and consumer reports.
const COMPOSITE: HidDescriptor = HidDescriptor {
    has_keyboard: true,
    has_mouse: true,
    has_consumer: true,
    keyboard_report_id: Some(1),
    mouse_report_id: Some(2),
    consumer_report_id: Some(3),
};

/// A plain keyboard whose map numbers no reports.
const KEYBOARD_ONLY: HidDescriptor = HidDescriptor {
    has_keyboard: true,
    has_mouse: false,
    has_consumer: false,
    keyboard_report_id: None,
    mouse_report_id: None,
    consumer_report_id: None,
};

/// A keyboard and a mouse without report IDs: no reference names either.
const UNNUMBERED_COMPOSITE: HidDescriptor = HidDescriptor {
    has_mouse: true,
    ..KEYBOARD_ONLY
};

/// One report ID holding keyboard and mouse fields, which no decoder handles.
const SHARED_ID: HidDescriptor = HidDescriptor {
    has_keyboard: true,
    has_mouse: true,
    has_consumer: false,
    keyboard_report_id: Some(1),
    mouse_report_id: Some(1),
    consumer_report_id: None,
};

fn reference(report_id: u8, report_type: ReportType) -> Option<ReportReference> {
    Some(ReportReference {
        report_id,
        report_type,
    })
}

// ── Report characteristics ───────────────────────────────────────────────────

#[test]
fn a_report_without_a_cccd_is_used_only_as_the_keyboard_led_output() {
    let output = |id| reference(id, ReportType::Output);
    assert_eq!(
        report_use(false, output(1), Some(&COMPOSITE)),
        ReportUse::KeyboardLeds
    );
    // Every report of an unnumbered keyboard-only map is the keyboard's.
    assert_eq!(
        report_use(false, output(0), Some(&KEYBOARD_ONLY)),
        ReportUse::KeyboardLeds
    );
    for (reference, descriptor) in [
        (output(2), Some(&COMPOSITE)),
        (output(9), Some(&COMPOSITE)),
        (output(0), Some(&UNNUMBERED_COMPOSITE)),
        (output(1), Some(&SHARED_ID)),
        // Without a map nothing says the output belongs to a keyboard.
        (output(1), None),
        (None, Some(&COMPOSITE)),
        (reference(1, ReportType::Input), Some(&COMPOSITE)),
        (reference(1, ReportType::Feature), Some(&COMPOSITE)),
    ] {
        assert_eq!(
            report_use(false, reference, descriptor),
            ReportUse::Ignore,
            "{reference:?}"
        );
    }
}

#[test]
fn a_cccd_never_turns_an_output_or_feature_report_into_input() {
    for report_type in [ReportType::Output, ReportType::Feature] {
        for descriptor in [Some(&COMPOSITE), Some(&KEYBOARD_ONLY), None] {
            assert_eq!(
                report_use(true, reference(1, report_type), descriptor),
                ReportUse::Ignore
            );
        }
    }
}

#[test]
fn an_input_report_subscribes_with_the_kind_its_reference_names() {
    let input = |id| reference(id, ReportType::Input);
    for (id, kind) in [
        (1, ReportKind::Keyboard),
        (2, ReportKind::Mouse),
        (3, ReportKind::Consumer),
    ] {
        assert_eq!(
            report_use(true, input(id), Some(&COMPOSITE)),
            ReportUse::Subscribe(Some(kind))
        );
    }
    // An unnumbered map, or none at all, leaves the kind to the classifier.
    for (reference, descriptor) in [
        (input(0), Some(&KEYBOARD_ONLY)),
        (None, Some(&UNNUMBERED_COMPOSITE)),
        (input(1), None),
        (None, None),
    ] {
        assert_eq!(
            report_use(true, reference, descriptor),
            ReportUse::Subscribe(None)
        );
    }
}

#[test]
fn a_numbered_map_skips_inputs_it_does_not_resolve_to_one_kind() {
    for (reference, descriptor) in [
        (reference(9, ReportType::Input), &COMPOSITE),
        // The reference was missing or could not be read.
        (None, &COMPOSITE),
        (reference(1, ReportType::Input), &SHARED_ID),
    ] {
        assert_eq!(
            report_use(true, reference, Some(descriptor)),
            ReportUse::Unresolved,
            "{reference:?}"
        );
    }
}

// ── Report Map long read ─────────────────────────────────────────────────────

#[test]
fn a_report_map_read_completes_on_a_short_fragment() {
    let mut read = LongRead::new(23).unwrap();
    let mut chunks = KEYBOARD_REPORT_DESCRIPTOR.chunks(22).peekable();
    assert_ne!(
        KEYBOARD_REPORT_DESCRIPTOR.len() % 22,
        0,
        "needs a short end"
    );
    while let Some(chunk) = chunks.next() {
        let last = chunks.peek().is_none();
        assert_eq!(report_map_step(&mut read, Fragment::Data(chunk)), Ok(last));
    }
    let descriptor = report_map(&read).unwrap();
    assert!(descriptor.has_keyboard && !descriptor.has_mouse);
}

#[test]
fn a_report_map_that_fills_whole_fragments_ends_on_an_end_response() {
    let mut read = LongRead::new(23).unwrap();
    assert_eq!(
        report_map_step(&mut read, Fragment::Data(&[1; 22])),
        Ok(false)
    );
    assert_eq!(
        report_map_step(&mut read, Fragment::Data(&[1; 22])),
        Ok(false)
    );
    assert_eq!(
        report_map_step(&mut read, Fragment::InvalidOffset),
        Ok(true)
    );
    assert_eq!(read.value().map(<[u8]>::len), Some(44));

    let mut read = LongRead::new(23).unwrap();
    assert_eq!(
        report_map_step(&mut read, Fragment::Data(&[1; 22])),
        Ok(false)
    );
    assert_eq!(
        report_map_step(&mut read, Fragment::AttributeNotLong),
        Ok(true)
    );
    assert_eq!(read.value().map(<[u8]>::len), Some(22));
}

#[test]
fn a_report_map_read_that_ends_wrongly_fails_as_unreadable() {
    // Attribute Not Long is valid only after the first fragment.
    let mut read = LongRead::new(23).unwrap();
    report_map_step(&mut read, Fragment::Data(&[1; 22])).unwrap();
    report_map_step(&mut read, Fragment::Data(&[1; 22])).unwrap();
    assert_eq!(
        report_map_step(&mut read, Fragment::AttributeNotLong),
        Err(ErrorTag::ReportMapReadFailed)
    );
    // An end response before any data, a fragment longer than the MTU allows,
    // data after the value ended, and any other failed request.
    let mut empty = LongRead::new(23).unwrap();
    assert_eq!(
        report_map_step(&mut empty, Fragment::InvalidOffset),
        Err(ErrorTag::ReportMapReadFailed)
    );
    assert_eq!(
        report_map_step(&mut empty, Fragment::Data(&[1; 23])),
        Err(ErrorTag::ReportMapReadFailed)
    );
    let mut done = LongRead::new(23).unwrap();
    assert_eq!(
        report_map_step(&mut done, Fragment::Data(&[1; 4])),
        Ok(true)
    );
    assert_eq!(
        report_map_step(&mut done, Fragment::Data(&[1])),
        Err(ErrorTag::ReportMapReadFailed)
    );
    assert_eq!(
        report_map_step(&mut LongRead::new(23).unwrap(), Fragment::Failed),
        Err(ErrorTag::ReportMapReadFailed)
    );
}

#[test]
fn a_report_map_longer_than_an_attribute_fails_as_too_large() {
    let mut read = LongRead::new(65).unwrap();
    for _ in 0..8 {
        assert_eq!(
            report_map_step(&mut read, Fragment::Data(&[1; 64])),
            Ok(false)
        );
    }
    assert_eq!(
        report_map_step(&mut read, Fragment::Data(&[1])),
        Err(ErrorTag::ReportMapTooLarge)
    );
    assert_eq!(
        report_map_step(&mut LongRead::new(23).unwrap(), Fragment::Truncated),
        Err(ErrorTag::ReportMapTooLarge)
    );
}

#[test]
fn only_a_complete_parsable_report_map_is_used() {
    let mut read = LongRead::new(23).unwrap();
    report_map_step(&mut read, Fragment::Data(&[0x05; 22])).unwrap();
    assert_eq!(report_map(&read).err(), Some(ErrorTag::ReportMapReadFailed));
    // A Usage Page item whose data byte is missing.
    let mut read = LongRead::new(23).unwrap();
    report_map_step(&mut read, Fragment::Data(&[0x05])).unwrap();
    assert_eq!(report_map(&read).err(), Some(ErrorTag::ReportMapInvalid));
}

// ── Notifications ────────────────────────────────────────────────────────────

const SUBSCRIPTIONS: [Subscription; 2] = [
    Subscription {
        value_handle: 0x20,
        kind: Some(ReportKind::Keyboard),
    },
    Subscription {
        value_handle: 0x24,
        kind: None,
    },
];

#[test]
fn a_notification_carries_its_subscription_kind_and_whole_payload() {
    let key_a = [0, 0, 4, 0, 0, 0, 0, 0];
    let note = notification(&SUBSCRIPTIONS, 0x20, &key_a).unwrap();
    assert_eq!(note.kind, Some(ReportKind::Keyboard));
    assert_eq!(note.data.as_slice(), key_a);
    let longest = notification(&SUBSCRIPTIONS, 0x24, &[1; MAX_REPORT_LEN]).unwrap();
    assert_eq!(longest.kind, None);
    assert_eq!(longest.data.len(), MAX_REPORT_LEN);
}

#[test]
fn notifications_from_other_handles_or_too_long_to_copy_are_dropped() {
    assert_eq!(notification(&SUBSCRIPTIONS, 0x30, &[0; 8]), None);
    assert_eq!(notification(&[], 0x20, &[0; 8]), None);
    assert_eq!(
        notification(&SUBSCRIPTIONS, 0x24, &[1; MAX_REPORT_LEN + 1]),
        None
    );
}
