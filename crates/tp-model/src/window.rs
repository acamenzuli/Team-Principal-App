//! Choosing the right window, and deciding what to do to it.
//!
//! Two things here are easy to get wrong and impossible to debug by eye:
//!
//! 1. **Which window is the game.** Sims open splash screens, loader windows,
//!    invisible message-only windows and IME helpers. "The first window that
//!    appears with the right PID" is wrong for most titles, and the failure is
//!    a launcher that resizes a 200x80 splash and then does nothing.
//! 2. **Client area versus outer window.** These differ by the frame, and on a
//!    triple setup a few pixels of horizontal error puts the horizon seam in
//!    the wrong place on every screen.
//!
//! Both are decided here, in pure logic, so they are tested rather than
//! discovered on a grid.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{PixelRect, WindowTarget};

// Win32 style bits, named here so this crate — and its tests — need no
// dependency on `windows`. Values are from winuser.h and are fixed forever.
pub const WS_BORDER: u32 = 0x0080_0000;
pub const WS_DLGFRAME: u32 = 0x0040_0000;
pub const WS_CAPTION: u32 = WS_BORDER | WS_DLGFRAME;
pub const WS_SYSMENU: u32 = 0x0008_0000;
pub const WS_THICKFRAME: u32 = 0x0004_0000;
pub const WS_MINIMIZEBOX: u32 = 0x0002_0000;
pub const WS_MAXIMIZEBOX: u32 = 0x0001_0000;
pub const WS_VISIBLE: u32 = 0x1000_0000;
pub const WS_CHILD: u32 = 0x4000_0000;
pub const WS_POPUP: u32 = 0x8000_0000;

pub const WS_EX_DLGMODALFRAME: u32 = 0x0000_0001;
pub const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
pub const WS_EX_WINDOWEDGE: u32 = 0x0000_0100;
pub const WS_EX_CLIENTEDGE: u32 = 0x0000_0200;
pub const WS_EX_STATICEDGE: u32 = 0x0002_0000;
pub const WS_EX_NOREDIRECTIONBITMAP: u32 = 0x0020_0000;

/// One top-level window, as enumeration found it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct WindowCandidate {
    /// The HWND as an integer, so this type needs no Win32 dependency.
    pub hwnd: u64,
    pub pid: u32,
    pub exe_name: Option<String>,
    pub class_name: String,
    pub title: String,
    pub rect: PixelRect,
    pub style: u32,
    pub ex_style: u32,
    pub visible: bool,
}

impl WindowCandidate {
    pub fn area(&self) -> u64 {
        self.rect.width as u64 * self.rect.height as u64
    }

    /// Windows that are never the game, whatever else matches.
    ///
    /// Filtering by *style* rather than by name is what makes this work across
    /// titles: a splash screen is small and frameless, a tool window announces
    /// itself, and a child window is not a top-level game window at all.
    pub fn is_plausible_game_window(&self, min: (u32, u32)) -> bool {
        if !self.visible || self.style & WS_VISIBLE == 0 {
            return false;
        }
        if self.style & WS_CHILD != 0 {
            return false;
        }
        if self.ex_style & WS_EX_TOOLWINDOW != 0 {
            return false;
        }
        // A zero-sized window is a message sink; a tiny one is a splash.
        self.rect.width >= min.0 && self.rect.height >= min.1
    }
}

/// Why a particular window was chosen, so a failure to find one can say what
/// it did look at rather than only that it failed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct WindowMatch {
    pub window: WindowCandidate,
    pub matched_by: Vec<String>,
}

/// Pick the game's window from everything currently open.
///
/// `pid` is the process the launcher started, when it knows it — protocol
/// launches such as `steam://` give none back, which is exactly why the other
/// criteria exist.
///
/// Among equally good matches the **largest** wins. A title that opens both a
/// splash and its render window will have both pass the filters for a moment,
/// and the game is invariably the bigger of the two.
pub fn pick_target(
    candidates: &[WindowCandidate],
    target: &WindowTarget,
    pid: Option<u32>,
) -> Option<WindowMatch> {
    let mut best: Option<(u32, WindowMatch)> = None;

    for c in candidates {
        if !c.is_plausible_game_window(target.min_size) {
            continue;
        }
        if target.require_visible && !c.visible {
            continue;
        }

        let mut score = 0u32;
        let mut matched: Vec<String> = Vec::new();

        // The PID is the strongest signal available, when there is one.
        if let Some(pid) = pid {
            if c.pid == pid {
                score += 8;
                matched.push("process id".into());
            }
        }
        if let Some(exe) = &target.exe_name {
            match &c.exe_name {
                Some(name) if name.eq_ignore_ascii_case(exe) => {
                    score += 4;
                    matched.push(format!("executable {name}"));
                }
                // An explicitly named executable that does not match is
                // disqualifying, not merely unscored — otherwise the launcher
                // grabs whatever else happens to be open.
                Some(_) => continue,
                None => {}
            }
        }
        if let Some(class) = &target.window_class {
            if c.class_name == *class {
                score += 2;
                matched.push(format!("window class {class}"));
            } else {
                continue;
            }
        }
        if let Some(pattern) = &target.title_regex {
            if title_matches(&c.title, pattern) {
                score += 1;
                matched.push(format!("title matching {pattern}"));
            } else {
                continue;
            }
        }

        // Nothing at all identified it. With no criteria and no PID the only
        // honest answer is "no", rather than resizing an arbitrary window.
        if score == 0 {
            continue;
        }

        let candidate = WindowMatch {
            window: c.clone(),
            matched_by: matched,
        };
        let better = match &best {
            None => true,
            Some((best_score, best_match)) => {
                score > *best_score || (score == *best_score && c.area() > best_match.window.area())
            }
        };
        if better {
            best = Some((score, candidate));
        }
    }

    best.map(|(_, m)| m)
}

/// Substring or regex-lite matching for window titles.
///
/// Deliberately not a full regex engine: a dependency for this would be larger
/// than the feature, and the patterns people actually write for a window title
/// are a prefix, a suffix, or a substring. `^` and `$` anchor; everything else
/// is literal and case-insensitive.
pub fn title_matches(title: &str, pattern: &str) -> bool {
    let title = title.to_lowercase();
    let pattern = pattern.to_lowercase();

    match (pattern.strip_prefix('^'), pattern.strip_suffix('$')) {
        (Some(rest), None) => title.starts_with(rest),
        (None, Some(rest)) => title.ends_with(rest),
        (Some(_), Some(_)) => {
            let inner = pattern.trim_start_matches('^').trim_end_matches('$');
            title == inner
        }
        (None, None) => title.contains(&pattern),
    }
}

/// The frame a window's style implies: how much bigger the outer window is
/// than its client area, on each side.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct FrameInsets {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl FrameInsets {
    pub fn is_zero(&self) -> bool {
        self.left == 0 && self.top == 0 && self.right == 0 && self.bottom == 0
    }
}

/// Grow a client rectangle into the outer window rectangle that produces it.
///
/// Asking for a 5120x1440 *client* area and setting that as the *window* rect
/// leaves the client area short by the frame — a seam in the wrong place on
/// every screen of a triple. This is why `RectMeans` exists and has no default.
pub fn outer_from_client(client: PixelRect, insets: FrameInsets) -> PixelRect {
    PixelRect {
        x: client.x - insets.left,
        y: client.y - insets.top,
        width: (client.width as i64 + insets.left as i64 + insets.right as i64).max(0) as u32,
        height: (client.height as i64 + insets.top as i64 + insets.bottom as i64).max(0) as u32,
    }
}

/// The inverse: what client area an outer rectangle yields.
pub fn client_from_outer(outer: PixelRect, insets: FrameInsets) -> PixelRect {
    PixelRect {
        x: outer.x + insets.left,
        y: outer.y + insets.top,
        width: (outer.width as i64 - insets.left as i64 - insets.right as i64).max(0) as u32,
        height: (outer.height as i64 - insets.top as i64 - insets.bottom as i64).max(0) as u32,
    }
}

/// Strip everything that draws a frame.
pub fn borderless_style(style: u32) -> u32 {
    style & !(WS_CAPTION | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU)
}

pub fn borderless_ex_style(ex_style: u32) -> u32 {
    ex_style & !(WS_EX_WINDOWEDGE | WS_EX_CLIENTEDGE | WS_EX_DLGMODALFRAME | WS_EX_STATICEDGE)
}

/// Does this window still have a frame?
pub fn is_borderless(style: u32, ex_style: u32) -> bool {
    style & (WS_CAPTION | WS_THICKFRAME) == 0
        && ex_style & (WS_EX_WINDOWEDGE | WS_EX_CLIENTEDGE | WS_EX_STATICEDGE) == 0
}

/// Has the window drifted from where it was put?
///
/// A tolerance exists because Windows itself nudges windows by a pixel in some
/// DPI configurations, and re-applying forever over a one-pixel disagreement
/// would fight the OS rather than the game.
pub fn has_drifted(actual: PixelRect, target: PixelRect, tolerance: i32) -> bool {
    (actual.x - target.x).abs() > tolerance
        || (actual.y - target.y).abs() > tolerance
        || (actual.width as i32 - target.width as i32).abs() > tolerance
        || (actual.height as i32 - target.height as i32).abs() > tolerance
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WindowTarget;

    fn target() -> WindowTarget {
        WindowTarget {
            exe_name: None,
            window_class: None,
            title_regex: None,
            min_size: (640, 480),
            require_visible: true,
            timeout_ms: 30_000,
        }
    }

    fn window(hwnd: u64, pid: u32, w: u32, h: u32) -> WindowCandidate {
        WindowCandidate {
            hwnd,
            pid,
            exe_name: Some("game.exe".into()),
            class_name: "GameWindowClass".into(),
            title: "The Game".into(),
            rect: PixelRect {
                x: 0,
                y: 0,
                width: w,
                height: h,
            },
            style: WS_VISIBLE | WS_CAPTION | WS_THICKFRAME,
            ex_style: WS_EX_WINDOWEDGE,
            visible: true,
        }
    }

    #[test]
    fn a_splash_screen_is_not_the_game() {
        // The failure this prevents: the launcher resizes a 200x80 splash,
        // reports success, and the game window is never touched.
        let splash = window(1, 100, 200, 80);
        let game = window(2, 100, 1920, 1080);
        let found = pick_target(&[splash, game], &target(), Some(100)).unwrap();
        assert_eq!(found.window.hwnd, 2);
    }

    #[test]
    fn the_largest_window_wins_a_tie() {
        // A title that opens a launcher window and a render window will have
        // both pass the filters for a moment. The game is the bigger one.
        let small = window(1, 100, 1280, 720);
        let big = window(2, 100, 5120, 1440);
        let found = pick_target(&[small, big], &target(), Some(100)).unwrap();
        assert_eq!(found.window.hwnd, 2);
    }

    #[test]
    fn tool_windows_and_children_are_never_the_game() {
        let mut tool = window(1, 100, 1920, 1080);
        tool.ex_style |= WS_EX_TOOLWINDOW;
        let mut child = window(2, 100, 1920, 1080);
        child.style |= WS_CHILD;
        assert!(pick_target(&[tool, child], &target(), Some(100)).is_none());
    }

    #[test]
    fn an_invisible_window_is_not_the_game() {
        // Message-only and pre-show windows exist with the right PID long
        // before the render window does.
        let mut hidden = window(1, 100, 1920, 1080);
        hidden.visible = false;
        hidden.style &= !WS_VISIBLE;
        assert!(pick_target(&[hidden], &target(), Some(100)).is_none());
    }

    #[test]
    fn a_named_executable_that_does_not_match_disqualifies() {
        // Otherwise a profile naming acs.exe would happily grab Notepad when
        // the game has not opened yet.
        let mut t = target();
        t.exe_name = Some("acs.exe".into());
        let notepad = window(1, 100, 1920, 1080);
        assert!(pick_target(&[notepad], &t, None).is_none());
    }

    #[test]
    fn nothing_identifying_it_means_no_match() {
        // No PID and no criteria: resizing an arbitrary window would be worse
        // than doing nothing.
        assert!(pick_target(&[window(1, 100, 1920, 1080)], &target(), None).is_none());
    }

    #[test]
    fn a_grandchild_process_is_found_by_executable_name() {
        // Steam hands off to a process the launcher never sees, so the PID it
        // started is useless. This is the normal case, not the exception.
        let mut t = target();
        t.exe_name = Some("game.exe".into());
        let found = pick_target(&[window(9, 4242, 1920, 1080)], &t, Some(100)).unwrap();
        assert_eq!(found.window.hwnd, 9);
        assert!(found.matched_by.iter().any(|m| m.contains("executable")));
    }

    #[test]
    fn the_match_says_what_identified_it() {
        let mut t = target();
        t.exe_name = Some("game.exe".into());
        t.window_class = Some("GameWindowClass".into());
        let found = pick_target(&[window(1, 100, 1920, 1080)], &t, Some(100)).unwrap();
        assert_eq!(
            found.matched_by.len(),
            3,
            "pid, exe and class: {:?}",
            found.matched_by
        );
    }

    // ------------------------------------------------------------- titles

    #[test]
    fn title_patterns_do_what_people_expect() {
        assert!(title_matches("Assetto Corsa Competizione", "assetto"));
        assert!(title_matches("Assetto Corsa Competizione", "^Assetto"));
        assert!(title_matches("Assetto Corsa Competizione", "Competizione$"));
        assert!(title_matches("Assetto Corsa", "^Assetto Corsa$"));
        assert!(!title_matches("Assetto Corsa", "^Corsa"));
        assert!(!title_matches("Assetto Corsa", "Assetto$"));
    }

    #[test]
    fn title_matching_ignores_case() {
        assert!(title_matches("iRacing.exe Simulator", "IRACING"));
    }

    // -------------------------------------------------------------- styles

    #[test]
    fn borderless_strips_every_frame_bit() {
        let before =
            WS_VISIBLE | WS_CAPTION | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU;
        let after = borderless_style(before);
        assert_eq!(after & WS_CAPTION, 0);
        assert_eq!(after & WS_THICKFRAME, 0);
        assert_eq!(after & WS_SYSMENU, 0);
        assert_eq!(
            after & WS_VISIBLE,
            WS_VISIBLE,
            "the window must stay visible"
        );
    }

    #[test]
    fn borderless_keeps_bits_it_does_not_own() {
        // WS_POPUP and anything a game set for its own reasons must survive.
        let before = WS_VISIBLE | WS_POPUP | WS_CAPTION;
        assert_eq!(borderless_style(before) & WS_POPUP, WS_POPUP);
    }

    #[test]
    fn borderless_is_detected_after_stripping() {
        let style = WS_VISIBLE | WS_CAPTION | WS_THICKFRAME;
        let ex = WS_EX_WINDOWEDGE | WS_EX_CLIENTEDGE;
        assert!(!is_borderless(style, ex));
        assert!(is_borderless(
            borderless_style(style),
            borderless_ex_style(ex)
        ));
    }

    #[test]
    fn an_unrelated_ex_style_survives() {
        let ex = WS_EX_WINDOWEDGE | WS_EX_NOREDIRECTIONBITMAP;
        assert_eq!(borderless_ex_style(ex), WS_EX_NOREDIRECTIONBITMAP);
    }

    // --------------------------------------------------------------- rects

    #[test]
    fn client_and_outer_rects_round_trip() {
        // Getting this backwards leaves the client area short by the frame,
        // which on a triple rig is a seam in the wrong place on every screen.
        let insets = FrameInsets {
            left: 8,
            top: 31,
            right: 8,
            bottom: 8,
        };
        let client = PixelRect {
            x: 0,
            y: 0,
            width: 5120,
            height: 1440,
        };
        let outer = outer_from_client(client, insets);

        assert_eq!(outer.x, -8);
        assert_eq!(outer.y, -31);
        assert_eq!(outer.width, 5136);
        assert_eq!(outer.height, 1479);
        assert_eq!(client_from_outer(outer, insets), client);
    }

    #[test]
    fn a_borderless_window_needs_no_adjustment() {
        let client = PixelRect {
            x: 100,
            y: 200,
            width: 1920,
            height: 1080,
        };
        assert_eq!(outer_from_client(client, FrameInsets::default()), client);
    }

    #[test]
    fn insets_larger_than_the_rect_do_not_underflow() {
        // u32 arithmetic here would wrap to about four billion pixels.
        let insets = FrameInsets {
            left: 100,
            top: 100,
            right: 100,
            bottom: 100,
        };
        let tiny = PixelRect {
            x: 0,
            y: 0,
            width: 50,
            height: 50,
        };
        let client = client_from_outer(tiny, insets);
        assert_eq!((client.width, client.height), (0, 0));
    }

    // ------------------------------------------------------------- drift

    #[test]
    fn drift_ignores_a_pixel_of_disagreement() {
        // Windows nudges windows by a pixel in some DPI configurations, and
        // re-applying over that would fight the OS rather than the game.
        let target = PixelRect {
            x: 0,
            y: 0,
            width: 5120,
            height: 1440,
        };
        let nudged = PixelRect {
            x: 1,
            y: 0,
            width: 5120,
            height: 1440,
        };
        assert!(!has_drifted(nudged, target, 2));

        let moved = PixelRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        assert!(
            has_drifted(moved, target, 2),
            "a sim resetting its own window is drift"
        );
    }
}
