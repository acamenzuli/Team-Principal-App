//! Parsing lengths the way a person types them.
//!
//! The UI accepts `47.5in`, `1200mm`, `120cm`, `1.2m`, `47 1/2"` and bare
//! numbers in the currently selected unit, and normalises everything to
//! millimetres. Round-tripping mm -> in -> mm must re-render the same stored
//! f64 and never re-quantise the model.

use tp_model::{LengthUnit, Mm};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseLengthError {
    #[error("empty input")]
    Empty,
    #[error("could not read a number from {0:?}")]
    NotANumber(String),
    #[error("unknown unit {0:?}")]
    UnknownUnit(String),
    #[error("negative lengths are not meaningful here")]
    Negative,
}

/// Parse a length. `default_unit` applies only when the input carries no
/// suffix of its own — an explicit suffix always wins over the UI preference,
/// so switching the display unit never reinterprets what someone typed.
pub fn parse_length(input: &str, default_unit: LengthUnit) -> Result<Mm, ParseLengthError> {
    let s = input.trim();
    if s.is_empty() {
        return Err(ParseLengthError::Empty);
    }

    // Split the trailing alphabetic/quote suffix from the numeric part.
    let split = s
        .rfind(|c: char| c.is_ascii_digit() || c == '.' || c == '/')
        .map(|i| i + 1)
        .unwrap_or(0);
    let (num_part, unit_part) = s.split_at(split);
    let num_part = num_part.trim();
    let unit_part = unit_part.trim();

    let value = parse_number(num_part)?;
    if value < 0.0 {
        return Err(ParseLengthError::Negative);
    }

    let unit = if unit_part.is_empty() {
        default_unit
    } else {
        match unit_part.to_ascii_lowercase().as_str() {
            "mm" => LengthUnit::Mm,
            "cm" => LengthUnit::Cm,
            "m" => return Ok(Mm(value * 1000.0)),
            "in" | "inch" | "inches" | "\"" | "''" => LengthUnit::Inch,
            other => return Err(ParseLengthError::UnknownUnit(other.to_string())),
        }
    };

    Ok(match unit {
        LengthUnit::Mm => Mm(value),
        LengthUnit::Cm => Mm::from_cm(value),
        LengthUnit::Inch => Mm::from_inches(value),
    })
}

/// Handles `47`, `47.5`, `1/2` and `47 1/2` — the last because tape measures
/// are still imperial and people write what they read.
fn parse_number(s: &str) -> Result<f64, ParseLengthError> {
    if s.is_empty() {
        return Err(ParseLengthError::Empty);
    }
    if let Ok(v) = s.parse::<f64>() {
        return Ok(v);
    }
    // "47 1/2" or "1/2"
    let (whole, frac) = match s.split_once(' ') {
        Some((w, f)) => (w.trim().parse::<f64>().map_err(|_| bad(s))?, f.trim()),
        None => (0.0, s),
    };
    let (n, d) = frac.split_once('/').ok_or_else(|| bad(s))?;
    let n: f64 = n.trim().parse().map_err(|_| bad(s))?;
    let d: f64 = d.trim().parse().map_err(|_| bad(s))?;
    if d == 0.0 {
        return Err(bad(s));
    }
    Ok(whole + n / d)
}

fn bad(s: &str) -> ParseLengthError {
    ParseLengthError::NotANumber(s.to_string())
}

/// Render a stored millimetre value in the display unit. Display only — this
/// value is never read back into the model.
pub fn format_length(mm: Mm, unit: LengthUnit) -> String {
    match unit {
        LengthUnit::Mm => format!("{:.1} mm", mm.0),
        LengthUnit::Cm => format!("{:.2} cm", mm.cm()),
        LengthUnit::Inch => format!("{:.3} in", mm.inches()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_suffix_beats_the_ui_preference() {
        // The whole point: someone typing "1200mm" while the UI is set to
        // inches means 1200 mm, not 1200 inches.
        assert_eq!(
            parse_length("1200mm", LengthUnit::Inch).unwrap(),
            Mm(1200.0)
        );
        assert_eq!(parse_length("120cm", LengthUnit::Inch).unwrap(), Mm(1200.0));
        assert_eq!(parse_length("1.2m", LengthUnit::Inch).unwrap(), Mm(1200.0));
    }

    #[test]
    fn bare_numbers_use_the_ui_preference() {
        assert_eq!(parse_length("1200", LengthUnit::Mm).unwrap(), Mm(1200.0));
        assert_eq!(parse_length("120", LengthUnit::Cm).unwrap(), Mm(1200.0));
        let inches = parse_length("47.5", LengthUnit::Inch).unwrap();
        assert!((inches.0 - 1206.5).abs() < 1e-9);
    }

    #[test]
    fn inch_spellings() {
        for s in ["47.5in", "47.5 in", "47.5inch", "47.5inches", "47.5\""] {
            let v = parse_length(s, LengthUnit::Mm).unwrap();
            assert!((v.0 - 1206.5).abs() < 1e-9, "{s} parsed as {v:?}");
        }
    }

    #[test]
    fn fractional_inches_because_tape_measures_are_imperial() {
        let v = parse_length("47 1/2in", LengthUnit::Mm).unwrap();
        assert!((v.0 - 1206.5).abs() < 1e-9, "{v:?}");
        let half = parse_length("1/2in", LengthUnit::Mm).unwrap();
        assert!((half.0 - 12.7).abs() < 1e-9);
    }

    #[test]
    fn round_trip_is_lossless_in_the_model() {
        // Switching display units must not drift the stored value. This is the
        // invariant that makes "display unit is a UI preference" true.
        for raw in [1193.0, 700.0, 12.5, 0.1, 5120.0] {
            let stored = Mm(raw);
            let as_inches = stored.inches();
            let back = Mm::from_inches(as_inches);
            assert!(
                (back.0 - stored.0).abs() < 1e-9,
                "{raw} drifted to {}",
                back.0
            );
            let back_cm = Mm::from_cm(stored.cm());
            assert!((back_cm.0 - stored.0).abs() < 1e-9);
        }
    }

    #[test]
    fn rejects_nonsense() {
        assert!(parse_length("", LengthUnit::Mm).is_err());
        assert!(parse_length("abc", LengthUnit::Mm).is_err());
        assert!(parse_length("12furlongs", LengthUnit::Mm).is_err());
        assert!(parse_length("-5mm", LengthUnit::Mm).is_err());
    }
}
