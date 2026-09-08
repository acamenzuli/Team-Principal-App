# 0007 — Display enumeration

Status: **implemented**, milestone 2. Read-only — nothing in this milestone
changes a display setting.

## Three sources, because no one API has it all

| Source | What it is used for | What it must *not* be used for |
| --- | --- | --- |
| CCD (`QueryDisplayConfig` + `DisplayConfigGetDeviceInfo`) | the monitor device path, the friendly name, the source-to-target mapping | resolution |
| `EnumDisplaySettingsExW` | the mode actually running, and position in virtual desktop space | physical size |
| EDID, read from the registry | physical size in millimetres, and the identity a rig screen binds to | **resolution** |

That last exclusion is not a preference. It is a format limit, found while
writing the parser tests.

## EDID cannot report a wide panel's native resolution

The detailed timing descriptor gives horizontal and vertical active pixels
**12 bits each** — a ceiling of 4095 — and the pixel clock is a `u16` in units
of 10 kHz, a ceiling of 655.35 MHz.

A 5120×1440 panel at 240 Hz needs 5120 pixels and about 1.94 GHz. Neither fits.
Such monitors advertise their real modes in CTA-861 or DisplayID **extension
blocks**, and their base block carries some lesser fallback timing.

The first version of the parser fixture claimed 5120 in the base block. The test
failed with `left: 1024` — the value silently truncated to 12 bits, exactly as
the format requires. The parser was right and the fixture was wrong.

Two things came out of that:

- `preferred_mode` is documented as *the base block's preferred timing*, not the
  native resolution, and the app never treats it as the latter.
- The test-blob builder now **asserts** rather than truncating, so a fixture
  cannot quietly lie again. `the_base_block_cannot_express_a_5120_wide_panel`
  pins the limit in a test.

## EDID lives in the registry

There is no display API that returns it. The blob is at:

```
HKLM\SYSTEM\CurrentControlSet\Enum\DISPLAY\<hardware id>\<instance>\Device Parameters
    EDID   (REG_BINARY)
```

and both path components come straight out of the CCD device path:

```
\\?\DISPLAY#SAM7179#5&1234abcd&0&UID4353#{e6f07b5f-…}
            ^hardware id  ^instance id
```

`WmiMonitorBasicDisplayParams` is the usual alternative and reports physical
size in **whole centimetres** — a 1193 mm panel comes back as 119. At 700 mm
that rounding is about 0.2° of FOV, with nothing telling the user it happened.
The raw blob carries millimetres in its detailed timing descriptor, so the app
reads the blob and records which source each measurement came from.

The key derivation is a pure string transform in `tp_edid::source`, tested —
including that a malformed device path cannot walk out of the intended key.

## Strictness: warn, never drop a monitor

A bad EDID checksum does **not** reject the blob. Shipping monitors get this
wrong, and refusing to parse means the user's screen is silently absent from the
app with no explanation. The checksum result is reported and logged instead.

Same principle throughout: a monitor with no readable EDID still appears, with a
placeholder identity and its physical size shown as *not reported* rather than
invented. A missing size is `None`, never `0` — a zero would sail into the FOV
calculator and produce a nonsense angle.

## Dead regions

The virtual desktop is the bounding box of every monitor. On a rig with panels
of different heights, parts of that box map to no panel; a window placed there
is addressable and invisible.

`tp_geometry::desktop_layout` computes them by coordinate compression: every
monitor edge becomes a grid line, so each cell is wholly covered or wholly
empty; uncovered cells are merged horizontally and then vertically. The tests
assert a partition invariant — bounding box area equals covered area plus dead
area, with no overlap between dead regions — which is what makes the result
trustworthy rather than plausible.

It is computed on the `DisplayProvider` trait rather than in each
implementation, so the real and mock providers cannot disagree about it.

## Pixel pitch

Reported per monitor, and flagged when it differs from the left-hand neighbour
by more than 10%. Where it does, a bezel gap measured in millimetres **cannot**
be converted to pixels across that seam, and the app says so instead of
returning a number that looks right.

Worth knowing which case you are in: a 5120×1440 49" and a 27" 1440p panel have
the same pitch to within a fraction of a percent — the 49" is two 27" 1440p
panels in one chassis. Against a 27" **1080p** side panel it differs by about a
third.

## What is deliberately not here

- **Any display change.** Milestone 9, behind the confirm-or-revert countdown
  and the panic hotkey.
- **`SetDisplayConfig`.** Enumeration needs CCD only for reading; the write path
  arrives with the safety rails.
- **Physical-scale layout.** This milestone draws *pixel* space. The layout
  editor in milestone 4 draws physical millimetres, which is a different
  picture and answers a different question.
