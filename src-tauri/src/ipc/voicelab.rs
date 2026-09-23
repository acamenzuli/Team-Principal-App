//! The Voice Lab's commands.
//!
//! Deliberately few. The tab talks to the Python service directly over
//! loopback for everything about voices, packs and generation — that is a
//! lot of surface, it is all local, and routing it through Rust would add a
//! hop and a second set of types for no gain. What crosses here is what only
//! the app can do: look at the hardware, build the module, and own the
//! service's lifetime.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, State};
use tp_model::{ModuleStatus, ServiceInfo, VoiceLabRequirements};

use crate::error::{AppError, AppResult};
use crate::voicelab::module::{self, Install, SharedInstall};
use crate::voicelab::service::SharedSupervisor;

/// Emitted as the module is built.
pub const MODULE_EVENT: &str = "voicelab://module";

/// Managed state: the installer's progress, shared with the thread doing it.
#[derive(Clone)]
pub struct VoiceLabState {
    pub install: SharedInstall,
    pub supervisor: SharedSupervisor,
}

impl VoiceLabState {
    pub fn new(supervisor: SharedSupervisor) -> Self {
        Self {
            install: Arc::new(Mutex::new(Install::default())),
            supervisor,
        }
    }
}

fn home_for(settings_home: Option<String>) -> PathBuf {
    module::home(settings_home.as_deref())
}

fn configured_home() -> PathBuf {
    let (prefs, _) = crate::settings::load();
    home_for(prefs.voice_lab.home)
}

/// What this machine has, and whether it is enough.
#[tauri::command]
pub fn voicelab_requirements() -> VoiceLabRequirements {
    crate::voicelab::gpu::requirements()
}

/// What is installed, read from disk rather than remembered.
#[tauri::command]
pub fn voicelab_module(state: State<'_, VoiceLabState>) -> ModuleStatus {
    let home = configured_home();
    let (busy, message, error, log) = state
        .install
        .lock()
        .map(|p| (p.busy, p.message.clone(), p.error.clone(), p.log.clone()))
        .unwrap_or_default();
    module::status(&home, busy, message, error, log)
}

/// Download and build the module. Returns immediately; progress arrives on
/// `voicelab://module`.
#[tauri::command]
pub fn voicelab_install_module(
    app: AppHandle,
    state: State<'_, VoiceLabState>,
) -> AppResult<ModuleStatus> {
    let home = configured_home();
    {
        let mut progress = state
            .install
            .lock()
            .map_err(|_| AppError::Config("the installer state is unreadable".into()))?;
        if progress.busy {
            return Err(AppError::Config(
                "the Voice Lab module is already being installed".into(),
            ));
        }
        *progress = Install {
            busy: true,
            message: "Starting…".into(),
            error: None,
            log: Vec::new(),
        };
    }
    let source = module::service_source(&app).ok_or_else(|| {
        AppError::Config("the voice service's files are missing from this install".into())
    })?;

    let progress = state.install.clone();
    let handle = app.clone();
    std::thread::Builder::new()
        .name("voicelab-install".into())
        .spawn(move || {
            module::install(&home, &source, progress, move |status| {
                if let Err(e) = handle.emit(MODULE_EVENT, &status) {
                    tracing::warn!(error = %e, "could not publish the Voice Lab install progress");
                }
            });
        })
        .map_err(|e| AppError::Config(format!("could not start the installer: {e}")))?;

    Ok(voicelab_module(state))
}

/// Delete the module and say how much space came back.
///
/// Recordings and generated packs are kept unless `keep_work` is false: they
/// are hours of somebody's evening, and "remove the download" should not
/// throw them away.
#[tauri::command]
pub fn voicelab_remove_module(
    app: AppHandle,
    state: State<'_, VoiceLabState>,
    keep_work: bool,
) -> AppResult<ModuleStatus> {
    let home = configured_home();
    state.supervisor.stop(&app);
    let freed = module::remove(&home, keep_work)?;
    if let Ok(mut progress) = state.install.lock() {
        *progress = Install {
            busy: false,
            message: format!("Removed. {} of disk space is back.", format_bytes(freed)),
            error: None,
            log: Vec::new(),
        };
    }
    Ok(voicelab_module(state))
}

/// Where the service is and how to talk to it.
#[tauri::command]
pub fn voicelab_service(state: State<'_, VoiceLabState>) -> ServiceInfo {
    state.supervisor.info()
}

/// Start it if it is not already up. Resolves when `/health` answers.
#[tauri::command]
pub fn voicelab_start_service(
    app: AppHandle,
    state: State<'_, VoiceLabState>,
) -> AppResult<ServiceInfo> {
    let (prefs, _) = crate::settings::load();
    let home = home_for(prefs.voice_lab.home.clone());
    let source = module::service_source(&app).ok_or_else(|| {
        AppError::Config("the voice service's files are missing from this install".into())
    })?;
    state.supervisor.start(
        &app,
        &home,
        &source,
        prefs.voice_lab.sounds_folder.as_deref(),
    )
}

#[tauri::command]
pub fn voicelab_stop_service(app: AppHandle, state: State<'_, VoiceLabState>) -> ServiceInfo {
    state.supervisor.stop(&app)
}

fn format_bytes(bytes: u64) -> String {
    const GB: u64 = 1024 * 1024 * 1024;
    const MB: u64 = 1024 * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{} MB", bytes / MB)
    } else {
        format!("{bytes} bytes")
    }
}

/// Stop the service if it has been idle, and keep the racing flag in step
/// with the launcher. Called on a timer from the app's setup.
pub fn tick(app: &AppHandle) {
    let Some(state) = app.try_state::<VoiceLabState>() else {
        return;
    };
    if !state.supervisor.is_running() {
        return;
    }
    let (prefs, _) = crate::settings::load();
    state
        .supervisor
        .stop_if_idle(app, prefs.voice_lab.idle_stop_minutes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freed_space_reads_like_a_person_wrote_it() {
        assert_eq!(format_bytes(14 * 1024 * 1024 * 1024), "14.0 GB");
        assert_eq!(format_bytes(512 * 1024 * 1024), "512 MB");
        assert_eq!(format_bytes(900), "900 bytes");
    }
}
