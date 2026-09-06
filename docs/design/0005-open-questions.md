# 0005 — What I need from you before milestone 1

Everything here blocks either the rig schema or the adapter ordering. The
PowerShell snippets are for *you*, run once, to gather data — the app itself
never shells out to anything.

---

## A. Displays

Run this and paste the output. It gives models, EDID serials, physical size and
the exact virtual-desktop rectangles including negative coordinates:

```powershell
Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorID | ForEach-Object {
  [PSCustomObject]@{
    Mfg    = -join [char[]]($_.ManufacturerName | ? {$_ -ne 0})
    Name   = -join [char[]]($_.UserFriendlyName | ? {$_ -ne 0})
    Serial = -join [char[]]($_.SerialNumberID   | ? {$_ -ne 0})
    Inst   = $_.InstanceName
  }
} | Format-List

Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorBasicDisplayParams |
  Select InstanceName, MaxHorizontalImageSize, MaxVerticalImageSize | Format-Table

Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.Screen]::AllScreens |
  Select DeviceName, Primary, @{n='Bounds';e={$_.Bounds}} | Format-Table -AutoSize
```

Note `MaxHorizontalImageSize` is in **whole centimetres** — a 1193 mm panel
reports `119`. That ±5 mm is why the app parses raw EDID from the registry
(mm precision, from the detailed timing descriptor) instead of trusting WMI, and
why manual override exists.

Also tell me: **refresh rates** you actually run each panel at, and whether all
three are on the same GPU.

## B. Curvature

1. Centre screen curvature radius — `1000R`, `1800R`, or from the spec sheet.
2. Are both 27" side panels **flat**? If either is curved I need its radius too.
3. For the centre panel, the datasheet's quoted screen width — and whether the
   datasheet calls it a developed/arc dimension or a chord. If unclear, just
   give me the model number and I will chase it down.

## C. Angles and seating — how to measure these properly

**Side-screen angle.** Do not eyeball it and do not use a phone level (a level
reads pitch and roll, not the yaw you need). Use the offset method:

1. Work at screen-centre height, rig on level ground.
2. Run a taut string across the **face** of the centre screen, touching both
   left and right visible-image corners, extended well past both sides. That
   string is the centre-screen plane.
3. For the left panel, measure perpendicular from the string to its **inner**
   visible corner (`d_in`) and to its **outer** visible corner (`d_out`).
   Angling inward means the outer corner comes toward you, so `d_out > d_in`.
4. `angle = asin((d_out - d_in) / W)` where `W` is that panel's visible width.

   Sanity check: a 27" 16:9 panel is 597 mm wide, so at 45° you should measure
   `d_out - d_in ≈ 423 mm`; at 60°, ≈ 518 mm.

Repeat for the right panel — **measure both, do not assume symmetry**. Rigs
rarely are, and the app supports asymmetry natively.

Just give me the four raw numbers per side (`d_in`, `d_out`, `W`, and which
panel) if you would rather I do the arithmetic.

**Eye-to-centre-screen distance.**

1. Sit in your normal driving position — hands on the wheel, head where it
   actually sits, headrest contact if you use one.
2. Measure horizontally from the bridge of your nose to the **nearest point of
   the centre screen's visible image** (its horizontal centre, at mid-height).
   Not the bezel, not the desk edge, not the monitor stand.
3. On a curved panel that nearest point is the middle of the curve. The app
   derives the chord-plane distance from it — you do not need to work that out.
4. Take three readings and average. At 700 mm, ±10 mm is about ±0.6° of FOV and
   does not matter; ±50 mm does.

**Eye height.** Floor-to-eye, and floor-to-vertical-centre-of-centre-screen. I
want both numbers, not the difference — it is easier to measure and I can
sanity-check the pair.

**Lateral offset.** Do you sit dead centre on the centre screen, or offset? If
offset, how far and which way.

**Bezels.** Per edge, per panel: the dark border between the visible image and
the outside of the chassis. Left and right often differ from top and bottom, and
the two 27"s may not match each other. Also the **mount gap** — any extra air
between adjacent panel chassis beyond the bezels themselves.

## D. Peripherals — VID/PID

Bulk method, much faster than clicking through Device Manager:

```powershell
Get-PnpDevice | Where-Object { $_.InstanceId -like 'HID\VID_*' } |
  Select-Object Status, FriendlyName, InstanceId |
  Sort-Object FriendlyName | Format-Table -AutoSize -Wrap
```

The `InstanceId` looks like `HID\VID_346E&PID_0004\7&1a2b3c4d&0&0000` — that
gives me vendor ID, product ID and the stable instance path in one line. Paste
the whole block and tell me which line is which device.

If you would rather do it per-device: Device Manager → Human Interface Devices
→ right-click → **Properties → Details** tab → Property dropdown → **Hardware
Ids** (and also **Device instance path**, and **Bus reported device
description**, which frequently holds the real product name when Windows shows
"HID-compliant game controller").

Then run `joy.cpl` and paste the list **in the order it displays them** — that
ordering is close to the DirectInput enumeration order, and it is the thing that
silently destroys in-game bindings when it changes.

Devices I have on the list, correct me if it is wrong or incomplete:
Invicta S-Series pedals, LaPrima wheelbase, Sim-Lab handbrake, HGP shifter,
Forte button box, Sim-Lab SQ1 shifter.

## E. vJoy

1. Which of the six route through vJoy, and which are direct HID?
2. What feeds each vJoy device — Joystick Gremlin, a vendor tool, a custom
   script, something else? I need the exact process name for the readiness gate.
3. Which vJoy device IDs (1–16) are in use?
4. vJoy driver version installed.

## F. Utilities

For each app that must be running before a session — SimHub plus anything else:

1. Exact executable name and install path.
2. Launch order and any real dependencies between them.
3. **How you can tell it is actually ready**, as opposed to merely running. For
   SimHub in particular: does its process go input-idle when ready, does it open
   a named window, does it listen on a TCP port? This is the difference between
   a real gate and a hardcoded five-second sleep, and I would rather ask than
   guess.
4. Which ones should be closed on teardown and which should stay up.

## G. Sims, ranked

Which titles do you *actually* play, in order of how much you race them? Wave
one is three adapters and I want them to be your top three, not the top three
from a generic list. Also flag anything you own but never touch, so I do not
spend verification effort on it.

## H. Session and product decisions

1. Default session mode — **centre only** or **full span**? (Per-profile
   overrides exist either way; this is the default a new profile is created
   with.)
2. Should SimHub stay visible on the side screens during a session? If yes, that
   effectively makes centre-only the default and means the display module must
   *not* touch the side panels at launch.
3. Product name for the installer, window title and `%APPDATA%` folder. Repo is
   "Team-Principal-App" — is "Team Principal" the shipping name?
4. Panic-restore hotkey — is `Ctrl+Alt+Shift+R` free on your system, or does
   SimHub / a vendor tool already claim it?

---

## Milestone 1 is unblocked by

Only **H3** (folder name). Everything else gates milestones 3–5. If you answer
H3 and sign off on the schemas in 0003 and 0004, I can build the skeleton while
you are measuring.
