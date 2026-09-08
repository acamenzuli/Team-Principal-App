//! Editing INI files without damaging them.
//!
//! Every sim in wave one keeps its settings in an INI file that the user has
//! very likely edited by hand, that the game rewrites on exit, and that carries
//! comments explaining what the values mean. A parser that reads it into a map
//! and writes the map back out destroys all of that — comment lines, ordering,
//! spacing, and any key it did not recognise.
//!
//! So this is not a parser. It is an *editor*: the file is kept as a list of
//! lines, and setting a value rewrites exactly one line. Everything else comes
//! out byte-for-byte as it went in, including the line endings, which are CRLF
//! in every one of these files and which a naive rewrite silently converts.
//!
//! ## It will not invent keys
//!
//! [`Ini::set`] fails on a key that is not already in the file. That is
//! deliberate and it is the most important rule here. A key name that is
//! *almost* right — right section, wrong spelling, or right for the version
//! before last — is silently ignored by the game, and the user is left with an
//! app that reports success and changes nothing. Refusing to create keys turns
//! that into an error message naming the key, and means an adapter's key names
//! are verified against the user's own file rather than against a forum post.
//!
//! Adding a key that genuinely belongs is [`Ini::insert`], which is a separate
//! and deliberate call.

use std::collections::BTreeMap;

/// One INI file, held as its lines so an edit changes only what it must.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ini {
    lines: Vec<String>,
    /// The terminator the file actually uses. Preserved rather than normalised:
    /// these files are CRLF, some tools that read them care, and rewriting a
    /// whole file's line endings is an invisible change nobody asked for.
    newline: &'static str,
    /// Whether the last line had a terminator. A file that ended without one
    /// should still end without one.
    trailing_newline: bool,
}

/// Where a key lives, or why it does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    NoSuchSection { section: String },
    NoSuchKey { section: String, key: String },
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyError::NoSuchSection { section } => {
                write!(f, "this file has no [{section}] section")
            }
            KeyError::NoSuchKey { section, key } => {
                write!(f, "[{section}] has no {key} setting")
            }
        }
    }
}

impl std::error::Error for KeyError {}

impl Ini {
    pub fn parse(text: &str) -> Ini {
        // Detect before splitting: a file with mixed endings keeps whichever it
        // uses first, which is what an editor that appended a line would do.
        let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let trailing_newline = text.ends_with('\n');

        let body = text.strip_suffix(newline).unwrap_or(text);
        let lines = if body.is_empty() && text.is_empty() {
            Vec::new()
        } else {
            body.split(newline).map(str::to_string).collect()
        };

        Ini {
            lines,
            newline,
            trailing_newline,
        }
    }

    pub fn to_text(&self) -> String {
        let mut out = self.lines.join(self.newline);
        if self.trailing_newline && !self.lines.is_empty() {
            out.push_str(self.newline);
        }
        out
    }

    /// The value of a key, if the file has one.
    pub fn get(&self, section: &str, key: &str) -> Option<String> {
        self.find(section, key).map(|i| {
            split_entry(&self.lines[i])
                .map(|(_, v)| v)
                .unwrap_or_default()
        })
    }

    /// Change an existing key's value, in place.
    ///
    /// Fails rather than adding the key. See the module docs: a key this file
    /// does not have is a key the game does not read, and silently creating one
    /// produces an app that claims success and changes nothing.
    ///
    /// An inline trailing comment survives, and stays in its column where the
    /// new value is no longer than the old. iRacing's renderer.ini documents
    /// every key's units in exactly such a comment — `; (mm) total width of
    /// each monitor` — and deleting those while writing a value would be the
    /// precise damage this module exists to prevent.
    pub fn set(&mut self, section: &str, key: &str, value: &str) -> Result<(), KeyError> {
        let index = self.find(section, key).ok_or_else(|| {
            if self.section_at(section).is_some() {
                KeyError::NoSuchKey {
                    section: section.to_string(),
                    key: key.to_string(),
                }
            } else {
                KeyError::NoSuchSection {
                    section: section.to_string(),
                }
            }
        })?;

        // Keep the key exactly as the file spells it, and keep the spacing
        // around the separator. A file written as `WIDTH = 1920` stays that way.
        let line = &self.lines[index];
        let eq = line.find('=').unwrap_or(line.len());
        let (before, after) = line.split_at(eq + 1);
        let leading: String = after.chars().take_while(|c| *c == ' ').collect();

        let (old_value, comment) = split_value_comment(&after[leading.len()..]);
        let comment = match comment {
            // Hold the comment's column while the value fits, so a file whose
            // comments line up still lines up afterwards.
            Some((gap, text)) => {
                let width = old_value.len() + gap.len();
                let pad = width.saturating_sub(value.len()).max(1);
                format!("{}{text}", " ".repeat(pad))
            }
            None => String::new(),
        };

        self.lines[index] = format!("{before}{leading}{value}{comment}");
        Ok(())
    }

    /// Add a key to an existing section, after its last entry.
    ///
    /// Separate from `set` so that creating a key is always a deliberate act.
    /// Placed after the section's last key rather than at the section's end, so
    /// a trailing comment block stays attached to what it describes.
    pub fn insert(&mut self, section: &str, key: &str, value: &str) -> Result<(), KeyError> {
        if self.find(section, key).is_some() {
            return self.set(section, key, value);
        }
        let start = self
            .section_at(section)
            .ok_or_else(|| KeyError::NoSuchSection {
                section: section.to_string(),
            })?;

        let mut insert_at = start + 1;
        for (offset, line) in self.lines[start + 1..].iter().enumerate() {
            if section_name(line).is_some() {
                break;
            }
            if split_entry(line).is_some() {
                insert_at = start + 1 + offset + 1;
            }
        }
        self.lines.insert(insert_at, format!("{key}={value}"));
        Ok(())
    }

    /// Does the file have this key at all? The question an adapter asks before
    /// claiming it can write a setting.
    pub fn has(&self, section: &str, key: &str) -> bool {
        self.find(section, key).is_some()
    }

    pub fn sections(&self) -> Vec<String> {
        self.lines.iter().filter_map(|l| section_name(l)).collect()
    }

    /// Every key in a section, in file order, for diagnostics.
    pub fn keys(&self, section: &str) -> Vec<String> {
        let Some(start) = self.section_at(section) else {
            return Vec::new();
        };
        let mut keys = Vec::new();
        for line in &self.lines[start + 1..] {
            if section_name(line).is_some() {
                break;
            }
            if let Some((k, _)) = split_entry(line) {
                keys.push(k);
            }
        }
        keys
    }

    fn section_at(&self, section: &str) -> Option<usize> {
        self.lines
            .iter()
            .position(|l| section_name(l).is_some_and(|s| s.eq_ignore_ascii_case(section)))
    }

    /// Case-insensitive on both, because these files are not consistent even
    /// within one game: AC shouts its keys and iRacing camel-cases them.
    fn find(&self, section: &str, key: &str) -> Option<usize> {
        let start = self.section_at(section)?;
        for (offset, line) in self.lines[start + 1..].iter().enumerate() {
            if section_name(line).is_some() {
                return None;
            }
            if let Some((k, _)) = split_entry(line) {
                if k.eq_ignore_ascii_case(key) {
                    return Some(start + 1 + offset);
                }
            }
        }
        None
    }
}

/// `[NAME]`, ignoring surrounding whitespace. Not a comment.
fn section_name(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if is_comment(trimmed) {
        return None;
    }
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    Some(inner.trim().to_string())
}

/// `key=value`, ignoring comments. Returns the key as the file spells it.
fn split_entry(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if is_comment(trimmed) || trimmed.is_empty() || trimmed.starts_with('[') {
        return None;
    }
    let (key, value) = trimmed.split_once('=')?;
    let (value, _) = split_value_comment(value);
    Some((key.trim().to_string(), value.trim().to_string()))
}

/// Separate a value from an inline trailing comment.
///
/// Returns the value, and the comment with the whitespace that preceded it so
/// the line can be put back together unchanged.
///
/// A comment must be preceded by whitespace. `;` and `//` do appear inside real
/// values — a Windows path, a time signature — and treating one as a comment
/// would silently truncate a setting.
fn split_value_comment(raw: &str) -> (&str, Option<(&str, &str)>) {
    let bytes = raw.as_bytes();
    for (i, window) in bytes.windows(2).enumerate() {
        let starts_comment =
            window[1] == b';' || (window[1] == b'/' && bytes.get(i + 2) == Some(&b'/'));
        if !(window[0] as char).is_whitespace() || !starts_comment {
            continue;
        }
        let value = raw[..=i].trim_end();
        let gap = &raw[value.len()..=i];
        return (value, Some((gap, &raw[i + 1..])));
    }
    (raw.trim_end(), None)
}

/// `;` and `//` are the two comment forms these files actually use. `#` is
/// deliberately *not* one: it appears inside real values, in colour codes and
/// in Windows device paths.
fn is_comment(trimmed: &str) -> bool {
    trimmed.starts_with(';') || trimmed.starts_with("//")
}

/// The values a file currently holds for a set of keys, for the before side of
/// a diff.
pub fn read_all(ini: &Ini, keys: &[(String, String)]) -> BTreeMap<String, Option<String>> {
    keys.iter()
        .map(|(section, key)| (format!("{section}/{key}"), ini.get(section, key)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file shaped like the ones this actually edits: CRLF, comments, mixed
    /// spacing, and a section whose keys are not alphabetical.
    const SAMPLE: &str = "; Assetto Corsa video settings\r\n\
                          [VIDEO]\r\n\
                          FULLSCREEN=1\r\n\
                          WIDTH = 1920\r\n\
                          HEIGHT=1080\r\n\
                          \r\n\
                          ; camera\r\n\
                          [CAMERA]\r\n\
                          MODE=DEFAULT\r\n";

    #[test]
    fn a_file_that_is_read_and_written_unchanged_is_byte_identical() {
        // The whole premise. If this fails, every edit is also damaging the
        // parts it did not touch.
        assert_eq!(Ini::parse(SAMPLE).to_text(), SAMPLE);
    }

    #[test]
    fn setting_a_value_changes_exactly_one_line() {
        let mut ini = Ini::parse(SAMPLE);
        ini.set("VIDEO", "HEIGHT", "1440").unwrap();
        let out = ini.to_text();

        assert!(out.contains("HEIGHT=1440"));
        assert!(
            out.contains("; Assetto Corsa video settings"),
            "comment kept"
        );
        assert!(out.contains("; camera"), "inner comment kept");
        assert!(out.contains("FULLSCREEN=1"), "neighbours untouched");
        assert_eq!(
            SAMPLE.lines().count(),
            out.lines().count(),
            "no lines added or removed"
        );
    }

    #[test]
    fn crlf_survives() {
        // These files are CRLF and a whole-file conversion is an invisible
        // change nobody asked for.
        let mut ini = Ini::parse(SAMPLE);
        ini.set("VIDEO", "WIDTH", "5760").unwrap();
        let out = ini.to_text();
        assert!(out.contains("\r\n"));
        assert!(!out.contains("\n\n"), "no bare LF crept in");
    }

    #[test]
    fn spacing_around_the_separator_is_preserved() {
        // `WIDTH = 1920` stays spaced; `HEIGHT=1080` stays tight.
        let mut ini = Ini::parse(SAMPLE);
        ini.set("VIDEO", "WIDTH", "5760").unwrap();
        ini.set("VIDEO", "HEIGHT", "1080").unwrap();
        let out = ini.to_text();
        assert!(out.contains("WIDTH = 5760"), "{out}");
        assert!(out.contains("HEIGHT=1080"), "{out}");
    }

    #[test]
    fn a_key_the_file_does_not_have_is_an_error_not_a_new_line() {
        // The rule that makes an adapter's key names verifiable against the
        // user's own file rather than against a forum post.
        let mut ini = Ini::parse(SAMPLE);
        let err = ini.set("VIDEO", "REFRESH_RATE", "120").unwrap_err();
        assert_eq!(
            err,
            KeyError::NoSuchKey {
                section: "VIDEO".into(),
                key: "REFRESH_RATE".into()
            }
        );
        assert_eq!(ini.to_text(), SAMPLE, "nothing was written");
    }

    #[test]
    fn a_missing_section_says_so_rather_than_blaming_the_key() {
        let mut ini = Ini::parse(SAMPLE);
        assert_eq!(
            ini.set("MONITORSETUP", "NumMonitors", "3").unwrap_err(),
            KeyError::NoSuchSection {
                section: "MONITORSETUP".into()
            }
        );
    }

    #[test]
    fn lookup_is_case_insensitive_both_ways() {
        // AC shouts its keys, iRacing camel-cases them, and neither is
        // consistent across versions.
        let ini = Ini::parse(SAMPLE);
        assert_eq!(ini.get("video", "fullscreen"), Some("1".into()));
        assert_eq!(ini.get("VIDEO", "Height"), Some("1080".into()));
    }

    #[test]
    fn the_files_own_spelling_of_a_key_is_kept() {
        let mut ini = Ini::parse(SAMPLE);
        ini.set("video", "fullscreen", "0").unwrap();
        assert!(ini.to_text().contains("FULLSCREEN=0"), "not fullscreen=0");
    }

    #[test]
    fn a_key_in_a_later_section_is_not_found_from_an_earlier_one() {
        // The bug that writes CAMERA's MODE into VIDEO because the scan ran off
        // the end of the section.
        let ini = Ini::parse(SAMPLE);
        assert_eq!(ini.get("VIDEO", "MODE"), None);
        assert_eq!(ini.get("CAMERA", "MODE"), Some("DEFAULT".into()));
    }

    #[test]
    fn a_commented_out_key_is_not_a_key() {
        let text = "[VIDEO]\n; WIDTH=1920\n// HEIGHT=1080\nFULLSCREEN=1\n";
        let ini = Ini::parse(text);
        assert!(!ini.has("VIDEO", "WIDTH"));
        assert!(!ini.has("VIDEO", "HEIGHT"));
        assert!(ini.has("VIDEO", "FULLSCREEN"));
    }

    #[test]
    fn a_hash_is_part_of_a_value_not_a_comment() {
        // Colour codes and device paths contain them, and treating one as a
        // comment truncates a real setting.
        let ini = Ini::parse("[UI]\nColour=#FF6B35\n");
        assert_eq!(ini.get("UI", "Colour"), Some("#FF6B35".into()));
    }

    #[test]
    fn inserting_puts_the_key_after_the_sections_last_entry() {
        // Not at the end of the section: a trailing comment block belongs to
        // whatever comes next and should stay there.
        let text = "[VIDEO]\nWIDTH=1920\nHEIGHT=1080\n\n; the camera section\n[CAMERA]\nMODE=X\n";
        let mut ini = Ini::parse(text);
        ini.insert("VIDEO", "REFRESH", "120").unwrap();
        assert_eq!(
            ini.to_text(),
            "[VIDEO]\nWIDTH=1920\nHEIGHT=1080\nREFRESH=120\n\n; the camera section\n[CAMERA]\nMODE=X\n"
        );
    }

    #[test]
    fn inserting_a_key_that_exists_sets_it_instead_of_duplicating_it() {
        let mut ini = Ini::parse(SAMPLE);
        ini.insert("VIDEO", "WIDTH", "5760").unwrap();
        assert_eq!(ini.to_text().matches("WIDTH").count(), 1);
    }

    #[test]
    fn a_file_with_no_trailing_newline_keeps_not_having_one() {
        let text = "[VIDEO]\nWIDTH=1920";
        let mut ini = Ini::parse(text);
        ini.set("VIDEO", "WIDTH", "2560").unwrap();
        assert_eq!(ini.to_text(), "[VIDEO]\nWIDTH=2560");
    }

    #[test]
    fn an_empty_file_round_trips() {
        assert_eq!(Ini::parse("").to_text(), "");
    }

    /// iRacing's renderer.ini, whose inline comments are the only documentation
    /// of what the units are.
    const COMMENTED: &str = "[MonitorSetup]\r\n\
        NumMonitors=1                    ; 1 or 3\r\n\
        MonitorWidth=545                 ; (mm) total width of each monitor\r\n\
        ScreenAngles=15                  ; (deg) side monitor angle\r\n";

    #[test]
    fn an_inline_comment_is_not_part_of_the_value() {
        let ini = Ini::parse(COMMENTED);
        assert_eq!(ini.get("MonitorSetup", "NumMonitors"), Some("1".into()));
        assert_eq!(ini.get("MonitorSetup", "MonitorWidth"), Some("545".into()));
    }

    #[test]
    fn setting_a_value_keeps_the_games_own_inline_comment() {
        // Deleting these while writing a value would destroy the only
        // documentation of what the numbers mean.
        let mut ini = Ini::parse(COMMENTED);
        ini.set("MonitorSetup", "NumMonitors", "3").unwrap();
        let out = ini.to_text();
        assert!(out.contains("NumMonitors=3"));
        assert!(out.contains("; 1 or 3"), "{out}");
    }

    #[test]
    fn a_comment_column_holds_while_the_value_fits() {
        let mut ini = Ini::parse(COMMENTED);
        ini.set("MonitorSetup", "MonitorWidth", "613").unwrap();
        let out = ini.to_text();
        let line = out.lines().find(|l| l.starts_with("MonitorWidth")).unwrap();
        let original = COMMENTED
            .lines()
            .find(|l| l.starts_with("MonitorWidth"))
            .unwrap();
        assert_eq!(
            line.find(';'),
            original.find(';'),
            "the comment moved: {line}"
        );
    }

    #[test]
    fn a_longer_value_pushes_the_comment_rather_than_eating_it() {
        let mut ini = Ini::parse(COMMENTED);
        ini.set(
            "MonitorSetup",
            "NumMonitors",
            "1234567890123456789012345678901234567890",
        )
        .unwrap();
        assert!(ini.to_text().contains("; 1 or 3"));
    }

    #[test]
    fn a_semicolon_with_no_space_before_it_stays_in_the_value() {
        // Real values contain them; a comment in these files is preceded by
        // whitespace.
        let ini = Ini::parse("[X]\nPath=C:\\a;C:\\b\n");
        assert_eq!(ini.get("X", "Path"), Some("C:\\a;C:\\b".into()));
    }

    #[test]
    fn keys_lists_a_section_in_file_order() {
        // What the UI shows when an adapter's key is missing: "here is what
        // your file actually has" beats "key not found".
        let ini = Ini::parse(SAMPLE);
        assert_eq!(ini.keys("VIDEO"), vec!["FULLSCREEN", "WIDTH", "HEIGHT"]);
        assert_eq!(ini.sections(), vec!["VIDEO", "CAMERA"]);
    }
}
