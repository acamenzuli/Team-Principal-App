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

use tauri::{AppHandle, Emitter};
pub use tp_model::{AxisReading, InputFrame, InputStatus};

/// The event carrying live values.
pub const EVENT: &str = "peripherals://input";

/// The event carrying what the monitor is *doing*.
///
/// Frames alone cannot distinguish "nothing has moved yet" from "this device
/// could never be opened" — and the second failed silently into the log, which
/// is how a Test button comes to look like it does nothing.
pub const STATUS_EVENT: &str = "peripherals://input-status";

/// Publish a status. A failure to publish is not worth failing over: the
/// window is usually gone by then.
fn publish(app: &AppHandle, status: InputStatus) {
    if let Err(e) = app.emit(STATUS_EVENT, &status) {
        tracing::debug!(error = %e, "could not publish a monitor status");
    }
}

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
    /// Which device this one reads. Stopping is matched against it — see
    /// [`crate::ipc::stop_input_monitor`].
    pub path: String,
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

/// What the device has, read synchronously so the caller holds it as a value.
#[cfg(windows)]
pub fn describe(instance_path: &str) -> crate::error::AppResult<InputStatus> {
    win::describe(instance_path)
}

#[cfg(not(windows))]
pub fn describe(_instance_path: &str) -> crate::error::AppResult<InputStatus> {
    Err(crate::error::AppError::Config(
        "live input is only available on Windows".into(),
    ))
}

#[cfg(windows)]
pub fn start(app: AppHandle, instance_path: String) -> Monitor {
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let path = instance_path.clone();

    publish(
        &app,
        InputStatus::Opening {
            instance_path: instance_path.clone(),
        },
    );

    std::thread::Builder::new()
        .name("input-monitor".into())
        .spawn(move || {
            if let Err(e) = win::run(&app, &instance_path, &thread_stop) {
                tracing::warn!(path = %instance_path, error = %e, "input monitor stopped");
                // The thread is where opening actually happens, so the command
                // that started it has already returned Ok. Without this, every
                // failure here is invisible to the person looking at the panel.
                publish(
                    &app,
                    InputStatus::Failed {
                        instance_path,
                        message: e.to_string(),
                    },
                );
            }
        })
        .expect("could not start the input monitor thread");

    Monitor { stop, path }
}

#[cfg(not(windows))]
pub fn start(app: AppHandle, instance_path: String) -> Monitor {
    // No HID layer to read. Say so rather than leaving a panel waiting for
    // frames that cannot arrive.
    let path = instance_path.clone();
    publish(
        &app,
        InputStatus::Failed {
            instance_path,
            message: "live input is only available on Windows".into(),
        },
    );
    Monitor {
        stop: Arc::new(AtomicBool::new(false)),
        path,
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

    use super::{publish, AxisReading, InputFrame, InputStatus, EVENT, MIN_INTERVAL_MS};
    use crate::error::{AppError, AppResult};

    /// Open the device, read its descriptor, close it, and say what it has.
    ///
    /// Synchronous and returned from the command rather than published as an
    /// event. The panel's *structure* — which axes, how many buttons, how many
    /// hats — is then a value the caller holds, not something it has to be
    /// told. An event can be missed; a return value cannot.
    pub fn describe(path: &str) -> AppResult<InputStatus> {
        // SAFETY: the handle and the preparsed data are released on every path.
        unsafe {
            let wide = HSTRING::from(path);
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
                operation: "open the device".into(),
                code: e.code().0 as u32,
                message: e.message(),
            })?;

            let result = describe_open(path, device);
            let _ = CloseHandle(device);
            result
        }
    }

    unsafe fn describe_open(path: &str, device: HANDLE) -> AppResult<InputStatus> {
        let mut preparsed = PHIDP_PREPARSED_DATA::default();
        if !HidD_GetPreparsedData(device, &mut preparsed) {
            return Err(AppError::Config(
                "this device reports no HID descriptor".into(),
            ));
        }

        let mut caps = HIDP_CAPS::default();
        let ok = HidP_GetCaps(preparsed, &mut caps).is_ok();
        let described = if ok && caps.InputReportByteLength > 0 {
            let (values, hats) = value_caps(preparsed, &caps);
            let buttons = button_caps(preparsed, &caps);
            Ok(listening(path, &values, &buttons, &hats))
        } else {
            Err(AppError::Config(
                "this device sends no input reports".into(),
            ))
        };

        let _ = HidD_FreePreparsedData(preparsed);
        described
    }

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

        let (value_caps, hat_caps) = value_caps(preparsed, &caps);
        let button_caps = button_caps(preparsed, &caps);

        // Publish the shape of the device before a single report arrives. The
        // panel can then draw every axis and button at rest, and mark them as
        // they are exercised — which is the whole point of a test panel. A
        // panel that only fills in as things move cannot tell you that a pedal
        // you have not pressed yet exists.
        publish(app, listening(path, &value_caps, &button_caps, &hat_caps));

        // A manual-reset event is not needed; the read completes or is
        // cancelled, and nothing else waits on this.
        let event = CreateEventW(None, false, false, None).map_err(|e| AppError::Win32 {
            operation: "create the monitor's wait event".into(),
            code: e.code().0 as u32,
            message: e.message(),
        })?;

        let mut report = vec![0u8; caps.InputReportByteLength as usize];
        let mut last_sent = Instant::now() - Duration::from_millis(MIN_INTERVAL_MS);

        // Ask for the current report once, so the panel opens showing where the
        // hardware actually is rather than an empty frame. Not every device
        // answers this — a wheel that reports only on change will refuse — and
        // that is fine: the loop below is the real source. Nothing is invented
        // when it fails; the panel simply shows no reading yet.
        if HidD_GetInputReport(
            device,
            report.as_mut_ptr() as *mut core::ffi::c_void,
            report.len() as u32,
        ) {
            let frame = decode(
                preparsed,
                &mut report,
                &value_caps,
                &button_caps,
                &hat_caps,
                path,
            );
            let _ = app.emit(EVENT, &frame);
        }

        let mut said_suspended = false;

        while !stop.load(Ordering::Relaxed) {
            // Suspended during a game session: sleep rather than exit, so
            // resuming does not need the device reopened.
            if super::is_suspended() {
                if !said_suspended {
                    said_suspended = true;
                    publish(
                        app,
                        InputStatus::Suspended {
                            instance_path: path.to_string(),
                        },
                    );
                }
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }

            if said_suspended {
                said_suspended = false;
                publish(app, listening(path, &value_caps, &button_caps, &hat_caps));
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
            let frame = decode(
                preparsed,
                &mut report[..n],
                &value_caps,
                &button_caps,
                &hat_caps,
                path,
            );
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

    /// The usage range a button cap covers.
    ///
    /// `HIDP_BUTTON_CAPS` holds a union, and `IsRange` says which half is
    /// valid. When it is false the cap describes **one** usage, and what sits
    /// where `UsageMax` would be is a reserved field — reading it anyway gives
    /// a count computed from garbage, which is how a shifter came to declare
    /// zero buttons. Most simple devices — shifters, button boxes, handbrakes
    /// — declare their buttons this way, so this is the common case rather
    /// than the exotic one.
    unsafe fn button_range(cap: &HIDP_BUTTON_CAPS) -> (u16, u16) {
        unsafe {
            if cap.IsRange {
                (cap.Anonymous.Range.UsageMin, cap.Anonymous.Range.UsageMax)
            } else {
                let usage = cap.Anonymous.NotRange.Usage;
                (usage, usage)
            }
        }
    }

    /// The usage a value cap describes. Same union, same rule.
    unsafe fn value_usage(cap: &HIDP_VALUE_CAPS) -> u16 {
        unsafe {
            if cap.IsRange {
                cap.Anonymous.Range.UsageMin
            } else {
                cap.Anonymous.NotRange.Usage
            }
        }
    }

    /// What the device declares it has, for the panel to draw at rest.
    ///
    /// Both the first announcement and the one after a session ends need this,
    /// and a panel that disagreed with itself between them would be worse than
    /// no panel.
    unsafe fn listening(
        path: &str,
        values: &[HIDP_VALUE_CAPS],
        buttons: &[HIDP_BUTTON_CAPS],
        hats: &[HIDP_VALUE_CAPS],
    ) -> InputStatus {
        InputStatus::Listening {
            instance_path: path.to_string(),
            axes: values
                .iter()
                // A closure inside an `unsafe fn` does not inherit its unsafe
                // context, so the union read is spelled out here.
                .map(|c| tp_model::axis_name(unsafe { value_usage(c) }).to_string())
                .collect(),
            buttons: buttons
                .iter()
                .map(|c| {
                    let (min, max) = unsafe { button_range(c) };
                    (max.saturating_sub(min) as u32) + 1
                })
                .sum(),
            hats: hats.len() as u32,
        }
    }

    /// Axes and hats, split.
    ///
    /// Both arrive as value caps and a hat used to be dropped on the floor by
    /// `is_axis`, which is why a wheel's POV switch appeared nowhere: it is not
    /// an axis, and it was not anything else either.
    unsafe fn value_caps(
        preparsed: PHIDP_PREPARSED_DATA,
        caps: &HIDP_CAPS,
    ) -> (Vec<HIDP_VALUE_CAPS>, Vec<HIDP_VALUE_CAPS>) {
        const HAT: u16 = 0x39;

        let mut count = caps.NumberInputValueCaps;
        if count == 0 {
            return (Vec::new(), Vec::new());
        }
        let mut out = vec![HIDP_VALUE_CAPS::default(); count as usize];
        if HidP_GetValueCaps(HidP_Input, out.as_mut_ptr(), &mut count, preparsed).is_err() {
            return (Vec::new(), Vec::new());
        }
        out.truncate(count as usize);

        let hats: Vec<HIDP_VALUE_CAPS> = out
            .iter()
            .filter(|c| c.UsagePage == 0x01 && unsafe { value_usage(c) } == HAT)
            .copied()
            .collect();
        out.retain(|c| tp_model::is_axis(c.UsagePage, unsafe { value_usage(c) }));
        (out, hats)
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
        hat_caps: &[HIDP_VALUE_CAPS],
        path: &str,
    ) -> InputFrame {
        let axes: Vec<AxisReading> = values
            .iter()
            .filter_map(|cap| {
                let usage = value_usage(cap);
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
            let (min, max) = button_range(cap);
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

        // Hats. Centred is `None` rather than a direction, so a POV nobody is
        // holding cannot light one up.
        let hats: Vec<Option<u16>> = hat_caps
            .iter()
            .map(|cap| {
                let usage = value_usage(cap);
                let mut raw = 0u32;
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
                tp_model::hat_degrees(raw, cap.LogicalMin, cap.LogicalMax)
            })
            .collect();

        InputFrame {
            instance_path: path.to_string(),
            axes,
            buttons: pressed,
            hats,
        }
    }
}
