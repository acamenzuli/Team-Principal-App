# 0001 — Stack decision

Status: **proposed**, awaiting sign-off.

## Tauri v2, not Electron

I agree with your call. Recording the reasoning so it is settled:

- **Licensing is the deciding factor.** An Electron app ships its business logic
  as readable JavaScript in an `app.asar` that anyone can unpack with a one-line
  npm command. A perpetual-licence product whose licence check lives there is
  cracked in an afternoon. Tauri compiles the Rust half to a native binary; the
  webview only ever sees the UI.
- **Win32 access is first-class.** The `windows` crate is Microsoft's own
  generated binding. Everything this app needs — CCD (`QueryDisplayConfig`),
  SetupAPI, HID, `CM_Register_Notification`, DirectInput8, Job Objects — is
  there with correct types. Electron would need `koffi` FFI declarations
  hand-written for every struct, and the CCD structs are large, nested and
  version-tagged. That is a bug farm.
- **Installer size and updater.** ~10 MB NSIS versus ~90 MB, and a signed
  updater with channel support built in.

Two costs, stated honestly so neither is a surprise later:

1. **WebView2 dependency.** Tauri renders in the Edge WebView2 runtime. It ships
   with Windows 11, so on target it is a non-issue, but it means UI rendering
   depends on a component we do not control or version-pin.
2. **Smaller UI ecosystem gravity.** Irrelevant here — the frontend is ordinary
   React and the layout editor is a `<canvas>`. Nothing needs a Chromium-only API.

Neither changes the answer. Building on Tauri v2.

## DPI awareness

The manifest declares Per-Monitor V2 from the first commit:

```xml
<dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
<dpiAware     xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
<longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
```

Both the 2016 and 2005 elements are present because older loaders read only the
latter. `longPathAware` is there because Steam library paths on a secondary
drive plus a UE5 `Saved\Config\WindowsNoEditor\` chain gets close to `MAX_PATH`.

Rules that follow from this and are enforced in review:

- Every geometry value crossing the Rust/TS boundary is in **physical pixels**
  or **millimetres**, never in DIPs. Types are named so this cannot be confused:
  `PhysicalPx`, `Mm`, `Deg`.
- The webview's own CSS pixels are a separate universe. The layout editor
  converts once, at the point of drawing, using a scale factor it holds
  explicitly. There is no implicit conversion anywhere.
- A milestone-1 test asserts the process is actually
  `PROCESS_PER_MONITOR_DPI_AWARE_V2` at runtime via
  `GetAwarenessFromDpiAwarenessContext`, so a Tauri upgrade cannot quietly
  regress it.

## Elevation

Default launch is unelevated. The problem is that User Interface Privilege
Isolation makes a *silent* failure the normal case: `SetWindowLongPtrW` against a
window owned by a higher-integrity process fails, and its return value is
indistinguishable from "the previous style was 0".

So the window module never trusts a return code. Every apply is:

1. `SetLastError(0)` before each call, check `GetLastError()` after.
2. **Read back** — `GetWindowRect` and `GetWindowLongPtrW` — and compare against
   what was asked for, within a tolerance of 0 px.
3. If read-back disagrees, classify: access-denied (offer elevation), or the
   game fought back (hand it to the watchdog).

Elevation is a relaunch via `ShellExecuteW` with the `runas` verb, offered
inline on the failing preflight row, never on startup. The relaunch must carry
the current display snapshot across so the panic-restore hotkey keeps working
through the transition — otherwise there is a window during which a bad topology
cannot be undone.

## Panic restore

`Ctrl+Alt+Shift+R` is registered with `RegisterHotKey` on a dedicated Rust
thread with its own message loop. It must **not** depend on the webview: if a
topology change blanks every panel, the webview may be unpainted or on a monitor
that no longer exists. The handler restores the last known-good snapshot
directly from the Rust side and only then notifies the UI.

## Signing

Azure Trusted Signing, wired through Tauri's NSIS `signCommand` hook. Noted as a
required pre-launch cost, roughly $10/month plus an identity validation. An
unsigned installer for an app that repositions other applications' windows and
edits game files will be SmartScreen-blocked, and no amount of engineering
compensates for that.
