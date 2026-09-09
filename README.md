# Team Principal

**One button between you and a race.**

A Windows app for sim racers with more than one screen. You describe your rig
once — how big the screens are, how they're angled, how far away you sit — and
every game gets set up from that. No more typing the same numbers into six
different sims and hoping.

[![CI](https://github.com/acamenzuli/Team-Principal-App/actions/workflows/ci.yml/badge.svg)](https://github.com/acamenzuli/Team-Principal-App/actions/workflows/ci.yml)

---

## The idea

Setting up triple screens is the same job over and over. Every sim asks for the
same facts in a different way — one wants millimetres, one wants a field of
view, one wants an angle — and you work them out again for each game.

Team Principal asks once.

Measure your screens and your seating position, and the app works out what every
game needs from that. Move your seat five centimetres and you change **one
number**, not twelve settings across six games.

---

## Getting it

Every change automatically builds a Windows installer.

1. Go to the [Actions tab](https://github.com/acamenzuli/Team-Principal-App/actions)
2. Click the newest run with a green tick
3. Scroll down to **Artifacts**
4. Download **team-principal-installer**, unzip it, run the setup file

Windows will show a blue "Windows protected your PC" warning the first time.
That's because the app isn't code-signed yet — click **More info**, then **Run
anyway**. This goes away once a signing certificate is bought.

There's also **team-principal-portable**: the same app as a single file with no
installer. And `tp-fakegame`, which is a test tool, not part of the product.

---

## What it does

### Home
Opens on what you actually want to know: is my rig as I left it, what can I
race, and is anything wrong. Anything that would spoil a session — a screen that
never got measured, nothing plugged in, a wheel that's moved position — is
listed with a button that takes you to it.

### Screen Setup
Where you describe the rig. The app reads what it can from the monitors
themselves; anything they don't report honestly, you measure with a tape. It
handles curved screens properly, and shows you the numbers changing as you type.

### Displays
What's plugged in and what it's doing. You can change resolutions and rearrange
screens from here — and if a change goes wrong, **it puts itself back after
fifteen seconds unless you confirm it.** There's also a panic key
(Ctrl+Alt+Shift+R) that undoes a display change even if you can't see anything.

### Peripherals
Every wheel, pedal set and button box, live. Unplug something and watch it go
red; plug it back in and it goes green on its own.

It also spots when Windows quietly reshuffles your controller order — which is
what silently breaks your bindings in games and is almost impossible to notice
until you're on track.

### Games
Every sim you have installed gets a profile automatically. Steam and Epic are
found on their own; anything else you can add by hand.

**Profiles outlive the game.** Uninstall something and its card stays, marked
NOT INSTALLED, with everything you set up still in it. Reinstall and it picks up
where it left off.

**Already set your triples up with SRWE or Resize Raccoon?** Run the game how
you like it, then press **Copy current layout**. The app reads the window and
remembers it — position, size, borderless, the lot — and puts it back there
every time you launch from then on.

### Let's race
A checklist that runs before the game starts. It checks your peripherals are
connected and starts whatever utilities you use, all at once rather than one
after another.

If something's wrong, it says so — and **fixes itself when you fix it.** Plug
the pedals back in and the red row turns green on its own. No button to hunt
for.

Launching is a second, deliberate press. The app never starts a game because a
checklist finished.

When you're done racing, it puts everything back: game settings restored,
utilities closed.

### Game Settings
Writes your rig's measurements into each sim's own settings file.

Before it writes anything, it shows you exactly what will change and why —
**every number says where it came from.** Every file is backed up first, and
there's a one-click "put it back" for any change it ever made.

---

## Which games

The app **launches, places windows for, and checks peripherals for any game at
all.** That part isn't per-title.

Writing settings *into* a game needs knowing what that game calls them, and
that's where games differ. So there are three levels, shown on a badge:

| Badge | Means |
| --- | --- |
| **● VERIFIED** | Settings read from a real file of that game. Written with confidence. |
| **▲ CORROBORATED** | Agreed across several independent sources. Written, carefully. |
| **○ NEEDS A FILE** | The game is recognised and its settings file is found — but nobody's confirmed what the settings are *called* yet, so the app won't touch it. |

Currently listed: iRacing, Assetto Corsa, Assetto Corsa Competizione, Assetto
Corsa EVO, Assetto Corsa Rally, rFactor 2, Le Mans Ultimate, Automobilista 2,
RaceRoom, DiRT Rally 2.0, BeamNG.drive.

**Turning a ○ into a ● takes one file.** Press **Inspect this game's config** and
the app lists what's in your settings file — the *names* only, no values, nothing
personal. Send that in and the game becomes fully supported.

This is deliberate. Guessing what a setting is called produces an app that says
"done" and changed nothing, or worse, changes the wrong thing. Every claim in
`docs/adapters/` records where it came from and what's still unconfirmed.

---

## It won't get you banned

This app will **never**:

- inject anything into a game
- read or write a game's memory
- hook the graphics pipeline
- bundle or run other people's tools

It moves windows using the same normal Windows calls any app uses, from the
outside, and changes game settings by editing the game's own settings files
while the game isn't running. Both are what sim racers already do by hand every
day. Neither gives an anti-cheat system anything to object to.

**And it never invents a setting.** If a game doesn't already have a setting,
the app won't create one — you get a message naming it instead of a change that
silently does nothing.

---

## Updates

The app checks for a new version each time it starts. What happens next is your
choice, in Settings:

- **Tell me** — a strip at the top of the window, and nothing happens until you
  press the button. *(the default)*
- **Install it** — downloads and installs on its own, then restarts.
- **Do nothing** — no checking at all.

Automatic updates won't interrupt a race. If a session is running, the update
waits.

**Your settings are never touched.** Rigs, profiles, backups and preferences
live separately from the program, so updating — or even reinstalling — keeps
everything.

---

## Other things

- **Start with Windows**, minimised and out of the way. Or always minimised, or
  neither. Both switches are in Settings.
- **Make it yours** — team name, logo, and an accent colour that runs through
  the whole app.
- **Diagnostics** — one button makes a single file with everything needed to
  work out what went wrong. It contains a plain-English list of exactly what's
  in it, and your Windows username is removed throughout. Your game settings
  files are not included.

---

## Where things are

Everything the app saves lives in `%APPDATA%\Team Principal`:

```
rigs\          your screen measurements
profiles\      per-game setup
snapshots\     display layouts, saved before any change
backups\       copies of every game file it has ever edited
logs\          what happened, and when
```

Nothing is written anywhere else, and nothing leaves your machine.

---

## Honest status

**Everything described above is built. None of it has been run on real hardware
yet.**

The parts that can be tested without a rig — the maths, the file editing, the
planning — are covered by 327 tests that run on every change. The parts that
touch Windows itself — moving windows, changing displays, reading wheels — are
written and check out, but have never met an actual monitor or wheel.

That's the next step, and it's why the version number starts with a zero.

---

## For developers

| Doc | What's in it |
| --- | --- |
| [DEV-SETUP.md](docs/DEV-SETUP.md) | Building it yourself |
| [RELEASING.md](docs/RELEASING.md) | Signing keys, certificates, cutting a release |
| [0003-rig-model.md](docs/design/0003-rig-model.md) | **The rig model, and the geometry it drives** |
| [0008-display-control.md](docs/design/0008-display-control.md) | Why changing the desktop is survivable |
| [adapters/README.md](docs/adapters/README.md) | How a game gets supported, and the rules for it |
| [0001-stack.md](docs/design/0001-stack.md) | Tauri vs Electron, DPI, elevation, signing |
| [0002-repo-structure.md](docs/design/0002-repo-structure.md) | Crate and module layout |
| [0004-profile-schema.md](docs/design/0004-profile-schema.md) | Profiles, the launch graph, storage |
| [0005-open-questions.md](docs/design/0005-open-questions.md) | What's still outstanding |
| [0006-visual-language.md](docs/design/0006-visual-language.md) | The design system |
| [0007-display-enumeration.md](docs/design/0007-display-enumeration.md) | How screens are detected |

Built with [Tauri](https://tauri.app), React and Rust. Windows 11.

© Alex Camenzuli
