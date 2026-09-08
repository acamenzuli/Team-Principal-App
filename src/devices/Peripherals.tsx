import { useCallback, useEffect, useState } from "react";

import { Section, StatusPill } from "../dashboard/primitives";
import { asIpcError, listDevices, type DetectedDevice } from "../ipc";
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

  const refresh = useCallback(async () => {
    try {
      setDevices(await listDevices());
      setScannedAt(new Date());
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    }
  }, []);

  useEffect(() => {
    void refresh();
    // A slow reconciliation poll. Event-driven hotplug via
    // CM_Register_Notification is the right mechanism and lands next; until
    // then this is honest about being a poll rather than pretending to be live.
    const timer = window.setInterval(() => void refresh(), 4000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  return (
    <div className="devices">
      <Section
        title="Peripherals"
        note={
          devices === null
            ? "scanning"
            : `${devices.length} game controller${devices.length === 1 ? "" : "s"}${
                scannedAt ? ` · checked ${scannedAt.toLocaleTimeString()}` : ""
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
                <th>Notes</th>
              </tr>
            </thead>
            <tbody>
              {devices.map((d) => (
                <tr key={d.device.instancePath ?? `${d.device.vid}-${d.device.pid}`}>
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
                </tr>
              ))}
            </tbody>
          </table>
        )}

        {/* Say plainly what is not finished, rather than letting a half-built
            page look like a broken one. */}
        <p className="devices__scope">
          <strong>Milestone 5, part one.</strong> This reads the HID layer: what is plugged in,
          its identity, and whether it is a vJoy device. DirectInput ordering, event-driven
          hotplug, and the live axis monitor are the rest of this milestone — until DirectInput is
          read, every device shows as <em>Connecting</em> rather than claiming a readiness that
          has not been checked.
        </p>
      </Section>
    </div>
  );
}

const hex = (n: number) => `0x${n.toString(16).toUpperCase().padStart(4, "0")}`;
