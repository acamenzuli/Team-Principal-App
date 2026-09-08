import { useCallback, useEffect, useState } from "react";

import { Section } from "../dashboard/primitives";
import { asIpcError, discoverGames, type InstalledGameInfo } from "../ipc";

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
                </tr>
              ))}
            </tbody>
          </table>
        )}

        <div className="devices__actions">
          <button className="btn btn--quiet" onClick={() => void load()}>
            Search again
          </button>
        </div>

        <p className="devices__scope">
          <strong>Milestone 7, part one.</strong> Finding games, the readiness gates, and the
          dependency scheduler are done and tested. Running a profile end to end — starting
          utilities, waiting on gates, launching, and tearing down through a Job Object — is the
          rest of this milestone.
        </p>
      </Section>
    </div>
  );
}
