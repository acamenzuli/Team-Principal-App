//! Game adapters: turning the rig into the numbers a sim reads.
//!
//! The premise of the whole product. You measure the rig once, in Screen Setup,
//! and every sim's resolution, window rectangle, field of view and bezel
//! compensation is *derived* from that — never typed per title.
//!
//! ## How an adapter is allowed to be wrong
//!
//! Config key names are the weak point. They differ between versions, forum
//! posts about them are undated, and a key that is *almost* right is silently
//! ignored by the game — leaving an app that reports success and changes
//! nothing. Three rules contain that:
//!
//! * **Nothing is created.** [`crate::Ini::set`] refuses a key the file does not
//!   already have, so every key an adapter writes is verified against the
//!   user's own file at the moment of writing. A wrong name becomes an error
//!   naming the key and listing what the section really contains.
//! * **Every value carries its reason.** A number in a diff with no explanation
//!   is a number nobody can check. `because` is shown next to each change.
//! * **An adapter declares what it has not verified.** [`Confidence`] is part of
//!   the adapter's description and is shown in the UI, so a setting derived
//!   from a documented file and one inferred from a forum thread do not look
//!   alike.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::catalog_games::{ConfigFormat, Derived};
use crate::ini::Ini;
use crate::ipc::RigSolutionInfo;
use crate::rig::{RigModel, ScreenRole, SessionMode};
use crate::xml::Xml;

/// How well an adapter's key names are actually known.
///
/// Shown in the UI, because "we read this out of the shipped file" and "someone
/// on a forum said this in 2019" are different claims and should not look the
/// same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Key names read from a real file of that game, or from the developer's
    /// own documentation. Recorded in `docs/adapters/<title>.md` with a source.
    Verified,
    /// Corroborated across independent second-hand sources, not seen in a real
    /// file. Written only where the user's own file already has the key.
    Corroborated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AdapterInfo {
    pub id: String,
    pub title: String,
    /// Plain sentence: what this adapter changes and what it leaves alone.
    pub scope: String,
    pub confidence: Confidence,
    /// Where the key names came from, so the claim can be checked.
    pub source: String,
    /// False when the title is recognised but nobody has confirmed what its
    /// settings are called. The app will show you the file and refuse to write
    /// to it — the honest middle state between "supported" and "never heard of
    /// it", and where most of the catalog starts.
    pub writes_settings: bool,
}

/// One setting an adapter wants to write, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Edit {
    /// INI section. Empty for XML, where the element name is the whole address.
    pub section: String,
    pub key: String,
    /// XML attribute, when the value is an attribute rather than element text.
    pub attribute: Option<String>,
    pub value: String,
    /// The reason, in the user's terms: "your centre screen is 597 mm of
    /// visible glass". Shown beside the change in the diff.
    pub because: String,
}

/// The file an adapter edits, with the path still to be resolved.
///
/// `{documents}` and `{localappdata}` are substituted by the platform layer.
/// Keeping them as tokens is what lets a plan be built and tested on a machine
/// that has neither folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct FileEdits {
    pub path_template: String,
    pub format: ConfigFormat,
    pub edits: Vec<Edit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AdapterPlan {
    pub adapter_id: String,
    pub files: Vec<FileEdits>,
    /// Things the plan could not express. Reported, never silently averaged.
    pub warnings: Vec<String>,
}

// ------------------------------------------------------------------ the diff

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ValueChange {
    pub section: String,
    pub key: String,
    /// What the file says now.
    pub from: String,
    pub to: String,
    pub because: String,
    /// Already correct. Shown but not counted as a change, so a second apply
    /// visibly does nothing rather than looking like it rewrote the file.
    pub unchanged: bool,
}

/// A key the adapter wanted and the file does not have.
///
/// Carries the section's real keys, because "your file does not have
/// SCREEN_WIDTH, it has these eleven things" is diagnosable and "key not found"
/// is not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct MissingKey {
    pub section: String,
    pub key: String,
    pub reason: String,
    pub section_contains: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub path: String,
    /// False when the file is not there at all — usually "the game has not been
    /// run yet", which is a different problem from a wrong key.
    pub exists: bool,
    pub changes: Vec<ValueChange>,
    pub missing: Vec<MissingKey>,
}

impl FileDiff {
    pub fn will_change(&self) -> bool {
        self.changes.iter().any(|c| !c.unchanged)
    }
}

/// Work out what applying these edits would do, without writing anything.
///
/// The preview half of preview-before-apply. `apply` below performs exactly the
/// changes this reports, from the same code path, so the diff cannot drift from
/// the result.
pub fn diff(path: &str, text: &str, format: ConfigFormat, edits: &[Edit]) -> FileDiff {
    let mut changes = Vec::new();
    let mut missing = Vec::new();

    for edit in edits {
        match read(text, format, edit) {
            Some(current) => changes.push(ValueChange {
                section: edit.section.clone(),
                key: edit.key.clone(),
                unchanged: current == edit.value,
                from: current,
                to: edit.value.clone(),
                because: edit.because.clone(),
            }),
            None => missing.push(MissingKey {
                section: edit.section.clone(),
                key: edit.key.clone(),
                reason: why_missing(text, format, edit),
                section_contains: names_in(text, format, &edit.section),
            }),
        }
    }

    FileDiff {
        path: path.to_string(),
        exists: true,
        changes,
        missing,
    }
}

fn read(text: &str, format: ConfigFormat, edit: &Edit) -> Option<String> {
    match format {
        ConfigFormat::Ini => Ini::parse(text).get(&edit.section, &edit.key),
        ConfigFormat::Xml => Xml::parse(text).get(&edit.key, edit.attribute.as_deref()),
    }
}

/// The editor's own explanation, so the message names the right thing —
/// a missing section reads differently from a missing key.
fn why_missing(text: &str, format: ConfigFormat, edit: &Edit) -> String {
    match format {
        ConfigFormat::Ini => Ini::parse(text)
            .set(&edit.section, &edit.key, &edit.value)
            .unwrap_err()
            .to_string(),
        ConfigFormat::Xml => Xml::parse(text)
            .set(&edit.key, edit.attribute.as_deref(), &edit.value)
            .unwrap_err()
            .to_string(),
    }
}

/// What the file really contains around where the key should have been.
fn names_in(text: &str, format: ConfigFormat, section: &str) -> Vec<String> {
    match format {
        ConfigFormat::Ini => Ini::parse(text).keys(section),
        ConfigFormat::Xml => Xml::parse(text).elements(),
    }
}

/// Apply the edits, returning the new file text.
///
/// Skips keys the file does not have rather than failing the whole write: the
/// diff has already shown them, and refusing to set the nine settings that do
/// exist because a tenth does not helps nobody. What it will never do is create
/// one.
pub fn apply(text: &str, format: ConfigFormat, edits: &[Edit]) -> String {
    match format {
        ConfigFormat::Ini => {
            let mut ini = Ini::parse(text);
            for edit in edits {
                // The error is the expected case for a key this version does
                // not have, and the diff already reported it.
                let _ = ini.set(&edit.section, &edit.key, &edit.value);
            }
            ini.to_text()
        }
        ConfigFormat::Xml => {
            let mut xml = Xml::parse(text);
            for edit in edits {
                let _ = xml.set(&edit.key, edit.attribute.as_deref(), &edit.value);
            }
            xml.to_text()
        }
    }
}

/// A preview across every file an adapter touches, plus what it could not do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AdapterPreview {
    pub files: Vec<FileDiff>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AppliedAdapterInfo {
    /// The backup this write can be undone from.
    pub backup: String,
    pub written: Vec<String>,
}

/// One backed-up operation, for the restore list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub id: String,
    pub taken_at: String,
    /// What caused the write — an adapter id, usually.
    pub reason: String,
    pub files: Vec<String>,
}

/// What a config file actually contains, for a title nobody has confirmed yet.
///
/// This is how a recognised-but-unwritten title becomes a real adapter without
/// anybody guessing: the app reads the file on the user's machine and reports
/// its structure, they send that in, and it becomes a catalog entry backed by a
/// real file. One round trip, no forum posts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ConfigInspection {
    pub path: String,
    pub exists: bool,
    pub format: ConfigFormat,
    /// INI sections with their keys, or XML element names.
    pub groups: Vec<ConfigGroup>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ConfigGroup {
    /// The INI section name, or empty for a flat XML listing.
    pub name: String,
    pub keys: Vec<String>,
}

/// Read a config file's structure. Names only — never values.
///
/// Deliberately not the values. What is needed to write an adapter is *what the
/// settings are called*; the numbers are the user's own business, and a
/// structure listing is something they can read and send without having to
/// judge what is in it.
pub fn inspect(path: &str, text: &str, format: ConfigFormat) -> ConfigInspection {
    let groups = match format {
        ConfigFormat::Ini => {
            let ini = Ini::parse(text);
            ini.sections()
                .into_iter()
                .map(|name| ConfigGroup {
                    keys: ini.keys(&name),
                    name,
                })
                .collect()
        }
        ConfigFormat::Xml => vec![ConfigGroup {
            name: String::new(),
            keys: Xml::parse(text).elements(),
        }],
    };

    ConfigInspection {
        path: path.to_string(),
        exists: true,
        format,
        groups,
    }
}

// --------------------------------------------------------------- the adapters

pub fn catalog() -> Vec<AdapterInfo> {
    crate::catalog_games::shipped()
        .into_iter()
        .map(|e| AdapterInfo {
            id: e.id,
            title: e.title,
            scope: e.scope,
            confidence: e.confidence,
            source: e.source,
            writes_settings: !e.files.iter().all(|f| f.writes.is_empty()),
        })
        .collect()
}

/// Which adapter, if any, handles a game with this name.
///
/// **Exact match against an explicit alias list, never a substring.** This was
/// substring matching first, and a test caught what that does: "Assetto Corsa
/// Competizione" contains "Assetto Corsa", so the fuzzy version would have
/// written Assetto Corsa's `video.ini` keys into a completely different game
/// with a completely different config format.
///
/// Guessing which game an adapter applies to is exactly as dangerous as
/// guessing a key name, and gets the same answer: don't.
pub fn for_game(name: &str) -> Option<AdapterInfo> {
    let wanted = crate::catalog_games::normalise(name);
    let entry = crate::catalog_games::shipped()
        .into_iter()
        .find(|e| e.aliases.contains(&wanted))?;
    catalog().into_iter().find(|a| a.id == entry.id)
}

pub fn plan(
    adapter_id: &str,
    rig: &RigModel,
    solution: &RigSolutionInfo,
    session: SessionMode,
) -> Option<AdapterPlan> {
    let entry = crate::catalog_games::shipped()
        .into_iter()
        .find(|e| e.id == adapter_id)?;

    let facts = RigFacts::from(rig, solution, session);
    let triple = facts.screen_count >= 3;

    // The entry's own notes first — what this adapter deliberately does not
    // touch is the thing a user most needs to know before wondering why a
    // setting did not change.
    let mut warnings = entry.notes.clone();
    warnings.extend(facts.warnings.clone());

    if !entry.writes_anything() {
        warnings.push(format!(
            "{} is recognised, but nobody has confirmed what its settings are \
             called yet, so Team Principal will not write to its config. Use \
             Inspect below to send its file layout in — that is what turns this \
             into a real adapter.",
            entry.title
        ));
    }

    let files = entry
        .files
        .iter()
        .map(|file| FileEdits {
            path_template: file.path_template.clone(),
            format: file.format,
            edits: file
                .writes
                .iter()
                .filter(|w| !w.triple_only || triple)
                .map(|w| {
                    let value = facts.render(&w.value);
                    Edit {
                        section: w.section.clone(),
                        key: w.key.clone(),
                        attribute: w.attribute.clone(),
                        because: w.because.replace("{}", &value),
                        value,
                    }
                })
                .collect(),
        })
        .collect();

    Some(AdapterPlan {
        adapter_id: entry.id,
        files,
        warnings,
    })
}

/// Every number the catalog's vocabulary can ask for, worked out once.
///
/// The arithmetic lives here rather than in each entry, which is what keeps a
/// catalog entry to "this value, in these units, under this key" — and means a
/// correction to how a measurement is derived fixes every title at once.
struct RigFacts {
    screen_width_mm: f64,
    screen_height_mm: f64,
    monitor_width_mm: f64,
    eye_distance_mm: f64,
    bezel_gap_mm: f64,
    side_angle_deg: f64,
    pixel_width: u32,
    pixel_height: u32,
    screen_count: u32,
    warnings: Vec<String>,
}

impl RigFacts {
    fn from(rig: &RigModel, solution: &RigSolutionInfo, session: SessionMode) -> RigFacts {
        let spans_everything = matches!(session, SessionMode::FullSpan { .. });
        let screens: Vec<_> = if spans_everything {
            rig.screens.iter().collect()
        } else {
            rig.screens
                .iter()
                .filter(|s| s.role == ScreenRole::Center)
                .collect()
        };

        let centre = solution
            .screens
            .iter()
            .find(|s| s.role == ScreenRole::Center);
        let centre_spec = rig.screens.iter().find(|s| s.role == ScreenRole::Center);

        let mut warnings = Vec::new();

        let screen_width_mm = centre.map(|c| c.flat_width_mm).unwrap_or(0.0);
        let screen_height_mm = centre.map(|c| c.flat_height_mm).unwrap_or(0.0);
        let bezels = centre_spec
            .map(|s| s.panel.bezel.left.0 + s.panel.bezel.right.0)
            .unwrap_or(0.0);
        let mount_gap = rig
            .screens
            .iter()
            .find(|s| s.role == ScreenRole::Left)
            .map(|s| s.mounting.gap.0)
            .unwrap_or(0.0);

        if mount_gap > 0.0 {
            warnings.push(format!(
                "Your screens have {mount_gap:.0} mm of mounting gap beyond the \
                 bezels. Sims that model the gap as part of the monitor width \
                 have it folded in, which is what makes their bezel \
                 compensation come out right."
            ));
        }

        let left = rig.screens.iter().find(|s| s.role == ScreenRole::Left);
        let right = rig.screens.iter().find(|s| s.role == ScreenRole::Right);
        let side_angle_deg = match (left, right) {
            (Some(l), Some(r)) => {
                let (l, r) = (l.mounting.angle.0, r.mounting.angle.0);
                // Every sim in the catalog takes one angle for both sides.
                // Averaging asymmetric angles silently would make the horizon
                // seam wrong on one side only, which is the hardest kind of
                // wrongness to diagnose by eye.
                if (l - r).abs() > 0.5 {
                    warnings.push(format!(
                        "Your side screens are at different angles ({l:.1}° and \
                         {r:.1}°). These sims take one value for both, so \
                         {:.1}° is used — the average. The seam will be \
                         slightly wrong on one side.",
                        (l + r) / 2.0
                    ));
                }
                (l + r) / 2.0
            }
            _ => 0.0,
        };

        if spans_everything {
            let widths: Vec<f64> = solution.screens.iter().map(|s| s.flat_width_mm).collect();
            if let (Some(min), Some(max)) = (
                widths.iter().cloned().reduce(f64::min),
                widths.iter().cloned().reduce(f64::max),
            ) {
                if max - min > 5.0 {
                    warnings.push(format!(
                        "Your screens are not all the same width ({min:.0} mm to \
                         {max:.0} mm). These sims take one width for all of \
                         them, so the centre screen's is used and the sides \
                         will be slightly off."
                    ));
                }
            }
        }

        let pixel_height = screens
            .iter()
            .map(|s| s.panel.native_resolution.height)
            .max()
            .unwrap_or(0);
        if screens.len() > 1
            && screens
                .iter()
                .any(|s| s.panel.native_resolution.height != pixel_height)
        {
            warnings.push(
                "Your screens are not all the same height in pixels. The tallest \
                 is used, which leaves dead space on the shorter ones — the \
                 Displays tab draws exactly where."
                    .into(),
            );
        }

        RigFacts {
            screen_width_mm,
            screen_height_mm,
            monitor_width_mm: screen_width_mm + bezels + mount_gap,
            eye_distance_mm: centre.map(|c| c.distance_mm).unwrap_or(0.0),
            bezel_gap_mm: bezels + mount_gap,
            side_angle_deg,
            pixel_width: screens
                .iter()
                .map(|s| s.panel.native_resolution.width)
                .sum(),
            pixel_height,
            screen_count: screens.len() as u32,
            warnings,
        }
    }

    fn render(&self, value: &Derived) -> String {
        match value {
            Derived::ScreenWidth { unit, decimals } => {
                fixed(unit.from_mm(self.screen_width_mm), *decimals)
            }
            Derived::ScreenHeight { unit, decimals } => {
                fixed(unit.from_mm(self.screen_height_mm), *decimals)
            }
            Derived::MonitorWidth { unit, decimals } => {
                fixed(unit.from_mm(self.monitor_width_mm), *decimals)
            }
            Derived::EyeDistance { unit, decimals } => {
                fixed(unit.from_mm(self.eye_distance_mm), *decimals)
            }
            Derived::BezelGap { unit, decimals } => {
                fixed(unit.from_mm(self.bezel_gap_mm), *decimals)
            }
            Derived::SideAngle { decimals } => fixed(self.side_angle_deg, *decimals),
            Derived::PixelWidth => self.pixel_width.to_string(),
            Derived::PixelHeight => self.pixel_height.to_string(),
            Derived::ScreenCount => self.screen_count.to_string(),
            Derived::Literal { value } => value.clone(),
            // The shape rFactor 2 and Le Mans Ultimate use: a whole screen
            // definition as one parenthesised value.
            Derived::Tuple { parts } => format!(
                "({})",
                parts
                    .iter()
                    .map(|p| self.render(p))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

fn fixed(value: f64, decimals: u8) -> String {
    format!("{value:.*}", decimals as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    const IRACING_FILE: &str = "[MonitorSetup]\r\n\
        NumMonitors=1                    ; 1 or 3\r\n\
        MonitorWidth=545                 ; (mm) total width of each monitor\r\n\
        ScreenWidth=515                  ; (mm) usable width of each screen\r\n\
        ScreenAngles=15                  ; (deg) side monitor angle\r\n\
        RenderViewPerMonitor=0           ; 0=off 1=separate view\r\n";

    #[test]
    fn a_diff_shows_the_current_value_and_the_reason() {
        let edits = vec![Edit {
            section: "MonitorSetup".into(),
            key: "NumMonitors".into(),
            attribute: None,
            value: "3".into(),
            because: "your rig has three screens".into(),
        }];
        let d = diff("renderer.ini", IRACING_FILE, ConfigFormat::Ini, &edits);
        assert_eq!(d.changes.len(), 1);
        assert_eq!(d.changes[0].from, "1");
        assert_eq!(d.changes[0].to, "3");
        assert!(!d.changes[0].unchanged);
        assert!(d.missing.is_empty());
        assert!(d.will_change());
    }

    #[test]
    fn a_value_already_correct_is_shown_but_not_counted() {
        // So a second apply visibly does nothing, rather than looking like it
        // rewrote the file.
        let edits = vec![Edit {
            section: "MonitorSetup".into(),
            key: "ScreenAngles".into(),
            attribute: None,
            value: "15".into(),
            because: "unchanged".into(),
        }];
        let d = diff("renderer.ini", IRACING_FILE, ConfigFormat::Ini, &edits);
        assert!(d.changes[0].unchanged);
        assert!(!d.will_change());
    }

    #[test]
    fn a_key_the_file_lacks_is_reported_with_what_the_section_does_have() {
        // "Your file has no SCREEN_WIDTH, it has these five things" is
        // diagnosable. "Key not found" is not.
        let edits = vec![Edit {
            section: "MonitorSetup".into(),
            key: "SCREEN_WIDTH_MM".into(),
            attribute: None,
            value: "515".into(),
            because: "guessed".into(),
        }];
        let d = diff("renderer.ini", IRACING_FILE, ConfigFormat::Ini, &edits);
        assert_eq!(d.missing.len(), 1);
        assert!(d.missing[0]
            .section_contains
            .contains(&"ScreenWidth".to_string()));
        assert!(!d.will_change());
    }

    #[test]
    fn applying_never_creates_a_key_the_file_does_not_have() {
        // The rule that stops a wrong key name becoming a silent no-op that
        // reports success.
        let edits = vec![
            Edit {
                section: "MonitorSetup".into(),
                key: "NumMonitors".into(),
                attribute: None,
                value: "3".into(),
                because: String::new(),
            },
            Edit {
                section: "MonitorSetup".into(),
                key: "TotallyMadeUp".into(),
                attribute: None,
                value: "1".into(),
                because: String::new(),
            },
        ];
        let out = apply(IRACING_FILE, ConfigFormat::Ini, &edits);
        assert!(out.contains("NumMonitors=3"), "the real key was written");
        assert!(!out.contains("TotallyMadeUp"), "the invented key was not");
    }

    #[test]
    fn applying_keeps_the_games_own_comments() {
        let edits = vec![Edit {
            section: "MonitorSetup".into(),
            key: "NumMonitors".into(),
            attribute: None,
            value: "3".into(),
            because: String::new(),
        }];
        let out = apply(IRACING_FILE, ConfigFormat::Ini, &edits);
        assert!(out.contains("; (mm) total width of each monitor"));
        assert!(out.contains("\r\n"), "still CRLF");
    }

    fn rig() -> RigModel {
        let mut rig = RigModel::new("test", "2026-01-01T00:00:00Z");
        rig.screens = vec![
            screen(ScreenId(1), ScreenRole::Left, 30.0),
            screen(ScreenId(2), ScreenRole::Center, 0.0),
            screen(ScreenId(3), ScreenRole::Right, 30.0),
        ];
        rig
    }

    fn screen(id: ScreenId, role: ScreenRole, angle: f64) -> ScreenSpec {
        ScreenSpec {
            id,
            role,
            binding: MonitorBinding::Unbound,
            panel: PanelSpec {
                native_resolution: Resolution {
                    width: 1920,
                    height: 1080,
                },
                visible_width: Measurement::manual(597.0),
                visible_height: Measurement::manual(336.0),
                width_measure: WidthMeasure::Chord,
                curvature: Curvature::Flat,
                bezel: Bezel {
                    left: Mm(8.0),
                    right: Mm(8.0),
                    top: Mm(8.0),
                    bottom: Mm(8.0),
                },
            },
            mounting: MountingSpec {
                angle: Deg(angle),
                gap: Mm::ZERO,
                vertical_offset: Mm::ZERO,
                distance_override: None,
                ..Default::default()
            },
        }
    }

    /// A solution built by hand rather than solved.
    ///
    /// `tp-geometry` depends on this crate, so this crate cannot call the
    /// solver — and should not. An adapter's job is to convert a solution into
    /// a game's key names; testing it against a literal keeps that separate
    /// from whether the trigonometry is right, which has its own tests.
    fn solution(rig: &RigModel) -> RigSolutionInfo {
        RigSolutionInfo {
            screens: rig
                .screens
                .iter()
                .map(|s| ScreenSolutionInfo {
                    id: s.id,
                    role: s.role.clone(),
                    distance_mm: 700.0,
                    flat_width_mm: s.panel.visible_width.mm.0,
                    flat_height_mm: s.panel.visible_height.mm.0,
                    h_fov_deg: 46.0,
                    v_fov_deg: 27.0,
                    span: SpanInfo {
                        left_deg: -23.0,
                        right_deg: 23.0,
                        bottom_deg: -13.5,
                        top_deg: 13.5,
                        asymmetry_deg: 0.0,
                    },
                    px_per_deg_h: 41.7,
                    px_per_deg_v: 40.0,
                    inner_gap: None,
                    curvature_error_deg: None,
                    centre: [0.0, 0.0, 700.0],
                    corners: [[0.0; 3]; 4],
                })
                .collect(),
            total_coverage_deg: 140.0,
            visible_coverage_deg: 138.0,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn iracing_gets_physical_millimetres_not_a_field_of_view() {
        // The reason this adapter is a unit conversion rather than a
        // calculation: iRacing wants the measurements and does its own maths.
        let rig = rig();
        let plan = plan(
            "iracing",
            &rig,
            &solution(&rig),
            SessionMode::FullSpan {
                fit: SpanFit::Letterbox,
            },
        )
        .unwrap();
        let edits = &plan.files[0].edits;

        assert_eq!(value(edits, "NumMonitors"), Some("3".into()));
        assert_eq!(value(edits, "ScreenWidth"), Some("597".into()));
        // 597 glass + 8 + 8 bezel.
        assert_eq!(value(edits, "MonitorWidth"), Some("613".into()));
        assert_eq!(value(edits, "ScreenAngles"), Some("30".into()));
        assert_eq!(value(edits, "RenderViewPerMonitor"), Some("1".into()));
    }

    #[test]
    fn a_centre_only_session_says_one_monitor_and_sets_no_angles() {
        let rig = rig();
        let plan = plan("iracing", &rig, &solution(&rig), SessionMode::CenterOnly).unwrap();
        let edits = &plan.files[0].edits;
        assert_eq!(value(edits, "NumMonitors"), Some("1".into()));
        assert_eq!(value(edits, "ScreenAngles"), None);
    }

    #[test]
    fn asymmetric_side_angles_are_warned_about_rather_than_averaged_silently() {
        // One field for both sides. Averaging without saying so makes the seam
        // wrong on one side only, which is the hardest kind of wrong to see.
        let mut rig = rig();
        rig.screens[0].mounting.angle = Deg(30.0);
        rig.screens[2].mounting.angle = Deg(40.0);
        let plan = plan(
            "iracing",
            &rig,
            &solution(&rig),
            SessionMode::FullSpan {
                fit: SpanFit::Letterbox,
            },
        )
        .unwrap();
        assert!(plan.warnings.iter().any(|w| w.contains("different angles")));
        assert_eq!(
            value(&plan.files[0].edits, "ScreenAngles"),
            Some("35".into())
        );
    }

    #[test]
    fn a_mount_gap_is_folded_into_monitor_width_and_said_so() {
        // iRacing has no field for it, and leaving it out makes its bezel
        // compensation short by exactly the gap.
        let mut rig = rig();
        rig.screens[0].mounting.gap = Mm(20.0);
        let plan = plan(
            "iracing",
            &rig,
            &solution(&rig),
            SessionMode::FullSpan {
                fit: SpanFit::Letterbox,
            },
        )
        .unwrap();
        assert_eq!(
            value(&plan.files[0].edits, "MonitorWidth"),
            Some("633".into())
        );
        assert!(plan.warnings.iter().any(|w| w.contains("mounting gap")));
    }

    #[test]
    fn assetto_corsa_sums_the_widths_and_says_it_does_not_do_the_triple_app() {
        let rig = rig();
        let plan = plan(
            "assetto_corsa",
            &rig,
            &solution(&rig),
            SessionMode::FullSpan {
                fit: SpanFit::Letterbox,
            },
        )
        .unwrap();
        let edits = &plan.files[0].edits;
        assert_eq!(value(edits, "WIDTH"), Some("5760".into()));
        assert_eq!(value(edits, "HEIGHT"), Some("1080".into()));
        assert_eq!(value(edits, "FULLSCREEN"), Some("0".into()));
        // Saying what it does not do is part of the adapter, not an omission.
        assert!(plan.warnings.iter().any(|w| w.contains("in-game app")));
    }

    #[test]
    fn every_edit_carries_a_reason() {
        // A number in a diff with no explanation is a number nobody can check.
        let rig = rig();
        for adapter in catalog() {
            let plan = plan(
                &adapter.id,
                &rig,
                &solution(&rig),
                SessionMode::FullSpan {
                    fit: SpanFit::Letterbox,
                },
            )
            .unwrap();
            for file in &plan.files {
                for edit in &file.edits {
                    assert!(!edit.because.is_empty(), "{} {}", adapter.id, edit.key);
                }
            }
        }
    }

    #[test]
    fn a_game_is_matched_to_its_adapter_however_its_install_is_named() {
        assert_eq!(
            for_game("Assetto Corsa").map(|a| a.id),
            Some("assetto_corsa".into())
        );
        assert_eq!(
            for_game("assettocorsa").map(|a| a.id),
            Some("assetto_corsa".into())
        );
        assert_eq!(for_game("iRacing").map(|a| a.id), Some("iracing".into()));
        assert_eq!(
            for_game("iRacing Simulator").map(|a| a.id),
            Some("iracing".into())
        );
    }

    #[test]
    fn a_similarly_named_game_gets_its_own_entry_not_its_neighbours() {
        // The reason matching is exact. Competizione, EVO and Rally all
        // contain "Assetto Corsa" and all have different config formats — a
        // substring match would have written one game's keys into another,
        // which is worse than having no adapter at all.
        assert_eq!(
            for_game("Assetto Corsa").map(|a| a.id),
            Some("assetto_corsa".into())
        );
        assert_eq!(
            for_game("Assetto Corsa Competizione").map(|a| a.id),
            Some("acc".into())
        );
        assert_eq!(
            for_game("Assetto Corsa EVO").map(|a| a.id),
            Some("ac_evo".into())
        );
        assert_eq!(
            for_game("Assetto Corsa Rally").map(|a| a.id),
            Some("ac_rally".into())
        );
    }

    #[test]
    fn a_recognised_title_with_an_unconfirmed_layout_writes_nothing() {
        // The honest middle state: the app knows the game and where its
        // settings live, and will not touch them until somebody has looked at
        // a real file.
        let acc = for_game("Assetto Corsa Competizione").unwrap();
        assert!(!acc.writes_settings);

        let rig = rig();
        let plan = plan(
            "acc",
            &rig,
            &solution(&rig),
            SessionMode::FullSpan {
                fit: SpanFit::Letterbox,
            },
        )
        .unwrap();
        assert!(plan.files.iter().all(|f| f.edits.is_empty()));
        assert!(plan.warnings.iter().any(|w| w.contains("Inspect")));
    }

    #[test]
    fn inspecting_reports_names_and_never_values() {
        // What is needed to write an adapter is what the settings are called.
        // The numbers are the user's own business, and a listing they can read
        // is one they will actually send.
        let found = inspect("renderer.ini", IRACING_FILE, ConfigFormat::Ini);
        let group = found
            .groups
            .iter()
            .find(|g| g.name == "MonitorSetup")
            .unwrap();
        assert!(group.keys.contains(&"ScreenWidth".to_string()));
        assert!(group.keys.contains(&"NumMonitors".to_string()));

        let text = serde_json::to_string(&found).unwrap();
        assert!(!text.contains("545"), "a value leaked into the listing");
        assert!(!text.contains("515"), "a value leaked into the listing");
    }

    #[test]
    fn inspecting_xml_lists_its_elements() {
        let found = inspect(
            "graphics_options.xml",
            "<r><screenWidth>2560</screenWidth><fullscreen>1</fullscreen></r>",
            ConfigFormat::Xml,
        );
        assert!(found.groups[0].keys.contains(&"screenWidth".to_string()));
        let text = serde_json::to_string(&found).unwrap();
        assert!(!text.contains("2560"), "a value leaked into the listing");
    }

    #[test]
    fn an_unknown_adapter_is_none_rather_than_an_empty_plan() {
        let rig = rig();
        assert!(plan(
            "some_sim_nobody_has_added",
            &rig,
            &solution(&rig),
            SessionMode::FullSpan {
                fit: SpanFit::Letterbox
            }
        )
        .is_none());
    }

    fn value(edits: &[Edit], key: &str) -> Option<String> {
        edits.iter().find(|e| e.key == key).map(|e| e.value.clone())
    }
}
