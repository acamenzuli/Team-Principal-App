import type { MonitorInfo, PixelRect } from "../ipc";
import type { DesktopLayoutInfo } from "../ipc";

/**
 * The virtual desktop drawn to scale, with dead regions marked.
 *
 * Not the rig schematic — that lives on Screen Setup and draws *physical*
 * space, in millimetres, from above. This draws pixel space, which is a
 * different question and the one that decides where a window can actually go:
 * two screens can be side by side in the room and nowhere near each other in
 * the desktop's coordinates.
 *
 * It earns its place now because dead regions are invisible until you see
 * them: on a rig with panels of different heights, part of the desktop is
 * addressable and maps to no glass. A window put there simply does not appear,
 * and no amount of reading coordinates makes that as obvious as one picture.
 */
export function DesktopMap({
  layout,
  monitors,
}: {
  layout: DesktopLayoutInfo;
  monitors: MonitorInfo[];
}) {
  const { bounds } = layout;
  // Draw in the desktop's own coordinates and let the viewBox do the scaling,
  // so nothing here has to know the rendered size.
  const pad = Math.round(Math.max(bounds.width, bounds.height) * 0.02);
  const viewBox = [
    bounds.x - pad,
    bounds.y - pad,
    bounds.width + pad * 2,
    bounds.height + pad * 2,
  ].join(" ");

  // Keep strokes and text a constant on-screen size regardless of desktop size.
  const unit = Math.max(bounds.width, bounds.height) / 600;

  return (
    <figure className="map">
      <svg viewBox={viewBox} className="map__svg" role="img" aria-label="Virtual desktop layout">
        <defs>
          <pattern
            id="dead-hatch"
            width={12 * unit}
            height={12 * unit}
            patternUnits="userSpaceOnUse"
            patternTransform="rotate(45)"
          >
            <line
              x1="0"
              y1="0"
              x2="0"
              y2={12 * unit}
              stroke="var(--fail)"
              strokeWidth={3 * unit}
              opacity="0.55"
            />
          </pattern>
        </defs>

        {/* The bounding box: what Windows considers addressable. */}
        <rect
          x={bounds.x}
          y={bounds.y}
          width={bounds.width}
          height={bounds.height}
          fill="none"
          stroke="var(--rule-strong)"
          strokeWidth={unit}
          strokeDasharray={`${6 * unit} ${6 * unit}`}
        />

        {layout.deadRegions.map((d, i) => (
          <g key={`dead-${i}`}>
            <rect x={d.x} y={d.y} width={d.width} height={d.height} fill="url(#dead-hatch)" />
            <rect
              x={d.x}
              y={d.y}
              width={d.width}
              height={d.height}
              fill="none"
              stroke="var(--fail)"
              strokeWidth={unit}
              opacity="0.7"
            />
          </g>
        ))}

        {monitors.map((m) => (
          <g key={m.devicePath}>
            <rect
              x={m.bounds.x}
              y={m.bounds.y}
              width={m.bounds.width}
              height={m.bounds.height}
              fill="var(--ground-raised)"
              stroke={m.isPrimary ? "var(--accent)" : "var(--rule-strong)"}
              strokeWidth={2 * unit}
            />
            <text
              x={m.bounds.x + m.bounds.width / 2}
              y={m.bounds.y + m.bounds.height / 2 - 6 * unit}
              textAnchor="middle"
              fill="var(--ink)"
              fontSize={16 * unit}
            >
              {m.friendlyName}
            </text>
            <text
              x={m.bounds.x + m.bounds.width / 2}
              y={m.bounds.y + m.bounds.height / 2 + 14 * unit}
              textAnchor="middle"
              fill="var(--ink-faint)"
              fontSize={13 * unit}
              fontFamily="var(--font-num)"
            >
              {m.bounds.width}×{m.bounds.height} at {m.bounds.x},{m.bounds.y}
            </text>
          </g>
        ))}
      </svg>

      <figcaption className="map__caption">
        {layout.isGapless ? (
          <span className="map__ok">
            <span aria-hidden="true">●</span> No dead space — every pixel of the desktop is on a
            panel.
          </span>
        ) : (
          <span className="map__dead">
            <span aria-hidden="true">▲</span>{" "}
            <strong className="num">{formatArea(layout.deadArea)}</strong> of the desktop maps to no
            panel, in {layout.deadRegions.length}{" "}
            {layout.deadRegions.length === 1 ? "region" : "regions"}. A window placed there is
            addressable and invisible.
          </span>
        )}
      </figcaption>
    </figure>
  );
}

function formatArea(px: number): string {
  const mpx = px / 1_000_000;
  return mpx >= 0.1 ? `${mpx.toFixed(2)} Mpx` : `${Math.round(px).toLocaleString()} px`;
}

export function rectLabel(r: PixelRect): string {
  return `${r.width}×${r.height} at ${r.x},${r.y}`;
}
