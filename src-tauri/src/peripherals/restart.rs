//! Restarting a device without touching its cable.
//!
//! Disable and re-enable, exactly as Device Manager does. The driver stack is
//! torn down and rebuilt and the device is configured again from nothing,
//! which is what a replug looks like from the driver's side of the port. It
//! does not cut power to the port — no Windows API does — so a device whose
//! firmware has locked up still needs its cable pulled, and the outcome says
//! so when the device does not come back.
//!
//! ## Why a second process
//!
//! `CM_Disable_DevNode` needs administrator rights, and this app runs
//! unelevated by design (see `app.manifest`). Running the whole app elevated
//! for one button would be the wrong trade, so the restart runs in a second
//! copy of this executable, started with the `runas` verb: one UAC prompt, one
//! device, exit. It reports back through its exit status, which is the whole
//! of what a process started that way can say — the log has the rest, since
//! both processes write to it. An instance that is already elevated skips the
//! prompt and does the work itself.
//!
//! Which node to restart, and the exit-status contract, live in
//! `tp_model::restart`, where they are tested.

#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};

use tp_model::{ReconnectOutcome, RestartResult};

use crate::error::{AppError, AppResult};

/// The command-line flag the helper is started with. Its one argument is the
/// instance ID of the node to restart.
pub const HELPER_FLAG: &str = "--restart-device";

/// Set while a restart is running. One at a time: each is a UAC prompt and a
/// device going away and coming back, and two of those interleaved is a screen
/// nobody can read.
#[cfg(windows)]
static IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// Clears the flag however the restart ends.
#[cfg(windows)]
struct Flight;

#[cfg(windows)]
impl Drop for Flight {
    fn drop(&mut self) {
        IN_FLIGHT.store(false, Ordering::Relaxed);
    }
}

/// Restart the physical device behind one HID interface.
///
/// `vid` and `pid` decide how far up the device tree the restart reaches — see
/// [`tp_model::node_to_restart`].
#[cfg(windows)]
pub fn reconnect(instance_path: &str, vid: u16, pid: u16) -> AppResult<ReconnectOutcome> {
    if IN_FLIGHT.swap(true, Ordering::Relaxed) {
        return Err(AppError::Config(
            "another device is being reconnected; wait for it to finish".into(),
        ));
    }
    let _flight = Flight;

    let chain = win::ancestry(instance_path)?;
    let target = &chain[tp_model::node_to_restart(&chain, vid, pid)];
    tracing::info!(device = %instance_path, target = %target, "restarting a device");

    if win::is_elevated() {
        report(win::restart(target))
    } else {
        win::restart_elevated(target)
    }
}

#[cfg(not(windows))]
pub fn reconnect(_instance_path: &str, _vid: u16, _pid: u16) -> AppResult<ReconnectOutcome> {
    Err(AppError::Config("restarting a device is only possible on Windows".into()))
}

/// The helper's whole life: restart one node and exit with the verdict.
///
/// Also what runs when the app is started by hand with the flag, which is a
/// legitimate way to restart a device from a script — and the reason the
/// argument is checked rather than trusted.
#[cfg(windows)]
pub fn helper_main(instance_id: &str) -> i32 {
    let id = instance_id.trim();
    // An instance ID is at most 200 characters and printable. Anything else
    // is not a device, and it is not sent to Windows.
    if id.is_empty() || id.len() > 200 || id.chars().any(char::is_control) {
        tracing::error!(argument = %instance_id, "not a device instance ID");
        return RestartResult::BadArguments.exit_code();
    }
    tracing::info!(id = %id, elevated = win::is_elevated(), "restarting a device as asked");
    let result = win::restart(id);
    tracing::info!(?result, "restart finished");
    result.exit_code()
}

#[cfg(not(windows))]
pub fn helper_main(_instance_id: &str) -> i32 {
    eprintln!("restarting a device is only possible on Windows");
    RestartResult::BadArguments.exit_code()
}

/// Turn what happened into what to tell the person.
///
/// Each failure names a different next step, which is why they are sentences
/// rather than a status the UI would have to translate.
#[cfg(windows)]
fn report(result: RestartResult) -> AppResult<ReconnectOutcome> {
    match result {
        RestartResult::Restarted => Ok(ReconnectOutcome::Restarted),
        RestartResult::NotFound => Err(AppError::Config(
            "Windows no longer lists this device. Unplug it and plug it back in.".into(),
        )),
        RestartResult::Refused => Err(AppError::Config(
            "Windows would not stop this device. Close anything using it — the game, or \
             the maker's own software — and try again."
                .into(),
        )),
        RestartResult::LeftDisabled => Err(AppError::Config(
            "It stopped but would not start again, so it is now disabled in Windows. \
             Enable it in Device Manager, or unplug it and plug it back in."
                .into(),
        )),
        RestartResult::NotBack => Err(AppError::Config(
            "It was stopped and started, but Windows has not seen it running again yet. \
             Give it a few seconds; if it stays red, the cable is the fix."
                .into(),
        )),
        RestartResult::BadArguments => Err(AppError::Config(
            "the restart was started without a device to restart".into(),
        )),
    }
}

#[cfg(windows)]
mod win {
    use std::time::{Duration, Instant};

    use tp_model::{ReconnectOutcome, RestartResult};
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Devices::DeviceAndDriverInstallation::*;
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_CANCELLED, HANDLE, WAIT_OBJECT_0, WIN32_ERROR,
    };

    use super::{report, HELPER_FLAG};
    use crate::error::{AppError, AppResult};

    /// How long to wait between attempts to start a stopped device. It is not
    /// always ready the instant it has stopped, and giving up leaves it
    /// disabled.
    const ENABLE_RETRY: Duration = Duration::from_millis(300);

    /// How long to wait for Windows to report the device running again.
    const STARTED_WAIT: Duration = Duration::from_secs(6);

    const STATUS_POLL: Duration = Duration::from_millis(100);

    /// How long the helper is given, all in. Its own work is bounded by the
    /// two waits above; the rest is process start-up.
    const HELPER_TIMEOUT_MS: u32 = 30_000;

    fn win32(operation: &str, e: windows::core::Error) -> AppError {
        AppError::Win32 {
            operation: operation.to_string(),
            code: e.code().0 as u32,
            message: e.message(),
        }
    }

    /// Whether this process already has administrator rights, in which case
    /// there is no prompt to show and no second process to start.
    pub fn is_elevated() -> bool {
        use windows::Win32::Security::{
            GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
        };
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        let mut token = HANDLE::default();
        // SAFETY: the token handle is closed below on every path, and the
        // elevation record is a plain struct passed with its own size.
        unsafe {
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
                return false;
            }
            let mut elevation = TOKEN_ELEVATION::default();
            let mut returned = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elevation as *mut TOKEN_ELEVATION as *mut core::ffi::c_void),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut returned,
            )
            .is_ok();
            let _ = CloseHandle(token);
            ok && elevation.TokenIsElevated != 0
        }
    }

    /// The instance IDs of the HID node and every node above it, nearest
    /// first.
    pub fn ancestry(instance_path: &str) -> AppResult<Vec<String>> {
        // SAFETY: the device-info set is destroyed on every path, and each
        // SetupDi call is given a buffer sized by its own preceding query.
        unsafe {
            let set = SetupDiCreateDeviceInfoList(None, None)
                .map_err(|e| win32("look the device up", e))?;
            let node = devnode_of(set, instance_path);
            let _ = SetupDiDestroyDeviceInfoList(set);
            let mut node = node?;

            let mut chain = Vec::new();
            loop {
                chain.push(device_id(node)?);
                let mut parent = 0u32;
                // The root has no parent, and that is the normal end. The cap
                // is for a tree deeper than any real one, which would mean
                // something other than a tree was being walked.
                if CM_Get_Parent(&mut parent, node, 0) != CR_SUCCESS || chain.len() >= 16 {
                    break;
                }
                node = parent;
            }
            Ok(chain)
        }
    }

    /// The device node an interface path belongs to.
    unsafe fn devnode_of(set: HDEVINFO, instance_path: &str) -> AppResult<u32> {
        let wide = HSTRING::from(instance_path);
        let mut interface = SP_DEVICE_INTERFACE_DATA {
            cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..Default::default()
        };
        SetupDiOpenDeviceInterfaceW(set, &wide, 0, Some(&mut interface))
            .map_err(|e| win32("find the device", e))?;

        // Sized by one call and filled by the next, the same two steps as
        // enumeration. The detail itself is the path already in hand; the
        // device record that comes back beside it is what is wanted.
        let mut size = 0u32;
        let _ = SetupDiGetDeviceInterfaceDetailW(set, &interface, None, 0, Some(&mut size), None);
        if size == 0 {
            return Err(AppError::Config("Windows reports nothing about this device path".into()));
        }
        let mut buffer = vec![0u8; size as usize];
        let detail = buffer.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
        (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
        let mut info = SP_DEVINFO_DATA {
            cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        SetupDiGetDeviceInterfaceDetailW(
            set,
            &interface,
            Some(detail),
            size,
            Some(&mut size),
            Some(&mut info),
        )
        .map_err(|e| win32("read the device's record", e))?;
        Ok(info.DevInst)
    }

    /// A node's instance ID — `USB\VID_346E&PID_001E\39001F00...`.
    unsafe fn device_id(node: u32) -> AppResult<String> {
        // An instance ID is at most 200 characters; one more for the
        // terminator.
        let mut buffer = [0u16; 201];
        let status = CM_Get_Device_IDW(node, &mut buffer, 0);
        if status != CR_SUCCESS {
            return Err(AppError::Config(format!(
                "Windows would not name a device above this one (CR {})",
                status.0
            )));
        }
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Ok(String::from_utf16_lossy(&buffer[..end]))
    }

    /// Stop the node, start it again, and wait to see it running.
    ///
    /// Runs in whichever process is elevated: the helper, or this one when it
    /// was started as an administrator.
    pub fn restart(instance_id: &str) -> RestartResult {
        let wide = HSTRING::from(instance_id);
        let mut node = 0u32;
        // SAFETY: every call takes the node handle by value and out-params
        // that live for the call.
        unsafe {
            let located = CM_Locate_DevNodeW(&mut node, &wide, CM_LOCATE_DEVNODE_NORMAL);
            if located != CR_SUCCESS {
                tracing::warn!(id = %instance_id, code = located.0, "no such device to restart");
                return RestartResult::NotFound;
            }

            let disabled = CM_Disable_DevNode(node, 0);
            if disabled != CR_SUCCESS {
                tracing::warn!(
                    id = %instance_id,
                    code = disabled.0,
                    "Windows would not stop the device"
                );
                return RestartResult::Refused;
            }
            tracing::info!(id = %instance_id, "device stopped");

            // Asked a few times before it is called a failure, because the
            // failure leaves the device disabled.
            let mut enabled = CM_Enable_DevNode(node, 0);
            for attempt in 1..=5 {
                if enabled == CR_SUCCESS {
                    break;
                }
                tracing::warn!(attempt, code = enabled.0, "could not start the device again yet");
                std::thread::sleep(ENABLE_RETRY);
                enabled = CM_Enable_DevNode(node, 0);
            }
            if enabled != CR_SUCCESS {
                tracing::error!(
                    id = %instance_id,
                    code = enabled.0,
                    "the device was stopped and could not be started again"
                );
                return RestartResult::LeftDisabled;
            }
            tracing::info!(id = %instance_id, "device started");

            // Read back rather than trust the return code: "enable" succeeding
            // means the request was accepted, not that the device came up.
            let deadline = Instant::now() + STARTED_WAIT;
            let mut problem = CM_PROB(0);
            loop {
                let mut status = CM_DEVNODE_STATUS_FLAGS(0);
                let mut current = CM_PROB(0);
                if CM_Get_DevNode_Status(&mut status, &mut current, node, 0) == CR_SUCCESS {
                    if status.contains(DN_STARTED) && !status.contains(DN_HAS_PROBLEM) {
                        tracing::info!(id = %instance_id, "device is running again");
                        return RestartResult::Restarted;
                    }
                    problem = if status.contains(DN_HAS_PROBLEM) {
                        current
                    } else {
                        CM_PROB(0)
                    };
                }
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(STATUS_POLL);
            }

            tracing::warn!(
                id = %instance_id,
                problem = problem.0,
                "the device has not come back"
            );
            if problem == CM_PROB_DISABLED {
                RestartResult::LeftDisabled
            } else {
                RestartResult::NotBack
            }
        }
    }

    /// Start a second copy of this executable as an administrator to do the
    /// restart, and wait for its verdict.
    pub fn restart_elevated(instance_id: &str) -> AppResult<ReconnectOutcome> {
        use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
        use windows::Win32::UI::Shell::{
            ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
            SHELLEXECUTEINFOW,
        };
        use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

        let exe = std::env::current_exe()
            .map_err(|e| AppError::Config(format!("could not find this program's file: {e}")))?;
        let exe = exe.to_string_lossy();
        let file = HSTRING::from(&*exe);
        let verb = HSTRING::from("runas");
        // Quoted, because an instance ID has backslashes and ampersands in it
        // and the helper has to read it back as one argument.
        let parameters = format!("{HELPER_FLAG} \"{instance_id}\"");
        let parameters = HSTRING::from(parameters.as_str());

        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            // A process handle to wait on; no shell error dialog, because the
            // outcome is reported in the app; and no async, because this
            // thread is going nowhere until the helper has finished.
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(parameters.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };

        tracing::info!(id = %instance_id, "asking for administrator rights to restart a device");

        // SAFETY: the strings outlive the call and the struct carries its own
        // size. The process handle it hands back is closed below.
        if let Err(e) = unsafe { ShellExecuteExW(&mut info) } {
            // The prompt was declined. Not a failure of anything: nothing was
            // touched, and the person knows why.
            if WIN32_ERROR::from_error(&e) == Some(ERROR_CANCELLED) {
                tracing::info!("the administrator prompt was declined");
                return Ok(ReconnectOutcome::Declined);
            }
            return Err(win32("start the administrator step", e));
        }

        // SAFETY: hProcess is a real handle once SEE_MASK_NOCLOSEPROCESS was
        // asked for and the call succeeded, and it is closed exactly once.
        let exit_code = unsafe {
            let waited = WaitForSingleObject(info.hProcess, HELPER_TIMEOUT_MS);
            let mut code = 0u32;
            let exited = waited == WAIT_OBJECT_0
                && GetExitCodeProcess(info.hProcess, &mut code).is_ok();
            let _ = CloseHandle(info.hProcess);
            exited.then_some(code)
        };

        // Never killed on a timeout: a helper caught between "stopped" and
        // "started" has to be allowed to finish, or the device stays stopped.
        let Some(code) = exit_code else {
            return Err(AppError::Config(
                "the administrator step did not finish in time; see the log".into(),
            ));
        };

        match RestartResult::from_exit_code(code as i32) {
            Some(result) => report(result),
            None => Err(AppError::Config(format!(
                "the administrator step ended without an answer (exit code {code}); see the log"
            ))),
        }
    }
}
