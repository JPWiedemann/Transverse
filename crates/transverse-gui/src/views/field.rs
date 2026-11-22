//! Field tab: |E| / potential / permittivity / mesh as an image with field lines, equipotentials,
//! conductors and a cursor readout.

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, TextureHandle, TextureOptions, vec2,
};
use egui_plot::{Plot, PlotImage, PlotPoint};
use transverse_core::postproc::EField;
use transverse_core::quasistatic::Accuracy;
use transverse_gui::raster::{FieldMode, Legend, RasterSpec, nice_ticks, probe, render};
use transverse_lines::model::{Bounds, FieldData};

/// Copper colour of the conductors (same as the Python plots).
const COPPER: Color32 = Color32::from_rgb(0xc8, 0x75, 0x33);
const LINE_BLUE: Color32 = Color32::from_rgb(0x9f, 0xd3, 0xff);
const IMAGE_WIDTH: usize = 1100;
const BAR_WIDTH: f32 = 110.0;
const EQUIPOTENTIALS: [f64; 9] = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];

struct RasterCache {
    key: (u64, FieldMode, bool, u64),
    texture: TextureHandle,
    /// Size of the image in plot units and its centre.
    centre: PlotPoint,
    size: [f32; 2],
    legend: Option<Legend>,
    bar: Option<TextureHandle>,
}

pub struct FieldView {
    pub mode: FieldMode,
    pub show_eq: bool,
    pub zoom: bool,
    pub show_mesh: bool,
    pub db_range: f64,
    generation: u64,
    raster: Option<RasterCache>,
    e: Option<(u64, EField)>,
    lines: Option<((u64, bool), Vec<Vec<[f64; 2]>>)>,
    eq: Option<(u64, Vec<[[f64; 2]; 2]>)>,
}

impl Default for FieldView {
    fn default() -> Self {
        Self {
            mode: FieldMode::EDbLines,
            show_eq: true,
            zoom: true,
            show_mesh: false,
            db_range: 40.0,
            generation: 0,
            raster: None,
            e: None,
            lines: None,
            eq: None,
        }
    }
}

impl FieldView {
    /// A new analysis result replaces the old one: drop all cached images and lines.
    pub fn new_result(&mut self) {
        self.generation += 1;
    }

    pub fn load(&mut self, ui: &std::collections::BTreeMap<String, serde_json::Value>) {
        if let Some(m) = ui.get("field_mode").and_then(|v| v.as_str()).and_then(FieldMode::from_key) {
            self.mode = m;
        }
        let flag = |k: &str, d: bool| ui.get(k).and_then(|v| v.as_bool()).unwrap_or(d);
        self.show_eq = flag("show_eq", self.show_eq);
        self.zoom = flag("zoom", self.zoom);
        self.show_mesh = flag("show_mesh", self.show_mesh);
        if let Some(d) = ui.get("db_range").and_then(|v| v.as_f64()).filter(|d| (5.0..=200.0).contains(d)) {
            self.db_range = d;
        }
    }

    pub fn save(&self, ui: &mut std::collections::BTreeMap<String, serde_json::Value>) {
        ui.insert("field_mode".into(), self.mode.key().into());
        ui.insert("show_eq".into(), self.show_eq.into());
        ui.insert("zoom".into(), self.zoom.into());
        ui.insert("show_mesh".into(), self.show_mesh.into());
        ui.insert("db_range".into(), self.db_range.into());
    }

    fn view(&self, f: &FieldData) -> Bounds {
        if self.zoom { f.view_zoom } else { f.view_full }
    }

    /// Build whatever the current options need and the cache does not have yet.
    fn update_caches(&mut self, ctx: &egui::Context, f: &FieldData) {
        let gen_ = self.generation;
        if self.e.as_ref().is_none_or(|(g, _)| *g != gen_) {
            self.e = Some((gen_, f.e_field(false)));
        }
        let view = self.view(f);
        if self.mode.has_field_lines() && self.lines.as_ref().is_none_or(|(k, _)| *k != (gen_, self.zoom)) {
            self.lines = Some(((gen_, self.zoom), f.field_lines(view, (2.2, 1.4))));
        }
        if self.eq.as_ref().is_none_or(|(g, _)| *g != gen_) {
            let segs = f.equipotentials(&EQUIPOTENTIALS).into_iter().flat_map(|(_, s)| s).collect();
            self.eq = Some((gen_, segs));
        }
        let key = (gen_, self.mode, self.zoom, if self.mode.is_db() { self.db_range.to_bits() } else { 0 });
        if self.raster.as_ref().is_none_or(|r| r.key != key) {
            let e = &self.e.as_ref().unwrap().1;
            let r = render(
                f,
                e,
                &RasterSpec { mode: self.mode, view, db_range: self.db_range, width: IMAGE_WIDTH },
            );
            let image = egui::ColorImage::from_rgba_unmultiplied([r.width, r.height], &r.rgba);
            let texture = ctx.load_texture("field", image, TextureOptions::LINEAR);
            // the image covers the view clipped to the grid
            let [vx0, vx1, vy0, vy1] = view;
            let (x0, x1) = (vx0.max(f.x[0]), vx1.min(f.x[f.nx() - 1]));
            let (y0, y1) = (vy0.max(f.y[0]), vy1.min(f.y[f.ny() - 1]));
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
            self.raster = Some(RasterCache {
                key,
                texture,
                centre: PlotPoint::new(0.5 * (x0 + x1), 0.5 * (y0 + y1)),
                size: [(x1 - x0) as f32, (y1 - y0) as f32],
                legend: r.legend,
                bar,
            });
        }
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
            ui.checkbox(&mut self.zoom, "Zoom to signal region");
            ui.add_enabled(
                self.mode != FieldMode::Mesh,
                egui::Checkbox::new(&mut self.show_mesh, "Mesh overlay"),
            );
            if self.mode.is_db() {
                ui.label("Range");
                ui.add(egui::DragValue::new(&mut self.db_range).range(5.0..=200.0).speed(0.5).suffix(" dB"));
            }
        });
        self.update_caches(ui.ctx(), f);
        let show_mesh = self.show_mesh || self.mode == FieldMode::Mesh;

        let mut title = format!("{}  -  1 V on signal, Z0 = {:.2} Ohm", self.mode.label(), f.z0);
        if let (true, Some(a)) = (show_mesh, acc) {
            title += &format!(
                "\nmesh {} x {} = {} nodes (finest level), min. cell {:.2} um",
                a.nx,
                a.ny,
                a.nodes,
                a.dmin_actual * 1e3
            );
        }
        ui.label(title);

        // ---- plot
        let avail = ui.available_size();
        let plot_h = (avail.y - 24.0).max(120.0);
        let plot_w = (avail.x - BAR_WIDTH).max(200.0);
        let view = self.view(f);
        let raster = self.raster.as_ref().unwrap();
        let (gen_, zoom) = (self.generation, self.zoom);
        let (tex_id, centre, size) = (raster.texture.id(), raster.centre, raster.size);
        let mut frame_rect = Rect::NOTHING;
        let mut readout = String::new();
        let e = &self.e.as_ref().unwrap().1;

        ui.horizontal(|ui| {
            let resp = Plot::new(("field", gen_, zoom))
                .width(plot_w)
                .height(plot_h)
                .data_aspect(1.0)
                .show_grid(false)
                .x_axis_label("x [mm]")
                .y_axis_label("y [mm]  (0 = signal layer)")
                .include_x(view[0])
                .include_x(view[1])
                .include_y(view[2])
                .include_y(view[3])
                .show(ui, |pui| {
                    pui.add(PlotImage::new("field", tex_id, centre, size));
                });
            let tr = resp.transform;
            frame_rect = *tr.frame();
            let painter = ui.painter_at(frame_rect);
            let to_screen = |p: [f64; 2]| tr.position_from_point(&PlotPoint::new(p[0], p[1]));

            // field lines with an arrowhead in the middle
            if self.mode.has_field_lines() {
                let stroke = Stroke::new(0.8, LINE_BLUE);
                for line in self.lines.as_ref().map(|l| l.1.as_slice()).unwrap_or(&[]) {
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
            let [bx0, bx1] = [tr.bounds().min()[0], tr.bounds().max()[0]];
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
                let (a, b) = (tr.bounds().min(), tr.bounds().max());
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
                readout = format!("x = {:.4} mm   y = {:.4} mm", v.x, v.y);
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
            if let (Some(l), Some(bar)) = (raster.legend.as_ref(), raster.bar.as_ref()) {
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
        ui.label(if readout.is_empty() { "Move the cursor over the plot for values. Scroll/ctrl+scroll and drag to zoom/pan, double-click to reset.".to_string() } else { readout });
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
            egui::CentralPanel::default().show_inside(ui, |ui| view.ui(ui, f, acc));
        });
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

    #[test]
    fn every_mode_renders_for_closed_and_open_structures_with_all_overlays() {
        let ctx = egui::Context::default();
        for open in [false, true] {
            let (f, acc) = analysis(open);
            let mut view = FieldView::default();
            view.new_result();
            for mode in FieldMode::ALL {
                for zoom in [true, false] {
                    view.mode = mode;
                    view.zoom = zoom;
                    view.show_mesh = true;
                    // two frames: the first builds the caches, the second draws with a hovering cursor
                    run_frame(&ctx, &mut view, &f, Some(&acc), None);
                    run_frame(&ctx, &mut view, &f, Some(&acc), Some(Pos2::new(500.0, 350.0)));
                }
            }
            let r = view.raster.as_ref().unwrap();
            assert!(r.legend.is_none(), "last mode is Mesh");
            assert!(view.lines.is_some() && view.eq.is_some());
            assert!(view.lines.as_ref().unwrap().1.len() > 20);
        }
    }

    #[test]
    fn caches_follow_results_and_options() {
        let ctx = egui::Context::default();
        let (f, acc) = analysis(false);
        let mut view = FieldView::default();
        view.new_result();
        run_frame(&ctx, &mut view, &f, Some(&acc), None);
        let key1 = view.raster.as_ref().unwrap().key;
        view.db_range = 60.0;
        run_frame(&ctx, &mut view, &f, Some(&acc), None);
        assert_ne!(view.raster.as_ref().unwrap().key, key1, "dB range changes the image");
        let lines_key = view.lines.as_ref().unwrap().0;
        view.zoom = false;
        run_frame(&ctx, &mut view, &f, Some(&acc), None);
        assert_ne!(view.lines.as_ref().unwrap().0, lines_key, "field lines depend on the view");
        view.new_result();
        run_frame(&ctx, &mut view, &f, Some(&acc), None);
        assert_eq!(view.e.as_ref().unwrap().0, 2);
    }

    #[test]
    fn options_round_trip_through_settings() {
        let mut v = FieldView {
            mode: FieldMode::Potential,
            show_eq: false,
            zoom: false,
            show_mesh: true,
            db_range: 55.0,
            ..Default::default()
        };
        let mut ui = std::collections::BTreeMap::new();
        v.save(&mut ui);
        let mut w = FieldView::default();
        w.load(&ui);
        assert_eq!(
            (w.mode, w.show_eq, w.zoom, w.show_mesh, w.db_range),
            (FieldMode::Potential, false, false, true, 55.0)
        );
        // garbage is ignored
        ui.insert("field_mode".into(), "nonsense".into());
        ui.insert("db_range".into(), 1e9.into());
        v.load(&ui);
        assert_eq!((v.mode, v.db_range), (FieldMode::Potential, 55.0));
    }
}
