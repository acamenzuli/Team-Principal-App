# Assetto Corsa

**Confidence:** Corroborated
**Checked:** 8 September 2026

Deliberately does less than the iRacing adapter. It sets the render resolution
and the window mode, and stops.

## File

`{documents}\Assetto Corsa\cfg\video.ini`

## Keys written

| Section | Key | Value derived from |
|---|---|---|
| `VIDEO` | `WIDTH` | sum of the session's screens' native widths |
| `VIDEO` | `HEIGHT` | tallest of the session's screens |
| `VIDEO` | `FULLSCREEN` | `0` — borderless windowed, so Team Principal can place the window and alt-tab stays instant |

## Keys deliberately not written

**The triple-screen geometry.** This is the important omission and it is a
deliberate one.

Assetto Corsa does not compute its triple-screen projection from `video.ini`. It
computes it from values entered in an **in-game app** — the panel with Screen
Width, Distance, Angle and Margins spinners that appears on the right of the
screen when triple-screen rendering is on.

Where those values are persisted, and under what key names, could not be
established to the standard this project requires. The sources that discuss them
give the *widget labels* from the in-game app (`SPINNER_SCREEN_WIDTH`,
`DISTANCE_SPINNER`, `ROTATION_SPINNER`, `MARGIN_SPINNER`), which are not
necessarily the ini keys, and none of them is dated.

Writing guessed key names here would be exactly the twelve-guessed-adapters
failure this project exists to avoid. So the app does not write them, and says
so on screen: your rig's numbers are on the Screen Setup tab, ready to be copied
into the in-game app by hand, until this is confirmed against a real file.

Quality settings (`ANISOTROPIC`, `AASAMPLES`, `VSYNC`, `SHADOW_MAP_SIZE`,
`AAQUALITY`, `DISABLE_LEGACY_HDR`, `FPS_CAP_MS`) are also left alone. The app has
no opinion on them.

## What would raise this to Verified

One real `video.ini` from a triple-screen install, with the in-game app's values
already set. That file answers both open questions at once: whether the triple
values live in `video.ini` at all, and what they are called. It is the single
most useful thing to send.

## Sources

Several independent second-hand sources agree on the path and on `[VIDEO]`
carrying `WIDTH`, `HEIGHT` and `FULLSCREEN`. None is a shipped file or developer
documentation, which is why this is `Corroborated` and not `Verified`:

- <https://www.overtake.gg/threads/ac-can%C2%B4t-find-widescreen-resolution.178157/>
- <https://steamcommunity.com/app/244210/discussions/0/490123197945846685/>
- <https://www.overtake.gg/threads/how-to-start-assetto-corsa-in-windowed-mode.84104/>
- <https://www.wsgf.org/dr/assetto-corsa/en>

Most of these are unreachable from the build container's egress policy and were
read through search summaries rather than directly, which is a further reason
the level is `Corroborated`.

## Not yet an adapter: Assetto Corsa Competizione

ACC is an Unreal Engine title and keeps its resolution in
`%LOCALAPPDATA%\AC2\Saved\Config\WindowsNoEditor\GameUserSettings.ini` as
`ResolutionSizeX` / `ResolutionSizeY` / `PreferredFullscreenMode`. That much is
corroborated.

It is not shipped because Unreal also keeps `LastUserConfirmed*` copies of those
values and reverts to them under conditions this project has not confirmed, and
because ACC's own triple-screen settings live elsewhere again. A resolution
adapter that silently loses its change on the next launch would be worse than no
adapter. It is the obvious next one to finish, with one real file to check
against.
