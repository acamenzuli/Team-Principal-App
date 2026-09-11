import { describeStage, fractionOf, useInstall } from "./useInstall";
import type { UpdateInfo } from "../ipc";
import "./updates.css";

/**
 * The new-version strip across the top of the app.
 *
 * A strip rather than a dialog: nobody opened this app to read about a new
 * version of it, and a modal would stand between them and the button they came
 * for.
 *
 * It stays up through the install and says what is happening — downloading,
 * verifying, restarting — because an update ends with the app being replaced,
 * and thirty silent seconds before that reads as a crash. A failure stays on
 * screen with its reason instead of the strip simply vanishing, which is what
 * happened when the error was thrown into a click handler and discarded.
 */
export function UpdateStrip({
  found,
  onDismiss,
}: {
  found: UpdateInfo | null;
  onDismiss: () => void;
}) {
  const { stage, busy, start, clear } = useInstall();

  // Nothing to offer and nothing happening.
  if (!found?.available && stage === null) return null;

  const failed = stage?.kind === "failed";
  const fraction = stage ? fractionOf(stage) : null;

  return (
    <div className={`strip${failed ? " strip--failed" : ""}`}>
      <span aria-hidden="true">{failed ? "▲" : "●"}</span>

      <span className="strip__text">
        {stage ? (
          describeStage(stage)
        ) : (
          <>
            Version <strong className="num">{found?.newVersion}</strong> is available.
          </>
        )}
      </span>

      {stage?.kind === "downloading" && (
        <span className={`strip__bar${fraction === null ? " strip__bar--unknown" : ""}`}>
          <span
            className="strip__fill"
            style={fraction === null ? undefined : { width: `${Math.round(fraction * 100)}%` }}
          />
        </span>
      )}

      {!busy && (
        <button className="btn btn--tiny" onClick={start}>
          {failed ? "Try again" : "Install and restart"}
        </button>
      )}

      {!busy && (
        <button
          className="btn btn--tiny btn--quiet"
          onClick={() => {
            clear();
            onDismiss();
          }}
        >
          Later
        </button>
      )}
    </div>
  );
}
