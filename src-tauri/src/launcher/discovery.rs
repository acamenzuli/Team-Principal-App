//! Finding installed games, so nobody has to type an install path.
//!
//! Steam records its library folders in `libraryfolders.vdf` and each installed
//! game in an `appmanifest_<id>.acf` beside it. Epic writes one JSON manifest
//! per game under ProgramData. Both are parsed rather than guessed at, and the
//! parsing itself lives in `tp_model::vdf` where it is tested against
//! truncated files, escaped Windows paths, and the older library format that
//! plenty of installs still use.

use std::path::{Path, PathBuf};

use tp_model::vdf;

/// A game found on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct InstalledGame {
    pub name: String,
    pub install_path: PathBuf,
    pub source: GameSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameSource {
    /// Launched as `steam://rungameid/<id>`.
    Steam { app_id: String },
    /// Launched as `com.epicgames.launcher://apps/<name>?action=launch`.
    Epic { app_name: String },
}

/// Everything installed, from every launcher this knows about.
pub fn discover() -> Vec<InstalledGame> {
    let mut games = steam_games();
    games.extend(epic_games());
    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    games.dedup_by(|a, b| a.install_path == b.install_path);
    tracing::info!(count = games.len(), "discovered installed games");
    games
}

// ------------------------------------------------------------------- Steam

pub fn steam_games() -> Vec<InstalledGame> {
    let Some(root) = steam_root() else {
        tracing::debug!("Steam does not appear to be installed");
        return Vec::new();
    };

    let library_file = root.join("steamapps").join("libraryfolders.vdf");
    let libraries = match std::fs::read_to_string(&library_file) {
        Ok(raw) => match vdf::parse_library_folders(&raw) {
            Ok(folders) => folders.into_iter().map(|f| PathBuf::from(f.path)).collect(),
            Err(e) => {
                tracing::warn!(file = %library_file.display(), error = %e, "could not read Steam libraries");
                // The main install is still a library even when the index is
                // unreadable, so fall back to it rather than finding nothing.
                vec![root.clone()]
            }
        },
        Err(_) => vec![root.clone()],
    };

    libraries
        .iter()
        .flat_map(|lib| games_in_library(lib))
        .collect()
}

fn games_in_library(library: &Path) -> Vec<InstalledGame> {
    let steamapps = library.join("steamapps");
    let Ok(entries) = std::fs::read_dir(&steamapps) else {
        return Vec::new();
    };

    entries
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("appmanifest_") && name.ends_with(".acf")
        })
        .filter_map(|e| {
            let raw = std::fs::read_to_string(e.path()).ok()?;
            let app = vdf::parse_app_manifest(&raw).ok()??;
            let install_path = steamapps.join("common").join(&app.install_dir);

            // A manifest can outlive the files it describes — an interrupted
            // uninstall leaves one behind. Listing a game that is not there
            // produces a launch that fails for no visible reason.
            install_path.is_dir().then_some(InstalledGame {
                name: app.name,
                install_path,
                source: GameSource::Steam { app_id: app.app_id },
            })
        })
        .collect()
}

/// Steam's install directory, from the registry.
#[cfg(windows)]
pub fn steam_root() -> Option<PathBuf> {
    read_registry_string(r"SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath")
        .or_else(|| read_registry_string(r"SOFTWARE\Valve\Steam", "InstallPath"))
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

#[cfg(not(windows))]
pub fn steam_root() -> Option<PathBuf> {
    None
}

#[cfg(windows)]
fn read_registry_string(key: &str, value: &str) -> Option<String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ,
    };

    // Steam records its path per-user as well as machine-wide, and which one
    // exists depends on how it was installed.
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let key_w = HSTRING::from(key);
        let value_w = HSTRING::from(value);
        let mut buf = [0u16; 512];
        let mut size = std::mem::size_of_val(&buf) as u32;

        // SAFETY: both strings outlive the call, and `size` matches the buffer.
        let status = unsafe {
            RegGetValueW(
                hive,
                PCWSTR(key_w.as_ptr()),
                PCWSTR(value_w.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if status == ERROR_SUCCESS {
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            let s = String::from_utf16_lossy(&buf[..end]).trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

// -------------------------------------------------------------------- Epic

/// Epic writes one JSON manifest per installed game.
pub fn epic_games() -> Vec<InstalledGame> {
    let Some(dir) = epic_manifest_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("item"))
        .filter_map(|e| {
            let raw = std::fs::read_to_string(e.path()).ok()?;
            let json: serde_json::Value = serde_json::from_str(&raw).ok()?;

            let install_path = PathBuf::from(json.get("InstallLocation")?.as_str()?);
            let app_name = json.get("AppName")?.as_str()?.to_string();
            let name = json
                .get("DisplayName")
                .and_then(|v| v.as_str())
                .unwrap_or(&app_name)
                .to_string();

            install_path.is_dir().then_some(InstalledGame {
                name,
                install_path,
                source: GameSource::Epic { app_name },
            })
        })
        .collect()
}

fn epic_manifest_dir() -> Option<PathBuf> {
    let program_data = std::env::var_os("ProgramData")?;
    let dir = PathBuf::from(program_data)
        .join("Epic")
        .join("EpicGamesLauncher")
        .join("Data")
        .join("Manifests");
    dir.is_dir().then_some(dir)
}

/// The URI that starts a game through its own launcher.
///
/// Protocol launches return no process handle, which is why the window matcher
/// has to work from the executable name instead — see `tp_model::window`.
pub fn launch_uri(source: &GameSource) -> String {
    match source {
        GameSource::Steam { app_id } => format!("steam://rungameid/{app_id}"),
        GameSource::Epic { app_name } => {
            format!("com.epicgames.launcher://apps/{app_name}?action=launch")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_uris_are_what_the_launchers_expect() {
        assert_eq!(
            launch_uri(&GameSource::Steam {
                app_id: "244210".into()
            }),
            "steam://rungameid/244210"
        );
        assert_eq!(
            launch_uri(&GameSource::Epic {
                app_name: "Fortnite".into()
            }),
            "com.epicgames.launcher://apps/Fortnite?action=launch"
        );
    }
}
