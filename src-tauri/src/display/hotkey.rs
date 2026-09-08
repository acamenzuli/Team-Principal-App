//! The panic hotkey: Ctrl+Alt+Shift+R puts the displays back.
//!
//! The last line of defence. Everything else on the display path assumes the
//! user can see something; this assumes they cannot. It has to work when the
//! app window is behind a game, when the WebView has stopped painting, and when
//! the only screen showing anything is the wrong one.
//!
//! Two Win32 facts shape it:
//!
//! * **`RegisterHotKey` binds to a thread, not to a window**, and delivers
//!   `WM_HOTKEY` to that thread's message queue. So this owns a thread with its
//!   own loop rather than hooking Tauri's — a hotkey registered on the UI
//!   thread stops being delivered whenever that thread is busy, which is
//!   precisely when it is needed.
//! * **Registration can fail because something else already owns the
//!   combination.** That is not an error to swallow: a panic hotkey the user
//!   believes in and that does nothing is worse than none, so a failure is
//!   logged loudly and reported in the UI.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::AppHandle;

/// The combination. Chosen because no sim binds it and Windows does not claim
/// it: Ctrl+Alt+Shift is three modifiers, which nothing hits by accident on a
/// wheel or a button box.
pub const DESCRIPTION: &str = "Ctrl+Alt+Shift+R";

/// Whether the hotkey is actually registered. Read by the UI, so it can say
/// "the panic hotkey is not available" rather than implying a safety net that
/// is not there.
#[derive(Clone, Default)]
pub struct HotkeyState(pub Arc<AtomicBool>);

impl HotkeyState {
    pub fn registered(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Register the hotkey and run its message loop until the app exits.
///
/// `on_press` runs on this thread. It is expected to be quick — it reverts a
/// display change — and a slow handler would delay a second press, which is
/// acceptable for a panic key that is idempotent anyway.
#[cfg(windows)]
pub fn start(app: AppHandle, on_press: impl Fn(&AppHandle) + Send + 'static) -> HotkeyState {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    let state = HotkeyState::default();
    let flag = state.0.clone();

    std::thread::Builder::new()
        .name("panic-hotkey".into())
        .spawn(move || {
            const ID: i32 = 0xB0_1D;

            // SAFETY: a NULL hwnd registers against this thread's queue, which
            // is the loop below. The id is ours alone within this thread.
            let ok = unsafe {
                RegisterHotKey(
                    None,
                    ID,
                    MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
                    b'R' as u32,
                )
            };
            if ok.is_err() {
                // Loud, because the user must not believe in a net that is not
                // there. The UI reads the flag and says so too.
                tracing::error!(
                    "could not register {DESCRIPTION}; something else on this \
                     machine already owns it. The panic hotkey is unavailable."
                );
                return;
            }
            flag.store(true, Ordering::Relaxed);
            tracing::info!("panic hotkey {DESCRIPTION} registered");

            let mut msg = MSG::default();
            // SAFETY: GetMessageW blocks on this thread's queue and writes one
            // message. A NULL hwnd means "any window on this thread", which
            // includes the thread-bound hotkey messages.
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                if msg.message == WM_HOTKEY && msg.wParam.0 as i32 == ID {
                    tracing::warn!("panic hotkey pressed");
                    on_press(&app);
                }
            }

            // SAFETY: unregistering the id this thread registered.
            unsafe {
                let _ = UnregisterHotKey(None, ID);
            }
            flag.store(false, Ordering::Relaxed);
        })
        .ok();

    state
}

#[cfg(not(windows))]
pub fn start(_app: AppHandle, _on_press: impl Fn(&AppHandle) + Send + 'static) -> HotkeyState {
    HotkeyState::default()
}
