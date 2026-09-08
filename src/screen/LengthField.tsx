import { useEffect, useState } from "react";

import { parseLength, type LengthUnit } from "../ipc";

/**
 * A length input that accepts what people actually type.
 *
 * `47.5in`, `1200mm`, `120cm`, `47 1/2"`, or a bare number in the current
 * display unit. An explicit suffix always beats the unit setting, so switching
 * the display unit never reinterprets something already typed.
 *
 * The parsing happens in Rust. That is a round trip per keystroke, which over
 * local IPC is far too fast to notice, and it means there is exactly one length
 * parser in the product and it is the one under test — rather than a second one
 * here that drifts.
 */
export function LengthField({
  label,
  hint,
  valueMm,
  unit,
  onChange,
  min,
}: {
  label: string;
  hint?: string;
  valueMm: number;
  unit: LengthUnit;
  onChange: (mm: number) => void;
  min?: number;
}) {
  const [text, setText] = useState(() => format(valueMm, unit));
  const [helper, setHelper] = useState<string | null>(null);
  const [bad, setBad] = useState(false);
  const [editing, setEditing] = useState(false);

  // While the field is focused the user owns the text. Outside that, the model
  // owns it — so changing the display unit, or re-detecting, updates the field.
  useEffect(() => {
    if (!editing) setText(format(valueMm, unit));
  }, [valueMm, unit, editing]);

  const commit = async (raw: string) => {
    setText(raw);
    if (raw.trim() === "") {
      setBad(true);
      setHelper(null);
      return;
    }
    try {
      const parsed = await parseLength(raw, unit);
      setBad(false);
      // Show the value in the units the field is *not* in, which is where a
      // typo becomes obvious.
      setHelper(
        unit === "mm"
          ? `${parsed.formattedInch} · ${parsed.formattedCm}`
          : `${parsed.formattedMm}`,
      );
      if (min === undefined || parsed.mm >= min) onChange(parsed.mm);
    } catch {
      setBad(true);
      setHelper(null);
    }
  };

  return (
    <label className="lf">
      <span className="lf__label">{label}</span>
      <input
        className={`lf__input num${bad ? " lf__input--bad" : ""}`}
        value={text}
        inputMode="decimal"
        onFocus={() => setEditing(true)}
        onBlur={() => {
          setEditing(false);
          setText(format(valueMm, unit));
        }}
        onChange={(e) => void commit(e.target.value)}
      />
      <span className={`lf__helper${bad ? " lf__helper--bad" : ""}`}>
        {bad ? "Not a length" : (helper ?? hint ?? " ")}
      </span>
    </label>
  );
}

/** Display only. The model is always millimetres. */
function format(mm: number, unit: LengthUnit): string {
  switch (unit) {
    case "cm":
      return (mm / 10).toFixed(2);
    case "inch":
      return (mm / 25.4).toFixed(3);
    default:
      return mm.toFixed(1);
  }
}

export function DegreeField({
  label,
  hint,
  value,
  onChange,
}: {
  label: string;
  hint?: string;
  value: number;
  onChange: (deg: number) => void;
}) {
  const [text, setText] = useState(() => value.toFixed(1));
  const [editing, setEditing] = useState(false);

  useEffect(() => {
    if (!editing) setText(value.toFixed(1));
  }, [value, editing]);

  return (
    <label className="lf">
      <span className="lf__label">{label}</span>
      <input
        className="lf__input num"
        value={text}
        inputMode="decimal"
        onFocus={() => setEditing(true)}
        onBlur={() => {
          setEditing(false);
          setText(value.toFixed(1));
        }}
        onChange={(e) => {
          setText(e.target.value);
          const n = Number(e.target.value);
          if (Number.isFinite(n)) onChange(n);
        }}
      />
      <span className="lf__helper">{hint ?? "degrees"}</span>
    </label>
  );
}
