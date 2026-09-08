import { useCallback, useEffect, useMemo, useState } from "react";

import { Section } from "../dashboard/primitives";
import {
  asIpcError,
  createProfile,
  discoverGames,
  listProfiles,
  type InstalledGameInfo,
  type Profile,
} from "../ipc";
import { Preflight } from "./Preflight";
import { ProfileEditor } from "./ProfileEditor";
import "./games.css";

/**
 * Installed games, found rather than typed, each with the profile that says how
 * to race it.
 *
 * Games are read from Steam's own library index and Epic's manifests. A game
 * whose folder has gone — an interrupted uninstall leaves the manifest behind —
 * is not listed, because a launch that fails for no visible reason is worse
 * than a missing row.
 *
 * A game has no profile until one is made. That is deliberate: the profile is
 * where "these peripherals must be connected, start SimHub first" lives, and a
 * profile invented on the user's behalf would check things nobody asked for.
 */
export function Games() {
  const [games, setGames] = useState<InstalledGameInfo[] | null>(null);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [racing, setRacing] = useState<Profile | null>(null);
  const [editing, setEditing] = useState<Profile | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [found, saved] = await Promise.all([discoverGames(), listProfiles()]);
      setGames(found);
      setProfiles(saved);
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  // A profile belongs to the game it names. Matching by name rather than by
  // install path means a game that moves keeps its profile.
  const byName = useMemo(() => {
    const map = new Map<string, Profile>();
    for (const p of profiles) map.set(p.name.toLowerCase(), p);
    return map;
  }, [profiles]);

  async function makeProfile(game: InstalledGameInfo) {
    setBusy(game.name);
    try {
      const created = await createProfile({
        name: game.name,
        launchUri: game.launchUri,
        installPath: game.installPath,
      });
      setProfiles((prev) => [...prev, created]);
      setEditing(created);
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    } finally {
      setBusy(null);
    }
  }

  // Profiles whose game is not installed on this machine. Shown rather than
  // hidden: a profile that vanished silently after a drive was unplugged would
  // look like the app lost it.
  const homeless = profiles.filter((p) => !games?.some((g) => g.name === p.name));

  return (
    <div className="devices">
      <Section
        title="Installed games"
        note={games === null ? "searching" : `${games.length} found`}
      >
        {error && <p className="warn warn--hard">{error}</p>}

        {games?.length === 0 && !error && (
          <p className="note">
            Nothing found. This reads Steam's library index and Epic's manifests — if your sims are
            installed through another launcher, they will arrive with profile support.
          </p>
        )}

        {games && games.length > 0 && (
          <table className="grid">
            <thead>
              <tr>
                <th>Game</th>
                <th>Launcher</th>
                <th>Profile</th>
                <th>Installed at</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {games.map((g) => {
                const profile = byName.get(g.name.toLowerCase());
                return (
                  <tr key={g.installPath}>
                    <td>
                      {g.name}
                      {g.hasAdapter && <span className="tag">adapter</span>}
                    </td>
                    <td>{g.launcher === "steam" ? "Steam" : "Epic"}</td>
                    <td>{profile ? <ProfileSummary profile={profile} /> : <span className="note">none yet</span>}</td>
                    <td className="num">{g.installPath}</td>
                    <td className="games__actions">
                      {profile ? (
                        <>
                          <button className="btn" onClick={() => setRacing(profile)}>
                            Let's race
                          </button>
                          <button className="btn btn--quiet" onClick={() => setEditing(profile)}>
                            Edit
                          </button>
                        </>
                      ) : (
                        <button
                          className="btn btn--quiet"
                          disabled={busy === g.name}
                          onClick={() => void makeProfile(g)}
                        >
                          {busy === g.name ? "Creating…" : "Create profile"}
                        </button>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}

        {homeless.length > 0 && (
          <p className="note note--pending">
            {homeless.map((p) => p.name).join(", ")}{" "}
            {homeless.length === 1 ? "has a profile" : "have profiles"} but was not found on this
            machine. The profile is kept — reinstall the game and it picks up where it left off.
          </p>
        )}

        <div className="devices__actions">
          <button className="btn btn--quiet" onClick={() => void load()}>
            Search again
          </button>
        </div>
      </Section>

      {editing && (
        <ProfileEditor
          key={editing.id}
          profile={editing}
          onSaved={(saved) => {
            setProfiles((prev) => prev.map((p) => (p.id === saved.id ? saved : p)));
            setEditing(saved);
          }}
          onDeleted={(id) => {
            setProfiles((prev) => prev.filter((p) => p.id !== id));
            setEditing(null);
          }}
          onClose={() => setEditing(null)}
        />
      )}

      {racing && (
        <Preflight
          key={racing.id}
          profileId={racing.id}
          name={racing.name}
          onClose={() => setRacing(null)}
        />
      )}
    </div>
  );
}

/** What this profile will actually check, in one line. */
function ProfileSummary({ profile }: { profile: Profile }) {
  const required = profile.peripherals.filter((p) => p.necessity === "required").length;
  const optional = profile.peripherals.length - required;
  const parts: string[] = [];
  if (required) parts.push(`${required} required`);
  if (optional) parts.push(`${optional} optional`);
  if (profile.utilities.length) {
    parts.push(`${profile.utilities.length} ${profile.utilities.length === 1 ? "utility" : "utilities"}`);
  }
  return <span className="note">{parts.length ? parts.join(", ") : "nothing checked yet"}</span>;
}
