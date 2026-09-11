import { Fragment, useCallback, useEffect, useState } from "react";

import { Section, StatusPill } from "../dashboard/primitives";
import { InputMonitor } from "./InputMonitor";
import {
  asIpcError,
  listDevices,
  onDevicesChanged,
  refreshDevices,
  type DetectedDevice,
} from "../ipc";
import "./devices.css";

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
    }).then((f) => {
      unlisten = f;
    });
    return () => unlisten?.();
  }, [load]);

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

                return (
                  <Fragment key={d.device.instancePath ?? `${d.device.vid}-${d.device.pid}`}>
                    <tr className={open ? "is-open" : undefined}>
                      <td>
                        {d.device.displayName}
                        {d.rawProductName && d.rawProductName !== d.device.displayName && (
                          <span className="devices__raw">{d.rawProductName}</span>
                        )}
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
                          <button
                            className="btn btn--quiet"
                            aria-expanded={open}
                            onClick={() => setWatching(open ? null : d)}
                          >
                            {open ? "Hide" : "Test"}
                          </button>
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

                    {/* Directly under the row it belongs to, not at the foot of
                        the table: a panel that opens somewhere else reads as
                        nothing having happened. */}
                    {open && d.device.instancePath && (
                      <tr className="devices__expanded">
                        <td colSpan={7}>
                          <InputMonitor
                            key={d.device.instancePath}
                            instancePath={d.device.instancePath}
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
          <button className="btn btn--quiet" onClick={() => void refreshDevices()}>
            Rescan now
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

const hex = (n: number) => `0x${n.toString(16).toUpperCase().padStart(4, "0")}`;
