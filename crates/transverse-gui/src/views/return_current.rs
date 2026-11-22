//! Return-current tab: share table, explanation and current density along the ground conductors.

use eframe::egui::{self, Align2, Color32, RichText, Stroke, vec2};
use egui_plot::{Line, LineStyle, Plot, PlotBounds, PlotPoint, PlotPoints, Polygon, Text, VLine};
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

/// The view the plot starts with and "Fit" returns to: the Python plot's x range.
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

#[derive(Default)]
pub struct ReturnView {
    /// View requested by a button, applied in the next plot pass.
    pending: Option<Bounds>,
}

impl ReturnView {
    pub fn ui(&mut self, ui: &mut egui::Ui, split: &SplitSummary, generation: u64) {
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
            if ui
                .button("Fit")
                .on_hover_text("x range of the signal region, y from 0 to the maximum")
                .clicked()
            {
                self.pending = Some(fit_bounds(split));
            }
            if ui.button("Show all").on_hover_text("the whole extent of all curves").clicked() {
                self.pending = Some(all_bounds(split));
            }
            ui.weak("wheel: zoom   left drag: zoom box   right drag: pan   double-click: reset");
        });

        let ymax = fit_bounds(split)[3];
        let hw = split.plot.half_width;
        let pending = self.pending.take();
        let plot = Plot::new(("return_current", generation))
            .height((ui.available_height() - READOUT_HEIGHT).max(160.0))
            .x_axis_label("x [mm]")
            .y_axis_label("J_s / I  [1/mm]")
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
            Some(x) => {
                let vals: Vec<String> = split
                    .profiles
                    .iter()
                    .map(|p| match profile_value(p, x) {
                        Some(v) => format!("{}: {v:.4}", p.label),
                        None => format!("{}: -", p.label),
                    })
                    .collect();
                format!("x = {x:.4} mm      J_s/I [1/mm]    {}", vals.join("     "))
            }
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
