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

use crate::ini::Ini;
use crate::ipc::RigSolutionInfo;
use crate::rig::{RigModel, ScreenRole, SessionMode};

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
}

/// One setting an adapter wants to write, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Edit {
    pub section: String,
    pub key: String,
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
pub fn diff(path: &str, text: &str, edits: &[Edit]) -> FileDiff {
    let ini = Ini::parse(text);
    let mut changes = Vec::new();
    let mut missing = Vec::new();

    for edit in edits {
        match ini.get(&edit.section, &edit.key) {
            Some(current) => changes.push(ValueChange {
                section: edit.section.clone(),
                key: edit.key.clone(),
                unchanged: current == edit.value,
                from: current,
                to: edit.value.clone(),
                because: edit.because.clone(),
            }),
            None => {
                let mut ini_clone = Ini::parse(text);
                let reason = ini_clone
                    .set(&edit.section, &edit.key, &edit.value)
                    .unwrap_err()
                    .to_string();
                missing.push(MissingKey {
                    section: edit.section.clone(),
                    key: edit.key.clone(),
                    reason,
                    section_contains: ini.keys(&edit.section),
                });
            }
        }
    }

    FileDiff {
        path: path.to_string(),
        exists: true,
        changes,
        missing,
    }
}

/// Apply the edits, returning the new file text.
///
/// Skips keys the file does not have rather than failing the whole write: the
/// diff has already shown them, and refusing to set the nine settings that do
/// exist because a tenth does not helps nobody. What it will never do is create
/// one.
pub fn apply(text: &str, edits: &[Edit]) -> String {
    let mut ini = Ini::parse(text);
    for edit in edits {
        // The error is the expected case for a key this version does not have,
        // and the diff already reported it.
        let _ = ini.set(&edit.section, &edit.key, &edit.value);
    }
    ini.to_text()
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

// --------------------------------------------------------------- the adapters

pub fn catalog() -> Vec<AdapterInfo> {
    vec![iracing_info(), assetto_corsa_info()]
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
/// guessing a key name, and gets the same answer: don't. A title nobody has
/// listed gets no adapter, which is a visible gap rather than a silent wrong
/// write.
pub fn for_game(name: &str) -> Option<AdapterInfo> {
    let wanted = normalise(name);
    catalog()
        .into_iter()
        .find(|a| aliases(&a.id).iter().any(|alias| *alias == wanted))
}

/// Every name an install of this game is known to go by, normalised.
///
/// Installs are named inconsistently — "Assetto Corsa" from Steam's manifest,
/// "assettocorsa" from the folder, "iRacing Simulator" from the registry — so
/// the list is explicit rather than clever.
fn aliases(adapter_id: &str) -> &'static [&'static str] {
    match adapter_id {
        "iracing" => &["iracing", "iracingsimulator", "iracingcom"],
        "assetto_corsa" => &["assettocorsa"],
        _ => &[],
    }
}

/// Lowercase, letters and digits only.
///
/// "Assetto Corsa", "assettocorsa" and "ASSETTO CORSA Competizione" have to
/// compare usefully, and the difference that matters between the first and the
/// last is a whole word rather than punctuation.
fn normalise(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

pub fn plan(
    adapter_id: &str,
    rig: &RigModel,
    solution: &RigSolutionInfo,
    session: SessionMode,
) -> Option<AdapterPlan> {
    match adapter_id {
        "iracing" => Some(iracing_plan(rig, solution, session)),
        "assetto_corsa" => Some(assetto_corsa_plan(rig, solution, session)),
        _ => None,
    }
}

fn iracing_info() -> AdapterInfo {
    AdapterInfo {
        id: "iracing".into(),
        title: "iRacing".into(),
        scope: "Writes the triple-screen geometry in renderer.ini — how many \
                screens, their physical widths in millimetres, and the side \
                angle. Leaves every graphics quality setting alone."
            .into(),
        confidence: Confidence::Verified,
        source: "Key names and their units read from a real renderer.ini with \
                 the game's own inline comments. See docs/adapters/iracing.md."
            .into(),
    }
}

/// iRacing is the adapter this whole app was designed around.
///
/// Its `[MonitorSetup]` section does not want a field of view. It wants the
/// physical measurements — millimetres of glass, millimetres including bezels,
/// and the angle the side screens are turned in — and computes the projection
/// itself. That is exactly the rig model, so this adapter is a unit conversion
/// rather than a calculation, and there is nothing to get subtly wrong.
fn iracing_plan(rig: &RigModel, solution: &RigSolutionInfo, session: SessionMode) -> AdapterPlan {
    let mut edits = Vec::new();
    let mut warnings = Vec::new();

    // FullSpan is the mode that puts the game across every screen. CenterOnly
    // and a custom rectangle are both single-surface as far as a sim's
    // triple-screen projection is concerned.
    let triple = matches!(session, SessionMode::FullSpan { .. }) && rig.screens.len() >= 3;
    let count = if triple { 3 } else { 1 };

    edits.push(Edit {
        section: "MonitorSetup".into(),
        key: "NumMonitors".into(),
        value: count.to_string(),
        because: if triple {
            "your rig has three screens and this session uses all of them".into()
        } else {
            "this session uses the centre screen only".into()
        },
    });

    let centre = solution
        .screens
        .iter()
        .find(|s| s.role == ScreenRole::Center);
    let centre_spec = rig.screens.iter().find(|s| s.role == ScreenRole::Center);

    if let (Some(centre), Some(spec)) = (centre, centre_spec) {
        // ScreenWidth is the usable glass. The solver's flat width is the right
        // number for a curved panel too — it is the chord, which is what a
        // renderer projecting onto a plane actually sees.
        edits.push(Edit {
            section: "MonitorSetup".into(),
            key: "ScreenWidth".into(),
            value: format!("{:.0}", centre.flat_width_mm),
            because: format!(
                "{:.0} mm of visible glass on your centre screen",
                centre.flat_width_mm
            ),
        });

        // MonitorWidth is screen plus bezels — iRacing's model assumes the
        // monitors are butted together, so the difference between the two is
        // the gap it compensates for. A rig with extra space between the
        // screens has to fold that space in here, or the bezel correction is
        // short by exactly the mount gap.
        let bezels = spec.panel.bezel.left.0 + spec.panel.bezel.right.0;
        let mount_gap = rig
            .screens
            .iter()
            .find(|s| s.role == ScreenRole::Left)
            .map(|s| s.mounting.gap.0)
            .unwrap_or(0.0);
        if mount_gap > 0.0 {
            warnings.push(format!(
                "Your screens have {mount_gap:.0} mm of mounting gap beyond the \
                 bezels. iRacing has no separate field for it, so it is folded \
                 into MonitorWidth — which is what makes its bezel compensation \
                 come out right."
            ));
        }
        edits.push(Edit {
            section: "MonitorSetup".into(),
            key: "MonitorWidth".into(),
            value: format!("{:.0}", centre.flat_width_mm + bezels + mount_gap),
            because: format!(
                "{:.0} mm of glass plus {:.0} mm of bezel{}",
                centre.flat_width_mm,
                bezels,
                if mount_gap > 0.0 {
                    format!(" plus {mount_gap:.0} mm of mounting gap")
                } else {
                    String::new()
                }
            ),
        });
    }

    if triple {
        let left = rig.screens.iter().find(|s| s.role == ScreenRole::Left);
        let right = rig.screens.iter().find(|s| s.role == ScreenRole::Right);
        if let (Some(left), Some(right)) = (left, right) {
            let (l, r) = (left.mounting.angle.0, right.mounting.angle.0);
            // One field for both sides. Averaging asymmetric angles silently
            // would make the horizon seam wrong on one side only, which is the
            // hardest kind of wrongness to diagnose by eye.
            if (l - r).abs() > 0.5 {
                warnings.push(format!(
                    "Your side screens are at different angles ({l:.1}° and \
                     {r:.1}°). iRacing has one field for both, so it is set to \
                     {:.1}° — the average. The seam will be slightly wrong on \
                     one side.",
                    (l + r) / 2.0
                ));
            }
            edits.push(Edit {
                section: "MonitorSetup".into(),
                key: "ScreenAngles".into(),
                value: format!("{:.0}", (l + r) / 2.0),
                because: format!("your side screens are turned in {:.1}°", (l + r) / 2.0),
            });
        }

        edits.push(Edit {
            section: "MonitorSetup".into(),
            key: "RenderViewPerMonitor".into(),
            value: "1".into(),
            because: "a separate view per screen is what makes the side screens \
                      geometrically correct rather than stretched"
                .into(),
        });
    }

    // Screens whose physical size differs enough to matter. iRacing takes one
    // width for all three.
    if triple {
        let widths: Vec<f64> = solution.screens.iter().map(|s| s.flat_width_mm).collect();
        if let (Some(min), Some(max)) = (
            widths.iter().cloned().reduce(f64::min),
            widths.iter().cloned().reduce(f64::max),
        ) {
            if max - min > 5.0 {
                warnings.push(format!(
                    "Your screens are not all the same width ({min:.0} mm to \
                     {max:.0} mm). iRacing takes one width for all three, so \
                     the centre screen's is used and the sides will be slightly \
                     off."
                ));
            }
        }
    }

    AdapterPlan {
        adapter_id: "iracing".into(),
        files: vec![FileEdits {
            path_template: "{documents}\\iRacing\\renderer.ini".into(),
            edits,
        }],
        warnings,
    }
}

fn assetto_corsa_info() -> AdapterInfo {
    AdapterInfo {
        id: "assetto_corsa".into(),
        title: "Assetto Corsa".into(),
        scope: "Writes the render resolution and windowed/fullscreen mode in \
                video.ini. Does not touch the in-game triple-screen app's own \
                numbers — see the note below."
            .into(),
        confidence: Confidence::Corroborated,
        source: "[VIDEO] WIDTH, HEIGHT and FULLSCREEN corroborated across \
                 several independent sources; not yet read from a shipped file. \
                 See docs/adapters/assetto-corsa.md."
            .into(),
    }
}

/// Assetto Corsa takes a resolution, and computes its triple-screen projection
/// from numbers entered in an in-game app rather than from this file.
///
/// So this adapter deliberately does *less* than the iRacing one: it sets the
/// render resolution derived from the rig and stops. Writing guessed key names
/// for the triple-screen values would be exactly the twelve-guessed-adapters
/// failure this project is trying to avoid.
fn assetto_corsa_plan(
    rig: &RigModel,
    _solution: &RigSolutionInfo,
    session: SessionMode,
) -> AdapterPlan {
    let mut warnings = Vec::new();

    let screens: Vec<_> = if matches!(session, SessionMode::FullSpan { .. }) {
        rig.screens.iter().collect()
    } else {
        rig.screens
            .iter()
            .filter(|s| s.role == ScreenRole::Center)
            .collect()
    };

    let width: u32 = screens
        .iter()
        .map(|s| s.panel.native_resolution.width)
        .sum();
    let height = screens
        .iter()
        .map(|s| s.panel.native_resolution.height)
        .max()
        .unwrap_or(0);

    if screens.len() > 1
        && screens
            .iter()
            .any(|s| s.panel.native_resolution.height != height)
    {
        warnings.push(
            "Your screens are not all the same height in pixels. The tallest is \
             used, which leaves dead space on the shorter ones — the Displays \
             tab draws exactly where."
                .into(),
        );
    }

    warnings.push(
        "Assetto Corsa's triple-screen geometry lives in its in-game app, not \
         in this file, and Team Principal does not write it yet. Your rig's \
         numbers are on the Screen Setup tab to copy across."
            .into(),
    );

    AdapterPlan {
        adapter_id: "assetto_corsa".into(),
        files: vec![FileEdits {
            path_template: "{documents}\\Assetto Corsa\\cfg\\video.ini".into(),
            edits: vec![
                Edit {
                    section: "VIDEO".into(),
                    key: "WIDTH".into(),
                    value: width.to_string(),
                    because: if screens.len() > 1 {
                        format!("{} screens side by side", screens.len())
                    } else {
                        "your centre screen's native width".into()
                    },
                },
                Edit {
                    section: "VIDEO".into(),
                    key: "HEIGHT".into(),
                    value: height.to_string(),
                    because: "your screens' native height".into(),
                },
                Edit {
                    section: "VIDEO".into(),
                    key: "FULLSCREEN".into(),
                    value: "0".into(),
                    because: "borderless windowed, so Team Principal can place \
                              the window and alt-tab stays instant"
                        .into(),
                },
            ],
        }],
        warnings,
    }
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
            value: "3".into(),
            because: "your rig has three screens".into(),
        }];
        let d = diff("renderer.ini", IRACING_FILE, &edits);
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
            value: "15".into(),
            because: "unchanged".into(),
        }];
        let d = diff("renderer.ini", IRACING_FILE, &edits);
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
            value: "515".into(),
            because: "guessed".into(),
        }];
        let d = diff("renderer.ini", IRACING_FILE, &edits);
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
                value: "3".into(),
                because: String::new(),
            },
            Edit {
                section: "MonitorSetup".into(),
                key: "TotallyMadeUp".into(),
                value: "1".into(),
                because: String::new(),
            },
        ];
        let out = apply(IRACING_FILE, &edits);
        assert!(out.contains("NumMonitors=3"), "the real key was written");
        assert!(!out.contains("TotallyMadeUp"), "the invented key was not");
    }

    #[test]
    fn applying_keeps_the_games_own_comments() {
        let edits = vec![Edit {
            section: "MonitorSetup".into(),
            key: "NumMonitors".into(),
            value: "3".into(),
            because: String::new(),
        }];
        let out = apply(IRACING_FILE, &edits);
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
    fn a_different_game_with_a_similar_name_is_not_claimed() {
        // The reason matching is exact. Competizione and EVO are different
        // games with different config formats, and both contain "Assetto
        // Corsa" — a substring match would have written one game's keys into
        // another, which is worse than having no adapter at all.
        assert_eq!(for_game("Assetto Corsa Competizione").map(|a| a.id), None);
        assert_eq!(for_game("Assetto Corsa EVO").map(|a| a.id), None);
        assert!(for_game("Dirt Rally 2.0").is_none());
    }

    #[test]
    fn an_unknown_adapter_is_none_rather_than_an_empty_plan() {
        let rig = rig();
        assert!(plan(
            "rfactor2",
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
