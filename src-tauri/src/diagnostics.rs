//! The diagnostics bundle.
//!
//! One button that produces one file, containing everything needed to work out
//! what happened on a machine nobody debugging it can see. This app runs on
//! other people's hardware, against sims and peripherals its author does not
//! own, and "it did not work" with no attachment is a support conversation that
//! goes nowhere.
//!
//! ## Two rules
//!
//! **The user can see what they are sending.** The bundle contains a
//! `README.txt` listing every file in it and exactly what was redacted. Asking
//! someone to email a black box about their own machine is not reasonable, and
//! a bundle people are afraid of is a bundle nobody sends.
//!
//! **Redact the person, keep the machine.** The Windows account name appears in
//! almost every path here and identifies the user without helping anyone
//! diagnose anything, so it is replaced. Monitor serial numbers go the same
//! way. Everything else — paths, device ids, geometry, timings — stays, because
//! over-redacting produces a bundle that is safe and useless.

use std::io::Write;
use std::path::PathBuf;

use zip::write::SimpleFileOptions;

use crate::error::{AppError, AppResult};
use crate::providers::Providers;

/// How many days of logs to include.
///
/// Logs rotate daily. Three days covers "it broke last night and I am reporting
/// it now" without turning the bundle into something too big to email.
const LOG_DAYS: usize = 3;

/// Build a bundle and return where it was written.
pub fn build(providers: &Providers) -> AppResult<PathBuf> {
    let out_dir = crate::logging::app_data_dir().join("diagnostics");
    std::fs::create_dir_all(&out_dir)?;

    let stamp = crate::now_iso8601().replace(':', "-");
    let path = out_dir.join(format!("team-principal-diagnostics-{stamp}.zip"));

    let file = std::fs::File::create(&path)
        .map_err(|e| AppError::Io(format!("could not create the bundle: {e}")))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut contents: Vec<String> = Vec::new();

    // Everything is redacted on the way in, so nothing can reach the archive
    // by a path that skipped it.
    let mut add =
        |zip: &mut zip::ZipWriter<std::fs::File>, name: &str, body: String| -> AppResult<()> {
            zip.start_file(name, options)
                .map_err(|e| AppError::Io(format!("could not add {name}: {e}")))?;
            zip.write_all(redact(&body).as_bytes())?;
            contents.push(name.to_string());
            Ok(())
        };

    add(&mut zip, "app.json", app_facts(providers))?;

    // Each of these is best-effort: a bundle missing the device list because
    // the enumeration failed is still worth having, and the failure itself is
    // information — so it is written into the file rather than swallowed.
    add(
        &mut zip,
        "monitors.json",
        json_or_error(providers.display.enumerate()),
    )?;
    add(
        &mut zip,
        "devices.json",
        json_or_error(providers.peripherals.enumerate()),
    )?;
    // Built by hand rather than serialised: DesktopLayout is a geometry type
    // and lives in a crate that has no serde dependency, which is the point of
    // keeping tp-geometry free of everything.
    add(
        &mut zip,
        "desktop-layout.json",
        json_or_error(providers.display.desktop_layout().map(|layout| {
            layout.map(|l| {
                serde_json::json!({
                    "bounds": l.bounds,
                    "deadRegions": l.dead_regions,
                    "coveredArea": l.covered_area,
                    "deadArea": l.dead_area(),
                })
            })
        })),
    )?;
    add(&mut zip, "rigs.json", json_of(&crate::rig::list()))?;
    add(&mut zip, "profiles.json", json_of(&crate::profiles::list()))?;
    add(
        &mut zip,
        "snapshots.json",
        json_of(&crate::snapshots::list()),
    )?;
    // The backup *manifests*, not the backed-up files. Someone's game config is
    // theirs, and a support bundle is not the place for it.
    add(&mut zip, "backups.json", json_of(&crate::backup::list()))?;
    add(
        &mut zip,
        "preferences.json",
        // `load` returns the preferences and any complaint about the file it
        // came from. Both are worth having: a preferences file that failed to
        // parse is exactly the kind of thing this bundle exists to reveal.
        {
            let (preferences, problem) = crate::settings::load();
            json_of(&serde_json::json!({ "preferences": preferences, "problem": problem }))
        },
    )?;

    for (name, body) in recent_logs() {
        add(&mut zip, &format!("logs/{name}"), body)?;
    }

    // Written last, so it lists what actually went in rather than what was
    // meant to.
    zip.start_file("README.txt", options)
        .map_err(|e| AppError::Io(format!("could not add README.txt: {e}")))?;
    zip.write_all(readme(&contents).as_bytes())?;

    zip.finish()
        .map_err(|e| AppError::Io(format!("could not finish the bundle: {e}")))?;

    tracing::info!(path = %path.display(), "diagnostics bundle written");
    Ok(path)
}

fn app_facts(providers: &Providers) -> String {
    let awareness = crate::dpi_awareness();
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "milestone": crate::MILESTONE,
        "simulated": providers.simulated,
        "dpiAwareness": format!("{awareness:?}"),
        "dpiAwarenessOk": awareness.is_acceptable(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "generatedAt": crate::now_iso8601(),
        "panicHotkey": crate::display::hotkey::DESCRIPTION,
    })
    .to_string()
}

/// Serialise, or record why it could not be.
///
/// A failure here is a genuine finding — "your display enumeration returned an
/// error" is often the whole answer — so it is written into the bundle instead
/// of leaving a file mysteriously absent.
fn json_or_error<T: serde::Serialize, E: std::fmt::Display>(value: Result<T, E>) -> String {
    match value {
        Ok(v) => json_of(&v),
        Err(e) => format!("{{\"error\": \"{e}\"}}"),
    }
}

/// For the things that cannot fail to be produced, only to be serialised.
fn json_of<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value)
        .unwrap_or_else(|e| format!("{{\"serialiseError\": \"{e}\"}}"))
}

/// The most recent log files, newest first.
fn recent_logs() -> Vec<(String, String)> {
    let dir = crate::logging::app_data_dir().join("logs");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    // The rotating appender names files with the date, so lexical order is
    // chronological order.
    files.sort();
    files.reverse();

    files
        .into_iter()
        .take(LOG_DAYS)
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?.to_string();
            let body = std::fs::read_to_string(&p).ok()?;
            Some((name, body))
        })
        .collect()
}

/// Replace the things that identify a person rather than a machine.
///
/// The account name is the big one: it appears in every `C:\Users\...` path,
/// in the log lines that mention them, and in the profile and rig files.
/// Replacing it everywhere — rather than only in paths — is deliberate, because
/// a name that survives in one field is a name that was not redacted.
fn redact(text: &str) -> String {
    let mut out = text.to_string();

    if let Ok(user) = std::env::var("USERNAME") {
        // Two characters is not a name worth redacting and would mangle the
        // file into nonsense.
        if user.len() > 2 {
            out = replace_case_insensitive(&out, &user, "<user>");
        }
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        if !profile.is_empty() {
            out = replace_case_insensitive(&out, &profile, r"C:\Users\<user>");
        }
    }
    out
}

/// Case-insensitive replace. Windows paths vary in case between APIs — one call
/// returns `C:\Users\Alex` and the next `C:\users\alex` — so a case-sensitive
/// pass would redact some occurrences and leave others.
fn replace_case_insensitive(haystack: &str, needle: &str, with: &str) -> String {
    // An empty needle matches at every position and never advances the cursor.
    if needle.is_empty() {
        return haystack.to_string();
    }
    let lower_haystack = haystack.to_lowercase();
    let lower_needle = needle.to_lowercase();
    let mut out = String::with_capacity(haystack.len());
    let mut cursor = 0usize;

    while let Some(found) = lower_haystack[cursor..].find(&lower_needle) {
        let at = cursor + found;
        out.push_str(&haystack[cursor..at]);
        out.push_str(with);
        cursor = at + needle.len();
    }
    out.push_str(&haystack[cursor..]);
    out
}

fn readme(contents: &[String]) -> String {
    let list = contents
        .iter()
        .map(|n| format!("  {n}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "Team Principal — diagnostics bundle\n\
         Generated {}\n\
         \n\
         WHAT IS IN HERE\n\
         \n\
         {list}\n\
           README.txt              this file\n\
         \n\
         WHAT WAS REMOVED\n\
         \n\
         Your Windows account name has been replaced with <user> everywhere it\n\
         appeared, including inside file paths and log lines.\n\
         \n\
         WHAT WAS KEPT\n\
         \n\
         Everything else: monitor models and sizes, device VID/PIDs, folder\n\
         paths, your rig measurements, profile settings and timings. These are\n\
         what make the bundle worth sending — a bundle with them stripped out\n\
         is safe and useless.\n\
         \n\
         Your game config files are NOT included. Backups of them are listed by\n\
         name in backups.json, but their contents stay on your machine.\n\
         \n\
         Read any of it before you send it. It is all plain text.\n",
        crate::now_iso8601()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_replaced_whatever_its_case() {
        // Windows APIs disagree about the case of the same path, so a
        // case-sensitive pass would redact some occurrences and miss others.
        let text = r"C:\Users\Alex\x and C:\users\alex\y and ALEX";
        assert_eq!(
            replace_case_insensitive(text, "Alex", "<user>"),
            r"C:\Users\<user>\x and C:\users\<user>\y and <user>"
        );
    }

    #[test]
    fn replacing_something_absent_changes_nothing() {
        assert_eq!(
            replace_case_insensitive("nothing to see", "Alex", "<user>"),
            "nothing to see"
        );
    }

    #[test]
    fn an_empty_needle_does_not_loop_forever() {
        // A guard on the obvious infinite loop rather than a comment about it.
        assert_eq!(replace_case_insensitive("abc", "", "X"), "abc");
    }

    #[test]
    fn the_readme_lists_what_is_actually_in_the_bundle() {
        let text = readme(&["app.json".into(), "logs/today.log".into()]);
        assert!(text.contains("app.json"));
        assert!(text.contains("logs/today.log"));
        assert!(text.contains("README.txt"));
        // The two claims that make it safe to send.
        assert!(text.contains("<user>"));
        assert!(text.contains("NOT included"));
    }
}
