//! Rig storage, and building a starter rig from detected hardware.
//!
//! `%APPDATA%\Team Principal\rigs\<uuid>.json`. One rig is active at a time,
//! recorded in preferences; the directory holds the rest so an alternative
//! setup can be kept without losing the current one.

pub mod detect;

use std::path::PathBuf;

use tp_model::RigModel;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

fn dir() -> PathBuf {
    crate::logging::app_data_dir().join("rigs")
}

fn path_for(id: Uuid) -> PathBuf {
    dir().join(format!("{id}.json"))
}

pub fn list() -> Vec<RigModel> {
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return Vec::new();
    };
    let mut rigs: Vec<RigModel> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
        .filter_map(|e| match std::fs::read_to_string(e.path()) {
            Ok(raw) => match serde_json::from_str::<RigModel>(&raw) {
                Ok(rig) => Some(rig),
                Err(err) => {
                    // One unreadable rig must not hide the others.
                    tracing::warn!(path = %e.path().display(), %err, "skipping unreadable rig");
                    None
                }
            },
            Err(err) => {
                tracing::warn!(path = %e.path().display(), %err, "could not read rig");
                None
            }
        })
        .collect();
    rigs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    rigs
}

pub fn load(id: Uuid) -> AppResult<RigModel> {
    let path = path_for(id);
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| AppError::Config(format!("could not read {}: {e}", path.display())))?;
    Ok(serde_json::from_str(&raw)?)
}

/// Save a rig, bumping its revision.
///
/// The revision is what drives recompute-and-propagate: every profile records
/// the revision it was computed against, so a rig change is detectable without
/// comparing every field.
pub fn save(rig: &RigModel) -> AppResult<RigModel> {
    let mut rig = rig.clone();
    rig.schema_version = tp_model::RIG_SCHEMA_VERSION;
    rig.revision += 1;
    rig.updated_at = crate::now_iso8601();

    std::fs::create_dir_all(dir())?;
    let path = path_for(rig.id);
    let json = serde_json::to_string_pretty(&rig)?;

    // Write-then-rename in the same directory, so a crash mid-write cannot
    // leave a half-written rig that the app then refuses to load.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;

    tracing::info!(id = %rig.id, revision = rig.revision, "rig saved");
    Ok(rig)
}

pub fn delete(id: Uuid) -> AppResult<()> {
    let path = path_for(id);
    if path.exists() {
        std::fs::remove_file(&path)?;
        tracing::info!(%id, "rig deleted");
    }
    Ok(())
}
