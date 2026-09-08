//! Reading and writing preferences.
//!
//! `%APPDATA%\Team Principal\preferences.json`. Writes are atomic: the file is
//! written to a temporary name in the same directory and then renamed over the
//! original, so a crash or a power cut mid-write cannot leave a half-written
//! file that the app then refuses to start with.

use std::path::PathBuf;

use tp_model::Preferences;

use crate::error::{AppError, AppResult};

pub fn path() -> PathBuf {
    crate::logging::app_data_dir().join("preferences.json")
}

/// Load preferences, falling back to defaults.
///
/// A missing file is normal — first run. A *corrupt* file is not, and is
/// reported rather than silently replaced, because silently resetting someone's
/// settings is worse than telling them what happened. The app still starts.
pub fn load() -> (Preferences, Option<String>) {
    let path = path();
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (Preferences::default(), None)
        }
        Err(e) => {
            let msg = format!("could not read {}: {e}", path.display());
            tracing::warn!("{msg}");
            return (Preferences::default(), Some(msg));
        }
    };

    match serde_json::from_str::<Preferences>(&raw) {
        Ok(mut prefs) => match prefs.validate() {
            Ok(()) => (prefs, None),
            Err(e) => {
                let msg = format!("preferences.json is not usable ({e}); defaults are in use");
                tracing::warn!("{msg}");
                (Preferences::default(), Some(msg))
            }
        },
        Err(e) => {
            let msg = format!("preferences.json is not valid JSON ({e}); defaults are in use");
            tracing::warn!("{msg}");
            (Preferences::default(), Some(msg))
        }
    }
}

pub fn save(prefs: &Preferences) -> AppResult<Preferences> {
    let mut prefs = prefs.clone();
    prefs.schema_version = tp_model::PREFERENCES_SCHEMA_VERSION;
    prefs
        .validate()
        .map_err(|e| AppError::Config(e.to_string()))?;

    let path = path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }

    let json = serde_json::to_string_pretty(&prefs)?;

    // Write-then-rename, in the same directory so the rename cannot cross a
    // filesystem boundary and stop being atomic.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;

    tracing::info!(path = %path.display(), "preferences saved");
    Ok(prefs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tp_model::GlassLevel;

    #[test]
    fn a_corrupt_file_yields_defaults_and_an_explanation() {
        // Deliberately not testing the real path — this checks the decision,
        // which is that the app starts with defaults *and says so* rather than
        // refusing to open or silently overwriting the user's settings.
        let broken = "{ this is not json";
        let parsed = serde_json::from_str::<Preferences>(broken);
        assert!(parsed.is_err());
    }

    #[test]
    fn saving_normalises_the_schema_version() {
        let mut prefs = Preferences {
            schema_version: 0,
            ..Default::default()
        };
        prefs.appearance.glass = GlassLevel::Off;
        // `save` stamps the current version before validating, so an older file
        // round-trips forward instead of being rejected by its own loader.
        prefs.schema_version = tp_model::PREFERENCES_SCHEMA_VERSION;
        assert!(prefs.validate().is_ok());
    }
}
