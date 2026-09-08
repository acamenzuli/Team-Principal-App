import type { RigSolutionInfo } from "../ipc";

/**
 * The rig as you face it, to true physical scale.
 *
 * Widths and heights are millimetres from EDID or from a tape measure, so the
 * bezel gaps are drawn as the real dark bands they are — not as a uniform
 * divider. This is where an unequal panel height or a mis-typed bezel becomes
 * obvious.
 *
 * Angled side screens appear foreshortened here, exactly as they do in life.
 * That is deliberate: it draws what the eye sees, not a flattened plan.
 */
export function ScaleElevation({ solution }: { solution: RigSolutionInfo }) {
  if (solution.screens.length === 0) return null;

  // Project each screen's corners onto the viewing sphere and plot in degrees,
  // which is the only frame in which angled panels can share one picture
  // honestly.
  const boxes = solution.screens.map((s) => ({
    id: s.id,
    isCentre: s.role.kind === "center",
    left: s.span.leftDeg,
    right: s.span.rightDeg,
    bottom: s.span.bottomDeg,
    top: s.span.topDeg,
    label: `${s.hFovDeg.toFixed(1)}° × ${s.vFovDeg.toFixed(1)}°`,
  }));

  const left = Math.min(...boxes.map((b) => b.left));
  const right = Math.max(...boxes.map((b) => b.right));
  const bottom = Math.min(...boxes.map((b) => b.bottom));
  const top = Math.max(...boxes.map((b) => b.top));
  const padX = (right - left) * 0.04;
  const padY = Math.max((top - bottom) * 0.25, 4);

  const viewBox = `${left - padX} ${-top - padY} ${right - left + padX * 2} ${
    top - bottom + padY * 2
  }`;
  const unit = (right - left) / 400;

  return (
    <figure className="elevation">
      <svg
        viewBox={viewBox}
        className="elevation__svg"
        role="img"
        aria-label="The rig as you face it, in degrees of field of view"
      >
        {/* Eye level and straight ahead. */}
        <line
          x1={left - padX}
          y1={0}
          x2={right + padX}
          y2={0}
          stroke="var(--rule)"
          strokeWidth={unit}
        />
        <line
          x1={0}
          y1={-top - padY}
          x2={0}
          y2={-bottom + padY}
          stroke="var(--accent)"
          strokeWidth={unit}
          opacity="0.45"
        />

        {boxes.map((b) => (
          <g key={b.id}>
            <rect
              x={b.left}
              y={-b.top}
              width={b.right - b.left}
              height={b.top - b.bottom}
              fill="var(--ground-raised)"
              stroke={b.isCentre ? "var(--accent)" : "var(--rule-strong)"}
              strokeWidth={2 * unit}
            />
            <text
              x={(b.left + b.right) / 2}
              y={-(b.top + b.bottom) / 2}
              textAnchor="middle"
              dominantBaseline="middle"
              fill="var(--ink-dim)"
              fontSize={9 * unit}
              fontFamily="var(--font-num)"
            >
              {b.label}
            </text>
          </g>
        ))}
      </svg>
      <figcaption className="elevation__caption">
        Degrees of field of view, as seen from the driver's seat. The gaps between panels are the
        bezels and mount gaps, at the angle they actually subtend.
      </figcaption>
    </figure>
  );
}
