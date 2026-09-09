import { useEffect, useState } from "react";

import { LengthField } from "../screen/LengthField";
import {
  asIpcError,
  currentRig,
  detectRig,
  listDevices,
  listMonitors,
  saveRig,
  solveRig,
  type DetectedDevice,
  type LengthUnit,
  type MonitorInfo,
  type RigModel,
  type RigSolutionInfo,
} from "../ipc";
import "./onboarding.css";

/**
 * First run.
 *
 * The wizard's job is to leave a *working rig model* behind, not to tour the
 * app. So it does everything that can be detected and asks only for what
 * cannot:
 *
 * * Panel sizes come from EDID where the monitor reports them honestly, and the
 *   screens where it does not are named, because a guessed size silently
 *   poisons every number derived from it.
 * * Seating distance is typed, because nothing on the machine knows it and
 *   every angle in the app depends on it. It is the one number worth
 *   interrupting somebody for.
 *
 * Skippable at every step. A wizard that traps somebody on the first screen of
 * a product they have just paid for is worse than no wizard.
 */
export function FirstRun({ unit, onDone }: { unit: LengthUnit; onDone: () => void }) {
  const [step, setStep] = useState(0);
  const [monitors, setMonitors] = useState<MonitorInfo[] | null>(null);
  const [devices, setDevices] = useState<DetectedDevice[]>([]);
  const [rig, setRig] = useState<RigModel | null>(null);
  const [solution, setSolution] = useState<RigSolutionInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    Promise.all([listMonitors(), listDevices(), currentRig()])
      .then(([m, d, r]) => {
        setMonitors(m);
        setDevices(d);
        setRig(r);
      })
      .catch((e) => setError(asIpcError(e).message));
  }, []);

  // Live numbers as the seating distance changes, so the effect of the one
  // typed value is visible while typing it rather than three screens later.
  useEffect(() => {
    if (!rig) return;
    let live = true;
    solveRig(rig, { kind: "full_span", fit: "letterbox" })
      .then((s) => live && setSolution(s))
      .catch(() => live && setSolution(null));
    return () => {
      live = false;
    };
  }, [rig]);

  async function redetect() {
    try {
      setRig(await detectRig(rig));
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    }
  }

  async function finish() {
    setSaving(true);
    try {
      if (rig) await saveRig(rig);
      onDone();
    } catch (e) {
      setError(asIpcError(e).message);
      setSaving(false);
    }
  }

  // Panels whose physical size EDID did not report. Named rather than counted:
  // "one screen needs measuring" is actionable, "some screens" is not.
  const unmeasured = (rig?.screens ?? []).filter(
    (s) => s.panel.visibleWidth.source !== "edid",
  );

  const steps = [
    {
      title: "Team Principal",
      body: (
        <>
          <p>
            One button turns “I want to race” into a checked rig: peripherals confirmed, utilities
            started, display set, game settings written, window placed.
          </p>
          <p>
            It works by measuring your rig <em>once</em>. Every game's resolution, field of view,
            triple-screen projection and bezel compensation is worked out from those measurements
            rather than typed in per title.
          </p>
          <p className="note">
            It never injects into a game, reads its memory, or hooks its renderer. Windows are
            moved with the same documented Win32 calls any tool uses, and game settings are changed
            by editing the game's own config files — backed up first, and shown to you as a diff
            before anything is written.
          </p>
        </>
      ),
    },
    {
      title: "Your screens",
      body: (
        <>
          {monitors === null ? (
            <p className="note note--pending">Looking…</p>
          ) : (
            <>
              <p>
                Found <strong>{monitors.length}</strong>{" "}
                {monitors.length === 1 ? "screen" : "screens"}.
              </p>
              <ul className="ob__list">
                {monitors.map((m) => (
                  <li key={m.devicePath}>
                    <span>{m.friendlyName}</span>
                    <span className="num">
                      {m.currentMode.resolution.width}×{m.currentMode.resolution.height}
                      {m.physicalSize
                        ? ` · ${Math.round(m.physicalSize.width)}×${Math.round(
                            m.physicalSize.height,
                          )} mm`
                        : " · size not reported"}
                    </span>
                  </li>
                ))}
              </ul>

              {unmeasured.length > 0 ? (
                <p className="note note--fail">
                  {unmeasured.map((s) => s.role).join(", ")} did not report a usable physical size,
                  so you will need to measure {unmeasured.length === 1 ? "it" : "them"} with a tape
                  on the Screen Setup tab. A guessed size quietly makes every angle wrong, so the
                  app will not invent one.
                </p>
              ) : (
                <p className="note">
                  Every panel reported its own physical size. Worth checking against a tape measure
                  once — some monitors round to the nearest centimetre.
                </p>
              )}

              <button className="btn btn--quiet" onClick={() => void redetect()}>
                Look again
              </button>
            </>
          )}
        </>
      ),
    },
    {
      title: "Where you sit",
      body: (
        <>
          <p>
            The one measurement nothing on this machine can work out, and the one everything else
            depends on. From your eyes to the nearest point of the centre screen.
          </p>
          {rig && (
            <div className="ob__field">
              <LengthField
                label="Eye to centre screen"
                hint="on a curved panel, to the middle of the curve"
                valueMm={rig.seating.eyeToCenter}
                unit={unit}
                min={100}
                onChange={(mm) =>
                  setRig({ ...rig, seating: { ...rig.seating, eyeToCenter: mm } })
                }
              />
            </div>
          )}
          {solution && (
            <p className="ob__readout">
              That gives you{" "}
              <strong className="num">{solution.totalCoverageDeg.toFixed(1)}°</strong> of view
              across {rig?.screens.length === 1 ? "the screen" : "all screens"}.
            </p>
          )}
          <p className="note">
            Rough is fine for now. Screen Setup has the rest — angles, bezels, curvature — and the
            numbers update as you type.
          </p>
        </>
      ),
    },
    {
      title: "Your hardware",
      body: (
        <>
          <p>
            Found <strong>{devices.length}</strong>{" "}
            {devices.length === 1 ? "device" : "devices"}.
          </p>
          {devices.length === 0 ? (
            <p className="note note--pending">
              Nothing yet. Plug your wheel in — the list fills itself, no rescan needed.
            </p>
          ) : (
            <ul className="ob__list">
              {devices.map((d) => (
                <li key={d.device.instancePath ?? d.device.displayName}>
                  <span>{d.device.displayName}</span>
                  <span className="num">{d.dinputPresent ? "games can see it" : "HID only"}</span>
                </li>
              ))}
            </ul>
          )}
          <p className="note">
            Which of these a game <em>requires</em> is set per game, on its profile. Nothing is
            assumed — a preflight that checks things nobody asked for is one people learn to ignore.
          </p>
        </>
      ),
    },
    {
      title: "Ready",
      body: (
        <>
          <p>Your rig is saved. From here:</p>
          <ol className="ob__next">
            <li>
              <strong>Screen Setup</strong> — angles, bezels and curvature, if you have not already.
            </li>
            <li>
              <strong>Games</strong> — every installed sim already has a profile. Say which
              peripherals it needs.
            </li>
            <li>
              <strong>Game Settings</strong> — write your measurements into the sims themselves.
            </li>
          </ol>
          <p className="note">
            Everything here is reversible. Config files are backed up before they are touched, and
            display changes revert on their own unless you confirm them.
          </p>
        </>
      ),
    },
  ];

  const current = steps[step];
  const last = step === steps.length - 1;

  return (
    <div className="ob">
      <div className="ob__panel glass">
        <header className="ob__head">
          <h1 className="ob__title">{current?.title}</h1>
          <span className="ob__dots" aria-label={`Step ${step + 1} of ${steps.length}`}>
            {steps.map((s, i) => (
              <span key={s.title} className={`ob__dot${i === step ? " ob__dot--on" : ""}`} />
            ))}
          </span>
        </header>

        <div className="ob__body">{current?.body}</div>

        {error && <p className="warn warn--hard">{error}</p>}

        <footer className="ob__foot">
          {/* Skippable throughout. Trapping somebody on the first screen of a
              product they have just paid for is worse than no wizard. */}
          <button className="btn btn--quiet" onClick={() => void finish()}>
            Skip setup
          </button>
          <span className="app__spacer" />
          {step > 0 && (
            <button className="btn btn--quiet" onClick={() => setStep(step - 1)}>
              Back
            </button>
          )}
          <button
            className="btn"
            disabled={saving}
            onClick={() => (last ? void finish() : setStep(step + 1))}
          >
            {last ? (saving ? "Saving…" : "Start racing") : "Next"}
          </button>
        </footer>
      </div>
    </div>
  );
}
