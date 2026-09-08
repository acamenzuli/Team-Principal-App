import { useEffect, useState } from "react";

import {
  asIpcError,
  cancelPreflight,
  onLaunchState,
  onStep,
  startPreflight,
  type ReadyState,
  type StepView,
} from "../ipc";
import "./preflight.css";

/**
 * The preflight checklist.
 *
 * A pure view over the step event stream. It never sequences anything — that
 * is the executor's job — which is what will let a self-healing check turn
 * green on its own by the same path its first result took.
 *
 * Status is icon plus colour plus word on every row. This is a go/no-go
 * screen, and colourblind users exist.
 */
export function Preflight({ gameName, onClose }: { gameName: string; onClose: () => void }) {
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

    startPreflight(gameName)
      .then(setSteps)
      .catch((e) => setError(asIpcError(e).message));

    return () => {
      unlistenStep?.();
      unlistenState?.();
      void cancelPreflight();
    };
  }, [gameName]);

  const preflight = steps.filter((s) => s.phase === "preflight");
  const done = preflight.filter((s) => s.status !== "pending" && s.status !== "running").length;
  const progress = preflight.length === 0 ? 0 : done / preflight.length;

  return (
    <div className="pf">
      <header className="pf__head">
        <h2 className="pf__title">{gameName}</h2>
        <button className="btn btn--quiet" onClick={onClose}>
          Close
        </button>
      </header>

      {error && <p className="warn warn--hard">{error}</p>}

      <ol className="pf__steps">
        {/* The steps genuinely are a sequence, so the connecting line is
            earned here rather than decorative. It fills as the run advances. */}
        <span className="pf__line" style={{ transform: `scaleY(${progress})` }} aria-hidden="true" />
        {preflight.map((s, i) => (
          <li className={`pf__step pf__step--${s.status}`} key={s.id}>
            <span className="pf__num num">{i + 1}</span>
            <span className="pf__icon" aria-hidden="true">
              {GLYPH[s.status]}
            </span>
            <span className="pf__body">
              <span className="pf__label">{s.label}</span>
              {s.detail && <span className="pf__detail">{s.detail}</span>}
            </span>
            <span className="pf__word">{WORD[s.status]}</span>
            <span className="pf__time num">
              {s.elapsedMs !== null ? `${(s.elapsedMs / 1000).toFixed(1)}s` : ""}
            </span>
          </li>
        ))}
      </ol>

      <div className={`pf__gate pf__gate--${state}`}>
        {state === "running" && <p>Checking…</p>}
        {state === "ready" && (
          <>
            <p className="pf__verdict">Ready</p>
            <p className="pf__sub">Everything passed.</p>
          </>
        )}
        {state === "ready_with_warnings" && (
          <>
            <p className="pf__verdict">Ready, with warnings</p>
            <p className="pf__sub">
              {preflight
                .filter((s) => s.status === "warning" || s.status === "failed")
                .map((s) => s.label)
                .join(", ")}
            </p>
          </>
        )}
        {state === "blocked" && (
          <>
            <p className="pf__verdict">Blocked</p>
            <p className="pf__sub">
              {preflight
                .filter((s) => s.status === "failed")
                .map((s) => s.detail || s.label)
                .join(" · ")}
            </p>
          </>
        )}
      </div>

      <p className="hint">
        Launching is deliberately a second click, and arrives with the full flow. These checks are
        real but few — the profile that says which utilities to start, which peripherals are
        required, and which display layout to use is the next milestone.
      </p>
    </div>
  );
}

/** Status is never carried by colour alone: glyph, colour and word, always. */
const GLYPH: Record<string, string> = {
  pending: "○",
  running: "◐",
  passed: "●",
  warning: "▲",
  failed: "■",
  skipped: "○",
};

const WORD: Record<string, string> = {
  pending: "WAIT",
  running: "RUN",
  passed: "PASS",
  warning: "WARN",
  failed: "FAIL",
  skipped: "SKIP",
};
