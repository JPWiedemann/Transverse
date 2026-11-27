//! WebAssembly layer: the line models for the web frontend. No logic of its own.
//!
//! The contract mirrors `transverse-lines`: the form is generated from `param_specs`, a parameter
//! set goes in as JSON (`{"w": 0.36, "plane1": true, ...}`, lengths in mm, frequencies in Hz) and an
//! [`AnalysisHandle`] comes out. Small results are JSON strings, bulk data (potentials, loss sweeps)
//! are typed arrays. Everything runs single-threaded; cancelling is done by the host terminating the
//! Web Worker that runs this module.

use std::cell::OnceCell;
use std::sync::Arc;

use serde::Serialize;
use transverse_core::postproc::EField;
use transverse_core::units::UnitSystem;
use transverse_core::{Ctl, TvError};
use transverse_lines::model::{Analysis, LineModel, ModeResult, ParamSet};
use transverse_lines::render::colormap::Cmap;
use transverse_lines::render::raster::{self, FieldMode, Legend, RasterSpec};
use transverse_lines::{find_model, registry};
use wasm_bindgen::prelude::*;

fn js_err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

fn json<T: Serialize + ?Sized>(v: &T) -> Result<String, JsError> {
    serde_json::to_string(v).map_err(js_err)
}

fn model(line_id: &str) -> Result<Arc<dyn LineModel>, JsError> {
    find_model(line_id).ok_or_else(|| js_err(format!("unknown line type '{line_id}'")))
}

fn param_set(params_json: &str) -> Result<ParamSet, JsError> {
    serde_json::from_str(params_json).map_err(|e| js_err(format!("parameters: {e}")))
}

/// A JS callback that receives progress messages. The wasm module is single-threaded, so the
/// `Send + Sync` the core asks for holds trivially.
struct JsProgress(js_sys::Function);
unsafe impl Send for JsProgress {}
unsafe impl Sync for JsProgress {}

fn ctl(on_progress: Option<js_sys::Function>) -> Ctl {
    match on_progress {
        Some(f) => {
            let f = JsProgress(f);
            Ctl::new().with_progress(move |msg| {
                let _ = f.0.call1(&JsValue::NULL, &JsValue::from_str(msg));
            })
        }
        None => Ctl::new(),
    }
}

#[derive(Serialize)]
struct LineInfo {
    id: &'static str,
    name: &'static str,
    synth_targets: &'static [transverse_lines::model::SynthTarget],
}

/// `[{id, name, synth_targets: [{id, label}]}]` for all line types.
#[wasm_bindgen]
pub fn line_types() -> Result<String, JsError> {
    let list: Vec<LineInfo> = registry()
        .iter()
        .map(|m| LineInfo { id: m.id(), name: m.name(), synth_targets: m.synth_targets() })
        .collect();
    json(&list)
}

/// The parameter form of a line type (`ParamSpec` list).
#[wasm_bindgen]
pub fn param_specs(line_id: &str) -> Result<String, JsError> {
    json(model(line_id)?.params())
}

/// Default values of all parameters.
#[wasm_bindgen]
pub fn defaults(line_id: &str) -> Result<String, JsError> {
    json(&model(line_id)?.defaults())
}

/// Wavelength [mm] for sizes given in λ, `undefined` if the line type has none.
#[wasm_bindgen]
pub fn wavelength_mm(line_id: &str, params_json: &str) -> Result<Option<f64>, JsError> {
    Ok(model(line_id)?.wavelength_mm(&param_set(params_json)?))
}

/// Cross-section sketch (`Schematic` as JSON, `null` if the line type has none).
#[wasm_bindgen]
pub fn schematic(line_id: &str, params_json: &str) -> Result<String, JsError> {
    json(&model(line_id)?.schematic(&param_set(params_json)?))
}

/// Analyse one geometry. `on_progress` receives status texts.
#[wasm_bindgen]
pub fn analyze(
    line_id: &str,
    params_json: &str,
    on_progress: Option<js_sys::Function>,
) -> Result<AnalysisHandle, JsError> {
    let p = param_set(params_json)?;
    let analysis = model(line_id)?.analyze(&p, &ctl(on_progress)).map_err(js_err)?;
    Ok(AnalysisHandle { analysis, params: None, e: OnceCell::new() })
}

/// Find the dimension `target_id` that gives `z0_target` Ohm. With `phase_deg` the line length is set
/// for that electrical length as well (`undefined` keeps it). The handle carries the new parameters.
#[wasm_bindgen]
pub fn synthesize(
    line_id: &str,
    params_json: &str,
    target_id: &str,
    z0_target: f64,
    phase_deg: Option<f64>,
    on_progress: Option<js_sys::Function>,
) -> Result<AnalysisHandle, JsError> {
    let p = param_set(params_json)?;
    let (new_p, analysis) =
        model(line_id)?.synthesize(&p, target_id, z0_target, phase_deg, &ctl(on_progress)).map_err(js_err)?;
    Ok(AnalysisHandle { analysis, params: Some(new_p), e: OnceCell::new() })
}

#[derive(Serialize)]
struct Summary<'a> {
    modes: &'a [ModeResult],
    report: &'a str,
    accuracy: &'a Option<transverse_core::quasistatic::Accuracy>,
    has_field: bool,
}

#[derive(Serialize)]
struct FieldInfo<'a> {
    nx: usize,
    ny: usize,
    /// Solution on a triangle mesh (`nx` and `ny` are then 2: only the extent).
    mesh: bool,
    z0: f64,
    conductors: &'a [transverse_lines::model::DrawRect],
    dashed_y: &'a [f64],
    dotted_y: &'a [f64],
    view_zoom: [f64; 4],
    view_full: [f64; 4],
}

/// A rendered field image: RGBA pixels, first row = top of the view (largest y).
#[wasm_bindgen]
pub struct FieldRaster {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    legend: Option<Legend>,
}

#[wasm_bindgen]
impl FieldRaster {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// `width * height * 4` bytes.
    pub fn rgba(&self) -> Vec<u8> {
        self.rgba.clone()
    }

    /// Colour bar (`{label, vmin, vmax, cmap, field_base}`) as JSON, `null` for the mesh view.
    pub fn legend(&self) -> Result<String, JsError> {
        json(&self.legend)
    }
}

/// The 256 colours of a colour map ("Inferno", "Viridis", "Greys") as RGB bytes, for colour bars.
#[wasm_bindgen]
pub fn colormap(name: &str) -> Result<Vec<u8>, JsError> {
    let cmap = Cmap::from_name(name).ok_or_else(|| js_err(format!("unknown colour map '{name}'")))?;
    Ok((0..256).flat_map(|i| cmap.sample(f64::from(i) / 255.0)).collect())
}

/// Column order of [`AnalysisHandle::sample`]; a row is one frequency.
pub const SAMPLE_COLUMNS: [&str; 9] = ["z0", "beta", "alpha_c", "alpha_d", "r", "g", "rs", "krough", "delta"];

/// The result of an analysis or synthesis, kept inside the module; the frontend asks for what it shows.
#[wasm_bindgen]
pub struct AnalysisHandle {
    analysis: Analysis,
    params: Option<ParamSet>,
    /// E field of the dielectric solution, computed on first use.
    e: OnceCell<EField>,
}

#[wasm_bindgen]
impl AnalysisHandle {
    /// Modes, report text, mesh accuracy (JSON).
    pub fn summary(&self) -> Result<String, JsError> {
        json(&Summary {
            modes: &self.analysis.modes,
            report: &self.analysis.report,
            accuracy: &self.analysis.accuracy,
            has_field: self.analysis.field.is_some(),
        })
    }

    /// Parameters found by a synthesis (JSON), `undefined` after a plain analysis.
    pub fn params(&self) -> Result<Option<String>, JsError> {
        self.params.as_ref().map(json).transpose()
    }

    /// Loss values at the given frequencies [Hz]: a flat array with [`SAMPLE_COLUMNS`] per row.
    pub fn sample(&self, freqs: &[f64]) -> Vec<f64> {
        let mut out = Vec::with_capacity(freqs.len() * SAMPLE_COLUMNS.len());
        for p in self.analysis.freq.sample(freqs) {
            out.extend([p.z0, p.beta, p.alpha_c, p.alpha_d, p.r, p.g, p.rs, p.krough, p.delta]);
        }
        out
    }

    /// Column names of [`sample`](Self::sample).
    pub fn sample_columns() -> Vec<String> {
        SAMPLE_COLUMNS.iter().map(|s| s.to_string()).collect()
    }

    /// Grid size, conductors, guide lines and view windows of the field (JSON), `null` without field.
    pub fn field_info(&self) -> Result<String, JsError> {
        json(&self.analysis.field.as_ref().map(|f| FieldInfo {
            nx: f.nx(),
            ny: f.ny(),
            mesh: f.mesh.is_some(),
            z0: f.z0,
            conductors: &f.conductors,
            dashed_y: &f.dashed_y,
            dotted_y: &f.dotted_y,
            view_zoom: f.view_zoom,
            view_full: f.view_full,
        }))
    }

    fn field(&self) -> Result<&transverse_lines::model::FieldData, JsError> {
        self.analysis.field.as_ref().ok_or_else(|| js_err(TvError::InvalidParam("no field data".into())))
    }

    /// Grid lines in x [mm].
    pub fn field_x(&self) -> Result<Vec<f64>, JsError> {
        Ok(self.field()?.x.clone())
    }

    /// Grid lines in y [mm].
    pub fn field_y(&self) -> Result<Vec<f64>, JsError> {
        Ok(self.field()?.y.clone())
    }

    /// Potential with dielectrics, `nx * ny` values, x fastest (for 1 V on the signal).
    pub fn field_v(&self) -> Result<Vec<f64>, JsError> {
        Ok(self.field()?.v.clone())
    }

    /// Potential in air.
    pub fn field_v0(&self) -> Result<Vec<f64>, JsError> {
        Ok(self.field()?.v0.clone())
    }

    /// Relative permittivity per cell, `(nx-1) * (ny-1)`.
    pub fn field_er(&self) -> Result<Vec<f64>, JsError> {
        Ok(self.field()?.er.clone())
    }

    /// 1 for conductor nodes, `nx * ny` values.
    pub fn field_fixed(&self) -> Result<Vec<u8>, JsError> {
        Ok(self.field()?.fixed.iter().map(|&b| u8::from(b)).collect())
    }

    fn e_field(&self) -> Result<&EField, JsError> {
        let f = self.field()?;
        Ok(self.e.get_or_init(|| f.e_field(false)))
    }

    /// Field image of `mode` (a `FieldMode` key such as "e_db_lines") over `view = [x0, x1, y0, y1]`
    /// mm, `width` pixels wide. The colour scale comes from `scale_view` (same form), so it stays
    /// stable while the drawn region changes. `imperial` selects V/mil instead of V/m.
    pub fn field_raster(
        &self,
        mode: &str,
        view: &[f64],
        scale_view: &[f64],
        imperial: bool,
        db_range: f64,
        width: u32,
    ) -> Result<FieldRaster, JsError> {
        let mode = FieldMode::from_key(mode).ok_or_else(|| js_err(format!("unknown field mode '{mode}'")))?;
        let bounds = |v: &[f64]| <[f64; 4]>::try_from(v).map_err(|_| js_err("a view needs 4 numbers"));
        let spec = RasterSpec {
            mode,
            view: bounds(view)?,
            scale_view: bounds(scale_view)?,
            system: if imperial { UnitSystem::Imperial } else { UnitSystem::Metric },
            db_range,
            width: width as usize,
        };
        let r = raster::render(self.field()?, self.e_field()?, &spec);
        Ok(FieldRaster { width: r.width as u32, height: r.height as u32, rgba: r.rgba, legend: r.legend })
    }

    /// Values at (`x`, `y`) mm as JSON `{v, e, er, in_conductor}` (`e` in V/m, `null` inside a
    /// conductor), `null` outside the grid.
    pub fn field_probe(&self, x: f64, y: f64) -> Result<String, JsError> {
        json(&raster::probe(self.field()?, self.e_field()?, x, y))
    }

    /// Field lines inside `view = [x0, x1, y0, y1]` as JSON: a list of polylines of `[x, y]` points.
    pub fn field_lines(&self, view: &[f64], density_x: f64, density_y: f64) -> Result<String, JsError> {
        let view: [f64; 4] = view.try_into().map_err(|_| js_err("view needs 4 numbers"))?;
        json(&self.field()?.field_lines(view, (density_x, density_y)))
    }

    /// Triangle edges that touch `view = [x0, x1, y0, y1]` as a flat array `x0, y0, x1, y1, ...` (mm): the mesh of
    /// a finite-element solution. Empty for a grid solution, whose lines are `field_x` and `field_y`.
    pub fn field_mesh_edges(&self, view: &[f64]) -> Result<Vec<f64>, JsError> {
        let view: [f64; 4] = view.try_into().map_err(|_| js_err("view needs 4 numbers"))?;
        Ok(self.field()?.mesh_edges(view).unwrap_or_default().into_iter().flatten().collect())
    }

    /// Equipotential lines for the given potentials [V] as JSON: `[[level, [[[x, y], [x, y]], ...]], ...]`.
    pub fn equipotentials(&self, levels: &[f64]) -> Result<String, JsError> {
        json(&self.field()?.equipotentials(levels))
    }
}
