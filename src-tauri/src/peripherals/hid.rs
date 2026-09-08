//! HID enumeration — ground truth for "is this physically plugged in".
//!
//! `SetupDiGetClassDevs` over `GUID_DEVINTERFACE_HID`, then open each interface
//! and ask the device about itself. Opened with no access rights at all
//! (`dwDesiredAccess = 0`) and full sharing, which is enough for `HidD_*`
//! queries and — critically — cannot take a device away from a running game.

use tp_model::DeviceRef;

/// One device as the HID layer sees it.
pub struct HidDevice {
    pub vid: u16,
    pub pid: u16,
    pub serial: Option<String>,
    pub product: Option<String>,
    pub manufacturer: Option<String>,
    /// The stable device interface path. The only way to tell two identical
    /// un-serialled devices apart.
    pub instance_path: String,
    pub usage_page: u16,
    pub usage: u16,
}

impl HidDevice {
    /// Game controllers only: usage page 1, usage 4 (joystick) or 5 (gamepad).
    ///
    /// Without this the list is every keyboard, mouse, touchpad and laptop
    /// sensor on the machine, and the dropdown becomes unusable.
    pub fn is_game_controller(&self) -> bool {
        self.usage_page == 0x01 && matches!(self.usage, 0x04 | 0x05)
    }

    pub fn to_ref(&self, display_name: String) -> DeviceRef {
        DeviceRef {
            vid: self.vid,
            pid: self.pid,
            serial: self.serial.clone(),
            instance_path: Some(self.instance_path.clone()),
            display_name,
        }
    }
}

#[cfg(windows)]
pub fn enumerate() -> Vec<HidDevice> {
    use windows::core::PCWSTR;
    use windows::Win32::Devices::DeviceAndDriverInstallation::*;
    use windows::Win32::Devices::HumanInterfaceDevice::HidD_GetHidGuid;

    let mut out = Vec::new();

    // SAFETY: the enumeration handle is closed on every path below, and each
    // SetupDi call is given a buffer sized by its own preceding query.
    unsafe {
        let hid_guid = HidD_GetHidGuid();

        let Ok(set) = SetupDiGetClassDevsW(
            Some(&hid_guid),
            PCWSTR::null(),
            None,
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        ) else {
            tracing::warn!("SetupDiGetClassDevsW failed; no HID devices will be listed");
            return out;
        };

        let mut index = 0u32;
        loop {
            let mut interface = SP_DEVICE_INTERFACE_DATA {
                cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiEnumDeviceInterfaces(set, None, &hid_guid, index, &mut interface).is_err() {
                break; // ERROR_NO_MORE_ITEMS
            }
            index += 1;

            // First call sizes the buffer, second fills it.
            let mut needed = 0u32;
            let _ =
                SetupDiGetDeviceInterfaceDetailW(set, &interface, None, 0, Some(&mut needed), None);
            if needed == 0 {
                continue;
            }

            let mut buffer = vec![0u8; needed as usize];
            let detail = buffer.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;

            if SetupDiGetDeviceInterfaceDetailW(
                set,
                &interface,
                Some(detail),
                needed,
                Some(&mut needed),
                None,
            )
            .is_err()
            {
                continue;
            }

            let path = wide_from_ptr((*detail).DevicePath.as_ptr());
            if path.is_empty() {
                continue;
            }

            if let Some(device) = query(&path) {
                out.push(device);
            }
        }

        let _ = SetupDiDestroyDeviceInfoList(set);
    }

    out
}

/// Open one HID interface and read its attributes and strings.
#[cfg(windows)]
fn query(path: &str) -> Option<HidDevice> {
    use windows::core::HSTRING;
    use windows::Win32::Devices::HumanInterfaceDevice::{
        HidD_GetAttributes, HidD_GetManufacturerString, HidD_GetProductString,
        HidD_GetSerialNumberString, HIDD_ATTRIBUTES,
    };
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let wide = HSTRING::from(path);

    // SAFETY: opened with zero desired access and full sharing, so this can
    // never take a device away from a running game — which is the one thing
    // this app must never do to a peripheral.
    unsafe {
        let handle = CreateFileW(
            &wide,
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
        .ok()?;

        let mut attributes = HIDD_ATTRIBUTES {
            Size: std::mem::size_of::<HIDD_ATTRIBUTES>() as u32,
            ..Default::default()
        };
        let got_attributes = HidD_GetAttributes(handle, &mut attributes);

        // Usage page and usage come from the preparsed data, not the
        // attributes, and are what separates a wheel from a keyboard.
        let (usage_page, usage) = usages(handle).unwrap_or((0, 0));

        let device = got_attributes.then(|| HidDevice {
            vid: attributes.VendorID,
            pid: attributes.ProductID,
            serial: hid_string(handle, HidD_GetSerialNumberString),
            product: hid_string(handle, HidD_GetProductString),
            manufacturer: hid_string(handle, HidD_GetManufacturerString),
            instance_path: path.to_string(),
            usage_page,
            usage,
        });

        let _ = CloseHandle(handle);
        device
    }
}

#[cfg(windows)]
unsafe fn usages(handle: windows::Win32::Foundation::HANDLE) -> Option<(u16, u16)> {
    use windows::Win32::Devices::HumanInterfaceDevice::*;

    let mut preparsed = PHIDP_PREPARSED_DATA::default();
    if HidD_GetPreparsedData(handle, &mut preparsed) {
        let mut caps = HIDP_CAPS::default();
        let ok = HidP_GetCaps(preparsed, &mut caps).is_ok();
        let _ = HidD_FreePreparsedData(preparsed);
        if ok {
            return Some((caps.UsagePage, caps.Usage));
        }
    }
    None
}

/// The `HidD_Get*String` family all share a signature; read one into a buffer.
#[cfg(windows)]
unsafe fn hid_string(
    handle: windows::Win32::Foundation::HANDLE,
    f: unsafe fn(windows::Win32::Foundation::HANDLE, *mut core::ffi::c_void, u32) -> bool,
) -> Option<String> {
    // 126 wide characters is the HID maximum for these strings.
    let mut buf = [0u16; 128];
    if !f(
        handle,
        buf.as_mut_ptr().cast(),
        std::mem::size_of_val(&buf) as u32,
    ) {
        return None;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let s = String::from_utf16_lossy(&buf[..end]).trim().to_string();
    (!s.is_empty()).then_some(s)
}

#[cfg(not(windows))]
pub fn enumerate() -> Vec<HidDevice> {
    Vec::new()
}

#[cfg(not(windows))]
fn wide_from_ptr(_p: *const u16) -> String {
    String::new()
}

#[cfg(windows)]
unsafe fn wide_from_ptr(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *p.add(len) != 0 && len < 1024 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
}
