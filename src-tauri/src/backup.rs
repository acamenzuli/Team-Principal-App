//! The backup store.
//!
//! Every file Team Principal writes is copied here first, with a timestamp, and
//! "restore everything to how it was" is one call away. This is not a nicety:
//! the app edits config files that people have spent hours tuning, and the only
//! acceptable worst case is that a bad write costs a click to undo.
//!
//! ## Layout
//!
//! ```text
//! %APPDATA%\Team Principal\backups\
//!     2026-09-08T17-42-11Z\
//!         manifest.json          what was backed up, and where it came from
//!         0-video.ini            the original bytes, numbered by order
//!         1-renderer.ini
//! ```
//!
//! One directory per *operation*, not per file, because "put it back how it was
//! before I applied the iRacing adapter" is the question people actually ask.
//! Restoring half of a two-file change would leave a state that never existed.
//!
//! ## Why the original bytes rather than a diff
//!
//! A diff is smaller and would be wrong. These files are rewritten by the games
//! themselves between sessions, so a patch computed against yesterday's file
//! may not apply to today's. The whole original always restores to exactly what
//! was there.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

fn dir() -> PathBuf {
    crate::logging::app_data_dir().join("backups")
}

/// What one backed-up operation contained.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Directory-safe timestamp, and the directory's own name.
    pub taken_at: String,
    /// What caused the write: an adapter id, or a description.
    pub reason: String,
    pub files: Vec<BackedUpFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackedUpFile {
    /// Where it came from, and where a restore puts it back.
    pub original_path: String,
    /// The copy's file name inside the backup directory.
    pub stored_as: String,
    /// False when the file did not exist before the write.
    ///
    /// Restoring then means *deleting* the file rather than writing an empty
    /// one — an empty config is not the same as no config, and some games
    /// treat the two very differently.
    pub existed: bool,
}

/// Copy every file about to be written, before writing any of them.
///
/// Returns the directory name, which is the handle a restore takes.
pub fn capture(reason: &str, paths: &[String]) -> AppResult<String> {
    let taken_at = timestamp();
    let target = dir().join(&taken_at);
    std::fs::create_dir_all(&target)?;

    let mut files = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let source = Path::new(path);
        let name = source
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");
        let stored_as = format!("{index}-{name}");

        let existed = source.is_file();
        if existed {
            std::fs::copy(source, target.join(&stored_as))
                .map_err(|e| AppError::Io(format!("could not back up {path}: {e}")))?;
        }
        files.push(BackedUpFile {
            original_path: path.clone(),
            stored_as,
            existed,
        });
    }

    let manifest = Manifest {
        taken_at: taken_at.clone(),
        reason: reason.to_string(),
        files,
    };
    std::fs::write(
        target.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;

    tracing::info!(backup = %taken_at, reason, count = paths.len(), "backed up before writing");
    Ok(taken_at)
}

pub fn list() -> Vec<Manifest> {
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return Vec::new();
    };
    let mut manifests: Vec<Manifest> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let raw = std::fs::read_to_string(e.path().join("manifest.json")).ok()?;
            serde_json::from_str::<Manifest>(&raw).ok()
        })
        .collect();
    // Newest first: the one worth undoing is almost always the last one.
    manifests.sort_by(|a, b| b.taken_at.cmp(&a.taken_at));
    manifests
}

/// Put one operation's files back exactly as they were.
///
/// Restores every file or none: a partial restore leaves a state that never
/// existed on this machine, which is worse than the state it was trying to fix.
/// So the copies are all read first, and only then written.
pub fn restore(taken_at: &str) -> AppResult<Vec<String>> {
    let source = dir().join(taken_at);
    let raw = std::fs::read_to_string(source.join("manifest.json"))
        .map_err(|e| AppError::Config(format!("no backup called {taken_at}: {e}")))?;
    let manifest: Manifest = serde_json::from_str(&raw)?;

    // Read everything before writing anything.
    let mut staged: Vec<(PathBuf, Option<Vec<u8>>)> = Vec::new();
    for file in &manifest.files {
        let destination = PathBuf::from(&file.original_path);
        if !file.existed {
            staged.push((destination, None));
            continue;
        }
        let bytes = std::fs::read(source.join(&file.stored_as)).map_err(|e| {
            AppError::Io(format!(
                "the backup of {} is unreadable, so nothing was restored: {e}",
                file.original_path
            ))
        })?;
        staged.push((destination, Some(bytes)));
    }

    let mut restored = Vec::new();
    for (destination, bytes) in staged {
        match bytes {
            // Write-then-rename, so a crash mid-restore cannot truncate the
            // very file it is rescuing.
            Some(bytes) => {
                let tmp = destination.with_extension("tp-restore");
                std::fs::write(&tmp, &bytes)?;
                std::fs::rename(&tmp, &destination)?;
            }
            // It did not exist before, so putting it back means removing it.
            None => {
                if destination.exists() {
                    std::fs::remove_file(&destination)?;
                }
            }
        }
        restored.push(destination.display().to_string());
    }

    tracing::info!(backup = %taken_at, count = restored.len(), "restored");
    Ok(restored)
}

/// A timestamp that is also a valid directory name on Windows.
///
/// Colons are legal in ISO-8601 and illegal in a Windows path, which is the
/// kind of detail that only shows up on the user's machine.
fn timestamp() -> String {
    crate::now_iso8601().replace(':', "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timestamp_is_a_legal_windows_directory_name() {
        // Colons are legal in ISO-8601 and illegal in a Windows path.
        let stamp = timestamp();
        for illegal in [':', '<', '>', '"', '/', '\\', '|', '?', '*'] {
            assert!(!stamp.contains(illegal), "{stamp} contains {illegal}");
        }
    }
}
