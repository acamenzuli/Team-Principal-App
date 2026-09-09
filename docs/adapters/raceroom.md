# RaceRoom Racing Experience

**Confidence:** Corroborated
**Checked:** 9 September 2026
**Writes settings:** not yet

## File

`{documents}\My Games\SimBin\RaceRoom Racing Experience\UserData\graphics_options.xml`

Typed elements, in the shape:

```xml
<screenWidth type="uint32">2560</screenWidth>
<screenHeight type="uint32">1080</screenHeight>
```

## Why nothing is written yet

`screenWidth` and `screenHeight` are corroborated, and they are only half the
job — RaceRoom also needs its aspect ratio setting changed before triple
resolutions become selectable, and the element that carries it was not
established. Writing a resolution the game will not offer produces a game that
launches at the old one, which looks like the adapter failing silently.

The `type="uint32"` attribute is also a hint that this file is validated on
read. Writing a value of the wrong type could be rejected wholesale rather than
per-key, which is a different failure mode from anything else in the catalog and
worth confirming before touching it.

## What would finish it

One real `graphics_options.xml` from a triple-screen install. **Inspect** gives
the element list.

## Sources

- <https://steamcommunity.com/app/211500/discussions/1/3057363335736032363/>
- <https://steamcommunity.com/app/211500/discussions/1/3395163747112620984/>
- <https://forum.kw-studios.com/index.php?threads/triple-screen-setup.19733/>
