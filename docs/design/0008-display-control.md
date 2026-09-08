# 0008 — Display control

The most dangerous thing this app does. A mode a panel cannot show is a black
screen, and a black screen on a rig whose only input is that screen is a machine
you cannot reach without holding the power button.

Everything below exists to make that outcome recoverable rather than to make it
unlikely. Unlikely is not good enough for something that happens on somebody
else's hardware.

## Four layers, outermost first

1. **Validation that needs no GPU** (`tp_model::topology`). One primary, primary
   at the origin, no overlaps, a contiguous desktop. Tested on a Linux runner.
2. **Validation against the driver's own mode list.** The one check that really
   does prevent a black screen, and the only one that needs the platform.
3. **Read-back.** `DISP_CHANGE_SUCCESSFUL` means *accepted*, not *achieved*.
4. **Confirm-or-revert, with the countdown in Rust.**

## Why the primary must sit at the origin

Windows expresses the whole virtual desktop relative to the primary monitor's
top-left corner. A plan that says "primary at 100,0" is not refused — Windows
shifts *every other monitor* by -100 to make the primary the origin, and the
layout that comes back is not the one that went in.

So the model refuses it, with a message that says why. The alternative is an
app that appears to work and silently produces a different desktop.

## Why a stranded monitor is refused

Windows requires a contiguous desktop. A monitor placed with a gap between it
and everything else is *moved* to close the gap, silently. Corner contact does
not count: two screens meeting at a single point are treated as disconnected.

Both are checked by walking shared edges, in `topology::disconnected`.

## Staging and one commit

Every device is written with `CDS_UPDATEREGISTRY | CDS_NORESET`, which changes
the registry and not the desktop. A single final `ChangeDisplaySettingsExW(NULL,
NULL, ...)` applies the lot.

Applying device by device instead would walk the desktop through intermediate
states — one monitor moved and the next not yet, so they overlap — and Windows
rearranges those. The end state would depend on the order of a loop.

Each device is also offered with `CDS_TEST` first, which changes nothing and
answers whether the mode would be accepted. That turns a black screen into a
message.

## Switching an output off

A mode of zero width and height. Not `CDS_DISABLE` with a real mode, which does
nothing. This is not documented anywhere prominent and is easy to get wrong.

## The countdown lives in Rust

**If the change made the screen unreadable, the user cannot click Keep.** That
single sentence decides the whole design:

* A timer in the frontend is a timer on a screen that may have just gone black,
  in a WebView that is being resized and re-composited — exactly the thing that
  stops painting.
* The countdown thread holds the snapshot. The revert would still happen if the
  WebView had crashed outright.
* Silence means revert. Fifteen seconds, the same as Windows' own applet,
  because everybody already knows that dialog.

The UI draws the number. It does not own it.

## The revert path is the apply path

A plan and a snapshot have the same shape, so "apply this plan" and "put it back
how it was" are one function with different arguments. The code that rescues the
user is therefore the code that gets exercised on every successful apply, rather
than a separate path that only runs when something has already gone wrong.

## A read-back mismatch reverts without asking

If the desktop does not match the plan, it is put back immediately and the user
is never asked to confirm it. The answer to "do you want to keep this" is
already no when what happened is not what was requested.

## The panic hotkey

Ctrl+Alt+Shift+R. Three modifiers, because nothing hits that by accident on a
wheel or a button box, and no sim binds it.

`RegisterHotKey` binds to a *thread*, not a window, and delivers `WM_HOTKEY` to
that thread's queue — so it owns a thread with its own message loop. Registering
it on the UI thread would mean it stops being delivered whenever that thread is
busy, which is precisely when it is needed.

Registration can fail because another program already owns the combination.
**That is reported, not swallowed.** A safety net the user believes in and that
does nothing is worse than none at all, so the UI says so in as many words.

## Snapshots on disk

`%APPDATA%\Team Principal\snapshots\<uuid>.json`, written before anything
changes, twenty kept.

In memory would be enough for the countdown. It is not enough for a crash or a
power cut in the middle of a change, which leaves the desktop changed with
nothing running to undo it. The file is what lets the next start offer to put it
back.

## What is deliberately not here

* **Detach and reattach of outputs via the CCD API.** `SetDisplayConfig` can do
  topology changes `ChangeDisplaySettingsEx` cannot — cloning, extending across
  adapters. It is also far easier to get catastrophically wrong, and nothing in
  the brief needs it yet.
* **Per-monitor DPI changes.** They require a signed-out session to take effect
  reliably, and an app that silently half-applies one is worse than an app that
  says it does not do that.
* **Applying a display plan as part of a launch.** The step action exists
  (`ApplyDisplaySnapshot`) and still reports that it is not wired up. Putting a
  countdown inside a preflight needs the preflight to be visible on the screen
  that is about to change, which is a design question this milestone does not
  answer.
