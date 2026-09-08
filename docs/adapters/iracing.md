# iRacing

**Confidence:** Verified
**Checked:** 8 September 2026

The adapter this whole application was designed around, and the one that proves
the premise. iRacing's `[MonitorSetup]` section does not ask for a field of
view. It asks for **the physical measurements** — millimetres of glass,
millimetres including bezels, and the angle the side screens are turned in — and
computes the projection itself.

That is precisely the rig model. So this adapter is a unit conversion rather
than a calculation, and there is nothing in it to get subtly wrong.

## File

`{documents}\iRacing\renderer.ini`

`{documents}` is resolved with `SHGetKnownFolderPath(FOLDERID_Documents)`, never
assembled from `%USERPROFILE%`. OneDrive redirects the Documents folder on a
great many consumer Windows installs, and the failure from assuming otherwise is
silent: a perfectly good config written to a folder the game does not read.

## Keys written

| Section | Key | Units | Value derived from |
|---|---|---|---|
| `MonitorSetup` | `NumMonitors` | count | 3 when the session spans the rig, else 1 |
| `MonitorSetup` | `ScreenWidth` | mm | centre screen's flat (chord) visible width |
| `MonitorSetup` | `MonitorWidth` | mm | that, plus both bezels, plus any mount gap |
| `MonitorSetup` | `ScreenAngles` | deg | side screen mounting angle |
| `MonitorSetup` | `RenderViewPerMonitor` | 0/1 | always 1 on a triple |

### Why the mount gap goes into `MonitorWidth`

iRacing's model assumes the monitors are butted together, so the difference
between `MonitorWidth` and `ScreenWidth` **is** the gap it compensates for. A rig
with extra space between the screens has to fold that space in here, or the
bezel correction comes out short by exactly the mount gap. The app says so in a
warning rather than doing it silently.

### Why `ScreenWidth` is the chord and not the arc

On a curved panel a tape measure laid along the glass gives the arc. What a
renderer projecting onto a plane sees is the chord. `tp-geometry` produces the
chord as `flat_width_mm`, and that is what goes here. See
`docs/design/0003-rig-model.md` §curvature.

## Keys deliberately not written

* `BezelProtectionPct` — a UI-placement preference, not geometry. It keeps the
  HUD off the bezels and is entirely a matter of taste.
* `Min3ViewZoomDistortion` — a camera behaviour toggle, not derivable from a
  measurement.
* Everything in `[Graphics Options]` — quality settings. The app has no opinion
  on them and rewriting them would be an unwanted surprise.
* **Resolution.** iRacing keeps it in a separate, renderer-specific file
  (`rendererDX11*.ini`, whose exact name varies with the renderer and with VR).
  The name is not confirmed to the standard this document requires, so it is not
  written. Windows owns resolution anyway — see the display-control milestone.

## What could not be checked from this machine

`support.iracing.com` is unreachable from the build container's network egress
policy, so iRacing's own support article could not be read directly. The key
names and their units here come from a real `renderer.ini` carrying the game's
own inline comments, which is why the confidence is `Verified` — the file
documents itself:

```ini
[MonitorSetup]
NumMonitors=3                    ; 1 or 3
MonitorWidth=545                 ; (mm) total width of each monitor (screen + bezels)
ScreenWidth=515                  ; (mm) usable width of each screen (no bezels)
ScreenAngles=15                  ; (deg) side monitor angle
RenderViewPerMonitor=1           ; 0=off 1=separate view on each monitor
```

Those comments are also why the INI editor preserves inline comments: writing a
value here while deleting `; (mm) usable width of each screen` would destroy the
only documentation of what the number means.

**Confirm on the rig:** open your own `renderer.ini` and check these five keys
exist with these spellings. If any does not, the app will tell you so by name
rather than silently skipping it — that check is automatic.

## Sources

- A real `renderer.ini` with the game's own inline comments:
  <https://github.com/CraigLager/iRacing/blob/master/renderer.ini>
- iRacing support, *Setting Up Three Monitors* (title and summary only; the site
  is blocked by this container's egress policy):
  <https://support.iracing.com/support/solutions/articles/31000171395-setting-up-three-monitors>
- Corroborating third-party guide:
  <https://simracingcockpit.gg/how-to-set-up-triple-monitors-in-iracing/>
