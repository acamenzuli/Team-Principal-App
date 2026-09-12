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

**The loop is: push, pull, build locally.** A local incremental build takes
about a minute; the Windows CI job takes nine. So CI runs its Linux checks on
every push — formatting, clippy, the platform-neutral tests, the IPC contract,
the frontend build, about two minutes — and the Windows job runs **on request
only**: Actions -> CI -> Run workflow.

That request is also what produces an installer as a run artifact, and what
replaces the **Test channel** release (tag `dev`) that installed copies update
themselves to. The test channel needs the `TAURI_SIGNING_PRIVATE_KEY` secret;
without it the job warns, says so in the run summary, and builds a plain
installer. See `docs/RELEASING.md`.

Ask for that build when there is something to install — not after every
change.

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

CI runs clippy on `stable`, which gains lints over time. Run `rustup update
stable` before trusting a local clippy pass — an older toolchain passing means
nothing about what CI will say.

`cargo check --all-targets` compiles `src-tauri`'s tests without running them,
and they can only run on Windows. A green cross-check therefore says nothing
about whether they pass, which is exactly how a broken test reached CI twice
in a row. This is the practical reason for the rule below about where pure
logic lives: anything that can be got wrong should be somewhere it can be
run.


Run against fixtures instead of hardware:

```powershell
cargo run -p team-principal -- --mock fixtures/rigs/mismatched-heights.json
```

From a non-Windows machine, the useful subset:

```bash
cargo test -p tp-model -p tp-geometry -p tp-edid
cargo check --workspace --target x86_64-pc-windows-msvc --all-targets
```

## Rules that are not negotiable

- **`tp-geometry` never depends on `windows`, `tauri`, or any game adapter.**
  Its manifest enforces this. Adapters consume its output; they never do their
  own trigonometry.
- **Pure logic goes in a crate that builds on any platform.** `src-tauri` cannot
  be compiled or tested off Windows, so anything testable — EDID parsing,
  registry-key derivation, layout math — lives in `tp-model`, `tp-geometry` or
  `tp-edid`. Only the platform call itself stays behind.
- **EDID never supplies resolution.** Its timing fields cap at 4095 px and
  655.35 MHz, so a 5120x1440 panel cannot express its native mode there.
  Windows owns resolution; EDID owns physical size and identity.
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
- **No dead IPC surface.** Every `#[tauri::command]` is reachable from the UI.
  A command with no caller is a claim the app does something it does not, and
  `scripts/check-ipc.mjs` only checks that both sides agree — not that anything
  uses them.

## Milestones

Stop at the end of each for hardware testing. Current: **11 built, none hardware-tested**.

1. ✅ Skeleton — Tauri, DPI manifest, logging, typed IPC, mock providers, CI
2. ✅ Display enumeration — CCD, EDID from the registry, dead regions
3. ✅ Geometry engine — rig solver, FOV, spans, frusta, bezels, best fit
4. ✅ Screen Setup — rig form, auto-fill from EDID, live numbers, rig views
5. ✅ Peripherals — HID, DirectInput ordering, event hotplug, catalog,
   status rules, debounce, live axis and button monitor
6. ✅ Window control — find, strip chrome, place, verify by read-back, watchdog
7. ✅ Launch orchestration — scheduler, gates, discovery, parallel executor,
   Job Object teardown
8. ✅ The "Let's race" flow — profiles, editor, self-healing checks, inline fix
   actions, the ready gate and its second-click launch
9. ✅ Display control — mode and layout planning, preview, staged apply with
   one commit, read-back, confirm-or-revert countdown, panic hotkey, snapshots
10. ✅ Adapters wave one — comment-preserving INI editor, backup store with
    one-click restore, preview-before-apply diff, iRacing and Assetto Corsa
11. ✅ Polish and packaging — first-run wizard, diagnostics bundle, licensing
    seam, landing page, splash, startup options, updater. Code signing is the
    last piece and is a purchase: see `docs/RELEASING.md`.
