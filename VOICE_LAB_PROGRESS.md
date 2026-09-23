# Voice Lab — progress

The running record for the Voice Lab work. The brief is `VOICE_LAB_UPDATE.md`
on the owner's desktop (not in the repo); the design is
`docs/design/0009-voice-lab.md`. A fresh session resumes from those two plus
this file.

Branch: `feature/voice-lab`. Nothing merged, tagged or published.

## Status

| Phase | State |
| --- | --- |
| 0 — Recon | **done** 2026-09-21 |
| 1 — Service and bare tab | **done** 2026-09-23 |
| 2 — Guided recorder and tone mapping | **done** 2026-09-23 |
| 3 — Full generation and review | **done** 2026-09-23 |
| 4 — Optional-module installer | **built, not yet run from clean** |
| 5 — Engine bake-off | Chatterbox and Chatterbox Turbo are both selectable; the blind A/B screen is not built |
| 6 — Extras | spotter, radio effect and zip export done; RVC not attempted |
| 7 — Release build and handoff | version bumped, changelog written; **installer not yet built** |

## What runs, and what it did

The whole pipeline has been run end to end on this machine, against a
**sandbox copy** of the CrewChief sounds folder. The real folder has only
ever been read.

* **765-clip preview in 16 minutes** with three workers (mean 3.3 s per
  attempt, 858 attempts for 765 clips). **749 passed**, 696 of them on the
  first attempt.
* Of the 16 that did not, every one was the checker being too strict rather
  than bad audio — twelve were "half-way" heard as "halfway". After the fix
  below, 15 of the 16 passed on a retry; the last two are genuine ("car
  ride" is not "car right").
* **Installed 764 files** into the sandbox in CrewChief's own layout —
  `alt/Sample/voice/...`, `voice/spotter_Sample/...`,
  `voice/radio_check_Sample/test`, `alt/Sample/personalisations/Alex/...` —
  all 22,050 Hz mono 16-bit, matching the installed pack.
* **Uninstalled exactly those 764** and left the existing Jerry voice's
  9,510 files untouched.
* **Resume works**, proved by accident: this session was suspended
  mid-generation and the restarted job skipped the 337 already-passed clips.
* GPU detection was checked against the real card: RTX 4090, 24,138 MB,
  driver 616.64 derived from Windows's `32.0.16.1664` — the same number
  `nvidia-smi` reports.

## Three findings that changed the design

Each was measured, not assumed, and each is recorded where the code does it.

1. **"P15" is not safe to send to the engine.** Literally, it came back as
   "P5", "Juan 5" or "15" in twelve attempts out of fifteen. Spelled "P
   fifteen" it was right fourteen times out of fifteen. Position calls are
   the most-heard phrases in the pack and a wrong number is the worst thing
   it could say, so `phrases.pronounce` rewrites subtitles into speakable
   text before the engine sees them, while the QC comparison still holds the
   clip to what CrewChief displays. (It also caught "10th position" →
   "ten th position", which the ordinal rule fixed.)
2. **Chatterbox's emotion knob costs intelligibility.** On two-word spotter
   calls: exaggeration 0.7 with cfg 0.3 gave 15/30 intelligible, 0.5/0.5
   gave 26/30. The tone map now stays near neutral and the *reference
   recording* for each tone carries the tone — which is why the recorder
   script asks for urgent lines to be read fast and sharp.
3. **Teacher-forced scoring only works on short phrases.** Reading the
   target against the audio separates good from garbled on two words
   (correct 0.4–0.9, garbled 0.00–0.05) but not on twelve (a correct clip
   scores mean 0.77 with a minimum of 0.01, and a deliberately wrong text
   against the same audio still scores 0.60). So that rescue is limited to
   four words or fewer, and longer mismatches are settled by comparing the
   letters — "half-way" and "halfway" are the same letters in the same
   order, which is certain rather than probable.

## Decisions (cumulative)

Phase 0's decisions still hold unless listed here. New ones:

14. **The tab talks to the service directly** over loopback rather than
    through Tauri commands. Seven commands cross the IPC boundary — the GPU
    check, the module, the service lifetime — and everything about voices,
    packs and generation is HTTP. Routing it all through Rust would have
    meant a second set of types for no gain.
15. **Recording happens in Python** (`sounddevice`), not the webview.
16. **The service source is bundled as a Tauri resource** (344 KB, 24
    files), so the installer size is unchanged. The environment is built on
    the user's machine from `uv.lock`.
17. **uv is pinned with its SHA-256** (0.12.17) compiled into the app and
    checked before the binary is ever run.
18. **Every install stage is verified by read-back**, never by exit status —
    uv#19622 returns non-zero from a `python install` that worked.
19. **The CI test channel now takes major.minor from `Cargo.toml`** instead
    of hard-coding `0.1`. Without that, bumping to 0.2.0 would still have
    shipped `0.1.<run>` test builds, and an installed 0.2.0 would have
    reported "up to date" against every one of them.
20. **The Windows CI job also runs on a commit message containing
    `[windows]`** (owner approved), so a build can be requested from a
    machine with no GitHub token. An ordinary push still does not build.
21. **`ABOUT_THIS_VOICE.txt` ships inside every pack**, naming the engine,
    saying the audio is AI-generated, recording the attestation and its
    date, and crediting CrewChief without implying affiliation.

## Open issues

* **Never tested with a real voice.** Every run used the engine's built-in
  sample voice. The recorder is written and the analysis is tested, but no
  one has spoken into it. This is the first thing to do.
* **CrewChief has not played a generated pack.** The files are in the right
  places in the right format, and CrewChief's own source says that is what
  it looks for, but it has not been proved by hearing it.
* **The module installer has not been run from clean.** The development
  environment was built by hand during recon with the same commands the
  installer issues, and each stage's verification has been exercised, but
  "press Download on a machine with nothing" has not been done. Removing
  `.voicelab-dev/` and pressing it is the test.
* **`specs.json` is frozen when a pack is created**, so a pack made before a
  pronunciation fix keeps the old text. Real users will not hit this
  (the rewrite exists from the first release); if it becomes a problem, a
  "refresh phrases" action on an existing pack is the fix.
* **Chatterbox Turbo is selectable but untested.** It ignores the emotion
  and pacing knobs, so its tone comes entirely from the reference.
* **No blind A/B bake-off screen.** Both engines can be chosen per pack,
  which is most of the value; the side-by-side comparison is not built.
* **Disk**: the module is ~14 GB. C: had 91 GB free before this work.

## How to resume

```bash
git checkout feature/voice-lab
source .voicelab-dev/dev-env.sh          # UV, PY, HF_HOME, TP_VOICELAB_HOME
cd voice-service && "$UV" sync --frozen  # no-op when nothing changed
"$PY" -m pytest -q                       # 24 tests, no GPU needed
```

To run the service by hand against the sandbox (what every test above used):

```bash
source .voicelab-dev/dev-env.sh
export VOICELAB_TOKEN=devtoken
"$PY" -W ignore -m voicelab --port 47321 --sounds "$TP_VOICELAB_HOME\\sandbox\\CrewChiefV4\\sounds"
curl -H "Authorization: Bearer devtoken" http://127.0.0.1:47321/health
```

The app itself now builds and runs locally (`npm run tauri dev`) — Rust
1.98.1, Node 24, and VS Build Tools 2022 were installed on 2026-09-21 with
the owner's approval, so the CI-only loop no longer applies.

If `.voicelab-dev/` is missing (fresh clone), the Voice Lab tab's own
Download button builds it; that is the same path a user takes.
