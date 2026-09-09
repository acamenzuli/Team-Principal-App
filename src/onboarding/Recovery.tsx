import { useEffect, useState } from "react";

import {
  asIpcError,
  dismissPendingSession,
  pendingSession,
  recoverSession,
  type PendingSession,
} from "../ipc";
import "./onboarding.css";

/**
 * "The last session did not finish."
 *
 * A session changes things outside this app — game config files, sometimes the
 * desktop. Teardown puts them back, but only if something is still running to
 * do it. A crash, a power cut or Task Manager leaves nothing.
 *
 * So a marker is written before anything changes and cleared only after
 * teardown finishes. Finding one here means the last session was interrupted,
 * and this is the offer to undo what it did. Showing it at startup rather than
 * burying it in a menu is the point: the person affected is the one who just
 * opened the app, and they will not go looking for a screen they do not know
 * exists.
 */
export function Recovery() {
  const [session, setSession] = useState<PendingSession | null>(null);
  const [result, setResult] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    pendingSession()
      .then(setSession)
      .catch(() => setSession(null));
  }, []);

  if (!session) return null;

  async function recover() {
    setBusy(true);
    try {
      setResult(await recoverSession());
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    } finally {
      setBusy(false);
    }
  }

  async function dismiss() {
    await dismissPendingSession();
    setSession(null);
  }

  return (
    <div className="ob">
      <div className="ob__panel glass">
        <header className="ob__head">
          <h1 className="ob__title">Last session did not finish</h1>
        </header>

        <div className="ob__body">
          <p>
            <strong>{session.profileName}</strong> was started on{" "}
            <span className="num">{session.startedAt}</span> and Team Principal closed before it
            could tidy up.
          </p>

          {session.hasConfigBackup ? (
            <p>
              Its game settings were changed. The originals are saved, and putting them back takes
              one click.
            </p>
          ) : (
            /* Worth saying plainly rather than offering a recovery that would
               do nothing and look like it failed. */
            <p className="note">
              Nothing had been changed yet when it stopped, so there is nothing to undo. This
              notice is only here so the interruption is not a silent one.
            </p>
          )}

          {result && (
            <ul className="ob__list">
              {result.length === 0 ? (
                <li>
                  <span>Nothing needed putting back.</span>
                </li>
              ) : (
                result.map((line) => (
                  <li key={line}>
                    <span>{line}</span>
                  </li>
                ))
              )}
            </ul>
          )}
        </div>

        {error && <p className="warn warn--hard">{error}</p>}

        <footer className="ob__foot">
          <button className="btn btn--quiet" onClick={() => void dismiss()}>
            {result ? "Close" : "Leave it as it is"}
          </button>
          <span className="app__spacer" />
          {!result && session.hasConfigBackup && (
            <button className="btn" disabled={busy} onClick={() => void recover()}>
              {busy ? "Putting it back…" : "Put my settings back"}
            </button>
          )}
        </footer>
      </div>
    </div>
  );
}
