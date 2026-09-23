//! Building the optional module on the user's machine.
//!
//! The installer ships none of this: Python, PyTorch and the models are
//! several gigabytes and most people will never open the tab. What ships is
//! the service's own source (a few hundred kilobytes, as Tauri resources)
//! and a pinned uv version with its hash. Pressing *Download* here fetches
//! uv from its GitHub release, has uv install CPython, and has uv build the
//! environment from `uv.lock` — the same lockfile the development machine
//! resolved, so what runs here is what was tested.
//!
//! **Nothing is trusted to have worked because a command exited zero.** uv
//! has a known Windows bug where `python install` succeeds and still returns
//! a non-zero status ([astral-sh/uv#19622]), and a half-extracted archive
//! looks exactly like a good one to an exit code. Every stage is verified by
//! what it produced: the archive by its SHA-256, the interpreter by running
//! it, the environment by importing torch and asking CUDA whether it is
//! there. This is the same rule the rest of the app applies to Win32.
//!
//! [astral-sh/uv#19622]: https://github.com/astral-sh/uv/issues/19622

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use tp_model::{ModuleStage, ModuleStatus, PYTHON_VERSION, UV_SHA256, UV_VERSION};

use crate::error::{AppError, AppResult};

/// Where the module lives. The setting wins; otherwise `%LOCALAPPDATA%`.
///
/// `LOCALAPPDATA`, not the app's usual `APPDATA`: this is gigabytes of
/// machine-specific binaries, and a roaming profile must not try to carry it.
pub fn home(configured: Option<&str>) -> PathBuf {
    if let Some(dir) = configured.map(str::trim).filter(|s| !s.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(env) = std::env::var_os("TP_VOICELAB_HOME") {
        return PathBuf::from(env);
    }
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Team Principal")
            .join("voicelab")
    }
    #[cfg(not(windows))]
    {
        crate::logging::app_data_dir().join("voicelab")
    }
}

pub fn uv_exe(home: &Path) -> PathBuf {
    home.join("bin").join("uv.exe")
}

pub fn python_exe(home: &Path) -> PathBuf {
    home.join("env").join("Scripts").join("python.exe")
}

/// Where the service's source is, so `uv sync` has a project to build.
///
/// Bundled as a Tauri resource in a shipped build; in development the copy in
/// the repository is used, so an edit is picked up without a rebuild.
pub fn service_source(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;

    if let Ok(dir) = app.path().resource_dir() {
        let bundled = dir.join("voice-service");
        if bundled.join("pyproject.toml").is_file() {
            return Some(bundled);
        }
    }
    let mut here = std::env::current_dir().ok()?;
    for _ in 0..4 {
        let candidate = here.join("voice-service");
        if candidate.join("pyproject.toml").is_file() {
            return Some(candidate);
        }
        here = here.parent()?.to_path_buf();
    }
    None
}

/// What is on disk, checked rather than remembered.
pub fn status(
    home: &Path,
    busy: bool,
    message: String,
    error: Option<String>,
    log: Vec<String>,
) -> ModuleStatus {
    let uv_present = uv_exe(home).is_file();
    let python_present = managed_python(home).is_some();
    let env_present = python_exe(home).is_file();
    let stage = if busy {
        ModuleStage::SyncingEnvironment
    } else if error.is_some() {
        ModuleStage::Failed
    } else if env_present && python_present && uv_present {
        ModuleStage::Ready
    } else {
        ModuleStage::NotInstalled
    };
    ModuleStatus {
        home: home.display().to_string(),
        stage,
        uv_present,
        python_present,
        env_present,
        bytes_on_disk: directory_size(home),
        message,
        error,
        log,
        busy,
    }
}

/// The interpreter uv installed, if it is really there.
fn managed_python(home: &Path) -> Option<PathBuf> {
    let root = home.join("python");
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let exe = entry.path().join("python.exe");
        if exe.is_file() {
            return Some(exe);
        }
    }
    None
}

fn directory_size(path: &Path) -> u64 {
    fn walk(path: &Path, total: &mut u64) {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            match entry.file_type() {
                Ok(t) if t.is_dir() => walk(&entry.path(), total),
                Ok(t) if t.is_file() => {
                    if let Ok(meta) = entry.metadata() {
                        *total += meta.len();
                    }
                }
                _ => {}
            }
        }
    }
    let mut total = 0;
    walk(path, &mut total);
    total
}

/// Progress, shared with whatever is watching.
#[derive(Default)]
pub struct Install {
    pub busy: bool,
    pub message: String,
    pub error: Option<String>,
    pub log: Vec<String>,
}

impl Install {
    fn say(&mut self, line: impl Into<String>) {
        let line = line.into();
        tracing::info!(target: "voicelab", "{line}");
        self.message = line.clone();
        self.log.push(line);
        // The panel shows the tail; the log file has all of it.
        if self.log.len() > 200 {
            self.log.drain(..self.log.len() - 200);
        }
    }
}

pub type SharedInstall = Arc<Mutex<Install>>;

/// Build the module. Long-running; call it on a thread.
///
/// Every stage is skipped when its result is already present, so pressing
/// Download again after a failed or interrupted attempt continues rather
/// than starting over — uv's own cache makes the same true of a partly
/// downloaded environment.
pub fn install(home: &Path, source: &Path, progress: SharedInstall, report: impl Fn(ModuleStatus)) {
    let say = |text: &str| {
        if let Ok(mut p) = progress.lock() {
            p.say(text);
        }
        report(status(
            home,
            true,
            text.to_string(),
            None,
            progress.lock().map(|p| p.log.clone()).unwrap_or_default(),
        ));
    };

    let result = (|| -> AppResult<()> {
        std::fs::create_dir_all(home)?;
        if !uv_exe(home).is_file() {
            say("Downloading uv…");
            fetch_uv(home)?;
        }
        verify_uv(home)?;

        if managed_python(home).is_none() {
            say(&format!("Installing Python {PYTHON_VERSION}…"));
            let _ = run_uv(
                home,
                &[
                    "python",
                    "install",
                    PYTHON_VERSION,
                    "--no-bin",
                    "--no-registry",
                ],
            );
        }
        // Verified by read-back, never by the exit status: uv#19622 returns
        // non-zero from a python install that worked.
        let python = managed_python(home).ok_or_else(|| {
            AppError::Config(
                "Python could not be installed. The Voice Lab log has the details.".into(),
            )
        })?;
        let version = run(&python, &["--version"])?;
        if !version.contains(PYTHON_VERSION) {
            return Err(AppError::Config(format!(
                "the installed Python reports {version:?}, not {PYTHON_VERSION}"
            )));
        }

        say("Building the Python environment — this is the big download…");
        run_uv_in(
            home,
            source,
            &["sync", "--frozen", "--python", PYTHON_VERSION, "--no-dev"],
        )?;

        say("Checking the GPU is usable from Python…");
        let python = python_exe(home);
        if !python.is_file() {
            return Err(AppError::Config(
                "the environment was not created. The Voice Lab log has the details.".into(),
            ));
        }
        let check = run(
            &python,
            &[
                "-c",
                "import torch;print('cuda' if torch.cuda.is_available() else 'cpu')",
            ],
        )?;
        if !check.contains("cuda") {
            return Err(AppError::Config(
                "Python is installed but CUDA is not available to it. Check the NVIDIA driver, then press Download again."
                    .into(),
            ));
        }
        say("Ready.");
        Ok(())
    })();

    if let Ok(mut p) = progress.lock() {
        p.busy = false;
        match &result {
            Ok(()) => p.message = "Ready.".into(),
            Err(e) => {
                p.error = Some(e.to_string());
                p.say(format!("Failed: {e}"));
            }
        }
    }
    let (message, error, log) = progress
        .lock()
        .map(|p| (p.message.clone(), p.error.clone(), p.log.clone()))
        .unwrap_or_default();
    report(status(home, false, message, error, log));
}

/// Download the pinned uv release and check its hash before it ever runs.
fn fetch_uv(home: &Path) -> AppResult<()> {
    let url = tp_model::uv_download_url();
    let bytes = download(&url)?;
    let digest = sha256_hex(&bytes);
    if digest != UV_SHA256 {
        return Err(AppError::Config(format!(
            "the uv download does not match its published checksum (expected {UV_SHA256}, got {digest}). \
             Nothing was installed."
        )));
    }
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin)?;
    extract_uv(&bytes, &bin)?;
    if !uv_exe(home).is_file() {
        return Err(AppError::Config(
            "the uv archive did not contain uv.exe".into(),
        ));
    }
    tracing::info!(%url, version = UV_VERSION, "uv installed");
    Ok(())
}

fn verify_uv(home: &Path) -> AppResult<()> {
    let reported = run(&uv_exe(home), &["--version"])?;
    if !reported.contains(UV_VERSION) {
        return Err(AppError::Config(format!(
            "uv reports {reported:?}, but this build expects {UV_VERSION}"
        )));
    }
    Ok(())
}

fn download(url: &str) -> AppResult<Vec<u8>> {
    // Blocking on purpose: this runs on its own thread, and a runtime here
    // would be a second async stack inside an app that has none.
    let response = tauri::async_runtime::block_on(async {
        reqwest::get(url)
            .await
            .map_err(|e| AppError::Io(format!("could not reach {url}: {e}")))?
            .error_for_status()
            .map_err(|e| AppError::Io(format!("{url} returned {e}")))?
            .bytes()
            .await
            .map_err(|e| AppError::Io(format!("the download from {url} was interrupted: {e}")))
    })?;
    Ok(response.to_vec())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn extract_uv(archive: &[u8], into: &Path) -> AppResult<()> {
    let reader = std::io::Cursor::new(archive);
    let mut zip = zip::ZipArchive::new(reader)
        .map_err(|e| AppError::Io(format!("the uv download is not a zip file: {e}")))?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::Io(format!("could not read the uv archive: {e}")))?;
        // Names come from a signed release, but a zip entry is still
        // attacker-controlled data in general; take the basename only, so
        // nothing can escape the folder.
        let Some(name) = entry.name().rsplit(['/', '\\']).next().map(str::to_string) else {
            continue;
        };
        if name.is_empty() || !name.ends_with(".exe") {
            continue;
        }
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| AppError::Io(format!("could not extract {name}: {e}")))?;
        std::fs::write(into.join(name), bytes)?;
    }
    Ok(())
}

/// Run uv with the module's own home as every cache and install root, so
/// nothing lands in the user's profile, PATH or registry.
fn uv_command(home: &Path) -> Command {
    let mut cmd = Command::new(uv_exe(home));
    cmd.env("UV_PYTHON_INSTALL_DIR", home.join("python"))
        .env("UV_CACHE_DIR", home.join("cache").join("uv"))
        .env("UV_PROJECT_ENVIRONMENT", home.join("env"))
        // uv would otherwise drop a python3.11.exe shim into ~/.local/bin and
        // register the interpreter under HKCU\Software\Python. Neither belongs
        // to this app, and removing the module must leave nothing behind.
        .env("UV_PYTHON_INSTALL_BIN", "0")
        .env("UV_PYTHON_INSTALL_REGISTRY", "0")
        .env("UV_NO_PROGRESS", "1");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn run_uv(home: &Path, args: &[&str]) -> AppResult<String> {
    let mut cmd = uv_command(home);
    cmd.args(args);
    output(cmd, &format!("uv {}", args.join(" ")))
}

fn run_uv_in(home: &Path, dir: &Path, args: &[&str]) -> AppResult<String> {
    let mut cmd = uv_command(home);
    cmd.current_dir(dir).args(args);
    output(cmd, &format!("uv {}", args.join(" ")))
}

fn run(exe: &Path, args: &[&str]) -> AppResult<String> {
    let mut cmd = Command::new(exe);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    output(cmd, &format!("{} {}", exe.display(), args.join(" ")))
}

fn output(mut cmd: Command, what: &str) -> AppResult<String> {
    let out = cmd
        .output()
        .map_err(|e| AppError::Io(format!("could not run {what}: {e}")))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !out.status.success() {
        tracing::warn!(target: "voicelab", %what, code = out.status.code(), %stderr, "command failed");
        return Err(AppError::Config(format!(
            "{what} failed: {}",
            first_useful_line(&stderr).unwrap_or_else(|| "no output".into())
        )));
    }
    tracing::debug!(target: "voicelab", %what, "ok");
    Ok(if stdout.is_empty() { stderr } else { stdout })
}

/// The first line of a tool's error output that says something.
fn first_useful_line(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("warning:"))
        .map(|l| l.trim_start_matches("error: ").to_string())
}

/// Delete everything the module installed, keeping the user's own work.
///
/// Voices and packs are recordings and generated audio — hours of somebody's
/// evening — so they stay unless they are asked for separately.
pub fn remove(home: &Path, keep_work: bool) -> AppResult<u64> {
    let before = directory_size(home);
    for name in ["bin", "python", "cache", "env", "models"] {
        let path = home.join(name);
        if path.exists() {
            std::fs::remove_dir_all(&path)
                .map_err(|e| AppError::Io(format!("could not remove {}: {e}", path.display())))?;
        }
    }
    if !keep_work {
        for name in ["voices", "packs", "exports", "backups"] {
            let path = home.join(name);
            if path.exists() {
                let _ = std::fs::remove_dir_all(&path);
            }
        }
    }
    Ok(before.saturating_sub(directory_size(home)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_setting_wins_over_the_default() {
        let chosen = home(Some("D:\\Voice Lab"));
        assert_eq!(chosen, PathBuf::from("D:\\Voice Lab"));
        assert_eq!(home(Some("   ")), home(None), "blank is not a choice");
    }

    #[test]
    fn a_tool_error_is_reported_as_a_sentence_not_a_wall() {
        let stderr = "warning: something noisy\nerror: No such file or directory (os error 2)\n  cause: whatever";
        assert_eq!(
            first_useful_line(stderr).as_deref(),
            Some("No such file or directory (os error 2)")
        );
        assert_eq!(first_useful_line("   \n\n"), None);
    }

    #[test]
    fn the_hash_is_computed_the_way_the_release_publishes_it() {
        // The empty string's SHA-256, so the encoding is what everyone means
        // by it rather than something byte-swapped.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn extraction_takes_the_basename_so_nothing_escapes_the_folder() {
        let dir = std::env::temp_dir().join(format!("tp-uv-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut buffer = Vec::new();
        {
            use std::io::Write;
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buffer));
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            zip.start_file("..\\..\\evil\\uv.exe", options).unwrap();
            zip.write_all(b"binary").unwrap();
            zip.start_file("notes.txt", options).unwrap();
            zip.write_all(b"ignored").unwrap();
            zip.finish().unwrap();
        }
        extract_uv(&buffer, &dir).unwrap();
        assert!(dir.join("uv.exe").is_file(), "the exe lands in the folder");
        assert!(
            !dir.join("notes.txt").exists(),
            "only executables are taken"
        );
        assert!(
            !dir.parent().unwrap().join("evil").exists(),
            "nothing escaped"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
