# The Unreal Engine titles

Assetto Corsa Competizione, Assetto Corsa EVO, Assetto Corsa Rally.

**Confidence:** Corroborated
**Checked:** 9 September 2026
**Writes settings:** none of them, yet

## Files

| Title | Path |
| --- | --- |
| ACC | `{localappdata}\AC2\Saved\Config\WindowsNoEditor\GameUserSettings.ini` |
| AC EVO | `{localappdata}\AC\Saved\Config\Windows\GameUserSettings.ini` |
| AC Rally | `{localappdata}\ACRally\Saved\Config\Windows\GameUserSettings.ini` |

ACC's is corroborated. The other two follow Unreal's convention and are
**inferred from it**, which is not the same thing — the app will look there, and
if the file is not found that is information rather than a failure.

Note the folder-name trap: ACC's is `AC2`, and Unreal 5 titles use `Windows`
where Unreal 4 used `WindowsNoEditor`. Neither is guessable from the game's
name.

## Why nothing is written to any of them

**Unreal keeps a second copy of the resolution.** Alongside `ResolutionSizeX` /
`ResolutionSizeY` it stores `LastUserConfirmedResolutionSizeX` /
`...SizeY`, and reverts to the confirmed pair under conditions this project has
not established. An adapter that writes only the first pair produces a change
that appears to work and is silently undone at the next launch — the worst
failure mode available, because it presents as intermittent.

Writing both pairs is the obvious guess. It is still a guess, and this is
exactly the case the verification protocol exists for.

Both EVO and Rally are additionally in early access, where config layouts move
between builds. An adapter written against one build and shipped against another
is a support burden with no upside.

## What would finish them

For each title, one real `GameUserSettings.ini`, plus one observation: set a
resolution by hand, launch the game, close it, and check whether the value
survived. That single answer settles the `LastUserConfirmed` question for all
three at once.

**Inspect** on the Game Settings tab produces the file's structure with no
values in it.
