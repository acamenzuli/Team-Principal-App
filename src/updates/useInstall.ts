import { useEffect, useState } from "react";

import { installUpdate, onUpdateStage, type UpdateStage } from "../ipc";

/**
 * The live state of an update install, shared by everything that can start one.
 *
 * An install is the one operation that ends with this app being replaced and
 * restarted, so silence during it reads as a crash. The backend publishes every
 * phase — including the refusals, which both callers previously dropped on the
 * floor — and this subscribes to them rather than inferring anything from a
 * disabled button.
 *
 * Subscribed always, not only while installing: an automatic install starts
 * without anybody pressing anything, and if it fails, whoever is looking at the
 * app should be told.
 */
export function useInstall() {
  const [stage, setStage] = useState<UpdateStage | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void onUpdateStage((next) => setStage(next)).then((f) => {
      if (cancelled) f();
      else unlisten = f;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const start = () => {
    setStage({ kind: "checking" });
    // The error also arrives as a Failed stage, which is what the UI renders.
    // Swallowing it here rather than throwing into an event handler keeps the
    // single source of truth on the event stream.
    void installUpdate().catch(() => {});
  };

  const busy = stage !== null && stage.kind !== "failed";

  return { stage, busy, start, clear: () => setStage(null) };
}

/** One line describing where an install is, for a strip or a settings row. */
export function describeStage(stage: UpdateStage): string {
  switch (stage.kind) {
    case "checking":
      return "Checking…";
    case "downloading":
      return stage.total === null
        ? `Downloading — ${megabytes(stage.downloaded)}`
        : `Downloading — ${megabytes(stage.downloaded)} of ${megabytes(stage.total)}`;
    case "installing":
      return "Verified. Installing…";
    case "restarting":
      return "Installed. Restarting…";
    case "failed":
      return stage.message;
  }
}

/** Fraction downloaded, or null when the server sent no length to measure. */
export function fractionOf(stage: UpdateStage): number | null {
  if (stage.kind !== "downloading" || stage.total === null || stage.total === 0) return null;
  return Math.min(1, stage.downloaded / stage.total);
}

const megabytes = (bytes: number) => `${(bytes / 1_000_000).toFixed(1)} MB`;
