//! Valve KeyValues, and finding installed games in it.
//!
//! Steam records its library folders and every installed game in its own text
//! format — nested `"key" "value"` pairs and `"key" { ... }` blocks. Parsing it
//! is how the app finds a sim's install path without asking anyone to type one.
//!
//! Small enough to hand-write, and worth hand-writing: the format has exactly
//! two constructs, and a dependency would be larger than the parser.
//!
//! Pure string work, so a malformed manifest is a test rather than a support
//! ticket.

use std::collections::BTreeMap;

/// A parsed KeyValues node: either a string or a block of children.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Block(BTreeMap<String, Value>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Block(map) => map.get(key),
            Value::String(_) => None,
        }
    }

    /// Case-insensitive lookup. Steam is inconsistent about capitalisation
    /// between versions — `appid` and `AppID` both appear in the wild.
    pub fn get_ci(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Block(map) => map
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            Value::String(_) => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            Value::Block(_) => None,
        }
    }

    /// Children of a block; empty for a string, so callers need no match.
    pub fn entries(&self) -> impl Iterator<Item = (&String, &Value)> {
        static EMPTY: std::sync::LazyLock<BTreeMap<String, Value>> =
            std::sync::LazyLock::new(BTreeMap::new);
        match self {
            Value::Block(map) => map.iter(),
            Value::String(_) => EMPTY.iter(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VdfError {
    #[error("unexpected end of file: {0}")]
    Truncated(&'static str),
    #[error("unexpected {found:?} at byte {at}")]
    Unexpected { found: char, at: usize },
}

/// Parse a KeyValues document into a single root block.
pub fn parse(input: &str) -> Result<Value, VdfError> {
    let mut chars = input.char_indices().peekable();
    let mut root = BTreeMap::new();

    loop {
        skip_trivia(&mut chars);
        if chars.peek().is_none() {
            break;
        }
        let (key, value) = parse_pair(&mut chars)?;
        root.insert(key, value);
    }
    Ok(Value::Block(root))
}

type Chars<'a> = std::iter::Peekable<std::str::CharIndices<'a>>;

fn parse_pair(chars: &mut Chars) -> Result<(String, Value), VdfError> {
    let key = parse_string(chars)?;
    skip_trivia(chars);

    match chars.peek() {
        Some((_, '{')) => {
            chars.next();
            let mut block = BTreeMap::new();
            loop {
                skip_trivia(chars);
                match chars.peek() {
                    Some((_, '}')) => {
                        chars.next();
                        break;
                    }
                    None => return Err(VdfError::Truncated("a block was never closed")),
                    _ => {
                        let (k, v) = parse_pair(chars)?;
                        block.insert(k, v);
                    }
                }
            }
            Ok((key, Value::Block(block)))
        }
        Some(_) => Ok((key, Value::String(parse_string(chars)?))),
        None => Err(VdfError::Truncated("a key had no value")),
    }
}

fn parse_string(chars: &mut Chars) -> Result<String, VdfError> {
    skip_trivia(chars);
    match chars.peek().copied() {
        Some((_, '"')) => {
            chars.next();
            let mut out = String::new();
            loop {
                match chars.next() {
                    Some((_, '\\')) => match chars.next() {
                        // Steam escapes backslashes in paths, which is the only
                        // escape that appears in practice.
                        Some((_, 'n')) => out.push('\n'),
                        Some((_, 't')) => out.push('\t'),
                        Some((_, c)) => out.push(c),
                        None => return Err(VdfError::Truncated("a string was never closed")),
                    },
                    Some((_, '"')) => return Ok(out),
                    Some((_, c)) => out.push(c),
                    None => return Err(VdfError::Truncated("a string was never closed")),
                }
            }
        }
        // Unquoted tokens are legal and do appear.
        Some((at, c)) if !c.is_whitespace() && c != '{' && c != '}' => {
            let mut out = String::new();
            while let Some((_, c)) = chars.peek().copied() {
                if c.is_whitespace() || c == '{' || c == '}' || c == '"' {
                    break;
                }
                out.push(c);
                chars.next();
            }
            if out.is_empty() {
                return Err(VdfError::Unexpected { found: c, at });
            }
            Ok(out)
        }
        Some((at, c)) => Err(VdfError::Unexpected { found: c, at }),
        None => Err(VdfError::Truncated("expected a key or value")),
    }
}

fn skip_trivia(chars: &mut Chars) {
    loop {
        match chars.peek().copied() {
            Some((_, c)) if c.is_whitespace() => {
                chars.next();
            }
            Some((_, '/')) => {
                // `//` to end of line. Steam writes these in libraryfolders.
                chars.next();
                if matches!(chars.peek(), Some((_, '/'))) {
                    for (_, c) in chars.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                } else {
                    return;
                }
            }
            _ => return,
        }
    }
}

/// A Steam library folder, as `libraryfolders.vdf` records it.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryFolder {
    pub path: String,
    /// App IDs Steam says are installed here. Present in modern versions;
    /// empty on older ones, where the manifests have to be listed instead.
    pub app_ids: Vec<String>,
}

/// Read `libraryfolders.vdf`.
pub fn parse_library_folders(input: &str) -> Result<Vec<LibraryFolder>, VdfError> {
    let root = parse(input)?;
    let folders = root.get_ci("libraryfolders").unwrap_or(&root);

    Ok(folders
        .entries()
        // Entries are numbered "0", "1", ...; anything else is metadata such as
        // "contentstatsid" and is not a library.
        .filter(|(k, _)| k.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|(_, v)| {
            let path = match v {
                // Older Steam wrote the path as the value directly.
                Value::String(s) => s.clone(),
                Value::Block(_) => v.get_ci("path")?.as_str()?.to_string(),
            };
            let app_ids = v
                .get_ci("apps")
                .map(|apps| apps.entries().map(|(k, _)| k.clone()).collect())
                .unwrap_or_default();
            Some(LibraryFolder { path, app_ids })
        })
        .collect())
}

/// One installed game, from an `appmanifest_*.acf`.
#[derive(Debug, Clone, PartialEq)]
pub struct SteamApp {
    pub app_id: String,
    pub name: String,
    /// The folder name under `steamapps/common`, not a full path — the library
    /// it belongs to supplies the rest.
    pub install_dir: String,
}

pub fn parse_app_manifest(input: &str) -> Result<Option<SteamApp>, VdfError> {
    let root = parse(input)?;
    let state = root.get_ci("AppState").unwrap_or(&root);

    let (Some(app_id), Some(install_dir)) = (
        state.get_ci("appid").and_then(Value::as_str),
        state.get_ci("installdir").and_then(Value::as_str),
    ) else {
        // A manifest without these is a download in progress or a leftover, not
        // an installed game. Skipping it beats inventing a path.
        return Ok(None);
    };

    Ok(Some(SteamApp {
        app_id: app_id.to_string(),
        name: state
            .get_ci("name")
            .and_then(Value::as_str)
            .unwrap_or(install_dir)
            .to_string(),
        install_dir: install_dir.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pairs_and_blocks() {
        let v = parse(
            r#"
            "root"
            {
                "name"  "Assetto Corsa"
                "nested" { "a" "1" }
            }
        "#,
        )
        .unwrap();
        let root = v.get("root").unwrap();
        assert_eq!(root.get("name").unwrap().as_str(), Some("Assetto Corsa"));
        assert_eq!(
            root.get("nested").unwrap().get("a").unwrap().as_str(),
            Some("1")
        );
    }

    #[test]
    fn handles_escaped_paths() {
        // Steam writes Windows paths with escaped backslashes, and getting this
        // wrong produces a path that does not exist.
        let v = parse(r#""path"  "D:\\SteamLibrary""#).unwrap();
        assert_eq!(v.get("path").unwrap().as_str(), Some(r"D:\SteamLibrary"));
    }

    #[test]
    fn skips_comments() {
        let v = parse("// a comment\n\"a\" \"1\" // trailing\n\"b\" \"2\"").unwrap();
        assert_eq!(v.get("a").unwrap().as_str(), Some("1"));
        assert_eq!(v.get("b").unwrap().as_str(), Some("2"));
    }

    #[test]
    fn lookups_ignore_case() {
        // `appid` and `AppID` both appear, depending on Steam's version.
        let v = parse(r#""AppState" { "AppID" "244210" }"#).unwrap();
        assert!(v.get_ci("appstate").unwrap().get_ci("appid").is_some());
    }

    #[test]
    fn a_truncated_file_is_an_error_not_a_panic() {
        // A half-written manifest during a Steam update is a real state.
        assert!(parse(r#""a" { "b" "1" "#).is_err());
        assert!(parse(r#""unterminated"#).is_err());
        assert!(parse(r#""key""#).is_err());
    }

    #[test]
    fn reads_modern_library_folders() {
        let vdf = r#"
        "libraryfolders"
        {
            "contentstatsid"  "1234567890"
            "0"
            {
                "path"  "C:\\Program Files (x86)\\Steam"
                "apps" { "244210" "12345" "244310" "6789" }
            }
            "1"
            {
                "path"  "D:\\SteamLibrary"
                "apps" { "805550" "999" }
            }
        }
        "#;
        let folders = parse_library_folders(vdf).unwrap();
        assert_eq!(
            folders.len(),
            2,
            "contentstatsid is metadata, not a library"
        );
        assert_eq!(folders[0].path, r"C:\Program Files (x86)\Steam");
        assert_eq!(folders[1].path, r"D:\SteamLibrary");
        assert!(folders[0].app_ids.contains(&"244210".to_string()));
    }

    #[test]
    fn reads_the_older_flat_form() {
        // Older Steam wrote the path as the value directly. People do not
        // upgrade Steam just because a launcher would prefer it.
        let vdf = r#"
        "LibraryFolders"
        {
            "TimeNextStatsReport"  "1234"
            "0"  "D:\\SteamLibrary"
        }
        "#;
        let folders = parse_library_folders(vdf).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].path, r"D:\SteamLibrary");
        assert!(folders[0].app_ids.is_empty());
    }

    #[test]
    fn reads_an_app_manifest() {
        let acf = r#"
        "AppState"
        {
            "appid"       "244210"
            "name"        "Assetto Corsa"
            "installdir"  "assettocorsa"
            "StateFlags"  "4"
        }
        "#;
        let app = parse_app_manifest(acf).unwrap().unwrap();
        assert_eq!(app.app_id, "244210");
        assert_eq!(app.name, "Assetto Corsa");
        assert_eq!(app.install_dir, "assettocorsa");
    }

    #[test]
    fn a_manifest_without_an_install_dir_is_skipped() {
        // A download in progress, or a leftover. Skipping beats inventing a path.
        let acf = r#""AppState" { "appid" "244210" }"#;
        assert_eq!(parse_app_manifest(acf).unwrap(), None);
    }

    #[test]
    fn a_manifest_with_no_name_falls_back_to_its_folder() {
        let acf = r#""AppState" { "appid" "1" "installdir" "somegame" }"#;
        let app = parse_app_manifest(acf).unwrap().unwrap();
        assert_eq!(app.name, "somegame", "better than an empty row");
    }
}
