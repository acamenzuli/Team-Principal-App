# Team Principal

A Windows 11 sim racing launcher and display manager. One button turns "I want to
race" into a verified rig: peripherals checked, utilities started, display
configured, game config written, window placed — then torn back down on exit.

Status: **pre-implementation.** Design documents only. See `docs/design/`.

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
diff before it is applied.

## Documents

| Doc | Contents |
| --- | --- |
| [0001-stack.md](docs/design/0001-stack.md) | Tauri vs Electron, DPI, elevation, signing |
| [0002-repo-structure.md](docs/design/0002-repo-structure.md) | Crate and module layout |
| [0003-rig-model.md](docs/design/0003-rig-model.md) | **The rig model schema and the geometry it drives** |
| [0004-profile-schema.md](docs/design/0004-profile-schema.md) | Profile, step graph, storage, migrations |
| [0005-open-questions.md](docs/design/0005-open-questions.md) | What I need from you before milestone 1 |
