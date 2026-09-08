import { useEffect, useState } from "react";

import { onInput, startInputMonitor, stopInputMonitor, type InputFrame } from "../ipc";

/**
 * Live axes and buttons for one device.
 *
 * This is how you confirm a pedal set is actually working, and it turns a
 * support conversation from twenty minutes of guessing into "press the brake
 * and tell me if the bar moves".
 *
 * Axes are drawn unipolar — at rest the bar is empty. A throttle sitting at
 * half deflection when your foot is off it would be alarming and wrong, and a
 * bipolar bar for a pedal reads exactly that way.
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
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    void onInput((next) => {
      // Frames for a device other than the one on screen would arrive if a
      // previous monitor were still winding down.
      if (next.instancePath === instancePath) setFrame(next);
    }).then((f) => {
      if (cancelled) f();
      else unlisten = f;
    });

    startInputMonitor(instancePath).catch(() =>
      setError("Couldn't read this device. Some peripherals only report while their vendor software is running."),
    );

    return () => {
      cancelled = true;
      unlisten?.();
      void stopInputMonitor();
    };
  }, [instancePath]);

  return (
    <div className="monitor">
      <header className="monitor__head">
        <h3 className="monitor__title">{name}</h3>
        <button className="btn btn--quiet" onClick={onClose}>
          Close
        </button>
      </header>

      {error && <p className="warn warn--hard">{error}</p>}

      {!error && !frame && (
        <p className="note">
          Waiting for input — move an axis or press a button. A device that never reports is
          usually one whose vendor software isn't running.
        </p>
      )}

      {frame && (
        <>
          <div className="axes">
            {frame.axes.length === 0 && <p className="note">This device reports no axes.</p>}
            {frame.axes.map((a, i) => (
              <div className="axis" key={`${a.name}-${i}`}>
                <span className="axis__name">{a.name}</span>
                <span className="axis__track">
                  <span
                    className="axis__fill"
                    style={{ width: `${Math.round(a.unipolar * 100)}%` }}
                  />
                </span>
                <span className="axis__value num">{a.value.toFixed(3)}</span>
              </div>
            ))}
          </div>

          {frame.buttons.length > 0 && (
            <div className="buttons">
              {frame.buttons.map((down, i) => (
                <span
                  key={i}
                  className={`button${down ? " button--down" : ""}`}
                  title={`Button ${i + 1}`}
                >
                  {i + 1}
                </span>
              ))}
            </div>
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
