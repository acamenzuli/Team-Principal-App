//! Shared model types for Team Principal.
//!
//! This crate is the IPC contract between the Rust backend and the React
//! frontend. It deliberately depends on neither `windows` nor `tauri`, which
//! means:
//!
//! * the TypeScript generator is a fast standalone build;
//! * these types compile and test on any platform, so CI can check them
//!   without a Windows runner;
//! * nothing platform-specific can leak into the wire format by accident.

pub mod adapter;
pub mod apps;
pub mod axes;
pub mod catalog;
pub mod catalog_games;
pub mod datauri;
pub mod detect;
pub mod device;
pub mod dinput;
pub mod display;
pub mod fixture;
pub mod graph;
pub mod ini;
pub mod ipc;
pub mod modes;
pub mod plan;
pub mod preferences;
pub mod presence;
pub mod profile;
pub mod restart;
pub mod rig;
pub mod topology;
pub mod units;
pub mod vdf;
pub mod window;
pub mod xml;

pub use adapter::{
    apply as apply_edits, catalog as adapter_catalog, diff as diff_edits,
    for_game as adapter_for_game, inspect as inspect_config, plan as adapter_plan, AdapterInfo,
    AdapterPlan, AdapterPreview, AppliedAdapterInfo, BackupInfo, Confidence, ConfigGroup,
    ConfigInspection, Edit, FileDiff, FileEdits, MissingKey, ValueChange,
};
pub use apps::{app_from_path, offerable, pick_exe, tidy as tidy_apps, RunningApp};
pub use axes::{axis_name, hat_degrees, is_axis, normalise, normalise_unipolar};
pub use catalog::{Catalog, CatalogEntry, DeviceKind};
pub use catalog_games::{ConfigFormat, Derived, GameEntry, Unit};
pub use datauri::{decode_image_data_uri, DecodedImage};
pub use detect::rig_from_monitors;
pub use device::*;
pub use device::{input_alias_key, InputKind};
pub use dinput::{format_guid, vid_pid_from_product_guid};
pub use display::*;
pub use fixture::Fixture;
pub use graph::{GraphError, Scheduler, StepView};
pub use ini::{Ini, KeyError as IniError};
pub use ipc::*;
pub use modes::{mode_of, sections, without_other_mode, DeviceMode, DeviceSection};
pub use plan::{build_steps, expected_exe, launch_exe};
pub use preferences::*;
pub use presence::{
    debounce, detect_drift, device_key, transitions, Debouncer, DeviceEvent, Observation,
};
pub use profile::*;
pub use restart::{node_to_restart, ReconnectOutcome, RestartResult};
pub use rig::*;
pub use topology::{
    describe as describe_topology_change, preview as preview_topology,
    validate as validate_topology, validate_modes, AvailableModes, ProblemSeverity, TopologyChange,
    TopologyPreview, TopologyProblem,
};
pub use units::*;
pub use window::{
    borderless_ex_style, borderless_style, capture, client_from_outer, has_drifted, is_borderless,
    outer_from_client, pick_capture, pick_target, title_matches, CaptureChoice, CapturedLayout,
    FrameInsets, WindowCandidate, WindowMatch,
};
pub use xml::{Xml, XmlError};
