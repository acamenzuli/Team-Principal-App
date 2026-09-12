import { useEffect, useMemo, useRef, useState } from "react";

import {
  deviceHistory,
  inputAliasKey,
  inputAliases,
  onDeviceEvent,
  onInput,
  onInputStatus,
  setInputAlias,
  startInputMonitor,
  stopInputMonitor,
  type DeviceEvent,
  type InputFrame,
  type InputKind,
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
  deviceKey,
  name,
  onClose,
}: {
  instancePath: string;
  deviceKey: string;
  name: string;
  onClose: () => void;
}) {
  const [frame, setFrame] = useState<InputFrame | null>(null);
  const [status, setStatus] = useState<InputStatus | null>(null);
  // What the user calls each control. "Brake" and "Upshift" are what somebody
  // thinks in; "axis 2" and "button 7" are what the hardware says, and this
  // panel is where the two are introduced to each other.
  const [aliases, setAliases] = useState<Record<string, string>>({});
  const [naming, setNaming] = useState<{ kind: InputKind; index: number } | null>(null);

  useEffect(() => {
    let live = true;
    inputAliases(deviceKey)
      .then((a) => live && setAliases(a))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [deviceKey]);

  const aliasOf = (kind: InputKind, index: number) =>
    aliases[inputAliasKey(deviceKey, kind, index)];

  const rename = async (kind: InputKind, index: number, value: string | null) => {
    try {
      setAliases(await setInputAlias(deviceKey, kind, index, value));
    } finally {
      setNaming(null);
    }
  };

  // Which controls have been exercised since this panel opened. Refs, not
  // state: they are written on every frame and only ever read during the
  // render that frame causes.
  const panel = useRef<HTMLDivElement>(null);
  // How many times each control has been used, not merely whether it has.
  // "It went green once and stopped" is a panel that answers the first
  // question and then goes blind: a stick that works and a stick that worked
  // ten minutes ago look identical. A count that keeps climbing is proof the
  // thing is still alive, every time you touch it.
  const axisUses = useRef<Map<number, number>>(new Map());
  const buttonUses = useRef<Map<number, number>>(new Map());
  const hatUses = useRef<Map<number, number>>(new Map());
  const restAxes = useRef<Map<number, number>>(new Map());
  /// Whether each control was active on the previous frame, so a use is
  /// counted on the edge rather than once per frame while it is held.
  const axisActive = useRef<Set<number>>(new Set());
  const buttonActive = useRef<Set<number>>(new Set());
  const hatActive = useRef<Set<number>>(new Set());

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
            if (rest === undefined) {
              restAxes.current.set(i, a.unipolar);
              return;
            }
            // Five per cent of travel. Below that is noise from a
            // potentiometer sitting still, and a count that climbs on its own
            // proves nothing.
            bump(axisUses, axisActive, i, Math.abs(a.unipolar - rest) > 0.05);
          });

          next.buttons.forEach((down, i) => bump(buttonUses, buttonActive, i, down));
          next.hats.forEach((degrees, i) => bump(hatUses, hatActive, i, degrees !== null));

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
        // The structure arrives here, not on an event.
        setStatus(await startInputMonitor(instancePath));
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
      axisUses.current.clear();
      buttonUses.current.clear();
      hatUses.current.clear();
      restAxes.current.clear();
      axisActive.current.clear();
      buttonActive.current.clear();
      hatActive.current.clear();
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
  const hatCount = declared?.hats ?? frame?.hats.length ?? 0;

  const exercised = axisUses.current.size + buttonUses.current.size + hatUses.current.size;
  const total = axisNames.length + buttonCount + hatCount;

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
          {total === 0 && status?.kind === "listening" && (
            <p className="note">
              This device declares no axes, buttons or hats. That is what its own descriptor
              says — nothing here is being hidden from you.
            </p>
          )}

          {axisNames.length > 0 && (
            <section className="io">
              <h4 className="io__title">
                Axes <span className="io__count num">{axisNames.length}</span>
              </h4>
              <div className="axes">
                {axisNames.map((axisName, i) => {
                  const reading = frame?.axes[i];
                  const uses = axisUses.current.get(i) ?? 0;
                  const active = axisActive.current.has(i);
                  const live = reading !== undefined;
                  return (
                    <div className={`axis${uses > 0 ? " axis--seen" : ""}`} key={`${axisName}-${i}`}>
                      {/* Dim until something reports, outlined while values are
                          arriving, filled while this control is actually being
                          moved right now, and green once it has been proven. */}
                      <span
                        className={`bub${live ? " bub--live" : ""}${uses > 0 ? " bub--seen" : ""}${
                          active ? " bub--active" : ""
                        }`}
                        aria-label={active ? "moving" : uses > 0 ? "moved" : "not moved yet"}
                      />
                      <Name
                        className="axis__name"
                        alias={aliasOf("axis", i)}
                        fallback={axisName}
                        index={i + 1}
                        uses={uses}
                        editing={naming?.kind === "axis" && naming.index === i}
                        onEdit={() => setNaming({ kind: "axis", index: i })}
                        onCancel={() => setNaming(null)}
                        onSave={(value) => void rename("axis", i, value)}
                      />
                      <span className="axis__track">
                        <span
                          className="axis__fill"
                          style={{ width: `${Math.round((reading?.unipolar ?? 0) * 100)}%` }}
                        />
                      </span>
                      <span className="axis__value num">
                        {reading ? reading.value.toFixed(3) : "—"}
                      </span>
                    </div>
                  );
                })}
              </div>
            </section>
          )}

          {hatCount > 0 && (
            <section className="io">
              <h4 className="io__title">
                Hats <span className="io__count num">{hatCount}</span>
              </h4>
              <div className="hats">
                {Array.from({ length: hatCount }, (_, i) => {
                  const degrees = frame?.hats[i] ?? null;
                  const uses = hatUses.current.get(i) ?? 0;
                  return (
                    <div className="hat" key={i}>
                      <span
                        className={`bub${uses > 0 ? " bub--seen" : ""}${
                          degrees !== null ? " bub--active" : ""
                        }`}
                      />
                      <Name
                        className="hat__name"
                        alias={aliasOf("hat", i)}
                        fallback="Hat"
                        index={i + 1}
                        uses={uses}
                        editing={naming?.kind === "hat" && naming.index === i}
                        onEdit={() => setNaming({ kind: "hat", index: i })}
                        onCancel={() => setNaming(null)}
                        onSave={(value) => void rename("hat", i, value)}
                      />
                      {/* The direction in words as well as degrees: "225°" is
                          not something anybody checks a POV switch against. */}
                      <span className="hat__value num">
                        {degrees === null ? "centred" : `${compass(degrees)} · ${degrees}°`}
                      </span>
                    </div>
                  );
                })}
              </div>
            </section>
          )}

          {buttonCount > 0 && (
            <section className="io">
              <h4 className="io__title">
                Buttons <span className="io__count num">{buttonCount}</span>
              </h4>
              <div className="buttons">
                {Array.from({ length: buttonCount }, (_, i) => {
                  const down = frame?.buttons[i] ?? false;
                  const uses = buttonUses.current.get(i) ?? 0;
                  return (
                    <span
                      key={i}
                      className={`bubbtn${down ? " bubbtn--down" : ""}${
                        uses > 0 ? " bubbtn--seen" : ""
                      }`}
                      title={`${aliasOf("button", i) ?? `Button ${i + 1}`} — ${
                        uses > 0 ? `used ${uses} ${uses === 1 ? "time" : "times"}` : "not used yet"
                      }. Click to name it.`}
                      role="button"
                      tabIndex={0}
                      onClick={() => setNaming({ kind: "button", index: i })}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" || e.key === " ") {
                          e.preventDefault();
                          setNaming({ kind: "button", index: i });
                        }
                      }}
                    >
                      {i + 1}
                      {uses > 0 && <span className="bubbtn__uses num">{uses}</span>}
                    </span>
                  );
                })}
              </div>

              {/* Naming happens below the grid: a thirty-pixel bubble is no
                  place for a text field, and the number has to stay visible
                  because the number is what a game's binding screen shows. */}
              {naming?.kind === "button" && (
                <NameField
                  label={`Name for button ${naming.index + 1}`}
                  value={aliasOf("button", naming.index) ?? ""}
                  onCancel={() => setNaming(null)}
                  onSave={(value) => void rename("button", naming.index, value)}
                />
              )}

              <Legend
                count={buttonCount}
                nameOf={(i) => aliasOf("button", i)}
                onPick={(i) => setNaming({ kind: "button", index: i })}
              />
            </section>
          )}

          {total > 0 && (
            <p className="monitor__tally">
              <span className="num">
                {exercised} of {total}
              </span>{" "}
              proven. Every control this device declares is listed above, numbered as the device
              numbers them — which is the numbering a game will show you. The number beside each
              is how many times you have used it, so a control that has gone quiet is as visible
              as one that never worked.
            </p>
          )}
        </>
      )}

      <History instancePath={instancePath} />

      <p className="hint">
        This reads the device's HID reports without acquiring it, so it cannot take input away
        from a game — and it stops entirely while a session is running.
      </p>
    </div>
  );
}

/**
 * A control's name: what you call it, over what the hardware calls it.
 *
 * The number never goes away. Whatever you name it, the number is what a
 * game's binding screen will show you, and matching the two is the entire
 * point of this panel.
 */
function Name({
  className,
  alias,
  fallback,
  index,
  uses,
  editing,
  onEdit,
  onCancel,
  onSave,
}: {
  className: string;
  alias: string | undefined;
  fallback: string;
  index: number;
  uses: number;
  editing: boolean;
  onEdit: () => void;
  onCancel: () => void;
  onSave: (value: string | null) => void;
}) {
  if (editing) {
    return (
      <span className={className}>
        <NameField
          label={`Name for ${alias ?? fallback} ${index}`}
          value={alias ?? ""}
          onCancel={onCancel}
          onSave={onSave}
        />
      </span>
    );
  }

  return (
    <span className={className}>
      <button className="io__rename" title="Click to name this control" onClick={onEdit}>
        {alias ?? fallback} <span className="io__n num">{index}</span>
        {/* The hardware's own name, kept where you named it something else —
            so a row can still be matched against what a game reports. */}
        {alias && <span className="io__raw">{fallback}</span>}
      </button>
      {uses > 0 && <span className="io__uses num">{uses}</span>}
    </span>
  );
}

/** One small form, used wherever a control is being named. */
function NameField({
  label,
  value,
  onSave,
  onCancel,
}: {
  label: string;
  value: string;
  onSave: (value: string | null) => void;
  onCancel: () => void;
}) {
  const [draft, setDraft] = useState(value);

  return (
    <form
      className="rename"
      onSubmit={(e) => {
        e.preventDefault();
        onSave(draft);
      }}
    >
      <input
        className="rename__field"
        value={draft}
        autoFocus
        aria-label={label}
        placeholder="Brake, Upshift, Pit limiter…"
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") onCancel();
        }}
      />
      <button className="btn btn--tiny" type="submit">
        Save
      </button>
      {value && (
        <button className="btn btn--tiny btn--quiet" type="button" onClick={() => onSave(null)}>
          Clear
        </button>
      )}
    </form>
  );
}

/**
 * The buttons that have names, listed under the grid.
 *
 * A thirty-pixel bubble cannot hold "Pit limiter", and shrinking the text
 * until it fits would make the number unreadable — which is the one thing on
 * it that has to stay readable.
 */
function Legend({
  count,
  nameOf,
  onPick,
}: {
  count: number;
  nameOf: (index: number) => string | undefined;
  onPick: (index: number) => void;
}) {
  const named = Array.from({ length: count }, (_, i) => i).filter((i) => nameOf(i));
  if (named.length === 0) {
    return <p className="hint">Click a button to give it a name — "Upshift", "Pit limiter".</p>;
  }

  return (
    <ul className="legend">
      {named.map((i) => (
        <li key={i}>
          <button className="legend__item" onClick={() => onPick(i)}>
            <span className="legend__n num">{i + 1}</span>
            {nameOf(i)}
          </button>
        </li>
      ))}
    </ul>
  );
}

/**
 * Every connection and disconnection this device has had, collapsed.
 *
 * Collapsed because it is the answer to a question you only sometimes have —
 * "did that drop out, or did I imagine it" — and a wall of timestamps above
 * the live readings would bury the thing you opened the panel for.
 *
 * Loaded when opened rather than kept live: it changes when a cable moves,
 * which is not often, and re-reading on every frame would be work nobody asked
 * for sixty times a second.
 */
function History({ instancePath }: { instancePath: string }) {
  const [events, setEvents] = useState<DeviceEvent[] | null>(null);

  // Loaded once and then kept live. A list you have to press a button to
  // refresh is wrong most of the time you are looking at it — and the moment
  // worth watching is the one where you wiggle the cable.
  useEffect(() => {
    let live = true;
    deviceHistory(instancePath)
      .then((h) => live && setEvents(h))
      .catch(() => live && setEvents([]));

    let unlisten: (() => void) | undefined;
    void onDeviceEvent((key, event) => {
      if (key === instancePath) setEvents((current) => [...(current ?? []), event]);
    }).then((f) => {
      if (!live) f();
      else unlisten = f;
    });

    return () => {
      live = false;
      unlisten?.();
    };
  }, [instancePath]);

  return (
    <details className="hist">
      <summary className="hist__summary">
        Connection history
        {events !== null && <span className="hist__count num">{events.length}</span>}
      </summary>

      {events === null && <p className="note">Reading…</p>}

      {events !== null && events.length === 0 && (
        <p className="note">
          Nothing recorded. History starts when the app does, so a device that has simply been
          plugged in the whole time has one entry at most.
        </p>
      )}

      {events !== null && events.length > 0 && (
        <ol className="hist__list">
          {/* Newest first: the reason anybody opens this is something that
              just happened. */}
          {[...events].reverse().map((event, i) => (
            <li className="hist__row" key={`${event.at}-${i}`}>
              <span className="hist__when num">{when(event.at)}</span>
              <span className={`hist__what hist__what--${event.to}`}>{describeEvent(event)}</span>
            </li>
          ))}
        </ol>
      )}
    </details>
  );
}

/** What happened, in the words somebody would use for it. */
function describeEvent(event: DeviceEvent): string {
  if (event.from === null) {
    return event.to === "connected" ? "Found, connected" : `Found, ${readable(event.to)}`;
  }
  return `${readable(event.from)} → ${readable(event.to)}`;
}

const readable = (status: DeviceEvent["to"]) =>
  status === "connected" ? "connected" : status === "connecting" ? "connecting" : "disconnected";

/** The stored time is RFC 3339; shown in the viewer's own timezone. */
function when(at: string): string {
  const date = new Date(at);
  return Number.isNaN(date.getTime()) ? at : date.toLocaleTimeString();
}

/**
 * Count a use on the edge, not once per frame.
 *
 * Holding a button down produces thirty frames a second; counting each of them
 * would turn a press into a meaningless number that climbs while you rest your
 * thumb. A use is one activation, from released to pressed.
 */
function bump(
  uses: React.MutableRefObject<Map<number, number>>,
  active: React.MutableRefObject<Set<number>>,
  index: number,
  isActive: boolean,
) {
  if (isActive) {
    if (!active.current.has(index)) {
      active.current.add(index);
      uses.current.set(index, (uses.current.get(index) ?? 0) + 1);
    }
  } else {
    active.current.delete(index);
  }
}

/** A hat's direction in words. Degrees alone is not how anybody checks a POV. */
function compass(degrees: number): string {
  const points = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"] as const;
  return points[Math.round(degrees / 45) % 8] ?? "N";
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
