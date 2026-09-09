//! The adapter catalog: every title, as data.
//!
//! Adapters used to be two hand-written functions. That does not scale to the
//! dozen-plus sims a real rig has installed, and hand-written means every new
//! title is a code change, a release and a reinstall.
//!
//! So a title is now an entry: where its config lives, what format it is, and
//! which of the rig's derived values go into which keys. Adding one is data.
//!
//! ## What this does not change
//!
//! The verification rule. An entry still declares its [`Confidence`], still
//! records its sources in `docs/adapters/`, and the editors underneath still
//! refuse to create a key the user's file does not already have. Making titles
//! cheap to add is not a licence to guess at them — it is what makes it
//! affordable to add them *properly*, one verified file at a time.
//!
//! An entry with no `writes` is a **known title with an unverified layout**: the
//! app can find its config and show you what is in it, but will not write to it
//! until somebody has looked. That is the honest middle state between "supported"
//! and "never heard of it", and it is where most of this list starts.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::adapter::Confidence;

/// How a game's config file is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFormat {
    Ini,
    Xml,
}

/// A number the rig model can produce, in the unit a given game wants.
///
/// This is the vocabulary a catalog entry is written in. Every sim that does
/// triple-screen properly asks for the same handful of physical facts — they
/// differ only in units, names and file format — so the entries stay short and
/// the arithmetic lives in one place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum Derived {
    /// Visible glass width of the centre screen. The chord, on a curved panel.
    ScreenWidth {
        unit: Unit,
        decimals: u8,
    },
    ScreenHeight {
        unit: Unit,
        decimals: u8,
    },
    /// Glass plus both bezels plus any mount gap — what a sim means by
    /// "monitor width" when it is compensating for the dark band between
    /// screens.
    MonitorWidth {
        unit: Unit,
        decimals: u8,
    },
    /// Eye to the centre screen.
    EyeDistance {
        unit: Unit,
        decimals: u8,
    },
    /// The physical dark band between two adjacent screens.
    BezelGap {
        unit: Unit,
        decimals: u8,
    },
    /// Side screen angle, degrees.
    SideAngle {
        decimals: u8,
    },
    /// Total pixel width across the session's screens.
    PixelWidth,
    PixelHeight,
    /// 1 or 3.
    ScreenCount,
    /// A fixed value the adapter wants regardless of the rig.
    Literal {
        value: String,
    },
    /// `(a, b, c)` — the shape rFactor 2 and Le Mans Ultimate use for a whole
    /// screen definition in one value.
    Tuple {
        parts: Vec<Derived>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Mm,
    Cm,
    Metres,
}

impl Unit {
    pub fn from_mm(self, mm: f64) -> f64 {
        match self {
            Unit::Mm => mm,
            Unit::Cm => mm / 10.0,
            Unit::Metres => mm / 1000.0,
        }
    }
}

/// One setting in one file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Write {
    /// INI section, or empty for XML.
    pub section: String,
    /// INI key, or XML element name.
    pub key: String,
    /// XML attribute, when the value is an attribute rather than element text.
    pub attribute: Option<String>,
    pub value: Derived,
    /// Why, in the user's terms. `{}` is replaced with the value.
    pub because: String,
    /// Only write this when the session spans every screen.
    #[serde(default)]
    pub triple_only: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFile {
    pub path_template: String,
    pub format: ConfigFormat,
    #[serde(default)]
    pub writes: Vec<Write>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct GameEntry {
    pub id: String,
    pub title: String,
    /// Every name an install of this game goes by, normalised. Matching is
    /// exact against this list — never a substring, because "Assetto Corsa
    /// Competizione" contains "Assetto Corsa" and they are different games with
    /// different config formats.
    pub aliases: Vec<String>,
    pub confidence: Confidence,
    pub scope: String,
    pub source: String,
    /// Things worth telling the user about this title that are not warnings
    /// about their rig — what this adapter deliberately does not touch, and
    /// where to find the rest. Shown alongside the diff.
    #[serde(default)]
    pub notes: Vec<String>,
    pub files: Vec<ConfigFile>,
}

impl GameEntry {
    /// Whether this entry can actually write anything yet.
    ///
    /// False means the app knows the title and where its settings live, and has
    /// not had a real file confirmed. It will show you what is in yours; it
    /// will not write to it.
    pub fn writes_anything(&self) -> bool {
        self.files.iter().any(|f| !f.writes.is_empty())
    }
}

/// The shipped catalog.
///
/// Embedded rather than hardcoded in Rust so that adding a title is data, and
/// so a corrected entry can be sent to a user as a file rather than a release.
/// `catalog::load` prefers a copy in the app data directory when one exists.
pub const SHIPPED: &str = include_str!("../data/adapters.json");

pub fn shipped() -> Vec<GameEntry> {
    serde_json::from_str(SHIPPED).unwrap_or_else(|e| {
        // A malformed embedded catalog is a build-time mistake, not a runtime
        // one, and failing loudly beats an app that silently supports nothing.
        panic!("the shipped adapter catalog does not parse: {e}");
    })
}

/// Lowercase, letters and digits only.
pub fn normalise(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_catalog_parses() {
        // It is embedded at build time, so a typo in it is a compile-time
        // problem that only this test will catch.
        assert!(!shipped().is_empty());
    }

    #[test]
    fn every_entry_is_internally_consistent() {
        for entry in shipped() {
            assert!(!entry.aliases.is_empty(), "{} has no aliases", entry.id);
            assert!(!entry.scope.is_empty(), "{} has no scope", entry.id);
            assert!(!entry.source.is_empty(), "{} has no source", entry.id);
            assert!(!entry.files.is_empty(), "{} names no files", entry.id);

            for alias in &entry.aliases {
                assert_eq!(
                    *alias,
                    normalise(alias),
                    "{}: aliases are stored normalised",
                    entry.id
                );
            }
            for file in &entry.files {
                assert!(
                    file.path_template.contains('{'),
                    "{}: {} has no path token",
                    entry.id,
                    file.path_template
                );
                for write in &file.writes {
                    assert!(!write.key.is_empty(), "{}: a write has no key", entry.id);
                    assert!(
                        !write.because.is_empty(),
                        "{}: {} has no reason",
                        entry.id,
                        write.key
                    );
                }
            }
        }
    }

    #[test]
    fn no_two_entries_claim_the_same_alias() {
        // An alias collision means one game silently gets another's settings.
        let mut seen: Vec<(String, String)> = Vec::new();
        for entry in shipped() {
            for alias in &entry.aliases {
                if let Some((other, _)) = seen.iter().find(|(a, _)| a == alias) {
                    panic!("{alias:?} is claimed by both {} and {}", other, entry.id);
                }
                seen.push((alias.clone(), entry.id.clone()));
            }
        }
    }

    #[test]
    fn only_verified_or_corroborated_entries_write_anything() {
        // The rule the whole protocol rests on: an entry nobody has confirmed
        // can show you your file, and must not write to it.
        for entry in shipped() {
            if entry.writes_anything() {
                assert!(
                    matches!(
                        entry.confidence,
                        Confidence::Verified | Confidence::Corroborated
                    ),
                    "{} writes without a confidence level",
                    entry.id
                );
            }
        }
    }

    #[test]
    fn units_convert_from_millimetres() {
        assert_eq!(Unit::Mm.from_mm(597.0), 597.0);
        assert_eq!(Unit::Cm.from_mm(597.0), 59.7);
        assert_eq!(Unit::Metres.from_mm(597.0), 0.597);
    }
}
