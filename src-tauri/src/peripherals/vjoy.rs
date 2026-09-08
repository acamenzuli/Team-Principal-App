//! vJoy detection.
//!
//! vJoy's devices appear as ordinary HID game controllers, which is the whole
//! point of it and also the problem: a vJoy device with nothing feeding it
//! enumerates perfectly, is listed by DirectInput, and reads dead centre on
//! every axis. A game will happily bind to it.
//!
//! ## Why this does not link vJoyInterface.dll
//!
//! The documented way to ask vJoy about itself is `vJoyEnabled` and
//! `GetVJDStatus` from `vJoyInterface.dll`. Shipping that DLL would mean
//! bundling a third-party binary in a product being sold, which this project
//! does not do.
//!
//! Instead vJoy is detected from what Windows already knows: its driver
//! service, its version in the registry, and its fixed VID/PID. That answers
//! "is vJoy installed, at what version, and is this device one of its
//! outputs". Whether a given device is *owned* by a feeder is then answered by
//! checking whether the feeder process the profile names is running — which is
//! the question the user actually has.
//!
//! If the user has vJoy installed, the DLL is already on their machine, and
//! loading it dynamically at runtime would give the exact `GetVJDStatus`
//! answer without bundling anything. That is a worthwhile refinement and is
//! deliberately left for when there is real hardware to verify it against.

use tp_model::{VJoyInfo, VJoyStatus};

/// vJoy's fixed identifiers. Stable across every version it has shipped.
pub const VJOY_VID: u16 = 0x1234;
pub const VJOY_PID: u16 = 0xBEAD;

pub fn is_vjoy_device(vid: u16, pid: u16) -> bool {
    vid == VJOY_VID && pid == VJOY_PID
}

#[derive(Debug, Clone, Default)]
pub struct VJoyProbe {
    pub installed: bool,
    pub driver_version: Option<String>,
}

impl VJoyProbe {
    pub fn info_for(&self, _vid: u16, _pid: u16) -> VJoyInfo {
        VJoyInfo {
            // Which of vJoy's 16 device slots this is cannot be told from the
            // HID layer alone; it needs the interface DLL or the feeder's own
            // configuration. Reported as unknown rather than guessed.
            device_id: 0,
            driver_version: self.driver_version.clone(),
            interface_version: None,
            status: if self.installed {
                VJoyStatus::Unknown
            } else {
                VJoyStatus::Missing
            },
            feeder_process: None,
            feeder_running: false,
        }
    }
}

#[cfg(windows)]
pub fn probe() -> VJoyProbe {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

    // The driver service key exists whenever the vJoy driver is installed.
    let service = HSTRING::from(r"SYSTEM\CurrentControlSet\Services\vjoy");
    let mut buf = [0u16; 256];
    let mut size = std::mem::size_of_val(&buf) as u32;

    // SAFETY: the key string outlives the call, and `size` matches the buffer.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(service.as_ptr()),
            w!("DisplayName"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };

    let installed = status == ERROR_SUCCESS;
    if !installed {
        tracing::debug!("no vJoy driver service found");
    }

    VJoyProbe {
        installed,
        driver_version: installed.then(|| {
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            String::from_utf16_lossy(&buf[..end]).trim().to_string()
        }),
    }
}

#[cfg(not(windows))]
pub fn probe() -> VJoyProbe {
    VJoyProbe::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vjoy_devices_are_recognised_by_their_fixed_ids() {
        assert!(is_vjoy_device(0x1234, 0xBEAD));
        assert!(!is_vjoy_device(0x1234, 0x0001));
        assert!(!is_vjoy_device(0x0EB7, 0xBEAD));
    }

    #[test]
    fn an_uninstalled_vjoy_reports_missing_rather_than_unknown() {
        let probe = VJoyProbe::default();
        assert_eq!(
            probe.info_for(VJOY_VID, VJOY_PID).status,
            VJoyStatus::Missing
        );
        assert!(!probe.info_for(VJOY_VID, VJOY_PID).feeder_running);
    }
}
