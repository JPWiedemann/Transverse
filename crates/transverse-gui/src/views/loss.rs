//! Loss-versus-frequency tab.

use std::collections::BTreeMap;

use eframe::egui::{self, Color32, vec2};
use egui_plot::{
    AxisHints, HPlacement, Line, LineStyle, Plot, PlotBounds, PlotPoints, VLine, uniform_grid_spacer,
};
use transverse_core::NP2DB;
use transverse_core::losses::{FreqResponse, sweep};
use transverse_core::units::{FREQ_UNITS, UnitSystem, factor, format_g, grid_steps_mm, parse_number};

use super::legend::{self, LegendEntry};
use super::nav;

const POINTS: usize = 400;
const DEFAULT_FMAX_HZ: f64 = 20e9;
/// Two lines of values under the plot.
const READOUT_HEIGHT: f32 = 44.0;
/// Curve colours (matplotlib C0, C1; the total uses the text colour).
const CONDUCTOR: Color32 = Color32::from_rgb(31, 119, 180);
const DIELECTRIC: Color32 = Color32::from_rgb(255, 127, 14);

/// What the loss tab shows.
#[derive(Clone, Copy)]
pub struct LossData<'a> {
    pub freq: &'a dyn FreqResponse,
    pub z0: f64,
    /// Analysis frequency [Hz].
    pub f_hz: f64,
    /// The frequency currently typed in the parameter form [Hz], if it parses.
    pub form_f_hz: Option<f64>,
    pub units: UnitSystem,
    /// Counts results; the plot resets its bounds when it changes.
    pub generation: u64,
}

pub struct LossView {
    fmax_text: String,
    fmax_unit: String,
    /// View requested by a button, applied in the next plot pass.
    pending: Option<[f64; 4]>,
    /// Visible bounds as of the last frame.
    bounds: Option<[f64; 4]>,
}

impl Default for LossView {
    fn default() -> Self {
        Self { fmax_text: "20".into(), fmax_unit: "GHz".into(), pending: None, bounds: None }
    }
}

/// (label, factor from the plotted dB/mm) of the left axis and of the right axis.
fn loss_axes(units: UnitSystem) -> (&'static str, f64, &'static str, f64) {
    match units {
        UnitSystem::Metric => ("Loss [dB/mm]", 1.0, "Loss [dB/in]", 25.4),
        UnitSystem::Imperial => ("Loss [dB/in]", 25.4, "Loss [dB/mm]", 1.0),
    }
}

/// Losses at `f_ghz` for the cursor readout, evaluated exactly: the unit system's unit first.
pub fn readout_text(freq: &dyn FreqResponse, f_ghz: f64, units: UnitSystem) -> String {
    let p = freq.at(f_ghz * 1e9);
    let (ac, ad) = (p.alpha_c * NP2DB / 1000.0, p.alpha_d * NP2DB / 1000.0);
    let (_, k1, _, k2) = loss_axes(units);
    let (u1, u2) = match units {
        UnitSystem::Metric => ("dB/mm", "dB/in"),
        UnitSystem::Imperial => ("dB/in", "dB/mm"),
    };
    let v = |name: &str, db_mm: f64| format!("{name}: {:.5} {u1} ({:.4} {u2})", db_mm * k1, db_mm * k2);
    format!(
        "f = {} GHz      {}      {}      {}",
        format_g(f_ghz),
        v("Conductor", ac),
        v("Dielectric", ad),
        v("Total", ac + ad)
    )
}

/// Top of the y range that fits the total loss between `f0_ghz` and `f1_ghz`: 5 % above the maximum.
pub fn y_fit(freq: &dyn FreqResponse, f0_ghz: f64, f1_ghz: f64) -> f64 {
    let to_db_mm = NP2DB / 1000.0;
    let (lo, hi) = (f0_ghz.max(0.0), f1_ghz.max(f0_ghz.max(0.0)));
    let m = (0..=60)
        .map(|k| lo + (hi - lo) * k as f64 / 60.0)
        .filter(|f| *f > 0.0)
        .map(|f| {
            let p = freq.at(f * 1e9);
            (p.alpha_c + p.alpha_d) * to_db_mm
        })
        .fold(0.0, f64::max);
    if m > 0.0 { 1.05 * m } else { 1.0 }
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

    pub fn ui(&mut self, ui: &mut egui::Ui, data: &LossData<'_>) {
        let LossData { freq, z0, f_hz, form_f_hz, units, generation } = *data;
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
            if ui
                .button("Fit")
                .on_hover_text("keep the frequency range, scale y to the curves in it")
                .clicked()
            {
                let [x0, x1, ..] = self.bounds.unwrap_or([0.0, fmax / 1e9, 0.0, 0.0]);
                self.pending = Some([x0, x1, 0.0, y_fit(freq, x0, x1)]);
            }
            if ui.button("Full range").on_hover_text("0 to f max").clicked() {
                self.pending = Some([0.0, fmax / 1e9, 0.0, y_fit(freq, 0.0, fmax / 1e9)]);
            }
        });
        ui.label(format!("Loss vs frequency (Z0 = {z0:.2} Ohm; er, tan d constant)"));

        let c = curves(freq, fmax);
        let pending = self.pending.take();
        let series = |pick: fn(&(f64, f64, f64, f64)) -> f64| {
            PlotPoints::from(c.iter().map(|r| [r.0, pick(r)]).collect::<Vec<_>>())
        };
        let total_colour = ui.visuals().text_color();
        // the primary (left) axis follows the unit system, the other one shows the second unit
        let (left_label, k_left, right_label, k_right) = loss_axes(units);
        let plot = Plot::new(("loss", generation, fmax.to_bits()))
            .height((ui.available_height() - READOUT_HEIGHT).max(160.0))
            .x_axis_label("Frequency [GHz]")
            .custom_y_axes(vec![
                AxisHints::new_y().label(left_label).formatter(move |mark, _| format_g(mark.value * k_left)),
                AxisHints::new_y()
                    .label(right_label)
                    .placement(HPlacement::Right)
                    .formatter(move |mark, _| format_g(mark.value * k_right)),
            ])
            .y_grid_spacer(uniform_grid_spacer(move |g| grid_steps_mm(g.base_step_size, 1.0 / k_left)))
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
        let b = resp.transform.bounds();
        self.bounds = Some([b.min()[0], b.max()[0], b.min()[1], b.max()[1]]);
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
            Some(x) => readout_text(freq, x, units),
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
    fn readout_leads_with_the_unit_of_the_selected_system() {
        let m = LossModel { z0: 50.0, eeff: 3.5, g_over_omega: 1e-12, dzdn: 1e6, sigma: 5.8e7, rq_mm: 0.0 };
        let metric = readout_text(&m, 2.0, UnitSystem::Metric);
        let imperial = readout_text(&m, 2.0, UnitSystem::Imperial);
        assert!(metric.starts_with("f = 2 GHz") && metric.contains("Total: "), "{metric}");
        let p = m.at(2e9);
        let total_mm = (p.alpha_c + p.alpha_d) * NP2DB / 1000.0;
        assert!(
            metric.contains(&format!("Total: {:.5} dB/mm ({:.4} dB/in)", total_mm, total_mm * 25.4)),
            "{metric}"
        );
        assert!(
            imperial.contains(&format!("Total: {:.5} dB/in ({:.4} dB/mm)", total_mm * 25.4, total_mm)),
            "{imperial}"
        );
        assert_eq!(loss_axes(UnitSystem::Imperial).0, "Loss [dB/in]");
        assert_eq!(loss_axes(UnitSystem::Metric).2, "Loss [dB/in]");
    }

    #[test]
    fn the_plot_reports_its_bounds_so_fit_can_use_them() {
        use eframe::egui::{Pos2, Rect, vec2};
        let ctx = egui::Context::default();
        let m = LossModel { z0: 50.0, eeff: 3.5, g_over_omega: 1e-12, dzdn: 1e6, sigma: 5.8e7, rq_mm: 0.0 };
        let mut view = LossView::default();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0))),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    view.ui(
                        ui,
                        &LossData {
                            freq: &m,
                            z0: 50.0,
                            f_hz: 1e9,
                            form_f_hz: None,
                            units: UnitSystem::Metric,
                            generation: 0,
                        },
                    )
                });
            });
        }
        let b = view.bounds.expect("bounds are stored after drawing");
        assert!(b[0] <= 0.0 + 1e-9 && b[1] >= 20.0 - 1e-9, "the default range 0..20 GHz is visible: {b:?}");
        assert!(b[3] > b[2]);
    }

    #[test]
    fn fit_follows_the_curves_in_the_visible_range() {
        let m = LossModel { z0: 50.0, eeff: 3.5, g_over_omega: 1e-12, dzdn: 1e6, sigma: 5.8e7, rq_mm: 0.0 };
        let total = |f_ghz: f64| {
            let p = m.at(f_ghz * 1e9);
            (p.alpha_c + p.alpha_d) * NP2DB / 1000.0
        };
        // the loss grows with frequency: the top is 5 % above the value at the right edge
        assert!((y_fit(&m, 0.0, 10.0) - 1.05 * total(10.0)).abs() < 1e-12);
        assert!((y_fit(&m, 2.0, 4.0) - 1.05 * total(4.0)).abs() < 1e-12);
        // a range left of zero or an empty one gives a sane value
        assert!(y_fit(&m, -5.0, -1.0) > 0.0 && y_fit(&m, 3.0, 3.0) > 0.0);
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
