//! Return-current tab: share table, explanation and current density along the ground conductors.

use eframe::egui::{self, Align2, Color32, RichText, Stroke};
use egui_plot::{Corner, Legend, Line, LineStyle, Plot, PlotPoint, PlotPoints, Polygon, Text, VLine};
use transverse_lines::model::{CurrentProfile, SplitSummary};

use super::nav;

/// matplotlib's default colour cycle (C0, C1, C2), as in the Python plot.
const CYCLE: [Color32; 3] =
    [Color32::from_rgb(31, 119, 180), Color32::from_rgb(255, 127, 14), Color32::from_rgb(44, 160, 44)];

/// Upper limit of the plot: 15 % above the largest value inside the shown x range.
fn y_top(split: &SplitSummary) -> f64 {
    let hw = split.plot.half_width;
    let m = split
        .profiles
        .iter()
        .flat_map(|p| p.x.iter().zip(&p.j))
        .filter(|(x, j)| x.abs() <= hw && j.is_finite())
        .map(|(_, j)| *j)
        .fold(0.0, f64::max);
    if m > 0.0 { 1.15 * m } else { 1.0 }
}

/// The points of a profile as one or two lines (two when it is not defined across the signal).
fn segments(p: &CurrentProfile) -> Vec<Vec<[f64; 2]>> {
    let pts = |keep: &dyn Fn(f64) -> bool| -> Vec<[f64; 2]> {
        p.x.iter().zip(&p.j).filter(|(x, _)| keep(**x)).map(|(x, j)| [*x, *j]).collect()
    };
    if p.break_at_zero { vec![pts(&|x| x < 0.0), pts(&|x| x > 0.0)] } else { vec![pts(&|_| true)] }
}

pub fn show(ui: &mut egui::Ui, split: &SplitSummary, generation: u64) {
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
    ui.add_space(4.0);

    let hw = split.plot.half_width;
    let ymax = y_top(split);
    let plot = Plot::new(("return_current", generation))
        .height(ui.available_height().max(160.0))
        .x_axis_label("x [mm]")
        .y_axis_label("J_s / I  [1/mm]")
        .default_x_bounds(-hw, hw)
        .default_y_bounds(0.0, ymax)
        .legend(Legend::default().position(Corner::RightTop));
    nav::configure(plot).show(ui, |pui| {
        nav::wheel_zoom(pui);
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
            let name = format!("{}, {:.1} %", p.label, p.share_pct);
            for seg in segments(p) {
                if seg.is_empty() {
                    continue;
                }
                let mut line = Line::new(name.clone(), PlotPoints::from(seg))
                    .color(CYCLE[p.color % CYCLE.len()])
                    .width(1.4);
                if p.dashed {
                    line = line.style(LineStyle::dashed_loose());
                }
                pui.line(line);
            }
        }
        for (x, t) in &split.plot.labels {
            pui.text(Text::new("", PlotPoint::new(*x, ymax * 0.97), t.as_str()).anchor(Align2::CENTER_TOP));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(break_at_zero: bool) -> CurrentProfile {
        CurrentProfile {
            key: "k".into(),
            label: "l".into(),
            share_pct: 10.0,
            x: vec![-2.0, -1.0, 0.0, 1.0, 2.0],
            j: vec![1.0, 2.0, 5.0, 3.0, 1.0],
            color: 0,
            dashed: false,
            break_at_zero,
        }
    }

    #[test]
    fn profiles_break_across_the_signal_only_when_asked() {
        assert_eq!(segments(&profile(false)).len(), 1);
        assert_eq!(segments(&profile(false))[0].len(), 5);
        let s = segments(&profile(true));
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].len(), s[1].len()), (2, 2), "x = 0 itself is dropped");
    }

    #[test]
    fn plot_height_follows_the_visible_range() {
        let mut split = SplitSummary { profiles: vec![profile(false)], ..Default::default() };
        split.plot.half_width = 1.5;
        assert!((y_top(&split) - 1.15 * 5.0).abs() < 1e-12);
        split.plot.half_width = 1.5;
        split.profiles[0].j[2] = 1.0;
        assert!((y_top(&split) - 1.15 * 3.0).abs() < 1e-12, "values outside |x| <= 1.5 do not count");
        assert_eq!(y_top(&SplitSummary::default()), 1.0);
    }
}
