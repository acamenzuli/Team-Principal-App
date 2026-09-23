//! Voice Lab: the IPC types, and every decision that does not need Windows.
//!
//! The Voice Lab turns a few minutes of the owner's voice into a CrewChief
//! voice pack. The heavy lifting is a Python service; the app's part is to
//! decide whether this machine can run it, to build the optional module on
//! the user's machine from a lockfile, and to start and stop the service.
//! The decisions in all of that — what a driver version means, whether a
//! GPU qualifies, which uv release to fetch and what its hash must be, how
//! the settings look — live here, where they run on every platform.
//! `src-tauri/src/voicelab/` is the platform call itself.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ------------------------------------------------------------------ settings

/// Voice Lab settings. Part of `Preferences`, and every field has a default
/// so an existing preferences file loads unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct VoiceLabSettings {
    /// The user asked for the module; the tab should offer to build it and
    /// start the service when opened.
    #[serde(default)]
    pub enabled: bool,
    /// Where the module lives. `None` means the default under `%LOCALAPPDATA%`.
    #[serde(default)]
    pub home: Option<String>,
    /// CrewChief's sounds folder, when the default is not where it is.
    #[serde(default)]
    pub sounds_folder: Option<String>,
    /// Baked into phrases that address the driver.
    #[serde(default)]
    pub your_name: Option<String>,
    /// How long the service may sit idle before the app stops it.
    #[serde(default = "default_idle_minutes")]
    pub idle_stop_minutes: u32,
    /// The engine new packs use.
    #[serde(default = "default_engine")]
    pub engine: String,
    /// GPU workers. `None` sizes the pool from free VRAM.
    #[serde(default)]
    pub workers: Option<u32>,
    /// The optional pit-radio effect, per pack.
    #[serde(default)]
    pub radio_effect: bool,
    /// Clips per phrase in a full pack.
    #[serde(default = "default_variants")]
    pub variants: u32,
}

fn default_idle_minutes() -> u32 {
    10
}

fn default_engine() -> String {
    "chatterbox".into()
}

fn default_variants() -> u32 {
    2
}

impl Default for VoiceLabSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            home: None,
            sounds_folder: None,
            your_name: None,
            idle_stop_minutes: default_idle_minutes(),
            engine: default_engine(),
            workers: None,
            radio_effect: false,
            variants: default_variants(),
        }
    }
}

// ----------------------------------------------------------------------- GPU

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

impl GpuVendor {
    /// PCI vendor ids, which is what DXGI reports.
    pub fn from_pci(vendor_id: u32) -> Self {
        match vendor_id {
            0x10DE => GpuVendor::Nvidia,
            0x1002 | 0x1022 => GpuVendor::Amd,
            0x8086 => GpuVendor::Intel,
            _ => GpuVendor::Other,
        }
    }
}

/// One display adapter, as the platform reported it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub name: String,
    pub vendor: GpuVendor,
    /// `number`, not `bigint`, for the reason given on `WindowTarget::timeout_ms`:
    /// no quantity here is near the precision limit and a bigint needs a cast
    /// at every call site.
    #[ts(type = "number")]
    pub vram_mb: u64,
    /// The Windows driver version, `32.0.16.1664` style, when it could be read.
    pub windows_driver: Option<String>,
    /// NVIDIA's own version number derived from it, `616.64` style.
    pub driver: Option<String>,
}

/// What the Voice Lab needs and whether this machine has it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct VoiceLabRequirements {
    pub ok: bool,
    /// The adapter that would be used, if any qualifies — else the best there is.
    pub gpu: Option<GpuInfo>,
    /// Every adapter seen, for the requirements screen.
    pub gpus: Vec<GpuInfo>,
    #[ts(type = "number")]
    pub min_vram_mb: u64,
    pub min_driver: String,
    /// Each unmet requirement, as a sentence.
    pub problems: Vec<String>,
}

/// Chatterbox needs about 3.5 GB and the transcriber about 2; one worker fits
/// in 8 GB with room to breathe, which is the smallest RTX card sold today.
pub const MIN_VRAM_MB: u64 = 8 * 1024 - 512;
/// The oldest NVIDIA driver PyTorch's CUDA 12.6 wheels run on, on Windows.
pub const MIN_NVIDIA_DRIVER: (u32, u32) = (528, 33);

/// NVIDIA's version number from the Windows one: `32.0.16.1664` → `616.64`.
///
/// The last digit of the third part and the whole fourth part are the
/// NVIDIA version with the point removed. That is how NVIDIA has packed it
/// for over a decade, and `nvidia-smi` on the development machine agrees:
/// `32.0.16.1664` there is driver 616.64.
pub fn nvidia_driver_from_windows(windows: &str) -> Option<String> {
    let parts: Vec<&str> = windows.trim().split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let third = parts[2];
    let fourth = parts[3];
    if fourth.len() != 4 || !fourth.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let last = third.chars().last().filter(|c| c.is_ascii_digit())?;
    let digits = format!("{last}{fourth}");
    Some(format!("{}.{}", &digits[..3], &digits[3..]))
}

/// `616.64` → `(616, 64)`.
pub fn parse_driver(version: &str) -> Option<(u32, u32)> {
    let (major, minor) = version.trim().split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// Decide from what the platform reported.
pub fn check_requirements(gpus: Vec<GpuInfo>) -> VoiceLabRequirements {
    let min_driver = format!("{}.{}", MIN_NVIDIA_DRIVER.0, MIN_NVIDIA_DRIVER.1);
    let mut problems = Vec::new();

    let nvidia: Vec<&GpuInfo> = gpus
        .iter()
        .filter(|g| g.vendor == GpuVendor::Nvidia)
        .collect();
    let best = nvidia
        .iter()
        .max_by_key(|g| g.vram_mb)
        .copied()
        .or_else(|| gpus.iter().max_by_key(|g| g.vram_mb));

    match best {
        None => problems.push("No graphics adapter was found.".to_string()),
        Some(gpu) if gpu.vendor != GpuVendor::Nvidia => problems.push(format!(
            "The Voice Lab needs an NVIDIA GPU with CUDA. This machine has {} ({}).",
            gpu.name,
            vendor_name(gpu.vendor)
        )),
        Some(gpu) => {
            if gpu.vram_mb < MIN_VRAM_MB {
                problems.push(format!(
                    "{} has {} of video memory; the Voice Lab needs at least 8 GB.",
                    gpu.name,
                    format_gb(gpu.vram_mb)
                ));
            }
            match gpu.driver.as_deref().and_then(parse_driver) {
                Some(version) if version < MIN_NVIDIA_DRIVER => problems.push(format!(
                    "The NVIDIA driver is {}.{}; the Voice Lab needs {} or newer. Update it from NVIDIA's site or GeForce Experience.",
                    version.0, version.1, min_driver
                )),
                Some(_) => {}
                None => problems.push(format!(
                    "The NVIDIA driver version could not be read; {} or newer is needed.",
                    min_driver
                )),
            }
        }
    }

    VoiceLabRequirements {
        ok: problems.is_empty(),
        gpu: best.cloned(),
        gpus,
        min_vram_mb: MIN_VRAM_MB,
        min_driver,
        problems,
    }
}

fn vendor_name(v: GpuVendor) -> &'static str {
    match v {
        GpuVendor::Nvidia => "NVIDIA",
        GpuVendor::Amd => "AMD",
        GpuVendor::Intel => "Intel",
        GpuVendor::Other => "another vendor",
    }
}

pub fn format_gb(mb: u64) -> String {
    let gb = mb as f64 / 1024.0;
    if gb >= 10.0 {
        format!("{gb:.0} GB")
    } else {
        format!("{gb:.1} GB")
    }
}

// -------------------------------------------------------------------- module

/// The uv release the module is built with. Pinned, with the SHA-256 of the
/// Windows archive from the release's own `.sha256` file, so what runs on
/// the user's machine is exactly what was tested — a newer uv with a
/// different opinion about lockfiles cannot arrive by surprise.
pub const UV_VERSION: &str = "0.12.17";
pub const UV_ARCHIVE: &str = "uv-x86_64-pc-windows-msvc.zip";
pub const UV_SHA256: &str = "a252121d5b59398fcb137c6ea448176459a44010f33f67e0072305a637119ca7";

pub fn uv_download_url() -> String {
    format!("https://github.com/astral-sh/uv/releases/download/{UV_VERSION}/{UV_ARCHIVE}")
}

/// The Python the service runs on. uv installs it; the version is pinned
/// because the lockfile was resolved against it.
pub const PYTHON_VERSION: &str = "3.11";

/// Where the module is, and what state it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ModuleStage {
    /// Nothing built yet.
    NotInstalled,
    /// Fetching uv from its GitHub release.
    DownloadingUv,
    /// uv fetching CPython.
    InstallingPython,
    /// uv building the environment from the lockfile.
    SyncingEnvironment,
    /// Everything present and verified by read-back.
    Ready,
    /// Something failed; `ModuleStatus::error` says what.
    Failed,
    /// Deleting the module.
    Removing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ModuleStatus {
    pub home: String,
    pub stage: ModuleStage,
    /// Whether each piece is present, as checked on disk — never remembered.
    pub uv_present: bool,
    pub python_present: bool,
    pub env_present: bool,
    /// Bytes under the home folder, so the tab can say what Remove reclaims.
    #[ts(type = "number")]
    pub bytes_on_disk: u64,
    /// A line for the progress panel.
    pub message: String,
    pub error: Option<String>,
    /// The last lines of the installer's output, newest last.
    pub log: Vec<String>,
    /// Whether an install or removal is running now.
    pub busy: bool,
}

// ------------------------------------------------------------------- service

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Stopped,
    Starting,
    Ready,
    Failed,
}

/// What the frontend needs to talk to the service, and whether it can.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInfo {
    pub state: ServiceState,
    /// `http://127.0.0.1:<port>` while the service is up.
    pub base_url: Option<String>,
    /// The per-session token every request must carry.
    pub token: Option<String>,
    pub pid: Option<u32>,
    pub message: String,
    /// Whether generation is paused because a session is running.
    pub paused_for_race: bool,
}

/// The dedicated-GPU rule: generation pauses while a session is in one of
/// these states, and resumes when it leaves them.
pub fn race_pauses_generation(state: crate::ReadyState) -> bool {
    matches!(
        state,
        crate::ReadyState::Launching | crate::ReadyState::Racing | crate::ReadyState::TearingDown
    )
}

// ------------------------------------------------------------------- helpers

/// Parse the one line the service prints on stdout when it starts.
pub fn parse_service_announcement(line: &str) -> Option<(u16, u32)> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let inner = value.get("voicelab")?;
    let port = inner.get("port")?.as_u64()? as u16;
    let pid = inner.get("pid")?.as_u64()? as u32;
    Some((port, pid))
}

/// A token that is unguessable and safe in a header.
pub fn make_token(random: [u8; 24]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    random
        .iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_versions_unpack_the_way_nvidia_smi_reports_them() {
        assert_eq!(
            nvidia_driver_from_windows("32.0.16.1664").as_deref(),
            Some("616.64")
        );
        assert_eq!(
            nvidia_driver_from_windows("31.0.15.5152").as_deref(),
            Some("551.52")
        );
        assert_eq!(
            nvidia_driver_from_windows("30.0.14.7168").as_deref(),
            Some("471.68")
        );
        assert_eq!(nvidia_driver_from_windows("32.0.16"), None);
        assert_eq!(nvidia_driver_from_windows("32.0.16.166"), None);
        assert_eq!(nvidia_driver_from_windows("garbage"), None);
    }

    #[test]
    fn driver_versions_compare_numerically() {
        assert_eq!(parse_driver("616.64"), Some((616, 64)));
        assert!(parse_driver("616.64").unwrap() > MIN_NVIDIA_DRIVER);
        assert!(parse_driver("528.02").unwrap() < MIN_NVIDIA_DRIVER);
        assert!(parse_driver("1000.1").unwrap() > parse_driver("999.99").unwrap());
        assert_eq!(parse_driver("nope"), None);
    }

    fn gpu(name: &str, vendor: GpuVendor, vram_mb: u64, driver: Option<&str>) -> GpuInfo {
        GpuInfo {
            name: name.into(),
            vendor,
            vram_mb,
            windows_driver: None,
            driver: driver.map(String::from),
        }
    }

    #[test]
    fn a_4090_on_a_current_driver_qualifies() {
        let r = check_requirements(vec![gpu(
            "NVIDIA GeForce RTX 4090",
            GpuVendor::Nvidia,
            24564,
            Some("616.64"),
        )]);
        assert!(r.ok, "{:?}", r.problems);
        assert_eq!(r.gpu.unwrap().name, "NVIDIA GeForce RTX 4090");
    }

    #[test]
    fn the_nvidia_card_is_chosen_over_a_bigger_integrated_one() {
        let r = check_requirements(vec![
            gpu("AMD Radeon Graphics", GpuVendor::Amd, 32768, None),
            gpu(
                "NVIDIA GeForce RTX 3070",
                GpuVendor::Nvidia,
                8192,
                Some("560.94"),
            ),
        ]);
        assert!(r.ok, "{:?}", r.problems);
        assert_eq!(r.gpu.unwrap().vendor, GpuVendor::Nvidia);
        assert_eq!(r.gpus.len(), 2);
    }

    #[test]
    fn each_unmet_requirement_is_a_sentence_the_user_can_act_on() {
        let r = check_requirements(vec![gpu(
            "NVIDIA GeForce GTX 1650",
            GpuVendor::Nvidia,
            4096,
            Some("472.12"),
        )]);
        assert!(!r.ok);
        assert_eq!(r.problems.len(), 2);
        assert!(r.problems[0].contains("4.0 GB"), "{}", r.problems[0]);
        assert!(r.problems[1].contains("472.12"), "{}", r.problems[1]);
        assert!(r.problems[1].contains("528.33"), "{}", r.problems[1]);

        let amd = check_requirements(vec![gpu("AMD Radeon RX 7900", GpuVendor::Amd, 24576, None)]);
        assert!(!amd.ok);
        assert!(amd.problems[0].contains("NVIDIA"), "{}", amd.problems[0]);

        let none = check_requirements(vec![]);
        assert!(!none.ok);
        assert!(none.gpu.is_none());
    }

    #[test]
    fn an_unreadable_driver_is_reported_not_assumed() {
        let r = check_requirements(vec![gpu(
            "NVIDIA GeForce RTX 4080",
            GpuVendor::Nvidia,
            16384,
            None,
        )]);
        assert!(!r.ok);
        assert!(
            r.problems[0].contains("could not be read"),
            "{}",
            r.problems[0]
        );
    }

    #[test]
    fn vram_formats_for_humans() {
        assert_eq!(format_gb(24564), "24 GB");
        assert_eq!(format_gb(8192), "8.0 GB");
        assert_eq!(format_gb(4096), "4.0 GB");
    }

    #[test]
    fn the_uv_pin_is_a_full_release_url_and_a_real_hash() {
        assert_eq!(
            uv_download_url(),
            "https://github.com/astral-sh/uv/releases/download/0.12.17/uv-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(UV_SHA256.len(), 64);
        assert!(UV_SHA256.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn the_service_announcement_parses_and_junk_does_not() {
        assert_eq!(
            parse_service_announcement(
                r#"{"voicelab": {"port": 47321, "pid": 4242, "version": "0.2.0"}}"#
            ),
            Some((47321, 4242))
        );
        assert_eq!(parse_service_announcement("Loading model..."), None);
        assert_eq!(parse_service_announcement(r#"{"other": 1}"#), None);
    }

    #[test]
    fn tokens_are_header_safe_and_long() {
        let t = make_token([7; 24]);
        assert_eq!(t.len(), 24);
        assert!(t.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn only_a_running_session_pauses_generation() {
        use crate::ReadyState::*;
        assert!(race_pauses_generation(Launching));
        assert!(race_pauses_generation(Racing));
        assert!(race_pauses_generation(TearingDown));
        for s in [
            Running,
            Ready,
            ReadyWithWarnings,
            Blocked,
            LaunchFailed,
            Done,
        ] {
            assert!(!race_pauses_generation(s), "{s:?}");
        }
    }

    #[test]
    fn settings_default_and_load_from_an_empty_object() {
        let s: VoiceLabSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, VoiceLabSettings::default());
        assert_eq!(s.idle_stop_minutes, 10);
        assert_eq!(s.engine, "chatterbox");
        assert_eq!(s.variants, 2);
        assert!(!s.enabled);
    }
}
