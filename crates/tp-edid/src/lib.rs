//! EDID parsing.
//!
//! EDID is the 128-byte block every monitor reports about itself. Team
//! Principal needs three things out of it:
//!
//! 1. **A stable identity.** Monitors are bound to rig screens by manufacturer,
//!    product code and serial — not by device path, which changes when a cable
//!    moves between ports.
//! 2. **Physical size in millimetres.** The FOV calculator needs real
//!    dimensions, and the user should never have to type them.
//! 3. **The preferred timing**, which is the panel's native resolution.
//!
//! ## On precision
//!
//! EDID states physical size twice. Bytes 0x15/0x16 give it in **whole
//! centimetres**, so a 1193 mm panel reports `119` and quantises to ±5 mm. The
//! first detailed timing descriptor gives the same dimensions in
//! **millimetres**. This parser prefers the descriptor and flags which source
//! it used, because ±5 mm at 700 mm is ±0.3° of FOV and the user deserves to
//! know whether a number was measured or rounded.
//!
//! ## On strictness
//!
//! A bad checksum does not reject the blob. Shipping monitors get this wrong,
//! and refusing to parse means the user's screen simply does not appear in the
//! app with no explanation. The checksum result is reported instead, so the UI
//! can note it while still using the data.

// Public rather than test-gated: integration tests link the library as an
// external crate, so a `cfg(test)` module would be invisible to them. It is
// also genuinely useful outside tests — generating a blob for a bug report
// beats asking someone to dump 128 bytes of their monitor.
pub mod build;
pub mod source;

use tp_model::{EdidIdentity, Mm, PhysicalSize};

const BLOCK_LEN: usize = 128;
const HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EdidError {
    #[error("EDID block is {0} bytes, needs at least {BLOCK_LEN}")]
    TooShort(usize),
    #[error("EDID header is not the expected 00 FF FF FF FF FF FF 00")]
    BadHeader,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edid {
    /// Three-character PNP vendor ID, e.g. "SAM", "DEL".
    pub manufacturer_id: String,
    pub product_code: u16,
    pub serial_number: u32,
    /// Week of manufacture, 1..=54. `None` when the byte carries the model-year
    /// flag instead.
    pub week: Option<u8>,
    pub year: u16,
    /// True when `year` is a model year rather than a manufacture year.
    pub is_model_year: bool,
    pub version: (u8, u8),
    /// Descriptor tag 0xFC. Often absent, often padded, sometimes wrong.
    pub monitor_name: Option<String>,
    /// Descriptor tag 0xFF. The best disambiguator between two identical
    /// panels — and plenty of monitors do not provide it.
    pub serial_string: Option<String>,
    pub physical_size: Option<Size>,
    /// The base block's preferred timing.
    ///
    /// **Not necessarily the native resolution.** The format gives active
    /// pixels 12 bits, capping this at 4095 in each axis, and the pixel clock
    /// is a u16 of 10 kHz units, capping it at 655.35 MHz. A 5120x1440 panel
    /// at 240 Hz exceeds both by a wide margin and advertises those modes in
    /// extension blocks instead. So the app takes resolution and refresh from
    /// Windows, and uses EDID for identity and physical size — the two things
    /// Windows will not tell it.
    pub preferred_mode: Option<Mode>,
    pub extension_count: u8,
    /// False when the block's bytes do not sum to zero mod 256. Reported
    /// rather than fatal: see the module docs.
    pub checksum_ok: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width_mm: f64,
    pub height_mm: f64,
    /// True when taken from the detailed timing descriptor (millimetres),
    /// false when from bytes 0x15/0x16 (whole centimetres, ±5 mm).
    pub millimetre_precision: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f64,
}

impl Edid {
    /// The identity a rig screen is bound by.
    pub fn identity(&self, cached_device_path: Option<String>) -> EdidIdentity {
        EdidIdentity {
            manufacturer_id: self.manufacturer_id.clone(),
            product_code: self.product_code,
            serial: self.serial_string.clone(),
            serial_number: self.serial_number,
            week_year: (self.week.unwrap_or(0), self.year),
            cached_device_path,
        }
    }

    pub fn physical_size_model(&self) -> Option<PhysicalSize> {
        self.physical_size.map(|s| PhysicalSize {
            width: Mm(s.width_mm),
            height: Mm(s.height_mm),
            millimetre_precision: s.millimetre_precision,
        })
    }

    /// True when this blob cannot be told apart from another of the same model.
    ///
    /// Two panels of one model with no serial string and a zero serial number
    /// are genuinely indistinguishable, and the app must say so and ask which
    /// is which rather than guessing and swapping the user's screens.
    pub fn is_ambiguous(&self) -> bool {
        self.serial_string.is_none() && self.serial_number == 0
    }
}

pub fn parse(bytes: &[u8]) -> Result<Edid, EdidError> {
    if bytes.len() < BLOCK_LEN {
        return Err(EdidError::TooShort(bytes.len()));
    }
    let b = &bytes[..BLOCK_LEN];
    if b[..8] != HEADER {
        return Err(EdidError::BadHeader);
    }

    let checksum_ok = b.iter().fold(0u8, |acc, x| acc.wrapping_add(*x)) == 0;

    // Bytes 0x10/0x11: 0xFF in the week byte means byte 0x11 is a model year.
    let (week, year, is_model_year) = if b[0x10] == 0xFF {
        (None, 1990 + b[0x11] as u16, true)
    } else {
        let w = b[0x10];
        (
            if w == 0 { None } else { Some(w) },
            1990 + b[0x11] as u16,
            false,
        )
    };

    let mut monitor_name = None;
    let mut serial_string = None;
    let mut dtd_size = None;
    let mut preferred_mode = None;

    // Four 18-byte descriptors at 0x36. The first is conventionally the
    // preferred timing; any of them may instead be a display descriptor.
    for i in 0..4 {
        let d = &b[0x36 + i * 18..0x36 + (i + 1) * 18];
        if d[0] == 0 && d[1] == 0 {
            match d[3] {
                0xFC => monitor_name = read_descriptor_text(&d[5..18]),
                0xFF => serial_string = read_descriptor_text(&d[5..18]),
                _ => {}
            }
        } else if preferred_mode.is_none() {
            let (mode, size) = parse_detailed_timing(d);
            preferred_mode = Some(mode);
            dtd_size = size;
        }
    }

    // Prefer the descriptor's millimetres over the header's centimetres.
    let physical_size = dtd_size.or_else(|| {
        let (w, h) = (b[0x15], b[0x16]);
        // Both zero means "undefined" — commonly a projector, or a panel that
        // simply declines to say. Inventing a size would be worse than none.
        (w != 0 && h != 0).then_some(Size {
            width_mm: w as f64 * 10.0,
            height_mm: h as f64 * 10.0,
            millimetre_precision: false,
        })
    });

    Ok(Edid {
        manufacturer_id: decode_manufacturer(u16::from_be_bytes([b[0x08], b[0x09]])),
        product_code: u16::from_le_bytes([b[0x0A], b[0x0B]]),
        serial_number: u32::from_le_bytes([b[0x0C], b[0x0D], b[0x0E], b[0x0F]]),
        week,
        year,
        is_model_year,
        version: (b[0x12], b[0x13]),
        monitor_name,
        serial_string,
        physical_size,
        preferred_mode,
        extension_count: b[0x7E],
        checksum_ok,
    })
}

/// Three 5-bit letters packed big-endian, 1 = 'A'. Bit 15 is reserved zero.
fn decode_manufacturer(m: u16) -> String {
    let letter = |v: u16| -> char {
        let v = (v & 0x1F) as u8;
        if (1..=26).contains(&v) {
            (b'A' + v - 1) as char
        } else {
            '?'
        }
    };
    [letter(m >> 10), letter(m >> 5), letter(m)]
        .iter()
        .collect()
}

/// Descriptor text is ASCII terminated by 0x0A and padded with spaces.
fn read_descriptor_text(bytes: &[u8]) -> Option<String> {
    let end = bytes.iter().position(|&c| c == 0x0A).unwrap_or(bytes.len());
    let s: String = bytes[..end]
        .iter()
        .map(|&c| {
            if (0x20..0x7F).contains(&c) {
                c as char
            } else {
                ' '
            }
        })
        .collect();
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// An 18-byte detailed timing descriptor.
///
/// Sizes and counts are split across low bytes and packed high nibbles, which
/// is where hand-rolled EDID parsers usually go wrong.
fn parse_detailed_timing(d: &[u8]) -> (Mode, Option<Size>) {
    let pixel_clock_hz = u16::from_le_bytes([d[0], d[1]]) as f64 * 10_000.0;

    let h_active = d[2] as u32 | ((d[4] as u32 & 0xF0) << 4);
    let h_blank = d[3] as u32 | ((d[4] as u32 & 0x0F) << 8);
    let v_active = d[5] as u32 | ((d[7] as u32 & 0xF0) << 4);
    let v_blank = d[6] as u32 | ((d[7] as u32 & 0x0F) << 8);

    let h_total = h_active + h_blank;
    let v_total = v_active + v_blank;
    let refresh_hz = if h_total > 0 && v_total > 0 {
        pixel_clock_hz / (h_total as f64 * v_total as f64)
    } else {
        0.0
    };

    let w_mm = d[12] as u32 | ((d[14] as u32 & 0xF0) << 4);
    let h_mm = d[13] as u32 | ((d[14] as u32 & 0x0F) << 8);
    let size = (w_mm != 0 && h_mm != 0).then_some(Size {
        width_mm: w_mm as f64,
        height_mm: h_mm as f64,
        millimetre_precision: true,
    });

    (
        Mode {
            width: h_active,
            height: v_active,
            refresh_hz,
        },
        size,
    )
}
