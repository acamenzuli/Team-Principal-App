import { useEffect, useState } from "react";

import {
  asIpcError,
  desktopLayout,
  listDevices,
  listMonitors,
  type DesktopLayoutInfo,
  type DetectedDevice,
  type MonitorInfo,
} from "../ipc";
import { DesktopMap, rectLabel } from "./DesktopMap";
import { Section, StatusPill } from "./primitives";

export function HardwarePanel() {
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  const [layout, setLayout] = useState<DesktopLayoutInfo | null>(null);
  const [devices, setDevices] = useState<DetectedDevice[]>([]);
  const [monitorError, setMonitorError] = useState<string | null>(null);
  const [deviceError, setDeviceError] = useState<string | null>(null);

  useEffect(() => {
    listMonitors().then(setMonitors).catch((e) => setMonitorError(asIpcError(e).message));
    desktopLayout()
      .then(setLayout)
      // The layout is derived from the same enumeration, so a failure here is
      // the same failure already reported above. Don't say it twice.
      .catch(() => setLayout(null));
    listDevices().then(setDevices).catch((e) => setDeviceError(asIpcError(e).message));
  }, []);

  return (
    <>
      <Section title="Displays" note={`${monitors.length} detected`}>
        {monitorError ? (
          <p className="note note--pending">{monitorError}</p>
        ) : (
          <table className="grid">
            <thead>
              <tr>
                <th>Monitor</th>
                <th>Native</th>
                <th>Mode</th>
                <th>Position</th>
                <th>Scale</th>
                <th>Physical size</th>
                <th>Pitch</th>
              </tr>
            </thead>
            <tbody>
              {monitors.map((m) => (
                <tr key={m.devicePath}>
                  <td>
                    {m.friendlyName}
                    {m.isPrimary && <span className="tag">PRIMARY</span>}
                  </td>
                  <td className="num">
                    {m.nativeResolution.width}×{m.nativeResolution.height}
                  </td>
                  <td className="num">
                    {m.currentMode.resolution.width}×{m.currentMode.resolution.height} @{" "}
                    {m.currentMode.refreshHz}Hz
                  </td>
                  <td className="num">
                    {m.bounds.x}, {m.bounds.y}
                  </td>
                  <td className="num">{Math.round(m.dpiScale * 100)}%</td>
                  <td className="num">
                    {m.physicalSize ? (
                      <>
                        {m.physicalSize.width.toFixed(0)} × {m.physicalSize.height.toFixed(0)} mm
                        {!m.physicalSize.millimetrePrecision && (
                          /* EDID bytes 0x15/0x16 are whole centimetres, so a
                             1193 mm panel reports 119 and quantises to ±5 mm.
                             Say so rather than presenting it as exact. */
                          <span className="tag tag--warn">±5mm</span>
                        )}
                      </>
                    ) : (
                      <span className="note">not reported</span>
                    )}
                  </td>
                  <td className="num">{pitchCell(layout, m)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {layout && monitors.length > 0 && (
          <>
            <DesktopMap layout={layout} monitors={monitors} />
            {!layout.isGapless && (
              <ul className="deadlist">
                {layout.deadRegions.map((d, i) => (
                  <li key={i} className="num">
                    {rectLabel(d)}
                  </li>
                ))}
              </ul>
            )}
          </>
        )}
      </Section>

      <Section title="Peripherals" note={`${devices.length} detected`}>
        {deviceError ? (
          <p className="note note--pending">{deviceError}</p>
        ) : (
          <table className="grid">
            <thead>
              <tr>
                <th>Device</th>
                <th>Status</th>
                <th>VID / PID</th>
                <th>HID</th>
                <th>DirectInput</th>
                <th>Notes</th>
              </tr>
            </thead>
            <tbody>
              {devices.map((d) => (
                <tr key={d.device.instancePath ?? `${d.device.vid}-${d.device.pid}`}>
                  <td>{d.device.displayName}</td>
                  <td>
                    <StatusPill status={d.status} />
                  </td>
                  <td className="num">
                    {hex(d.device.vid)} / {hex(d.device.pid)}
                  </td>
                  <td className="num">{d.hidPresent ? "yes" : "no"}</td>
                  <td className="num">
                    {d.dinputPresent ? `slot ${d.dinputSlot ?? "?"}` : "not listed"}
                  </td>
                  <td>
                    {d.isVirtual && <span className="tag">vJoy</span>}
                    {d.bindingDrift && (
                      /* A badge on top of Connected, not a fourth state: the
                         device works, but its DirectInput identity moved and
                         in-game bindings are now silently wrong. */
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
      </Section>
    </>
  );
}

const hex = (n: number) => `0x${n.toString(16).toUpperCase().padStart(4, "0")}`;

/**
 * Pixel pitch, and whether it differs from the panel to its left.
 *
 * Where adjacent panels differ, a bezel gap measured in millimetres cannot be
 * converted to pixels across that seam at all — so the app flags it rather than
 * producing a number that looks right and is not.
 */
function pitchCell(layout: DesktopLayoutInfo | null, m: MonitorInfo) {
  const pitch = layout?.pitches.find((p) => p.devicePath === m.devicePath);
  if (!pitch || pitch.pxPerMm === null) return <span className="note">unknown</span>;
  return (
    <>
      {pitch.pxPerMm.toFixed(2)} px/mm
      {pitch.differsFromNeighbour && (
        <span className="tag tag--warn" title="A bezel gap cannot be expressed in pixels across this seam">
          ▲ differs
        </span>
      )}
    </>
  );
}
