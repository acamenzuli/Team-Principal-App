//! The crash-recovery marker.
//!
//! A session changes things outside this app: game config files are rewritten,
//! the desktop may be reconfigured, utilities are started. Teardown puts all of
//! that back — but only if something is still running to do it.
//!
//! If the app is killed mid-session, or the machine loses power, nothing is.
//! So a marker is written to disk *before* anything is changed and cleared only
//! after teardown finishes. Finding one at startup means the last session did
//! not end cleanly, and the app offers to run the teardown it never got to.
//!
//! This is the difference between "teardown works" and "teardown works, and
//! also works after a crash", which was in the brief and is the harder half.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

fn path() -> PathBuf {
    crate::logging::app_data_dir().join("state.json")
}

/// What a session changed, and therefore what has to be undone.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InFlight {
    pub profile_id: Uuid,
    pub profile_name: String,
    pub started_at: String,
    /// The backup this session's config writes can be undone from. `None` until
    /// something has actually been written.
    #[serde(default)]
    pub config_backup: Option<String>,
    /// The topology to go back to, if this session changed the display.
    #[serde(default)]
    pub display_snapshot: Option<Uuid>,
}

/// Record that a session has begun, before it changes anything.
pub fn begin(profile_id: Uuid, profile_name: &str) -> AppResult<()> {
    let marker = InFlight {
        profile_id,
        profile_name: profile_name.to_string(),
        started_at: crate::now_iso8601(),
        config_backup: None,
        display_snapshot: None,
    };
    write(&marker)
}

/// Note what this session changed, so a recovery run knows what to undo.
pub fn record_backup(backup: &str) {
    let Some(mut marker) = pending() else {
        return;
    };
    marker.config_backup = Some(backup.to_string());
    if let Err(e) = write(&marker) {
        tracing::warn!(error = %e, "could not record the backup on the session marker");
    }
}

/// The session that did not finish, if there is one.
pub fn pending() -> Option<InFlight> {
    let raw = std::fs::read_to_string(path()).ok()?;
    match serde_json::from_str(&raw) {
        Ok(marker) => Some(marker),
        Err(err) => {
            // An unreadable marker is still evidence a session was running.
            // Deleting it silently would throw away the one clue that anything
            // needs undoing, so it is left alone and reported.
            tracing::warn!(%err, "the session marker is unreadable");
            None
        }
    }
}

/// Teardown finished. Nothing is outstanding.
pub fn clear() {
    let path = path();
    if path.exists() {
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!(error = %e, "could not clear the session marker");
        }
    }
}

fn write(marker: &InFlight) -> AppResult<()> {
    let path = path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Write-then-rename: a marker truncated by a crash is worse than no marker,
    // because it looks like corruption rather than like an unfinished session.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(marker)?)
        .map_err(|e| AppError::Io(format!("could not write the session marker: {e}")))?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}
