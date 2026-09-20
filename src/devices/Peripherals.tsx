import { Fragment, useCallback, useEffect, useState } from "react";

import { Section, StatusPill } from "../dashboard/primitives";
import { InputMonitor } from "./InputMonitor";
import {
  asIpcError,
  listDevices,
  onDevicesChanged,
  reconnectDevice,
  refreshDevices,
  setDeviceAlias,
  type DetectedDevice,
  type ReconnectOutcome,
} from "../ipc";
import "./devices.css";

/**
 * How the last press of Reconnect ended for one device. Kept per device and
 * shown under its row, because "it failed" at the top of the page does not say
 * which of three button boxes it was talking about.
 */
type Reconnected = {
  outcome: ReconnectOutcome | "failed";
  at: Date;
  /** What to do next. Only a failure has one. */
  message?: string;
};

const RECONNECT_HINT =
  "Restart it in Windows — what unplugging it and plugging it back in does, without " +
  "touching the cable. Windows asks for an administrator's OK first.";

/**
 * Peripherals.
 *
 * The list is what the machine actually reports, refreshed on a slow poll. It
 * is deliberately not a checklist of what you own — a device you unplugged
 * should disappear, because the question this page answers is "can I race right
 * now", not "what did I buy".
 */
export function Peripherals() {
  const [devices, setDevices] = useState<DetectedDevice[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [scannedAt, setScannedAt] = useState<Date | null>(null);
  const [watching, setWatching] = useState<DetectedDevice | null>(null);
  const [rescanning, setRescanning] = useState(false);
  // The device being restarted, by path. One at a time — the backend refuses
  // a second — so every other Reconnect waits rather than queues.
  const [reconnecting, setReconnecting] = useState<string | null>(null);
  const [reconnected, setReconnected] = useState<Record<string, Reconnected>>({});

  const load = useCallback(async () => {
    try {
      setDevices(await listDevices());
      setScannedAt(new Date());
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    }
  }, []);

  useEffect(() => {
    // One read for the current state, then events. The backend publishes only
    // when a debounced status actually changes, so this component never polls
    // and never re-renders to be told nothing happened.
    void load();
    let unlisten: (() => void) | undefined;
    void onDevicesChanged((next) => {
      setDevices(next);
      setScannedAt(new Date());
      // An asked-for rescan always publishes, changed or not, so this is a
      // reliable end to the button's busy state rather than a guess at one.
      setRescanning(false);
    }).then((f) => {
      unlisten = f;
    });
    return () => unlisten?.();
  }, [load]);

  const reconnect = async (d: DetectedDevice) => {
    const path = d.device.instancePath;
    if (path === null) return;
    // The test panel holds the device open. Closed first, so the restart is
    // not fighting this app for the very thing it is restarting — and so the
    // panel is not left reporting a failure for a device that is fine.
    if (watching?.device.instancePath === path) setWatching(null);
    setReconnecting(path);
    try {
      const outcome = await reconnectDevice(path);
      setReconnected((prev) => ({ ...prev, [path]: { outcome, at: new Date() } }));
    } catch (e) {
      setReconnected((prev) => ({
        ...prev,
        [path]: { outcome: "failed", at: new Date(), message: asIpcError(e).message },
      }));
    } finally {
      setReconnecting(null);
    }
  };

  return (
    <div className="devices">
      <Section
        title="Peripherals"
        note={
          devices === null
            ? "scanning"
            : `${devices.length} game controller${devices.length === 1 ? "" : "s"}${
                scannedAt ? ` · updated ${scannedAt.toLocaleTimeString()}` : ""
              }`
        }
      >
        {error && <p className="warn warn--hard">{error}</p>}

        {devices !== null && devices.length === 0 && !error && (
          <p className="note">
            No game controllers detected. If your wheel is plugged in and powered, check that
            Windows lists it in <span className="num">joy.cpl</span> — this page shows exactly what
            the HID layer reports, nothing more.
          </p>
        )}

        {devices !== null && devices.length > 0 && (
          <table className="grid">
            <thead>
              <tr>
                <th>Device</th>
                <th>Status</th>
                <th>VID / PID</th>
                <th>Serial</th>
                <th>DirectInput</th>
                <th>Notes</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {devices.map((d) => {
                // Compared by path, never by object identity: the list is
                // replaced wholesale whenever a device arrives or leaves, and
                // an identity check would quietly stop matching.
                const open =
                  d.device.instancePath !== null &&
                  watching?.device.instancePath === d.device.instancePath;
                // What the last Reconnect on this device came to, if anything.
                const note =
                  d.device.instancePath === null ? undefined : reconnected[d.device.instancePath];

                return (
                  <Fragment key={d.device.instancePath ?? `${d.device.vid}-${d.device.pid}`}>
                    <tr className={open ? "is-open" : undefined}>
                      <td>
                        <DeviceName device={d} onProblem={setError} />
                      </td>
                      <td>
                        <StatusPill status={d.status} />
                      </td>
                      <td className="num">
                        {hex(d.device.vid)} / {hex(d.device.pid)}
                      </td>
                      <td className="num">
                        {d.device.serial ?? <span className="note">none reported</span>}
                      </td>
                      <td className="num">
                        {d.dinputPresent ? (
                          <>
                            slot {d.dinputSlot ?? "?"}
                            {d.dinputInstanceGuid && (
                              <span className="devices__raw">{d.dinputInstanceGuid}</span>
                            )}
                          </>
                        ) : (
                          <span className="note">not listed</span>
                        )}
                      </td>
                      <td>
                        {d.isVirtual && <span className="tag">vJoy</span>}
                        <ModeTag device={d} />
                        {!d.dinputPresent && (
                          <span className="tag tag--warn" title="Games cannot see it yet">
                            ▲ not in DirectInput
                          </span>
                        )}
                        {d.bindingDrift && (
                          <span className="tag tag--warn">
                            ▲ slot moved {d.bindingDrift.expectedSlot ?? "?"} →{" "}
                            {d.bindingDrift.actualSlot ?? "?"}
                          </span>
                        )}
                      </td>
                      <td>
                        {d.device.instancePath ? (
                          <div className="devices__act">
                            <button
                              className="btn btn--quiet"
                              aria-expanded={open}
                              onClick={() => setWatching(open ? null : d)}
                            >
                              {open ? "Hide" : "Test"}
                            </button>
                            {/* vJoy is software: there is no cable to pull, so
                                there is nothing a restart could do for it. */}
                            {!d.isVirtual && (
                              <button
                                className="btn btn--quiet"
                                disabled={reconnecting !== null || d.status === "disconnected"}
                                title={
                                  d.status === "disconnected"
                                    ? "Not plugged in, so there is nothing to restart"
                                    : RECONNECT_HINT
                                }
                                onClick={() => void reconnect(d)}
                              >
                                {reconnecting === d.device.instancePath
                                  ? "Reconnecting…"
                                  : "Reconnect"}
                              </button>
                            )}
                          </div>
                        ) : (
                          // No device path means nothing to open, and a button
                          // here would be a promise the app cannot keep.
                          <span
                            className="note"
                            title="Windows reports no device path for this one"
                          >
                            can't test
                          </span>
                        )}
                      </td>
                    </tr>

                    {/* How the last Reconnect ended, under the row it was
                        pressed on. A failure says what to do next and stays
                        until dismissed; the row above going red and green
                        again is the evidence for a success. */}
                    {note && (
                      <tr className="devices__expanded">
                        <td colSpan={7}>
                          <ReconnectNote
                            result={note}
                            onDismiss={() => {
                              const path = d.device.instancePath;
                              setReconnected((prev) =>
                                Object.fromEntries(
                                  Object.entries(prev).filter(([key]) => key !== path),
                                ),
                              );
                            }}
                          />
                        </td>
                      </tr>
                    )}

                    {/* Directly under the row it belongs to, not at the foot of
                        the table: a panel that opens somewhere else reads as
                        nothing having happened. */}
                    {open && d.device.instancePath && (
                      <tr className="devices__expanded">
                        <td colSpan={7}>
                          <InputMonitor
                            key={d.device.instancePath}
                            instancePath={d.device.instancePath}
                            deviceKey={d.aliasKey}
                            name={d.device.displayName}
                            onClose={() => setWatching(null)}
                          />
                        </td>
                      </tr>
                    )}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
        )}

        <div className="devices__actions">
          <button
            className="btn btn--quiet"
            disabled={rescanning}
            onClick={() => {
              setRescanning(true);
              void refreshDevices().catch((e) => {
                setError(asIpcError(e).message);
                setRescanning(false);
              });
            }}
          >
            {rescanning ? "Rescanning…" : "Rescan now"}
          </button>
          <span className="hint">
            This list updates itself — Windows reports device arrival and removal, and a device
            that bounces while it enumerates never reaches this page.
          </span>
        </div>

        <p className="devices__scope">
          This page is the truth about what is plugged in. Which of these a game <em>requires</em>
          is set per game, on its profile in Games — nothing here is assumed, because a preflight
          that checks things nobody asked for is one people learn to scroll past.
        </p>
      </Section>
    </div>
  );
}

/**
 * A device's name, and the means to change it.
 *
 * A rig with three identical un-serialled button boxes is three identical rows,
 * and no catalog can fix that — only the person who knows which one is bolted
 * to the left of the wheel can. So the name is theirs to set, and it is stored
 * against the device rather than against any game: what you call a pedal set
 * does not change because you launched a different sim.
 *
 * The saved name arrives back through the device event like every other change,
 * so this does not hold its own copy of the truth.
 */
function DeviceName({
  device,
  onProblem,
}: {
  device: DetectedDevice;
  onProblem: (message: string | null) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(device.device.displayName);

  const save = async (name: string | null) => {
    try {
      await setDeviceAlias(device.aliasKey, name);
      onProblem(null);
      setEditing(false);
    } catch (e) {
      onProblem(asIpcError(e).message);
    }
  };

  if (editing) {
    return (
      <form
        className="rename"
        onSubmit={(e) => {
          e.preventDefault();
          void save(draft);
        }}
      >
        <input
          className="rename__field"
          value={draft}
          autoFocus
          aria-label="Name for this device"
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") setEditing(false);
          }}
        />
        <button className="btn btn--tiny" type="submit">
          Save
        </button>
        {device.renamed && (
          <button className="btn btn--tiny btn--quiet" type="button" onClick={() => void save(null)}>
            Reset
          </button>
        )}
      </form>
    );
  }

  return (
    <button
      className="rename__open"
      title="Click to rename"
      onClick={() => {
        setDraft(device.device.displayName);
        setEditing(true);
      }}
    >
      {device.device.displayName}
      {/* What Windows calls it, kept visible under a name you chose — so a
          renamed row can still be matched against what a game will show. A
          part of a split device also shows its collection, because four parts
          of one wheel carry the same Windows name and that number is the only
          thing on the row that says which is which. */}
      {rawLine(device) && <span className="devices__raw">{rawLine(device)}</span>}
    </button>
  );
}

function rawLine(device: DetectedDevice): string {
  const raw =
    device.rawProductName && device.rawProductName !== device.device.displayName
      ? device.rawProductName
      : null;
  return [raw, device.section?.id ?? null].filter((s) => s !== null).join(" · ");
}

/**
 * Which presentation a device is using, when it has more than one.
 *
 * An Asetek wheel in legacy input mode is listed as several controllers, one
 * per share of its inputs — what games with a per-device input limit need,
 * and what Automobilista 2 needs. Each part is a row, named and tested on its
 * own; the tag says which part this is. A wheel in normal mode is one row,
 * and says so, because a single row could otherwise be read as three parts
 * having gone missing. A split device from a maker with no such mode is
 * simply in parts.
 */
function ModeTag({ device }: { device: DetectedDevice }) {
  const part = device.section;
  if (device.mode === "legacy" && part) {
    return (
      <span
        className="tag tag--legacy"
        title={
          `Legacy input mode: the wheel presents its inputs as ${part.count} separate ` +
          `controllers, each within the 32-input limit some games have. This is part ` +
          `${part.index}, collection ${part.id}. Name and test each part on its own; ` +
          `games see them separately too. Switch modes in RaceHub.`
        }
      >
        legacy mode · part {part.index} of {part.count}
      </span>
    );
  }
  if (device.mode === "normal") {
    return (
      <span
        className="tag"
        title={
          "Normal input mode: one controller carrying every input. Games that take at " +
          "most 32 inputs from one device — Automobilista 2 — need legacy input mode, " +
          "switched in RaceHub."
        }
      >
        normal mode
      </span>
    );
  }
  if (part) {
    return (
      <span
        className="tag"
        title={`Windows lists this device as ${part.count} controllers. This is collection ${part.id}.`}
      >
        part {part.index} of {part.count}
      </span>
    );
  }
  return null;
}

/**
 * How the last Reconnect ended, in a line, with the next step when there is
 * one.
 *
 * A success is deliberately modest: the row above going red and then green
 * again is the proof, and this only says when. A declined prompt is not a
 * failure and is not coloured like one — nothing happened, which is what was
 * asked for.
 */
function ReconnectNote({ result, onDismiss }: { result: Reconnected; onDismiss: () => void }) {
  const when = result.at.toLocaleTimeString();
  const tone =
    result.outcome === "failed" ? "fail" : result.outcome === "declined" ? "quiet" : "ok";
  return (
    <p className={`devices__outcome devices__outcome--${tone}`}>
      <span>
        {result.outcome === "restarted" &&
          `Restarted at ${when} — Windows reports it running again.`}
        {result.outcome === "declined" &&
          `Nothing changed — the administrator prompt was declined at ${when}.`}
        {result.outcome === "failed" && `Not reconnected — ${result.message}`}
      </span>
      <button className="btn btn--tiny btn--quiet" type="button" onClick={onDismiss}>
        Dismiss
      </button>
    </p>
  );
}

const hex = (n: number) => `0x${n.toString(16).toUpperCase().padStart(4, "0")}`;
