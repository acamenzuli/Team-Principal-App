//! Reading the EDID blob out of the registry.
//!
//! The key derivation lives in `tp_edid::source`, where it is pure string work
//! and covered by tests that run on any machine. This module is only the
//! Windows call.

use tp_edid::source::registry_key_for;

/// Read the EDID blob for a monitor. `None` when the key or value is absent,
/// which happens with some virtual and remote-desktop displays.
#[cfg(windows)]
pub fn read_edid(monitor_device_path: &str) -> Option<Vec<u8>> {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_BINARY};

    let key = HSTRING::from(registry_key_for(monitor_device_path)?);

    // EDID is a 128-byte base block plus extension blocks; 512 covers every
    // panel in practice, and the call reports the true size regardless.
    let mut buf = vec![0u8; 512];
    let mut size = buf.len() as u32;

    // SAFETY: `key` outlives the call. `size` is set to the buffer's length and
    // the API writes at most that many bytes, updating `size` to what it wrote.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key.as_ptr()),
            w!("EDID"),
            RRF_RT_REG_BINARY,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };

    if status != ERROR_SUCCESS {
        tracing::debug!(path = %monitor_device_path, ?status, "no EDID in the registry");
        return None;
    }

    buf.truncate(size as usize);
    Some(buf)
}

#[cfg(not(windows))]
pub fn read_edid(_monitor_device_path: &str) -> Option<Vec<u8>> {
    None
}
