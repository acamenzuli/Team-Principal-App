import { useEffect, useState } from "react";

import { asIpcError, solveCurvature, type CurveResult } from "../ipc";
import { Section } from "./primitives";

/**
 * A live bench over the real geometry crate.
 *
 * It exists this early for two reasons. It exercises the whole pipe — Rust
 * math, ts-rs bindings, typed client — with one number changing on screen. And
 * it lets the rig owner check the curved-screen model against a datasheet
 * before a single line of Screen Setup UI exists.
 *
 * The two wrong answers are shown next to the right one on purpose. A 49"
 * 1000R panel is overstated by more than 15 degrees by the naive calculation
 * most FOV tools do, and seeing that gap is more convincing than being told.
 */
export function CurvatureBench() {
  const [width, setWidth] = useState("1193");
  const [radius, setRadius] = useState("1000");
  const [eye, setEye] = useState("700");
  const [asChord, setAsChord] = useState(false);
  const [result, setResult] = useState<CurveResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const w = Number(width);
    const r = Number(radius);
    const d = Number(eye);
    if (!Number.isFinite(w) || !Number.isFinite(d) || w <= 0 || d <= 0) return;

    solveCurvature({
      arcOrChordMm: w,
      radiusMm: Number.isFinite(r) && r > 0 ? r : null,
      measuredAsChord: asChord,
      eyeDistanceMm: d,
    })
      .then((r) => {
        setResult(r);
        setError(null);
      })
      .catch((e) => setError(asIpcError(e).message));
  }, [width, radius, eye, asChord]);

  return (
    <Section title="Curved screen bench" note="geometry engine, live">
      <div className="bench">
        <div className="bench__inputs">
          <Field label={asChord ? "Chord width" : "Arc width"} suffix="mm" value={width} onChange={setWidth} />
          <Field label="Curve radius" suffix="mm (blank = flat)" value={radius} onChange={setRadius} />
          <Field label="Eye to nearest point" suffix="mm" value={eye} onChange={setEye} />
          <label className="field__check">
            <input type="checkbox" checked={asChord} onChange={(e) => setAsChord(e.target.checked)} />
            <span>Width entered is the chord, not the arc</span>
          </label>
        </div>

        {error && <p className="note note--fail">{error}</p>}

        {result && (
          <div className="bench__out">
            <Readout label="Horizontal FOV" value={result.hFovDeg} unit="°" emphasis />
            <Readout label="Chord width" value={result.chordMm} unit="mm" />
            <Readout label="Arc width" value={result.arcMm} unit="mm" />
            <Readout label="Sagitta" value={result.sagittaMm} unit="mm" />
            <Readout label="Chord-plane distance" value={result.chordPlaneDistanceMm} unit="mm" />
            <Readout label="Subtended at centre" value={result.subtendedDeg} unit="°" />
            {result.worstCaseErrorDeg !== null && (
              <Readout label="Worst-case flat error" value={result.worstCaseErrorDeg} unit="°" />
            )}
          </div>
        )}

        {result && result.naiveHFovDeg - result.hFovDeg > 0.05 && (
          <p className="bench__warning">
            <span className="bench__warning-mark">▲</span>
            Treating the arc as a flat width would give{" "}
            <strong className="num">{result.naiveHFovDeg.toFixed(2)}°</strong> — overstating your FOV
            by <strong className="num">{(result.naiveHFovDeg - result.hFovDeg).toFixed(2)}°</strong>.
            The edges of a curved panel wrap toward you, so they are separated by the chord and sit a
            sagitta further away. Pairing the chord with the chord-plane distance is what makes{" "}
            <strong className="num">{result.hFovDeg.toFixed(2)}°</strong> correct.
          </p>
        )}
      </div>
    </Section>
  );
}

function Field(props: { label: string; suffix: string; value: string; onChange: (v: string) => void }) {
  return (
    <label className="field">
      <span className="field__label">{props.label}</span>
      <span className="field__row">
        <input
          className="field__input num"
          inputMode="decimal"
          value={props.value}
          onChange={(e) => props.onChange(e.target.value)}
        />
        <span className="field__suffix">{props.suffix}</span>
      </span>
    </label>
  );
}

function Readout(props: { label: string; value: number; unit: string; emphasis?: boolean }) {
  return (
    <div className={props.emphasis ? "readout readout--key" : "readout"}>
      <span className="readout__label">{props.label}</span>
      <span className="readout__value num">
        {props.value.toFixed(2)}
        <span className="readout__unit">{props.unit}</span>
      </span>
    </div>
  );
}
