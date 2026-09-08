//! A Job Object, so nothing survives that should not.
//!
//! Utilities started for a session are put in a job with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. If Team Principal exits — cleanly, by
//! crash, or by being killed from Task Manager — Windows closes the last handle
//! to the job and terminates everything in it.
//!
//! Without this, a crash during a session leaves SimHub, a vJoy feeder and
//! whatever else running, and the user's next launch finds them already up in
//! an unknown state. That is worse than a crash.
//!
//! The job is deliberately *not* used for the game itself. Killing a sim
//! because the launcher died would lose a race.

use crate::error::{AppError, AppResult};

/// Owns the job. Dropping it terminates every process inside.
pub struct JobObject {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
}

// SAFETY: a job handle is an opaque kernel handle, usable from any thread.
#[cfg(windows)]
unsafe impl Send for JobObject {}
#[cfg(windows)]
unsafe impl Sync for JobObject {}

#[cfg(windows)]
impl JobObject {
    pub fn new() -> AppResult<Self> {
        use windows::Win32::System::JobObjects::*;

        // SAFETY: the handle is stored and closed exactly once, in Drop.
        unsafe {
            let handle = CreateJobObjectW(None, None).map_err(|e| AppError::Win32 {
                operation: "create a job object for the session".into(),
                code: e.code().0 as u32,
                message: e.message(),
            })?;

            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(|e| AppError::Win32 {
                operation: "configure the session job object".into(),
                code: e.code().0 as u32,
                message: e.message(),
            })?;

            Ok(Self { handle })
        }
    }

    /// Put a process in the job. It and its children die with the job.
    pub fn adopt(&self, pid: u32) -> AppResult<()> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::JobObjects::AssignProcessToJobObject;
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
        };

        // SAFETY: the process handle is closed on both paths.
        unsafe {
            let process =
                OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid).map_err(|e| {
                    AppError::Win32 {
                        operation: format!("open process {pid} to manage it"),
                        code: e.code().0 as u32,
                        message: e.message(),
                    }
                })?;
            let result = AssignProcessToJobObject(self.handle, process);
            let _ = CloseHandle(process);

            result.map_err(|e| AppError::Win32 {
                operation: format!("put process {pid} under the session job"),
                code: e.code().0 as u32,
                message: e.message(),
            })
        }
    }

    /// Terminate everything in the job now, rather than waiting for drop.
    pub fn terminate_all(&self) {
        use windows::Win32::System::JobObjects::TerminateJobObject;
        // SAFETY: `handle` is valid until Drop.
        unsafe {
            let _ = TerminateJobObject(self.handle, 0);
        }
    }
}

#[cfg(windows)]
impl Drop for JobObject {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        // Closing the last handle is what triggers KILL_ON_JOB_CLOSE.
        // SAFETY: closed exactly once.
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(not(windows))]
impl JobObject {
    pub fn new() -> AppResult<Self> {
        Err(AppError::Config("job objects require Windows".into()))
    }
    pub fn adopt(&self, _pid: u32) -> AppResult<()> {
        Ok(())
    }
    pub fn terminate_all(&self) {}
}
