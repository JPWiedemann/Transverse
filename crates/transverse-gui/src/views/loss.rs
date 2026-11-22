//! Loss-versus-frequency tab.

use std::collections::BTreeMap;

use eframe::egui::{self, Color32};
use egui_plot::{AxisHints, HPlacement, Legend, Line, LineStyle, Plot, PlotPoints, VLine};
use transverse_core::NP2DB;
use transverse_core::losses::{FreqResponse, sweep};
use transverse_core::units::{FREQ_UNITS, factor, format_g, parse_number};

use super::nav;

const POINTS: usize = 400;
const DEFAULT_FMAX_HZ: f64 = 20e9;

pub struct LossView {
    fmax_text: String,
    fmax_unit: String,
}

impl Default for LossView {
    fn default() -> Self {
        Self { fmax_text: "20".into(), fmax_unit: "GHz".into() }
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
    /// Highest frequency [Hz] of the plot; an invalid entry falls back to 20 GHz.
    pub fn fmax_hz(&self) -> f64 {
        let v =
            parse_number(&self.fmax_text).zip(factor(&FREQ_UNITS, &self.fmax_unit, None)).map(|(v, f)| v * f);
        v.filter(|v| *v > 0.0).unwrap_or(DEFAULT_FMAX_HZ)
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

    pub fn ui(&mut self, ui: &mut egui::Ui, freq: &dyn FreqResponse, z0: f64, f_hz: f64, generation: u64) {
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
        });
        ui.label(format!("Loss vs frequency (Z0 = {z0:.2} Ohm; er, tan d constant)"));

        let fmax = self.fmax_hz();
        let c = curves(freq, fmax);
        let series = |pick: fn(&(f64, f64, f64, f64)) -> f64| {
            PlotPoints::from(c.iter().map(|r| [r.0, pick(r)]).collect::<Vec<_>>())
        };
        let plot = Plot::new(("loss", generation, fmax.to_bits()))
            .height(ui.available_height().max(160.0))
            .x_axis_label("Frequency [GHz]")
            .custom_y_axes(vec![
                AxisHints::new_y().label("Loss [dB/mm]"),
                AxisHints::new_y()
                    .label("Loss [dB/in]")
                    .placement(HPlacement::Right)
                    .formatter(|mark, _| format_g(mark.value * 25.4)),
            ])
            .include_y(0.0)
            .legend(Legend::default());
        let total_colour = ui.visuals().text_color();
        nav::configure(plot).show(ui, |pui| {
            nav::wheel_zoom(pui);
            pui.line(Line::new("Conductor", series(|r| r.1)).width(1.6));
            pui.line(Line::new("Dielectric", series(|r| r.2)).width(1.6));
            pui.line(Line::new("Total", series(|r| r.3)).color(total_colour).width(2.4));
            if f_hz <= fmax {
                pui.add(
                    VLine::new("", f_hz / 1e9)
                        .color(Color32::GRAY)
                        .width(0.9)
                        .style(LineStyle::dashed_loose()),
                );
            }
        });
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
}
