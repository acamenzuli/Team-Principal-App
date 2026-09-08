//! Resolving adapter plans against this machine, and writing them.
//!
//! The adapters themselves — which keys, which values, and why — live in
//! `tp_model::adapter`, where they are tested without a filesystem. This is the
//! part that cannot be: finding the files, backing them up, and writing them.

use std::path::PathBuf;

use tp_model::{AdapterPlan, FileDiff};

use crate::error::{AppError, AppResult};

/// Expand `{documents}` and `{localappdata}` in an adapter's path template.
///
/// **Documents is looked up, never assembled.** `%USERPROFILE%\Documents` is
/// wrong on any machine where OneDrive has redirected the folder, which is the
/// default on a lot of consumer Windows installs — and the failure is silent:
/// the app writes a perfectly good config file to a folder the game does not
/// read, and reports success.
pub fn resolve(template: &str) -> AppResult<PathBuf> {
    let documents = known_documents()?;
    let local = local_app_data()?;
    let expanded = template
        .replace("{documents}", &documents.display().to_string())
        .replace("{localappdata}", &local.display().to_string());
    Ok(PathBuf::from(expanded))
}

#[cfg(windows)]
fn known_documents() -> AppResult<PathBuf> {
    use windows::Win32::UI::Shell::{FOLDERID_Documents, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

    // SAFETY: the returned PWSTR is owned by the shell and freed below; the
    // token argument is None, meaning the calling user.
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT, None)
            .map_err(|e| AppError::Config(format!("could not find your Documents folder: {e}")))?;
        let owned = path.to_string().unwrap_or_default();
        windows::Win32::System::Com::CoTaskMemFree(Some(path.0 as *const _));
        if owned.is_empty() {
            return Err(AppError::Config(
                "Windows reported an empty Documents folder".into(),
            ));
        }
        Ok(PathBuf::from(owned))
    }
}

#[cfg(not(windows))]
fn known_documents() -> AppResult<PathBuf> {
    Err(AppError::Config(
        "adapters need Windows to find your Documents folder".into(),
    ))
}

fn local_app_data() -> AppResult<PathBuf> {
    std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .map_err(|_| AppError::Config("LOCALAPPDATA is not set".into()))
}

/// What applying this plan would do to the files on this machine.
///
/// Reads only. This is the diff the user sees before anything is written, and
/// it is produced by the same code that performs the write.
pub fn preview(plan: &AdapterPlan) -> Vec<FileDiff> {
    plan.files
        .iter()
        .map(|file| {
            // A template that will not resolve — no Documents folder — reads
            // the same as a file that is not there: nothing to change yet.
            let Ok(path) = resolve(&file.path_template) else {
                return missing_file(&file.path_template);
            };
            match std::fs::read_to_string(&path) {
                Ok(text) => tp_model::diff_edits(&path.display().to_string(), &text, &file.edits),
                // Almost always "the game has not been run yet", which is worth
                // saying rather than reporting as a failure.
                Err(_) => missing_file(&path.display().to_string()),
            }
        })
        .collect()
}

/// A file that is not there. Almost always "the game has not been run yet",
/// which the UI says in those words rather than reporting a failure.
fn missing_file(path: &str) -> FileDiff {
    FileDiff {
        path: path.to_string(),
        exists: false,
        changes: Vec::new(),
        missing: Vec::new(),
    }
}

/// Back up, then write.
///
/// Every file is copied to the backup store before any of them is written, and
/// the backup's id comes back so the UI can offer to undo exactly this change.
/// A file that does not exist is not created: an adapter writes settings into a
/// game's own config, and inventing that file would be inventing every setting
/// in it.
pub fn apply(plan: &AdapterPlan) -> AppResult<AppliedAdapter> {
    let mut targets = Vec::new();
    for file in &plan.files {
        let path = resolve(&file.path_template)?;
        if !path.is_file() {
            return Err(AppError::Config(format!(
                "{} is not there. Run the game once so it writes its settings, \
                 then try again — Team Principal edits that file, it does not \
                 invent it.",
                path.display()
            )));
        }
        targets.push((path, file));
    }

    let backup = crate::backup::capture(
        &plan.adapter_id,
        &targets
            .iter()
            .map(|(p, _)| p.display().to_string())
            .collect::<Vec<_>>(),
    )?;

    let mut written = Vec::new();
    for (path, file) in targets {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| AppError::Io(format!("could not read {}: {e}", path.display())))?;
        let updated = tp_model::apply_edits(&text, &file.edits);

        // Write-then-rename in the same directory, so a crash mid-write cannot
        // leave the game with a truncated config.
        let tmp = path.with_extension("tp-tmp");
        std::fs::write(&tmp, updated)?;
        std::fs::rename(&tmp, &path)?;
        written.push(path.display().to_string());
    }

    tracing::info!(adapter = %plan.adapter_id, %backup, "adapter applied");
    Ok(AppliedAdapter { backup, written })
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedAdapter {
    /// The backup id, so the UI can offer to undo exactly this.
    pub backup: String,
    pub written: Vec<String>,
}
