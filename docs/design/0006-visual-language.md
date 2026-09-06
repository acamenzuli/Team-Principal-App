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

## The choice: a pit wall timing screen

The brief explicitly rules out the generic dashboard look — identical rounded
cards with soft shadows, tracked-out all-caps eyebrow labels, arrows appended to
button text. So the app is built as an **instrument**, not a web page.

| Decision | Why |
| --- | --- |
| Near-black ground (`#0B0D0F`), not dark grey | A dim room. Grey glows; black recedes and lets the status colours carry. |
| Hairline rules, not cards | One shared 1 px rule (`#1E242A`) separates regions. No shadows, no elevation, no nested panels. A timing screen is a grid of rows. |
| Corner radius: 2 px maximum | Squared corners read as instrumentation. Rounded ones read as a web app. |
| Monospace tabular numerals for every number | Columns line up, and a value updating live does not jitter its neighbours. Non-negotiable on a screen whose whole job is numbers. |
| Base type 16 px, data rows 18 px, readouts 22 px | Sized for the viewing distance, not for a laptop 500 mm away. |
| Hit targets 44 px minimum | Gloved hands, and a trackball balanced on a rig rail. |

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
