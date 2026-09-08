//! Turning raw HID report values into something a person can read.
//!
//! A HID device reports each axis as an integer in whatever range it declares,
//! and the declarations vary wildly: an 8-bit gamepad stick is 0..255, a
//! load-cell pedal set is 0..65535, and some devices declare a *signed* range
//! like -32768..32767. Getting the conversion wrong produces a bar that sits at
//! half deflection when the pedal is up, or one that moves backwards.
//!
//! All of that is arithmetic, so it lives here and is tested, rather than being
//! discovered by pressing a brake pedal and squinting.

/// HID usage IDs on the Generic Desktop page (0x01) that sim hardware uses.
///
/// Named rather than numbered at the call site, because `0x33` meaning "the
/// axis a wheel's rotation usually lands on" is exactly the kind of thing that
/// gets mis-transcribed once and is then wrong forever.
pub fn axis_name(usage: u16) -> &'static str {
    match usage {
        0x30 => "X",
        0x31 => "Y",
        0x32 => "Z",
        0x33 => "Rx",
        0x34 => "Ry",
        0x35 => "Rz",
        0x36 => "Slider",
        0x37 => "Dial",
        0x38 => "Wheel",
        0x39 => "Hat",
        _ => "Axis",
    }
}

/// Is this usage one of the axes worth showing?
pub fn is_axis(usage_page: u16, usage: u16) -> bool {
    usage_page == 0x01 && (0x30..=0x38).contains(&usage)
}

/// Normalise a raw report value to `-1.0 ..= 1.0`.
///
/// `bit_size` is needed because a device declaring a signed logical range
/// reports the value as an unsigned field of that width; it has to be
/// sign-extended before it means anything. Without that a brake at rest reads
/// as fully pressed.
pub fn normalise(raw: u32, logical_min: i32, logical_max: i32, bit_size: u16) -> f64 {
    // A zero-width or inverted range is a malformed descriptor. Return centre
    // rather than dividing by zero or producing an infinity that then paints a
    // bar off the side of the screen.
    if logical_max <= logical_min {
        return 0.0;
    }

    let value = if logical_min < 0 {
        sign_extend(raw, bit_size)
    } else {
        raw as i64
    };
    let span = logical_max as f64 - logical_min as f64;
    let t = (value as f64 - logical_min as f64) / span;
    (t.clamp(0.0, 1.0) * 2.0 - 1.0).clamp(-1.0, 1.0)
}

/// Normalise to `0.0 ..= 1.0`, which is what a pedal wants: a throttle at rest
/// should read zero, not minus one.
pub fn normalise_unipolar(raw: u32, logical_min: i32, logical_max: i32, bit_size: u16) -> f64 {
    (normalise(raw, logical_min, logical_max, bit_size) + 1.0) / 2.0
}

/// Interpret the low `bits` of `raw` as a two's-complement signed value.
fn sign_extend(raw: u32, bits: u16) -> i64 {
    if bits == 0 || bits >= 32 {
        return raw as i32 as i64;
    }
    let sign_bit = 1u32 << (bits - 1);
    let mask = (1u64 << bits) - 1;
    let v = (raw as u64) & mask;
    if v & (sign_bit as u64) != 0 {
        (v as i64) - (1i64 << bits)
    } else {
        v as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unsigned_axis_maps_end_to_end() {
        // A typical 16-bit load cell: 0 at rest, full scale pressed.
        assert_eq!(normalise(0, 0, 65535, 16), -1.0);
        assert_eq!(normalise(65535, 0, 65535, 16), 1.0);
        assert!(
            (normalise(32767, 0, 65535, 16)).abs() < 0.001,
            "centre is centre"
        );
    }

    #[test]
    fn a_pedal_reads_zero_at_rest_in_unipolar() {
        // The bar for a throttle should be empty when your foot is off it.
        assert_eq!(normalise_unipolar(0, 0, 65535, 16), 0.0);
        assert_eq!(normalise_unipolar(65535, 0, 65535, 16), 1.0);
    }

    #[test]
    fn a_signed_axis_is_sign_extended() {
        // A wheel declaring -32768..32767 reports the value as an unsigned
        // 16-bit field. Reading it as unsigned puts centre at full lock.
        assert!(
            (normalise(0, -32768, 32767, 16)).abs() < 0.001,
            "0 raw is centre"
        );
        assert_eq!(
            normalise(0x8000, -32768, 32767, 16),
            -1.0,
            "most negative is full left"
        );
        assert!(
            (normalise(0x7FFF, -32768, 32767, 16) - 1.0).abs() < 0.001,
            "full right"
        );
    }

    #[test]
    fn eight_bit_devices_work_too() {
        assert_eq!(normalise(0, 0, 255, 8), -1.0);
        assert_eq!(normalise(255, 0, 255, 8), 1.0);
        assert_eq!(
            normalise(0x80, -128, 127, 8),
            -1.0,
            "signed 8-bit sign-extends"
        );
    }

    #[test]
    fn a_malformed_range_is_centre_not_infinity() {
        // A descriptor with min == max would otherwise divide by zero and paint
        // a bar off the side of the screen.
        assert_eq!(normalise(500, 100, 100, 16), 0.0);
        assert_eq!(normalise(500, 100, 50, 16), 0.0);
    }

    #[test]
    fn values_outside_the_declared_range_are_clamped() {
        // Devices do report outside their own declared range. Clamping keeps
        // the bar inside its track instead of overflowing the layout.
        assert_eq!(normalise(70000, 0, 65535, 17), 1.0);
        assert!((-1.0..=1.0).contains(&normalise(u32::MAX, 0, 65535, 32)));
    }

    #[test]
    fn axis_names_cover_what_sim_hardware_reports() {
        assert_eq!(axis_name(0x30), "X");
        assert_eq!(axis_name(0x35), "Rz");
        assert_eq!(axis_name(0x36), "Slider");
        assert_eq!(
            axis_name(0xFF),
            "Axis",
            "an unknown usage is still shown, just unnamed"
        );
    }

    #[test]
    fn only_generic_desktop_axes_are_axes() {
        assert!(is_axis(0x01, 0x30));
        assert!(is_axis(0x01, 0x35));
        assert!(!is_axis(0x01, 0x39), "a hat is a direction, not an axis");
        assert!(!is_axis(0x09, 0x30), "usage page 9 is buttons");
        assert!(!is_axis(0x02, 0x30), "simulation controls page is not this");
    }

    #[test]
    fn sign_extension_handles_the_edges() {
        assert_eq!(sign_extend(0, 16), 0);
        assert_eq!(sign_extend(0x7FFF, 16), 32767);
        assert_eq!(sign_extend(0x8000, 16), -32768);
        assert_eq!(sign_extend(0xFFFF, 16), -1);
        // Degenerate widths must not panic on a shift overflow.
        assert_eq!(sign_extend(5, 0), 5);
        assert_eq!(sign_extend(5, 64), 5);
    }
}
