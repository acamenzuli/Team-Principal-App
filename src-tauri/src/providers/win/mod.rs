//! Win32 provider implementations.
//!
//! Milestone 1 puts the seams in place and implements only the DPI-awareness
//! check, because getting DPI wrong at the start is the one mistake that is
//! genuinely miserable to retrofit. Real enumeration lands in milestone 2.

pub mod dpi;

use tp_model::{DetectedDevice, MonitorInfo, TopologySnapshot};

use super::{DisplayProvider, PeripheralProvider, ProcessProvider, WindowProvider};
use crate::error::{AppError, AppResult};

fn todo_in(what: &str, milestone: u8) -> AppError {
    AppError::NotYetImplemented {
        what: what.to_string(),
        milestone,
    }
}

#[derive(Default)]
pub struct WinDisplayProvider;
impl WinDisplayProvider {
    pub fn new() -> Self {
        Self
    }
}
impl DisplayProvider for WinDisplayProvider {
    fn enumerate(&self) -> AppResult<Vec<MonitorInfo>> {
        Err(todo_in("monitor enumeration", 2))
    }
    fn capture_snapshot(&self) -> AppResult<TopologySnapshot> {
        Err(todo_in("topology snapshots", 9))
    }
}

#[derive(Default)]
pub struct WinPeripheralProvider;
impl WinPeripheralProvider {
    pub fn new() -> Self {
        Self
    }
}
impl PeripheralProvider for WinPeripheralProvider {
    fn enumerate(&self) -> AppResult<Vec<DetectedDevice>> {
        Err(todo_in("peripheral enumeration", 5))
    }
}

#[derive(Default)]
pub struct WinWindowProvider;
impl WinWindowProvider {
    pub fn new() -> Self {
        Self
    }
}
impl WindowProvider for WinWindowProvider {
    fn find_window(&self, _exe_name: &str) -> AppResult<Option<u64>> {
        Err(todo_in("window control", 6))
    }
}

#[derive(Default)]
pub struct WinProcessProvider;
impl WinProcessProvider {
    pub fn new() -> Self {
        Self
    }
}
impl ProcessProvider for WinProcessProvider {
    fn is_running(&self, _exe_name: &str) -> AppResult<bool> {
        Err(todo_in("process orchestration", 7))
    }
}
