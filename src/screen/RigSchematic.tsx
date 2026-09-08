import type { RigSolutionInfo } from "../ipc";

/**
 * The rig seen from above.
 *
 * This is the view that catches a wrong angle instantly. A number in a field
 * looks equally plausible at 45 and 65 degrees; the shape of the rig does not.
 *
 * Drawn straight from the solver's output — the screens are line segments
 * between their solved corners, the eye is the origin, and the sight lines are
 * the actual angular spans. Nothing here recomputes geometry, so what you see
 * is what the adapters will be told.
 */
/** A solved corner as its top-down coordinates. */
function xz(corner: number[] | undefined): { x: number; z: number } {
  return { x: corner?.[0] ?? 0, z: corner?.[2] ?? 0 };
}

export function RigSchematic({ solution }: { solution: RigSolutionInfo }) {
  if (solution.screens.length === 0) {
    return <p className="note">Nothing to draw yet — add a centre screen.</p>;
  }

  // Top-down: x across, z away from the driver. The eye is at the origin.
  //
  // ts-rs does emit `corners` as a proper 4-tuple of 3-tuples, but collecting
  // two of them into an array literal widens that to number[], and
  // noUncheckedIndexedAccess then makes each component possibly undefined.
  // `xz` is where that is handled, once.
  const edges = solution.screens.flatMap((s) => [xz(s.corners[0]), xz(s.corners[2])]);
  const maxX = Math.max(...edges.map((e) => Math.abs(e.x)), 100);
  const maxZ = Math.max(...edges.map((e) => e.z), 100);
  const pad = Math.max(maxX, maxZ) * 0.12;

  // z grows away from the driver, and the driver belongs at the bottom of the
  // picture, so the vertical axis is flipped.
  const viewBox = `${-maxX - pad} ${-pad} ${(maxX + pad) * 2} ${maxZ + pad * 2}`;
  const unit = Math.max(maxX, maxZ) / 400;

  return (
    <figure className="schematic">
      <svg
        viewBox={viewBox}
        className="schematic__svg"
        role="img"
        aria-label="The rig seen from above"
        style={{ transform: "scaleY(-1)" }}
      >
        {/* Sight lines to each screen's edges: the actual solved angles. */}
        {solution.screens.map((s) =>
          [s.corners[0], s.corners[2]].map((c, i) => (
            <line
              key={`${s.id}-ray-${i}`}
              x1={0}
              y1={0}
              x2={xz(c).x}
              y2={xz(c).z}
              stroke="var(--rule-strong)"
              strokeWidth={unit}
              strokeDasharray={`${5 * unit} ${5 * unit}`}
            />
          )),
        )}

        {/* Straight ahead, so an asymmetric seating position is visible. */}
        <line
          x1={0}
          y1={0}
          x2={0}
          y2={maxZ}
          stroke="var(--accent)"
          strokeWidth={unit}
          opacity="0.45"
        />

        {/* Each screen as its own surface, inboard corner to outboard corner. */}
        {solution.screens.map((s) => (
          <line
            key={s.id}
            x1={xz(s.corners[0]).x}
            y1={xz(s.corners[0]).z}
            x2={xz(s.corners[2]).x}
            y2={xz(s.corners[2]).z}
            stroke={s.role.kind === "center" ? "var(--accent)" : "var(--ink)"}
            strokeWidth={6 * unit}
            strokeLinecap="round"
          />
        ))}

        {/* The driver. */}
        <circle cx={0} cy={0} r={9 * unit} fill="var(--accent)" />
      </svg>

      <figcaption className="schematic__caption">
        <span className="num">{solution.totalCoverageDeg.toFixed(1)}°</span> total coverage,
        edge to edge. Seen from above; the dot is your eye point.
      </figcaption>
    </figure>
  );
}
