# Team Principal

A Windows 11 sim racing launcher and display manager. One button turns "I want to
race" into a verified rig: peripherals checked, utilities started, display
configured, game config written, window placed — then torn back down on exit.

Status: **milestones 1–10 built, none hardware-tested.** Everything below is
implemented and its pure logic is covered by tests that run on every push. The
Win32 half — display changes, window placement, HID, the panic hotkey — compiles
and type-checks but has not yet been run against real hardware. That is the next
step, not a finished one.

## Getting the app

Every push builds a signed-later Windows installer. To get it without
installing a toolchain:

1. Open the [Actions tab](https://github.com/acamenzuli/Team-Principal-App/actions)
2. Click the newest run
3. Scroll to **Artifacts** and download **team-principal-installer**
4. Unzip and run the setup `.exe`

Windows SmartScreen will warn on first launch — the installer is not code-signed
yet. Click **More info → Run anyway**. Code signing (Azure Trusted Signing) is a
required pre-launch cost, noted in `docs/design/0001-stack.md`.

`team-principal-portable` is the same app as a single `.exe` with no installer.
`tp-fakegame` is the test harness for the window watchdog, not part of the product.

To build it yourself, see `docs/DEV-SETUP.md`.

## What makes it different

You describe your physical rig once — panel sizes, bezels, angles, seating
distance — in Screen Setup. Every game's resolution, window rectangle, FOV,
triple-screen projection and bezel compensation is *derived* from that physical
model rather than typed in per title. Move your seat 5 cm and one number changes.

## Safety and anti-cheat stance

This application will **never**:

- inject code into a game process
- read or write game process memory
- hook the render pipeline, or load into a game's address space by any means
- ship, bundle, or shell out to third-party binaries (MultiMonitorTool, SRWE,
  nircmd, or similar)

It manipulates windows only through documented Win32 user-mode APIs
(`SetWindowLongPtrW`, `SetWindowPos`) applied from outside the process, and edits
game settings only by rewriting the game's own configuration files on disk while
the game is not running. Both techniques are long-established — they are what the
tools sim racers already run every day — and neither gives an anti-cheat
system cause to flag the app.

Every file the app writes is backed up first, and every change is previewed as a
diff before it is applied. The app never creates a settings key a game does not
already have, so a key name that is wrong for your version of a game produces a
message rather than a silent no-op.

## What it does

| Area | |
| --- | --- |
| **Screen Setup** | Describe the rig once. Panel sizes from EDID where the monitor reports them honestly, measured by hand where it does not. Curvature is chord-and-sagitta, not a marketing radius. |
| **Displays** | What is attached, what it is doing, and how to change it — behind a preview, a read-back check, a fifteen-second confirm-or-revert countdown, and a panic hotkey. |
| **Peripherals** | HID and DirectInput enumeration, hotplug by OS event rather than polling, and a live axis and button monitor. DirectInput slot drift is detected, because it silently destroys game bindings. |
| **Games** | Every installed sim gets a profile automatically. Profiles outlive the install: uninstall a game and the card stays with everything you configured. |
| **Let's race** | A visible preflight that checks peripherals and starts utilities in parallel, heals itself when you plug something back in, and holds the launch behind a deliberate second press. |
| **Game Settings** | Your rig's measurements written into each sim's own config file, previewed as a diff, backed up first. |
| **Diagnostics** | One button, one zip, with a README inside listing exactly what it contains and what was redacted. |

## Licensing

There is no licence check in this build, and the Licence panel in Settings says
so rather than showing a reassuring tick over nothing.

The seam is in `src-tauri/src/licence.rs`: one trait, three methods, one
`provider()` function to change. No vendor is named anywhere in the codebase,
and no billing is implemented — a merchant-of-record will handle the money.
Nothing sensitive reaches the frontend, because `src/` ships as readable
JavaScript inside the installer: the UI is told a tier, a four-character
reference and a message, and never a key, a token, an endpoint or a machine id.

## Documents

| Doc | Contents |
| --- | --- |
| [0001-stack.md](docs/design/0001-stack.md) | Tauri vs Electron, DPI, elevation, signing |
| [0002-repo-structure.md](docs/design/0002-repo-structure.md) | Crate and module layout |
| [0003-rig-model.md](docs/design/0003-rig-model.md) | **The rig model schema and the geometry it drives** |
| [0004-profile-schema.md](docs/design/0004-profile-schema.md) | Profile, step graph, storage, migrations |
| [0005-open-questions.md](docs/design/0005-open-questions.md) | What I still need from you |
| [0006-visual-language.md](docs/design/0006-visual-language.md) | The glass design system |
| [0007-display-enumeration.md](docs/design/0007-display-enumeration.md) | CCD, EDID and dead regions |
| [0008-display-control.md](docs/design/0008-display-control.md) | **Why changing the desktop is survivable** |
| [adapters/README.md](docs/adapters/README.md) | **The adapter verification protocol** |
