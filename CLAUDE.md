# Team Principal — working notes

Windows 11 desktop app. Tauri v2, React + TypeScript frontend, Rust backend.
Design documents in `docs/design/` are the contract; read `0003-rig-model.md`
before touching anything geometric.

## Where work happens

**Build and run locally on the Windows machine.** Push to GitHub to sync.

The app is inseparable from Win32: displays, HID, DirectInput, window
manipulation. A Linux container can type-check the Windows target but cannot
link it, run it, or see a single piece of real hardware. WSL cannot either.
Setup: `docs/DEV-SETUP.md`.

Branch: `claude/sim-racing-launcher-display-i60v81`.

CI also builds a Windows installer on every push and uploads it as a run
artifact, so the app can be put on the rig without a toolchain there. Actions
tab -> newest run -> Artifacts -> `team-principal-installer`.

## Commands

```powershell
npm run tauri dev            # run the app
npm run build                # typecheck + build frontend
cargo test --workspace       # everything (Windows only — includes the DPI assertion)
cargo test -p tp-geometry    # the math, ~1s
node scripts/check-ipc.mjs   # command surface has not drifted
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Run against fixtures instead of hardware:

```powershell
cargo run -p team-principal -- --mock fixtures/rigs/mismatched-heights.json
```

From a non-Windows machine, the useful subset:

```bash
cargo test -p tp-model -p tp-geometry
cargo check --workspace --target x86_64-pc-windows-msvc --all-targets
```

## Rules that are not negotiable

- **`tp-geometry` never depends on `windows`, `tauri`, or any game adapter.**
  Its manifest enforces this. Adapters consume its output; they never do their
  own trigonometry.
- **All geometry is physical pixels and millimetres.** Never DIPs. The process
  is Per-Monitor V2 DPI aware and a test asserts it.
- **`src/bindings/` is generated**, by `cargo test -p tp-model`. Never hand-edit.
  CI fails on a stale diff.
- **No shelling out to third-party binaries.** No MultiMonitorTool, SRWE,
  nircmd. Everything native. This is a product being sold.
- **No injection, no memory reads, no render hooks.** Window positioning via
  `SetWindowPos` and config-file editing only. See the README.
- **Preview before apply.** Every file the app writes is backed up first and
  diffed to the user before anything changes.
- **Never claim credit for work not done.** `ActionTaken::AlreadyRunning` exists
  so the UI cannot say "Started" when it started nothing.
- **Win32 results are verified by read-back**, never by return code. UIPI makes
  silent failure the normal case.

## Milestones

Stop at the end of each for hardware testing. Current: **1 complete**.

1. ✅ Skeleton — Tauri, DPI manifest, logging, typed IPC, mock providers, CI
2. Display enumeration (read-only, EDID physical size)
3. Geometry engine in full
4. Screen Setup UI
5. Peripherals
6. Window control
7. Launch orchestration
8. The "Let's race" flow
9. Display control (topology, confirm-or-revert, panic hotkey)
10. Adapters wave one
11. Polish and packaging
