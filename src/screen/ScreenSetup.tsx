import { useCallback, useEffect, useMemo, useState } from "react";

import { Section } from "../dashboard/primitives";
import {
  asIpcError,
  currentRig,
  deleteRig,
  detectRig,
  listRigs,
  fitRig,
  saveRig,
  solveRig,
  type BestFitInfo,
  type LengthUnit,
  type RigModel,
  type RigSolutionInfo,
  type ScreenSpec,
  type SessionMode,
} from "../ipc";
import { DegreeField, LengthField } from "./LengthField";
import { RigSchematic } from "./RigSchematic";
import { ScaleElevation } from "./ScaleElevation";
import "./screen.css";

const CENTRE_ONLY: SessionMode = { kind: "center_only" };
const FULL_SPAN: SessionMode = { kind: "full_span", fit: "letterbox" };

/**
 * Screen Setup — the one place physical rig data is entered.
 *
 * Everything else in the product derives from what is on this page. The
 * detectable half is filled in from the monitors; what remains is only what no
 * API can know — bezels, angles, and how far back the driver sits.
 *
 * The numbers on the right update as you type, before anything is saved, so a
 * wrong measurement shows itself immediately rather than at the next race.
 */
export function ScreenSetup({ unit }: { unit: LengthUnit }) {
  const [rig, setRig] = useState<RigModel | null>(null);
  const [rigs, setRigs] = useState<RigModel[]>([]);
  const [solution, setSolution] = useState<RigSolutionInfo | null>(null);
  const [fit, setFit] = useState<BestFitInfo | null>(null);
  const [session, setSession] = useState<SessionMode>(FULL_SPAN);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);

  useEffect(() => {
    currentRig()
      .then(setRig)
      .catch((e) => setError(asIpcError(e).message));
    // Every saved rig, so the switcher can offer them. A failure here is not
    // worth an error: one rig is the normal case and the switcher simply does
    // not appear.
    listRigs()
      .then(setRigs)
      .catch(() => setRigs([]));
  }, []);

  // Re-solve on every edit. The solver is pure and microseconds fast, so there
  // is no reason to debounce and every reason not to: the numbers should track
  // the field being typed into.
  useEffect(() => {
    if (!rig) return;
    solveRig(rig, session).then(setSolution).catch(() => setSolution(null));
    fitRig(rig, session).then(setFit).catch(() => setFit(null));
  }, [rig, session]);

  const update = useCallback((next: RigModel) => {
    setRig(next);
    setSaved(null);
  }, []);

  const updateScreen = useCallback(
    (index: number, change: (s: ScreenSpec) => ScreenSpec) => {
      setRig((prev) => {
        if (!prev) return prev;
        const screens = prev.screens.map((s, i) => (i === index ? change(s) : s));
        return { ...prev, screens };
      });
      setSaved(null);
    },
    [],
  );

  const onDetect = async () => {
    try {
      setRig(await detectRig(rig));
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    }
  };

  const onSave = async () => {
    if (!rig) return;
    try {
      const stored = await saveRig(rig);
      setRig(stored);
      setSaved(`Saved as revision ${stored.revision}.`);
      setError(null);
    } catch (e) {
      setError(asIpcError(e).message);
    }
  };

  const blocking = useMemo(
    () => solution?.warnings.filter((w) => w.kind === "missing_physical_size") ?? [],
    [solution],
  );

  if (error && !rig) return <p className="app__error">{error}</p>;
  if (!rig) return <p className="note">Reading your displays…</p>;

  return (
    <div className="setup">
      <div className="setup__form">
        {/* More than one rig is a real case — a wheel stand for the desk and a
            proper cockpit, or a triple you sometimes run as a single. The
            storage has always kept them; this is the switch. */}
        {rigs.length > 1 && (
          <Section title="Rig" note={`${rigs.length} saved`}>
            <div className="setup__rigs">
              {rigs.map((r) => (
                <button
                  key={r.id}
                  className={`setup__rig${r.id === rig.id ? " setup__rig--on" : ""}`}
                  onClick={() => setRig(r)}
                >
                  <span>{r.name}</span>
                  <span className="num">
                    {r.screens.length} {r.screens.length === 1 ? "screen" : "screens"}
                  </span>
                </button>
              ))}
            </div>
            {rigs.length > 1 && (
              <button
                className="btn btn--quiet"
                onClick={() =>
                  void deleteRig(rig.id)
                    .then(() => listRigs())
                    .then((all) => {
                      setRigs(all);
                      if (all[0]) setRig(all[0]);
                    })
                    .catch((e) => setError(asIpcError(e).message))
                }
              >
                Delete “{rig.name}”
              </button>
            )}
          </Section>
        )}

        <Section
          title="Where you sit"
          note="Measured from your normal driving position, hands on the wheel."
        >
          <div className="fields">
            <LengthField
              label="Eye to centre screen"
              hint="To the nearest point of the visible image"
              valueMm={rig.seating.eyeToCenter}
              unit={unit}
              min={1}
              onChange={(mm) =>
                update({ ...rig, seating: { ...rig.seating, eyeToCenter: mm } })
              }
            />
            <LengthField
              label="Eye height offset"
              hint="Positive if your eyes are above screen centre"
              valueMm={rig.seating.eyeHeightOffset}
              unit={unit}
              onChange={(mm) =>
                update({ ...rig, seating: { ...rig.seating, eyeHeightOffset: mm } })
              }
            />
            <LengthField
              label="Sideways offset"
              hint="Positive if you sit right of centre"
              valueMm={rig.seating.lateralOffset}
              unit={unit}
              onChange={(mm) =>
                update({ ...rig, seating: { ...rig.seating, lateralOffset: mm } })
              }
            />
          </div>
        </Section>

        {rig.screens.map((screen, index) => (
          <ScreenCard
            key={screen.id}
            screen={screen}
            index={index}
            unit={unit}
            onChange={updateScreen}
          />
        ))}

        <div className="setup__actions">
          <button className="btn" onClick={onSave}>
            Save rig
          </button>
          <button className="btn btn--quiet" onClick={onDetect}>
            Re-detect displays
          </button>
          {saved && <span className="setup__saved">{saved}</span>}
          {error && <span className="setup__error">{error}</span>}
        </div>
        <p className="hint">
          Re-detecting keeps every bezel, angle and gap you have entered — screens are matched by
          the monitor's own identity, not by which port it is in.
        </p>
      </div>

      <aside className="setup__results">
        <Section title="What this rig gives you">
          <div className="modes">
            <button
              className={`modebtn${session.kind === "center_only" ? " modebtn--on" : ""}`}
              onClick={() => setSession(CENTRE_ONLY)}
            >
              Centre only
            </button>
            <button
              className={`modebtn${session.kind === "full_span" ? " modebtn--on" : ""}`}
              onClick={() => setSession(FULL_SPAN)}
            >
              All screens
            </button>
          </div>

          {blocking.length > 0 && (
            <p className="warn warn--hard">
              <span aria-hidden="true">■</span> {blocking[0]!.message}
            </p>
          )}

          {solution && solution.screens.length > 0 && (
            <>
              <div className="figures">
                {solution.screens.map((s) => (
                  <div className="figure" key={s.id}>
                    <span className="figure__label">{roleLabel(s.role.kind)}</span>
                    <span className="figure__value num">{s.hFovDeg.toFixed(2)}°</span>
                    <span className="figure__sub num">
                      {s.vFovDeg.toFixed(2)}° vertical · {s.distanceMm.toFixed(0)} mm ·{" "}
                      {s.pxPerDegH.toFixed(1)} px/°
                    </span>
                    {s.span.asymmetryDeg > 1 && (
                      <span className="figure__sub num">
                        off-centre: {s.span.leftDeg.toFixed(1)}° to {s.span.rightDeg.toFixed(1)}°
                      </span>
                    )}
                    {s.innerGap && (
                      <span className="figure__sub num">
                        gap {s.innerGap.mm.toFixed(1)} mm · {s.innerGap.deg.toFixed(2)}° ·{" "}
                        {s.innerGap.px === null
                          ? "px n/a (pitch differs)"
                          : `${s.innerGap.px.toFixed(0)} px`}
                      </span>
                    )}
                  </div>
                ))}
                <div className="figure figure--total">
                  <span className="figure__label">Total coverage</span>
                  <span className="figure__value num">
                    {solution.totalCoverageDeg.toFixed(2)}°
                  </span>
                  <span className="figure__sub num">
                    {solution.visibleCoverageDeg.toFixed(2)}° on glass, the rest is bezel
                  </span>
                </div>
              </div>

              <RigSchematic solution={solution} />
              <ScaleElevation solution={solution} />

              {solution.warnings.length > 0 && (
                <ul className="warnings">
                  {solution.warnings.map((w, i) => (
                    <li key={i} className="warn">
                      <span aria-hidden="true">▲</span> {w.message}
                    </li>
                  ))}
                </ul>
              )}

              {fit && (
                <p className={`fitnote${fit.isExact ? " fitnote--exact" : ""}`}>
                  <strong>For sims that accept one screen size:</strong>{" "}
                  {fit.widthMm.toFixed(0)} × {fit.heightMm.toFixed(0)} mm, {fit.bezelMm.toFixed(1)} mm
                  bezel, {fit.distanceMm.toFixed(0)} mm away, {fit.angleDeg.toFixed(1)}° angle.{" "}
                  {fit.isExact
                    ? "Your screens match, so these values are exact."
                    : `These are a best fit — your worst screen is off by ${fit.worstErrorDeg.toFixed(1)}°.`}
                </p>
              )}
            </>
          )}
        </Section>
      </aside>
    </div>
  );
}

function ScreenCard({
  screen,
  index,
  unit,
  onChange,
}: {
  screen: ScreenSpec;
  index: number;
  unit: LengthUnit;
  onChange: (index: number, change: (s: ScreenSpec) => ScreenSpec) => void;
}) {
  const set = (change: (s: ScreenSpec) => ScreenSpec) => onChange(index, change);
  const isCentre = screen.role.kind === "center";
  const curved = screen.panel.curvature.kind === "radius";

  return (
    <Section
      title={roleLabel(screen.role.kind)}
      note={`${screen.panel.nativeResolution.width}×${screen.panel.nativeResolution.height} · ${sourceLabel(screen.panel.visibleWidth.source)}`}
    >
      <div className="fields">
        <LengthField
          label={curved ? "Visible width (along the curve)" : "Visible width"}
          hint="The image, not the chassis"
          valueMm={screen.panel.visibleWidth.mm}
          unit={unit}
          min={1}
          onChange={(mm) =>
            set((s) => ({
              ...s,
              panel: { ...s.panel, visibleWidth: { mm, source: "manual" } },
            }))
          }
        />
        <LengthField
          label="Visible height"
          valueMm={screen.panel.visibleHeight.mm}
          unit={unit}
          min={1}
          onChange={(mm) =>
            set((s) => ({
              ...s,
              panel: { ...s.panel, visibleHeight: { mm, source: "manual" } },
            }))
          }
        />
        {!isCentre && (
          <DegreeField
            label="Angle inward"
            hint="0 = flat with the centre"
            value={screen.mounting.angle}
            onChange={(deg) => set((s) => ({ ...s, mounting: { ...s.mounting, angle: deg } }))}
          />
        )}
      </div>

      <div className="fields">
        <LengthField
          label="Bezel left"
          valueMm={screen.panel.bezel.left}
          unit={unit}
          onChange={(mm) =>
            set((s) => ({ ...s, panel: { ...s.panel, bezel: { ...s.panel.bezel, left: mm } } }))
          }
        />
        <LengthField
          label="Bezel right"
          valueMm={screen.panel.bezel.right}
          unit={unit}
          onChange={(mm) =>
            set((s) => ({ ...s, panel: { ...s.panel, bezel: { ...s.panel.bezel, right: mm } } }))
          }
        />
        {!isCentre && (
          <LengthField
            label="Mount gap"
            hint="Extra air beyond both bezels"
            valueMm={screen.mounting.gap}
            unit={unit}
            onChange={(mm) => set((s) => ({ ...s, mounting: { ...s.mounting, gap: mm } }))}
          />
        )}
        {!isCentre && (
          <LengthField
            label="Height offset"
            hint="Positive if this panel sits higher"
            valueMm={screen.mounting.verticalOffset}
            unit={unit}
            onChange={(mm) =>
              set((s) => ({ ...s, mounting: { ...s.mounting, verticalOffset: mm } }))
            }
          />
        )}
      </div>

      <div className="fields fields--inline">
        <label className="toggle">
          <input
            type="checkbox"
            checked={curved}
            onChange={(e) =>
              set((s) => ({
                ...s,
                panel: {
                  ...s.panel,
                  curvature: e.target.checked ? { kind: "radius", radius: 1000 } : { kind: "flat" },
                },
              }))
            }
          />
          <span className="toggle__track" aria-hidden="true">
            <span className="toggle__knob" />
          </span>
          <span>Curved</span>
        </label>
        {curved && screen.panel.curvature.kind === "radius" && (
          <LengthField
            label="Curve radius"
            hint="1000R, 1800R — from the spec sheet"
            valueMm={screen.panel.curvature.radius}
            unit={unit}
            min={100}
            onChange={(mm) =>
              set((s) => ({ ...s, panel: { ...s.panel, curvature: { kind: "radius", radius: mm } } }))
            }
          />
        )}
      </div>
    </Section>
  );
}

function roleLabel(kind: string): string {
  switch (kind) {
    case "center":
      return "Centre screen";
    case "left":
      return "Left screen";
    case "right":
      return "Right screen";
    default:
      return "Auxiliary screen";
  }
}

function sourceLabel(source: string): string {
  switch (source) {
    case "edid":
      return "size read from the monitor";
    case "manual":
      return "size you measured";
    default:
      return "size not known";
  }
}
