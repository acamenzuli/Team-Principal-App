# 0004 — Profile schema, step graph, storage

Status: **proposed**, awaiting sign-off.

## Profile

```rust
pub struct Profile {
    pub schema_version: u32,
    pub id: Uuid,
    pub name: String,                       // "ACC — centre only"
    pub game: GameRef,
    pub rig: RigBinding,
    pub session_mode: SessionMode,
    pub display_plan: DisplayPlan,
    pub window_plan: WindowPlan,
    pub peripherals: Vec<PeripheralRequirement>,
    pub steps: Vec<StepSpec>,               // one graph; each node declares its phase
    pub teardown: TeardownPolicy,
    pub adapter_overrides: AdapterOverrides,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct RigBinding {
    pub rig_id: Uuid,
    pub computed_against_revision: u64,
    pub derived_snapshot: DerivedSnapshot,  // exactly the numbers last written; drives the diff
}

pub struct WindowPlan {
    pub target: WindowTarget,
    pub rect: RectSource,                   // FromGeometry | Explicit(PixelRect)
    pub rect_means: RectMeans,              // ClientArea | OuterWindow  <-- must be explicit
    pub borderless: bool,
    pub always_on_top: bool,
    pub hide_taskbar: bool,
    pub watchdog: WatchdogPolicy,
}

pub struct WindowTarget {
    pub exe_name: Option<String>,
    pub window_class: Option<String>,
    pub title_regex: Option<String>,
    pub min_size_px: (u32, u32),            // splash/loader filter
    pub require_styles: WindowStyleFilter,  // filter by style, not by "first window seen"
    pub timeout_ms: u64,
}

pub struct WatchdogPolicy {
    pub reapply_count: u32,                 // N re-applies
    pub reapply_window_ms: u64,             // over M ms after launch
    pub drift_check_interval_ms: Option<u64>,   // None = no ongoing drift correction
    pub stop_after_stable_ms: Option<u64>,
}
```

`rect_means` is a required field with no default. A profile that does not say
whether its rectangle is the client area or the outer window is ambiguous by
exactly the border width, and on a triple setup a few pixels of horizontal error
puts the horizon seam in the wrong place on every screen.

## Step graph

One graph, not two lists. Each node declares which phase it belongs to; the
executor enforces that every `Preflight` node reaches a terminal state before
any `Launch` node starts. That is the gate.

```rust
pub struct StepSpec {
    pub id: StepId,
    pub label: String,                  // "Start SimHub"
    pub phase: Phase,                   // Preflight | Launch | Teardown
    pub depends_on: Vec<StepId>,
    pub action: StepAction,
    pub gate: ReadinessGate,
    pub timeout_ms: u64,
    pub severity: Severity,             // Fatal | Warning
    pub fix: Option<FixAction>,         // Start | Retry | Skip | OpenSettings | OpenUrl
    pub min_visible_ms: u64,            // default 350
}

pub enum StepAction {
    EnsureProcess { .. },   // start only if absent -> reports AlreadyRunning vs Started
    StartProcess  { .. },   // always start
    CheckPeripheral(DeviceRef),
    CheckDisplayTopology,
    ApplyDisplaySnapshot(SnapshotId),
    CheckGameInstalled, CheckConfigWritable, CheckNotAlreadyRunning,
    ApplyAdapter { adapter_id: String },
    LaunchGame,
    ApplyWindowGeometry,
}

pub enum ReadinessGate {
    Immediate,
    Delay { ms: u64 },
    ProcessExists { exe: String },
    ProcessAbsent { exe: String },          // "not already running", checked rather than assumed
    InputIdle { timeout_ms: u64 },          // WaitForInputIdle
    WindowExists(WindowTarget),
    NamedMutex { name: String },
    NamedEvent { name: String },
    TcpPort { host: String, port: u16 },
    FileAppears { path: PathBuf },
    PeripheralConnected(DeviceRef),
    All(Vec<ReadinessGate>),
    Any(Vec<ReadinessGate>),
}
```

`All` / `Any` composition is what makes vJoy expressible without a sleep:
*driver present* **and** *feeder process running* **and** *the vJoy device
enumerated at the HID layer* **and** *DirectInput lists it*.

`EnsureProcess` vs `StartProcess` is what makes the honesty rule mechanical
rather than a copywriting convention. Every step records:

```rust
pub struct StepOutcome {
    pub status: StepStatus,             // Pending|Running|Passed|Warning|Failed|Skipped
    pub action_taken: ActionTaken,      // Started | AlreadyRunning | NoActionNeeded | Skipped
    pub detail: String,
    pub started_at: Option<Instant>,
    pub finished_at: Option<Instant>,
}
```

The UI renders "Already running" from `ActionTaken::AlreadyRunning`. It cannot
claim credit it does not have, because the string is derived from what happened.

The executor emits `StepOutcome` changes as Tauri events. The preflight UI is a
pure view over that stream — it never sequences anything, and re-running a
subgraph is a backend concern. Self-healing works the same way: peripheral and
process watchers feed the executor, which re-evaluates only the affected nodes
and their dependents. No Retry button is required for a condition you just fixed
by hand.

### What self-healing may and may not touch

Only failed steps are re-checked, and only their *gate*. Re-running a step's
action would restart SimHub because a pedal check failed, which is exactly the
behaviour that makes a preflight useless. Two further exclusions:

* **Skipped is never healed.** A skipped step never ran — something it needed
  failed, or the user overrode it. Passing it because its gate happens to be
  open would claim a result nothing produced.
* **`Immediate` is never healed.** A step gated on `Immediate` failed in its
  action, and `Immediate` is open by definition; re-checking it would turn every
  failed action green a moment later while nothing had changed. "The game is
  installed" going green over a folder that is still missing is precisely the
  lie this app exists not to tell.

A retry the user *presses* does re-run the action, because that is what they
asked for. It resets the step and everything downstream, never anything
upstream.

## The launch gate

`ReadyState` is what the panel under the checklist says:

```rust
pub enum ReadyState {
    Running, Ready, ReadyWithWarnings, Blocked,   // preflight
    Launching, Racing, LaunchFailed,              // after the second click
}
```

The first four are computed by the scheduler from the preflight phase alone. The
last three are published by the executor around the launch phase, and
`LaunchFailed` is deliberately distinct from `Blocked`: the checks passing and
the game not starting is a different problem from the checks not passing.

Launching is a second, deliberate press. The executor does not run the launch
phase when preflight settles; it waits for `Request::Launch`. The scheduler's
phase gate is the belt to that pair of braces — even a driver that asked early
would be refused while a fatal preflight step is failing.

"Race anyway" skips whatever is still failing so the gate opens. Skipped, never
passed: the checklist keeps saying the check was overridden rather than
satisfied, and `ActionTaken::Skipped` is what the row's text is derived from.

## Utilities

```rust
pub struct UtilitySpec {
    pub label: String,              // "SimHub"
    pub exe_path: String,
    pub args: Vec<String>,
    pub ready_when: ReadinessGate,  // the field that replaces the hardcoded sleep
    pub timeout_ms: u64,
    pub required: bool,             // Fatal vs Warning
    pub after: Vec<String>,         // labels, so reordering the list cannot break the chain
}
```

`after` names other utilities by label rather than by id. A stale name is
dropped when the graph is built, not carried through as a dangling dependency —
one removed utility must not make the whole profile unrunnable.

`tp_model::plan::build_steps` turns a profile into the graph. It lives in the
model rather than in the executor so that the shape of a run — what depends on
what, what is fatal, what merely warns — is tested without starting a single
process.

## Peripheral requirement

```rust
pub struct PeripheralRequirement {
    pub device: DeviceRef,
    pub necessity: Necessity,               // Required | Optional
    pub expect_dinput_slot: Option<u32>,
    pub expect_instance_guid: Option<Guid>, // ordering-drift detection
    pub vendor_process: Option<ProcessRequirement>,
    pub vjoy: Option<VJoyBinding>,          // { device_id, feeder: Option<ProcessRequirement> }
}

pub struct DeviceRef {
    pub vid: u16,
    pub pid: u16,
    pub serial: Option<String>,             // preferred disambiguator
    pub instance_path: Option<String>,      // fallback for identical un-serialled devices
    pub display_name: String,               // user-renameable, always
}
```

## Storage

```
%APPDATA%\Team Principal\
    rigs\<uuid>.json
    profiles\<uuid>.json
    snapshots\<uuid>.json          display topology snapshots
    backups\<iso8601>\<title>\...  every file the app writes, backed up first
    catalog\devices.json           VID/PID -> friendly name, updatable out-of-band
    logs\                          rotating
    state.json                     crash-recovery marker: what was mid-flight
```

JSON, not TOML — the structures are deeply nested arrays, JSON diffs cleanly in
git for support purposes, and `serde_json` round-trips f64 exactly.

`state.json` is written before any mutation and cleared after teardown
completes. If the app finds one on startup, a crash left the machine
half-configured: it offers to run the pending teardown before doing anything
else. This is what makes "teardown must also run correctly after a crash"
actually true.

## Migrations

Every persisted file carries `schema_version`. Migrations are pure functions
`fn v1_to_v2(Value) -> Result<Value>`, chained, each with a golden-file test
(`fixtures/migrations/profile.v1.json` → `profile.v2.expected.json`). The
loader never guesses: an unknown future version is refused with a clear message
rather than partially parsed.

## Licensing seam

```rust
pub trait LicenseProvider {
    fn verify(&self) -> LicenseState;
}
pub enum LicenseState { Licensed { major_version: u32 }, Trial { days_left: u32 }, Unlicensed }
```

v1 ships `AlwaysLicensed`. Feature checks call the trait from day one so the
call sites exist. The licence file schema includes `purchased_major_version`
from the start even though nothing enforces it — that field is the only way to
fund ongoing adapter maintenance without a subscription, and it cannot be added
retroactively to licences already issued.

No subscription plumbing: no periodic revalidation, no grace countdown, no
recurring auth, no server contact after activation. Ed25519 signature verified
against a public key in the binary. Vendor integration (Lemon Squeezy / Paddle /
Keygen) sits behind a second trait so no vendor name appears in core code.
