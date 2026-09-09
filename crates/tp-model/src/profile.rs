//! Profiles and the launch step graph. See docs/design/0004-profile-schema.md.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::device::DeviceRef;
use crate::rig::SessionMode;
use crate::units::PixelRect;

pub const PROFILE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub schema_version: u32,
    pub id: Uuid,
    pub name: String,
    pub game: GameRef,
    pub rig: RigBinding,
    pub session_mode: SessionMode,
    pub window_plan: WindowPlan,
    pub peripherals: Vec<PeripheralRequirement>,
    /// Utilities to have running before the game starts.
    #[serde(default)]
    pub utilities: Vec<UtilitySpec>,
    /// One graph. Each node declares its phase; the executor enforces that
    /// every preflight node is terminal before any launch node starts. That
    /// enforcement *is* the ready gate.
    pub steps: Vec<StepSpec>,
    pub teardown: TeardownPolicy,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct GameRef {
    pub adapter_id: String,
    pub install_path: Option<String>,
    pub launch: LaunchMethod,
    /// Which launcher this came from, kept as its own field rather than read
    /// back out of `launch`. A profile outlives the install it was made from,
    /// and "you had this on Steam" is still true after the game is gone.
    #[serde(default)]
    pub platform: Platform,
    /// Cover art on disk, found once and remembered.
    ///
    /// A path rather than the image, so a profile stays a small readable JSON
    /// file. Steam already has the artwork cached locally — no download, no
    /// third-party service, nothing that stops working offline.
    #[serde(default)]
    pub art_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Steam,
    Epic,
    /// Added by hand, or a launcher this build does not know.
    #[default]
    Other,
}

impl Platform {
    pub fn label(self) -> &'static str {
        match self {
            Platform::Steam => "Steam",
            Platform::Epic => "Epic",
            Platform::Other => "Other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum LaunchMethod {
    Executable {
        path: String,
        args: Vec<String>,
        working_dir: Option<String>,
    },
    /// `steam://rungameid/<appid>`. No PID comes back, so the launcher polls for
    /// the expected executable and correlates afterwards.
    Steam {
        app_id: String,
    },
    Epic {
        app_name: String,
    },
    /// `shell:appsFolder\<PFN>!App`
    Uwp {
        package_family_name: String,
    },
    Uri {
        uri: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct RigBinding {
    pub rig_id: Uuid,
    pub computed_against_revision: u64,
    /// Exactly the numbers last written to this game, keyed by a stable field
    /// name (`"center.h_fov_deg"`, `"bezel.inner_px"`). Stored so the
    /// recompute-and-propagate diff is exact and works offline, with no need to
    /// reconstruct the previous rig. A flat name->number map rather than free
    /// JSON so the diff is mechanical and the TypeScript type is honest.
    pub derived_snapshot: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct WindowPlan {
    pub target: WindowTarget,
    pub rect: RectSource,
    /// Required, with no default. A rectangle that does not say whether it is
    /// the client area or the outer window is ambiguous by exactly the border
    /// width, and a few pixels of horizontal error puts the horizon seam in the
    /// wrong place on every screen of a triple.
    pub rect_means: RectMeans,
    pub borderless: bool,
    pub always_on_top: bool,
    pub hide_taskbar: bool,
    pub watchdog: WatchdogPolicy,
    /// Place the window automatically when the game starts.
    ///
    /// Off until the geometry has actually been proven once by hand, because
    /// an automatic placement that is wrong is far more annoying than no
    /// automatic placement: it happens every launch and it is not obvious what
    /// did it.
    #[serde(default)]
    pub auto_apply: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum RectMeans {
    ClientArea,
    OuterWindow,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum RectSource {
    /// Computed from the rig for this session mode. The normal case.
    FromGeometry,
    Explicit {
        rect: PixelRect,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct WindowTarget {
    pub exe_name: Option<String>,
    pub window_class: Option<String>,
    pub title_regex: Option<String>,
    /// Splash and loader filter. Never "the first window that appears".
    pub min_size: (u32, u32),
    pub require_visible: bool,
    /// Millisecond durations cross into TypeScript as `number`, not `bigint`.
    /// A u64 arrives as a bigint that cannot be compared with or divided by a
    /// plain number without a cast at every call site, and no duration this app
    /// expresses comes anywhere near the precision limit. The same
    /// `#[ts(type = ...)]` appears on every `_ms` field below for that reason.
    #[ts(type = "number")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogPolicy {
    /// Re-apply N times over `reapply_window_ms` after launch, because many
    /// sims reset their own window when the render device initialises.
    pub reapply_count: u32,
    #[ts(type = "number")]
    pub reapply_window_ms: u64,
    /// Ongoing drift correction. `None` disables it.
    #[ts(type = "number | null")]
    pub drift_check_interval_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub stop_after_stable_ms: Option<u64>,
}

impl Default for WatchdogPolicy {
    fn default() -> Self {
        Self {
            reapply_count: 5,
            reapply_window_ms: 15_000,
            drift_check_interval_ms: Some(2_000),
            stop_after_stable_ms: Some(60_000),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PeripheralRequirement {
    pub device: DeviceRef,
    pub necessity: Necessity,
    pub expect_dinput_slot: Option<u32>,
    pub expect_instance_guid: Option<String>,
    pub vendor_process: Option<ProcessRequirement>,
    pub vjoy: Option<VJoyRequirement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum Necessity {
    /// Blocks launch when disconnected.
    Required,
    /// Warns only.
    Optional,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRequirement {
    pub exe_name: String,
    pub service_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct VJoyRequirement {
    pub device_id: u8,
    pub feeder: Option<ProcessRequirement>,
}

/// A utility the profile wants running before a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct UtilitySpec {
    pub label: String,
    pub exe_path: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// How to know it is *ready*, not merely running. This is the field that
    /// replaces the hardcoded sleep every other launcher relies on.
    pub ready_when: ReadinessGate,
    #[ts(type = "number")]
    pub timeout_ms: u64,
    /// Whether failing to start it blocks the launch.
    pub required: bool,
    /// Labels of utilities that must be ready first. Names rather than ids, so
    /// reordering the list does not break the chain.
    #[serde(default)]
    pub after: Vec<String>,
}

// ---------------------------------------------------------------- step graph

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
#[ts(type = "number")]
#[serde(transparent)]
pub struct StepId(pub u32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StepSpec {
    pub id: StepId,
    pub label: String,
    pub phase: Phase,
    /// Steps with no dependency between them run in parallel. A preflight that
    /// insists on being sequential is a preflight nobody uses.
    pub depends_on: Vec<StepId>,
    pub action: StepAction,
    pub gate: ReadinessGate,
    #[ts(type = "number")]
    pub timeout_ms: u64,
    pub severity: Severity,
    pub fix: Option<FixAction>,
    /// Minimum time a row stays visible, so an instant pass is readable rather
    /// than a flicker. Never used to pad the total run.
    #[ts(type = "number")]
    pub min_visible_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Preflight,
    Launch,
    Teardown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Blocks the ready gate.
    Fatal,
    /// Ready with warnings.
    Warning,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum StepAction {
    /// Start only if not already running. Reports AlreadyRunning vs Started,
    /// which is what makes the honesty rule mechanical rather than a
    /// copywriting convention.
    EnsureProcess {
        exe_path: String,
        args: Vec<String>,
    },
    StartProcess {
        exe_path: String,
        args: Vec<String>,
    },
    CheckPeripheral {
        device: DeviceRef,
    },
    CheckDisplayTopology,
    ApplyDisplaySnapshot {
        snapshot_id: Uuid,
    },
    CheckGameInstalled,
    CheckConfigWritable,
    CheckNotAlreadyRunning,
    ApplyAdapter {
        adapter_id: String,
    },
    LaunchGame,
    ApplyWindowGeometry,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum ReadinessGate {
    Immediate,
    Delay {
        #[ts(type = "number")]
        ms: u64,
    },
    ProcessExists {
        exe: String,
    },
    /// Nothing by this name is running. The check that stops a second copy of
    /// a sim being launched over the one already open.
    ProcessAbsent {
        exe: String,
    },
    /// WaitForInputIdle — the process has drained its startup queue.
    InputIdle {
        #[ts(type = "number")]
        timeout_ms: u64,
    },
    WindowExists {
        target: WindowTarget,
    },
    NamedMutex {
        name: String,
    },
    NamedEvent {
        name: String,
    },
    TcpPort {
        host: String,
        port: u16,
    },
    FileAppears {
        path: String,
    },
    PeripheralConnected {
        device: DeviceRef,
    },
    /// Composition is what makes vJoy expressible without a sleep: driver
    /// present AND feeder running AND device enumerated AND DirectInput sees it.
    All {
        gates: Vec<ReadinessGate>,
    },
    Any {
        gates: Vec<ReadinessGate>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum FixAction {
    Start,
    Retry,
    Skip,
    OpenSettings { section: String },
    OpenUrl { url: String },
}

// ------------------------------------------------------------------ runtime

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Passed,
    Warning,
    Failed,
    Skipped,
}

/// What the app actually did, as opposed to what it attempted. The UI renders
/// "Already running" from this rather than from a hand-written string, so it
/// cannot claim credit for work it did not do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ActionTaken {
    Started,
    AlreadyRunning,
    NoActionNeeded,
    Skipped,
    NotAttempted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StepOutcome {
    pub id: StepId,
    pub status: StepStatus,
    pub action_taken: ActionTaken,
    /// The one-line live detail shown on the row.
    pub detail: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    #[ts(type = "number | null")]
    pub elapsed_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ReadyState {
    Running,
    Ready,
    ReadyWithWarnings,
    Blocked,
    /// The user pressed Launch and the launch-phase steps are running.
    Launching,
    /// The game is up.
    Racing,
    /// The launch itself failed, which is a different thing from a preflight
    /// that blocked — the checks passed and the start did not.
    LaunchFailed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct TeardownPolicy {
    pub restore_display: bool,
    pub restore_configs: bool,
    pub close_utilities: bool,
    /// Whether utilities stay running when a preflight is cancelled part-way.
    pub keep_utilities_on_cancel: bool,
}

impl Default for TeardownPolicy {
    fn default() -> Self {
        Self {
            restore_display: true,
            restore_configs: true,
            close_utilities: false,
            keep_utilities_on_cancel: true,
        }
    }
}
