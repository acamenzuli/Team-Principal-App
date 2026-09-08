//! A builder for EDID blobs, used by the tests.
//!
//! Real captured blobs would be better, but they cannot be obtained without the
//! hardware. This constructs byte-accurate ones — correct packing, correct
//! checksum — so the parser is tested against the format rather than against
//! whatever the parser itself happens to produce.
//!
//! Also useful for reproducing a monitor from a bug report without owning it.

/// The fields of a detailed timing descriptor, named so a call site cannot
/// transpose two of them unnoticed.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub h_active: u32,
    pub h_blank: u32,
    pub v_active: u32,
    pub v_blank: u32,
    /// Pixel clock in units of 10 kHz. A u16, so the ceiling is 655.35 MHz.
    pub pixel_clock_10khz: u16,
    pub width_mm: u32,
    pub height_mm: u32,
}

pub struct EdidBuilder {
    bytes: [u8; 128],
}

impl EdidBuilder {
    pub fn new(manufacturer: &str, product_code: u16) -> Self {
        let mut bytes = [0u8; 128];
        bytes[..8].copy_from_slice(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);

        let l = |c: u8| ((c - b'A' + 1) & 0x1F) as u16;
        let m = manufacturer.as_bytes();
        let packed = (l(m[0]) << 10) | (l(m[1]) << 5) | l(m[2]);
        bytes[0x08..0x0A].copy_from_slice(&packed.to_be_bytes());
        bytes[0x0A..0x0C].copy_from_slice(&product_code.to_le_bytes());

        bytes[0x12] = 1; // EDID version
        bytes[0x13] = 4; // revision
        Self { bytes }
    }

    pub fn serial_number(mut self, n: u32) -> Self {
        self.bytes[0x0C..0x10].copy_from_slice(&n.to_le_bytes());
        self
    }

    pub fn made(mut self, week: u8, year: u16) -> Self {
        self.bytes[0x10] = week;
        self.bytes[0x11] = (year - 1990) as u8;
        self
    }

    /// Sets the model-year form: 0xFF in the week byte.
    pub fn model_year(mut self, year: u16) -> Self {
        self.bytes[0x10] = 0xFF;
        self.bytes[0x11] = (year - 1990) as u8;
        self
    }

    /// Bytes 0x15/0x16 — whole centimetres.
    pub fn size_cm(mut self, w: u8, h: u8) -> Self {
        self.bytes[0x15] = w;
        self.bytes[0x16] = h;
        self
    }

    /// The preferred timing descriptor, at 0x36.
    pub fn preferred_timing(mut self, t: Timing) -> Self {
        let Timing {
            h_active,
            h_blank,
            v_active,
            v_blank,
            pixel_clock_10khz,
            width_mm: w_mm,
            height_mm: h_mm,
        } = t;
        // The format gives active and blanking 12 bits each. Silently
        // truncating is how a 5120-wide panel becomes 1024 with no warning, so
        // refuse instead — a test fixture that lies is worse than no fixture.
        for (name, v) in [
            ("h_active", h_active),
            ("h_blank", h_blank),
            ("v_active", v_active),
            ("v_blank", v_blank),
        ] {
            assert!(
                v <= 0xFFF,
                "{name}={v} does not fit EDID's 12-bit field (max 4095). Real monitors \
                 wider than that carry their native mode in a CTA-861 or DisplayID \
                 extension block, not in the base detailed timing."
            );
        }
        let d = &mut self.bytes[0x36..0x48];
        d[0..2].copy_from_slice(&pixel_clock_10khz.to_le_bytes());
        d[2] = (h_active & 0xFF) as u8;
        d[3] = (h_blank & 0xFF) as u8;
        d[4] = (((h_active >> 8) & 0x0F) << 4) as u8 | ((h_blank >> 8) & 0x0F) as u8;
        d[5] = (v_active & 0xFF) as u8;
        d[6] = (v_blank & 0xFF) as u8;
        d[7] = (((v_active >> 8) & 0x0F) << 4) as u8 | ((v_blank >> 8) & 0x0F) as u8;
        d[12] = (w_mm & 0xFF) as u8;
        d[13] = (h_mm & 0xFF) as u8;
        d[14] = (((w_mm >> 8) & 0x0F) << 4) as u8 | ((h_mm >> 8) & 0x0F) as u8;
        self
    }

    /// A display descriptor in slot 1..=3 (0x48, 0x5A, 0x6C).
    pub fn text_descriptor(mut self, slot: usize, tag: u8, text: &str) -> Self {
        let at = 0x36 + slot * 18;
        let d = &mut self.bytes[at..at + 18];
        d[0..3].copy_from_slice(&[0, 0, 0]);
        d[3] = tag;
        d[4] = 0;
        let src = text.as_bytes();
        for i in 0..13 {
            d[5 + i] = match src.get(i) {
                Some(&c) => c,
                None if i == src.len() => 0x0A, // terminator
                None => 0x20,                   // space padding
            };
        }
        self
    }

    pub fn extensions(mut self, n: u8) -> Self {
        self.bytes[0x7E] = n;
        self
    }

    /// Finish with a correct checksum.
    pub fn build(mut self) -> Vec<u8> {
        let sum = self.bytes[..127]
            .iter()
            .fold(0u8, |a, x| a.wrapping_add(*x));
        self.bytes[0x7F] = (0u8).wrapping_sub(sum);
        self.bytes.to_vec()
    }

    /// Finish with a deliberately wrong checksum, as shipping monitors do.
    pub fn build_with_bad_checksum(mut self) -> Vec<u8> {
        let sum = self.bytes[..127]
            .iter()
            .fold(0u8, |a, x| a.wrapping_add(*x));
        self.bytes[0x7F] = (0u8).wrapping_sub(sum).wrapping_add(1);
        self.bytes.to_vec()
    }
}
