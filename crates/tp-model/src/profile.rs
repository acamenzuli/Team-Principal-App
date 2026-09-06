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
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogPolicy {
    /// Re-apply N times over `reapply_window_ms` after launch, because many
    /// sims reset their own window when the render device initialises.
    pub reapply_count: u32,
    pub reapply_window_ms: u64,
    /// Ongoing drift correction. `None` disables it.
    pub drift_check_interval_ms: Option<u64>,
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
    pub timeout_ms: u64,
    pub severity: Severity,
    pub fix: Option<FixAction>,
    /// Minimum time a row stays visible, so an instant pass is readable rather
    /// than a flicker. Never used to pad the total run.
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
        ms: u64,
    },
    ProcessExists {
        exe: String,
    },
    /// WaitForInputIdle — the process has drained its startup queue.
    InputIdle {
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
