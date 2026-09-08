import { DisplayControl } from "../displays/DisplayControl";
import { CurvatureBench } from "./CurvatureBench";
import { HardwarePanel } from "./HardwarePanel";
import "./dashboard.css";

/**
 * The Displays tab: what is attached, what it is doing, and how to change it.
 *
 * Reading comes first and changing comes second, in that order on the page,
 * because the layout you have is the context for the one you are proposing.
 */
export function Dashboard() {
  return (
    <div className="dash">
      <HardwarePanel />
      <DisplayControl />
      <CurvatureBench />
    </div>
  );
}
