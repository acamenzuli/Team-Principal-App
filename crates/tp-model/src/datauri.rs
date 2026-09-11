//! Decoding a `data:` URI, for images the user hands us.
//!
//! The frontend reads a chosen file with `FileReader` and sends it as a data
//! URI, which is the same path the team logo already takes. Decoding it here
//! rather than trusting it means the extension on disk describes the bytes
//! actually written: a file named `.png` holding a JPEG is the kind of thing
//! that works in a browser and then fails in something stricter later.

/// The image types accepted, and the extension each is written with.
const IMAGE_TYPES: [(&str, &str); 3] = [
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/webp", "webp"),
];

/// An image decoded from a `data:` URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedImage {
    /// The extension to write it with, chosen from the declared type rather
    /// than from whatever the original file was called.
    pub extension: &'static str,
    pub bytes: Vec<u8>,
}

/// Decode `data:image/png;base64,...`, or refuse.
///
/// Refuses anything that is not one of the three image types the app displays,
/// and anything that is not base64 — a `data:` URI can be percent-encoded
/// instead, and quietly writing those bytes would produce a file that is not
/// the image it claims to be.
pub fn decode_image_data_uri(uri: &str) -> Result<DecodedImage, &'static str> {
    let rest = uri.strip_prefix("data:").ok_or("not a data URI")?;
    let (meta, payload) = rest.split_once(',').ok_or("a data URI needs a comma")?;

    let (mime, params) = meta.split_once(';').unwrap_or((meta, ""));
    if !params.split(';').any(|p| p.trim() == "base64") {
        return Err("only base64 data URIs are accepted");
    }

    let extension = IMAGE_TYPES
        .iter()
        .find(|(m, _)| m.eq_ignore_ascii_case(mime.trim()))
        .map(|(_, ext)| *ext)
        .ok_or("that is not a PNG, JPEG or WebP")?;

    Ok(DecodedImage {
        extension,
        bytes: base64_decode(payload.trim())?,
    })
}

/// Standard base64, decoding. The encoder lives in the app crate because that
/// is where images are read; this lives here because it is pure and therefore
/// testable without Windows.
fn base64_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    fn sextet(b: u8) -> Option<u32> {
        match b {
            b'A'..=b'Z' => Some((b - b'A') as u32),
            b'a'..=b'z' => Some((b - b'a') as u32 + 26),
            b'0'..=b'9' => Some((b - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    // Whitespace is legal in a data URI payload and browsers emit none, but a
    // hand-edited profile might carry some.
    let clean: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();

    let body = clean
        .strip_suffix(b"==")
        .unwrap_or_else(|| clean.strip_suffix(b"=").unwrap_or(&clean));
    let padding = clean.len() - body.len();
    if padding > 2 || clean.len() % 4 != 0 {
        return Err("that base64 is truncated");
    }

    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in body.chunks(4) {
        let mut n = 0u32;
        for (i, &b) in chunk.iter().enumerate() {
            n |= sextet(b).ok_or("that is not base64")? << (18 - 6 * i);
        }
        // A 4-character group is 3 bytes; a trailing 3 is 2 bytes, a trailing
        // 2 is 1. Anything else is not a base64 group at all.
        match chunk.len() {
            4 => out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]),
            3 => out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8]),
            2 => out.push((n >> 16) as u8),
            _ => return Err("that base64 is truncated"),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_what_the_browser_produces() {
        // "PNG?" — not a real PNG, but the encoding is the thing under test.
        let decoded = decode_image_data_uri("data:image/png;base64,UE5HPw==").unwrap();
        assert_eq!(decoded.extension, "png");
        assert_eq!(decoded.bytes, b"PNG?");
    }

    #[test]
    fn every_padding_length_round_trips() {
        for (encoded, expected) in [
            ("YQ==", &b"a"[..]),
            ("YWI=", &b"ab"[..]),
            ("YWJj", &b"abc"[..]),
            ("YWJjZA==", &b"abcd"[..]),
        ] {
            let uri = format!("data:image/png;base64,{encoded}");
            assert_eq!(decode_image_data_uri(&uri).unwrap().bytes, expected);
        }
    }

    #[test]
    fn the_extension_comes_from_the_declared_type() {
        let jpeg = decode_image_data_uri("data:image/jpeg;base64,YWJj").unwrap();
        assert_eq!(jpeg.extension, "jpg", "not whatever the file was called");
    }

    #[test]
    fn a_type_the_app_cannot_display_is_refused() {
        // An SVG would render, and would also execute script in a webview.
        assert!(decode_image_data_uri("data:image/svg+xml;base64,YWJj").is_err());
        assert!(decode_image_data_uri("data:text/html;base64,YWJj").is_err());
    }

    #[test]
    fn a_percent_encoded_data_uri_is_refused_rather_than_written_as_bytes() {
        assert!(decode_image_data_uri("data:image/png,%89PNG").is_err());
    }

    #[test]
    fn rubbish_is_an_error_and_not_a_panic() {
        for bad in [
            "",
            "data:",
            "data:image/png;base64",
            "data:image/png;base64,YWJ",
            "data:image/png;base64,!!!!",
            "https://example.com/cover.png",
        ] {
            assert!(
                decode_image_data_uri(bad).is_err(),
                "{bad:?} should be refused"
            );
        }
    }
}
