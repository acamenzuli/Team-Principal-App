//! What graphics hardware is in this machine.
//!
//! DXGI for the adapters — name, vendor and video memory — and the registry
//! for the driver version, because DXGI does not carry one. Both are read;
//! neither is assumed. What the numbers *mean* is decided in
//! [`tp_model::voicelab::check_requirements`], where it is tested.
//!
//! Adapters are enumerated rather than "the first one": a laptop with an
//! Intel display adapter and an NVIDIA card must find the NVIDIA card, and a
//! machine with two cards should report the larger.

use tp_model::{GpuInfo, GpuVendor, VoiceLabRequirements};

/// Every display adapter, best first is not guaranteed — the caller decides.
#[cfg(windows)]
pub fn adapters() -> Vec<GpuInfo> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};

    let drivers = driver_versions();
    let mut out = Vec::new();

    // SAFETY: the factory and each adapter are COM objects released by their
    // Drop impls; the descriptor is a plain struct filled by the call.
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            tracing::warn!("DXGI is unavailable, so no GPU could be identified");
            return out;
        };
        let mut index = 0u32;
        while let Ok(adapter) = factory.EnumAdapters1(index) {
            index += 1;
            let Ok(desc) = adapter.GetDesc1() else {
                continue;
            };
            // The Microsoft Basic Render Driver is software, not a GPU.
            const DXGI_ADAPTER_FLAG_SOFTWARE: u32 = 2;
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE != 0 {
                continue;
            }
            let end = desc
                .Description
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(desc.Description.len());
            let name = String::from_utf16_lossy(&desc.Description[..end])
                .trim()
                .to_string();
            let vendor = GpuVendor::from_pci(desc.VendorId);
            let windows_driver = drivers
                .iter()
                .find(|(id, _)| *id == desc.DeviceId)
                .map(|(_, v)| v.clone());
            let driver = windows_driver
                .as_deref()
                .and_then(tp_model::nvidia_driver_from_windows)
                .filter(|_| vendor == GpuVendor::Nvidia);
            out.push(GpuInfo {
                name,
                vendor,
                vram_mb: (desc.DedicatedVideoMemory as u64) / (1024 * 1024),
                windows_driver,
                driver,
            });
        }
    }
    out
}

/// `(PCI device id, driver version)` for every display adapter the registry
/// knows about.
///
/// The display class key holds one subkey per adapter with `DriverVersion` and
/// `MatchingDeviceId` (`pci\ven_10de&dev_2684`), which is what ties a registry
/// entry to a DXGI adapter. WMI would answer this too and takes a second to
/// start; this takes microseconds and adds no dependency.
#[cfg(windows)]
fn driver_versions() -> Vec<(u32, String)> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
    };

    const CLASS_KEY: PCWSTR =
        w!("SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e968-e325-11ce-bfc1-08002be10318}");

    let mut out = Vec::new();
    // SAFETY: every key opened here is closed on every path; the buffers are
    // sized before each call and the returned lengths are honoured.
    unsafe {
        let mut class = HKEY::default();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, CLASS_KEY, None, KEY_READ, &mut class).is_err() {
            return out;
        }
        let mut index = 0u32;
        loop {
            let mut name = [0u16; 64];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(
                class,
                index,
                Some(windows::core::PWSTR(name.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
            .is_err()
            {
                break;
            }
            index += 1;
            let subkey_name: Vec<u16> = name[..len as usize]
                .iter()
                .copied()
                .chain(std::iter::once(0))
                .collect();
            let mut subkey = HKEY::default();
            if RegOpenKeyExW(
                class,
                PCWSTR(subkey_name.as_ptr()),
                None,
                KEY_READ,
                &mut subkey,
            )
            .is_err()
            {
                continue;
            }
            let version = read_string(subkey, w!("DriverVersion"));
            let matching = read_string(subkey, w!("MatchingDeviceId"));
            let _ = RegCloseKey(subkey);
            if let (Some(version), Some(device_id)) =
                (version, matching.as_deref().and_then(pci_device_id))
            {
                out.push((device_id, version));
            }
        }
        let _ = RegCloseKey(class);
    }
    out
}

#[cfg(windows)]
fn read_string(
    key: windows::Win32::System::Registry::HKEY,
    name: windows::core::PCWSTR,
) -> Option<String> {
    use windows::Win32::System::Registry::RegQueryValueExW;

    let mut buffer = [0u16; 128];
    let mut size = std::mem::size_of_val(&buffer) as u32;
    // SAFETY: the buffer outlives the call and `size` is its byte length.
    unsafe {
        RegQueryValueExW(
            key,
            name,
            None,
            None,
            Some(buffer.as_mut_ptr() as *mut u8),
            Some(&mut size),
        )
        .ok()
        .ok()?;
    }
    let chars = (size as usize / 2).min(buffer.len());
    let end = buffer[..chars]
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(chars);
    Some(String::from_utf16_lossy(&buffer[..end]))
}

/// `pci\ven_10de&dev_2684` → `0x2684`. Case-insensitive; anything else is None.
pub fn pci_device_id(matching_device_id: &str) -> Option<u32> {
    let lower = matching_device_id.to_ascii_lowercase();
    let start = lower.find("dev_")? + 4;
    let hex: String = lower[start..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    if hex.is_empty() {
        return None;
    }
    u32::from_str_radix(&hex, 16).ok()
}

#[cfg(not(windows))]
pub fn adapters() -> Vec<GpuInfo> {
    Vec::new()
}

/// The requirements verdict for this machine.
pub fn requirements() -> VoiceLabRequirements {
    tp_model::check_requirements(adapters())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matching_device_id_yields_its_pci_device() {
        assert_eq!(pci_device_id("pci\\ven_10de&dev_2684"), Some(0x2684));
        assert_eq!(
            pci_device_id("PCI\\VEN_10DE&DEV_2684&SUBSYS_1"),
            Some(0x2684)
        );
        assert_eq!(pci_device_id("pci\\ven_8086&dev_a780"), Some(0xa780));
        assert_eq!(pci_device_id("something else"), None);
        assert_eq!(pci_device_id("pci\\ven_10de&dev_"), None);
    }
}
