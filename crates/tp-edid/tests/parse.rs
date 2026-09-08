//! Parser tests against byte-accurate constructed blobs.

use tp_edid::build::{EdidBuilder, Timing};
use tp_edid::{parse, EdidError};

/// A 49" 5120x1440 super-ultrawide, shaped like a real one.
///
/// Its base-block preferred timing is 3840x1080 at 60 Hz, *not* its native
/// 5120x1440 at 240 Hz. That is not a simplification — the base block cannot
/// hold the native mode. Active pixels get 12 bits (max 4095) and the pixel
/// clock is a u16 of 10 kHz units (max 655.35 MHz), while 5120x1440 at 240 Hz
/// needs 5120 pixels and about 1.94 GHz. Monitors like this advertise their
/// real modes in CTA-861 extension blocks, which is exactly why the app takes
/// resolution and refresh from Windows and uses EDID only for identity and
/// physical size.
///
/// The physical size in the descriptor is still the panel's true size; it does
/// not depend on the timing.
fn ultrawide_49() -> Vec<u8> {
    EdidBuilder::new("SAM", 0x7179)
        .serial_number(0x0102_0304)
        .made(12, 2023)
        // 4000 x 1125 total at 270.00 MHz -> 60.0 Hz
        .preferred_timing(Timing {
            h_active: 3840,
            h_blank: 160,
            v_active: 1080,
            v_blank: 45,
            pixel_clock_10khz: 27000,
            width_mm: 1193,
            height_mm: 336,
        })
        .size_cm(119, 34)
        .text_descriptor(1, 0xFC, "Odyssey G9")
        .text_descriptor(2, 0xFF, "HNMX500123")
        .extensions(1)
        .build()
}

fn dell_27() -> Vec<u8> {
    EdidBuilder::new("DEL", 0x41CF)
        .serial_number(0x0000_1234)
        .made(30, 2022)
        .preferred_timing(Timing {
            h_active: 2560,
            h_blank: 160,
            v_active: 1440,
            v_blank: 80,
            pixel_clock_10khz: 24806,
            width_mm: 597,
            height_mm: 336,
        })
        .size_cm(60, 34)
        .text_descriptor(1, 0xFC, "DELL S2721DGF")
        .text_descriptor(2, 0xFF, "9GH2Z13")
        .build()
}

#[test]
fn reads_identity() {
    let e = parse(&ultrawide_49()).unwrap();
    assert_eq!(e.manufacturer_id, "SAM");
    assert_eq!(e.product_code, 0x7179);
    assert_eq!(e.serial_number, 0x0102_0304);
    assert_eq!(e.week, Some(12));
    assert_eq!(e.year, 2023);
    assert!(!e.is_model_year);
    assert_eq!(e.version, (1, 4));
    assert_eq!(e.extension_count, 1);
    assert!(e.checksum_ok);
}

#[test]
fn reads_descriptor_text_without_padding() {
    let e = parse(&ultrawide_49()).unwrap();
    // Descriptor text is 0x0A-terminated and space-padded to 13 bytes; neither
    // the terminator nor the padding may survive into the string.
    assert_eq!(e.monitor_name.as_deref(), Some("Odyssey G9"));
    assert_eq!(e.serial_string.as_deref(), Some("HNMX500123"));
}

#[test]
fn prefers_millimetres_over_centimetres() {
    // The blob says 119 cm in the header and 1193 mm in the descriptor. Taking
    // the header value would lose 3 mm and, at 700 mm viewing distance, about
    // 0.2 deg of FOV — with nothing telling the user a rounding happened.
    let e = parse(&ultrawide_49()).unwrap();
    let size = e.physical_size.expect("size");
    assert_eq!(size.width_mm, 1193.0);
    assert_eq!(size.height_mm, 336.0);
    assert!(size.millimetre_precision);
}

#[test]
fn falls_back_to_centimetres_and_says_so() {
    // No detailed timing at all: some panels only fill in the header bytes.
    let blob = EdidBuilder::new("ACR", 0x0001).size_cm(60, 34).build();
    let e = parse(&blob).unwrap();
    let size = e.physical_size.expect("size");
    assert_eq!((size.width_mm, size.height_mm), (600.0, 340.0));
    assert!(
        !size.millimetre_precision,
        "a centimetre-derived size must be flagged so the UI can offer a manual override"
    );
}

#[test]
fn no_size_at_all_is_none_not_zero() {
    // Reporting 0x0 mm would sail into the FOV calculator and produce a
    // division by zero or a nonsense angle. None makes the UI ask.
    let blob = EdidBuilder::new("ACR", 0x0002).build();
    assert!(parse(&blob).unwrap().physical_size.is_none());
}

#[test]
fn decodes_packed_timing_fields() {
    let e = parse(&ultrawide_49()).unwrap();
    let m = e.preferred_mode.expect("mode");
    // 3840 needs the high nibble of byte 4 and 1080 the high nibble of byte 7.
    // Dropping either silently truncates to 768 / 56.
    assert_eq!((m.width, m.height), (3840, 1080));
    assert!(
        (m.refresh_hz - 60.0).abs() < 0.01,
        "refresh {}",
        m.refresh_hz
    );

    let d = parse(&dell_27()).unwrap().preferred_mode.expect("mode");
    assert_eq!((d.width, d.height), (2560, 1440));
    assert!(
        (d.refresh_hz - 60.0).abs() < 0.01,
        "refresh {}",
        d.refresh_hz
    );
}

#[test]
fn model_year_form_is_distinguished() {
    let blob = EdidBuilder::new("LGD", 0x0003).model_year(2024).build();
    let e = parse(&blob).unwrap();
    assert_eq!(e.year, 2024);
    assert!(e.is_model_year);
    assert_eq!(e.week, None);
}

#[test]
fn a_bad_checksum_is_reported_not_fatal() {
    // Shipping monitors get this wrong. Rejecting the blob would make the
    // user's screen silently absent from the app, which is worse.
    let blob = EdidBuilder::new("SAM", 0x7179)
        .serial_number(7)
        .size_cm(119, 34)
        .build_with_bad_checksum();
    let e = parse(&blob).expect("must still parse");
    assert!(!e.checksum_ok);
    assert_eq!(e.manufacturer_id, "SAM");
}

#[test]
fn rejects_what_is_not_edid() {
    assert_eq!(parse(&[0u8; 40]), Err(EdidError::TooShort(40)));
    assert_eq!(parse(&[0u8; 128]), Err(EdidError::BadHeader));
}

#[test]
fn detects_indistinguishable_panels() {
    // Two of the same model with no serial string and a zero serial number
    // cannot be told apart. The app must say so and ask which is which rather
    // than guess and swap the user's left and right screens.
    let anonymous = EdidBuilder::new("ACR", 0x0001).size_cm(60, 34).build();
    assert!(parse(&anonymous).unwrap().is_ambiguous());

    assert!(!parse(&ultrawide_49()).unwrap().is_ambiguous());

    // A serial number alone is enough, even with no serial string.
    let numbered = EdidBuilder::new("ACR", 0x0001).serial_number(42).build();
    assert!(!parse(&numbered).unwrap().is_ambiguous());
}

#[test]
fn maps_into_the_binding_identity() {
    let e = parse(&dell_27()).unwrap();
    let id = e.identity(Some(r"\\?\DISPLAY#DELA1CF#1&C".into()));
    assert_eq!(id.manufacturer_id, "DEL");
    assert_eq!(id.product_code, 0x41CF);
    assert_eq!(id.serial.as_deref(), Some("9GH2Z13"));
    assert_eq!(id.week_year, (30, 2022));
    assert!(id.cached_device_path.is_some());
}

#[test]
fn trailing_extension_blocks_are_ignored_not_rejected() {
    // A monitor that reports extension blocks hands us more than 128 bytes.
    let mut blob = ultrawide_49();
    blob.extend_from_slice(&[0x02; 128]);
    let e = parse(&blob).unwrap();
    assert_eq!(e.manufacturer_id, "SAM");
    assert!(
        e.checksum_ok,
        "the base block checksum must not include the extension"
    );
}

#[test]
#[should_panic(expected = "does not fit EDID's 12-bit field")]
fn the_base_block_cannot_express_a_5120_wide_panel() {
    // Documents a format limit that shapes the whole display module: EDID is
    // the source of truth for *identity and physical size*, never for the
    // resolution the panel is actually running. Windows owns that.
    let _ = EdidBuilder::new("SAM", 0x7179).preferred_timing(Timing {
        h_active: 5120,
        h_blank: 160,
        v_active: 1440,
        v_blank: 90,
        pixel_clock_10khz: 48470,
        width_mm: 1193,
        height_mm: 336,
    });
}
