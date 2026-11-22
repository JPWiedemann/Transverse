//! Display units for lengths and frequencies. Internally everything is mm and Hz.

/// (symbol, factor to mm)
pub const LEN_UNITS: [(&str, f64); 7] =
    [("µm", 1e-3), ("mm", 1.0), ("cm", 10.0), ("m", 1000.0), ("mil", 0.0254), ("in", 25.4), ("ft", 304.8)];

/// (symbol, factor to Hz)
pub const FREQ_UNITS: [(&str, f64); 4] = [("Hz", 1.0), ("kHz", 1e3), ("MHz", 1e6), ("GHz", 1e9)];

/// Symbol of the relative-to-wavelength unit for mesh sizes.
pub const LAMBDA: &str = "λ";

/// Factor of a unit symbol in `table`; `lambda_mm` is the wavelength for the λ unit.
pub fn factor(table: &[(&str, f64)], unit: &str, lambda_mm: Option<f64>) -> Option<f64> {
    if unit == LAMBDA {
        return lambda_mm.filter(|l| l.is_finite() && *l > 0.0);
    }
    table.iter().find(|(s, _)| *s == unit).map(|(_, f)| *f)
}

/// Unit system for lengths shown on plots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitSystem {
    Metric,
    Imperial,
}

impl UnitSystem {
    pub fn key(self) -> &'static str {
        match self {
            Self::Metric => "metric",
            Self::Imperial => "imperial",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        [Self::Metric, Self::Imperial].into_iter().find(|s| s.key() == k)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Metric => "Metric",
            Self::Imperial => "Imperial",
        }
    }

    /// Prefixes the automatic choice picks from, smallest first: (symbol, factor to mm).
    pub fn auto_units(self) -> [(&'static str, f64); 4] {
        match self {
            Self::Metric => [("µm", 1e-3), ("mm", 1.0), ("cm", 10.0), ("m", 1000.0)],
            Self::Imperial => [("µin", 2.54e-5), ("mil", 0.0254), ("in", 25.4), ("ft", 304.8)],
        }
    }
}

/// The unit that suits a quantity of size `span_mm`: the largest one in which the span is at least 1.
/// Falls back to the smallest unit for tiny spans and to mm / in for invalid ones.
pub fn auto_length_unit(system: UnitSystem, span_mm: f64) -> (&'static str, f64) {
    let units = system.auto_units();
    if !span_mm.is_finite() || span_mm <= 0.0 {
        return units[1];
    }
    units.into_iter().rev().find(|(_, f)| span_mm / f >= 1.0).unwrap_or(units[0])
}

/// Smallest 1, 2 or 5 times a power of ten that is >= `x` (x > 0).
pub fn nice_step(x: f64) -> f64 {
    let mag = 10f64.powf(x.log10().floor());
    [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * mag).find(|s| *s >= x * (1.0 - 1e-12)).unwrap_or(10.0 * mag)
}

/// Grid steps [mm] that are round numbers in a unit of `factor` mm: `base_step_mm` is the smallest
/// useful step; the three levels are 1, 10 and 100 times a nice step.
pub fn grid_steps_mm(base_step_mm: f64, factor: f64) -> [f64; 3] {
    let s = nice_step((base_step_mm / factor).max(f64::MIN_POSITIVE)) * factor;
    [s, 10.0 * s, 100.0 * s]
}

/// Parse user input: decimal point or comma, surrounding spaces ignored.
pub fn parse_number(text: &str) -> Option<f64> {
    let v: f64 = text.trim().replace(',', ".").parse().ok()?;
    v.is_finite().then_some(v)
}

/// `v` with `digits` significant digits (fixed notation, no exponent; large values keep all integer digits).
pub fn format_sig(v: f64, digits: usize) -> String {
    if v == 0.0 || !v.is_finite() {
        return format!("{v}");
    }
    let decimals = (digits as i32 - 1 - v.abs().log10().floor() as i32).clamp(0, 12) as usize;
    format!("{v:.decimals$}")
}

/// Format a value like Python's `{:.6g}`: six significant digits, no trailing zeros.
pub fn format_g(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return format!("{v}");
    }
    let exp = v.abs().log10().floor() as i32;
    if !(-5..6).contains(&exp) {
        let s = format!("{v:.5e}");
        let (m, e) = s.split_once('e').unwrap();
        let m = if m.contains('.') { m.trim_end_matches('0').trim_end_matches('.') } else { m };
        return format!("{m}e{e}");
    }
    let decimals = (5 - exp).max(0) as usize;
    let s = format!("{v:.decimals$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factors_and_lambda() {
        assert_eq!(factor(&LEN_UNITS, "mil", None), Some(0.0254));
        assert_eq!(factor(&FREQ_UNITS, "GHz", None), Some(1e9));
        assert_eq!(factor(&LEN_UNITS, "λ", None), None);
        assert_eq!(factor(&LEN_UNITS, "λ", Some(50.0)), Some(50.0));
        assert_eq!(factor(&LEN_UNITS, "parsec", None), None);
    }

    #[test]
    fn auto_unit_picks_the_largest_unit_the_span_fills() {
        let m = |mm| auto_length_unit(UnitSystem::Metric, mm).0;
        assert_eq!(
            [m(0.001), m(0.04), m(0.999), m(1.0), m(1.28), m(9.9), m(10.0), m(250.0), m(1000.0), m(5000.0)],
            ["µm", "µm", "µm", "mm", "mm", "mm", "cm", "cm", "m", "m"]
        );
        let i = |mm| auto_length_unit(UnitSystem::Imperial, mm).0;
        // 0.04 mm = 1.6 mil; 1.28 mm = 50 mil; 30 mm = 1.2 in; 0.0005 mm = 20 µin
        assert_eq!([i(0.0005), i(0.04), i(1.28), i(30.0), i(400.0)], ["µin", "mil", "mil", "in", "ft"]);
        // below the smallest unit it stays at the smallest, invalid spans use the default
        assert_eq!(m(1e-9), "µm");
        assert_eq!(auto_length_unit(UnitSystem::Metric, f64::NAN), ("mm", 1.0));
        assert_eq!(auto_length_unit(UnitSystem::Imperial, -1.0).0, "mil");
        // the factor belongs to the symbol
        assert_eq!(auto_length_unit(UnitSystem::Imperial, 100.0), ("in", 25.4));
    }

    #[test]
    fn nice_steps_and_grid_levels() {
        for (x, want) in [
            (0.8, 1.0),
            (1.0, 1.0),
            (1.1, 2.0),
            (2.0, 2.0),
            (3.0, 5.0),
            (7.0, 10.0),
            (0.013, 0.02),
            (130.0, 200.0),
        ] {
            assert!((nice_step(x) - want).abs() < 1e-12 * want, "{x}");
        }
        // 0.013 mm minimum step shown in µm (factor 1e-3) becomes 20 µm = 0.02 mm
        let g = grid_steps_mm(0.013, 1e-3);
        assert!((g[0] - 0.02).abs() < 1e-12 && (g[1] - 0.2).abs() < 1e-12 && (g[2] - 2.0).abs() < 1e-12);
        // in mil the steps are round mil values: 0.013 mm = 0.51 mil -> 1 mil = 0.0254 mm
        let g = grid_steps_mm(0.013, 0.0254);
        assert!((g[0] - 0.0254).abs() < 1e-12 && (g[1] - 0.254).abs() < 1e-12);
        // keys round trip
        assert_eq!(UnitSystem::from_key("imperial"), Some(UnitSystem::Imperial));
        assert_eq!(UnitSystem::from_key(UnitSystem::Metric.key()), Some(UnitSystem::Metric));
        assert_eq!(UnitSystem::from_key("furlong"), None);
    }

    #[test]
    fn parse_accepts_comma_and_rejects_garbage() {
        assert_eq!(parse_number(" 0,36053 "), Some(0.36053));
        assert_eq!(parse_number("1e-3"), Some(1e-3));
        assert_eq!(parse_number("abc"), None);
        assert_eq!(parse_number("inf"), None);
    }

    #[test]
    fn significant_digits_in_fixed_notation() {
        assert_eq!(format_sig(0.123456, 5), "0.12346");
        assert_eq!(format_sig(123.456, 5), "123.46");
        assert_eq!(format_sig(15340.2, 5), "15340");
        assert_eq!(format_sig(1534022.0, 5), "1534022");
        assert_eq!(format_sig(-0.00123456, 3), "-0.00123");
        assert_eq!(format_sig(0.0, 5), "0");
        assert_eq!(format_sig(f64::NAN, 5), "NaN");
    }

    #[test]
    fn format_matches_python_g() {
        assert_eq!(format_g(0.36053), "0.36053");
        assert_eq!(format_g(18.0), "18");
        assert_eq!(format_g(600000.0), "600000");
        assert_eq!(format_g(0.0), "0");
        assert_eq!(format_g(0.1 + 0.2), "0.3");
        assert_eq!(format_g(1.23456789e-7), "1.23457e-7");
    }
}
