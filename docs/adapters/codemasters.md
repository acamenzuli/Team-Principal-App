# DiRT Rally 2.0 (Codemasters EGO engine)

**Confidence:** Corroborated
**Checked:** 9 September 2026
**Writes settings:** not yet

## File

`{documents}\My Games\DiRT Rally 2.0\hardwaresettings\hardware_settings_config.xml`

The file does not exist until the game has been run once — it is generated on
first launch. The app says so rather than reporting a missing file as an error.

## Why nothing is written yet

The path is corroborated across several independent sources. The element layout
is not.

There is also a known behaviour worth recording before anything writes here:
multiple sources report the game **resetting `hardware_settings_config.xml` to
defaults** under conditions that are not clearly established. An adapter that
writes a file the game then silently reverts is worse than no adapter — it
produces a setting that works once and stops, which is the hardest kind of
problem to report. Confirming what triggers the reset is part of finishing this
one, not an optional extra.

## What would finish it

One real `hardware_settings_config.xml`, plus knowing whether the game preserves
hand edits across a launch. **Inspect** covers the first half.

## Sources

- <https://www.overtake.gg/threads/dirt-rally-2-0-hardware_settings_config-xml.165184/>
- <https://www.simrig.se/documentation/control-center/game-setup/dirt-rally2.html>
- <https://www.xsimulator.net/community/threads/dirt-rally-2-hardware_settings_config-xml-always-reset-default.14127/>
- <https://www.pcgamingwiki.com/wiki/DiRT_Rally_2.0>
