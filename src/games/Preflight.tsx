import { useEffect, useState } from "react";

import {
  asIpcError,
  cancelPreflight,
  launchGame,
  onLaunchState,
  onStep,
  retryStep,
  skipStep,
  startPreflight,
  type ReadyState,
  type StepStatus,
  type StepView,
} from "../ipc";
import "./preflight.css";

/**
 * The preflight checklist and the ready gate.
 *
 * A pure view over the step event stream. It never sequences anything — that is
 * the executor's job — which is what lets a self-healing check turn green on
 * its own by the same path its first result took. A row that goes red because
 * the pedals came unplugged goes green again when they are plugged back in,
 * with nobody pressing anything.
 *
 * Status is icon plus colour plus word on every row. This is a go/no-go screen,
 * and colourblind users exist.
 *
 * Launching is a second, deliberate press. The button below the list is the
 * only thing that starts the game, and the executor holds the launch phase
 * until it is pressed — the checklist finishing does not launch anything.
 */
export function Preflight({
  profileId,
  name,
  onClose,
}: {
  profileId: string;
  name: string;
  onClose: () => void;
}) {
  const [steps, setSteps] = useState<StepView[]>([]);
  const [state, setState] = useState<ReadyState>("running");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let unlistenStep: (() => void) | undefined;
    let unlistenState: (() => void) | undefined;

    void onStep((step) => {
      setSteps((prev) => prev.map((s) => (s.id === step.id ? step : s)));
    }).then((f) => (unlistenStep = f));
    void onLaunchState(setState).then((f) => (unlistenState = f));

    startPreflight(profileId)
      .then(setSteps)
      .catch((e) => setError(asIpcError(e).message));

    return () => {
      unlistenStep?.();
      unlistenState?.();
      void cancelPreflight();
    };
  }, [profileId]);

  const preflight = steps.filter((s) => s.phase === "preflight");
  const launch = steps.filter((s) => s.phase === "launch");
  const done = preflight.filter((s) => TERMINAL.has(s.status)).length;
  const progress = preflight.length === 0 ? 0 : done / preflight.length;
  const blockers = preflight.filter((s) => s.status === "failed" && s.severity === "fatal");
  const started = state === "launching" || state === "racing" || state === "launch_failed";
  const rows = started ? [...preflight, ...launch] : preflight;

  return (
    <div className="pf">
      <header className="pf__head">
        <h2 className="pf__title">{name}</h2>
        <button className="btn btn--quiet" onClick={onClose}>
          Close
        </button>
      </header>

      {error && <p className="warn warn--hard">{error}</p>}

      <ol className="pf__steps">
        {/* The steps genuinely are a sequence, so the connecting line is
            earned here rather than decorative. It fills as the run advances. */}
        <span className="pf__line" style={{ transform: `scaleY(${progress})` }} aria-hidden="true" />
        {rows.map((s, i) => (
          <li className={`pf__step pf__step--${s.status}`} key={s.id}>
            <span className="pf__num num">{i + 1}</span>
            <span className="pf__icon" aria-hidden="true">
              {GLYPH[s.status]}
            </span>
            <span className="pf__body">
              <span className="pf__label">{s.label}</span>
              {detailOf(s) && <span className="pf__detail">{detailOf(s)}</span>}
            </span>
            <Fixes step={s} launched={started} />
            <span className="pf__word">{WORD[s.status]}</span>
            <span className="pf__time num">
              {s.elapsedMs !== null ? `${(s.elapsedMs / 1000).toFixed(1)}s` : ""}
            </span>
          </li>
        ))}
      </ol>

      <div className={`pf__gate pf__gate--${state}`}>
        <div className="pf__verdict-block">
          <p className="pf__verdict">{VERDICT[state]}</p>
          <p className="pf__sub">{subtitle(state, preflight, blockers)}</p>
        </div>

        {/* Launch is a deliberate second press, and it is the only thing on
            this screen that starts a game. */}
        {(state === "ready" || state === "ready_with_warnings") && (
          <button className="btn btn--go" onClick={() => void launchGame(false)}>
            Launch {name}
          </button>
        )}

        {state === "blocked" && (
          <button
            className="btn btn--quiet btn--override"
            onClick={() => void launchGame(true)}
            title={blockers.map((s) => s.label).join(", ")}
          >
            Race anyway
          </button>
        )}
      </div>

      {state === "blocked" && (
        <p className="hint">
          Racing anyway marks {blockers.length === 1 ? "that check" : "those checks"} skipped rather
          than passed, and the list keeps saying so.
        </p>
      )}
    </div>
  );
}

/**
 * The fix buttons on a failed row.
 *
 * A red row with no button is a dead end. Retry re-runs the step and everything
 * downstream of it; Skip stops it blocking the gate without ever claiming it
 * passed. Once the game is up neither means anything, so neither is shown.
 */
function Fixes({ step, launched }: { step: StepView; launched: boolean }) {
  const stuck = step.status === "failed" || step.status === "skipped";
  if (launched || !stuck) return <span />;

  return (
    <span className="pf__fixes">
      <button className="btn btn--tiny" onClick={() => void retryStep(step.id)}>
        {step.fix?.kind === "start" ? "Start" : "Retry"}
      </button>
      {step.status === "failed" && step.severity === "fatal" && (
        <button className="btn btn--tiny btn--quiet" onClick={() => void skipStep(step.id)}>
          Skip
        </button>
      )}
    </span>
  );
}

/**
 * The row's live text.
 *
 * "Already running" comes from what the executor actually did, never from a
 * hand-written string — which is the whole reason ActionTaken exists.
 */
function detailOf(step: StepView): string {
  if (step.actionTaken === "already_running" && step.detail) return step.detail;
  return step.detail;
}

function subtitle(state: ReadyState, preflight: StepView[], blockers: StepView[]): string {
  switch (state) {
    case "running":
      return "Checking…";
    case "ready":
      return "Everything passed.";
    case "ready_with_warnings":
      return (
        preflight
          .filter((s) => s.status === "warning" || s.status === "failed" || s.status === "skipped")
          .map((s) => s.label)
          .join(", ") || "Some checks did not pass, none of them blocking."
      );
    case "blocked":
      return blockers.map((s) => s.detail || s.label).join(" · ");
    case "launching":
      return "Starting the game.";
    case "racing":
      return "The game is up. Applying the saved window geometry to it comes next.";
    case "launch_failed":
      return "The checks passed but the game did not start.";
  }
}

const TERMINAL = new Set<StepStatus>(["passed", "warning", "failed", "skipped"]);

const VERDICT: Record<ReadyState, string> = {
  running: "Checking",
  ready: "Ready",
  ready_with_warnings: "Ready, with warnings",
  blocked: "Blocked",
  launching: "Launching",
  racing: "Racing",
  launch_failed: "Launch failed",
};

/** Status is never carried by colour alone: glyph, colour and word, always. */
const GLYPH: Record<StepStatus, string> = {
  pending: "○",
  running: "◐",
  passed: "●",
  warning: "▲",
  failed: "■",
  skipped: "◌",
};

const WORD: Record<StepStatus, string> = {
  pending: "WAIT",
  running: "RUN",
  passed: "PASS",
  warning: "WARN",
  failed: "FAIL",
  skipped: "SKIP",
};
