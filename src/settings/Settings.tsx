import { useEffect, useRef, useState } from "react";

import { describeStage, useInstall } from "../updates/useInstall";
import {
  accentPresets as fetchAccentPresets,
  appInfo,
  asIpcError,
  checkForUpdate,
  createDiagnostics,
  licenceState as fetchLicenceState,
  revealFile,
  setRunAtStartup,
  startupState as fetchStartupState,
  type AccentPreset,
  type GlassLevel,
  type LengthUnit,
  type LicenceState,
  type Preferences,
  type StartupState,
  type UpdateInfo,
  type UpdatePolicy,
} from "../ipc";
import "./settings.css";

const MAX_LOGO_BYTES = 512 * 1024;

/**
 * Preferences.
 *
 * Every change saves immediately and re-applies the theme, so the setting and
 * its effect are never out of step — there is no Save button to forget, and no
 * state that exists only in this component.
 */
export function Settings({
  prefs,
  onChange,
}: {
  prefs: Preferences;
  onChange: (next: Preferences) => Promise<void>;
}) {
  const [presets, setPresets] = useState<AccentPreset[]>([]);
  const [error, setError] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    fetchAccentPresets().then(setPresets).catch(() => setPresets([]));
  }, []);

  const update = async (next: Preferences) => {
    try {
      await onChange(next);
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    }
  };

  const pickLogo = async (file: File) => {
    if (file.size > MAX_LOGO_BYTES) {
      setError(
        `That image is ${Math.round(file.size / 1024)} KB. The limit is ${MAX_LOGO_BYTES / 1024} KB — ` +
          `the logo is stored inside preferences.json so it travels with your settings.`,
      );
      return;
    }
    const dataUri = await new Promise<string>((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result));
      reader.onerror = () => reject(reader.error);
      reader.readAsDataURL(file);
    });
    // Rust validates the mime and size again before writing. This check is only
    // so the user gets an answer without a round trip.
    await update({ ...prefs, team: { ...prefs.team, logo: { dataUri, fileName: file.name } } });
  };

  return (
    <div className="settings">
      <Group
        title="Appearance"
        note="Accent drives every highlight in the app, so one colour moves the whole interface."
      >
        <Row label="Accent colour">
          <div className="swatches">
            {presets.map((p) => (
              <button
                key={p.hex}
                className={`swatch${prefs.appearance.accent === p.hex ? " swatch--on" : ""}`}
                style={{ background: p.hex, color: p.foreground }}
                title={`${p.name} — ${p.hex}`}
                aria-label={p.name}
                aria-pressed={prefs.appearance.accent === p.hex}
                onClick={() =>
                  update({ ...prefs, appearance: { ...prefs.appearance, accent: p.hex } })
                }
              >
                {prefs.appearance.accent === p.hex && <span aria-hidden="true">✓</span>}
              </button>
            ))}
            <label className="swatch swatch--custom" title="Custom colour">
              <input
                type="color"
                value={prefs.appearance.accent}
                onChange={(e) =>
                  update({
                    ...prefs,
                    appearance: { ...prefs.appearance, accent: e.target.value.toUpperCase() },
                  })
                }
              />
              <span aria-hidden="true">+</span>
            </label>
          </div>
          <p className="hint num">{prefs.appearance.accent}</p>
        </Row>

        <Row
          label="Glass"
          hint="Translucency looks good and costs contrast. You read this from over 700 mm away in a dim room, so it is a setting rather than a fixed style."
        >
          <Segmented<GlassLevel>
            value={prefs.appearance.glass}
            options={[
              ["off", "Off", "Flat and opaque. Highest contrast."],
              ["subtle", "Subtle", "Glass on the chrome; data stays solid."],
              ["full", "Full", "Glass on data surfaces too."],
            ]}
            onChange={(glass) => update({ ...prefs, appearance: { ...prefs.appearance, glass } })}
          />
        </Row>

        <Row label="Motion" hint="Your Windows reduced-motion setting also applies.">
          <Toggle
            checked={prefs.appearance.reduceMotion}
            label="Reduce motion"
            onChange={(reduceMotion) =>
              update({ ...prefs, appearance: { ...prefs.appearance, reduceMotion } })
            }
          />
        </Row>
      </Group>

      <Group title="Team" note="Shown in the title bar. Yours, not ours.">
        <Row label="Team name">
          <input
            className="text-input"
            value={prefs.team.name ?? ""}
            maxLength={60}
            placeholder="e.g. Camenzuli Racing"
            onChange={(e) => update({ ...prefs, team: { ...prefs.team, name: e.target.value } })}
          />
        </Row>

        <Row label="Logo" hint="PNG, JPEG, SVG or WebP, up to 512 KB.">
          <div className="logo-row">
            <div className="logo-well">
              {prefs.team.logo ? (
                <img src={prefs.team.logo.dataUri} alt="" />
              ) : (
                <span className="logo-well__empty">No logo</span>
              )}
            </div>
            <div className="logo-actions">
              <button className="btn" onClick={() => fileInput.current?.click()}>
                {prefs.team.logo ? "Replace" : "Choose image"}
              </button>
              {prefs.team.logo && (
                <button
                  className="btn btn--quiet"
                  onClick={() => update({ ...prefs, team: { ...prefs.team, logo: null } })}
                >
                  Remove
                </button>
              )}
              {prefs.team.logo && <p className="hint">{prefs.team.logo.fileName}</p>}
            </div>
            <input
              ref={fileInput}
              type="file"
              accept="image/png,image/jpeg,image/svg+xml,image/webp"
              hidden
              onChange={(e) => {
                const file = e.target.files?.[0];
                if (file) void pickLogo(file);
                e.target.value = "";
              }}
            />
          </div>
        </Row>
      </Group>

      <Group
        title="Units"
        note="A display preference only. Measurements are always stored in millimetres, so switching never changes a saved value."
      >
        <Row label="Show lengths in">
          <Segmented<LengthUnit>
            value={prefs.units}
            options={[
              ["mm", "Millimetres", "1193 mm"],
              ["cm", "Centimetres", "119.30 cm"],
              ["inch", "Inches", "46.969 in"],
            ]}
            onChange={(units) => update({ ...prefs, units })}
          />
        </Row>
      </Group>

      <Startup prefs={prefs} onChange={update} />

      <Updates prefs={prefs} onChange={update} />

      <Group title="First-time setup" note={prefs.onboarded ? "done" : "not finished"}>
        <p className="note">
          The guided walk-through: what was detected, and the one measurement nothing on this
          machine can work out for itself.
        </p>
        <div className="settings__actions">
          <button
            className="btn btn--quiet"
            onClick={() => void update({ ...prefs, onboarded: false })}
          >
            Run it again
          </button>
        </div>
      </Group>

      <Diagnostics />

      <Licence />

      {error && (
        <p className="settings__error">
          <span aria-hidden="true">■</span> {error}
        </p>
      )}
    </div>
  );
}

/**
 * Updates.
 *
 * Ask is the default rather than Automatic, and deliberately. This app can be
 * mid-session with a game running and utilities started, and an update that
 * restarts the launcher without being asked would tear that down. Automatic is
 * there because plenty of people would rather never think about it — but it is
 * a choice somebody makes, not one made for them.
 */
function Updates({
  prefs,
  onChange,
}: {
  prefs: Preferences;
  onChange: (next: Preferences) => Promise<void>;
}) {
  const [found, setFound] = useState<UpdateInfo | null>(null);
  const [version, setVersion] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [checkedAt, setCheckedAt] = useState<Date | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const { stage, busy, start } = useInstall();

  // The version this build is, shown before anything is checked. It used to
  // come from the result of a check, so the heading read "v" until you pressed
  // a button — on the one screen where you go to find out what you are running.
  useEffect(() => {
    appInfo()
      .then((info) => setVersion(info.version))
      .catch(() => setVersion(null));
  }, []);

  async function check() {
    setChecking(true);
    try {
      setFound(await checkForUpdate());
      setCheckedAt(new Date());
      setProblem(null);
    } catch (e) {
      setProblem(asIpcError(e).message);
    } finally {
      setChecking(false);
    }
  }

  return (
    <Group title="Updates" note={version ? `v${version}` : ""}>
      <Row label="When a new version is available">
        <Segmented<UpdatePolicy>
          value={prefs.updates}
          options={[
            ["ask", "Tell me", "install it when you say so"],
            ["automatic", "Install it", "and restart, unless a race is running"],
            ["never", "Do nothing", "no checking at all"],
          ]}
          onChange={(updates) => void onChange({ ...prefs, updates })}
        />
      </Row>

      {/* The build you are running, stated plainly. During testing the only
          question that matters when something looks wrong is whether the fix
          is even in the build — and that should never take a conversation. */}
      <Row label="This build">
        <span className="num settings__build">{version ? `v${version}` : "…"}</span>
      </Row>

      <div className="settings__actions">
        <button
          className="btn btn--quiet"
          disabled={checking || busy}
          onClick={() => void check()}
        >
          {checking ? "Checking…" : "Check now"}
        </button>
        {found?.outcome.kind === "available" && !busy && (
          <button className="btn" onClick={start}>
            Install {found.outcome.version} and restart
          </button>
        )}
        {checkedAt && !busy && (
          <span className="hint">last checked {checkedAt.toLocaleTimeString()}</span>
        )}
      </div>

      {/* The install reports itself, in the same words as the strip at the top
          of the app — including a refusal, which this screen used to discard
          entirely by throwing it into a click handler. */}
      {stage && (
        <p className={stage.kind === "failed" ? "settings__error" : "note"}>
          {stage.kind === "failed" && <span aria-hidden="true">■</span>} {describeStage(stage)}
        </p>
      )}

      {/* Each outcome says which one it is. None of them is dressed as another:
          a repository with nothing released in it is not a network fault, and
          neither of those is "up to date". */}
      {found && !stage && <Outcome info={found} />}

      {problem && (
        <p className="settings__error">
          <span aria-hidden="true">■</span> {problem}
        </p>
      )}

      <p className="hint">
        An update replaces the program only. Rigs, game profiles, peripherals, snapshots and
        backups live in your app data folder and are not touched — which is why updating keeps
        everything, and why uninstalling does too.
      </p>
    </Group>
  );
}

/** What a check found, in the words that fit the case. */
function Outcome({ info }: { info: UpdateInfo }) {
  const { outcome } = info;

  switch (outcome.kind) {
    case "no_key":
      return (
        <p className="note note--fail">
          This build has no update signing key compiled in, so it cannot check — a signed update
          could not be verified, and installing one unverified is not something this app will do.
          That is a property of how it was built rather than something to fix here; see
          docs/RELEASING.md.
        </p>
      );

    case "nothing_published":
      return (
        <p className="note">
          Nothing has been published yet. The update service is reachable and this build can
          verify what it finds there — there is simply no release to compare against, which is
          what an unreleased product looks like.
        </p>
      );

    case "up_to_date":
      return (
        <p className="note">
          Up to date. Nothing newer than <span className="num">v{info.currentVersion}</span> has
          been published.
        </p>
      );

    case "available":
      return outcome.notes ? <p className="note">{outcome.notes}</p> : null;

    case "unreachable":
      return (
        <p className="note">
          Couldn't reach the update service: {outcome.message}. Usually this machine being
          offline, which is the normal state of a rig in a garage — nothing is wrong with the
          app, and the next check will find it.
        </p>
      );
  }
}

/**
 * Starting with Windows, and starting out of the way.
 *
 * Two separate switches because they answer different questions. Most people
 * want the app there when they sit down without it taking the screen, and want
 * it front and centre when they open it themselves.
 *
 * The startup switch reflects the registry, re-read rather than remembered:
 * Windows disables startup entries through Task Manager and through its own
 * heuristics without telling the app, and a switch showing On over an entry
 * Windows turned off is a lie the user finds out about on the morning it
 * matters.
 */
function Startup({
  prefs,
  onChange,
}: {
  prefs: Preferences;
  onChange: (next: Preferences) => Promise<void>;
}) {
  const [state, setState] = useState<StartupState | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    fetchStartupState()
      .then(setState)
      .catch(() => setState(null));
  }, []);

  async function toggle(enabled: boolean) {
    try {
      setState(await setRunAtStartup(enabled));
      await onChange({ ...prefs, runAtStartup: enabled });
      setProblem(null);
    } catch (e) {
      setProblem(asIpcError(e).message);
    }
  }

  return (
    <Group title="Starting up">
      <Row label="With Windows" hint="starts minimised, so it is there without being in the way">
        <Toggle
          checked={state?.enabled ?? false}
          label={state?.enabled ? "On" : "Off"}
          onChange={(v) => void toggle(v)}
        />
      </Row>

      {state?.stale && (
        <p className="note note--fail">
          Windows has a startup entry for Team Principal that points somewhere else — the app has
          been moved or reinstalled since. Turn this off and on again to repair it.
        </p>
      )}

      <Row label="Always minimised" hint="even when you open it yourself">
        <Toggle
          checked={prefs.startMinimised}
          label={prefs.startMinimised ? "On" : "Off"}
          onChange={(v) => void onChange({ ...prefs, startMinimised: v })}
        />
      </Row>

      {problem && (
        <p className="settings__error">
          <span aria-hidden="true">■</span> {problem}
        </p>
      )}
    </Group>
  );
}

/**
 * The support bundle.
 *
 * The one screen in this app whose job is to make a bad day recoverable. It
 * says what goes in the file *before* the file is made, because a bundle
 * somebody is unsure about is a bundle they do not send — and then nobody can
 * help them.
 */
function Diagnostics() {
  const [path, setPath] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  async function build() {
    setBusy(true);
    try {
      setPath(await createDiagnostics());
      setProblem(null);
    } catch (e) {
      setProblem(asIpcError(e).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <Group title="Diagnostics" note="one file, for when something goes wrong">
      <p className="note">
        Collects the app's logs and everything it has detected — monitors, peripherals, your rig,
        your profiles — into a single zip you can send. Your Windows account name is replaced
        throughout, and your game config files are not included. The zip contains a plain-text
        README listing exactly what is in it.
      </p>

      <div className="settings__actions">
        <button className="btn" disabled={busy} onClick={() => void build()}>
          {busy ? "Collecting…" : "Create a diagnostics bundle"}
        </button>
        {path && (
          <button className="btn btn--quiet" onClick={() => void revealFile(path)}>
            Show me the file
          </button>
        )}
      </div>

      {path && <p className="note num settings__path">{path}</p>}
      {problem && (
        <p className="settings__error">
          <span aria-hidden="true">■</span> {problem}
        </p>
      )}
    </Group>
  );
}

/**
 * Licensing.
 *
 * There is no licence check in this build and the panel says so rather than
 * showing a reassuring green tick over nothing. When a vendor is chosen, one
 * file in Rust changes and this panel starts telling the truth about a real
 * entitlement without any of it passing through the frontend.
 */
function Licence() {
  const [state, setState] = useState<LicenceState | null>(null);

  useEffect(() => {
    fetchLicenceState()
      .then(setState)
      .catch(() => setState(null));
  }, []);

  if (!state) return null;

  return (
    <Group title="Licence" note={state.tier === "licensed" ? "active" : "unlicensed"}>
      {state.message && <p className="note">{state.message}</p>}
      {state.reference && (
        <p className="note num">
          Key {state.reference}
          {state.validUntil ? ` · valid until ${state.validUntil}` : " · perpetual"}
        </p>
      )}
      {state.offline && (
        <p className="note note--fail">
          Running on a cached licence — the service could not be reached. Everything still works;
          this is here so it is not a surprise later.
        </p>
      )}
    </Group>
  );
}

function Group(props: { title: string; note?: string; children: React.ReactNode }) {
  return (
    <section className="group glass">
      <header className="group__head">
        <h2 className="group__title">{props.title}</h2>
        {props.note && <p className="group__note">{props.note}</p>}
      </header>
      <div className="group__body">{props.children}</div>
    </section>
  );
}

function Row(props: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="row">
      <div className="row__label">
        <span>{props.label}</span>
        {props.hint && <p className="hint">{props.hint}</p>}
      </div>
      <div className="row__control">{props.children}</div>
    </div>
  );
}

function Segmented<T extends string>(props: {
  value: T;
  options: [T, string, string][];
  onChange: (v: T) => void;
}) {
  return (
    <div className="segmented" role="radiogroup">
      {props.options.map(([value, label, description]) => (
        <button
          key={value}
          role="radio"
          aria-checked={props.value === value}
          className={`segmented__item${props.value === value ? " segmented__item--on" : ""}`}
          onClick={() => props.onChange(value)}
        >
          <span className="segmented__label">{label}</span>
          <span className="segmented__desc">{description}</span>
        </button>
      ))}
    </div>
  );
}

function Toggle(props: { checked: boolean; label: string; onChange: (v: boolean) => void }) {
  return (
    <label className="toggle">
      <input
        type="checkbox"
        checked={props.checked}
        onChange={(e) => props.onChange(e.target.checked)}
      />
      <span className="toggle__track" aria-hidden="true">
        <span className="toggle__knob" />
      </span>
      <span>{props.label}</span>
    </label>
  );
}
