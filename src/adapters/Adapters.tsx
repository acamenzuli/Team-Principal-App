import { useCallback, useEffect, useState } from "react";

import { Section } from "../dashboard/primitives";
import {
  applyAdapter,
  asIpcError,
  currentRig,
  listAdapters,
  listBackups,
  previewAdapter,
  restoreBackup,
  type AdapterInfo,
  type AdapterPreview,
  type BackupInfo,
  type FileDiff,
  type RigModel,
  type SessionMode,
} from "../ipc";
import "./adapters.css";

/**
 * Writing your rig into the games.
 *
 * The premise of the product, made visible: you measured the rig once, and
 * these are the numbers that go into each sim. Three rules the screen exists to
 * enforce:
 *
 * * **Nothing is written until you have read the diff.** The preview is
 *   generated when you pick an adapter, not behind a button.
 * * **Every value says why.** A number with no reason is a number nobody can
 *   check against their own tape measure.
 * * **How well each adapter is known is shown.** A key name read out of the
 *   game's own file and one corroborated across forum posts are different
 *   claims, and they do not get the same badge.
 */
export function Adapters() {
  // The session mode belongs to this screen rather than to preferences: which
  // screens a title uses is a per-title decision, and the adapter's numbers
  // change completely between them.
  const [session, setSession] = useState<SessionMode>({ kind: "full_span", fit: "letterbox" });
  const [adapters, setAdapters] = useState<AdapterInfo[]>([]);
  const [rig, setRig] = useState<RigModel | null>(null);
  const [chosen, setChosen] = useState<string | null>(null);
  const [preview, setPreview] = useState<AdapterPreview | null>(null);
  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [applied, setApplied] = useState<string | null>(null);

  const loadBackups = useCallback(async () => {
    try {
      setBackups(await listBackups());
    } catch {
      // A missing backup directory is the normal first-run state, not a fault.
      setBackups([]);
    }
  }, []);

  useEffect(() => {
    Promise.all([listAdapters(), currentRig()])
      .then(([found, r]) => {
        setAdapters(found);
        setRig(r);
      })
      .catch((e) => setError(asIpcError(e).message));
    void loadBackups();
  }, [loadBackups]);

  useEffect(() => {
    if (!chosen || !rig) return;
    let live = true;
    setPreview(null);
    previewAdapter({ adapterId: chosen, rig, session })
      .then((p) => live && setPreview(p))
      .catch((e) => live && setError(asIpcError(e).message));
    return () => {
      live = false;
    };
  }, [chosen, rig, session]);

  async function write() {
    if (!chosen || !rig) return;
    setBusy(true);
    try {
      const result = await applyAdapter({ adapterId: chosen, rig, session });
      setApplied(result.backup);
      setError(null);
      await loadBackups();
      // Re-read, so the screen shows the file as it now is rather than as it was.
      setPreview(await previewAdapter({ adapterId: chosen, rig, session }));
    } catch (e) {
      setError(asIpcError(e).message);
    } finally {
      setBusy(false);
    }
  }

  const willChange = preview?.files.some((f) => f.changes.some((c) => !c.unchanged)) ?? false;

  return (
    <div className="ad">
      <Section title="Game settings" note={`${adapters.length} adapters`}>
        <p className="note">
          Your rig's measurements, written into each sim's own config file. Nothing is typed twice
          and nothing is guessed — every value below traces back to a number on the Screen Setup
          tab.
        </p>

        <div className="ad__session" role="radiogroup" aria-label="Session">
          {SESSIONS.map(([mode, label]) => (
            <button
              key={label}
              role="radio"
              aria-checked={session.kind === mode.kind}
              className={`ad__choice${session.kind === mode.kind ? " ad__choice--on" : ""}`}
              onClick={() => setSession(mode)}
            >
              {label}
            </button>
          ))}
        </div>

        {error && <p className="warn warn--hard">{error}</p>}

        <ul className="ad__list">
          {adapters.map((a) => (
            <li
              key={a.id}
              className={`ad__adapter${chosen === a.id ? " ad__adapter--on" : ""}`}
            >
              <button className="ad__pick" onClick={() => setChosen(chosen === a.id ? null : a.id)}>
                <span className="ad__title">
                  {a.title}
                  <ConfidenceBadge confidence={a.confidence} />
                </span>
                <span className="ad__scope">{a.scope}</span>
              </button>
              {chosen === a.id && <p className="ad__source">{a.source}</p>}
            </li>
          ))}
        </ul>
      </Section>

      {chosen && (
        <Section title="What will change" note={preview === null ? "reading" : undefined}>
          {preview === null ? (
            <p className="note note--pending">Reading your config files…</p>
          ) : (
            <>
              {preview.files.map((f) => (
                <FileBlock key={f.path} diff={f} />
              ))}

              {preview.warnings.map((w) => (
                <p className="note note--fail" key={w}>
                  {w}
                </p>
              ))}

              {applied && (
                <p className="note">
                  Written. The previous versions are saved — use Put it back below to undo exactly
                  this change.
                </p>
              )}

              <div className="ad__actions">
                <button className="btn" disabled={!willChange || busy} onClick={() => void write()}>
                  {busy ? "Writing…" : willChange ? "Back up and write" : "Nothing to change"}
                </button>
              </div>
            </>
          )}
        </Section>
      )}

      {backups.length > 0 && (
        <Section title="Backups" note={`${backups.length} kept`}>
          <p className="note">
            Every file this app writes is copied here first. Restoring puts a whole change back at
            once — half of a two-file change would leave a state that never existed.
          </p>
          <ul className="ad__backups">
            {backups.map((b) => (
              <li key={b.id}>
                <span className="num">{b.takenAt.replace(/-/g, ":").replace("T", " ")}</span>
                <span className="note">{b.reason}</span>
                <span className="note num">{b.files.length} files</span>
                <button
                  className="btn btn--quiet"
                  onClick={() =>
                    void restoreBackup(b.id)
                      .then(() => loadBackups())
                      .catch((e) => setError(asIpcError(e).message))
                  }
                >
                  Put it back
                </button>
              </li>
            ))}
          </ul>
        </Section>
      )}
    </div>
  );
}

function FileBlock({ diff }: { diff: FileDiff }) {
  if (!diff.exists) {
    return (
      <div className="ad__file">
        <p className="ad__path num">{diff.path}</p>
        <p className="note note--pending">
          Not there yet. Run the game once so it writes its settings — Team Principal edits that
          file, it does not invent it.
        </p>
      </div>
    );
  }

  return (
    <div className="ad__file">
      <p className="ad__path num">{diff.path}</p>

      <table className="grid ad__diff">
        <thead>
          <tr>
            <th>Setting</th>
            <th>Now</th>
            <th>Becomes</th>
            <th>Because</th>
          </tr>
        </thead>
        <tbody>
          {diff.changes.map((c) => (
            <tr key={`${c.section}/${c.key}`} className={c.unchanged ? "ad__same" : undefined}>
              <td className="num">
                [{c.section}] {c.key}
              </td>
              <td className="num">{c.from}</td>
              <td className="num">{c.unchanged ? "—" : c.to}</td>
              <td className="ad__why">{c.unchanged ? "already correct" : c.because}</td>
            </tr>
          ))}
        </tbody>
      </table>

      {/* A key this version of the game does not have. Shown with what the
          section really contains, because "key not found" is not diagnosable
          and "your file has these eleven things instead" is. */}
      {diff.missing.map((m) => (
        <div className="ad__missing" key={`${m.section}/${m.key}`}>
          <p className="warn warn--hard">
            {m.reason}. This setting is left alone — Team Principal never adds a key your game does
            not already have, because a key it does not read is a change that looks applied and
            is not.
          </p>
          {m.sectionContains.length > 0 && (
            <p className="note num">[{m.section}] has: {m.sectionContains.join(", ")}</p>
          )}
        </div>
      ))}
    </div>
  );
}

/** Which screens a title uses. The adapter's numbers differ completely. */
const SESSIONS: [SessionMode, string][] = [
  [{ kind: "full_span", fit: "letterbox" }, "All screens"],
  [{ kind: "center_only" }, "Centre only"],
];

/**
 * How well this adapter's key names are actually known.
 *
 * Never colour alone: word plus glyph, like every other status in this app.
 */
function ConfidenceBadge({ confidence }: { confidence: AdapterInfo["confidence"] }) {
  return confidence === "verified" ? (
    <span className="tag" title="Key names read from a real file of that game, or its own docs">
      ● VERIFIED
    </span>
  ) : (
    <span
      className="tag tag--warn"
      title="Corroborated across independent second-hand sources, not seen in a shipped file"
    >
      ▲ CORROBORATED
    </span>
  );
}
