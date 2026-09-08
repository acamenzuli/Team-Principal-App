//! Finding the game's window.
//!
//! `EnumWindows` gives every top-level window; the *choosing* is pure logic in
//! `tp_model::window`, where it is tested against splash screens, tool windows
//! and the grandchild-process case that Steam produces on every launch.
//!
//! Polling is used rather than `SetWinEventHook` for now. The hook is the
//! better mechanism and is worth having, but it requires a message loop on the
//! thread that installs it, and getting that wrong produces a launcher that
//! misses the window entirely. Polling every 250 ms for up to the profile's
//! timeout is unglamorous, correct, and costs nothing measurable.

use tp_model::{WindowCandidate, WindowMatch, WindowTarget};

use crate::error::{AppError, AppResult};

/// Poll until the target window appears, or the profile's timeout expires.
pub fn find_target(
    target: &WindowTarget,
    pid: Option<u32>,
    deadline: std::time::Instant,
) -> AppResult<WindowMatch> {
    let mut looked_at = 0usize;
    loop {
        let candidates = enumerate_windows();
        looked_at = looked_at.max(candidates.len());

        if let Some(found) = tp_model::pick_target(&candidates, target, pid) {
            tracing::info!(
                hwnd = found.window.hwnd,
                title = %found.window.title,
                matched = ?found.matched_by,
                "found the game window"
            );
            return Ok(found);
        }

        if std::time::Instant::now() >= deadline {
            // Say what was searched, not just that it failed. "No window
            // matched" with no further detail is the least useful possible
            // message when a game has in fact opened.
            return Err(AppError::Config(format!(
                "no window matched after searching {looked_at} open windows. \
                 Looked for: {}. If the game is running, its window may open later than \
                 expected, or under a different executable name.",
                describe(target)
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

fn describe(target: &WindowTarget) -> String {
    let mut parts = Vec::new();
    if let Some(exe) = &target.exe_name {
        parts.push(format!("executable {exe}"));
    }
    if let Some(class) = &target.window_class {
        parts.push(format!("window class {class}"));
    }
    if let Some(title) = &target.title_regex {
        parts.push(format!("title {title}"));
    }
    parts.push(format!(
        "at least {}x{}",
        target.min_size.0, target.min_size.1
    ));
    parts.join(", ")
}

#[cfg(windows)]
pub fn enumerate_windows() -> Vec<WindowCandidate> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT, TRUE};
    use windows::Win32::UI::WindowsAndMessaging::*;

    let mut found: Vec<WindowCandidate> = Vec::new();

    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` is the &mut Vec passed to EnumWindows below, which
        // outlives the synchronous enumeration.
        let out = unsafe { &mut *(lparam.0 as *mut Vec<WindowCandidate>) };

        // SAFETY: `hwnd` is valid for the duration of this callback.
        unsafe {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return TRUE;
            }

            let mut pid = 0u32;
            let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));

            out.push(WindowCandidate {
                hwnd: hwnd.0 as u64,
                pid,
                exe_name: crate::window::find::exe_name_of(pid),
                class_name: text(|buf| GetClassNameW(hwnd, buf)),
                title: text(|buf| GetWindowTextW(hwnd, buf)),
                rect: tp_model::PixelRect {
                    x: rect.left,
                    y: rect.top,
                    width: (rect.right - rect.left).max(0) as u32,
                    height: (rect.bottom - rect.top).max(0) as u32,
                },
                style: GetWindowLongPtrW(hwnd, GWL_STYLE) as u32,
                ex_style: GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32,
                visible: IsWindowVisible(hwnd).as_bool(),
            });
        }
        TRUE
    }

    // SAFETY: the callback only writes to `found`, which outlives the call.
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&mut found as *mut _ as isize));
    }
    found
}

/// Read a fixed-size WCHAR field via a Win32 getter.
#[cfg(windows)]
fn text(mut get: impl FnMut(&mut [u16]) -> i32) -> String {
    let mut buf = [0u16; 512];
    let len = get(&mut buf).max(0) as usize;
    String::from_utf16_lossy(&buf[..len.min(buf.len())])
        .trim()
        .to_string()
}

/// The executable behind a PID.
///
/// This is what makes the grandchild case work: Steam hands off to a process
/// the launcher never started, so its PID is useless and the executable name is
/// the only thing left that identifies the game.
#[cfg(windows)]
pub fn exe_name_of(pid: u32) -> Option<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    if pid == 0 {
        return None;
    }

    // SAFETY: the handle is closed on every path. QUERY_LIMITED_INFORMATION is
    // the least privilege that answers this, and works across integrity levels
    // where the fuller rights would be refused.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(handle);

        ok.then(|| {
            String::from_utf16_lossy(&buf[..len as usize])
                .rsplit('\\')
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .filter(|s| !s.is_empty())
    }
}

#[cfg(not(windows))]
pub fn enumerate_windows() -> Vec<WindowCandidate> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn exe_name_of(_pid: u32) -> Option<String> {
    None
}
