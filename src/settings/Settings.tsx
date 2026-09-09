import { useEffect, useRef, useState } from "react";

import {
  accentPresets as fetchAccentPresets,
  asIpcError,
  createDiagnostics,
  licenceState as fetchLicenceState,
  revealFile,
  type AccentPreset,
  type GlassLevel,
  type LengthUnit,
  type LicenceState,
  type Preferences,
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
