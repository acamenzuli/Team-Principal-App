import { useCallback, useEffect, useMemo, useState } from "react";

import { Section } from "../dashboard/primitives";
import {
  applyTopology,
  asIpcError,
  availableModes,
  currentTopology,
  keepTopology,
  listMonitors,
  listSnapshots,
  onConfirmState,
  panicHotkey,
  previewTopology,
  restoreSnapshot,
  revertTopology,
  type AvailableModes,
  type ConfirmState,
  type DisplayMode,
  type HotkeyInfo,
  type MonitorInfo,
  type SnapshotEntry,
  type TopologyPreview,
  type TopologySnapshot,
} from "../ipc";
import "./displays.css";

/**
 * Changing the desktop, survivably.
 *
 * The rules this screen exists to make visible:
 *
 * * **Nothing is written until you have read what will change.** The preview is
 *   generated from the plan on every edit, and Apply is disabled while anything
 *   is blocking.
 * * **Silence means revert.** After a change lands, a countdown runs in Rust —
 *   not here — and puts the desktop back unless you say to keep it. If the
 *   change blanked the screen, this component is exactly what you cannot see.
 * * **The hotkey is reported, not assumed.** Another program may already own
 *   the combination, and a safety net you believe in that does nothing is worse
 *   than none.
 */
export function DisplayControl() {
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  const [current, setCurrent] = useState<TopologySnapshot | null>(null);
  const [plan, setPlan] = useState<TopologySnapshot | null>(null);
  const [modes, setModes] = useState<AvailableModes[]>([]);
  const [preview, setPreview] = useState<TopologyPreview | null>(null);
  const [confirm, setConfirm] = useState<ConfirmState | null>(null);
  const [hotkey, setHotkey] = useState<HotkeyInfo | null>(null);
  const [snapshots, setSnapshots] = useState<TopologySnapshot[]>([]);
  const [mismatch, setMismatch] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const [found, topology, available, keys, saved] = await Promise.all([
        listMonitors(),
        currentTopology(),
        availableModes(),
        panicHotkey(),
        listSnapshots(),
      ]);
      setMonitors(found);
      setCurrent(topology);
      setPlan(topology);
      setModes(available);
      setHotkey(keys);
      setSnapshots(saved);
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onConfirmState((state) => {
      setConfirm(state);
      // Settled, either way: what is on screen now is the truth, so re-read it
      // rather than assuming which way it went.
      if (state.outcome) void load();
    }).then((f) => (unlisten = f));
    return () => unlisten?.();
  }, [load]);

  // The preview is regenerated on every edit rather than on an explicit press.
  // A preview you have to ask for is a preview people stop asking for.
  useEffect(() => {
    if (!plan || !current) return;
    let live = true;
    previewTopology(plan)
      .then((p) => live && setPreview(p))
      .catch((e) => live && setError(asIpcError(e).message));
    return () => {
      live = false;
    };
  }, [plan, current]);

  const names = useMemo(() => {
    const map = new Map<string, string>();
    for (const m of monitors) map.set(m.devicePath, m.friendlyName);
    return map;
  }, [monitors]);

  const counting = confirm !== null && confirm.outcome === null && confirm.secondsLeft > 0;

  function editEntry(devicePath: string, change: Partial<SnapshotEntry>) {
    setPlan((p) =>
      p === null
        ? p
        : {
            ...p,
            monitors: p.monitors.map((m) =>
              m.devicePath === devicePath ? { ...m, ...change } : m,
            ),
          },
    );
  }

  /** Exactly one primary, so setting one clears the rest. */
  function setPrimary(devicePath: string) {
    setPlan((p) =>
      p === null
        ? p
        : {
            ...p,
            monitors: p.monitors.map((m) => ({
              ...m,
              isPrimary: m.devicePath === devicePath,
            })),
          },
    );
  }

  async function apply() {
    if (!plan) return;
    setBusy(true);
    setMismatch([]);
    try {
      const differences = await applyTopology(plan);
      setMismatch(differences);
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    } finally {
      setBusy(false);
    }
  }

  const dirty = plan !== null && current !== null && JSON.stringify(plan) !== JSON.stringify(current);

  return (
    <div className="dc">
      <Section title="Display control" note={hotkey?.registered ? hotkey.combination : undefined}>
        {error && <p className="warn warn--hard">{error}</p>}

        {hotkey && !hotkey.registered && (
          <p className="note note--fail">
            <strong>The panic hotkey is not available.</strong> Something else on this machine
            already owns {hotkey.combination}. Changes still revert on their own after fifteen
            seconds — that safety net is intact — but you cannot force it early from a screen you
            cannot see.
          </p>
        )}

        {plan === null ? (
          <p className="note note--pending">Reading the desktop…</p>
        ) : (
          <table className="grid dc__table">
            <thead>
              <tr>
                <th>Screen</th>
                <th>On</th>
                <th>Mode</th>
                <th>Position</th>
                <th>Primary</th>
              </tr>
            </thead>
            <tbody>
              {plan.monitors.map((m) => (
                <MonitorRow
                  key={m.devicePath}
                  entry={m}
                  name={names.get(m.devicePath) ?? m.devicePath}
                  modes={modes.find((a) => a.devicePath === m.devicePath)?.modes ?? []}
                  disabled={counting || busy}
                  onChange={(change) => editEntry(m.devicePath, change)}
                  onPrimary={() => setPrimary(m.devicePath)}
                />
              ))}
            </tbody>
          </table>
        )}

        <Preview preview={preview} dirty={dirty} />

        {mismatch.length > 0 && (
          <div className="dc__mismatch">
            <p className="warn warn--hard">
              Windows accepted the change and then did something else. It has been put back.
            </p>
            <ul className="dc__list">
              {mismatch.map((d) => (
                <li key={d} className="num">
                  {d}
                </li>
              ))}
            </ul>
          </div>
        )}

        {counting ? (
          <Countdown state={confirm} />
        ) : (
          <div className="dc__actions">
            <button
              className="btn"
              disabled={!dirty || busy || preview === null || !canApply(preview)}
              onClick={() => void apply()}
            >
              {busy ? "Applying…" : "Apply"}
            </button>
            {dirty && (
              <button className="btn btn--quiet" onClick={() => setPlan(current)}>
                Discard changes
              </button>
            )}
          </div>
        )}

        {confirm?.outcome && !counting && <Outcome state={confirm} />}
      </Section>

      {snapshots.length > 0 && (
        <Section title="Saved layouts" note={`${snapshots.length} kept`}>
          <p className="note">
            Captured automatically before every change, so a crash or a power cut in the middle of
            one still leaves something to put the desktop back with.
          </p>
          <ul className="dc__snapshots">
            {snapshots.map((s) => (
              <li key={s.id}>
                <span className="num">{s.capturedAt}</span>
                <span className="note">
                  {s.monitors.filter((m) => m.active).length} screens on
                </span>
                <button
                  className="btn btn--quiet"
                  disabled={counting || busy}
                  onClick={() => void restoreSnapshot(s.id).then(setMismatch)}
                >
                  Restore
                </button>
              </li>
            ))}
          </ul>
        </Section>
      )}
    </div>
  );
}

function MonitorRow({
  entry,
  name,
  modes,
  disabled,
  onChange,
  onPrimary,
}: {
  entry: SnapshotEntry;
  name: string;
  modes: DisplayMode[];
  disabled: boolean;
  onChange: (change: Partial<SnapshotEntry>) => void;
  onPrimary: () => void;
}) {
  return (
    <tr className={entry.active ? undefined : "dc__row--off"}>
      <td>{name}</td>
      <td>
        <input
          type="checkbox"
          checked={entry.active}
          disabled={disabled}
          aria-label={`${name} on`}
          onChange={(e) => onChange({ active: e.target.checked })}
        />
      </td>
      <td>
        <select
          className="text-input num"
          disabled={disabled || !entry.active || modes.length === 0}
          value={modeKey(entry.mode)}
          onChange={(e) => {
            const mode = modes.find((m) => modeKey(m) === e.target.value);
            if (mode) onChange({ mode });
          }}
        >
          {/* The current mode may not be in the list — a driver can report a
              mode it will not enumerate. Showing it anyway beats a select that
              silently displays someone else's mode. */}
          {!modes.some((m) => modeKey(m) === modeKey(entry.mode)) && (
            <option value={modeKey(entry.mode)}>{modeLabel(entry.mode)} (current)</option>
          )}
          {modes.map((m) => (
            <option key={modeKey(m)} value={modeKey(m)}>
              {modeLabel(m)}
            </option>
          ))}
        </select>
      </td>
      <td className="dc__position">
        <input
          className="text-input num"
          type="number"
          step={1}
          disabled={disabled || !entry.active}
          aria-label={`${name} x`}
          value={entry.position[0]}
          onChange={(e) => onChange({ position: [Number(e.target.value), entry.position[1]] })}
        />
        <input
          className="text-input num"
          type="number"
          step={1}
          disabled={disabled || !entry.active}
          aria-label={`${name} y`}
          value={entry.position[1]}
          onChange={(e) => onChange({ position: [entry.position[0], Number(e.target.value)] })}
        />
      </td>
      <td>
        <input
          type="radio"
          name="primary"
          checked={entry.isPrimary}
          disabled={disabled || !entry.active}
          aria-label={`${name} primary`}
          onChange={onPrimary}
        />
      </td>
    </tr>
  );
}

/** What will change, and what is wrong with it. Never hidden behind a button. */
function Preview({ preview, dirty }: { preview: TopologyPreview | null; dirty: boolean }) {
  if (preview === null) return null;

  const blocking = preview.problems.filter((p) => p.severity === "blocking");
  const warnings = preview.problems.filter((p) => p.severity === "warning");

  return (
    <div className="dc__preview">
      {dirty && preview.changes.length > 0 && (
        <>
          <h3 className="dc__preview-title">This will change</h3>
          <ul className="dc__list">
            {preview.changes.map((c, i) => (
              <li key={i}>{describe(c)}</li>
            ))}
          </ul>
        </>
      )}

      {blocking.map((p) => (
        <p className="warn warn--hard" key={p.message}>
          {p.message}
        </p>
      ))}
      {warnings.map((p) => (
        <p className="note note--fail" key={p.message}>
          {p.message}
        </p>
      ))}
    </div>
  );
}

/**
 * The countdown.
 *
 * The number comes from Rust. This only draws it — which is the point: if the
 * change made the screen unreadable, nothing here runs and the revert still
 * happens on time.
 */
function Countdown({ state }: { state: ConfirmState }) {
  return (
    <div className="dc__confirm" role="alertdialog" aria-live="assertive">
      <p className="dc__confirm-title">Keep these display settings?</p>
      <p className="dc__confirm-sub">
        Reverting in <span className="num dc__count">{state.secondsLeft}</span> seconds. If you
        cannot read this, do nothing — it goes back on its own.
      </p>
      <div className="dc__actions">
        <button className="btn" onClick={() => void keepTopology()}>
          Keep these settings
        </button>
        <button className="btn btn--quiet" onClick={() => void revertTopology()}>
          Put it back now
        </button>
      </div>
    </div>
  );
}

function Outcome({ state }: { state: ConfirmState }) {
  if (!state.outcome) return null;
  const failed = state.outcome === "revert_failed";
  return (
    <p className={failed ? "warn warn--hard" : "note"}>
      {OUTCOME[state.outcome]}
    </p>
  );
}

const OUTCOME: Record<NonNullable<ConfirmState["outcome"]>, string> = {
  kept: "Kept. These are your display settings now.",
  reverted_on_timeout: "Put back — nobody confirmed it within fifteen seconds.",
  reverted_on_request: "Put back.",
  reverted_on_mismatch:
    "Put back: Windows accepted the change and then produced something else, so it was not worth asking about.",
  revert_failed:
    "The change could not be undone. Use Windows' own display settings, or restart — the layout before the change is saved under Saved layouts.",
};

function canApply(preview: TopologyPreview): boolean {
  return !preview.problems.some((p) => p.severity === "blocking");
}

function modeKey(mode: DisplayMode): string {
  return `${mode.resolution.width}x${mode.resolution.height}@${mode.refreshHz}`;
}

function modeLabel(mode: DisplayMode): string {
  return `${mode.resolution.width} × ${mode.resolution.height} · ${mode.refreshHz} Hz`;
}

/** Mirrors tp_model::topology::describe, so both sides say the same words. */
function describe(change: TopologyPreview["changes"][number]): string {
  switch (change.kind) {
    case "resolution":
      return `${change.monitor}: ${change.from.width}×${change.from.height} to ${change.to.width}×${change.to.height}`;
    case "refresh_rate":
      return `${change.monitor}: ${change.fromHz} Hz to ${change.toHz} Hz`;
    case "position":
      return `${change.monitor}: moves from ${change.from[0]},${change.from[1]} to ${change.to[0]},${change.to[1]}`;
    case "primary":
      return change.from
        ? `Primary screen: ${change.from} to ${change.to}`
        : `Primary screen: ${change.to}`;
    case "activated":
      return `${change.monitor}: switched on`;
    case "deactivated":
      return `${change.monitor}: switched off`;
  }
}
