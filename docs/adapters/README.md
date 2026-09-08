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

There is deliberately no third level below these. A key name nobody can
corroborate does not get written at all.

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
3. Add the plan function in `crates/tp-model/src/adapter.rs`, with a test that
   asserts the values it derives from a known rig.
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
