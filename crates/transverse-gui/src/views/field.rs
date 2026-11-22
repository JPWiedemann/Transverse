//! Field tab: |E| / potential / permittivity / mesh as an image with field lines, equipotentials,
//! conductors and a cursor readout.
//!
//! The image always covers the visible region (plus a margin) at screen resolution and is
//! re-rendered when the user zooms or pans far enough, so deep zooms stay sharp and zooming out never
//! shows blank space.

use std::collections::BTreeMap;

use eframe::egui::{
    self, Align2, Color32, FontId, PointerButton, Pos2, Rect, Sense, Shape, Stroke, TextureHandle,
    TextureOptions, Vec2, vec2,
};
use egui_plot::{Plot, PlotBounds, PlotImage, PlotPoint, uniform_grid_spacer};
use transverse_core::postproc::EField;
use transverse_core::quasistatic::Accuracy;
use transverse_core::units::{UnitSystem, auto_length_unit, format_sig, grid_steps_mm};
use transverse_gui::raster::{FieldMode, Legend, RasterSpec, nice_ticks, probe, render};
use transverse_lines::model::{Bounds, FieldData};

/// Copper colour of the conductors (same as the Python plots).
const COPPER: Color32 = Color32::from_rgb(0xc8, 0x75, 0x33);
const LINE_BLUE: Color32 = Color32::from_rgb(0x9f, 0xd3, 0xff);
const BAR_WIDTH: f32 = 110.0;
const EQUIPOTENTIALS: [f64; 9] = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];
/// The image extends this fraction of the visible size beyond each edge.
const MARGIN: f64 = 0.25;
/// Re-render when the visible width changed by more than this factor since the last render.
const RERENDER_RATIO: (f64, f64) = (0.6, 1.4);
/// Height reserved for the title (two lines) and the readout line, so toggling options never
/// changes the plot size.
const READOUT_HEIGHT: f32 = 24.0;

/// Field lines as polylines in mm.
type Polylines = Vec<Vec<[f64; 2]>>;

struct Render {
    key: (u64, FieldMode, u64),
    /// Visible bounds this image was rendered for.
    visible: Bounds,
    /// Region the image was asked to cover (visible plus margin); the grid may end inside it.
    covered: Bounds,
    texture: TextureHandle,
    centre: PlotPoint,
    size: [f32; 2],
    legend: Option<Legend>,
    bar: Option<TextureHandle>,
    lines: Polylines,
}

pub struct FieldView {
    pub mode: FieldMode,
    pub show_eq: bool,
    pub show_mesh: bool,
    pub db_range: f64,
    /// Metric or imperial lengths on the axes and in the readout (prefix chosen automatically).
    pub units: UnitSystem,
    generation: u64,
    /// Visible bounds of the plot as of the last frame.
    plot_bounds: Option<Bounds>,
    /// View requested by a button or a finished zoom box, applied in the next plot pass.
    pending_view: Option<Bounds>,
    /// Start of a zoom box being dragged (screen position).
    box_start: Option<Pos2>,
    render: Option<Render>,
    e: Option<(u64, EField)>,
    eq: Option<(u64, Vec<[[f64; 2]; 2]>)>,
}

impl Default for FieldView {
    fn default() -> Self {
        Self {
            mode: FieldMode::EDbLines,
            show_eq: true,
            show_mesh: false,
            db_range: 40.0,
            units: UnitSystem::Metric,
            generation: 0,
            plot_bounds: None,
            pending_view: None,
            box_start: None,
            render: None,
            e: None,
            eq: None,
        }
    }
}

/// Bounds for a zoom box dragged from `a` to `b` (data coordinates) in a plot frame of `frame`
/// points, extended to the aspect of the frame so the result is already what the plot would show
/// (the plot enforces 1:1 data aspect and would otherwise stretch the view for one frame).
/// None for boxes smaller than a few points.
fn box_zoom_bounds(a: [f64; 2], b: [f64; 2], frame: Vec2, points_per_unit: f64) -> Option<Bounds> {
    let (w, h) = ((a[0] - b[0]).abs(), (a[1] - b[1]).abs());
    if w * points_per_unit < 4.0 || h * points_per_unit < 4.0 || frame.x <= 0.0 || frame.y <= 0.0 {
        return None;
    }
    let ratio = frame.x as f64 / frame.y as f64;
    let (bw, bh) = if w / h < ratio { (h * ratio, h) } else { (w, w / ratio) };
    let (cx, cy) = (0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1]));
    Some([cx - 0.5 * bw, cx + 0.5 * bw, cy - 0.5 * bh, cy + 0.5 * bh])
}

fn expand(b: Bounds, frac: f64) -> Bounds {
    let (dx, dy) = ((b[1] - b[0]) * frac, (b[3] - b[2]) * frac);
    [b[0] - dx, b[1] + dx, b[2] - dy, b[3] + dy]
}

fn contains(outer: Bounds, inner: Bounds) -> bool {
    outer[0] <= inner[0] && outer[1] >= inner[1] && outer[2] <= inner[2] && outer[3] >= inner[3]
}

impl FieldView {
    /// A new analysis result replaces the old one: drop all cached images and lines and show the
    /// signal region again.
    pub fn new_result(&mut self) {
        self.generation += 1;
        self.plot_bounds = None;
        self.pending_view = None;
        self.box_start = None;
        self.render = None;
    }

    pub fn load(&mut self, ui: &BTreeMap<String, serde_json::Value>) {
        if let Some(m) = ui.get("field_mode").and_then(|v| v.as_str()).and_then(FieldMode::from_key) {
            self.mode = m;
        }
        let flag = |k: &str, d: bool| ui.get(k).and_then(|v| v.as_bool()).unwrap_or(d);
        self.show_eq = flag("show_eq", self.show_eq);
        self.show_mesh = flag("show_mesh", self.show_mesh);
        if let Some(u) = ui.get("length_system").and_then(|v| v.as_str()).and_then(UnitSystem::from_key) {
            self.units = u;
        }
        if let Some(d) = ui.get("db_range").and_then(|v| v.as_f64()).filter(|d| (5.0..=200.0).contains(d)) {
            self.db_range = d;
        }
    }

    pub fn save(&self, ui: &mut BTreeMap<String, serde_json::Value>) {
        ui.insert("field_mode".into(), self.mode.key().into());
        ui.insert("show_eq".into(), self.show_eq.into());
        ui.insert("show_mesh".into(), self.show_mesh.into());
        ui.insert("db_range".into(), self.db_range.into());
        ui.insert("length_system".into(), self.units.key().into());
    }

    /// Unit (symbol, factor to mm) for lengths, chosen from the size of the visible region.
    fn length_unit(&self, f: &FieldData) -> (&'static str, f64) {
        let b = self.plot_bounds.unwrap_or(f.view_zoom);
        auto_length_unit(self.units, (b[1] - b[0]).max(b[3] - b[2]))
    }

    fn needs_render(&self, key: (u64, FieldMode, u64), visible: Bounds) -> bool {
        let Some(r) = &self.render else { return true };
        if r.key != key || !contains(r.covered, visible) {
            return true;
        }
        let ratio = (visible[1] - visible[0]) / (r.visible[1] - r.visible[0]);
        !(RERENDER_RATIO.0..=RERENDER_RATIO.1).contains(&ratio)
    }

    /// Build whatever the current options need and the caches do not have yet.
    /// `plot_px` is the plot size in screen pixels (sets the image resolution).
    fn update_caches(&mut self, ctx: &egui::Context, f: &FieldData, visible: Bounds, plot_px: Vec2) {
        let gen_ = self.generation;
        if self.e.as_ref().is_none_or(|(g, _)| *g != gen_) {
            self.e = Some((gen_, f.e_field(false)));
        }
        if self.eq.as_ref().is_none_or(|(g, _)| *g != gen_) {
            let segs = f.equipotentials(&EQUIPOTENTIALS).into_iter().flat_map(|(_, s)| s).collect();
            self.eq = Some((gen_, segs));
        }
        let key = (gen_, self.mode, if self.mode.is_db() { self.db_range.to_bits() } else { 0 });
        if !self.needs_render(key, visible) {
            return;
        }
        let e = &self.e.as_ref().unwrap().1;
        let want = expand(visible, MARGIN);
        let width = ((plot_px.x as f64 * (1.0 + 2.0 * MARGIN)) as usize).clamp(400, 2400);
        let r = render(
            f,
            e,
            &RasterSpec {
                mode: self.mode,
                view: want,
                scale_view: f.view_zoom,
                db_range: self.db_range,
                width,
            },
        );
        let image = egui::ColorImage::from_rgba_unmultiplied([r.width, r.height], &r.rgba);
        let texture = ctx.load_texture("field", image, TextureOptions::LINEAR);
        // the image covers the wanted region clipped to the grid
        let (x0, x1) = (want[0].max(f.x[0]), want[1].min(f.x[f.nx() - 1]));
        let (y0, y1) = (want[2].max(f.y[0]), want[3].min(f.y[f.ny() - 1]));
        let bar = r.legend.as_ref().map(|l| {
            let rgba: Vec<u8> = (0..256)
                .rev()
                .flat_map(|i| {
                    let c = l.cmap.sample(i as f64 / 255.0);
                    [c[0], c[1], c[2], 255]
                })
                .collect();
            ctx.load_texture(
                "colorbar",
                egui::ColorImage::from_rgba_unmultiplied([1, 256], &rgba),
                TextureOptions::LINEAR,
            )
        });
        let lines = if self.mode.has_field_lines() { f.field_lines(want, (2.2, 1.4)) } else { Vec::new() };
        self.render = Some(Render {
            key,
            visible,
            covered: want,
            texture,
            centre: PlotPoint::new(0.5 * (x0 + x1), 0.5 * (y0 + y1)),
            size: [(x1 - x0) as f32, (y1 - y0) as f32],
            legend: r.legend,
            bar,
            lines,
        });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, f: &FieldData, acc: Option<&Accuracy>) {
        // ---- controls
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("field_mode").selected_text(self.mode.label()).width(190.0).show_ui(
                ui,
                |ui| {
                    for m in FieldMode::ALL {
                        ui.selectable_value(&mut self.mode, m, m.label());
                    }
                },
            );
            ui.add_enabled(
                self.mode.allows_equipotentials(),
                egui::Checkbox::new(&mut self.show_eq, "Equipotentials"),
            );
            ui.add_enabled(
                self.mode != FieldMode::Mesh,
                egui::Checkbox::new(&mut self.show_mesh, "Mesh overlay"),
            );
            if self.mode.is_db() {
                ui.label("Range");
                ui.add(egui::DragValue::new(&mut self.db_range).range(5.0..=200.0).speed(0.5).suffix(" dB"));
            }
            egui::ComboBox::from_id_salt("length_system")
                .selected_text(self.units.label())
                .width(80.0)
                .show_ui(ui, |ui| {
                    for u in [UnitSystem::Metric, UnitSystem::Imperial] {
                        ui.selectable_value(&mut self.units, u, u.label());
                    }
                });
            if ui.button("Signal region").clicked() {
                self.pending_view = Some(f.view_zoom);
            }
            if ui.button("Full view").clicked() {
                self.pending_view = Some(f.view_full);
            }
        });
        let show_mesh = self.show_mesh || self.mode == FieldMode::Mesh;
        let (unit, unit_f) = self.length_unit(f);

        // title: always two lines, so showing the mesh info does not resize the plot
        let mut title = format!("{}  -  1 V on signal, Z0 = {:.2} Ohm\n", self.mode.label(), f.z0);
        match (show_mesh, acc) {
            (true, Some(a)) => {
                title += &format!(
                    "mesh {} x {} = {} nodes (finest level), min. cell {} {}",
                    a.nx,
                    a.ny,
                    a.nodes,
                    format_sig(a.dmin_actual / unit_f, 3),
                    unit
                );
            }
            _ => title.push('\u{a0}'),
        }
        ui.add(egui::Label::new(title).truncate());

        // ---- plot
        let avail = ui.available_size();
        let plot_h = (avail.y - READOUT_HEIGHT).max(120.0);
        let plot_w = (avail.x - BAR_WIDTH).max(200.0);
        let ppp = ui.ctx().pixels_per_point();
        let visible = self.plot_bounds.unwrap_or(f.view_zoom);
        self.update_caches(ui.ctx(), f, visible, vec2(plot_w, plot_h) * ppp);

        let (gen_, initial) = (self.generation, f.view_zoom);
        let pending = self.pending_view.take();
        let r = self.render.as_ref().unwrap();
        let (tex_id, centre, size) = (r.texture.id(), r.centre, r.size);
        let e = &self.e.as_ref().unwrap().1;
        let mut readout = String::new();
        let mut new_bounds = None;
        let box_start = self.box_start;
        let mut box_next = box_start;
        let mut zoom_to = None;

        ui.horizontal(|ui| {
            let resp = Plot::new(("field", gen_))
                .width(plot_w)
                .height(plot_h)
                .data_aspect(1.0)
                .show_grid(false)
                .x_axis_label(format!("x [{unit}]"))
                .y_axis_label(format!("y [{unit}]  (0 = signal layer)"))
                .x_axis_formatter(move |mark, _| {
                    format_sig(mark.value / unit_f, 6).trim_end_matches('0').trim_end_matches('.').to_string()
                })
                .y_axis_formatter(move |mark, _| {
                    format_sig(mark.value / unit_f, 6).trim_end_matches('0').trim_end_matches('.').to_string()
                })
                // fixed axis width: the plot frame must not depend on the label text (which changes with the unit)
                .y_axis_min_width(60.0)
                .x_grid_spacer(uniform_grid_spacer(move |g| grid_steps_mm(g.base_step_size, unit_f)))
                .y_grid_spacer(uniform_grid_spacer(move |g| grid_steps_mm(g.base_step_size, unit_f)))
                .default_x_bounds(initial[0], initial[1])
                .default_y_bounds(initial[2], initial[3])
                // right drag pans, the wheel zooms and the left-drag zoom box is handled below, because the
                // built-in one stretches the view for a frame before the 1:1 aspect is restored
                .allow_zoom(false)
                .allow_scroll(false)
                .allow_boxed_zoom(false)
                .pan_pointer_button(PointerButton::Secondary)
                .show(ui, |pui| {
                    if let Some(v) = pending {
                        pui.set_plot_bounds(PlotBounds::from_min_max([v[0], v[2]], [v[1], v[3]]));
                    }
                    super::nav::wheel_zoom(pui);
                    pui.add(PlotImage::new("field", tex_id, centre, size));
                });
            let tr = resp.transform;
            let frame_rect = *tr.frame();
            let b = tr.bounds();
            new_bounds = Some([b.min()[0], b.max()[0], b.min()[1], b.max()[1]]);
            let painter = ui.painter_at(frame_rect);
            let rr = &resp.response;
            if rr.drag_started_by(PointerButton::Primary) {
                box_next = rr.interact_pointer_pos();
            }
            if let (Some(a), Some(b)) = (box_next, rr.interact_pointer_pos().or_else(|| rr.hover_pos())) {
                if rr.dragged_by(PointerButton::Primary) {
                    let rect = Rect::from_two_pos(a, b);
                    painter.rect_stroke(
                        rect,
                        0.0,
                        Stroke::new(4.0, Color32::DARK_BLUE),
                        egui::StrokeKind::Middle,
                    );
                    painter.rect_stroke(
                        rect,
                        0.0,
                        Stroke::new(2.0, Color32::WHITE),
                        egui::StrokeKind::Middle,
                    );
                } else if rr.drag_stopped_by(PointerButton::Primary) {
                    let (va, vb) = (tr.value_from_position(a), tr.value_from_position(b));
                    zoom_to = box_zoom_bounds(
                        [va.x, va.y],
                        [vb.x, vb.y],
                        frame_rect.size(),
                        tr.dpos_dvalue_x().abs(),
                    );
                    box_next = None;
                } else {
                    box_next = None;
                }
            }
            let to_screen = |p: [f64; 2]| tr.position_from_point(&PlotPoint::new(p[0], p[1]));

            // field lines with an arrowhead in the middle
            if self.mode.has_field_lines() {
                let stroke = Stroke::new(0.8, LINE_BLUE);
                for line in &r.lines {
                    let pts: Vec<Pos2> = line.iter().map(|p| to_screen(*p)).collect();
                    if pts.len() >= 3 {
                        let m = pts.len() / 2;
                        let d = (pts[m + 1] - pts[m - 1]).normalized();
                        if d.is_finite() {
                            let n = vec2(-d.y, d.x);
                            let tip = pts[m] + d * 3.0;
                            painter.add(Shape::line(
                                vec![tip - d * 6.0 + n * 2.5, tip, tip - d * 6.0 - n * 2.5],
                                stroke,
                            ));
                        }
                    }
                    painter.add(Shape::line(pts, stroke));
                }
            }
            // equipotentials
            if self.show_eq && self.mode.allows_equipotentials() {
                let stroke = Stroke::new(0.8, Color32::from_white_alpha(200));
                let segs: Vec<Shape> = self
                    .eq
                    .as_ref()
                    .map(|e| e.1.as_slice())
                    .unwrap_or(&[])
                    .iter()
                    .map(|s| Shape::line_segment([to_screen(s[0]), to_screen(s[1])], stroke))
                    .collect();
                painter.extend(segs);
            }
            // conductors
            for c in &f.conductors {
                let r = Rect::from_two_pos(to_screen([c.x0, c.y1]), to_screen([c.x1, c.y0]));
                painter.rect_filled(r, 0.0, COPPER);
                painter.rect_stroke(r, 0.0, Stroke::new(0.5, Color32::BLACK), egui::StrokeKind::Inside);
            }
            let [bx0, bx1] = [b.min()[0], b.max()[0]];
            for (ys, alpha, width, dash) in [(&f.dashed_y, 200u8, 0.8, 6.0), (&f.dotted_y, 90u8, 0.5, 2.0)] {
                for &y in ys {
                    let pts = [to_screen([bx0, y]), to_screen([bx1, y])];
                    painter.extend(Shape::dashed_line(
                        &pts,
                        Stroke::new(width, Color32::from_white_alpha(alpha)),
                        dash,
                        dash,
                    ));
                }
            }
            // mesh
            if show_mesh {
                let (a, b) = (b.min(), b.max());
                let (alpha, col) = if self.mode == FieldMode::Mesh { (150u8, 0u8) } else { (130u8, 255u8) };
                let stroke = Stroke::new(0.4, Color32::from_rgba_unmultiplied(col, col, col, alpha));
                let (y_lo, y_hi) = (a[1].max(f.y[0]), b[1].min(f.y[f.ny() - 1]));
                let (x_lo, x_hi) = (a[0].max(f.x[0]), b[0].min(f.x[f.nx() - 1]));
                let mut segs = Vec::new();
                for &x in f.x.iter().filter(|&&x| x >= a[0] && x <= b[0]) {
                    segs.push(Shape::line_segment([to_screen([x, y_lo]), to_screen([x, y_hi])], stroke));
                }
                for &y in f.y.iter().filter(|&&y| y >= a[1] && y <= b[1]) {
                    segs.push(Shape::line_segment([to_screen([x_lo, y]), to_screen([x_hi, y])], stroke));
                }
                painter.extend(segs);
            }

            // cursor readout
            if let Some(pos) = resp.response.hover_pos().filter(|p| frame_rect.contains(*p)) {
                let v = tr.value_from_position(pos);
                readout = format!(
                    "x = {} {unit}   y = {} {unit}",
                    format_sig(v.x / unit_f, 5),
                    format_sig(v.y / unit_f, 5)
                );
                match probe(f, e, v.x, v.y) {
                    Some(p) if p.in_conductor => readout += "   (conductor)",
                    Some(p) => {
                        readout += &format!("   V = {:.4} V", p.v);
                        if p.e.is_finite() {
                            readout += &format!(
                                "   |E| = {:.4} V/mm ({:.1} dB)   er = {}",
                                p.e,
                                20.0 * p.e.max(1e-30).log10(),
                                p.er
                            );
                        } else {
                            readout += &format!("   er = {}", p.er);
                        }
                    }
                    None => {}
                }
            }

            // colour bar next to the plot, as tall as the plot frame
            let (rect, _) = ui.allocate_exact_size(vec2(BAR_WIDTH, plot_h), Sense::hover());
            if let (Some(l), Some(bar)) = (r.legend.as_ref(), r.bar.as_ref()) {
                let bar_rect = Rect::from_min_max(
                    Pos2::new(rect.left() + 6.0, frame_rect.top()),
                    Pos2::new(rect.left() + 22.0, frame_rect.bottom()),
                );
                let p = ui.painter();
                p.image(
                    bar.id(),
                    bar_rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
                let text_col = ui.visuals().text_color();
                for t in nice_ticks(l.vmin, l.vmax, 6) {
                    let y = bar_rect.bottom() - ((t - l.vmin) / (l.vmax - l.vmin)) as f32 * bar_rect.height();
                    p.line_segment(
                        [Pos2::new(bar_rect.right(), y), Pos2::new(bar_rect.right() + 4.0, y)],
                        Stroke::new(1.0, text_col),
                    );
                    p.text(
                        Pos2::new(bar_rect.right() + 6.0, y),
                        Align2::LEFT_CENTER,
                        format!("{t:.4}").trim_end_matches('0').trim_end_matches('.'),
                        FontId::proportional(11.0),
                        text_col,
                    );
                }
                p.text(
                    Pos2::new(bar_rect.left(), bar_rect.top() - 4.0),
                    Align2::LEFT_BOTTOM,
                    &l.label,
                    FontId::proportional(10.0),
                    text_col,
                );
            }
        });

        self.box_start = box_next;
        if let Some(b) = zoom_to {
            self.pending_view = Some(b);
            ui.ctx().request_repaint();
        }

        // the next frame decides from these bounds whether the image must be re-rendered
        if let Some(nb) = new_bounds {
            let same = self
                .plot_bounds
                .is_some_and(|o| o.iter().zip(&nb).all(|(a, b)| (a - b).abs() <= 1e-9 * (1.0 + a.abs())));
            if !same {
                self.plot_bounds = Some(nb);
                ui.ctx().request_repaint();
            }
        }
        let hint = "Wheel: zoom   left drag: zoom box   right drag: pan   double-click: reset";
        ui.add(egui::Label::new(if readout.is_empty() { hint.to_string() } else { readout }).truncate());
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use transverse_core::Ctl;
    use transverse_lines::cpwg_inner::CpwgInner;
    use transverse_lines::model::{LineModel, ParamSet, ParamValue};

    fn run_frame(
        ctx: &egui::Context,
        view: &mut FieldView,
        f: &FieldData,
        acc: Option<&Accuracy>,
        hover: Option<Pos2>,
    ) {
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 700.0))),
            ..Default::default()
        };
        if let Some(p) = hover {
            input.events.push(egui::Event::PointerMoved(p));
        }
        let _ = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| view.ui(ui, f, acc));
        });
    }

    fn frames(ctx: &egui::Context, view: &mut FieldView, f: &FieldData, acc: &Accuracy, n: usize) {
        for _ in 0..n {
            run_frame(ctx, view, f, Some(acc), None);
        }
    }

    fn analysis(open: bool) -> (FieldData, Accuracy) {
        let m = CpwgInner;
        let mut set: ParamSet = m.defaults();
        if open {
            set.set("plane1", ParamValue::Bool(false));
            set.set("fence1", ParamValue::Bool(false));
        }
        let a = m.analyze(&set, &Ctl::new()).unwrap();
        (a.field.unwrap(), a.accuracy.unwrap())
    }

    fn close(a: Bounds, b: Bounds) -> bool {
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= 1e-6 * (1.0 + x.abs()))
    }

    #[test]
    fn every_mode_renders_for_closed_and_open_structures_with_all_overlays() {
        let ctx = egui::Context::default();
        for open in [false, true] {
            let (f, acc) = analysis(open);
            let mut view = FieldView::default();
            view.new_result();
            for mode in FieldMode::ALL {
                for pending in [f.view_zoom, f.view_full] {
                    view.mode = mode;
                    view.show_mesh = true;
                    view.pending_view = Some(pending);
                    // two frames: the first builds the caches, the second draws with a hovering cursor
                    run_frame(&ctx, &mut view, &f, Some(&acc), None);
                    run_frame(&ctx, &mut view, &f, Some(&acc), Some(Pos2::new(500.0, 350.0)));
                }
            }
            assert!(view.render.as_ref().unwrap().legend.is_none(), "last mode is Mesh");
            view.mode = FieldMode::EDbLines;
            frames(&ctx, &mut view, &f, &acc, 2);
            assert!(view.render.as_ref().unwrap().lines.len() > 20);
        }
    }

    #[test]
    fn image_follows_the_visible_bounds_with_hysteresis() {
        let ctx = egui::Context::default();
        let (f, acc) = analysis(false);
        let mut view = FieldView::default();
        view.new_result();
        frames(&ctx, &mut view, &f, &acc, 3);
        let key = view.render.as_ref().unwrap().key;
        let v = view.render.as_ref().unwrap().visible;
        let w = v[1] - v[0];
        let shift = |b: Bounds, dx: f64| [b[0] + dx, b[1] + dx, b[2], b[3]];
        let zoom = |b: Bounds, k: f64| {
            let (cx, cy) = (0.5 * (b[0] + b[1]), 0.5 * (b[2] + b[3]));
            [cx + (b[0] - cx) * k, cx + (b[1] - cx) * k, cy + (b[2] - cy) * k, cy + (b[3] - cy) * k]
        };
        // small moves stay inside the margin and keep the image
        assert!(!view.needs_render(key, shift(v, 0.1 * w)));
        assert!(!view.needs_render(key, zoom(v, 1.3)));
        assert!(!view.needs_render(key, zoom(v, 0.7)));
        // panning beyond the margin, zooming in or out a lot re-renders
        assert!(view.needs_render(key, shift(v, 0.3 * w)));
        assert!(view.needs_render(key, zoom(v, 0.5)));
        assert!(view.needs_render(key, zoom(v, 1.45)));
        // other options re-render too
        assert!(view.needs_render((key.0, FieldMode::Potential, 0), v));
        assert!(view.needs_render((key.0 + 1, key.1, key.2), v));

        // the next frame renders for the bounds the plot reported
        let target = zoom(v, 0.4);
        view.plot_bounds = Some(target);
        run_frame(&ctx, &mut view, &f, Some(&acc), None);
        assert_eq!(view.render.as_ref().unwrap().visible, target);
    }

    #[test]
    fn colour_scale_does_not_change_when_zooming() {
        let ctx = egui::Context::default();
        let (f, acc) = analysis(false);
        let mut view = FieldView::default();
        view.new_result();
        frames(&ctx, &mut view, &f, &acc, 3);
        let a = view.render.as_ref().unwrap().legend.clone().unwrap();
        view.plot_bounds = Some(expand(f.view_zoom, -0.45));
        run_frame(&ctx, &mut view, &f, Some(&acc), None);
        let b = view.render.as_ref().unwrap().legend.clone().unwrap();
        assert_eq!((a.vmin, a.vmax), (b.vmin, b.vmax));
    }

    #[test]
    fn toggling_the_mesh_overlay_does_not_change_the_view() {
        let ctx = egui::Context::default();
        let (f, acc) = analysis(false);
        let mut view = FieldView::default();
        view.new_result();
        frames(&ctx, &mut view, &f, &acc, 4);
        let before = view.plot_bounds.unwrap();
        for k in 0..8 {
            view.show_mesh = k % 2 == 0;
            frames(&ctx, &mut view, &f, &acc, 2);
            assert!(
                close(view.plot_bounds.unwrap(), before),
                "toggle {k}: {:?} vs {before:?}",
                view.plot_bounds
            );
        }
    }

    #[test]
    fn view_buttons_do_not_remember_earlier_zoom_or_pan() {
        let ctx = egui::Context::default();
        let (f, acc) = analysis(false);
        let mut view = FieldView::default();
        view.new_result();
        frames(&ctx, &mut view, &f, &acc, 2);
        view.pending_view = Some(f.view_zoom);
        frames(&ctx, &mut view, &f, &acc, 2);
        let first = view.plot_bounds.unwrap();
        // wander off somewhere else, then press the button again
        view.pending_view = Some(expand(f.view_zoom, -0.4));
        frames(&ctx, &mut view, &f, &acc, 2);
        assert!(!close(view.plot_bounds.unwrap(), first));
        view.pending_view = Some(f.view_zoom);
        frames(&ctx, &mut view, &f, &acc, 2);
        assert!(close(view.plot_bounds.unwrap(), first), "{:?} vs {first:?}", view.plot_bounds);
        // the full view shows (at least) the whole domain
        view.pending_view = Some(f.view_full);
        frames(&ctx, &mut view, &f, &acc, 2);
        let b = view.plot_bounds.unwrap();
        let tol = 1e-6;
        assert!(b[0] <= f.view_full[0] + tol && b[1] >= f.view_full[1] - tol);
        assert!(b[2] <= f.view_full[2] + tol && b[3] >= f.view_full[3] - tol);
    }

    #[test]
    fn a_new_result_resets_to_the_signal_region() {
        let ctx = egui::Context::default();
        let (f, acc) = analysis(false);
        let mut view = FieldView::default();
        view.new_result();
        frames(&ctx, &mut view, &f, &acc, 2);
        view.pending_view = Some(f.view_full);
        frames(&ctx, &mut view, &f, &acc, 2);
        view.new_result();
        assert!(view.plot_bounds.is_none() && view.render.is_none());
        frames(&ctx, &mut view, &f, &acc, 2);
        let b = view.plot_bounds.unwrap();
        // the signal region (default bounds) is visible again; the full domain is not
        assert!(b[0] <= f.view_zoom[0] + 1e-6 && b[1] >= f.view_zoom[1] - 1e-6);
        assert!(b[1] - b[0] < 0.8 * (f.view_full[1] - f.view_full[0]));
    }

    #[test]
    fn zoom_box_is_fitted_to_the_plot_aspect() {
        let frame = vec2(800.0, 400.0); // 2:1
        // wide box: height grows
        let b = box_zoom_bounds([0.0, 0.0], [4.0, 1.0], frame, 100.0).unwrap();
        assert!(((b[1] - b[0]) / (b[3] - b[2]) - 2.0).abs() < 1e-12);
        assert!((b[0] - 0.0).abs() < 1e-12 && (b[1] - 4.0).abs() < 1e-12, "box width is kept");
        assert!((0.5 * (b[2] + b[3]) - 0.5).abs() < 1e-12, "box centre is kept");
        // tall box: width grows
        let b = box_zoom_bounds([0.0, 0.0], [1.0, 4.0], frame, 100.0).unwrap();
        assert!(((b[1] - b[0]) / (b[3] - b[2]) - 2.0).abs() < 1e-12);
        assert!((b[2] - 0.0).abs() < 1e-12 && (b[3] - 4.0).abs() < 1e-12);
        // the corners may be given in any order; tiny boxes are ignored
        assert_eq!(
            box_zoom_bounds([4.0, 1.0], [0.0, 0.0], frame, 100.0),
            box_zoom_bounds([0.0, 0.0], [4.0, 1.0], frame, 100.0)
        );
        assert!(box_zoom_bounds([0.0, 0.0], [0.03, 1.0], frame, 100.0).is_none());
        assert!(box_zoom_bounds([0.0, 0.0], [1.0, 1.0], vec2(0.0, 10.0), 100.0).is_none());
    }

    fn pointer(
        ctx: &egui::Context,
        view: &mut FieldView,
        f: &FieldData,
        acc: &Accuracy,
        events: Vec<egui::Event>,
    ) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 700.0))),
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| view.ui(ui, f, Some(acc)));
        });
    }

    #[test]
    fn dragging_a_zoom_box_zooms_without_a_stretched_intermediate_view() {
        use egui::{Event, Modifiers};
        let ctx = egui::Context::default();
        let (f, acc) = analysis(false);
        let mut view = FieldView::default();
        view.new_result();
        frames(&ctx, &mut view, &f, &acc, 3);
        let before = view.plot_bounds.unwrap();
        let (a, b) = (Pos2::new(400.0, 250.0), Pos2::new(560.0, 330.0));
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        pointer(&ctx, &mut view, &f, &acc, vec![Event::PointerMoved(a)]);
        pointer(&ctx, &mut view, &f, &acc, vec![button(a, true)]);
        pointer(&ctx, &mut view, &f, &acc, vec![Event::PointerMoved(Pos2::new(480.0, 290.0))]);
        pointer(&ctx, &mut view, &f, &acc, vec![Event::PointerMoved(b)]);
        assert!(view.box_start.is_some(), "the box is being dragged");
        pointer(&ctx, &mut view, &f, &acc, vec![button(b, false)]);
        let target = view.pending_view.expect("release requests the zoom");
        // the requested view has the aspect of the plot, so the plot has nothing left to correct
        let plot = view.plot_bounds.unwrap();
        let (ta, pa) =
            ((target[1] - target[0]) / (target[3] - target[2]), (plot[1] - plot[0]) / (plot[3] - plot[2]));
        assert!((ta - pa).abs() < 1e-6 * pa, "aspect {ta} vs plot {pa}");
        assert!(view.box_start.is_none());
        frames(&ctx, &mut view, &f, &acc, 1);
        assert!(close(view.plot_bounds.unwrap(), target), "{:?} vs {target:?}", view.plot_bounds);
        // and it is a real zoom in
        assert!(view.plot_bounds.unwrap()[1] - view.plot_bounds.unwrap()[0] < 0.5 * (before[1] - before[0]));
    }

    #[test]
    fn length_unit_follows_the_visible_size_and_the_unit_system() {
        let ctx = egui::Context::default();
        let (f, acc) = analysis(false);
        let mut view = FieldView::default();
        view.new_result();
        frames(&ctx, &mut view, &f, &acc, 3);
        // the signal region is about 1.3 mm wide
        assert_eq!(view.length_unit(&f).0, "mm");
        view.units = UnitSystem::Imperial;
        assert_eq!(view.length_unit(&f).0, "mil");
        // zooming in changes the prefix, in both systems
        for (b, metric, imperial) in [
            ([-0.02, 0.02, -0.01, 0.01], "µm", "mil"),
            ([-0.001, 0.001, -0.0005, 0.0005], "µm", "µin"),
            ([-5.0, 5.0, -3.0, 3.0], "cm", "mil"),
            ([-40.0, 40.0, -20.0, 20.0], "cm", "in"),
            ([-500.0, 500.0, -300.0, 300.0], "m", "ft"),
        ] {
            view.plot_bounds = Some(b);
            view.units = UnitSystem::Metric;
            assert_eq!(view.length_unit(&f).0, metric, "{b:?}");
            view.units = UnitSystem::Imperial;
            assert_eq!(view.length_unit(&f).0, imperial, "{b:?}");
        }
    }

    #[test]
    fn switching_the_unit_system_does_not_move_the_view() {
        let ctx = egui::Context::default();
        let (f, acc) = analysis(false);
        let mut view = FieldView::default();
        view.new_result();
        frames(&ctx, &mut view, &f, &acc, 4);
        let before = view.plot_bounds.unwrap();
        for k in 0..6 {
            view.units = if k % 2 == 0 { UnitSystem::Imperial } else { UnitSystem::Metric };
            frames(&ctx, &mut view, &f, &acc, 2);
            assert!(
                close(view.plot_bounds.unwrap(), before),
                "switch {k}: {:?} vs {before:?}",
                view.plot_bounds
            );
        }
    }

    #[test]
    fn options_round_trip_through_settings() {
        let mut v = FieldView {
            mode: FieldMode::Potential,
            show_eq: false,
            show_mesh: true,
            db_range: 55.0,
            units: UnitSystem::Imperial,
            ..Default::default()
        };
        let mut ui = BTreeMap::new();
        v.save(&mut ui);
        let mut w = FieldView::default();
        w.load(&ui);
        assert_eq!((w.mode, w.show_eq, w.show_mesh, w.db_range), (FieldMode::Potential, false, true, 55.0));
        assert_eq!(w.units, UnitSystem::Imperial);
        // garbage is ignored
        ui.insert("field_mode".into(), "nonsense".into());
        ui.insert("db_range".into(), 1e9.into());
        ui.insert("length_system".into(), "furlong".into());
        v.load(&ui);
        assert_eq!((v.mode, v.db_range, v.units), (FieldMode::Potential, 55.0, UnitSystem::Imperial));
    }
}
