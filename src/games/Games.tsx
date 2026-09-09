import { useCallback, useEffect, useMemo, useState } from "react";

import { Section } from "../dashboard/primitives";
import {
  asIpcError,
  gameLibrary,
  setAutoApply,
  type ProfileCard,
} from "../ipc";
import { Preflight } from "./Preflight";
import { ProfileEditor } from "./ProfileEditor";
import "./games.css";

/**
 * The library: every game found, and every profile ever made.
 *
 * Three rules this screen exists to hold:
 *
 * * **A profile appears by itself.** Anything installed gets one on the first
 *   scan, so there is no create step between finding a game and configuring it.
 * * **A profile outlives its install.** Uninstall a game and its card stays,
 *   marked not installed, with everything you set still in it. Losing a tuned
 *   profile because a drive was unplugged would be indefensible.
 * * **Automatic placement is off until it has worked once.** The switch replays
 *   a rectangle you have already watched land, never a fresh calculation — an
 *   automatic placement that is wrong happens every launch and is not obvious
 *   what did it.
 */
export function Games() {
  const [cards, setCards] = useState<ProfileCard[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [racing, setRacing] = useState<ProfileCard | null>(null);
  const [editing, setEditing] = useState<ProfileCard | null>(null);

  const load = useCallback(async () => {
    try {
      setCards(await gameLibrary());
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  // Installed first, then alphabetical. What you can race now is what you came
  // to this screen for.
  const sorted = useMemo(
    () =>
      [...(cards ?? [])].sort(
        (a, b) =>
          Number(b.installed) - Number(a.installed) ||
          a.profile.name.localeCompare(b.profile.name),
      ),
    [cards],
  );

  const installed = sorted.filter((c) => c.installed).length;

  async function toggleAuto(card: ProfileCard, enabled: boolean) {
    try {
      await setAutoApply(card.profile.id, enabled);
      await load();
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    }
  }

  return (
    <div className="devices">
      <Section
        title="Games"
        note={cards === null ? "searching" : `${installed} of ${sorted.length} installed`}
      >
        {error && <p className="warn warn--hard">{error}</p>}

        {cards !== null && sorted.length === 0 && (
          <p className="note">
            Nothing found yet. This reads Steam's library index and Epic's manifests — if your sims
            are installed through another launcher, add them by hand and the profile works the same.
          </p>
        )}

        <div className="lib">
          {sorted.map((card) => (
            <GameCard
              key={card.profile.id}
              card={card}
              onRace={() => setRacing(card)}
              onEdit={() => setEditing(card)}
              onToggleAuto={(v) => void toggleAuto(card, v)}
            />
          ))}
        </div>

        <div className="devices__actions">
          <button className="btn btn--quiet" onClick={() => void load()}>
            Scan again
          </button>
        </div>
      </Section>

      {editing && (
        <ProfileEditor
          key={editing.profile.id}
          profile={editing.profile}
          onSaved={() => void load()}
          onDeleted={() => {
            setEditing(null);
            void load();
          }}
          onClose={() => setEditing(null)}
        />
      )}

      {racing && (
        <Preflight
          key={racing.profile.id}
          profileId={racing.profile.id}
          name={racing.profile.name}
          onClose={() => setRacing(null)}
        />
      )}
    </div>
  );
}

function GameCard({
  card,
  onRace,
  onEdit,
  onToggleAuto,
}: {
  card: ProfileCard;
  onRace: () => void;
  onEdit: () => void;
  onToggleAuto: (enabled: boolean) => void;
}) {
  const { profile, installed, art, installPath, platform } = card;
  const saved = profile.windowPlan.rect.kind === "explicit";

  return (
    <article className={`card${installed ? "" : " card--gone"}`}>
      <div className="card__art">
        {art ? (
          <img src={art} alt="" />
        ) : (
          /* An honest blank rather than a wrong picture. Epic does not cache
             its store art anywhere stable, and this app does not fetch it. */
          <span className="card__monogram" aria-hidden="true">
            {monogram(profile.name)}
          </span>
        )}
        {!installed && <span className="card__gone">NOT INSTALLED</span>}
      </div>

      <div className="card__body">
        <h3 className="card__name">{profile.name}</h3>

        <dl className="card__facts">
          <dt>From</dt>
          <dd>{platform}</dd>
          <dt>Folder</dt>
          <dd className="num card__path" title={installPath ?? undefined}>
            {installPath ?? "not on this machine"}
          </dd>
          <dt>Checks</dt>
          <dd>{summarise(card)}</dd>
        </dl>

        {/* The switch. Disabled until a rectangle has actually been proven,
            with the reason in the label rather than a silent grey control. */}
        <label className={`card__auto${saved ? "" : " card__auto--locked"}`}>
          <input
            type="checkbox"
            checked={profile.windowPlan.autoApply}
            disabled={!saved}
            onChange={(e) => onToggleAuto(e.target.checked)}
          />
          <span className="toggle__track" aria-hidden="true">
            <span className="toggle__knob" />
          </span>
          <span>
            {saved
              ? "Place the window automatically"
              : "Place the window once on the Windows tab, then this switch turns on"}
          </span>
        </label>

        <div className="card__actions">
          <button className="btn" disabled={!installed} onClick={onRace}>
            Let's race
          </button>
          <button className="btn btn--quiet" onClick={onEdit}>
            Edit profile
          </button>
        </div>
      </div>
    </article>
  );
}

/** What this profile will actually check, in one line. */
function summarise({ profile }: ProfileCard): string {
  const required = profile.peripherals.filter((p) => p.necessity === "required").length;
  const optional = profile.peripherals.length - required;
  const parts: string[] = [];
  if (required) parts.push(`${required} required`);
  if (optional) parts.push(`${optional} optional`);
  if (profile.utilities.length) {
    parts.push(
      `${profile.utilities.length} ${profile.utilities.length === 1 ? "utility" : "utilities"}`,
    );
  }
  return parts.length ? parts.join(", ") : "nothing yet";
}

/** Initials, for a game with no cover art. */
function monogram(name: string): string {
  return name
    .split(/\s+/)
    .filter((w) => /[a-z0-9]/i.test(w))
    .slice(0, 2)
    .map((w) => w[0]?.toUpperCase() ?? "")
    .join("");
}
