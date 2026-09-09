//! User preferences.
//!
//! Stored at `%APPDATA%\Team Principal\preferences.json`. Small, versioned, and
//! deliberately separate from the rig model and from profiles: losing your
//! accent colour must never be able to take a rig description with it.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::LengthUnit;

pub const PREFERENCES_SCHEMA_VERSION: u32 = 1;

/// The default accent: pit-lane amber.
pub const DEFAULT_ACCENT: &str = "#FFB02E";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub schema_version: u32,
    /// Display unit for lengths. A UI preference only — the model is always
    /// millimetres.
    pub units: LengthUnit,
    pub appearance: Appearance,
    pub team: TeamBranding,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            schema_version: PREFERENCES_SCHEMA_VERSION,
            units: LengthUnit::Mm,
            appearance: Appearance::default(),
            team: TeamBranding::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Appearance {
    /// `#RRGGBB`. Every interactive and highlight colour in the UI derives from
    /// this one value.
    pub accent: String,
    /// How much translucency and blur the chrome uses.
    pub glass: GlassLevel,
    /// Suppresses the few transitions the app uses. The OS
    /// `prefers-reduced-motion` setting also does this; whichever asks, wins.
    pub reduce_motion: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            accent: DEFAULT_ACCENT.to_string(),
            glass: GlassLevel::Subtle,
            reduce_motion: false,
        }
    }
}

/// Glass costs contrast, and this app is read from over 700 mm away in a dim
/// room. The level is a real setting rather than a fixed style so that the look
/// can be dialled back without the app becoming a different product.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum GlassLevel {
    /// Flat opaque surfaces. Highest contrast, and the fastest to draw.
    Off,
    /// Translucent chrome over a blurred backdrop; data surfaces stay solid.
    #[default]
    Subtle,
    /// Translucency on data surfaces too.
    Full,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TeamBranding {
    /// Shown beside the logo in the title bar. Empty means no name.
    pub name: Option<String>,
    pub logo: Option<TeamLogo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TeamLogo {
    /// The image inlined as a `data:` URI.
    ///
    /// Inlined rather than referenced by path so that preferences.json is
    /// self-contained: it survives being copied to another machine, and the app
    /// never has to reason about a logo file that has been moved or deleted.
    pub data_uri: String,
    /// The original filename, so the settings screen can say what is loaded.
    pub file_name: String,
}

/// The largest logo accepted, before base64 expansion.
pub const MAX_LOGO_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PreferencesError {
    #[error("{0:?} is not a colour in #RRGGBB form")]
    BadAccent(String),
    #[error("that image is {actual} KB; the limit is {} KB", MAX_LOGO_BYTES / 1024)]
    LogoTooLarge { actual: usize },
    #[error("only PNG, JPEG, SVG and WebP images are accepted")]
    LogoNotAnImage,
    #[error("preferences are version {found}, but this build understands {supported}")]
    UnsupportedVersion { found: u32, supported: u32 },
}

impl Preferences {
    /// Reject anything that would render as a broken UI, and normalise the rest.
    ///
    /// Called on both save and load: a preferences file edited by hand, or
    /// written by a newer build, must not be able to leave the app unusable.
    pub fn validate(&mut self) -> Result<(), PreferencesError> {
        if self.schema_version > PREFERENCES_SCHEMA_VERSION {
            return Err(PreferencesError::UnsupportedVersion {
                found: self.schema_version,
                supported: PREFERENCES_SCHEMA_VERSION,
            });
        }
        self.appearance.accent = normalise_hex(&self.appearance.accent)
            .ok_or_else(|| PreferencesError::BadAccent(self.appearance.accent.clone()))?;
        if let Some(logo) = &self.team.logo {
            validate_logo(&logo.data_uri)?;
        }
        if let Some(name) = &self.team.name {
            let trimmed = name.trim();
            self.team.name = (!trimmed.is_empty()).then(|| trimmed.chars().take(60).collect());
        }
        Ok(())
    }
}

/// Accept `#rgb`, `#rrggbb`, and the same without the hash; emit `#RRGGBB`.
pub fn normalise_hex(input: &str) -> Option<String> {
    let s = input.trim().trim_start_matches('#');
    let expanded = match s.len() {
        3 => s.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => s.to_string(),
        _ => return None,
    };
    expanded
        .chars()
        .all(|c| c.is_ascii_hexdigit())
        .then(|| format!("#{}", expanded.to_ascii_uppercase()))
}

fn validate_logo(data_uri: &str) -> Result<(), PreferencesError> {
    let rest = data_uri
        .strip_prefix("data:")
        .ok_or(PreferencesError::LogoNotAnImage)?;
    let (mime, payload) = rest
        .split_once(',')
        .ok_or(PreferencesError::LogoNotAnImage)?;

    let accepted = ["image/png", "image/jpeg", "image/svg+xml", "image/webp"];
    if !accepted.iter().any(|m| mime.starts_with(m)) {
        return Err(PreferencesError::LogoNotAnImage);
    }

    // base64 is 4 characters per 3 bytes; close enough to police a size cap
    // without pulling in a decoder just to measure.
    let approx_bytes = payload.len() / 4 * 3;
    if approx_bytes > MAX_LOGO_BYTES {
        return Err(PreferencesError::LogoTooLarge {
            actual: approx_bytes / 1024,
        });
    }
    Ok(())
}

/// Black or white, whichever is actually more legible on the given accent.
///
/// Computed rather than chosen, so a custom accent cannot produce a button
/// whose label is unreadable.
///
/// Note there is no luminance threshold here. Thresholds are the usual way to
/// write this and they are wrong: the first version of this function used 0.45,
/// which picked white for `#FF6B35` at 2.8:1 when black would have given 7.4:1.
/// The crossover is at luminance 0.179, not somewhere near the middle — but
/// rather than encode that constant, just compute both contrasts and take the
/// better one. Same answer, nothing to get wrong.
pub fn accent_foreground(accent: &str) -> &'static str {
    const DARK: &str = "#0B0D0F";
    const LIGHT: &str = "#FFFFFF";
    match relative_luminance(accent) {
        Some(l) if contrast_with(l, LIGHT) > contrast_with(l, DARK) => LIGHT,
        Some(_) => DARK,
        None => DARK,
    }
}

/// WCAG contrast ratio between a known luminance and a named colour.
fn contrast_with(luminance: f64, other: &str) -> f64 {
    let other = relative_luminance(other).unwrap_or(0.0);
    contrast_ratio(luminance, other)
}

/// WCAG contrast ratio between two relative luminances. 1.0 is identical,
/// 21.0 is black on white.
pub fn contrast_ratio(a: f64, b: f64) -> f64 {
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

pub fn relative_luminance(hex: &str) -> Option<f64> {
    let s = normalise_hex(hex)?;
    let b = s.as_bytes();
    let channel = |i: usize| -> f64 {
        let v = u8::from_str_radix(std::str::from_utf8(&b[i..i + 2]).ok().unwrap_or("0"), 16)
            .unwrap_or(0) as f64
            / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    Some(0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5))
}

/// Accents offered in the settings screen. A custom hex is always allowed.
pub fn accent_presets() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Pit lane", "#FFB02E"),
        ("Safety car", "#FF6B35"),
        ("Green flag", "#3BD16F"),
        ("Telemetry", "#5AA9FF"),
        ("Scuderia", "#FF2D2D"),
        ("Papaya", "#FF8000"),
        ("Petronas", "#00D2BE"),
        ("Ice", "#C8D3DC"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        let mut p = Preferences::default();
        assert!(p.validate().is_ok());
        assert_eq!(p.appearance.accent, DEFAULT_ACCENT);
    }

    #[test]
    fn hex_forms_are_normalised() {
        for input in ["#ffb02e", "ffb02e", "  #FFB02E  "] {
            assert_eq!(normalise_hex(input).as_deref(), Some("#FFB02E"), "{input}");
        }
        // Shorthand expands.
        assert_eq!(normalise_hex("#f0a").as_deref(), Some("#FF00AA"));
        // And nonsense is refused rather than rendered as a broken colour.
        for bad in ["", "#12345", "rgb(1,2,3)", "#gggggg", "chartreuse"] {
            assert_eq!(normalise_hex(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_bad_accent_is_rejected_not_rendered() {
        let mut p = Preferences {
            appearance: Appearance {
                accent: "not a colour".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(matches!(p.validate(), Err(PreferencesError::BadAccent(_))));
    }

    #[test]
    fn accent_foreground_stays_legible() {
        // A bright accent needs dark text; a dark one needs light text.
        assert_eq!(accent_foreground("#FFB02E"), "#0B0D0F");
        assert_eq!(accent_foreground("#FFFFFF"), "#0B0D0F");
        assert_eq!(accent_foreground("#00206B"), "#FFFFFF");
        assert_eq!(accent_foreground("#000000"), "#FFFFFF");
        // Every preset must clear WCAG AA for large text against its chosen
        // foreground. This is what caught the threshold bug: #FF6B35 was being
        // given white at 2.8:1 when black gives 7.4:1.
        for (name, hex) in accent_presets() {
            let fg = accent_foreground(hex);
            let ratio = contrast_ratio(
                relative_luminance(hex).unwrap(),
                relative_luminance(fg).unwrap(),
            );
            assert!(ratio >= 4.5, "{name} ({hex}) on {fg} is only {ratio:.1}:1");
        }
    }

    #[test]
    fn foreground_is_the_better_of_the_two_not_a_threshold() {
        // Mid-luminance accents are where a naive threshold goes wrong.
        for hex in ["#FF6B35", "#FF2D2D", "#5AA9FF", "#808080", "#7F5AF0"] {
            let l = relative_luminance(hex).unwrap();
            let chosen = accent_foreground(hex);
            let other = if chosen == "#FFFFFF" {
                "#0B0D0F"
            } else {
                "#FFFFFF"
            };
            let chosen_ratio = contrast_ratio(l, relative_luminance(chosen).unwrap());
            let other_ratio = contrast_ratio(l, relative_luminance(other).unwrap());
            assert!(
                chosen_ratio >= other_ratio,
                "{hex}: picked {chosen} at {chosen_ratio:.1}:1 over {other} at {other_ratio:.1}:1"
            );
        }
    }

    #[test]
    fn logos_are_policed() {
        let png = format!("data:image/png;base64,{}", "A".repeat(100));
        let mut p = Preferences::default();
        p.team.logo = Some(TeamLogo {
            data_uri: png,
            file_name: "logo.png".into(),
        });
        assert!(p.validate().is_ok());

        // A script masquerading as a logo must not be stored, let alone rendered.
        p.team.logo = Some(TeamLogo {
            data_uri: "data:text/html,<script>alert(1)</script>".into(),
            file_name: "logo.png".into(),
        });
        assert_eq!(p.validate(), Err(PreferencesError::LogoNotAnImage));

        // And neither must a 4 MB one.
        p.team.logo = Some(TeamLogo {
            data_uri: format!("data:image/png;base64,{}", "A".repeat(6_000_000)),
            file_name: "huge.png".into(),
        });
        assert!(matches!(
            p.validate(),
            Err(PreferencesError::LogoTooLarge { .. })
        ));
    }

    #[test]
    fn team_names_are_trimmed_and_capped() {
        let mut p = Preferences::default();
        p.team.name = Some("   ".into());
        p.validate().unwrap();
        assert_eq!(p.team.name, None, "whitespace is not a team name");

        p.team.name = Some("x".repeat(200));
        p.validate().unwrap();
        assert_eq!(p.team.name.as_ref().unwrap().len(), 60);
    }

    #[test]
    fn a_newer_file_is_refused_rather_than_half_read() {
        let mut p = Preferences {
            schema_version: 99,
            ..Default::default()
        };
        assert!(matches!(
            p.validate(),
            Err(PreferencesError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn round_trips_through_json() {
        let mut original = Preferences::default();
        original.team.name = Some("Team Principal".into());
        original.appearance.glass = GlassLevel::Full;
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(
            serde_json::from_str::<Preferences>(&json).unwrap(),
            original
        );
    }
}

// ------------------------------------------------------------------ licensing

/// What the app is entitled to do.
///
/// No vendor is named anywhere in this project: a merchant-of-record handles
/// the money and a licence service handles the keys, and neither is chosen. The
/// types here define the shape of the question so there is exactly one place to
/// answer it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Everything that only reads: enumerate the rig, solve the geometry, show
    /// the numbers. Deliberately generous — someone evaluating this needs to
    /// see their own rig measured before they will believe it.
    #[default]
    Unlicensed,
    /// The full product.
    Licensed,
}

/// What the UI is told about licensing, and the whole of it.
///
/// No key, no token, no endpoint, no machine id. A licence check whose inputs
/// the UI can see is one anyone can read out of the installer, because `src/`
/// ships as readable JavaScript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct LicenceState {
    pub tier: Tier,
    /// The last four characters of the key. Enough for a person to tell two
    /// licences apart, useless to anyone who copies it.
    pub reference: Option<String>,
    /// When the cached entitlement stops being trusted. `None` for a perpetual
    /// licence already verified.
    pub valid_until: Option<String>,
    /// Running on a cached answer because the service was unreachable.
    /// Surfaced rather than hidden: someone whose licence check is silently
    /// failing should find out before the day it stops working.
    pub offline: bool,
    /// Written for a person, not a log.
    pub message: Option<String>,
}

impl Default for LicenceState {
    fn default() -> Self {
        Self {
            tier: Tier::Unlicensed,
            reference: None,
            valid_until: None,
            offline: false,
            message: None,
        }
    }
}
