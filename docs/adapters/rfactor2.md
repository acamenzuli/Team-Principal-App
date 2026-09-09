# rFactor 2 and Le Mans Ultimate

**Confidence:** Corroborated
**Checked:** 9 September 2026
**Writes settings:** not yet

Two titles, one engine, one config layout. Le Mans Ultimate is built on rFactor
2 and keeps its triple-screen settings in the same file with the same key names.

## Why these two matter more than most

They ask for the same thing iRacing does: **physical measurements**, not a field
of view. The triple-screen values are metres and degrees:

```ini
ViewParams=(0.610, 0.340, 0.600, 60.000, 0.020)
LeftView=(0.610, 0.340, 0.600, 60.000, 0.020)
RightView=(0.610, 0.340, 0.600, 60.000, 0.020)
```

Corroborated as: screen width (m), screen height (m), eye distance (m), side
angle (deg), bezel gap (m).

That is the rig model exactly. Every one of those five numbers is already solved
in `tp-geometry`, so this adapter is a unit conversion with nothing to get
subtly wrong — the same shape as iRacing's, which is why the catalog's `Tuple`
value type exists.

## File

* rFactor 2: `<Documents parent>\rFactor2\UserData\Config_DX11.ini`
* Le Mans Ultimate: `<Documents parent>\Le Mans Ultimate\UserData\Config_DX11.ini`

Both install under the user profile rather than under Documents proper, which is
why the path templates step up a level. **This is the least confident part of
the entry** — installs vary, and Steam's own library folder is another
possibility. It is the first thing to check against a real machine.

## Why nothing is written yet

Three things are corroborated but not confirmed against a shipped file:

1. The exact path on a real install.
2. Whether the five tuple members are in the order given above.
3. Whether `ViewParams` is the centre screen or a global default, with
   `LeftView` / `RightView` overriding it.

Getting (2) wrong would put the eye distance where the screen height goes, which
does not fail — it renders a plausible, wrong image. That is precisely the
failure this protocol exists to prevent, so the adapter reads the file and does
not write it.

## What would finish it

One real `Config_DX11.ini` from either title, with triple screens already
configured in-game. The **Inspect** button on the Game Settings tab produces
exactly what is needed, with no values in it.

With that file, both adapters become `Verified` in one change — they share the
entry shape, so confirming one confirms the other's key names.

## Sources

- <https://steamcommunity.com/app/2399420/discussions/0/7221029098493259626/>
- <https://lemansultimate.wiki.gg/wiki/Settings_and_Configuration>
- <https://docs.departedreality.com/dr-sim-manager/general/sources/le-mans-ultimate/>
- <https://wiki.simracingonlinux.com/games/le-mans-ultimate/>

Most are unreachable from the build container's egress policy and were read
through search summaries rather than directly, which is a further reason the
level is `Corroborated`.
