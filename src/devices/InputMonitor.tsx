import { useEffect, useMemo, useRef, useState } from "react";

import {
  onInput,
  onInputStatus,
  startInputMonitor,
  stopInputMonitor,
  type InputFrame,
  type InputStatus,
} from "../ipc";

/**
 * Live axes and buttons for one device.
 *
 * This is how you confirm a pedal set is actually working, and it turns a
 * support conversation from twenty minutes of guessing into "press the brake
 * and tell me if the bar moves".
 *
 * Two things this panel refuses to do:
 *
 * - **Wait in silence.** The command that starts the monitor returns as soon as
 *   the reading thread exists, so its success means nothing about the device.
 *   Everything the thread learns arrives on a status event, and the header says
 *   which state we are in — opening, listening, paused, or failed and why.
 * - **Draw only what has moved.** Every axis and button the device *declares*
 *   is drawn the moment it is open, at rest. A panel that filled in as you
 *   moved things could never tell you a pedal was missing.
 *
 * Each control is then ticked once it has been exercised, which is the actual
 * question being asked: not "does the device exist" but "does this pedal work".
 */
export function InputMonitor({
  instancePath,
  name,
  onClose,
}: {
  instancePath: string;
  name: string;
  onClose: () => void;
}) {
  const [frame, setFrame] = useState<InputFrame | null>(null);
  const [status, setStatus] = useState<InputStatus | null>(null);

  // Which controls have been exercised since this panel opened. Refs, not
  // state: they are written on every frame and only ever read during the
  // render that frame causes.
  const panel = useRef<HTMLDivElement>(null);
  const movedAxes = useRef<Set<number>>(new Set());
  const restAxes = useRef<Map<number, number>>(new Map());
  const usedButtons = useRef<Set<number>>(new Set());

  // Opening a panel has to be visible. On a long device list the row being
  // tested can sit below the fold, and a panel that appears off-screen is
  // indistinguishable from a button that did nothing. "nearest" so an already
  // visible panel does not yank the page.
  useEffect(() => {
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    panel.current?.scrollIntoView({ block: "nearest", behavior: reduced ? "auto" : "smooth" });
  }, []);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let cancelled = false;

    // Both listeners are in place *before* the monitor is started, and that
    // ordering is the whole thing. `listen` is asynchronous: it round-trips to
    // the backend to register. Starting the monitor first meant the device was
    // opened, its descriptor read and both statuses published while the
    // listener was still being registered — and Tauri does not replay events,
    // so they were gone. The panel then sat on "Starting…" for a device that
    // had opened perfectly, and never drew the axes and buttons that arrived
    // with the status it missed.
    void (async () => {
      const [stopFrames, stopStatus] = await Promise.all([
        onInput((next) => {
          if (next.instancePath !== instancePath) return;
          next.axes.forEach((a, i) => {
            const rest = restAxes.current.get(i);
            if (rest === undefined) restAxes.current.set(i, a.unipolar);
            // Five per cent of travel. Below that is noise from a
            // potentiometer sitting still, and a tick that appears on its own
            // proves nothing.
            else if (Math.abs(a.unipolar - rest) > 0.05) movedAxes.current.add(i);
          });
          next.buttons.forEach((down, i) => {
            if (down) usedButtons.current.add(i);
          });
          setFrame(next);
        }),
        onInputStatus((next) => {
          if (next.instancePath === instancePath) setStatus(next);
        }),
      ]);

      if (cancelled) {
        stopFrames();
        stopStatus();
        return;
      }
      unlisteners.push(stopFrames, stopStatus);

      try {
        await startInputMonitor(instancePath);
      } catch (e) {
        setStatus({
          kind: "failed",
          instancePath,
          message: e instanceof Error ? e.message : String(e),
        });
      }
    })();

    return () => {
      cancelled = true;
      unlisteners.forEach((f) => f());
      void stopInputMonitor(instancePath);
      movedAxes.current.clear();
      restAxes.current.clear();
      usedButtons.current.clear();
    };
  }, [instancePath]);

  // The declared shape is what gets drawn. Frames fill values into it; they
  // never decide how many rows there are, so the panel cannot shrink when a
  // report happens to omit something.
  const declared = status?.kind === "listening" ? status : null;
  const axisNames = useMemo(
    () => declared?.axes ?? frame?.axes.map((a) => a.name) ?? [],
    [declared, frame],
  );
  const buttonCount = declared?.buttons ?? frame?.buttons.length ?? 0;

  const exercised = movedAxes.current.size + usedButtons.current.size;
  const total = axisNames.length + buttonCount;

  return (
    <div className="monitor" ref={panel}>
      <header className="monitor__head">
        <div>
          <h3 className="monitor__title">{name}</h3>
          <p className="monitor__state">{describe(status, frame !== null)}</p>
        </div>
        <button className="btn btn--quiet" onClick={onClose}>
          Close
        </button>
      </header>

      {status?.kind === "failed" && (
        <p className="warn warn--hard">
          {status.message}
          <span className="monitor__why">
            Some peripherals only report while their vendor software is running, and a few refuse
            to be opened at all while another program holds them.
          </span>
        </p>
      )}

      {status?.kind === "suspended" && (
        <p className="warn">
          Paused while a session is running. The monitor stands down during a race — nothing here
          can take input from a game, and it stays out of the way regardless.
        </p>
      )}

      {status?.kind !== "failed" && (
        <>
          <div className="axes">
            {axisNames.length === 0 && status?.kind === "listening" && (
              <p className="note">This device declares no axes — buttons only.</p>
            )}
            {axisNames.map((axisName, i) => {
              const reading = frame?.axes[i];
              const moved = movedAxes.current.has(i);
              return (
                <div className={`axis${moved ? " axis--seen" : ""}`} key={`${axisName}-${i}`}>
                  <span className="axis__name">{axisName}</span>
                  <span className="axis__track">
                    <span
                      className="axis__fill"
                      style={{ width: `${Math.round((reading?.unipolar ?? 0) * 100)}%` }}
                    />
                  </span>
                  <span className="axis__value num">
                    {reading ? reading.value.toFixed(3) : "—"}
                  </span>
                  <span className="axis__seen" aria-label={moved ? "moved" : "not moved yet"}>
                    {moved ? "✓" : ""}
                  </span>
                </div>
              );
            })}
          </div>

          {buttonCount > 0 && (
            <div className="buttons">
              {Array.from({ length: buttonCount }, (_, i) => {
                const down = frame?.buttons[i] ?? false;
                const used = usedButtons.current.has(i);
                return (
                  <span
                    key={i}
                    className={`button${down ? " button--down" : ""}${used ? " button--seen" : ""}`}
                    title={`Button ${i + 1}${used ? " — pressed during this test" : ""}`}
                  >
                    {i + 1}
                  </span>
                );
              })}
            </div>
          )}

          {total > 0 && (
            <p className="monitor__tally">
              <span className="num">
                {exercised} of {total}
              </span>{" "}
              checked — move an axis and it ticks, press a button and it keeps a green outline.
              Anything still unmarked after you have tried it is the thing to report.
            </p>
          )}
        </>
      )}

      <p className="hint">
        This reads the device's HID reports without acquiring it, so it cannot take input away
        from a game — and it stops entirely while a session is running.
      </p>
    </div>
  );
}

/** One line saying exactly where this panel is, so it is never just blank. */
function describe(status: InputStatus | null, hasFrame: boolean): string {
  if (status === null) return "Starting…";
  switch (status.kind) {
    case "opening":
      return "Opening the device…";
    case "failed":
      return "Could not read this device";
    case "suspended":
      return "Paused — a session is running";
    case "listening":
      return hasFrame
        ? "Live"
        : "Listening — move an axis or press a button (a device only reports when something changes)";
  }
}
