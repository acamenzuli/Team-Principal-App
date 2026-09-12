import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { Section } from "../dashboard/primitives";
import {
  addGame,
  asIpcError,
  captureWindow,
  gameLibrary,
  setAutoApply,
  setGameArt,
  type CaptureResult,
  type LibraryMode,
  type LibraryView,
  type OpenWindow,
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
export function Games({
  view,
  onView,
}: {
  view: LibraryView;
  onView: (next: LibraryView) => void;
}) {
  const [cards, setCards] = useState<ProfileCard[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [racing, setRacing] = useState<ProfileCard | null>(null);
  const [editing, setEditing] = useState<ProfileCard | null>(null);
  const [captured, setCaptured] = useState<CaptureResult | null>(null);
  const [capturingFor, setCapturingFor] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [scan, setScan] = useState<string | null>(null);
  const [scanning, setScanning] = useState(false);

  const load = useCallback(async () => {
    try {
      setCards(await gameLibrary());
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    }
  }, []);

  /**
   * Scan, and say what came of it.
   *
   * A scan that finds exactly what it found last time renders an identical
   * list, which is indistinguishable from a button that does nothing — and
   * that is precisely what it looked like. So this reports the outcome:
   * what was added, what went away, or that nothing changed.
   */
  const rescan = useCallback(async () => {
    setScanning(true);
    const before = cards ?? [];
    try {
      const after = await gameLibrary();
      setCards(after);
      setError(null);

      const ids = new Set(before.map((c) => c.profile.id));
      const added = after.filter((c) => !ids.has(c.profile.id));
      const installedNow = after.filter((c) => c.installed).length;
      const installedBefore = before.filter((c) => c.installed).length;

      if (added.length > 0) {
        setScan(
          `Found ${added.length} new ${added.length === 1 ? "game" : "games"}: ` +
            added.map((c) => c.profile.name).join(", "),
        );
      } else if (installedNow !== installedBefore) {
        setScan(`${installedNow} installed now, ${installedBefore} before.`);
      } else {
        setScan(
          `Nothing new — ${after.length} ${after.length === 1 ? "profile" : "profiles"}, ` +
            `${installedNow} installed. Checked at ${new Date().toLocaleTimeString()}.`,
        );
      }
    } catch (e) {
      setError(asIpcError(e).message);
      setScan(null);
    } finally {
      setScanning(false);
    }
  }, [cards]);

  async function chooseArt(card: ProfileCard, dataUri: string | null) {
    try {
      const updated = await setGameArt(card.profile.id, dataUri);
      setCards((current) =>
        (current ?? []).map((c) => (c.profile.id === updated.profile.id ? updated : c)),
      );
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    }
  }

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

  /**
   * Copy a running game's screen setup.
   *
   * The rig is very likely already set up — with SRWE, Resize Raccoon or by
   * hand — and that setup took real effort. Reading the window beats asking
   * somebody to describe, in numbers, something they can already see.
   */
  async function copyLayout(card: ProfileCard, hwnd?: string) {
    setCapturingFor(card.profile.id);
    try {
      const result = await captureWindow(card.profile.id, hwnd);
      setCaptured(result);
      if (result.kind === "captured") await load();
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    } finally {
      setCapturingFor(null);
    }
  }

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

        {/* How you look at your own library is your business: a wall of
            covers at 4K and a wall at 1080p want different answers, and thirty
            titles want a list. */}
        <div className="lib__view">
          <div className="seg" role="group" aria-label="How to show the library">
            {(["grid", "list"] as LibraryMode[]).map((mode) => (
              <button
                key={mode}
                className={`seg__btn${view.mode === mode ? " seg__btn--on" : ""}`}
                aria-pressed={view.mode === mode}
                onClick={() => onView({ ...view, mode })}
              >
                {mode === "grid" ? "Covers" : "List"}
              </button>
            ))}
          </div>

          <label className="lib__size">
            <span className="hint">Size</span>
            <input
              type="range"
              min={ART_MIN}
              max={ART_MAX}
              step={4}
              value={view.artPx}
              aria-label="Cover size"
              onChange={(e) => onView({ ...view, artPx: Number(e.target.value) })}
            />
            <span className="num hint">{view.artPx}px</span>
          </label>
        </div>

        <div
          className={`lib${view.mode === "list" ? " lib--list" : ""}`}
          style={{
            ["--art" as string]: `${view.artPx}px`,
            // A row wants a thumbnail, not a cover: the same slider, scaled
            // down and clamped, so dragging it still does something sensible
            // in both views rather than producing 200-pixel-tall rows.
            ["--thumb" as string]: `${Math.min(96, Math.max(36, Math.round(view.artPx / 2.5)))}px`,
          }}
        >
          {view.mode === "list" &&
            sorted.map((card) => (
              <GameRow
                key={card.profile.id}
                card={card}
                onRace={() => setRacing(card)}
                onEdit={() => setEditing(card)}
                onToggleAuto={(v) => void toggleAuto(card, v)}
                onCopyLayout={() => void copyLayout(card)}
                onArt={(dataUri) => void chooseArt(card, dataUri)}
                capturing={capturingFor === card.profile.id}
              />
            ))}

          {view.mode === "grid" &&
            sorted.map((card) => (
            <GameCard
              key={card.profile.id}
              card={card}
              onRace={() => setRacing(card)}
              onEdit={() => setEditing(card)}
              onToggleAuto={(v) => void toggleAuto(card, v)}
              onCopyLayout={() => void copyLayout(card)}
              onArt={(dataUri) => void chooseArt(card, dataUri)}
              capturing={capturingFor === card.profile.id}
            />
          ))}
        </div>

        <div className="devices__actions">
          <button className="btn btn--quiet" disabled={scanning} onClick={() => void rescan()}>
            {scanning ? "Scanning…" : "Scan again"}
          </button>
          {/* Steam and Epic are found; everything else is not. Plenty of sims
              install outside both, and without this they are simply absent. */}
          <button className="btn btn--quiet" onClick={() => setAdding(true)}>
            Add a game by hand
          </button>
        </div>

        {scan && <p className="note">{scan}</p>}
      </Section>

      {adding && (
        <AddGame
          onAdded={() => {
            setAdding(false);
            void load();
          }}
          onClose={() => setAdding(false)}
        />
      )}

      {captured && (
        <CaptureOutcome
          result={captured}
          onChoose={(hwnd) => {
            const card = sorted.find((c) => c.profile.id === capturingFor);
            if (card) void copyLayout(card, hwnd);
          }}
          onClose={() => setCaptured(null)}
        />
      )}

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

/**
 * One game, as a row.
 *
 * Not the cover card squeezed flat: a row is a different job. Every value sits
 * in the same column on every row, so the eye runs down "which of these is
 * installed" or "which have a saved layout" instead of hunting through
 * twenty-three repeated blocks. Columns are fixed or fractional — never sized
 * by their contents — because a column that resizes per row is not a column.
 */
function GameRow({
  card,
  onRace,
  onEdit,
  onToggleAuto,
  onCopyLayout,
  onArt,
  capturing,
}: {
  card: ProfileCard;
  onRace: () => void;
  onEdit: () => void;
  onToggleAuto: (enabled: boolean) => void;
  onCopyLayout: () => void;
  onArt: (dataUri: string | null) => void;
  capturing: boolean;
}) {
  const { profile, installed, art, installPath, platform } = card;
  const saved = profile.windowPlan.rect.kind === "explicit";
  const artInput = useRef<HTMLInputElement>(null);

  return (
    <article className={`row${installed ? "" : " row--gone"}`}>
      <button
        className="row__art"
        title={art ? "Change the picture" : "Add a picture"}
        onClick={() => artInput.current?.click()}
      >
        {art ? (
          <img src={art} alt="" />
        ) : (
          <span className="row__monogram" aria-hidden="true">
            {monogram(profile.name)}
          </span>
        )}
      </button>
      <input
        ref={artInput}
        className="visually-hidden"
        type="file"
        accept="image/png,image/jpeg,image/webp"
        onChange={(e) => {
          const file = e.target.files?.[0];
          e.target.value = "";
          if (!file) return;
          const reader = new FileReader();
          reader.onload = () => onArt(String(reader.result));
          reader.readAsDataURL(file);
        }}
      />

      <div className="row__id">
        <h3 className="row__name">{profile.name}</h3>
        <span className="row__where num" title={installPath ?? undefined}>
          {installed ? (installPath ?? platform) : "not on this machine"}
        </span>
      </div>

      <span className="row__from">{platform}</span>

      <span className="row__checks">{summarise(card)}</span>

      {/* The same switch as the card, and disabled for the same reason: a
          rectangle has to have been seen working before it can be replayed. */}
      <label
        className={`row__auto${saved ? "" : " row__auto--locked"}`}
        title={
          saved
            ? "Place the window automatically"
            : "Run the game how you like it, press Copy current layout, and this switches itself on"
        }
      >
        <input
          type="checkbox"
          checked={profile.windowPlan.autoApply}
          disabled={!saved}
          onChange={(e) => onToggleAuto(e.target.checked)}
        />
        <span className="toggle__track" aria-hidden="true">
          <span className="toggle__knob" />
        </span>
        <span className="row__autolabel">Auto</span>
      </label>

      <div className="row__actions">
        <button className="btn btn--tiny" disabled={!installed} onClick={onRace}>
          Let's race
        </button>
        <button
          className="btn btn--tiny btn--quiet"
          disabled={capturing}
          title="Run the game in the layout you want, then press this"
          onClick={onCopyLayout}
        >
          {capturing ? "Reading…" : "Copy layout"}
        </button>
        <button className="btn btn--tiny btn--quiet" onClick={onEdit}>
          Edit
        </button>
      </div>
    </article>
  );
}

function GameCard({
  card,
  onRace,
  onEdit,
  onToggleAuto,
  onCopyLayout,
  onArt,
  capturing,
}: {
  card: ProfileCard;
  onRace: () => void;
  onEdit: () => void;
  onToggleAuto: (enabled: boolean) => void;
  onCopyLayout: () => void;
  onArt: (dataUri: string | null) => void;
  capturing: boolean;
}) {
  const { profile, installed, art, installPath, platform } = card;
  const saved = profile.windowPlan.rect.kind === "explicit";
  const artInput = useRef<HTMLInputElement>(null);

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

        {/* Art is found in Steam's local cache, so a game from anywhere else
            has none. Rather than guess at a picture or fetch one over the
            network, you point at one. */}
        <div className="card__artpick">
          <button className="btn btn--tiny" onClick={() => artInput.current?.click()}>
            {art ? "Change art" : "Add art"}
          </button>
          {art && (
            <button className="btn btn--tiny btn--quiet" onClick={() => onArt(null)}>
              Remove
            </button>
          )}
        </div>
        <input
          ref={artInput}
          className="visually-hidden"
          type="file"
          accept="image/png,image/jpeg,image/webp"
          onChange={(e) => {
            const file = e.target.files?.[0];
            // Cleared straight away so picking the same file twice still
            // fires — a change event does not repeat for an identical value.
            e.target.value = "";
            if (!file) return;
            const reader = new FileReader();
            reader.onload = () => onArt(String(reader.result));
            reader.readAsDataURL(file);
          }}
        />
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
              : "Run the game how you like it, press Copy current layout, and this switches itself on"}
          </span>
        </label>

        <div className="card__actions">
          <button className="btn" disabled={!installed} onClick={onRace}>
            Let's race
          </button>
          {/* For a rig that is already set up. Reading the window beats asking
              somebody to type out numbers they can already see on screen. */}
          <button
            className="btn btn--quiet"
            disabled={capturing}
            title="Run the game in the layout you want, then press this"
            onClick={onCopyLayout}
          >
            {capturing ? "Reading…" : "Copy current layout"}
          </button>
          <button className="btn btn--quiet" onClick={onEdit}>
            Edit
          </button>
        </div>
      </div>
    </article>
  );
}

/**
 * Adding a game the launchers do not know about.
 *
 * Two fields, because two is all it needs: a name, and the executable. The
 * executable is the valuable half — naming it up front gives the launcher
 * something to watch for and the window matcher something to find, which is
 * exactly what a Steam profile lacks until somebody copies a layout.
 */
function AddGame({ onAdded, onClose }: { onAdded: () => void; onClose: () => void }) {
  const [name, setName] = useState("");
  const [path, setPath] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function add() {
    setBusy(true);
    try {
      await addGame(name, path);
      onAdded();
    } catch (e) {
      setProblem(asIpcError(e).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="cap">
      <div className="cap__panel glass">
        <h3 className="cap__title">Add a game</h3>
        <p className="note">
          For anything Steam and Epic do not list — iRacing, a title bought direct, an older sim
          with its own installer.
        </p>

        <div className="field">
          <label className="field__label">Name</label>
          <input
            className="text-input"
            value={name}
            placeholder="rFactor 2"
            onChange={(e) => setName(e.target.value)}
          />
        </div>

        <div className="field">
          <label className="field__label">Program</label>
          <input
            className="text-input num"
            value={path}
            placeholder="C:\\Games\\rFactor2\\Bin64\\rFactor2.exe"
            onChange={(e) => setPath(e.target.value)}
          />
          <span className="hint">
            The game's own .exe, not its launcher — that is the window this app will place.
          </span>
        </div>

        {problem && <p className="warn warn--hard">{problem}</p>}

        <div className="cap__actions">
          <button className="btn btn--quiet" onClick={onClose}>
            Cancel
          </button>
          <button className="btn" disabled={busy || !name.trim() || !path.trim()} onClick={() => void add()}>
            {busy ? "Adding…" : "Add"}
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * What copying the layout produced.
 *
 * Four outcomes, and each says something different. Reporting "could not find
 * the game" as a generic failure would send somebody looking for a bug in the
 * app when the answer is that the game is not running.
 */
function CaptureOutcome({
  result,
  onChoose,
  onClose,
}: {
  result: CaptureResult;
  onChoose: (hwnd: string) => void;
  onClose: () => void;
}) {
  return (
    <div className="cap">
      <div className="cap__panel glass">
        {result.kind === "captured" && (
          <>
            <h3 className="cap__title">Copied</h3>
            <p>
              <strong className="num">
                {result.layout.rect.width} × {result.layout.rect.height}
              </strong>{" "}
              at <span className="num">{result.layout.rect.x},{result.layout.rect.y}</span>,{" "}
              {result.layout.borderless ? "borderless" : "with its frame"}
              {result.layout.alwaysOnTop && ", always on top"}.
            </p>
            {result.layout.covers.length > 0 && (
              <p className="note">
                {result.layout.coversExactly
                  ? `Covers ${result.layout.covers.join(", ")} exactly.`
                  : `Overlaps ${result.layout.covers.join(", ")}, but does not line up with them — it will still be saved and replayed as it is.`}
              </p>
            )}
            {result.layout.exeName && (
              <p className="note">
                Learned that it runs as <span className="num">{result.layout.exeName}</span>, which
                is what lets a launch find this window at all — a Steam or Epic profile has no way
                to know that on its own.
              </p>
            )}
            <p className="note">
              Automatic placement is now on for this game. Every launch puts the window back here.
            </p>
          </>
        )}

        {result.kind === "choose" && (
          <>
            <h3 className="cap__title">Which one is the game?</h3>
            <p className="note">
              More than one window could be it. Picking wrong would save the wrong geometry onto
              this profile, so this asks rather than guesses.
            </p>
            <ul className="cap__list">
              {result.windows.map((w: OpenWindow) => (
                <li key={w.candidate.hwnd}>
                  <button className="cap__pick" onClick={() => onChoose(String(w.candidate.hwnd))}>
                    <span>{w.candidate.title || "(untitled)"}</span>
                    <span className="num">
                      {w.candidate.exeName ?? "unknown"} · {w.candidate.rect.width}×
                      {w.candidate.rect.height}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </>
        )}

        {result.kind === "not_running" && (
          <>
            <h3 className="cap__title">Not running</h3>
            <p>
              This profile expects <span className="num">{result.exe}</span>, and nothing by that
              name is running. Start the game, get it looking how you want, then press Copy current
              layout again.
            </p>
          </>
        )}

        {result.kind === "nothing_found" && (
          <>
            <h3 className="cap__title">No game window found</h3>
            <p>
              Nothing open looks like a game window. Start the game first — a splash screen is
              deliberately ignored, so wait until you are actually on track.
            </p>
          </>
        )}

        <div className="cap__actions">
          <button className="btn" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
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

/**
 * The slider's range, matching the clamp the backend applies on save.
 *
 * Small enough to fit thirty titles on a screen, large enough to recognise a
 * cover from a driving position — which is further away than a desk.
 */
const ART_MIN = 56;
const ART_MAX = 320;
