//! The live axis and button monitor.
//!
//! This is how you confirm a wheel or a pedal set is actually working, and it
//! is what makes a support conversation short: "press the brake and tell me if
//! the bar moves" beats twenty minutes of guessing.
//!
//! ## Why it reads HID reports rather than using DirectInput
//!
//! DirectInput can read axes, but only after `Acquire()`, and acquiring a
//! device is precisely the operation that can interfere with a game. Reading
//! the HID input report needs no acquisition at all: Windows gives every open
//! handle its own report queue, so this observes the same reports the game
//! receives without competing for them.
//!
//! The handle is opened read-only with full sharing, and monitoring is stopped
//! entirely while a session is running — see [`suspend`]. Both are belt and
//! braces on top of a mechanism that already cannot steal input.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::AppHandle;
pub use tp_model::{AxisReading, InputFrame};

/// The event carrying live values.
pub const EVENT: &str = "peripherals://input";

/// Roughly 30 updates a second. Faster is invisible on screen and only costs
/// IPC traffic; slower makes a pedal feel laggy to the person testing it.
const MIN_INTERVAL_MS: u64 = 33;

/// Set while a game session is running.
///
/// The monitor must not read a device during a session. Nothing here can steal
/// input, but "the launcher was reading my wheel" is a support theory that
/// costs more to disprove than to prevent.
static SUSPENDED: AtomicBool = AtomicBool::new(false);

pub fn suspend(yes: bool) {
    SUSPENDED.store(yes, Ordering::Relaxed);
    tracing::info!(suspended = yes, "input monitoring");
}

pub fn is_suspended() -> bool {
    SUSPENDED.load(Ordering::Relaxed)
}

/// A running monitor. Dropping it stops the thread.
pub struct Monitor {
    stop: Arc<AtomicBool>,
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Managed state: at most one device is monitored at a time, because only one
/// is on screen and reading the rest would be work nobody asked for.
#[derive(Default)]
pub struct ActiveMonitor(pub std::sync::Mutex<Option<Monitor>>);

#[cfg(windows)]
pub fn start(app: AppHandle, instance_path: String) -> Monitor {
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();

    std::thread::Builder::new()
        .name("input-monitor".into())
        .spawn(move || {
            if let Err(e) = win::run(&app, &instance_path, &thread_stop) {
                tracing::warn!(path = %instance_path, error = %e, "input monitor stopped");
            }
        })
        .expect("could not start the input monitor thread");

    Monitor { stop }
}

#[cfg(not(windows))]
pub fn start(_app: AppHandle, _instance_path: String) -> Monitor {
    Monitor {
        stop: Arc::new(AtomicBool::new(false)),
    }
}

#[cfg(windows)]
mod win {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use tauri::{AppHandle, Emitter};
    use windows::core::HSTRING;
    use windows::Win32::Devices::HumanInterfaceDevice::*;
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::Storage::FileSystem::*;
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
    use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

    use super::{AxisReading, InputFrame, EVENT, MIN_INTERVAL_MS};
    use crate::error::{AppError, AppResult};

    pub fn run(app: &AppHandle, path: &str, stop: &AtomicBool) -> AppResult<()> {
        // SAFETY: every handle opened below is closed before returning, on all
        // paths including the error ones.
        unsafe {
            let wide = HSTRING::from(path);

            // GENERIC_READ with full sharing and overlapped I/O. Read-only, so
            // this cannot write to the device; fully shared, so it cannot lock
            // a game out of it.
            let device = CreateFileW(
                &wide,
                GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )
            .map_err(|e| AppError::Win32 {
                operation: "open the device for monitoring".into(),
                code: e.code().0 as u32,
                message: e.message(),
            })?;

            let result = pump(app, path, stop, device);
            let _ = CloseHandle(device);
            result
        }
    }

    unsafe fn pump(
        app: &AppHandle,
        path: &str,
        stop: &AtomicBool,
        device: HANDLE,
    ) -> AppResult<()> {
        let mut preparsed = PHIDP_PREPARSED_DATA::default();
        if !HidD_GetPreparsedData(device, &mut preparsed) {
            return Err(AppError::Config(
                "this device reports no HID descriptor".into(),
            ));
        }

        let mut caps = HIDP_CAPS::default();
        let caps_ok = HidP_GetCaps(preparsed, &mut caps).is_ok();
        if !caps_ok || caps.InputReportByteLength == 0 {
            let _ = HidD_FreePreparsedData(preparsed);
            return Err(AppError::Config(
                "this device sends no input reports".into(),
            ));
        }

        let value_caps = value_caps(preparsed, &caps);
        let button_caps = button_caps(preparsed, &caps);

        // A manual-reset event is not needed; the read completes or is
        // cancelled, and nothing else waits on this.
        let event = CreateEventW(None, false, false, None).map_err(|e| AppError::Win32 {
            operation: "create the monitor's wait event".into(),
            code: e.code().0 as u32,
            message: e.message(),
        })?;

        let mut report = vec![0u8; caps.InputReportByteLength as usize];
        let mut last_sent = Instant::now() - Duration::from_millis(MIN_INTERVAL_MS);

        while !stop.load(Ordering::Relaxed) {
            // Suspended during a game session: sleep rather than exit, so
            // resuming does not need the device reopened.
            if super::is_suspended() {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }

            let mut overlapped = OVERLAPPED {
                hEvent: event,
                ..Default::default()
            };
            let read = ReadFile(
                device,
                Some(report.as_mut_slice()),
                None,
                Some(&mut overlapped),
            );

            // A pending read is the normal case: reports arrive when the user
            // moves something, which may be never.
            if read.is_err() {
                // Wait with a timeout so the stop flag is checked regularly
                // rather than only when the user happens to move an axis.
                if WaitForSingleObject(event, 250) != WAIT_OBJECT_0 {
                    let _ = CancelIoEx(device, Some(&overlapped));
                    continue;
                }
            }

            let mut transferred = 0u32;
            if GetOverlappedResult(device, &overlapped, &mut transferred, false).is_err() {
                continue;
            }
            if transferred == 0 {
                continue;
            }

            // Throttle to something a screen can show. Sim hardware reports at
            // 1000 Hz; forwarding all of it would be pure IPC traffic.
            if last_sent.elapsed() < Duration::from_millis(MIN_INTERVAL_MS) {
                continue;
            }
            last_sent = Instant::now();

            let n = transferred as usize;
            let frame = decode(preparsed, &mut report[..n], &value_caps, &button_caps, path);
            if let Err(e) = app.emit(EVENT, &frame) {
                tracing::debug!(error = %e, "could not publish an input frame");
                break;
            }
        }

        let _ = CancelIoEx(device, None);
        let _ = CloseHandle(event);
        let _ = HidD_FreePreparsedData(preparsed);
        Ok(())
    }

    unsafe fn value_caps(
        preparsed: PHIDP_PREPARSED_DATA,
        caps: &HIDP_CAPS,
    ) -> Vec<HIDP_VALUE_CAPS> {
        let mut count = caps.NumberInputValueCaps;
        if count == 0 {
            return Vec::new();
        }
        let mut out = vec![HIDP_VALUE_CAPS::default(); count as usize];
        if HidP_GetValueCaps(HidP_Input, out.as_mut_ptr(), &mut count, preparsed).is_err() {
            return Vec::new();
        }
        out.truncate(count as usize);
        out.retain(|c| tp_model::is_axis(c.UsagePage, unsafe { c.Anonymous.Range.UsageMin }));
        out
    }

    unsafe fn button_caps(
        preparsed: PHIDP_PREPARSED_DATA,
        caps: &HIDP_CAPS,
    ) -> Vec<HIDP_BUTTON_CAPS> {
        let mut count = caps.NumberInputButtonCaps;
        if count == 0 {
            return Vec::new();
        }
        let mut out = vec![HIDP_BUTTON_CAPS::default(); count as usize];
        if HidP_GetButtonCaps(HidP_Input, out.as_mut_ptr(), &mut count, preparsed).is_err() {
            return Vec::new();
        }
        out.truncate(count as usize);
        out
    }

    unsafe fn decode(
        preparsed: PHIDP_PREPARSED_DATA,
        report: &mut [u8],
        values: &[HIDP_VALUE_CAPS],
        buttons: &[HIDP_BUTTON_CAPS],
        path: &str,
    ) -> InputFrame {
        let axes: Vec<AxisReading> = values
            .iter()
            .filter_map(|cap| {
                let usage = cap.Anonymous.Range.UsageMin;
                let mut raw = 0u32;
                // NTSTATUS rather than a Result, so it cannot be `?`-ed
                // straight into the surrounding Option.
                if HidP_GetUsageValue(
                    HidP_Input,
                    cap.UsagePage,
                    None,
                    usage,
                    &mut raw,
                    preparsed,
                    report,
                )
                .ok()
                .is_err()
                {
                    return None;
                }

                // The conversion, including sign extension for devices that
                // declare a negative logical minimum, is tested in tp-model.
                Some(AxisReading {
                    name: tp_model::axis_name(usage).to_string(),
                    value: tp_model::normalise(raw, cap.LogicalMin, cap.LogicalMax, cap.BitSize),
                    unipolar: tp_model::normalise_unipolar(
                        raw,
                        cap.LogicalMin,
                        cap.LogicalMax,
                        cap.BitSize,
                    ),
                })
            })
            .collect();

        let mut pressed = Vec::new();
        for cap in buttons {
            let (min, max) = (cap.Anonymous.Range.UsageMin, cap.Anonymous.Range.UsageMax);
            let count = (max.saturating_sub(min) as usize) + 1;
            let mut usages = vec![0u16; count];
            let mut length = count as u32;
            if HidP_GetUsages(
                HidP_Input,
                cap.UsagePage,
                None,
                usages.as_mut_ptr(),
                &mut length,
                preparsed,
                report,
            )
            .is_ok()
            {
                usages.truncate(length as usize);
                // Present in the list means pressed. Expand to one flag per
                // declared button so the UI can draw a stable grid rather than
                // a list that changes length as buttons are held.
                pressed.extend((min..=max).map(|u| usages.contains(&u)));
            }
        }

        InputFrame {
            instance_path: path.to_string(),
            axes,
            buttons: pressed,
        }
    }
}
