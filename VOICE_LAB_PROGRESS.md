# Voice Lab — progress

The running record for the Voice Lab work. The brief is `VOICE_LAB_UPDATE.md`
on the owner's desktop (not in the repo); the design is
`docs/design/0009-voice-lab.md`. A fresh session resumes from those two plus
this file.

Branch: `feature/voice-lab`. Nothing merged, tagged or published.

## Status

| Phase | State |
| --- | --- |
| 0 — Recon | **done** (2026-09-21) — questions asked, awaiting answers |
| 1 — Service and bare tab | not started |
| 2 — Guided recorder and tone mapping | not started |
| 3 — Full generation and review | not started |
| 4 — Optional-module installer | not started |
| 5 — Engine bake-off | not started |
| 6 — Extras | not started |
| 7 — Release build and handoff | not started |

## What Phase 0 found

### The repo (confirmed, matches the brief)

- Tauri v2, React 18 + TypeScript (Vite), Rust workspace. Windows only.
- **Tabs** are a `View` union and a `Tab` button in `src/App.tsx`; each tab
  is a folder under `src/` with its own `.css`, global class names, shared
  primitives copied rather than imported (`Group`, `Row`, `Toggle` in
  `Settings.tsx`; `.btn`, `.note`, `.glass`, `.badge` reused everywhere).
- **Tokens** in `src/design/tokens.css`; the rule is glass on chrome, solid
  under data (`docs/design/0006-visual-language.md`).
- **Settings** are one `Preferences` struct in `crates/tp-model/src/preferences.rs`
  with `#[serde(default)]` on every field added since v1, saved atomically by
  `src-tauri/src/settings/mod.rs`. Adding a `voiceLab` section with a default
  is backwards compatible and needs no migration; `schema_version` stays 1.
- **Process management** is `src-tauri/src/launcher/`: `process.rs` spawns,
  `job.rs` is a Job Object that kills children with the app, `gates.rs`
  polls `ReadinessGate`s (including `TcpPort`), `run.rs` publishes
  `launch://state` (`ReadyState`) — the hook for pausing generation during a
  race.
- **IPC**: every command is a `#[tauri::command]` in `src-tauri/src/ipc/mod.rs`
  with a wrapper in `src/ipc.ts`; `scripts/check-ipc.mjs` fails CI when the
  two drift. Payload types live in `tp-model` with `#[ts(export)]`; the
  generated `src/bindings/*.ts` are committed and CI fails on a stale diff.
- **Versions** live in three places — `package.json`, `Cargo.toml`
  `[workspace.package]`, `src-tauri/tauri.conf.json` — all `0.1.0`. CI's
  test channel overwrites the version with `0.1.<run number>` (run 79 is the
  latest); the minor bump has to flow through that step too.
- **CSP** (`tauri.conf.json`): `connect-src ipc: http://ipc.localhost` — the
  frontend cannot reach the service until `http://127.0.0.1:*` and
  `ws://127.0.0.1:*` are added.
- HTTP client, SHA-256 and zip crates are already in the Rust dependency
  tree via the updater plugin (`reqwest` 0.13 with native-tls, `sha2`,
  `zip` 5), so downloading and verifying uv costs no new dependency.
- No `CHANGELOG.md` exists yet.

### The machine

- RTX 4090, 24 GB, driver 616.64. Windows 11 Pro 10.0.26200.
- **No Rust, no Node, no MSVC Build Tools, no real Python** (only the Store
  redirector stub). `winget` is present. Nothing can be compiled or
  type-checked locally; CI's Linux job runs on every push (fmt, clippy,
  tp-model tests, bindings drift, IPC check, frontend build) and the Windows
  job is manual (Actions → CI → Run workflow). The repo is public, so CI run
  status can be read without a token. `gh` is not installed.
- A GitHub credential is stored in Windows Credential Manager, so `git push`
  works from this machine.
- Disk: **91 GB free of 1.9 TB** before this work. The dev environment and
  models now take about 14 GB.
- **This Claude session runs inside the packaged (MSIX) Claude desktop app,
  so every write its processes make under `%LOCALAPPDATA%` and `%APPDATA%`
  is virtualised into `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\`.**
  Files written there are invisible to the real Team Principal app, and long
  virtualised paths broke a Hugging Face download. Consequences:
  - the development Voice Lab home is `.voicelab-dev/` inside the repo
    (gitignored; Documents is not virtualised), selected by the
    `TP_VOICELAB_HOME` environment variable, which the product supports too;
  - the sandbox copy of the CrewChief sounds folder is
    `.voicelab-dev/sandbox/CrewChiefV4/sounds/` (~1 GB: everything except
    `driver_names` and other people's `personalisations`);
  - anything the app's own data folder is tested with from this session is
    virtual as well. The real `%APPDATA%\Team Principal` was only read.
- Registry writes are virtualised too; the `HKCU\Software\Python\Astral`
  key uv wrote before `--no-registry` was passed has been removed.

### CrewChief on this machine

- `CrewChiefV4.exe` 4.19.1.34 at `C:\Program Files (x86)\Britton IT Ltd\CrewChiefV4\`.
- Sounds at `%LOCALAPPDATA%\CrewChiefV4\sounds\` (2.1 GB), sound pack
  `en201`. Override key in CrewChief's `user.config`:
  `override_default_sound_pack_location` (empty here). Current
  `chief_name` = "Jim (default)", `spotter_name` = "Clare",
  `PERSONALISATION_NAME` = "Alex".
- `user.config` lives at
  `%LOCALAPPDATA%\Britton_IT_Ltd\CrewChiefV4.exe_Url_<hash>\<version>\user.config`
  (one folder per version; 4.19.1.34 is the latest).
- **Every WAV is 16-bit PCM, mono, 22,050 Hz** — spotter, chief, alt voice,
  radio check, driver names alike.
- Layout as actually installed:
  - `voice/<category>/<intent>/N.wav` + `subtitles.csv` (`N.wav,"text"`)
  - `alt/<Name>/voice/...`, `alt/<Name>/personalisations/...`,
    `alt/<Name>/driver_names/`, `alt/<Name>/composite_personalisation_stubs/`
    — an alt voice mirrors the whole tree, not only `voice/`. The one alt
    voice installed is `Jerry`; it has no `spotter` under `voice/`.
  - `voice/spotter_<Name>/<intent>/` (17 intents) and
    `voice/radio_check_<Name>/test/` for each named spotter.
  - `personalisations/<Name>/prefixes_and_suffixes/{ok,please,come_on,well_done,bad_luck,oh_dear}/`,
    numbered globally 1–24 across the six stubs; no subtitles.
  - filename suffixes: `_op_prefix_<stub>`, `_op_suffix_<stub>`,
    `_rq_suffix_please`, `sweary_N`, `_male`.
- Phrase inventory from `subtitles.csv` (chief tree, excluding
  `spotter_*`, `radio_check_*`, `codriver*`): **1,801 intent folders,
  6,246 lines, 4,422 distinct texts.** A further **1,239 folders have WAVs but
  no subtitles**: 1,051 `numbers` (text derivable from the folder name:
  `10point3`, `1_30`, `point9seconds`, `hundred_and`…), 160 `corners`, and
  28 miscellaneous intents (`virtual_safety_car`, `rejoin_clear`…). These get
  folder-name-derived text, flagged *derived*.
- autovoicepack's `phrase_inventory.csv` (MIT, dev reference only, in the
  session scratchpad): 9,145 rows over 3,274 folders. The gap is `numbers`
  (1,101 rows), `corners` (964), `codriver` (1,836 — rally, out of scope for
  v1) and hand-added `personalisations/YOUR_NAME` rows (29). Its
  `text_for_tts` column is the model for our per-clip pronunciation override.

### The Python side works on this GPU

- uv 0.12.17 (SHA-256 verified against the release's `.sha256`), CPython
  3.11.16 managed by uv, `voice-service/uv.lock`: 102 packages, torch
  2.6.0+cu126 from PyTorch's index.
- `torch.cuda.is_available()` is true; Chatterbox generates a clip in
  1.3–2.3 s after warm-up with 3.1 GB peak VRAM (so several workers fit in
  24 GB); faster-whisper `large-v3-turbo` transcribes a clip in 0.15–0.3 s
  on CUDA with `import torch` first so CTranslate2 finds cuDNN.
- Three test clips and their transcripts are in the session scratchpad; the
  transcripts match, with the expected normalisation needs ("Your P3" for
  "You're P3", "1.4" for "one point four").

## Decisions

1. **Dev home is repo-local, product home is `%LOCALAPPDATA%\Team Principal\voicelab`**,
   both via `TP_VOICELAB_HOME`. Reason: MSIX virtualisation of this session
   (above). `LOCALAPPDATA` rather than the app's `APPDATA` because roaming
   gigabytes of binaries would be absurd.
2. **Recording happens in Python (`sounddevice`), not in the webview.**
   Lossless PCM, device selection, no dependency on WebView2's microphone
   permission prompt, and the trimming/VAD has to happen in Python anyway.
   Live levels stream to the tab over a WebSocket.
3. **The service is a child in a Job Object, port chosen by the app, token in
   an environment variable, readiness = `/health` 200.** See the design doc.
4. **Pause rule for GPU contention**: generation is paused while
   `ReadyState` is `launching`, `racing` or `tearing_down`, and while any
   profile's game executable is running; resumed otherwise.
5. **`pykakasi` (GPL-3.0) and `gradio` are excluded** from the environment
   with uv's marker-that-never-matches override. Both are chatterbox-tts
   dependencies the service never imports (pykakasi is Japanese-only and
   loaded lazily behind a try/except). `setuptools<81` is added because
   Resemble's watermarker imports `pkg_resources`.
6. **Chatterbox's Perth watermark stays on.** Inaudible, free, and it marks
   the audio as synthetic, which is what the attribution requirement wants.
7. **QC transcription uses `large-v3-turbo`** (1.6 GB, MIT) rather than
   `large-v3` (3 GB): four times faster for short phrases, accuracy
   difference immaterial at this phrase length.
8. **Phrase text for folders without `subtitles.csv` is derived from the
   folder name** and flagged, so the QC transcript is what vouches for it.
9. **Rally co-driver phrases (`codriver*`) are out of scope for v1.**
10. **Settings are a `voiceLab` section in `Preferences` with `#[serde(default)]`**
    — no schema bump, no migration.
11. **The version becomes 0.2.0** in all three files and the CI test-channel
    step will take `major.minor` from `Cargo.toml` instead of hard-coding
    `0.1`, so the bump reaches installed copies.
12. **The uv binary is fetched, not bundled**: pinned version and SHA-256
    compiled into the app, downloaded from astral-sh/uv's GitHub release,
    verified before it runs. Keeps the installer at its current size.
13. **The service source is bundled as Tauri resources** (a few hundred KB);
    the environment is built on the user's machine from `uv.lock` with
    `uv sync --frozen` and `UV_PROJECT_ENVIRONMENT` pointing at the data
    folder, so nothing is written into Program Files.

## Open issues

- **Local toolchain.** Without Rust, Node and Build Tools nothing can be
  built or run here; every check is a CI round-trip (~2 min for Linux;
  Windows on request). Asked in the Phase 0 questions.
- **Triggering the Windows build.** `workflow_dispatch` needs a token this
  machine does not have. Proposed: let the Windows job also run on a push
  whose commit message contains `[windows]`, so a build can be requested
  from a commit without changing the "on request only" rule.
- **faster-whisper's cuDNN**: relies on `import torch` first so the DLLs in
  `torch/lib` are on the loader path. Works; should be made explicit in the
  service (`os.add_dll_directory`) rather than incidental.
- **Disk**: 91 GB free before work; module ≈ 14 GB; a full pack ≈ 1 GB.

## Phase 0 questions (asked 2026-09-21)

Recorded here so the answers can be too.

1. Install Rust + Node + VS Build Tools locally (winget, ~4 GB, outside the
   repo) so I can build and run — or stay CI-only?
2. Keep the Voice Lab home on C: (91 GB free) or offer another drive?
3. May "Select this voice in CrewChief" edit `chief_name` / `spotter_name`
   in CrewChief's `user.config` (preview + backup, CrewChief closed), or
   should it only open CrewChief with instructions?
4. OK to add the `[windows]` commit-message trigger for the Windows CI job?

## How to resume

```bash
git checkout feature/voice-lab
source .voicelab-dev/dev-env.sh        # UV, PY, HF_HOME, TP_VOICELAB_HOME
cd voice-service && "$UV" sync --frozen  # no-op when nothing changed
"$PY" -m voicelab --help
```

If `.voicelab-dev/` is missing (fresh clone), rebuild it: download uv
0.12.17 into `.voicelab-dev/bin`, `uv python install 3.11 --no-bin --no-registry`
with `UV_PYTHON_INSTALL_DIR` set, then `uv sync --frozen`. Models download on
first use into `HF_HOME`. Copy the CrewChief sounds folder (minus
`driver_names` and `personalisations`) to `.voicelab-dev/sandbox/CrewChiefV4/sounds`.
