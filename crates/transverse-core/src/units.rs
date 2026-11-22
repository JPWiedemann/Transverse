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

/// Parse user input: decimal point or comma, surrounding spaces ignored.
pub fn parse_number(text: &str) -> Option<f64> {
    let v: f64 = text.trim().replace(',', ".").parse().ok()?;
    v.is_finite().then_some(v)
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
    fn parse_accepts_comma_and_rejects_garbage() {
        assert_eq!(parse_number(" 0,36053 "), Some(0.36053));
        assert_eq!(parse_number("1e-3"), Some(1e-3));
        assert_eq!(parse_number("abc"), None);
        assert_eq!(parse_number("inf"), None);
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
