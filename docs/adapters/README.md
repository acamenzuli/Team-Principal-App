# Adapters — the verification protocol

The brief on this is explicit, and it is the rule:

> Search for the current config file locations and key names, and check the date
> on your sources. I would rather have four verified adapters than twelve
> guessed ones.

So every adapter has a file in this folder recording **where its key names came
from**, with the date of the source and what could not be confirmed. An adapter
with no such file does not ship.

## Confidence levels

`tp_model::adapter::Confidence` is part of the wire type and is shown in the UI,
because these two claims are not the same and must not look the same:

| Level | Means |
|---|---|
| `Verified` | Key names read from a real file of that game, or from the developer's own documentation. |
| `Corroborated` | Agreed across independent second-hand sources. Not seen in a shipped file. |

Orthogonal to both, and shown in the UI as its own badge: **whether the entry
writes anything at all.** An entry with no `writes` is a *recognised title with
an unconfirmed layout* — the app knows the game, finds its config, and will show
you what is in it without touching it.

That is the honest middle state between "supported" and "never heard of it", and
it is where most of the catalog starts. It is also what makes covering a dozen
sims possible without guessing at any of them: a title is listed the moment its
config file is located, and starts writing the moment one real file confirms
what its settings are called.

## Turning a listed title into a writing one

The **Inspect** button on the Game Settings tab reads the game's config on the
user's own machine and reports its structure — section and key names for INI,
element names for XML, **and no values at all**. What an adapter needs is what
the settings are *called*; the numbers are the user's own business, and a
listing with nothing personal in it is one they can read and send without having
to weigh anything up.

One of those turns a forum-post guess into a fact. That is the whole loop, and
it is why this scales.

## The catalog is data

Entries live in `crates/tp-model/data/adapters.json` and are embedded at build
time. Adding a title is a JSON entry: where the file is, what format it is, and
which of the rig's derived values go under which keys.

The vocabulary of derived values — screen width, monitor width, eye distance,
bezel gap, side angle, pixel dimensions, screen count, in millimetres,
centimetres or metres — is shared across every entry, so the arithmetic lives in
one place and a correction to how a measurement is derived fixes every title at
once.

## The runtime check that makes this safe

Documentation of key names goes stale. Games rename things between versions,
forum posts are undated, and a key that is *almost* right is silently ignored by
the game — leaving an app that reports success and changed nothing.

So `tp_model::Ini::set` **refuses to create a key the file does not already
have.** Every key an adapter writes is therefore verified against the user's own
file, on their machine, at the moment of writing. A name that is wrong for their
version becomes a message naming the key and listing what that section really
contains, instead of a silent no-op.

This is the strongest form of verification available, and it is the reason a
`Corroborated` adapter is safe to ship: the worst case is a setting that is
skipped and reported, never one that is silently wrong.

## Adding an adapter

1. Find the config file and its key names. Prefer, in order: the developer's
   documentation, a real file with the game's own inline comments, a widely
   corroborated third-party source.
2. Write `docs/adapters/<title>.md` from the template below, with URLs and
   dates.
3. Add the entry to `crates/tp-model/data/adapters.json`. Only reach for Rust if
   the game needs a *derived value* the vocabulary does not yet have.
4. Every `Edit` carries a `because`. A number in a diff with no reason is a
   number nobody can check against their own tape measure.
5. Whatever the adapter cannot express goes in `warnings`, in the user's words.
   Silently averaging two different values is worse than saying you did.

## Template

```markdown
# <Title>

**Confidence:** Verified | Corroborated
**Checked:** <date>

## File

Path, and how it is found.

## Keys written

| Section | Key | Units | Source |

## Keys deliberately not written

And why.

## Sources
```
