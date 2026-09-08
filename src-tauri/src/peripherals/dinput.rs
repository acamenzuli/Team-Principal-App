//! DirectInput enumeration — what *games* actually see.
//!
//! The HID layer knows what is plugged in. DirectInput knows what a game will
//! find, and in what order. Those are different answers, and the gap between
//! them is a real state a device passes through on every hotplug: present,
//! usable by Windows, and invisible to the sim.
//!
//! The enumeration **order** matters more than it looks. Games store bindings
//! against a device's slot and instance GUID, and the order can change after a
//! reboot or after something unrelated is unplugged — at which point the
//! shifter is bound to the handbrake and nothing on screen says why. Capturing
//! the order is what makes that detectable before a session rather than on the
//! grid.
//!
//! Nothing here acquires a device. Enumeration is read-only; acquiring is what
//! could take input away from a game, and this module never does it.

/// One device as DirectInput lists it.
pub struct DiDevice {
    /// Position in the enumeration. This is what games bind against.
    pub slot: u32,
    /// Identifies this device on this machine — the stronger drift signal,
    /// because a slot can shift merely because something else was unplugged.
    pub instance_guid: String,
    /// Decoded from the product GUID, so matching to a HID device needs no
    /// name comparison. Names are often identical across a pair of pedals.
    pub vid: u16,
    pub pid: u16,
    pub product_name: Option<String>,
}

#[cfg(windows)]
pub fn enumerate() -> Vec<DiDevice> {
    use windows::core::{Interface, GUID, HRESULT};
    use windows::Win32::Devices::HumanInterfaceDevice::{
        DirectInput8Create, IDirectInput8W, DI8DEVCLASS_GAMECTRL, DIDEVICEINSTANCEW,
        DIEDFL_ATTACHEDONLY, DIRECTINPUT_VERSION,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;

    // Collected by the callback below. DirectInput's enumeration API is a
    // callback with a user pointer, so the list has to live somewhere the
    // callback can reach.
    struct Collected(Vec<DiDevice>);

    unsafe extern "system" fn on_device(
        instance: *mut DIDEVICEINSTANCEW,
        context: *mut core::ffi::c_void,
    ) -> windows::core::BOOL {
        // SAFETY: `context` is the &mut Collected passed to EnumDevices below,
        // which outlives the enumeration; `instance` is valid for this call.
        let collected = unsafe { &mut *(context as *mut Collected) };
        let instance = unsafe { &*instance };

        let product = instance.guidProduct;
        let Some((vid, pid)) = tp_model::vid_pid_from_product_guid(
            product.data1,
            product.data2,
            product.data3,
            product.data4,
        ) else {
            // Not a HID-derived product GUID. Listing it without a VID/PID
            // would leave a row that can never be matched to real hardware.
            return true.into();
        };

        let guid = instance.guidInstance;
        collected.0.push(DiDevice {
            slot: collected.0.len() as u32,
            instance_guid: tp_model::format_guid(guid.data1, guid.data2, guid.data3, guid.data4),
            vid,
            pid,
            product_name: wide_field(&instance.tszProductName),
        });
        true.into()
    }

    let mut collected = Collected(Vec::new());

    // SAFETY: the DirectInput8 object is released when `di` drops; the context
    // pointer refers to `collected`, which outlives the synchronous call.
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return Vec::new();
        };

        let mut raw: Option<IDirectInput8W> = None;
        let hr: HRESULT = DirectInput8Create(
            module.into(),
            DIRECTINPUT_VERSION,
            &IDirectInput8W::IID as *const GUID,
            &mut raw as *mut _ as *mut _,
            None,
        )
        .map(|()| HRESULT(0))
        .unwrap_or_else(|e| e.code());

        let Some(di) = raw else {
            tracing::warn!(
                ?hr,
                "DirectInput8Create failed; ordering will be unavailable"
            );
            return Vec::new();
        };

        if let Err(e) = di.EnumDevices(
            DI8DEVCLASS_GAMECTRL,
            Some(on_device),
            &mut collected as *mut _ as *mut core::ffi::c_void,
            DIEDFL_ATTACHEDONLY,
        ) {
            tracing::warn!(error = %e, "DirectInput EnumDevices failed");
        }
    }

    collected.0
}

/// A fixed-size WCHAR field from a DirectInput struct.
#[cfg(windows)]
fn wide_field(buf: &[u16]) -> Option<String> {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let s = String::from_utf16_lossy(&buf[..end]).trim().to_string();
    (!s.is_empty()).then_some(s)
}

#[cfg(not(windows))]
pub fn enumerate() -> Vec<DiDevice> {
    Vec::new()
}
