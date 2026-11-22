//! Loss-versus-frequency tab.

use std::collections::BTreeMap;

use eframe::egui::{self, Color32, vec2};
use egui_plot::{AxisHints, HPlacement, Line, LineStyle, Plot, PlotBounds, PlotPoints, VLine};
use transverse_core::NP2DB;
use transverse_core::losses::{FreqResponse, sweep};
use transverse_core::units::{FREQ_UNITS, factor, format_g, parse_number};

use super::legend::{self, LegendEntry};
use super::nav;

const POINTS: usize = 400;
const DEFAULT_FMAX_HZ: f64 = 20e9;
/// Two lines of values under the plot.
const READOUT_HEIGHT: f32 = 44.0;
/// Curve colours (matplotlib C0, C1; the total uses the text colour).
const CONDUCTOR: Color32 = Color32::from_rgb(31, 119, 180);
const DIELECTRIC: Color32 = Color32::from_rgb(255, 127, 14);

pub struct LossView {
    fmax_text: String,
    fmax_unit: String,
    /// View requested by the Fit button, applied in the next plot pass.
    pending: Option<[f64; 4]>,
}

impl Default for LossView {
    fn default() -> Self {
        Self { fmax_text: "20".into(), fmax_unit: "GHz".into(), pending: None }
    }
}

/// Curves in dB/mm over `fmax`: (GHz, conductor, dielectric, total).
pub fn curves(freq: &dyn FreqResponse, fmax_hz: f64) -> Vec<(f64, f64, f64, f64)> {
    let to_db_mm = NP2DB / 1000.0;
    let fs = sweep(fmax_hz, POINTS);
    freq.sample(&fs)
        .iter()
        .zip(&fs)
        .map(|(p, f)| {
            (f / 1e9, p.alpha_c * to_db_mm, p.alpha_d * to_db_mm, (p.alpha_c + p.alpha_d) * to_db_mm)
        })
        .collect()
}

impl LossView {
    /// The "f max" entry [Hz]; an invalid entry falls back to 20 GHz.
    pub fn fmax_hz(&self) -> f64 {
        let v =
            parse_number(&self.fmax_text).zip(factor(&FREQ_UNITS, &self.fmax_unit, None)).map(|(v, f)| v * f);
        v.filter(|v| *v > 0.0).unwrap_or(DEFAULT_FMAX_HZ)
    }

    /// Upper end of the plot: the entry, but never below `floor_hz` (the analysis frequency and the
    /// frequency typed in the parameter form), so that frequency is always inside the plot.
    pub fn effective_fmax_hz(&self, floor_hz: f64) -> f64 {
        self.fmax_hz().max(if floor_hz.is_finite() { floor_hz } else { 0.0 })
    }

    pub fn load(&mut self, ui: &BTreeMap<String, serde_json::Value>) {
        if let Some(t) = ui.get("fmax_text").and_then(|v| v.as_str()) {
            self.fmax_text = t.to_string();
        }
        if let Some(u) =
            ui.get("fmax_unit").and_then(|v| v.as_str()).filter(|u| FREQ_UNITS.iter().any(|f| f.0 == *u))
        {
            self.fmax_unit = u.to_string();
        }
    }

    pub fn save(&self, ui: &mut BTreeMap<String, serde_json::Value>) {
        ui.insert("fmax_text".into(), self.fmax_text.clone().into());
        ui.insert("fmax_unit".into(), self.fmax_unit.clone().into());
    }

    /// `f_hz`: analysis frequency; `form_f_hz`: the frequency currently typed in the parameter form.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        freq: &dyn FreqResponse,
        z0: f64,
        f_hz: f64,
        form_f_hz: Option<f64>,
        generation: u64,
    ) {
        let floor = f_hz.max(form_f_hz.unwrap_or(0.0));
        let fmax = self.effective_fmax_hz(floor);
        ui.horizontal(|ui| {
            ui.label("f max");
            let bad = parse_number(&self.fmax_text).is_none();
            ui.add_sized(
                [90.0, ui.spacing().interact_size.y],
                egui::TextEdit::singleline(&mut self.fmax_text).text_color_opt(bad.then_some(Color32::RED)),
            );
            egui::ComboBox::from_id_salt("fmax_unit").selected_text(&self.fmax_unit).width(48.0).show_ui(
                ui,
                |ui| {
                    for (u, _) in FREQ_UNITS {
                        ui.selectable_value(&mut self.fmax_unit, u.to_string(), u);
                    }
                },
            );
            if fmax > self.fmax_hz() {
                ui.weak(format!("plot extended to {} GHz, the set frequency", format_g(fmax / 1e9)));
            }
            if ui.button("Fit").clicked() {
                self.pending = Some([0.0, fmax / 1e9, 0.0, 0.0]); // y filled in below
            }
        });
        ui.label(format!("Loss vs frequency (Z0 = {z0:.2} Ohm; er, tan d constant)"));

        let c = curves(freq, fmax);
        let top = 1.05 * c.iter().map(|r| r.3).fold(0.0, f64::max);
        let pending = self.pending.take().map(|v| [v[0], v[1], v[2], top.max(f64::MIN_POSITIVE)]);
        let series = |pick: fn(&(f64, f64, f64, f64)) -> f64| {
            PlotPoints::from(c.iter().map(|r| [r.0, pick(r)]).collect::<Vec<_>>())
        };
        let total_colour = ui.visuals().text_color();
        let plot = Plot::new(("loss", generation, fmax.to_bits()))
            .height((ui.available_height() - READOUT_HEIGHT).max(160.0))
            .x_axis_label("Frequency [GHz]")
            .custom_y_axes(vec![
                AxisHints::new_y().label("Loss [dB/mm]"),
                AxisHints::new_y()
                    .label("Loss [dB/in]")
                    .placement(HPlacement::Right)
                    .formatter(|mark, _| format_g(mark.value * 25.4)),
            ])
            .default_x_bounds(0.0, fmax / 1e9)
            .include_y(0.0);
        let resp = nav::configure(plot).show(ui, |pui| {
            nav::wheel_zoom(pui);
            if let Some(v) = pending {
                pui.set_plot_bounds(PlotBounds::from_min_max([v[0], v[2]], [v[1], v[3]]));
            }
            pui.line(Line::new("Conductor", series(|r| r.1)).color(CONDUCTOR).width(1.6));
            pui.line(Line::new("Dielectric", series(|r| r.2)).color(DIELECTRIC).width(1.6));
            pui.line(Line::new("Total", series(|r| r.3)).color(total_colour).width(2.4));
            pui.add(
                VLine::new("", f_hz / 1e9).color(Color32::GRAY).width(0.9).style(LineStyle::dashed_loose()),
            );
        });
        let entry =
            |label: &str, color, width| LegendEntry { label: label.into(), color, dashed: false, width };
        legend::draw(
            ui,
            *resp.transform.frame(),
            0.0,
            &[
                entry("Conductor", CONDUCTOR, 1.6),
                entry("Dielectric", DIELECTRIC, 1.6),
                entry("Total", total_colour, 2.4),
            ],
        );

        // loss at the cursor frequency, evaluated exactly (not read off the sampled curves)
        let text = match nav::hover_x(&resp).filter(|x| *x > 0.0) {
            Some(x) => {
                let p = freq.at(x * 1e9);
                let (ac, ad) = (p.alpha_c * NP2DB / 1000.0, p.alpha_d * NP2DB / 1000.0);
                let v =
                    |name: &str, db_mm: f64| format!("{name}: {db_mm:.5} dB/mm ({:.4} dB/in)", db_mm * 25.4);
                format!(
                    "f = {} GHz      {}      {}      {}",
                    format_g(x),
                    v("Conductor", ac),
                    v("Dielectric", ad),
                    v("Total", ac + ad)
                )
            }
            None => "Move the cursor over the plot for the losses at that frequency.".to_string(),
        };
        ui.add_sized(vec2(ui.available_width(), READOUT_HEIGHT - 4.0), egui::Label::new(text).wrap());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use transverse_core::losses::LossModel;

    #[test]
    fn curves_cover_the_sweep_and_add_up() {
        let m = LossModel { z0: 50.0, eeff: 3.5, g_over_omega: 1e-12, dzdn: 1e6, sigma: 5.8e7, rq_mm: 0.0 };
        let c = curves(&m, 10e9);
        assert_eq!(c.len(), POINTS);
        assert!((c[0].0 - 10.0 / POINTS as f64).abs() < 1e-12 && (c[POINTS - 1].0 - 10.0).abs() < 1e-12);
        assert!(c.iter().all(|r| (r.3 - r.1 - r.2).abs() < 1e-15));
        // conductor loss ~ sqrt(f), dielectric loss ~ f
        let (a, b) = (c[99], c[399]); // 2.5 GHz and 10 GHz
        assert!((b.1 / a.1 - 2.0).abs() < 1e-9 && (b.2 / a.2 - 4.0).abs() < 1e-9);
    }

    #[test]
    fn fmax_entry_is_parsed_with_its_unit_and_falls_back_when_invalid() {
        let mut v = LossView::default();
        assert_eq!(v.fmax_hz(), 20e9);
        v.fmax_text = "500".into();
        v.fmax_unit = "MHz".into();
        assert_eq!(v.fmax_hz(), 500e6);
        v.fmax_text = "5,5".into();
        v.fmax_unit = "GHz".into();
        assert_eq!(v.fmax_hz(), 5.5e9);
        for bad in ["", "abc", "-3", "0"] {
            v.fmax_text = bad.into();
            assert_eq!(v.fmax_hz(), 20e9, "{bad:?}");
        }
        // settings round trip, garbage units ignored
        v.fmax_text = "8".into();
        let mut ui = BTreeMap::new();
        v.save(&mut ui);
        let mut w = LossView::default();
        w.load(&ui);
        assert_eq!(w.fmax_hz(), 8e9);
        ui.insert("fmax_unit".into(), "parsec".into());
        w.load(&ui);
        assert_eq!(w.fmax_unit, "GHz");
    }

    #[test]
    fn plot_range_never_ends_below_the_set_frequency() {
        let v = LossView { fmax_text: "5".into(), ..Default::default() };
        // entry larger than the set frequency: the entry wins
        assert_eq!(v.effective_fmax_hz(2e9), 5e9);
        // set frequency larger than the entry: it wins
        assert_eq!(v.effective_fmax_hz(12e9), 12e9);
        // exactly equal
        assert_eq!(v.effective_fmax_hz(5e9), 5e9);
        // nonsense floors do not break the range
        assert_eq!(v.effective_fmax_hz(f64::NAN), 5e9);
        assert_eq!(v.effective_fmax_hz(-1.0), 5e9);
    }
}
