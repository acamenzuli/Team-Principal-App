import { useCallback, useEffect, useState } from "react";

import { Section } from "../dashboard/primitives";
import {
  asIpcError,
  listWindows,
  placeWindow,
  stopWatchingWindow,
  type OpenWindow,
  type PixelRect,
  type WindowResult,
} from "../ipc";
import "./windowctl.css";

/**
 * Window control, exercisable by hand.
 *
 * The real use is inside the launch flow, where geometry is applied to a game
 * the app started. This panel exposes the same machinery against any window
 * that is already open, because that is the only way to test it without a game
 * — and because when a title *does* misbehave, being able to point the tool at
 * it by hand is worth having.
 */
export function WindowControl() {
  const [windows, setWindows] = useState<OpenWindow[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [rect, setRect] = useState<PixelRect>({ x: 0, y: 0, width: 1280, height: 720 });
  const [borderless, setBorderless] = useState(true);
  const [watch, setWatch] = useState(true);
  const [result, setResult] = useState<WindowResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const all = await listWindows();
      setWindows(all.filter((w) => w.plausible));
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const apply = async () => {
    if (!selected) return;
    try {
      setResult(await placeWindow({ hwnd: selected, rect, means: "outer_window", borderless, watch }));
      setError(null);
    } catch (e) {
      setResult(null);
      setError(asIpcError(e).message);
    }
  };

  return (
    <div className="wc">
      <Section title="Window control" note="Apply borderless geometry to any open window">
        {error && <p className="warn warn--hard">{error}</p>}

        <div className="wc__row">
          <label className="lf wc__pick">
            <span className="lf__label">Window</span>
            <select
              className="lf__input"
              value={selected ?? ""}
              onChange={(e) => setSelected(e.target.value || null)}
            >
              <option value="">Choose a window…</option>
              {windows.map((w) => (
                <option key={w.candidate.hwnd} value={String(w.candidate.hwnd)}>
                  {w.candidate.title || "(untitled)"} — {w.candidate.exeName ?? "?"} (
                  {w.candidate.rect.width}×{w.candidate.rect.height})
                </option>
              ))}
            </select>
            <span className="lf__helper">
              Only windows that pass the splash and tool-window filters are listed.
            </span>
          </label>
          <button className="btn btn--quiet" onClick={() => void refresh()}>
            Refresh
          </button>
        </div>

        <div className="wc__row">
          {(["x", "y", "width", "height"] as const).map((field) => (
            <label className="lf wc__num" key={field}>
              <span className="lf__label">{field}</span>
              <input
                className="lf__input num"
                inputMode="numeric"
                value={rect[field]}
                onChange={(e) => {
                  const n = Number(e.target.value);
                  if (Number.isFinite(n)) setRect((r) => ({ ...r, [field]: Math.round(n) }));
                }}
              />
              <span className="lf__helper">{field === "x" ? "negative is left of primary" : " "}</span>
            </label>
          ))}
        </div>

        <div className="wc__row wc__row--options">
          <label className="toggle">
            <input type="checkbox" checked={borderless} onChange={(e) => setBorderless(e.target.checked)} />
            <span className="toggle__track" aria-hidden="true">
              <span className="toggle__knob" />
            </span>
            <span>Borderless</span>
          </label>
          <label className="toggle">
            <input type="checkbox" checked={watch} onChange={(e) => setWatch(e.target.checked)} />
            <span className="toggle__track" aria-hidden="true">
              <span className="toggle__knob" />
            </span>
            <span>Put it back if it moves</span>
          </label>
          <button className="btn" onClick={() => void apply()} disabled={!selected}>
            Apply
          </button>
          <button className="btn btn--quiet" onClick={() => void stopWatchingWindow()}>
            Stop watching
          </button>
        </div>

        {result && (
          <div className="wc__result">
            <p>
              <strong>{result.title || "(untitled)"}</strong> — read back after the change, not
              assumed from a return value.
            </p>
            <table className="grid">
              <tbody>
                <tr>
                  <td>Asked for</td>
                  <td className="num">{fmt(result.requested)}</td>
                </tr>
                <tr>
                  <td>Outer window</td>
                  <td className="num">{fmt(result.actualOuter)}</td>
                </tr>
                <tr>
                  <td>Client area</td>
                  <td className="num">{fmt(result.actualClient)}</td>
                </tr>
                <tr>
                  <td>Frame</td>
                  <td>{result.borderless ? "removed" : "still present"}</td>
                </tr>
                <tr>
                  <td>Watchdog</td>
                  <td>{result.watching ? "running" : "off"}</td>
                </tr>
              </tbody>
            </table>
          </div>
        )}

        <p className="hint">
          To test the watchdog without a game, run <span className="num">tp-fakegame.exe</span> from
          the CI artifacts — it opens a window and then resets its own geometry on a timer, which is
          what sims do when their render device initialises.
        </p>
        <p className="hint">
          Placing a window on a named rig screen — rather than by typing a rectangle — needs the
          link from a rig screen to its monitor's pixel rectangle, and arrives with launch
          orchestration in the next milestone.
        </p>
      </Section>
    </div>
  );
}

const fmt = (r: PixelRect) => `${r.width}×${r.height} at ${r.x},${r.y}`;
