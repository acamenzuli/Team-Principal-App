//! Profile storage.
//!
//! `%APPDATA%\Team Principal\profiles\<uuid>.json`, written atomically like
//! everything else the app owns: write-then-rename in the same directory, so a
//! crash mid-save cannot leave a file the app then refuses to load.

use std::path::PathBuf;

use tp_model::{
    GameRef, LaunchMethod, Profile, RectMeans, RectSource, RigBinding, SessionMode, TeardownPolicy,
    WatchdogPolicy, WindowPlan, WindowTarget, PROFILE_SCHEMA_VERSION,
};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

fn dir() -> PathBuf {
    crate::logging::app_data_dir().join("profiles")
}

fn path_for(id: Uuid) -> PathBuf {
    dir().join(format!("{id}.json"))
}

pub fn list() -> Vec<Profile> {
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return Vec::new();
    };
    let mut profiles: Vec<Profile> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
        .filter_map(|e| {
            let raw = std::fs::read_to_string(e.path()).ok()?;
            match serde_json::from_str::<Profile>(&raw) {
                Ok(p) => Some(p),
                Err(err) => {
                    // One unreadable profile must not hide the rest.
                    tracing::warn!(path = %e.path().display(), %err, "skipping unreadable profile");
                    None
                }
            }
        })
        .collect();
    profiles.sort_by_key(|p| p.name.to_lowercase());
    profiles
}

pub fn load(id: Uuid) -> AppResult<Profile> {
    let path = path_for(id);
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| AppError::Config(format!("could not read {}: {e}", path.display())))?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn save(profile: &Profile) -> AppResult<Profile> {
    let mut profile = profile.clone();
    profile.schema_version = PROFILE_SCHEMA_VERSION;
    profile.updated_at = crate::now_iso8601();

    std::fs::create_dir_all(dir())?;
    let path = path_for(profile.id);
    let json = serde_json::to_string_pretty(&profile)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;

    tracing::info!(id = %profile.id, name = %profile.name, "profile saved");
    Ok(profile)
}

pub fn delete(id: Uuid) -> AppResult<()> {
    let path = path_for(id);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// A profile for a game that has none yet.
///
/// Deliberately empty of utilities and peripherals rather than guessing: a
/// preflight that checks things nobody asked for is one people learn to ignore.
pub fn starter(name: &str, launch: LaunchMethod, install_path: Option<String>) -> Profile {
    let now = crate::now_iso8601();
    Profile {
        schema_version: PROFILE_SCHEMA_VERSION,
        id: Uuid::new_v4(),
        name: name.to_string(),
        game: GameRef {
            adapter_id: String::new(),
            install_path,
            launch,
        },
        rig: RigBinding {
            rig_id: Uuid::nil(),
            computed_against_revision: 0,
            derived_snapshot: Default::default(),
        },
        session_mode: SessionMode::CenterOnly,
        window_plan: WindowPlan {
            target: WindowTarget {
                exe_name: None,
                window_class: None,
                title_regex: None,
                min_size: (640, 480),
                require_visible: true,
                timeout_ms: 60_000,
            },
            rect: RectSource::FromGeometry,
            rect_means: RectMeans::ClientArea,
            borderless: true,
            always_on_top: false,
            hide_taskbar: false,
            watchdog: WatchdogPolicy::default(),
        },
        peripherals: Vec::new(),
        utilities: Vec::new(),
        steps: Vec::new(),
        teardown: TeardownPolicy::default(),
        created_at: now.clone(),
        updated_at: now,
    }
}
