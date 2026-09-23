import { useCallback, useEffect, useRef, useState } from "react";

import { api, type Clip, type Connection, type JobProgress } from "./client";

type Filter = "all" | "failed" | "marked" | "derived" | "preview";

const FILTERS: [Filter, string, string][] = [
  ["all", "Everything", "every clip in the pack"],
  ["failed", "Needs a look", "the check was not satisfied"],
  ["marked", "Marked", "ones you flagged"],
  ["derived", "Guessed words", "no subtitle existed, so the words came from the folder name"],
  ["preview", "Preview set", "what you hear in the first minutes of a race"],
];

/**
 * Listening to what was made.
 *
 * Every clip with what it was asked to say, what the checking model heard,
 * and the audio itself. A clip that failed is here rather than hidden: the
 * pack ships without it, and this is where you decide whether it needed a
 * different spelling or just another try.
 */
export function Review({
  conn,
  packId,
  job,
  onError,
}: {
  conn: Connection;
  packId: string;
  job: JobProgress | null;
  onError: (message: string | null) => void;
}) {
  const [clips, setClips] = useState<Clip[]>([]);
  const [total, setTotal] = useState(0);
  const [filter, setFilter] = useState<Filter>("failed");
  const [search, setSearch] = useState("");
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const player = useRef<HTMLAudioElement | null>(null);

  const load = useCallback(async () => {
    try {
      const query: Record<string, string | number | boolean | undefined> = { limit: 300 };
      if (filter === "failed") query.state_filter = "failed";
      if (filter === "marked") query.marked = true;
      if (filter === "derived") query.derived = true;
      if (filter === "preview") query.preview = true;
      if (search.trim()) query.q = search.trim();
      const result = await api.clips(conn, packId, query);
      setClips(result.clips);
      setTotal(result.total);
      onError(null);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  }, [conn, packId, filter, search, onError]);

  useEffect(() => {
    void load();
  }, [load]);

  // While a job runs the verdicts keep changing; refresh gently rather than
  // on every one of a thousand events.
  useEffect(() => {
    if (!job || !["running", "finishing"].includes(job.state)) return;
    const timer = window.setInterval(() => void load(), 5000);
    return () => window.clearInterval(timer);
  }, [job?.state, job, load]);

  const play = (clip: Clip) => {
    if (!clip.audio) return;
    const url = api.clipUrl(conn, packId, clip.rel_path, clip.audio);
    if (player.current) {
      player.current.pause();
    }
    const audio = new Audio(url);
    player.current = audio;
    void audio.play().catch((e) => onError(`Could not play that clip: ${e}`));
  };

  const mark = async (clip: Clip) => {
    try {
      await api.mark(conn, packId, clip.rel_path, !clip.review?.marked);
      await load();
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  const saveText = async (clip: Clip) => {
    setBusy(true);
    try {
      const text = draft.trim();
      await api.setText(conn, packId, clip.rel_path, text === clip.subtitle ? null : text);
      await api.generate(conn, packId, { scope: "selection", rel_paths: [clip.rel_path], regenerate: true });
      setEditing(null);
      onError(null);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const again = async (clip: Clip) => {
    setBusy(true);
    try {
      await api.generate(conn, packId, { scope: "selection", rel_paths: [clip.rel_path], regenerate: true });
      onError(null);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const allFailed = clips.filter((c) => c.state === "failed").map((c) => c.rel_path);

  return (
    <div className="voicelab__screen">
      <div className="group glass">
        <header className="group__head">
          <h2 className="group__title">Listen</h2>
          <p className="group__note">{total.toLocaleString()} clips</p>
        </header>
        <div className="group__body">
          <div className="voicelab__filters">
            <div className="segmented" role="radiogroup">
              {FILTERS.map(([id, label, description]) => (
                <button
                  key={id}
                  role="radio"
                  aria-checked={filter === id}
                  className={`segmented__item${filter === id ? " segmented__item--on" : ""}`}
                  onClick={() => setFilter(id)}
                >
                  <span className="segmented__label">{label}</span>
                  <span className="segmented__desc">{description}</span>
                </button>
              ))}
            </div>
            <input
              className="text-input"
              placeholder="Search the words, or a folder"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>

          {filter === "failed" && allFailed.length > 0 && (
            <div className="settings__actions">
              <button
                className="btn btn--quiet"
                disabled={busy}
                onClick={() =>
                  void api
                    .generate(conn, packId, { scope: "selection", rel_paths: allFailed, regenerate: true })
                    .then(() => onError(null))
                    .catch((e) => onError(String(e)))
                }
              >
                Try all {allFailed.length} again
              </button>
            </div>
          )}

          {clips.length === 0 ? (
            <p className="note">
              {filter === "failed"
                ? "Nothing failed. Every clip said what it was asked to say."
                : "Nothing here yet."}
            </p>
          ) : (
            <table className="voicelab__clips">
              <thead>
                <tr>
                  <th>State</th>
                  <th>Said</th>
                  <th>Heard back</th>
                  <th>Where</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {clips.map((clip) => (
                  <tr key={clip.rel_path} className={clip.review?.marked ? "voicelab__clip--marked" : undefined}>
                    <td>
                      <span
                        className={
                          clip.state === "passed"
                            ? "status status--pass"
                            : clip.state === "failed"
                              ? "status status--fail"
                              : "status status--idle"
                        }
                      >
                        <span aria-hidden="true">
                          {clip.state === "passed" ? "●" : clip.state === "failed" ? "■" : "○"}
                        </span>{" "}
                        {clip.state === "passed" ? "KEPT" : clip.state === "failed" ? "NO" : "WAIT"}
                      </span>
                      {clip.attempts > 1 && <span className="note num"> ×{clip.attempts}</span>}
                    </td>
                    <td>
                      {editing === clip.rel_path ? (
                        <div className="voicelab__edit">
                          <input
                            className="text-input"
                            value={draft}
                            autoFocus
                            onChange={(e) => setDraft(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter") void saveText(clip);
                              if (e.key === "Escape") setEditing(null);
                            }}
                          />
                          <button className="btn btn--small" onClick={() => void saveText(clip)} disabled={busy}>
                            Say it this way
                          </button>
                          <button className="btn btn--quiet btn--small" onClick={() => setEditing(null)}>
                            Cancel
                          </button>
                        </div>
                      ) : (
                        <button
                          className="voicelab__text"
                          title="Change how this is pronounced"
                          onClick={() => {
                            setEditing(clip.rel_path);
                            setDraft(clip.text);
                          }}
                        >
                          {clip.text}
                          {clip.derived && <span className="badge badge--warn">guessed</span>}
                        </button>
                      )}
                      {clip.text !== clip.subtitle && (
                        <p className="note">CrewChief shows: {clip.subtitle}</p>
                      )}
                    </td>
                    <td>
                      {clip.transcript ? (
                        <>
                          <span className={clip.state === "failed" ? "voicelab__heard--bad" : undefined}>
                            {clip.transcript}
                          </span>
                          {clip.reasons.length > 0 && (
                            <p className="note">{describe(clip.reasons)}</p>
                          )}
                        </>
                      ) : (
                        <span className="note">—</span>
                      )}
                    </td>
                    <td className="num voicelab__where">{clip.intent}</td>
                    <td className="voicelab__clip-actions">
                      {clip.audio && (
                        <button className="btn btn--quiet btn--small" onClick={() => play(clip)}>
                          Play
                        </button>
                      )}
                      <button className="btn btn--quiet btn--small" onClick={() => void again(clip)} disabled={busy}>
                        Again
                      </button>
                      <button
                        className="btn btn--quiet btn--small"
                        onClick={() => void mark(clip)}
                        title={clip.review?.marked ? "Unmark" : "Mark to come back to"}
                      >
                        {clip.review?.marked ? "Unmark" : "Mark"}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>
    </div>
  );
}

function describe(reasons: string[]): string {
  const names: Record<string, string> = {
    transcript_mismatch: "said something else",
    too_short: "too short",
    too_long: "too long",
    clipping: "clipped",
    too_quiet: "too quiet",
    long_pause: "a long pause in the middle",
    spectral_noise: "noisy",
    silent: "silent",
    empty_or_invalid: "no audio",
    low_confidence: "hard to make out",
  };
  return reasons.map((r) => names[r] ?? r).join(", ");
}
