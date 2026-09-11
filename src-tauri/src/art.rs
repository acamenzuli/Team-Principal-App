//! Cover art, found on this machine.
//!
//! Steam already downloaded the artwork for every game in your library and
//! keeps it in its own cache. So the art comes off the local disk: no network
//! call, no third-party service, nothing that stops working offline or that
//! quietly sends a list of your games somewhere.
//!
//! ## Two cache layouts
//!
//! Steam changed this. Older clients wrote a flat directory keyed by a filename
//! prefix; newer ones write a directory per app id. Both are still on real
//! machines — a client that has been upgraded rather than reinstalled has the
//! old files sitting there — so both are checked, newest layout first.
//!
//! ```text
//! appcache/librarycache/<appid>/library_600x900.jpg     newer
//! appcache/librarycache/<appid>_library_600x900.jpg     older
//! ```
//!
//! ## Epic
//!
//! Epic does not cache its store art locally in any documented, stable place.
//! Rather than guess at one — or fetch it over the network, which this app does
//! not do — an Epic game gets no art and the card falls back to a monogram.
//! That is an honest blank rather than a wrong picture.

use std::path::{Path, PathBuf};

use crate::launcher::GameSource;

/// The portrait art files Steam keeps, best first.
///
/// Portrait before landscape because the card is portrait; `header.jpg` is the
/// last resort because every game has one and it is the wrong shape.
const NAMES: [&str; 4] = [
    "library_600x900.jpg",
    "library_600x900_2x.jpg",
    "library_capsule.jpg",
    "header.jpg",
];

/// Find cover art for a game, or nothing.
pub fn find(source: &GameSource) -> Option<PathBuf> {
    let GameSource::Steam { app_id } = source else {
        return None;
    };
    let cache = crate::launcher::discovery::steam_root()?
        .join("appcache")
        .join("librarycache");

    for name in NAMES {
        // Newer layout: a directory per app id.
        let nested = cache.join(app_id).join(name);
        if nested.is_file() {
            return Some(nested);
        }
        // Older layout: flat, prefixed with the app id.
        let flat = cache.join(format!("{app_id}_{name}"));
        if flat.is_file() {
            return Some(flat);
        }
    }
    None
}

/// Read an image as a data URI for the WebView.
///
/// Inlined rather than served through Tauri's asset protocol. The protocol
/// would need a filesystem scope wide enough to cover every Steam library on
/// the machine, and these images are around a hundred kilobytes — not worth
/// opening that door for.
pub fn as_data_uri(path: &Path) -> Option<String> {
    // A cover that is not a cover. Steam's cache holds only small images, so
    // anything this large is a sign the path is wrong rather than a big JPEG.
    const LIMIT: u64 = 8 * 1024 * 1024;

    let size = std::fs::metadata(path).ok()?.len();
    if size > LIMIT {
        tracing::warn!(path = %path.display(), size, "ignoring an implausibly large cover image");
        return None;
    }

    let bytes = std::fs::read(path).ok()?;
    let mime = match path
        .extension()
        .and_then(|e| e.to_str())?
        .to_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        _ => return None,
    };
    Some(format!("data:{mime};base64,{}", base64(&bytes)))
}

/// Standard base64. Written out rather than pulled in as a dependency: it is
/// twenty lines and this is the only place in the app that needs it.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18 & 63) as usize] as char);
        out.push(ALPHABET[(n >> 12 & 63) as usize] as char);
        // Pad to a multiple of four, which is what makes it decodable.
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Where art the user chose themselves is kept.
///
/// Inside the app's own data folder rather than left where they found it: a
/// picture on a desktop gets tidied away, and a card that loses its cover
/// because a file moved is a bug report. It also means the art travels with
/// the profiles in a diagnostics bundle and survives an uninstall like
/// everything else there.
fn chosen_dir() -> PathBuf {
    crate::logging::app_data_dir().join("art")
}

/// Save an image the user picked for a profile, and return where it went.
///
/// One file per profile: choosing a new cover replaces the old one rather than
/// leaving a folder that grows every time somebody changes their mind. The
/// extension comes from the declared image type, so the name on disk describes
/// the bytes in it.
pub fn save_chosen(profile_id: &str, data_uri: &str) -> crate::error::AppResult<PathBuf> {
    use crate::error::AppError;

    // 8 MB, the same ceiling `as_data_uri` reads back. Anything larger is not
    // cover art, and a 40 MB photograph inlined into every card would make the
    // library screen crawl.
    const LIMIT: usize = 8 * 1024 * 1024;

    let image = tp_model::decode_image_data_uri(data_uri)
        .map_err(|e| AppError::Config(format!("that image could not be read: {e}")))?;

    if image.bytes.len() > LIMIT {
        return Err(AppError::Config(format!(
            "that image is {} MB. The limit is 8 MB — cover art is a picture, not a photograph.",
            image.bytes.len() / 1_000_000
        )));
    }

    let dir = chosen_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Config(format!("could not make {}: {e}", dir.display())))?;

    // Any previous cover for this profile, whatever type it was: choosing a
    // new one replaces the old rather than leaving a folder that grows every
    // time somebody changes their mind.
    clear_chosen(profile_id);

    let path = dir.join(format!("{profile_id}.{}", image.extension));
    std::fs::write(&path, &image.bytes)
        .map_err(|e| AppError::Config(format!("could not write {}: {e}", path.display())))?;

    tracing::info!(path = %path.display(), bytes = image.bytes.len(), "saved chosen cover art");
    Ok(path)
}

/// Forget art the user chose, so the detected art (if any) comes back.
pub fn clear_chosen(profile_id: &str) {
    let dir = chosen_dir();
    for ext in ["png", "jpg", "webp"] {
        let _ = std::fs::remove_file(dir.join(format!("{profile_id}.{ext}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_test_vectors() {
        // RFC 4648 section 10. Padding is the part that is easy to get wrong.
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_handles_bytes_above_127() {
        // A JPEG is mostly high bytes; a sign error here would corrupt every
        // image while passing the ASCII vectors above.
        assert_eq!(base64(&[0xFF, 0xD8, 0xFF]), "/9j/");
        assert_eq!(base64(&[0x00, 0x00, 0x00]), "AAAA");
    }

    #[test]
    fn epic_gets_no_art_rather_than_the_wrong_art() {
        assert_eq!(
            find(&GameSource::Epic {
                app_name: "Fortnite".into()
            }),
            None
        );
    }
}
