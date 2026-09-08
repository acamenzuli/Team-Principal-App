# 0006 — Visual language

Status: **proposed**. Written before any screen was built, as the brief
requires. The tokens live in `src/design/tokens.css`; this document is the
reasoning behind them, so future screens extend the system instead of
improvising.

## The conditions this is designed for

Not a desk. The user is strapped into a rig, 700 mm or more from a 49" panel,
in a dim room, frequently already gloved with both hands on a wheel and no
mouse within reach. Every decision below follows from that and from nothing
else.

## The choice: a pit wall timing screen, behind glass

**Revised at the owner's request.** The original system was flat and hard-edged:
hairlines, 2 px corners, no elevation. Glass was then asked for explicitly, and
this is how the two are reconciled rather than one simply replacing the other.

Glass buys depth and the sense of a built product. It costs contrast — and this
UI is read from over 700 mm away in a dim room, which is precisely the condition
where lost contrast hurts. So the rule is:

> **Glass on the chrome. Solid ground under the data.**

Translucency and blur go on the title bar, section panes, and settings sheets —
surfaces that frame. The tables, readouts and status rows sit on an opaque
surface, because nothing should ever be read *through* a blur.

That is enforced by a token rather than by discipline: `--data-bg` is opaque at
the default glass level, and only becomes translucent when the user chooses
**Full**.

| Decision | Why |
| --- | --- |
| Near-black ground (`#0A0C0F`) with a faint accent field | A dim room, and a backdrop-filter over a flat colour produces nothing — the blur needs something behind it. The field is two very low-opacity pools, far too diffuse to compete with text. |
| Glass panes for chrome, solid surfaces for data | The compromise above. |
| Corner radius 8 px on panes, 4 px on controls | 2 px read as instrumentation but fought the glass; 8 px reads as a pane rather than a pill. |
| Monospace tabular numerals for every number | Columns line up, and a live value does not jitter its neighbours. Non-negotiable on a screen whose whole job is numbers. |
| Base type 16 px, data rows 18 px, readouts 22 px | Sized for the viewing distance, not for a laptop 500 mm away. |
| Hit targets 44 px minimum | Gloved hands, and a trackball balanced on a rig rail. |

### Glass is a setting, not a style

`GlassLevel::{Off, Subtle, Full}`, stored in preferences.

- **Off** — flat, opaque, highest contrast, cheapest to draw. The honest answer
  for anyone who finds blur hard to read, and the fallback where
  `backdrop-filter` is unsupported.
- **Subtle** (default) — glass on chrome, solid data.
- **Full** — translucency on data surfaces too.

Every component reads `--glass-*` and `--data-bg`; none of them knows which
level is active.

## Accent colour

One stored value, `--accent`. Every tint in the app is `color-mix`ed from it in
CSS — washes, edges, glows, hover states, the focus ring, the headline figure on
a readout. Changing it is a single property assignment, not a re-render and not
eight hard-coded shades to keep in sync.

The foreground that pairs with it (black or white) is **computed in Rust and
tested**, so a custom colour cannot produce a button whose label is unreadable.
Worth recording how that went: the first implementation used a luminance
threshold of 0.45, which handed white to `#FF6B35` at 2.8:1 when black gives
7.4:1. The real crossover is 0.179. Rather than encode that constant, the
function now computes both contrasts and takes the better one — same answer,
nothing to get wrong. A test asserts every preset clears 4.5:1.

## Team branding

The title bar carries the user's logo and team name. Before a logo is set, a
mark generated from the accent stands in, so first run looks deliberate rather
than unfinished.

The logo is stored as a `data:` URI inside `preferences.json` rather than as a
file path: the settings file stays self-contained, survives being copied to
another machine, and the app never has to reason about a logo that has been
moved or deleted. Capped at 512 KB, and the mime type is validated in Rust —
`data:text/html,<script>…</script>` is not a logo.

## Status colour

Status is **never** carried by colour alone. Every status is icon + colour +
word, on every row, always. This is a go/no-go screen and colourblind users
exist.

| State | Colour | Glyph | Word |
| --- | --- | --- | --- |
| Passed | `#3BD16F` green | `●` | PASS |
| Warning | `#FFB02E` amber | `▲` | WARN |
| Failed / blocked | `#FF4D4D` red | `■` | FAIL |
| Running | `#5AA9FF` blue | `◐` | RUN |
| Pending / skipped | `#5C666F` grey | `○` | WAIT / SKIP |

Amber doubles as the single interactive accent. It is the pit-lane colour and
the only saturated hue in the chrome, so anything amber is either a warning or
something you can press — and context always disambiguates.

## Motion

Only where a state actually changes: a row settling into passed, the progress
line advancing, the ready panel arriving. No entrance animation on load, no
decorative transitions. `prefers-reduced-motion` removes all of it.

## Copy

- Failures say what happened and what to do: *"Pedals not detected. Check the
  USB cable — they'll connect automatically."* Never *"Error: device
  enumeration failed (0x80070002)."*
- Active voice, sentence case.
- One vocabulary. The button says **Let's race** and every reference to the flow
  uses that name.
- Never claim credit: a utility already running shows **Already running**, not
  **Started**. This is enforced by the type system — see `ActionTaken` in
  `crates/tp-model/src/profile.rs`.
