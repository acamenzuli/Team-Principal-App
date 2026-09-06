//! A fake game, for testing the window watchdog.
//!
//! Real sims reset their own window when the render device initialises, when a
//! session loads, and on alt-tab. We cannot test that reliably against a real
//! title, so this binary reproduces the behaviour on demand:
//!
//! ```text
//! tp-fakegame --resize-after 3000 --resize-every 5000
//! ```
//!
//! Opens a normal window with a caption, then after `--resize-after` ms moves
//! and resizes itself back to a default rect — and keeps doing so every
//! `--resize-every` ms if asked. If the watchdog is working, the window snaps
//! back to the profile's geometry each time.

fn main() {
    let args = Args::parse(std::env::args().skip(1));
    #[cfg(windows)]
    windows_impl::run(args);
    #[cfg(not(windows))]
    {
        // Building on Linux is useful for `cargo check`; running is not.
        eprintln!(
            "tp-fakegame is a Windows-only test harness (parsed args: {args:?}). \
             Build and run it on the target machine."
        );
        std::process::exit(2);
    }
}

#[derive(Debug, Clone)]
pub struct Args {
    pub title: String,
    pub width: i32,
    pub height: i32,
    pub resize_after_ms: u64,
    pub resize_every_ms: Option<u64>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            title: "Fake Sim (tp-fakegame)".into(),
            width: 1280,
            height: 720,
            resize_after_ms: 3000,
            resize_every_ms: None,
        }
    }
}

impl Args {
    fn parse(it: impl Iterator<Item = String>) -> Args {
        let mut a = Args::default();
        let v: Vec<String> = it.collect();
        let mut i = 0;
        while i < v.len() {
            let next = |i: usize| v.get(i + 1).cloned().unwrap_or_default();
            match v[i].as_str() {
                "--title" => a.title = next(i),
                "--width" => a.width = next(i).parse().unwrap_or(a.width),
                "--height" => a.height = next(i).parse().unwrap_or(a.height),
                "--resize-after" => {
                    a.resize_after_ms = next(i).parse().unwrap_or(a.resize_after_ms)
                }
                "--resize-every" => a.resize_every_ms = next(i).parse().ok(),
                _ => {}
            }
            i += 1;
        }
        a
    }
}

#[cfg(windows)]
mod windows_impl {
    use std::sync::OnceLock;

    use super::Args;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// The window procedure is a bare `extern "system"` fn with no state of its
    /// own, so the parsed arguments live here. `OnceLock` rather than
    /// `static mut` because the latter is undefined behaviour the moment two
    /// threads touch it, and Windows may call a wndproc from more than one.
    static ARGS: OnceLock<Args> = OnceLock::new();

    pub fn run(args: Args) {
        let _ = ARGS.set(args.clone());

        // SAFETY: a standard register-class / create-window / pump-messages
        // sequence. Every handle is checked, and the wndproc below touches only
        // the OnceLock and its own arguments.
        unsafe {
            let instance = GetModuleHandleW(None).expect("module handle");
            let class = w!("TpFakeGameWindow");

            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0 {
                panic!("RegisterClassW failed: {}", std::io::Error::last_os_error());
            }

            let title: Vec<u16> = args
                .title
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                PCWSTR(title.as_ptr()),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                args.width,
                args.height,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .expect("create window");

            // Timer 1 fires once, timer 2 repeats. Split so that "resets itself
            // once when the render device initialises" and "fights the window
            // manager forever" are independently testable against the watchdog.
            SetTimer(Some(hwnd), TIMER_ONCE, args.resize_after_ms as u32, None);
            if let Some(every) = args.resize_every_ms {
                SetTimer(Some(hwnd), TIMER_REPEAT, every as u32, None);
            }

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    const TIMER_ONCE: usize = 1;
    const TIMER_REPEAT: usize = 2;

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_TIMER => {
                let args = ARGS.get().cloned().unwrap_or_default();
                if wparam.0 == TIMER_ONCE {
                    let _ = unsafe { KillTimer(Some(hwnd), TIMER_ONCE) };
                }
                eprintln!("[tp-fakegame] resetting my own window, the way sims do");
                let _ = unsafe {
                    SetWindowPos(
                        hwnd,
                        None,
                        100,
                        100,
                        args.width,
                        args.height,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    )
                };
                LRESULT(0)
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }
}
