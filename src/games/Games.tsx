import { useCallback, useEffect, useState } from "react";

import { Section } from "../dashboard/primitives";
import { asIpcError, discoverGames, type InstalledGameInfo } from "../ipc";
import { Preflight } from "./Preflight";

/**
 * Installed games, found rather than typed.
 *
 * Read from Steam's own library index and Epic's manifests. A game whose folder
 * has gone — an interrupted uninstall leaves the manifest behind — is not
 * listed, because a launch that fails for no visible reason is worse than a
 * missing row.
 */
export function Games() {
  const [games, setGames] = useState<InstalledGameInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [racing, setRacing] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setGames(await discoverGames());
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <div className="devices">
      <Section
        title="Installed games"
        note={games === null ? "searching" : `${games.length} found`}
      >
        {error && <p className="warn warn--hard">{error}</p>}

        {games?.length === 0 && !error && (
          <p className="note">
            Nothing found. This reads Steam's library index and Epic's manifests — if your sims
            are installed through another launcher, they will arrive with profile support.
          </p>
        )}

        {games && games.length > 0 && (
          <table className="grid">
            <thead>
              <tr>
                <th>Game</th>
                <th>Launcher</th>
                <th>Installed at</th>
                <th>Starts with</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {games.map((g) => (
                <tr key={g.installPath}>
                  <td>
                    {g.name}
                    {g.hasAdapter && <span className="tag">adapter</span>}
                  </td>
                  <td>{g.launcher === "steam" ? "Steam" : "Epic"}</td>
                  <td className="num">{g.installPath}</td>
                  <td className="num">{g.launchUri}</td>
                  <td>
                    <button className="btn" onClick={() => setRacing(g.name)}>
                      Let's race
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}

        {racing && <Preflight key={racing} gameName={racing} onClose={() => setRacing(null)} />}

        <div className="devices__actions">
          <button className="btn btn--quiet" onClick={() => void load()}>
            Search again
          </button>
        </div>

        <p className="devices__scope">
          <strong>Milestone 7.</strong> The scheduler runs, gates are checked, and the session's
          utilities go into a Job Object so nothing outlives a crash. The profile that says which
          utilities to start and which peripherals are required is next, along with the full
          "Let's race" screen.
        </p>
      </Section>
    </div>
  );
}
