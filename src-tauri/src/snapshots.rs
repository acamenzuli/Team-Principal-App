//! Topology snapshot storage.
//!
//! `%APPDATA%\Team Principal\snapshots\<uuid>.json`, written atomically like
//! everything else the app owns.
//!
//! Snapshots are kept rather than held only in memory for one reason: a crash
//! or a power cut in the middle of a display change leaves the desktop in the
//! changed state with nothing running to undo it. The file on disk is what lets
//! the next start offer to put it back.

use std::path::PathBuf;

use tp_model::TopologySnapshot;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

fn dir() -> PathBuf {
    crate::logging::app_data_dir().join("snapshots")
}

fn path_for(id: Uuid) -> PathBuf {
    dir().join(format!("{id}.json"))
}

pub fn list() -> Vec<TopologySnapshot> {
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return Vec::new();
    };
    let mut snapshots: Vec<TopologySnapshot> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
        .filter_map(|e| {
            let raw = std::fs::read_to_string(e.path()).ok()?;
            match serde_json::from_str::<TopologySnapshot>(&raw) {
                Ok(s) => Some(s),
                Err(err) => {
                    tracing::warn!(path = %e.path().display(), %err, "skipping unreadable snapshot");
                    None
                }
            }
        })
        .collect();
    // Newest first: the one worth restoring is almost always the last one taken.
    snapshots.sort_by(|a, b| b.captured_at.cmp(&a.captured_at));
    snapshots
}

pub fn load(id: Uuid) -> AppResult<TopologySnapshot> {
    let path = path_for(id);
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| AppError::Config(format!("could not read {}: {e}", path.display())))?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn save(snapshot: &TopologySnapshot) -> AppResult<()> {
    std::fs::create_dir_all(dir())?;
    let path = path_for(snapshot.id);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(snapshot)?)?;
    std::fs::rename(&tmp, &path)?;
    tracing::info!(id = %snapshot.id, "topology snapshot saved");
    Ok(())
}

pub fn delete(id: Uuid) -> AppResult<()> {
    let path = path_for(id);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// Keep the most recent `keep` snapshots and drop the rest.
///
/// Called after each capture. Without it every display change adds a file
/// forever, and a list of two hundred timestamps is a list nobody reads.
pub fn prune(keep: usize) {
    for snapshot in list().into_iter().skip(keep) {
        if let Err(e) = delete(snapshot.id) {
            tracing::warn!(id = %snapshot.id, error = %e, "could not prune a snapshot");
        }
    }
}
