import { CurvatureBench } from "./CurvatureBench";
import { HardwarePanel } from "./HardwarePanel";
import "./dashboard.css";

/**
 * Milestone 1 dashboard.
 *
 * Two panels, both deliberately chosen to prove a whole pipeline end to end
 * rather than to look finished:
 *
 * - Hardware reads through the provider seam, so real and mock are already
 *   interchangeable.
 * - Curvature runs the actual geometry crate over IPC, so the Rust math, the
 *   generated bindings and the typed client are all exercised by one number
 *   changing on screen.
 */
export function Dashboard() {
  return (
    <div className="dash">
      <HardwarePanel />
      <CurvatureBench />
    </div>
  );
}
