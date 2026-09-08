//! Planning a display change, and saying what it will do before it does it.
//!
//! Changing the desktop topology is the most dangerous thing this app does. A
//! wrong mode is a black screen, and a black screen on a rig with no second
//! input is a machine you cannot reach without a reboot. So every rule that can
//! be checked without touching the GPU is checked here, where it is tested, and
//! the Win32 layer below only carries out a plan that already passed.
//!
//! Three ideas hold it together:
//!
//! * **A plan is a snapshot.** The desired state has exactly the shape of the
//!   captured one, so "apply this plan" and "put it back how it was" are the
//!   same operation with different arguments — and the revert path is therefore
//!   the path that gets exercised on every apply.
//! * **The diff is the preview.** Nothing is written until the user has seen a
//!   list of the specific changes, in the same words the result will use.
//! * **Windows' own constraints are checked first.** The primary must sit at
//!   the origin, panels may not overlap, and the desktop must be contiguous.
//!   Windows silently *rearranges* a layout that breaks these rather than
//!   refusing it, which means a plan that looks applied can be quietly wrong.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::display::{DisplayMode, SnapshotEntry, TopologySnapshot};
use crate::units::{PixelRect, Resolution};

/// The modes one output can actually run, as the driver reports them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AvailableModes {
    pub device_path: String,
    pub modes: Vec<DisplayMode>,
}

/// One thing a plan would change, in the words the UI shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum TopologyChange {
    Resolution {
        monitor: String,
        from: Resolution,
        to: Resolution,
    },
    RefreshRate {
        monitor: String,
        from_hz: u32,
        to_hz: u32,
    },
    Position {
        monitor: String,
        from: (i32, i32),
        to: (i32, i32),
    },
    Primary {
        from: Option<String>,
        to: String,
    },
    Activated {
        monitor: String,
    },
    Deactivated {
        monitor: String,
    },
}

/// How badly a plan is wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ProblemSeverity {
    /// Windows will refuse it, or accept it and produce something else.
    Blocking,
    /// It will apply as asked, and you probably did not mean it.
    Warning,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TopologyProblem {
    pub severity: ProblemSeverity,
    pub message: String,
}

/// Everything the confirm screen needs: what changes, and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TopologyPreview {
    pub changes: Vec<TopologyChange>,
    pub problems: Vec<TopologyProblem>,
}

impl TopologyPreview {
    /// Nothing to do. Applying is still safe, but there is no reason to.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn can_apply(&self) -> bool {
        !self
            .problems
            .iter()
            .any(|p| p.severity == ProblemSeverity::Blocking)
    }
}

/// What a plan would do to the desktop as it stands.
///
/// `names` maps device path to something a person recognises. A device path is
/// the right key and the wrong label — nobody knows which monitor
/// `\\?\DISPLAY#SAM7179#5&...` is.
pub fn preview(
    current: &TopologySnapshot,
    desired: &TopologySnapshot,
    names: &BTreeMap<String, String>,
) -> TopologyPreview {
    TopologyPreview {
        changes: diff(current, desired, names),
        problems: validate(desired, names),
    }
}

fn label(path: &str, names: &BTreeMap<String, String>) -> String {
    names.get(path).cloned().unwrap_or_else(|| path.to_string())
}

fn diff(
    current: &TopologySnapshot,
    desired: &TopologySnapshot,
    names: &BTreeMap<String, String>,
) -> Vec<TopologyChange> {
    let mut changes = Vec::new();

    for want in &desired.monitors {
        let name = label(&want.device_path, names);
        let Some(have) = current
            .monitors
            .iter()
            .find(|m| m.device_path == want.device_path)
        else {
            // A monitor the plan knows and the desktop does not. Treated as an
            // activation rather than dropped, so a plan written while a screen
            // was unplugged still says what it intends to do with it.
            if want.active {
                changes.push(TopologyChange::Activated { monitor: name });
            }
            continue;
        };

        if have.active != want.active {
            changes.push(if want.active {
                TopologyChange::Activated {
                    monitor: name.clone(),
                }
            } else {
                TopologyChange::Deactivated {
                    monitor: name.clone(),
                }
            });
            // Everything else about an output being switched off is noise.
            if !want.active {
                continue;
            }
        }

        if have.mode.resolution != want.mode.resolution {
            changes.push(TopologyChange::Resolution {
                monitor: name.clone(),
                from: have.mode.resolution,
                to: want.mode.resolution,
            });
        }
        if have.mode.refresh_hz != want.mode.refresh_hz {
            changes.push(TopologyChange::RefreshRate {
                monitor: name.clone(),
                from_hz: have.mode.refresh_hz,
                to_hz: want.mode.refresh_hz,
            });
        }
        if have.position != want.position {
            changes.push(TopologyChange::Position {
                monitor: name.clone(),
                from: have.position,
                to: want.position,
            });
        }
    }

    // Primary is one change about the desktop, not one per monitor.
    let now = current.monitors.iter().find(|m| m.is_primary);
    let next = desired.monitors.iter().find(|m| m.is_primary);
    if let Some(next) = next {
        if now.map(|m| &m.device_path) != Some(&next.device_path) {
            changes.push(TopologyChange::Primary {
                from: now.map(|m| label(&m.device_path, names)),
                to: label(&next.device_path, names),
            });
        }
    }

    changes
}

/// Every rule a plan must satisfy before it is worth handing to Windows.
pub fn validate(
    desired: &TopologySnapshot,
    names: &BTreeMap<String, String>,
) -> Vec<TopologyProblem> {
    let mut problems = Vec::new();
    let active: Vec<&SnapshotEntry> = desired.monitors.iter().filter(|m| m.active).collect();

    if active.is_empty() {
        problems.push(blocking(
            "This plan turns every screen off. There would be nothing left to \
             undo it with.",
        ));
        return problems;
    }

    // Exactly one primary.
    let primaries: Vec<&&SnapshotEntry> = active.iter().filter(|m| m.is_primary).collect();
    match primaries.len() {
        1 => {
            // Windows expresses the whole desktop relative to the primary's
            // top-left, so the primary is the origin by definition. A plan that
            // puts it elsewhere is not refused — every other monitor is shifted
            // to compensate, which is not what was asked for.
            let primary = primaries[0];
            if primary.position != (0, 0) {
                problems.push(blocking(&format!(
                    "{} is the primary screen but is not at the origin. Windows \
                     defines the desktop from the primary's top-left corner, so \
                     it would move everything else instead.",
                    label(&primary.device_path, names)
                )));
            }
        }
        0 => problems.push(blocking(
            "No screen is marked primary. Windows needs one to measure the \
             desktop from.",
        )),
        n => problems.push(blocking(&format!(
            "{n} screens are marked primary. There can only be one.",
        ))),
    }

    if desired.monitors.iter().any(|m| !m.active && m.is_primary) {
        problems.push(blocking(
            "A screen that is switched off cannot be the primary one.",
        ));
    }

    // Overlaps. Windows accepts them and the result is a desktop where part of
    // one screen is unreachable, which looks like a bug in this app.
    for (i, a) in active.iter().enumerate() {
        for b in active.iter().skip(i + 1) {
            if overlaps(rect_of(a), rect_of(b)) {
                problems.push(blocking(&format!(
                    "{} and {} overlap. Windows would accept it and leave part \
                     of one screen unreachable.",
                    label(&a.device_path, names),
                    label(&b.device_path, names)
                )));
            }
        }
    }

    // Contiguity. Windows *silently repositions* a stranded monitor, so a plan
    // that looks applied can be quietly different from what was asked.
    if let Some(stranded) = disconnected(&active) {
        problems.push(blocking(&format!(
            "{} does not touch any other screen. Windows would move it rather \
             than leave a gap in the desktop.",
            stranded
                .iter()
                .map(|m| label(&m.device_path, names))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    problems
}

/// Whether every planned mode is one its output can actually run.
///
/// Separate from `validate` because it needs the driver's mode list, which only
/// the platform layer can produce. An unsupported mode is the one mistake that
/// really does end in a black screen.
pub fn validate_modes(
    desired: &TopologySnapshot,
    available: &[AvailableModes],
    names: &BTreeMap<String, String>,
) -> Vec<TopologyProblem> {
    let mut problems = Vec::new();
    for monitor in desired.monitors.iter().filter(|m| m.active) {
        let Some(modes) = available
            .iter()
            .find(|a| a.device_path == monitor.device_path)
        else {
            // No list is not the same as an empty list. Saying nothing here
            // would be a silent pass on the one check that matters most.
            problems.push(TopologyProblem {
                severity: ProblemSeverity::Warning,
                message: format!(
                    "No mode list for {} — the plan cannot be checked against \
                     what it supports.",
                    label(&monitor.device_path, names)
                ),
            });
            continue;
        };
        if !modes.modes.contains(&monitor.mode) {
            problems.push(blocking(&format!(
                "{} cannot run {}x{} at {} Hz.",
                label(&monitor.device_path, names),
                monitor.mode.resolution.width,
                monitor.mode.resolution.height,
                monitor.mode.refresh_hz
            )));
        }
    }
    problems
}

fn blocking(message: &str) -> TopologyProblem {
    TopologyProblem {
        severity: ProblemSeverity::Blocking,
        message: message.to_string(),
    }
}

fn rect_of(entry: &SnapshotEntry) -> PixelRect {
    PixelRect {
        x: entry.position.0,
        y: entry.position.1,
        width: entry.mode.resolution.width,
        height: entry.mode.resolution.height,
    }
}

/// Strictly overlapping. Touching edges are how a multi-monitor desktop is
/// meant to look, so they are not an overlap.
fn overlaps(a: PixelRect, b: PixelRect) -> bool {
    let ax2 = a.x + a.width as i32;
    let ay2 = a.y + a.height as i32;
    let bx2 = b.x + b.width as i32;
    let by2 = b.y + b.height as i32;
    a.x < bx2 && b.x < ax2 && a.y < by2 && b.y < ay2
}

/// Do two rectangles share any edge length at all?
fn touches(a: PixelRect, b: PixelRect) -> bool {
    let ax2 = a.x + a.width as i32;
    let ay2 = a.y + a.height as i32;
    let bx2 = b.x + b.width as i32;
    let by2 = b.y + b.height as i32;

    let horizontally_aligned = a.y < by2 && b.y < ay2;
    let vertically_aligned = a.x < bx2 && b.x < ax2;

    // A shared vertical edge with any overlap in y, or the reverse. Corner
    // contact alone is not enough: Windows treats a desktop joined only at a
    // corner as disconnected.
    (horizontally_aligned && (ax2 == b.x || bx2 == a.x))
        || (vertically_aligned && (ay2 == b.y || by2 == a.y))
        || overlaps(a, b)
}

/// The monitors not reachable from the first one by walking shared edges.
fn disconnected<'a>(active: &[&'a SnapshotEntry]) -> Option<Vec<&'a SnapshotEntry>> {
    if active.len() < 2 {
        return None;
    }
    let mut reached = vec![false; active.len()];
    reached[0] = true;
    let mut grew = true;
    while grew {
        grew = false;
        for i in 0..active.len() {
            if !reached[i] {
                continue;
            }
            for j in 0..active.len() {
                if reached[j] || !touches(rect_of(active[i]), rect_of(active[j])) {
                    continue;
                }
                reached[j] = true;
                grew = true;
            }
        }
    }
    let stranded: Vec<&SnapshotEntry> = active
        .iter()
        .enumerate()
        .filter(|(i, _)| !reached[*i])
        .map(|(_, m)| *m)
        .collect();
    (!stranded.is_empty()).then_some(stranded)
}

/// A one-line description of a change, for the preview list and the log.
pub fn describe(change: &TopologyChange) -> String {
    match change {
        TopologyChange::Resolution { monitor, from, to } => format!(
            "{monitor}: {}x{} to {}x{}",
            from.width, from.height, to.width, to.height
        ),
        TopologyChange::RefreshRate {
            monitor,
            from_hz,
            to_hz,
        } => format!("{monitor}: {from_hz} Hz to {to_hz} Hz"),
        TopologyChange::Position { monitor, from, to } => format!(
            "{monitor}: moves from {},{} to {},{}",
            from.0, from.1, to.0, to.1
        ),
        TopologyChange::Primary { from, to } => match from {
            Some(from) => format!("primary screen: {from} to {to}"),
            None => format!("primary screen: {to}"),
        },
        TopologyChange::Activated { monitor } => format!("{monitor}: switched on"),
        TopologyChange::Deactivated { monitor } => format!("{monitor}: switched off"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, w: u32, h: u32, x: i32, y: i32, primary: bool) -> SnapshotEntry {
        SnapshotEntry {
            device_path: path.into(),
            active: true,
            mode: DisplayMode {
                resolution: Resolution {
                    width: w,
                    height: h,
                },
                refresh_hz: 120,
                bits_per_pixel: 32,
            },
            position: (x, y),
            is_primary: primary,
        }
    }

    fn snapshot(monitors: Vec<SnapshotEntry>) -> TopologySnapshot {
        TopologySnapshot {
            id: uuid::Uuid::nil(),
            captured_at: String::new(),
            monitors,
        }
    }

    /// The reference triple: 1920x1080 centre at the origin, one either side.
    fn triple() -> TopologySnapshot {
        snapshot(vec![
            entry("left", 1920, 1080, -1920, 0, false),
            entry("centre", 1920, 1080, 0, 0, true),
            entry("right", 1920, 1080, 1920, 0, false),
        ])
    }

    fn names() -> BTreeMap<String, String> {
        [
            ("left".to_string(), "Left".to_string()),
            ("centre".to_string(), "Centre".to_string()),
            ("right".to_string(), "Right".to_string()),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn a_valid_triple_has_nothing_wrong_with_it() {
        assert_eq!(validate(&triple(), &names()), vec![]);
    }

    #[test]
    fn no_change_produces_no_changes() {
        let preview = preview(&triple(), &triple(), &names());
        assert!(preview.is_empty());
        assert!(preview.can_apply());
    }

    #[test]
    fn a_primary_away_from_the_origin_is_refused() {
        // Windows would accept this and shift every other monitor to compensate,
        // which is not what was asked for.
        let mut plan = triple();
        plan.monitors[1].position = (100, 0);
        let problems = validate(&plan, &names());
        assert!(problems.iter().any(|p| p.message.contains("origin")));
        assert!(problems
            .iter()
            .any(|p| p.severity == ProblemSeverity::Blocking));
    }

    #[test]
    fn overlapping_screens_are_refused_but_touching_ones_are_not() {
        let ok = triple();
        assert_eq!(validate(&ok, &names()), vec![]);

        let mut bad = triple();
        bad.monitors[2].position = (1900, 0); // 20 px of overlap
        assert!(validate(&bad, &names())
            .iter()
            .any(|p| p.message.contains("overlap")));
    }

    #[test]
    fn a_screen_touching_nothing_is_refused() {
        // Windows moves a stranded monitor rather than leaving a gap, so the
        // plan that came back would not be the plan that went in.
        let mut plan = triple();
        plan.monitors[2].position = (5000, 0);
        assert!(validate(&plan, &names())
            .iter()
            .any(|p| p.message.contains("does not touch")));
    }

    #[test]
    fn corner_contact_does_not_count_as_touching() {
        // Two screens meeting at a single point leave a desktop Windows treats
        // as disconnected, and it rearranges it.
        let plan = snapshot(vec![
            entry("centre", 1920, 1080, 0, 0, true),
            entry("right", 1920, 1080, 1920, 1080, false),
        ]);
        assert!(validate(&plan, &names())
            .iter()
            .any(|p| p.message.contains("does not touch")));
    }

    #[test]
    fn stacked_screens_touch_along_their_shared_edge() {
        let plan = snapshot(vec![
            entry("centre", 1920, 1080, 0, 0, true),
            entry("below", 1920, 1080, 0, 1080, false),
        ]);
        assert_eq!(validate(&plan, &names()), vec![]);
    }

    #[test]
    fn turning_everything_off_is_refused_before_anything_else() {
        let mut plan = triple();
        for m in &mut plan.monitors {
            m.active = false;
            m.is_primary = false;
        }
        let problems = validate(&plan, &names());
        assert_eq!(problems.len(), 1, "one clear reason, not a cascade");
        assert!(problems[0].message.contains("every screen off"));
    }

    #[test]
    fn two_primaries_and_no_primary_are_both_caught() {
        let mut two = triple();
        two.monitors[0].is_primary = true;
        assert!(validate(&two, &names())
            .iter()
            .any(|p| p.message.contains("only be one")));

        let mut none = triple();
        none.monitors[1].is_primary = false;
        assert!(validate(&none, &names())
            .iter()
            .any(|p| p.message.contains("No screen is marked primary")));
    }

    #[test]
    fn switching_a_screen_off_reports_that_and_nothing_else_about_it() {
        // Its resolution and position stop being interesting the moment it is
        // off, and listing them would bury the change that matters.
        let mut plan = triple();
        plan.monitors[0].active = false;
        plan.monitors[0].mode.resolution = Resolution {
            width: 1280,
            height: 720,
        };
        plan.monitors[0].position = (-1280, 0);

        let changes = diff(&triple(), &plan, &names());
        assert_eq!(
            changes,
            vec![TopologyChange::Deactivated {
                monitor: "Left".into()
            }]
        );
    }

    #[test]
    fn the_primary_moving_is_one_change_not_two() {
        let mut plan = triple();
        plan.monitors[1].is_primary = false;
        plan.monitors[2].is_primary = true;
        let changes = diff(&triple(), &plan, &names());
        assert_eq!(
            changes,
            vec![TopologyChange::Primary {
                from: Some("Centre".into()),
                to: "Right".into(),
            }]
        );
    }

    #[test]
    fn a_mode_the_panel_cannot_run_is_blocking() {
        let mut plan = triple();
        plan.monitors[1].mode.resolution = Resolution {
            width: 5120,
            height: 1440,
        };

        let available = vec![AvailableModes {
            device_path: "centre".into(),
            modes: vec![DisplayMode {
                resolution: Resolution {
                    width: 1920,
                    height: 1080,
                },
                refresh_hz: 120,
                bits_per_pixel: 32,
            }],
        }];

        let problems = validate_modes(&plan, &available, &names());
        assert!(problems
            .iter()
            .any(|p| p.message.contains("cannot run 5120x1440")));
        assert!(problems
            .iter()
            .any(|p| p.severity == ProblemSeverity::Blocking));
    }

    #[test]
    fn a_missing_mode_list_warns_rather_than_passing_silently() {
        // A silent pass on the one check that ends in a black screen would be
        // the worst possible default.
        let problems = validate_modes(&triple(), &[], &names());
        assert_eq!(problems.len(), 3);
        assert!(problems
            .iter()
            .all(|p| p.severity == ProblemSeverity::Warning));
    }

    #[test]
    fn device_paths_are_never_shown_when_a_name_is_known() {
        // Nobody knows which monitor \\?\DISPLAY#SAM7179#5&... is.
        let mut plan = triple();
        plan.monitors[0].position = (-1920, 40);
        for change in diff(&triple(), &plan, &names()) {
            assert!(!describe(&change).contains("left"), "{change:?}");
        }
    }
}
