//! Starting, watching and stopping the voice service.
//!
//! The service is a child process, not a library: PyTorch owns a CUDA
//! context and several gigabytes, and a crash in it must not take the
//! launcher with it. It is started when the tab needs it and stopped when it
//! has been idle for a while or when the app exits.
//!
//! Three things keep it contained:
//!
//! * **A Job Object.** The same one the launcher uses for session utilities
//!   (`launcher::job`), so if Team Principal dies — cleanly, by crash, or
//!   from Task Manager — Windows terminates the service with it. A stray
//!   Python process holding 4 GB of VRAM after a crash is exactly the kind
//!   of mess this app exists not to leave.
//! * **Loopback and a token.** It binds 127.0.0.1 on a port this side picks,
//!   and every request must carry a token passed in the environment — not on
//!   the command line, where any process on the machine can read it.
//! * **Read-back.** "Started" means `/health` answered, not that `spawn`
//!   returned. The announcement the service prints is checked against the
//!   port we asked for.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use tp_model::{ServiceInfo, ServiceState};

use crate::error::{AppError, AppResult};
use crate::launcher::job::JobObject;

/// Emitted whenever the service's state changes.
pub const SERVICE_EVENT: &str = "voicelab://service";

/// How long to wait for `/health` before giving up on a start.
const READY_TIMEOUT: Duration = Duration::from_secs(90);

pub struct Running {
    child: Child,
    pub port: u16,
    pub token: String,
    pub pid: u32,
    _job: JobObject,
    pub started_at: Instant,
}

#[derive(Default)]
pub struct Supervisor {
    inner: Mutex<Option<Running>>,
    /// Set while a session is in flight, so generation is paused rather than
    /// competing with the game for the GPU.
    paused_for_race: Mutex<bool>,
}

pub type SharedSupervisor = Arc<Supervisor>;

impl Supervisor {
    pub fn info(&self) -> ServiceInfo {
        let guard = self.inner.lock().ok();
        let running = guard.as_ref().and_then(|g| g.as_ref());
        let paused = self.paused_for_race.lock().map(|p| *p).unwrap_or(false);
        match running {
            Some(r) => ServiceInfo {
                state: ServiceState::Ready,
                base_url: Some(format!("http://127.0.0.1:{}", r.port)),
                token: Some(r.token.clone()),
                pid: Some(r.pid),
                message: "The voice service is running.".into(),
                paused_for_race: paused,
            },
            None => ServiceInfo {
                state: ServiceState::Stopped,
                base_url: None,
                token: None,
                pid: None,
                message: "The voice service is not running.".into(),
                paused_for_race: paused,
            },
        }
    }

    pub fn is_running(&self) -> bool {
        self.inner.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    /// Start it, or return what is already running.
    pub fn start(
        &self,
        app: &AppHandle,
        home: &Path,
        source: &Path,
        sounds: Option<&str>,
    ) -> AppResult<ServiceInfo> {
        {
            let guard = self.inner.lock().map_err(lock_poisoned)?;
            if guard.is_some() {
                drop(guard);
                return Ok(self.info());
            }
        }
        publish(app, starting());

        let python = super::module::python_exe(home);
        if !python.is_file() {
            return Err(AppError::Config(
                "the Voice Lab module is not installed yet. Open the Voice Lab tab and press Download.".into(),
            ));
        }

        let port = free_port()?;
        let token = new_token();
        let job = JobObject::new()?;

        let mut command = Command::new(&python);
        command
            .arg("-W")
            .arg("ignore")
            .arg("-m")
            .arg("voicelab")
            .arg("--port")
            .arg(port.to_string())
            .arg("--home")
            .arg(home);
        if let Some(sounds) = sounds.filter(|s| !s.trim().is_empty()) {
            command.arg("--sounds").arg(sounds);
        }
        command
            // The token travels in the environment: a command line is
            // readable by any process on the machine, an environment block
            // is not.
            .env("VOICELAB_TOKEN", &token)
            .env("PYTHONUNBUFFERED", "1")
            .current_dir(source)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command
            .spawn()
            .map_err(|e| AppError::Config(format!("could not start the voice service: {e}")))?;
        let pid = child.id();
        if let Err(e) = job.adopt(pid) {
            // Not fatal — the service still works — but it would outlive a
            // crash, so it is worth saying out loud in the log.
            tracing::warn!(error = %e, pid, "the voice service is not in the job object");
        }

        // Its two streams go to the log. The stdout reader also watches for
        // the one announcement line, which is how a port of 0 would be
        // learned and how this one is confirmed.
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let announced: Arc<Mutex<Option<(u16, u32)>>> = Arc::new(Mutex::new(None));
        if let Some(stdout) = stdout {
            let announced = announced.clone();
            std::thread::Builder::new()
                .name("voicelab-stdout".into())
                .spawn(move || {
                    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                        if let Some(found) = tp_model::parse_service_announcement(&line) {
                            if let Ok(mut slot) = announced.lock() {
                                *slot = Some(found);
                            }
                            tracing::info!(target: "voicelab", port = found.0, pid = found.1, "service announced itself");
                        } else if !line.trim().is_empty() {
                            tracing::info!(target: "voicelab", "{line}");
                        }
                    }
                })
                .ok();
        }
        if let Some(stderr) = stderr {
            std::thread::Builder::new()
                .name("voicelab-stderr".into())
                .spawn(move || {
                    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                        if !line.trim().is_empty() {
                            tracing::info!(target: "voicelab", "{line}");
                        }
                    }
                })
                .ok();
        }

        // Ready means /health answered, not that spawn returned.
        let deadline = Instant::now() + READY_TIMEOUT;
        let mut ready = false;
        while Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Err(AppError::Config(format!(
                        "the voice service stopped immediately ({status}). The Voice Lab log says why."
                    )));
                }
                Ok(None) => {}
                Err(e) => tracing::warn!(error = %e, "could not check on the voice service"),
            }
            if health(port, &token).is_ok() {
                ready = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        if !ready {
            let _ = child.kill();
            return Err(AppError::Config(
                "the voice service did not answer within ninety seconds. The Voice Lab log has the details.".into(),
            ));
        }
        if let Ok(slot) = announced.lock() {
            if let Some((announced_port, _)) = *slot {
                if announced_port != port {
                    let _ = child.kill();
                    return Err(AppError::Config(format!(
                        "the voice service started on port {announced_port}, not the {port} it was given"
                    )));
                }
            }
        }

        let running = Running {
            child,
            port,
            token,
            pid,
            _job: job,
            started_at: Instant::now(),
        };
        *self.inner.lock().map_err(lock_poisoned)? = Some(running);
        let info = self.info();
        publish(app, info.clone());
        tracing::info!(port, pid, "voice service ready");
        Ok(info)
    }

    /// Ask it to exit, and make sure it did.
    pub fn stop(&self, app: &AppHandle) -> ServiceInfo {
        let taken = self.inner.lock().ok().and_then(|mut g| g.take());
        if let Some(mut running) = taken {
            // Politely first: it cancels a running job and tears down CUDA.
            let _ = post(running.port, &running.token, "/shutdown");
            let deadline = Instant::now() + Duration::from_secs(8);
            loop {
                match running.child.try_wait() {
                    Ok(Some(_)) => break,
                    _ if Instant::now() >= deadline => {
                        let _ = running.child.kill();
                        break;
                    }
                    _ => std::thread::sleep(Duration::from_millis(100)),
                }
            }
            let _ = running.child.wait();
            tracing::info!(pid = running.pid, "voice service stopped");
        }
        let info = self.info();
        publish(app, info.clone());
        info
    }

    /// Stop it if it has been idle longer than this. Returns whether it did.
    pub fn stop_if_idle(&self, app: &AppHandle, idle_minutes: u32) -> bool {
        if idle_minutes == 0 {
            return false;
        }
        let Some((port, token)) = self
            .inner
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|r| (r.port, r.token.clone())))
        else {
            return false;
        };
        match health(port, &token) {
            Ok(body) => {
                let idle = body
                    .get("idle_seconds")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                if idle >= f64::from(idle_minutes) * 60.0 {
                    tracing::info!(idle, "the voice service has been idle; stopping it");
                    self.stop(app);
                    return true;
                }
                false
            }
            // Unreachable but still recorded as running: it died. Clear it,
            // rather than leaving the tab talking to nothing.
            Err(_) => {
                tracing::warn!("the voice service stopped answering; clearing it");
                self.stop(app);
                true
            }
        }
    }

    /// A session started or ended. Pauses or resumes generation.
    pub fn set_racing(&self, app: &AppHandle, racing: bool) {
        let changed = {
            let mut guard = match self.paused_for_race.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let changed = *guard != racing;
            *guard = racing;
            changed
        };
        if !changed {
            return;
        }
        let Some((port, token)) = self
            .inner
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|r| (r.port, r.token.clone())))
        else {
            return;
        };
        let path = if racing {
            "/jobs/pause"
        } else {
            "/jobs/resume"
        };
        match post(port, &token, path) {
            Ok(_) => tracing::info!(
                racing,
                "voice generation {}",
                if racing {
                    "paused for a session"
                } else {
                    "resumed"
                }
            ),
            Err(e) => {
                tracing::warn!(error = %e, "could not {} voice generation", if racing { "pause" } else { "resume" })
            }
        }
        publish(app, self.info());
    }
}

fn starting() -> ServiceInfo {
    ServiceInfo {
        state: ServiceState::Starting,
        base_url: None,
        token: None,
        pid: None,
        message: "Starting the voice service…".into(),
        paused_for_race: false,
    }
}

fn publish(app: &AppHandle, info: ServiceInfo) {
    if let Err(e) = app.emit(SERVICE_EVENT, &info) {
        tracing::warn!(error = %e, "could not publish the voice service state");
    }
}

fn lock_poisoned<T>(_: T) -> AppError {
    AppError::Config("the voice service state is unreadable; restart the app".into())
}

/// Ask the operating system for a free port, then let go of it.
///
/// There is a window between releasing it and the service binding it. The
/// alternative — passing 0 and reading the port the service announces —
/// trades that window for a slower start and a second failure mode, and the
/// announcement is checked anyway.
fn free_port() -> AppResult<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| AppError::Io(format!("could not find a free port: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| AppError::Io(format!("could not read the port: {e}")))?
        .port();
    drop(listener);
    Ok(port)
}

fn new_token() -> String {
    let mut bytes = [0u8; 24];
    getrandom(&mut bytes);
    tp_model::make_token(bytes)
}

#[cfg(windows)]
fn getrandom(buffer: &mut [u8]) {
    use windows::Win32::Security::Cryptography::{
        BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    };

    // SAFETY: the buffer is valid for its own length for the call.
    unsafe {
        let status = BCryptGenRandom(None, buffer, BCRYPT_USE_SYSTEM_PREFERRED_RNG);
        if status.is_err() {
            // Never silently fall back to something guessable.
            panic!("the system random number generator failed: {status:?}");
        }
    }
}

#[cfg(not(windows))]
fn getrandom(buffer: &mut [u8]) {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Development only; the shipped build is Windows and uses BCrypt.
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x5DEECE66D);
    let mut state = seed ^ (std::process::id() as u64) << 32;
    for byte in buffer.iter_mut() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *byte = (state >> 33) as u8;
    }
}

fn health(port: u16, token: &str) -> AppResult<serde_json::Value> {
    get(port, token, "/health")
}

fn get(port: u16, token: &str, path: &str) -> AppResult<serde_json::Value> {
    request(port, token, path, false)
}

fn post(port: u16, token: &str, path: &str) -> AppResult<serde_json::Value> {
    request(port, token, path, true)
}

fn request(port: u16, token: &str, path: &str, post: bool) -> AppResult<serde_json::Value> {
    let url = format!("http://127.0.0.1:{port}{path}");
    tauri::async_runtime::block_on(async {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            // Loopback only; a proxy has no business here and a corporate
            // one would break the call.
            .no_proxy()
            .build()
            .map_err(|e| AppError::Io(format!("could not build a client: {e}")))?;
        let builder = if post {
            client.post(&url)
        } else {
            client.get(&url)
        };
        let response = builder
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| AppError::Io(format!("the voice service did not answer: {e}")))?
            .error_for_status()
            .map_err(|e| AppError::Io(format!("the voice service refused {path}: {e}")))?;
        response
            .json::<serde_json::Value>()
            .await
            .map_err(|e| AppError::Io(format!("the voice service sent something unreadable: {e}")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_port_is_free() {
        let port = free_port().unwrap();
        assert!(port > 1024);
        // Nothing is holding it, so it can be bound.
        let listener = std::net::TcpListener::bind(("127.0.0.1", port));
        assert!(listener.is_ok(), "port {port} was not actually free");
    }

    #[test]
    fn tokens_are_long_unguessable_and_header_safe() {
        let a = new_token();
        let b = new_token();
        assert_eq!(a.len(), 24);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(a, b, "two tokens in a row must not match");
    }
}
