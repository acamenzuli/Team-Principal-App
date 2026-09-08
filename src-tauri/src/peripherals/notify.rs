//! Device arrival and removal, from the OS.
//!
//! `CM_Register_Notification` with a device-interface filter, rather than
//! `WM_DEVICECHANGE`. The difference matters: `WM_DEVICECHANGE` needs a window
//! to deliver to, so an app without one has to create a hidden message-only
//! window and pump it. This has no such requirement — the callback arrives on a
//! thread the OS supplies.
//!
//! The callback does the least possible work: it pokes the watch thread and
//! returns. Enumerating from inside a device notification callback is a good
//! way to deadlock against the very subsystem that is mid-change.

#![cfg(windows)]

use std::sync::mpsc::Sender;

use windows::Win32::Devices::DeviceAndDriverInstallation::*;
use windows::Win32::Devices::HumanInterfaceDevice::HidD_GetHidGuid;
use windows::Win32::Foundation::ERROR_SUCCESS;

/// Holds the registration alive. Dropping it unregisters.
pub struct Notification {
    handle: HCMNOTIFICATION,
    /// Kept so the sender the callback points at outlives the registration.
    _wake: Box<Sender<()>>,
}

// SAFETY: HCMNOTIFICATION is an opaque OS handle, valid from any thread, and
// the boxed sender is only read by the callback.
unsafe impl Send for Notification {}

impl Drop for Notification {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            // SAFETY: the handle came from CM_Register_Notification and is
            // unregistered exactly once, here.
            unsafe {
                let _ = CM_Unregister_Notification(self.handle);
            }
        }
    }
}

pub fn register(wake: Sender<()>) -> Option<Notification> {
    let wake = Box::new(wake);

    let mut filter = CM_NOTIFY_FILTER {
        cbSize: std::mem::size_of::<CM_NOTIFY_FILTER>() as u32,
        FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE,
        ..Default::default()
    };
    // SAFETY: writing the interface GUID into the union member that
    // CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE selects.
    unsafe {
        filter.u.DeviceInterface.ClassGuid = HidD_GetHidGuid();
    }

    let mut handle = HCMNOTIFICATION::default();

    // SAFETY: `filter` is fully initialised above; the context pointer is the
    // boxed sender, which the returned Notification keeps alive for as long as
    // the registration exists.
    let status = unsafe {
        CM_Register_Notification(
            &filter,
            Some(&*wake as *const Sender<()> as *const core::ffi::c_void),
            Some(on_device_change),
            &mut handle,
        )
    };

    if status != CR_SUCCESS {
        tracing::warn!(
            ?status,
            "CM_Register_Notification failed; falling back to polling only"
        );
        return None;
    }

    tracing::info!("watching for HID device arrival and removal");
    Some(Notification {
        handle,
        _wake: wake,
    })
}

/// Do as little as possible here and return.
unsafe extern "system" fn on_device_change(
    _notify: HCMNOTIFICATION,
    context: *const core::ffi::c_void,
    action: CM_NOTIFY_ACTION,
    _data: *const CM_NOTIFY_EVENT_DATA,
    _size: u32,
) -> u32 {
    if matches!(
        action,
        CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL | CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL
    ) {
        // SAFETY: `context` is the boxed Sender registered above, which the
        // Notification keeps alive for the lifetime of the registration.
        let wake = unsafe { &*(context as *const Sender<()>) };
        // A full queue means a rescan is already pending, which is the same
        // outcome. Never block here.
        let _ = wake.send(());
    }
    ERROR_SUCCESS.0
}
