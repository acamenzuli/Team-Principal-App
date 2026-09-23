# Changelog

Versions the app reports, and what changed in them. Test-channel builds
carry the same major and minor with the CI run number as the patch, so
`0.2.41` is a development build of 0.2.

## 0.2.0 — Voice Lab

**Your own voice as the crew chief.** A new tab turns about five minutes of
recorded speech into a complete CrewChief voice pack — the chief, the
spotter, the radio check and the phrases that say your name — and installs
it where CrewChief looks.

- **Guided recording.** Twenty-odd lines in three tones. Each take is
  trimmed, checked for clipping and background noise, and kept or rejected
  with a reason, because a reference recorded too close to the microphone
  makes every generated clip worse.
- **The phrases are your own CrewChief's.** The list is read from the
  `subtitles.csv` files in your sounds folder when the pack is made, so it
  matches the pack you have rather than one frozen into this app.
- **Every clip is checked.** A second model transcribes each one and
  compares it with what was asked for; anything that does not match is
  generated again with a new seed. What is left over is listed with the
  audio, so you can hear the disagreement and decide.
- **A preview first.** The phrases heard in the first minutes of a race —
  the spotter, positions, gaps, flags, fuel, the start and the finish —
  about a quarter of an hour of work, and enough to go racing with.
- **It yields the graphics card.** Generation pauses on its own when a
  session starts and carries on when you finish.
- **Nothing extra in the installer.** Python, PyTorch and the models are
  downloaded into the app's own folder when you first ask for them, and
  **Remove Voice Lab** takes every byte back.

Everything runs on this machine. No recording and no audio leaves it.

Needs an NVIDIA card with 8 GB and driver 528.33 or newer; on anything else
the tab explains why rather than disappearing.

**Your settings, rigs, profiles and backups are untouched by this update.**

## 0.1.x

Milestones 1 to 11: displays, the rig model and geometry, screen setup,
peripherals, window control, launch orchestration, the "Let's race" flow,
display control with confirm-or-revert, the first game adapters, and
packaging. See the README.
