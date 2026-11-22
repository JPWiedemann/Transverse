//! Plot legend with line samples, so solid and dashed lines can be told apart
//! (egui_plot's own legend only shows a coloured dot).

use eframe::egui::{self, Color32, FontId, Pos2, Rect, Shape, Stroke, vec2};

pub struct LegendEntry {
    pub label: String,
    pub color: Color32,
    pub dashed: bool,
    pub width: f32,
}

const ROW: f32 = 17.0;
const SAMPLE: f32 = 30.0;
const PAD: f32 = 7.0;

/// Draw the legend in the top-right corner of `frame`, `top_offset` points below its upper edge.
pub fn draw(ui: &egui::Ui, frame: Rect, top_offset: f32, entries: &[LegendEntry]) {
    if entries.is_empty() {
        return;
    }
    let painter = ui.painter_at(frame);
    let font = FontId::proportional(12.0);
    let text_col = ui.visuals().text_color();
    let galleys: Vec<_> =
        entries.iter().map(|e| painter.layout_no_wrap(e.label.clone(), font.clone(), text_col)).collect();
    let text_w = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max);
    let size = vec2(PAD + SAMPLE + 6.0 + text_w + PAD, entries.len() as f32 * ROW + 2.0 * PAD - 4.0);
    let rect =
        Rect::from_min_size(Pos2::new(frame.right() - size.x - 8.0, frame.top() + 8.0 + top_offset), size);
    painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color.gamma_multiply(0.85));
    painter.rect_stroke(rect, 3.0, ui.visuals().widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
    for (i, (e, g)) in entries.iter().zip(galleys).enumerate() {
        let y = rect.top() + PAD + i as f32 * ROW + ROW / 2.0 - 2.0;
        let (a, b) = (Pos2::new(rect.left() + PAD, y), Pos2::new(rect.left() + PAD + SAMPLE, y));
        let stroke = Stroke::new(e.width.max(1.2), e.color);
        if e.dashed {
            painter.extend(Shape::dashed_line(&[a, b], stroke, 6.0, 4.0));
        } else {
            painter.line_segment([a, b], stroke);
        }
        painter.galley(Pos2::new(b.x + 6.0, y - g.size().y / 2.0), g, text_col);
    }
}
