//! Return-current tab: share table, explanation and current density along the ground conductors.

use eframe::egui::{self, Align2, Color32, RichText, Stroke, vec2};
use egui_plot::{
    Line, LineStyle, Plot, PlotBounds, PlotPoint, PlotPoints, Polygon, Text, VLine, uniform_grid_spacer,
};
use transverse_core::units::{UnitSystem, auto_length_unit, format_g, format_sig, grid_steps_mm};
use transverse_lines::model::{Bounds, CurrentProfile, SplitSummary};

use super::legend::{self, LegendEntry};
use super::nav;

/// matplotlib's default colour cycle (C0, C1, C2), as in the Python plot.
const CYCLE: [Color32; 3] =
    [Color32::from_rgb(31, 119, 180), Color32::from_rgb(255, 127, 14), Color32::from_rgb(44, 160, 44)];

/// Two lines of values under the plot.
const READOUT_HEIGHT: f32 = 44.0;

/// Upper limit of the plot: 15 % above the largest value with |x| <= `limit`.
fn y_top(split: &SplitSummary, limit: f64) -> f64 {
    let m = split
        .profiles
        .iter()
        .flat_map(|p| p.x.iter().zip(&p.j))
        .filter(|(x, j)| x.abs() <= limit && j.is_finite())
        .map(|(_, j)| *j)
        .fold(0.0, f64::max);
    if m > 0.0 { 1.15 * m } else { 1.0 }
}

/// The view the plot starts with and "Signal region" returns to: the Python plot's x range.
pub fn fit_bounds(split: &SplitSummary) -> Bounds {
    let hw = split.plot.half_width;
    [-hw, hw, 0.0, y_top(split, hw)]
}

/// Everything the profiles contain.
pub fn all_bounds(split: &SplitSummary) -> Bounds {
    let (lo, hi) = split
        .profiles
        .iter()
        .flat_map(|p| p.x.iter().copied())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |a, x| (a.0.min(x), a.1.max(x)));
    if lo < hi { [lo, hi, 0.0, y_top(split, f64::INFINITY)] } else { fit_bounds(split) }
}

/// Top of the y range that fits the curves inside `x0..x1`: 15 % above their maximum there,
/// evaluated at the points and at both edges.
pub fn y_fit(split: &SplitSummary, x0: f64, x1: f64) -> f64 {
    let mut m = 0.0f64;
    for p in &split.profiles {
        for (x, j) in p.x.iter().zip(&p.j) {
            if *x >= x0 && *x <= x1 && j.is_finite() {
                m = m.max(*j);
            }
        }
        for x in [x0, x1] {
            m = m.max(profile_value(p, x).filter(|v| v.is_finite()).unwrap_or(0.0));
        }
    }
    if m > 0.0 { 1.15 * m } else { 1.0 }
}

/// The points of a profile as one or two lines (two when it is not defined across the signal).
fn segments(p: &CurrentProfile) -> Vec<Vec<[f64; 2]>> {
    let pts = |keep: &dyn Fn(f64) -> bool| -> Vec<[f64; 2]> {
        p.x.iter().zip(&p.j).filter(|(x, _)| keep(**x)).map(|(x, j)| [*x, *j]).collect()
    };
    if p.break_at_zero { vec![pts(&|x| x < 0.0), pts(&|x| x > 0.0)] } else { vec![pts(&|_| true)] }
}

/// Value of a profile at `x` by linear interpolation; None outside its range or across the signal
/// (where a profile that is broken at zero is not defined).
pub fn profile_value(p: &CurrentProfile, x: f64) -> Option<f64> {
    let (first, last) = (*p.x.first()?, *p.x.last()?);
    if x < first || x > last {
        return None;
    }
    let i = p.x.partition_point(|&v| v <= x).clamp(1, p.x.len().saturating_sub(1).max(1));
    if p.x.len() < 2 {
        return (x == first).then(|| p.j[0]);
    }
    let (x0, x1) = (p.x[i - 1], p.x[i]);
    if x == x0 {
        return Some(p.j[i - 1]); // exactly on a point, also next to a gap
    }
    if p.break_at_zero && x0 < 0.0 && x1 > 0.0 {
        return None;
    }
    let t = if x1 > x0 { (x - x0) / (x1 - x0) } else { 0.0 };
    Some(p.j[i - 1] + t * (p.j[i] - p.j[i - 1]))
}

/// Values of all curves at `x_mm`, in `unit` (`uf` mm per unit): "x = ...   J_s/I [1/unit]   name: v ...".
pub fn readout_text(split: &SplitSummary, x_mm: f64, unit: &str, uf: f64) -> String {
    let vals: Vec<String> = split
        .profiles
        .iter()
        .map(|p| match profile_value(p, x_mm) {
            Some(v) => format!("{}: {}", p.label, format_sig(v * uf, 4)),
            None => format!("{}: -", p.label),
        })
        .collect();
    format!("x = {} {unit}      J_s/I [1/{unit}]    {}", format_sig(x_mm / uf, 5), vals.join("     "))
}

#[derive(Default)]
pub struct ReturnView {
    /// View requested by a button, applied in the next plot pass.
    pending: Option<Bounds>,
    /// Visible bounds of the plot as of the last frame.
    bounds: Option<Bounds>,
}

impl ReturnView {
    pub fn ui(&mut self, ui: &mut egui::Ui, split: &SplitSummary, generation: u64, units: UnitSystem) {
        egui::Grid::new("split_table").striped(true).num_columns(3).spacing([24.0, 3.0]).show(ui, |ui| {
            ui.label(RichText::new("Ground conductor").strong());
            ui.label(RichText::new("E-field share").strong());
            ui.label(RichText::new("Return current (HF)").strong());
            ui.end_row();
            for r in &split.rows {
                ui.horizontal(|ui| {
                    ui.add_space(18.0 * f32::from(r.indent));
                    ui.label(&r.label);
                });
                ui.label(RichText::new(format!("{:6.1} %", r.field_pct)).monospace());
                ui.label(RichText::new(format!("{:6.1} %", r.current_pct)).monospace());
                ui.end_row();
            }
        });
        ui.add_space(4.0);
        ui.label(&split.info);
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if ui.button("Fit y").on_hover_text("keep the x range, scale y to the curves in it").clicked() {
                let [x0, x1, ..] = self.bounds.unwrap_or_else(|| fit_bounds(split));
                self.pending = Some([x0, x1, 0.0, y_fit(split, x0, x1)]);
            }
            if ui
                .button("Signal region")
                .on_hover_text("the trace and its gaps plus the nearest ground")
                .clicked()
            {
                self.pending = Some(fit_bounds(split));
            }
            if ui.button("Full view").on_hover_text("the whole domain").clicked() {
                self.pending = Some(all_bounds(split));
            }
            ui.weak("wheel: zoom   left drag: zoom box   right drag: pan   double-click: reset");
        });

        // x in a length unit that suits the visible range, J_s/I per that unit
        let span = self.bounds.map_or(2.0 * split.plot.half_width, |b| b[1] - b[0]);
        let (unit, uf) = auto_length_unit(units, span);
        let ymax = fit_bounds(split)[3];
        let hw = split.plot.half_width;
        let pending = self.pending.take();
        let plot = Plot::new(("return_current", generation))
            .height((ui.available_height() - READOUT_HEIGHT).max(160.0))
            .x_axis_label(format!("x [{unit}]"))
            .y_axis_label(format!("J_s / I  [1/{unit}]"))
            .x_axis_formatter(move |m, _| format_g(m.value / uf))
            .y_axis_formatter(move |m, _| format_g(m.value * uf))
            .x_grid_spacer(uniform_grid_spacer(move |g| grid_steps_mm(g.base_step_size, uf)))
            .y_grid_spacer(uniform_grid_spacer(move |g| grid_steps_mm(g.base_step_size, 1.0 / uf)))
            .y_axis_min_width(60.0)
            .default_x_bounds(-hw, hw)
            .default_y_bounds(0.0, ymax);
        let resp = nav::configure(plot).show(ui, |pui| {
            nav::wheel_zoom(pui);
            if let Some(v) = pending {
                pui.set_plot_bounds(PlotBounds::from_min_max([v[0], v[2]], [v[1], v[3]]));
            }
            if let Some([a, b]) = split.plot.signal {
                let pts = vec![[a, 0.0], [b, 0.0], [b, ymax], [a, ymax]];
                pui.polygon(
                    Polygon::new("", PlotPoints::from(pts))
                        .fill_color(Color32::from_rgba_unmultiplied(31, 119, 180, 24))
                        .stroke(Stroke::NONE),
                );
            }
            for &x in &split.plot.markers {
                pui.add(VLine::new("", x).color(Color32::GRAY).width(0.8).style(LineStyle::dotted_loose()));
            }
            for p in &split.profiles {
                for seg in segments(p) {
                    if seg.is_empty() {
                        continue;
                    }
                    let mut line = Line::new(p.label.clone(), PlotPoints::from(seg))
                        .color(CYCLE[p.color % CYCLE.len()])
                        .width(1.4);
                    if p.dashed {
                        line = line.style(LineStyle::dashed_loose());
                    }
                    pui.line(line);
                }
            }
            for (x, t) in &split.plot.labels {
                pui.text(
                    Text::new("", PlotPoint::new(*x, ymax * 0.97), t.as_str()).anchor(Align2::CENTER_TOP),
                );
            }
        });

        let b = resp.transform.bounds();
        self.bounds = Some([b.min()[0], b.max()[0], b.min()[1], b.max()[1]]);

        let entries: Vec<LegendEntry> = split
            .profiles
            .iter()
            .map(|p| LegendEntry {
                label: format!("{}, {:.1} %", p.label, p.share_pct),
                color: CYCLE[p.color % CYCLE.len()],
                dashed: p.dashed,
                width: 1.6,
            })
            .collect();
        legend::draw(ui, *resp.transform.frame(), 14.0, &entries);

        // values of all curves at the cursor
        let text = match nav::hover_x(&resp) {
            Some(x) => readout_text(split, x, unit, uf),
            None => {
                "Move the cursor over the plot for the current density of every curve at that x.".to_string()
            }
        };
        ui.add_sized(vec2(ui.available_width(), READOUT_HEIGHT - 4.0), egui::Label::new(text).wrap());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(break_at_zero: bool) -> CurrentProfile {
        CurrentProfile {
            key: "k".into(),
            label: "l".into(),
            share_pct: 10.0,
            x: vec![-2.0, -1.0, 1.0, 2.0],
            j: vec![1.0, 2.0, 4.0, 1.0],
            color: 0,
            dashed: false,
            break_at_zero,
        }
    }

    #[test]
    fn profiles_break_across_the_signal_only_when_asked() {
        assert_eq!(segments(&profile(false)).len(), 1);
        assert_eq!(segments(&profile(false))[0].len(), 4);
        let s = segments(&profile(true));
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].len(), s[1].len()), (2, 2));
    }

    #[test]
    fn values_at_the_cursor_interpolate_and_respect_gaps() {
        let (cont, broken) = (profile(false), profile(true));
        // linear interpolation inside a side
        assert_eq!(profile_value(&cont, -1.5), Some(1.5));
        assert_eq!(profile_value(&broken, 1.5), Some(2.5));
        // exactly on a point and at the ends
        assert_eq!(profile_value(&broken, -1.0), Some(2.0));
        assert_eq!(profile_value(&broken, 2.0), Some(1.0));
        assert_eq!(profile_value(&broken, -2.0), Some(1.0));
        // across the signal a continuous profile interpolates, a broken one is undefined
        assert_eq!(profile_value(&cont, 0.0), Some(3.0));
        assert_eq!(profile_value(&broken, 0.0), None);
        // outside the data
        assert_eq!(profile_value(&cont, -2.5), None);
        assert_eq!(profile_value(&cont, 2.5), None);
        // degenerate profiles do not panic
        let empty = CurrentProfile { x: vec![], j: vec![], ..profile(false) };
        assert_eq!(profile_value(&empty, 0.0), None);
        let single = CurrentProfile { x: vec![1.0], j: vec![5.0], ..profile(false) };
        assert_eq!(profile_value(&single, 1.0), Some(5.0));
        assert_eq!(profile_value(&single, 1.1), None);
    }

    #[test]
    fn readout_is_in_the_chosen_length_unit() {
        let split = SplitSummary { profiles: vec![profile(false)], ..Default::default() };
        // x = -1.5 mm, value 1.5 1/mm
        let m = readout_text(&split, -1.5, "mm", 1.0);
        assert_eq!(m, "x = -1.5000 mm      J_s/I [1/mm]    l: 1.500");
        // in mil: x = -59.055 mil, and the density per mil is 0.0254 times the density per mm
        let i = readout_text(&split, -1.5, "mil", 0.0254);
        assert!(i.starts_with("x = -59.055 mil      J_s/I [1/mil]"), "{i}");
        assert!(i.ends_with("l: 0.03810"), "{i}");
        // outside the data and across a gap
        assert!(readout_text(&split, 5.0, "mm", 1.0).ends_with("l: -"));
        let mut broken = split.clone();
        broken.profiles[0].break_at_zero = true;
        assert!(readout_text(&broken, 0.0, "mm", 1.0).ends_with("l: -"));
    }

    #[test]
    fn fit_scales_y_to_the_curves_inside_the_visible_x_range() {
        let mut split = SplitSummary { profiles: vec![profile(false)], ..Default::default() };
        // profile: x [-2, -1, 1, 2], j [1, 2, 4, 1]
        assert_eq!(y_fit(&split, -2.0, 2.0), 1.15 * 4.0);
        // only the left part: peak 2 at x = -1
        // (the profile is continuous across the gap, so the value at the edge x = -0.5 is 2.5)
        assert_eq!(y_fit(&split, -2.0, -0.5), 1.15 * 2.5);
        assert_eq!(y_fit(&split, -2.0, -1.0), 1.15 * 2.0);
        // edges count even without a data point: at x = 0 the profile is 3, at x = 1.5 it is 2.5
        assert!(
            (y_fit(&split, -0.5, 0.5) - 1.15 * 3.5).abs() < 1e-12,
            "values at 0.5 and -0.5 are 3.25 .. no: {}",
            y_fit(&split, -0.5, 0.5)
        );
        // nothing visible -> a sane default
        split.profiles.clear();
        assert_eq!(y_fit(&split, 0.0, 1.0), 1.0);
    }

    #[test]
    fn the_plot_reports_its_bounds_so_fit_can_use_them() {
        use eframe::egui::{Pos2, Rect, vec2};
        let ctx = egui::Context::default();
        let mut split = SplitSummary { profiles: vec![profile(false)], ..Default::default() };
        split.plot.half_width = 1.5;
        let mut view = ReturnView::default();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 700.0))),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| view.ui(ui, &split, 0, UnitSystem::Metric));
            });
        }
        let b = view.bounds.expect("bounds are stored after drawing");
        assert!(b[0] <= -1.5 + 1e-9 && b[1] >= 1.5 - 1e-9, "the signal region is visible: {b:?}");
    }

    #[test]
    fn fit_and_show_all_bounds() {
        let mut split = SplitSummary { profiles: vec![profile(false)], ..Default::default() };
        split.plot.half_width = 1.5;
        // the peak value 4 is at |x| = 1 (inside), the end value 1 at |x| = 2 (outside the fit range)
        assert_eq!(fit_bounds(&split), [-1.5, 1.5, 0.0, 1.15 * 4.0]);
        assert_eq!(all_bounds(&split), [-2.0, 2.0, 0.0, 1.15 * 4.0]);
        split.profiles[0].j[1] = 1.0;
        split.profiles[0].j[2] = 1.0;
        split.profiles[0].j[3] = 3.0;
        assert_eq!(fit_bounds(&split)[3], 1.15 * 1.0, "values outside the fit range do not count");
        assert_eq!(all_bounds(&split)[3], 1.15 * 3.0);
        assert_eq!(fit_bounds(&SplitSummary::default())[3], 1.0);
    }
}
