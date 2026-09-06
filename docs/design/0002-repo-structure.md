# 0002 — Repo structure

Status: **proposed**, awaiting sign-off.

## One change from your outline, and why

You put `geometry/` inside `src-tauri/src/`. I want it as a **separate crate in a
Cargo workspace** instead:

```
crates/tp-geometry/     pure math
crates/tp-model/        serde types + TS bindings
src-tauri/              the app
```

Three reasons, all practical:

1. **The "no Win32 dependency" rule becomes a compiler error rather than a code
   review note.** `tp-geometry`'s `Cargo.toml` simply does not list `windows`.
   It cannot drift.
2. **Test cycle time.** `cargo test -p tp-geometry` builds a few thousand lines
   of pure math in about a second. As a module inside `src-tauri` it would drag
   the whole Tauri dependency tree along on every run, and milestone 3 is
   nothing but running those tests over and over.
3. **`--simulate` and the fake-game harness** can link the math without linking
   the app.

Same argument applies to `tp-model`: the types that cross IPC live in a crate
that depends on neither `windows` nor `tauri`, so the TypeScript generator is a
fast, standalone build step.

## Layout

```
Team-Principal-App/
├── .github/workflows/ci.yml        fmt, clippy -D warnings, test, tsc, bindings drift check
├── docs/
│   ├── design/                     these documents
│   └── adapters/<title>.md         per-title verification record
├── fixtures/
│   ├── rigs/                       *.json — mock DisplayProvider inputs
│   │   ├── alex-rig.json           your rig, once confirmed
│   │   ├── triple-1440p-identical.json
│   │   ├── single-ultrawide.json
│   │   ├── mismatched-heights.json     exercises dead zones
│   │   └── mixed-dpi.json
│   └── devices/                    *.json — mock PeripheralProvider scripts
│       ├── steady-state.json
│       ├── hotplug-bounce.json         arrives, bounces twice, settles
│       ├── drop-mid-session.json
│       └── dinput-order-swap.json      two identical devices swap slots
├── crates/
│   ├── tp-model/                   Rig, Profile, Step, Device, enums. serde + specta.
│   │                               No windows, no tauri, no game adapters.
│   ├── tp-geometry/                Rig -> RigSolution. Depends on tp-model only.
│   └── tp-fakegame/                test binary: opens a window, waits, resizes itself
├── src-tauri/
│   ├── app.manifest                PerMonitorV2 + longPathAware
│   ├── build.rs                    embeds manifest, emits TS bindings
│   ├── tauri.conf.json
│   └── src/
│       ├── main.rs  lib.rs
│       ├── ipc/                    Tauri commands, typed error surface, event channels
│       ├── profile/                load/save, schema_version, migrations
│       ├── display/
│       │   ├── provider.rs         trait DisplayProvider
│       │   ├── win32/              CCD, EnumDisplay*, ChangeDisplaySettingsEx
│       │   ├── edid.rs             raw EDID parse (mm precision, not WMI cm)
│       │   ├── snapshot.rs         capture / restore / confirm-or-revert
│       │   └── mock.rs             fixture-driven
│       ├── peripherals/
│       │   ├── provider.rs         trait PeripheralProvider
│       │   ├── hid.rs  dinput.rs  rawinput.rs  vjoy.rs
│       │   ├── notify.rs           CM_Register_Notification
│       │   ├── debounce.rs         750 ms state machine
│       │   ├── catalog.rs          VID/PID -> friendly name
│       │   └── mock.rs             scripted hotplug sequences
│       ├── window/                 find, restyle, position, verify, watchdog
│       ├── launcher/
│       │   ├── graph.rs            dependency graph + parallel executor
│       │   ├── gates.rs            readiness gate implementations
│       │   ├── discovery.rs        Steam libraryfolders.vdf / appmanifest, Epic manifests
│       │   ├── job.rs              Job Object so nothing is orphaned
│       │   └── teardown.rs         reverse graph, crash-recovery on next start
│       ├── adapters/               one file per title, trait objects
│       ├── license/                LicenseProvider trait, AlwaysLicensed in v1
│       └── logging/                rotating file, diagnostics bundle
└── src/                            React
    ├── screen/    devices/    layout/    profile/    dashboard/
    ├── bindings.ts                 GENERATED — never hand-edited
    └── design/tokens.ts            the token system, written before any screen
```

## IPC contract

**specta + tauri-specta**, not ts-rs. ts-rs generates struct shapes only;
tauri-specta also generates the command signatures and the event payload types.
The drift that actually bites is a renamed command or a changed event payload,
and only tauri-specta catches those. CI regenerates `src/bindings.ts` and fails
if it differs from what is committed.

## Provider traits

Four seams, mocked from milestone 1 so nothing ever hard-depends on real
hardware: `DisplayProvider`, `WindowProvider`, `ProcessProvider`,
`PeripheralProvider`. Selection is a runtime flag (`--mock=fixtures/rigs/alex-rig.json`),
not a cargo feature, so a single build can switch — which makes `--simulate`
usable in the shipped binary for support purposes.
