# 0009 — Voice Lab

Status: **in progress**. The brief is `VOICE_LAB_UPDATE.md` (kept outside the
repo, on the owner's desktop); the running record is `VOICE_LAB_PROGRESS.md`
in the repo root. This document is the shape of the thing and the reasons for
it, so that later phases extend the design instead of improvising.

## What it is

A tab that turns about five minutes of the owner's own voice into a complete
CrewChief voice pack — chief and spotter — with every clip checked by a
second model before it is allowed into the pack, and one click to install it
where CrewChief looks.

Everything heavy lives in a Python service the app starts when the tab needs
it and stops when it does not. Nothing heavy ships in the installer.

## The three places code lives

```
voice-service/                 Python. FastAPI on 127.0.0.1, PyTorch + CUDA.
  pyproject.toml, uv.lock      pinned; the lock is the contract with the GPU
  src/voicelab/                the service — see "Service layout" below

crates/tp-model/src/voicelab.rs
                               the IPC types and every pure decision: GPU
                               requirements, driver-version parsing, the data
                               folder layout, the pinned uv release. Tested
                               on Linux like everything else in tp-model.

src-tauri/src/voicelab/        the platform half, kept small:
  gpu.rs                       DXGI adapter enumeration + registry driver version
  module.rs                    download uv, install Python, sync the env, remove
  service.rs                   spawn, health, idle stop, pause on race, kill on exit

src/voicelab/                  React. One tab, five screens.
```

The rule from `0002-repo-structure.md` holds: anything that can be got wrong
lives where it can be run. The Rust side spawns a process and reads a
registry key; the decisions about what those mean are in `tp-model`.

## The service is a sidecar, not a plugin

- Started on demand by `voicelab::service`, as a child of the app, inside a
  Job Object (`launcher::job::JobObject`) so that it dies with the app —
  cleanly, by crash, or by Task Manager — the same guarantee session
  utilities get.
- Bound to `127.0.0.1` on a port the app picks (bind `:0`, read the port,
  release it, pass it on). A per-session token travels in an environment
  variable, never on the command line where any process can read it, and
  every request must carry it as a bearer token.
- Readiness is `GET /health` returning 200, polled by the app the way the
  launcher polls any `ReadinessGate`. Health is fast because models load
  lazily: the service is "ready" in a second or two, and the first
  synthesis pays for the model load.
- The frontend talks to the service directly — HTTP for calls, WebSocket for
  progress — after asking Rust for the port and token. The CSP allows
  `http://127.0.0.1:*` and `ws://127.0.0.1:*` for exactly that.
- Stopped when idle for ten minutes (a setting), and on app exit. A running
  job counts as activity; a job paused for a race does too, so a pack does
  not lose its worker halfway through a session.

Why not a Tauri sidecar binary (`externalBin`)? Because the Python
environment is built on the user's machine, after install, from a lockfile.
There is no binary to bundle, and the whole point of the optional module is
that there is not.

## The optional module

The installer stays the size it is. The tab, on first open, checks the GPU
(name, VRAM, driver version — DXGI and the registry, no third-party tool)
and shows the requirements. If the machine qualifies, it offers to build
the module into `%LOCALAPPDATA%\Team Principal\voicelab\`:

```
voicelab\
  bin\uv.exe            fetched from astral-sh/uv's GitHub release, pinned
                        version, SHA-256 checked against the value compiled in
  python\               CPython 3.11, installed by uv (no PATH shims, no
                        registry entries — both are explicitly off)
  cache\uv\             wheel cache; what makes a retried install resumable
  env\                  the virtualenv, from uv.lock, exactly
  models\               Hugging Face cache: Chatterbox, faster-whisper,
                        silero-vad — each from its official repository
  voices\<voice>\       cleaned reference clips + consent metadata
  packs\<pack>\         generated clips, job state, QC log
  sandbox\              development only: a copy of the CrewChief sounds folder
  logs\
```

`LOCALAPPDATA`, not the app's usual `APPDATA`: this is gigabytes of
machine-specific binaries, and a roaming profile must never try to carry
it. Settings stay in `preferences.json` where every other setting is.

Removal deletes `bin`, `python`, `cache`, `env` and `models` and reports the
space reclaimed. Voices and packs are the user's work and are kept unless
they say otherwise.

### Verified by read-back

`uv python install` has a known Windows bug ([astral-sh/uv#19622]) where the
install succeeds and the command exits non-zero. So the module installer
does what the rest of this app does with Win32: it never trusts a return
code. Each stage is verified by what it produced — the interpreter runs and
reports its version, `uv sync --frozen` leaves a `python.exe` that can
`import torch` and see CUDA, the model files are present at the sizes the
hub reports.

[astral-sh/uv#19622]: https://github.com/astral-sh/uv/issues/19622

## Service layout

```
voicelab/
  server.py        FastAPI app, token middleware, CORS for the webview origins
  config.py        paths (all passed in or derived; nothing hard-coded)
  crewchief.py     sounds folder discovery, subtitles.csv parsing, WAV format
                   probe, the phrase inventory, install / uninstall / backup
  phrases.py       inventory → job items; number and corner folder names;
                   your_name substitution
  tones.py         the tone map: folder patterns → tone + speed + emotion
  engines/
    base.py        TTSEngine: load(), synthesize(text, refs, tone) -> audio
    chatterbox.py  the default engine
    registry.py    what is installed, what is licensed to ship
  audio.py         trim, fades, loudness match, resample, radio effect, write
  recorder.py      sounddevice capture, VAD split, SNR, clipping, waveform
  qc.py            faster-whisper transcription, normalisation, WER, duration
  jobs.py          the queue: GPU workers, resumable state, pause / resume
  packs.py         pack metadata, attribution file, export
  models.py        model presence and download with progress
```

## Phrases come from the user's CrewChief

No phrase text ships in the app. The inventory is read from `subtitles.csv`
in the user's own sounds folder at run time (1,801 intent folders and 6,246
lines in the current pack). Folders with no subtitles — the 1,052 `numbers`
folders, some `corners`, and about thirty odd intents — get their text
derived from the folder name (`10point3` → "ten point three") and are
flagged *derived* in the review screen so the transcript check carries the
weight there.

The reference project's `phrase_inventory.csv` was compared against this
during recon and is not bundled; it is a development reference only.

## Output matches what is there

The current CrewChief pack is 16-bit PCM, mono, 22,050 Hz, and every clip
in it was inspected to say so. The service probes the existing `voice\`
folder rather than assuming that, and writes what it finds.

Loudness is matched to CrewChief's own spotter clips, measured in LUFS at
generation time, so a generated pack sits at the level the user is already
used to hearing.

## Quality control

Every clip is transcribed by faster-whisper, both texts are normalised
(numbers, "P3" → "P 3", punctuation) and the word error rate is computed.
Duration is checked against the expected speaking rate; clipping, leading
and trailing silence, and spectral oddities are checked. A failure
regenerates with a new seed, up to N attempts, and every attempt is logged
with engine, seed, attempt number and WER. Persistent failures are listed
for review rather than quietly dropped or quietly kept.

## GPU contention

When a run is generating and the launcher starts a session — the deliberate
second press, or a known sim appearing — the run pauses, the tab says so,
and it resumes when the session ends. The launcher's `launch://state`
stream is the trigger; the tab is a view over that state like every other
screen.

## Licensing

Only models whose licences permit commercial use are shipped or
auto-downloaded:

| Model | Licence | Role |
| --- | --- | --- |
| Chatterbox (Resemble AI) | MIT | default TTS engine |
| faster-whisper + Systran CT2 large-v3-turbo weights | MIT | QC transcription |
| silero-vad | MIT | recording trim and split |

Every engine candidate's licence is verified before it is added, and a
non-commercial model may exist only behind a development flag that the
shipped build does not read.

## Consent and attribution

Recording is guided live-mic only; there is no way to import a file. Before
generating, the user attests that the voice is theirs or used with the
speaker's permission, and that attestation and its date are written into the
pack's metadata. Every installed pack carries `ABOUT_THIS_VOICE.txt` naming
the engine, stating the audio is AI-generated, and crediting CrewChief
without implying affiliation.
