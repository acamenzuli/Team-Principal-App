//! Which running programs are worth offering as "software I need running".
//!
//! The alternative to this is a catalog of known utilities — SimHub is at this
//! path, RaceHub at that one — and it is the wrong shape for the same reason
//! the adapter catalog is kept honest: a guessed path is confidently wrong, and
//! the list would be stale the moment a vendor renamed an installer. It would
//! also never cover the app somebody wrote themselves.
//!
//! So the app asks the machine what is running and lets the user point at it.
//! What is left to decide is which processes are worth showing, because a bare
//! process list is two hundred entries of Windows internals.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A program running right now, as something the user could pick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct RunningApp {
    /// The executable's file name, e.g. `SimHub.exe`. This is what a
    /// `ProcessExists` gate matches on.
    pub exe: String,
    /// The full path, which is what actually gets started.
    pub exe_path: String,
    /// A tidied name for the list: the file name without its extension.
    pub label: String,
}

/// Folders whose contents are Windows, not applications.
///
/// Matched case-insensitively against the start of the path. A process living
/// here is part of the operating system and offering it as a racing utility
/// would bury the two entries somebody is looking for.
const SYSTEM_PREFIXES: [&str; 4] = [
    r"c:\windows\",
    r"c:\program files\windowsapps\microsoft.windows.",
    r"c:\programdata\microsoft\windows defender\",
    r"\??\c:\windows\",
];

/// Is this worth offering in the picker?
pub fn offerable(exe_path: &str) -> bool {
    let lower = exe_path.to_ascii_lowercase();

    if lower.is_empty() || !lower.ends_with(".exe") {
        return false;
    }
    if SYSTEM_PREFIXES.iter().any(|p| lower.starts_with(p)) {
        return false;
    }
    // The app itself. Offering Team Principal as a utility Team Principal must
    // start before racing is a loop, and it looks like a bug even though the
    // executor would refuse it.
    if lower.ends_with(r"\teamprincipal.exe") {
        return false;
    }
    true
}

/// Turn a raw path into an entry, or `None` when it is not worth offering.
pub fn app_from_path(exe_path: &str) -> Option<RunningApp> {
    if !offerable(exe_path) {
        return None;
    }
    let exe = exe_path.rsplit(['\\', '/']).next()?.to_string();
    let label = exe
        .strip_suffix(".exe")
        .or_else(|| exe.strip_suffix(".EXE"))
        .unwrap_or(&exe)
        .to_string();
    Some(RunningApp {
        exe,
        exe_path: exe_path.to_string(),
        label,
    })
}

/// One entry per program, alphabetically.
///
/// Deduplicated by path because a great many applications run several copies
/// of themselves — a browser is a dozen — and the same name a dozen times is
/// a list nobody can use.
pub fn tidy(paths: impl IntoIterator<Item = String>) -> Vec<RunningApp> {
    let mut out: Vec<RunningApp> = Vec::new();
    for path in paths {
        let Some(app) = app_from_path(&path) else {
            continue;
        };
        if !out
            .iter()
            .any(|a| a.exe_path.eq_ignore_ascii_case(&app.exe_path))
        {
            out.push(app);
        }
    }
    out.sort_by_key(|a| a.label.to_lowercase());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_internals_are_not_racing_utilities() {
        assert!(!offerable(r"C:\Windows\System32\svchost.exe"));
        assert!(!offerable(r"C:\Windows\explorer.exe"));
        assert!(!offerable(r"\??\C:\Windows\system32\conhost.exe"));
    }

    #[test]
    fn ordinary_applications_are_offered() {
        assert!(offerable(r"C:\Program Files (x86)\SimHub\SimHub.exe"));
        assert!(offerable(r"D:\Games\CrewChief\CrewChiefV4.exe"));
        assert!(offerable(r"C:\Users\alex\Desktop\my own tool.exe"));
    }

    #[test]
    fn the_app_does_not_offer_itself() {
        // A utility Team Principal must start before racing, which is Team
        // Principal, is a loop — and looks like a bug even when refused.
        assert!(!offerable(
            r"C:\Program Files\Team Principal\TeamPrincipal.exe"
        ));
    }

    #[test]
    fn a_path_that_is_not_an_executable_is_not_offered() {
        assert!(!offerable(""));
        assert!(!offerable(r"C:\Program Files\Thing\thing.dll"));
        assert!(!offerable("System"));
    }

    #[test]
    fn the_label_is_the_name_without_the_extension() {
        let app = app_from_path(r"C:\Program Files (x86)\SimHub\SimHub.exe").unwrap();
        assert_eq!(app.label, "SimHub");
        assert_eq!(app.exe, "SimHub.exe", "what a ProcessExists gate matches");
        assert_eq!(app.exe_path, r"C:\Program Files (x86)\SimHub\SimHub.exe");
    }

    #[test]
    fn a_program_running_twelve_times_appears_once() {
        let list = tidy(vec![
            r"C:\Apps\Browser\browser.exe".to_string(),
            r"C:\Apps\Browser\browser.exe".to_string(),
            r"C:\APPS\BROWSER\BROWSER.EXE".to_string(),
            r"C:\Program Files (x86)\SimHub\SimHub.exe".to_string(),
        ]);
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn the_list_is_alphabetical_regardless_of_case() {
        let list = tidy(vec![
            r"C:\a\zebra.exe".to_string(),
            r"C:\a\Apple.exe".to_string(),
            r"C:\a\mango.exe".to_string(),
        ]);
        let labels: Vec<&str> = list.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(labels, ["Apple", "mango", "zebra"]);
    }

    #[test]
    fn two_different_programs_with_the_same_name_both_appear() {
        // Two installs of the same tool is a real configuration, and collapsing
        // them would silently pick one.
        let list = tidy(vec![
            r"C:\Program Files\Thing\tool.exe".to_string(),
            r"D:\Portable\Thing\tool.exe".to_string(),
        ]);
        assert_eq!(list.len(), 2);
    }
}
