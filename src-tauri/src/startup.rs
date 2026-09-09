//! Starting with Windows.
//!
//! One registry value under the current user's `Run` key. Not a scheduled task,
//! not a service, not a shortcut in the Startup folder:
//!
//! * A **scheduled task** needs elevation to create and is invisible to the
//!   Task Manager startup tab, where people go to turn things off. An app that
//!   cannot be disabled where everyone looks for the switch is an app that gets
//!   uninstalled.
//! * A **service** is wrong for something with a window.
//! * A **Startup-folder shortcut** is a file that goes stale the moment the app
//!   moves, and leaves litter behind when it is uninstalled badly.
//!
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` needs no elevation,
//! appears in Task Manager's startup list where the user expects it, and is
//! removed by writing one value away.
//!
//! ## Read back, never assumed
//!
//! The toggle reflects what the registry actually says, re-read each time.
//! Windows itself disables startup entries — through Task Manager, or through
//! its own "app impact" heuristics — without telling the app, and a switch
//! showing On over an entry Windows has turned off is a lie the user only finds
//! out about the morning it matters.

use crate::error::{AppError, AppResult};

/// The value name under `Run`. Also what Task Manager shows in its list.
const VALUE: &str = "Team Principal";

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// The command that would be registered: this executable, started minimised.
///
/// Minimised because a boot start should not take the screen. Someone who opens
/// the app themselves gets the window; the machine starting it does not.
fn command() -> AppResult<String> {
    let exe = std::env::current_exe()
        .map_err(|e| AppError::Config(format!("could not find this program's own path: {e}")))?;
    // Quoted: the path contains spaces on any normal install, and an unquoted
    // value is silently truncated at the first one.
    Ok(format!("\"{}\" --minimised", exe.display()))
}

/// Is the app registered to start with Windows?
#[cfg(windows)]
pub fn is_enabled() -> bool {
    read_value().is_some()
}

/// Whether the registered command still points at this executable.
///
/// False after the app has been moved or reinstalled elsewhere, which leaves a
/// startup entry that launches nothing. Worth surfacing rather than silently
/// repairing: quietly rewriting a registry value the user did not ask about is
/// not this app's business.
#[cfg(windows)]
pub fn is_stale() -> bool {
    match (read_value(), command()) {
        (Some(existing), Ok(wanted)) => !existing.eq_ignore_ascii_case(&wanted),
        _ => false,
    }
}

#[cfg(windows)]
pub fn set(enabled: bool) -> AppResult<()> {
    if enabled {
        write_value(&command()?)
    } else {
        delete_value()
    }
}

#[cfg(windows)]
fn read_value() -> Option<String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};

    let key = HSTRING::from(RUN_KEY);
    let value = HSTRING::from(VALUE);
    let mut buf = [0u16; 1024];
    let mut size = std::mem::size_of_val(&buf) as u32;

    // SAFETY: both strings outlive the call, and `size` matches the buffer.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let chars = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..chars.min(buf.len())]))
}

#[cfg(windows)]
fn write_value(command: &str) -> AppResult<()> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ,
    };

    let key_name = HSTRING::from(RUN_KEY);
    let value_name = HSTRING::from(VALUE);
    // A registry string is NUL-terminated and its length is counted in bytes
    // including that terminator. Getting either wrong writes a value Windows
    // reads back truncated.
    let wide: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(wide.as_ptr() as *const u8, std::mem::size_of_val(&wide[..]))
    };

    let mut key = HKEY::default();
    // SAFETY: the strings outlive the call; the key is closed below.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_name.as_ptr()),
            None,
            KEY_SET_VALUE,
            &mut key,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(registry_error("opening the startup key", status.0));
    }

    // SAFETY: `bytes` is the byte view of `wide`, which outlives the call.
    let status =
        unsafe { RegSetValueExW(key, PCWSTR(value_name.as_ptr()), None, REG_SZ, Some(bytes)) };
    // SAFETY: `key` came from RegOpenKeyExW above and is closed exactly once.
    unsafe {
        let _ = RegCloseKey(key);
    }

    if status != ERROR_SUCCESS {
        return Err(registry_error("writing the startup entry", status.0));
    }
    tracing::info!(%command, "registered to start with Windows");
    Ok(())
}

#[cfg(windows)]
fn delete_value() -> AppResult<()> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
    };

    let key_name = HSTRING::from(RUN_KEY);
    let value_name = HSTRING::from(VALUE);
    let mut key = HKEY::default();

    // SAFETY: the string outlives the call; the key is closed below.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_name.as_ptr()),
            None,
            KEY_SET_VALUE,
            &mut key,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(registry_error("opening the startup key", status.0));
    }

    // SAFETY: as above.
    let status = unsafe { RegDeleteValueW(key, PCWSTR(value_name.as_ptr())) };
    // SAFETY: `key` came from RegOpenKeyExW above and is closed exactly once.
    unsafe {
        let _ = RegCloseKey(key);
    }

    // Already absent is success: the user asked for it to be off, and it is.
    if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
        return Err(registry_error("removing the startup entry", status.0));
    }
    tracing::info!("no longer starting with Windows");
    Ok(())
}

#[cfg(windows)]
fn registry_error(operation: &str, code: u32) -> AppError {
    AppError::Win32 {
        operation: operation.to_string(),
        code,
        message: std::io::Error::from_raw_os_error(code as i32).to_string(),
    }
}

#[cfg(not(windows))]
pub fn is_enabled() -> bool {
    false
}

#[cfg(not(windows))]
pub fn is_stale() -> bool {
    false
}

#[cfg(not(windows))]
pub fn set(_enabled: bool) -> AppResult<()> {
    Err(AppError::Config(
        "starting with the machine requires Windows".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registered_command_is_quoted_and_minimised() {
        // An unquoted path is silently truncated at its first space, which is
        // every normal install of this app.
        let Ok(command) = command() else { return };
        assert!(command.starts_with('"'), "{command}");
        assert!(command.contains("\" --minimised"), "{command}");
    }
}
