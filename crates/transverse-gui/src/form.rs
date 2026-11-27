//! Editable state of a parameter form: text fields with unit dropdowns, flags and choices.
//!
//! The form is generated from a line type's [`ParamSpec`]s. Text is kept as typed so that
//! half-finished input is never lost; conversion to internal units (mm, Hz) happens in
//! [`FormState::to_params`].

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use transverse_core::units::{self, FREQ_UNITS, LAMBDA, LEN_UNITS, format_g, parse_number};
use transverse_lines::model::{LineModel, ParamKind, ParamSet, ParamSpec, ParamValue, Preset, is_visible};

/// Edit state of one parameter.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldState {
    /// Number with unit: `text` is shown in `unit`.
    Quantity {
        text: String,
        unit: String,
    },
    Flag(bool),
    Choice(usize),
}

/// Input error with the offending field.
#[derive(Clone, Debug, PartialEq)]
pub struct FormError {
    pub key: &'static str,
    pub label: &'static str,
    pub message: String,
}

impl fmt::Display for FormError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.label, self.message)
    }
}

/// Unit symbols offered for a parameter (empty if it has a fixed unit or none).
pub fn unit_options(spec: &ParamSpec) -> Vec<&'static str> {
    match spec.kind {
        ParamKind::Length { lambda, .. } => {
            let mut v: Vec<&str> = LEN_UNITS.iter().map(|u| u.0).collect();
            if lambda {
                v.push(LAMBDA);
            }
            v
        }
        ParamKind::Frequency { .. } => FREQ_UNITS.iter().map(|u| u.0).collect(),
        _ => Vec::new(),
    }
}

/// Initial display unit; the fixed unit label for plain numbers.
fn default_unit(spec: &ParamSpec) -> &'static str {
    match spec.kind {
        ParamKind::Length { unit, .. }
        | ParamKind::Frequency { unit, .. }
        | ParamKind::Number { unit, .. } => unit,
        _ => "",
    }
}

/// Factor from the displayed unit to the internal one.
fn factor(spec: &ParamSpec, unit: &str, lambda_mm: Option<f64>) -> Option<f64> {
    match spec.kind {
        ParamKind::Length { .. } => units::factor(&LEN_UNITS, unit, lambda_mm),
        ParamKind::Frequency { .. } => units::factor(&FREQ_UNITS, unit, None),
        ParamKind::Number { scale, .. } => Some(scale),
        _ => None,
    }
}

fn is_lambda(unit: &str) -> bool {
    unit == LAMBDA
}

#[derive(Clone, Debug, PartialEq)]
pub struct FormState {
    pub fields: BTreeMap<&'static str, FieldState>,
}

impl FormState {
    /// Form with the line type's defaults in their default display units.
    pub fn new(model: &dyn LineModel) -> Self {
        let mut s = Self { fields: BTreeMap::new() };
        s.set_params(model, &model.defaults());
        s
    }

    /// Show `set` in the fields, keeping each field's current display unit.
    pub fn set_params(&mut self, model: &dyn LineModel, set: &ParamSet) {
        let lam = model.wavelength_mm(set);
        for spec in model.params() {
            let Some(v) = set.get(spec.key) else { continue };
            let new = match (spec.kind, v) {
                (ParamKind::Bool(_), ParamValue::Bool(b)) => FieldState::Flag(b),
                (ParamKind::Choice(..), ParamValue::Num(i)) => FieldState::Choice(i as usize),
                (_, ParamValue::Num(x)) => {
                    let unit = match self.fields.get(spec.key) {
                        Some(FieldState::Quantity { unit, .. }) => unit.clone(),
                        _ => default_unit(spec).to_string(),
                    };
                    // fall back to the default unit if a λ reference is unavailable
                    let (unit, f) = match factor(spec, &unit, lam) {
                        Some(f) => (unit, f),
                        None => {
                            let u = default_unit(spec);
                            (u.to_string(), factor(spec, u, lam).unwrap_or(1.0))
                        }
                    };
                    FieldState::Quantity { text: format_g(x / f), unit }
                }
                _ => continue,
            };
            self.fields.insert(spec.key, new);
        }
    }

    /// Set the switches and choices of a preset; all other fields stay as they are.
    pub fn apply_preset(&mut self, preset: &Preset) {
        for (key, value) in preset.values {
            match (self.fields.get_mut(key), value) {
                (Some(FieldState::Flag(b)), ParamValue::Bool(v)) => *b = *v,
                (Some(FieldState::Choice(i)), ParamValue::Num(v)) => *i = *v as usize,
                _ => {}
            }
        }
    }

    /// Flags and choices only (everything else at its default): enough to decide which controls are
    /// shown while some text field holds unfinished input.
    pub fn control_set(&self, model: &dyn LineModel) -> ParamSet {
        let mut set = model.defaults();
        for (k, f) in &self.fields {
            match f {
                FieldState::Flag(b) => set.set(k, ParamValue::Bool(*b)),
                FieldState::Choice(i) => set.set(k, ParamValue::Num(*i as f64)),
                FieldState::Quantity { .. } => {}
            }
        }
        set
    }

    /// Parameters in internal units. Sizes given in λ refer to the wavelength computed from the
    /// other fields. A field that is hidden cannot hold anything up: if its text is no number, it keeps
    /// its default (and its text).
    pub fn to_params(&self, model: &dyn LineModel) -> Result<ParamSet, FormError> {
        let mut set = model.defaults();
        let controls = self.control_set(model);
        let err = |spec: &ParamSpec, message: String| FormError { key: spec.key, label: spec.label, message };
        // first pass: everything not given in λ
        for pass in [false, true] {
            let lam = if pass { model.wavelength_mm(&set) } else { None };
            for spec in model.params() {
                let Some(state) = self.fields.get(spec.key) else { continue };
                match state {
                    FieldState::Flag(b) => set.set(spec.key, ParamValue::Bool(*b)),
                    FieldState::Choice(i) => set.set(spec.key, ParamValue::Num(*i as f64)),
                    FieldState::Quantity { text, unit } => {
                        if is_lambda(unit) != pass {
                            continue;
                        }
                        let value = parse_number(text)
                            .ok_or_else(|| format!("'{}' is not a number", text.trim()))
                            .and_then(|v| {
                                factor(spec, unit, lam).map(|f| v * f).ok_or("no wavelength reference".into())
                            });
                        match value {
                            Ok(v) => set.set(spec.key, ParamValue::Num(v)),
                            Err(message) if is_visible(spec.visible_when, &controls) => {
                                return Err(err(spec, message));
                            }
                            Err(_) => {}
                        }
                    }
                }
            }
        }
        Ok(set)
    }

    /// Switch the display unit of a field and convert the shown value (an unparsable text is kept).
    pub fn change_unit(&mut self, model: &dyn LineModel, key: &str, new_unit: &str) {
        let Some(spec) = model.params().iter().find(|s| s.key == key) else { return };
        let lam = self.to_params(model).ok().and_then(|s| model.wavelength_mm(&s));
        let Some(FieldState::Quantity { text, unit }) = self.fields.get_mut(spec.key) else { return };
        let old = unit.clone();
        if !unit_options(spec).contains(&new_unit) {
            return;
        }
        if let (Some(v), Some(fo), Some(fnew)) =
            (parse_number(text), factor(spec, &old, lam), factor(spec, new_unit, lam))
        {
            *text = format_g(v * fo / fnew);
        }
        *unit = new_unit.to_string();
    }

    pub fn to_saved(&self) -> BTreeMap<String, SavedField> {
        self.fields
            .iter()
            .map(|(k, f)| {
                let s = match f {
                    FieldState::Quantity { text, unit } => SavedField {
                        value: Some(text.clone()),
                        unit: (!unit.is_empty()).then(|| unit.clone()),
                        ..Default::default()
                    },
                    FieldState::Flag(b) => SavedField { flag: Some(*b), ..Default::default() },
                    FieldState::Choice(i) => SavedField { choice: Some(*i), ..Default::default() },
                };
                (k.to_string(), s)
            })
            .collect()
    }

    /// Restore saved fields; unknown keys, wrong kinds and invalid units are ignored.
    pub fn load_saved(&mut self, model: &dyn LineModel, saved: &BTreeMap<String, SavedField>) {
        for spec in model.params() {
            let Some(s) = saved.get(spec.key) else { continue };
            match (spec.kind, self.fields.get_mut(spec.key)) {
                (ParamKind::Bool(_), Some(FieldState::Flag(b))) => {
                    if let Some(v) = s.flag {
                        *b = v;
                    }
                }
                (ParamKind::Choice(opts, _), Some(FieldState::Choice(i))) => {
                    if let Some(v) = s.choice.filter(|v| *v < opts.len()) {
                        *i = v;
                    }
                }
                (_, Some(FieldState::Quantity { text, unit })) => {
                    if let Some(u) = &s.unit {
                        let fixed = matches!(spec.kind, ParamKind::Number { .. });
                        if !fixed && unit_options(spec).contains(&u.as_str()) {
                            *unit = u.clone();
                        }
                    }
                    if let Some(v) = &s.value {
                        *text = v.clone();
                    }
                }
                _ => {}
            }
        }
    }
}

/// One saved field (same idea as the Python settings file: text as typed plus the unit).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedField {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flag: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use transverse_lines::planar::{PlanarLine, PlanarParams};

    fn text(f: &FormState, k: &str) -> (String, String) {
        match &f.fields[k] {
            FieldState::Quantity { text, unit } => (text.clone(), unit.clone()),
            other => panic!("{k}: {other:?}"),
        }
    }

    #[test]
    fn defaults_show_in_their_display_units_and_convert_back() {
        let m = PlanarLine;
        let f = FormState::new(&m);
        assert_eq!(text(&f, "w"), ("0.3605".into(), "mm".into()));
        assert_eq!(text(&f, "t"), ("18".into(), "µm".into()));
        assert_eq!(text(&f, "f"), ("1".into(), "GHz".into()));
        assert_eq!(text(&f, "sigma"), ("58".into(), "MS/m".into()));
        assert_eq!(text(&f, "dmin"), ("2".into(), "µm".into()));
        assert_eq!(f.fields["plane1"], FieldState::Flag(true));
        assert_eq!(f.fields["mesh_mode"], FieldState::Choice(0));
        let set = f.to_params(&m).unwrap();
        let p = PlanarParams::from_set(&set).unwrap();
        let d = PlanarParams::default();
        assert!(
            (p.t - d.t).abs() < 1e-15 && (p.sigma - d.sigma).abs() < 1.0 && (p.dmin - d.dmin).abs() < 1e-15
        );
        assert_eq!(p.max_nodes, d.max_nodes);
    }

    #[test]
    fn unit_change_converts_the_shown_value() {
        let m = PlanarLine;
        let mut f = FormState::new(&m);
        f.change_unit(&m, "w", "mil");
        assert_eq!(text(&f, "w").1, "mil");
        assert_eq!(text(&f, "w").0, format_g(0.3605 / 0.0254));
        f.change_unit(&m, "w", "µm");
        assert_eq!(text(&f, "w").0, "360.5");
        // internal value unchanged by unit switching
        assert!((f.to_params(&m).unwrap().num("w").unwrap() - 0.3605).abs() < 1e-12);
        // editing in the new unit, decimal comma accepted
        if let Some(FieldState::Quantity { text, .. }) = f.fields.get_mut("w") {
            *text = "400,5".into();
        }
        assert!((f.to_params(&m).unwrap().num("w").unwrap() - 0.4005).abs() < 1e-12);
        // unknown unit and unknown key are ignored, unparsable text survives a unit change
        f.change_unit(&m, "w", "parsec");
        f.change_unit(&m, "nonexistent", "mm");
        assert_eq!(text(&f, "w").1, "µm");
        if let Some(FieldState::Quantity { text, .. }) = f.fields.get_mut("w") {
            *text = "abc".into();
        }
        f.change_unit(&m, "w", "mm");
        assert_eq!(text(&f, "w"), ("abc".into(), "mm".into()));
        // plain numbers have a fixed unit
        f.change_unit(&m, "sigma", "S/m");
        assert_eq!(text(&f, "sigma").1, "MS/m");
    }

    #[test]
    fn lambda_units_refer_to_the_wavelength_of_the_other_fields() {
        let m = PlanarLine;
        let mut f = FormState::new(&m);
        assert!(unit_options(m.params().iter().find(|p| p.key == "dmin").unwrap()).contains(&"λ"));
        assert!(!unit_options(m.params().iter().find(|p| p.key == "w").unwrap()).contains(&"λ"));
        f.change_unit(&m, "dmin", "λ");
        let lam = m.wavelength_mm(&m.defaults()).unwrap();
        assert_eq!(text(&f, "dmin").0, format_g(0.002 / lam));
        if let Some(FieldState::Quantity { text, .. }) = f.fields.get_mut("dmin") {
            *text = "0.001".into();
        }
        let set = f.to_params(&m).unwrap();
        assert!((set.num("dmin").unwrap() - 0.001 * lam).abs() < 1e-12);
        // changing the frequency moves the wavelength and with it the absolute size
        if let Some(FieldState::Quantity { text, .. }) = f.fields.get_mut("f") {
            *text = "2".into();
        }
        assert!((f.to_params(&m).unwrap().num("dmin").unwrap() - 0.001 * lam / 2.0).abs() < 1e-12);
    }

    #[test]
    fn errors_name_the_field() {
        let m = PlanarLine;
        let mut f = FormState::new(&m);
        if let Some(FieldState::Quantity { text, .. }) = f.fields.get_mut("h1") {
            *text = "".into();
        }
        let e = f.to_params(&m).unwrap_err();
        assert_eq!((e.key, e.label), ("h1", "Height h1"));
        assert!(e.to_string().starts_with("Height h1:"));
        // λ without a usable wavelength (in a field that is shown: the manual mesh sizes)
        let mut f = FormState::new(&m);
        f.fields.insert("mesh_mode", FieldState::Choice(1));
        f.change_unit(&m, "dmin", "λ");
        if let Some(FieldState::Quantity { text, .. }) = f.fields.get_mut("f") {
            *text = "0".into();
        }
        assert_eq!(f.to_params(&m).unwrap_err().message, "no wavelength reference");
    }

    #[test]
    fn set_params_keeps_units_and_save_load_round_trips() {
        let m = PlanarLine;
        let mut f = FormState::new(&m);
        f.change_unit(&m, "t", "mil");
        f.change_unit(&m, "f", "MHz");
        let mut set = m.defaults();
        set.set("t", ParamValue::Num(0.0508));
        f.set_params(&m, &set);
        assert_eq!(text(&f, "t"), ("2".into(), "mil".into()));
        assert_eq!(text(&f, "f").1, "MHz");

        if let Some(FieldState::Flag(b)) = f.fields.get_mut("plane1") {
            *b = false;
        }
        f.fields.insert("mesh_mode", FieldState::Choice(1));
        let saved = f.to_saved();
        let json = serde_json::to_string(&saved).unwrap();
        let mut g = FormState::new(&m);
        g.load_saved(&m, &serde_json::from_str(&json).unwrap());
        assert_eq!(g, f);
    }

    #[test]
    fn loading_ignores_garbage() {
        let m = PlanarLine;
        let mut f = FormState::new(&m);
        let before = f.clone();
        let mut saved = BTreeMap::new();
        saved.insert("nonexistent".to_string(), SavedField { value: Some("1".into()), ..Default::default() });
        saved.insert("w".to_string(), SavedField { unit: Some("parsec".into()), ..Default::default() });
        saved.insert("sigma".to_string(), SavedField { unit: Some("S/m".into()), ..Default::default() });
        saved.insert("mesh_mode".to_string(), SavedField { choice: Some(9), ..Default::default() });
        saved.insert("plane1".to_string(), SavedField { value: Some("x".into()), ..Default::default() });
        f.load_saved(&m, &saved);
        assert_eq!(f, before);
    }

    #[test]
    fn a_hidden_field_cannot_block_and_a_preset_keeps_the_dimensions() {
        let m = PlanarLine;
        let mut f = FormState::new(&m);
        let type_in = |f: &mut FormState, key: &'static str, value: &str| {
            let Some(FieldState::Quantity { text, .. }) = f.fields.get_mut(key) else { panic!("{key}") };
            *text = value.to_string();
        };
        type_in(&mut f, "w", "0.2");
        type_in(&mut f, "s", "abc");
        assert_eq!(f.to_params(&m).unwrap_err().key, "s");
        // without a coplanar ground the gap is hidden: the analysis runs, the text stays
        let stripline = m.presets().iter().find(|p| p.id == "stripline").unwrap();
        f.apply_preset(stripline);
        let set = f.to_params(&m).unwrap();
        assert_eq!(set.num("w").unwrap(), 0.2);
        assert_eq!(set.num("s").unwrap(), PlanarParams::default().s);
        assert!(!set.flag("coplanar").unwrap());
        assert_eq!(text(&f, "s").0, "abc");
        // back on, the field is checked again
        f.apply_preset(&m.presets()[0]);
        assert_eq!(f.to_params(&m).unwrap_err().key, "s");
    }
}
