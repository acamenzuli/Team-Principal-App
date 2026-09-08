//! Starting things.
//!
//! Two kinds, and they behave differently in the one way that matters:
//!
//! * A direct executable gives back a PID, which is the strongest signal the
//!   window matcher has.
//! * A protocol launch — `steam://rungameid/...` — gives back nothing. The
//!   handler process exits immediately and the game appears later as a
//!   grandchild the launcher never started. That is not an edge case; it is how
//!   most sims start.

use std::path::Path;
use std::process::Command;

use crate::error::{AppError, AppResult};

/// What starting something produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Started {
    /// A direct launch, with the process id.
    Pid(u32),
    /// A protocol launch. Nothing to correlate by; the window matcher has to
    /// work from the executable name instead.
    Detached,
}

/// Start an executable.
pub fn start_executable(path: &str, args: &[String], working_dir: Option<&str>) -> AppResult<u32> {
    let exe = Path::new(path);
    if !exe.is_file() {
        return Err(AppError::Config(format!(
            "{path} is not there. If the game moved, re-detect it on the Games tab."
        )));
    }

    let mut command = Command::new(exe);
    command.args(args);
    // Default to the executable's own folder: plenty of games read relative
    // paths and fail in confusing ways when started from elsewhere.
    match working_dir {
        Some(dir) => {
            command.current_dir(dir);
        }
        None => {
            if let Some(parent) = exe.parent() {
                command.current_dir(parent);
            }
        }
    }

    let child = command
        .spawn()
        .map_err(|e| AppError::Config(format!("could not start {path}: {e}")))?;

    let pid = child.id();
    tracing::info!(%path, pid, "started");
    Ok(pid)
}

/// Open a URI with whatever handles it — Steam, Epic, the Store.
#[cfg(windows)]
pub fn start_uri(uri: &str) -> AppResult<Started> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    // Only schemes a launcher legitimately uses. A profile is user data, and
    // handing an arbitrary string to ShellExecute would make it a way to run
    // anything at all.
    const ALLOWED: [&str; 4] = [
        "steam:",
        "com.epicgames.launcher:",
        "shell:appsfolder",
        "ms-windows-store:",
    ];
    let lower = uri.to_ascii_lowercase();
    if !ALLOWED.iter().any(|s| lower.starts_with(s)) {
        return Err(AppError::Config(format!(
            "{uri} is not a launcher address. Team Principal only opens Steam, Epic and Store links."
        )));
    }

    let wide = HSTRING::from(uri);

    // SAFETY: the string outlives the call. ShellExecuteW returns a pseudo
    // handle that must not be closed.
    let result = unsafe { ShellExecuteW(None, None, &wide, None, None, SW_SHOWNORMAL) };

    // Values at or below 32 are error codes rather than handles.
    if result.0 as usize <= 32 {
        return Err(AppError::Config(format!(
            "nothing on this machine opens {uri}. Is the launcher installed?"
        )));
    }

    tracing::info!(%uri, "opened; the game will appear as a separate process");
    Ok(Started::Detached)
}

#[cfg(not(windows))]
pub fn start_uri(_uri: &str) -> AppResult<Started> {
    Err(AppError::Config("protocol launches require Windows".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_executable_says_so_usefully() {
        let err = start_executable("/no/such/game.exe", &[], None).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("is not there"), "{message}");
        assert!(
            message.contains("Games tab"),
            "it says what to do: {message}"
        );
    }
}
