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
│   └── rigs/                       *.json — one file drives every mock provider
│       ├── triple-1440p-identical.json  the easy case
│       ├── single-ultrawide.json        exercises curvature
│       ├── mismatched-heights.json      dead zones + 33% pitch mismatch
│       ├── mixed-dpi.json               catches a dropped DPI manifest
│       └── hotplug-and-drift.json       all three device states, plus
│                                        DirectInput drift and a dead vJoy feeder
│                                        (scripted hotplug *sequences* land in M5)
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

## IPC contract — changed during milestone 1

I proposed specta + tauri-specta. **Built with ts-rs instead**, plus a small
check script. Recording the change and the reason rather than making it
silently:

specta 2 is still on release candidates, and pinning the IPC contract of a
commercial product to an RC in its first commit is a bad trade. ts-rs 10 is
stable and generates the payload types cleanly (including `rename_all_fields`
on tagged enums, which the step graph needs).

That leaves exactly one gap — ts-rs knows nothing about command *names*, so a
renamed command would compile on both sides and fail only when a user clicks
it. `scripts/check-ipc.mjs` closes it: it parses every `#[tauri::command]` out
of the Rust sources and every `invoke<T>("name")` out of `src/ipc.ts`, and
fails CI if either side has an entry the other lacks. Revisit specta when it
reaches a stable release.

Two rules that come with this:

* `src/bindings/` is generated by `cargo test -p tp-model` and **is committed**.
  CI regenerates and fails on a diff, so a stale contract cannot merge.
* Every type that crosses IPC lives in `tp-model` — including the ones that
  exist only for IPC, like `AppInfo` and `IpcError`. They were originally in the
  Tauri crate, but that crate cannot build on Linux, which would have forced the
  binding-drift check onto a Windows runner for no reason.

The wire format is camelCase throughout (`#[serde(rename_all = "camelCase")]`),
so TypeScript reads like TypeScript.

## Provider traits

Four seams, mocked from milestone 1 so nothing ever hard-depends on real
hardware: `DisplayProvider`, `WindowProvider`, `ProcessProvider`,
`PeripheralProvider`. Selection is a runtime flag (`--mock=fixtures/rigs/alex-rig.json`),
not a cargo feature, so a single build can switch — which makes `--simulate`
usable in the shipped binary for support purposes.
