//! Cross-section tab: draws a line type's [`Schematic`] scaled to the available space.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, vec2};
use transverse_lines::model::{Anchor, SchemColor, SchemItem, Schematic};

fn colour(c: SchemColor) -> Color32 {
    match c {
        SchemColor::UpperDielectric => Color32::from_rgb(0xe6, 0xc9, 0x7a),
        SchemColor::LowerDielectric => Color32::from_rgb(0x7f, 0xb0, 0xe6),
        SchemColor::CustomGap => Color32::from_rgb(0x93, 0xd1, 0x7f),
        SchemColor::Copper => Color32::from_rgb(0xd9, 0x82, 0x4f),
        SchemColor::CopperEdge => Color32::from_rgb(0x8a, 0x4a, 0x25),
        SchemColor::Signal => Color32::from_rgb(0x1f, 0x5f, 0xbf),
        SchemColor::Text => Color32::from_gray(0x22),
        SchemColor::Muted => Color32::from_gray(0x66),
    }
}

fn align(a: Anchor) -> Align2 {
    match a {
        Anchor::Center => Align2::CENTER_CENTER,
        Anchor::Left => Align2::LEFT_CENTER,
        Anchor::Right => Align2::RIGHT_CENTER,
    }
}

/// Scale and offset that fit a `sw` x `sh` sketch (plus a margin) into `rect`, centred.
pub fn fit(rect: Rect, sw: f64, sh: f64) -> (f32, Pos2) {
    let margin = 24.0;
    let scale = ((rect.width() as f64 - 2.0 * margin) / sw)
        .min((rect.height() as f64 - 2.0 * margin) / sh)
        .max(0.05) as f32;
    let origin = rect.center() - vec2(sw as f32, sh as f32) * scale * 0.5;
    (scale, origin)
}

fn arrow_heads(painter: &egui::Painter, a: Pos2, b: Pos2, size: f32, col: Color32) {
    let d = (b - a).normalized();
    if !d.is_finite() {
        return;
    }
    let n = vec2(-d.y, d.x) * size * 0.4;
    for (tip, dir) in [(a, d), (b, -d)] {
        let base = tip + dir * size;
        painter.add(Shape::convex_polygon(vec![tip, base + n, base - n], col, Stroke::NONE));
    }
}

pub fn show(ui: &mut egui::Ui, s: &Schematic) {
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_gray(0xfa));
    let (scale, origin) = fit(rect, s.width, s.height);
    let p = |x: f64, y: f64| origin + vec2(x as f32, y as f32) * scale;
    let font =
        |bold: bool| FontId::proportional((11.0 * scale).clamp(9.0, 20.0) * if bold { 1.1 } else { 1.0 });
    let line_w = (1.2 * scale).clamp(0.8, 2.5);

    for item in &s.items {
        match item {
            SchemItem::Rect { x0, y0, x1, y1, fill, outline } => {
                let r = Rect::from_two_pos(p(*x0, *y0), p(*x1, *y1));
                painter.rect_filled(r, 0.0, colour(*fill));
                if let Some(o) = outline {
                    painter.rect_stroke(r, 0.0, Stroke::new(1.0, colour(*o)), egui::StrokeKind::Inside);
                }
            }
            SchemItem::Line { x0, y0, x1, y1, color, dashed, arrows } => {
                let (a, b) = (p(*x0, *y0), p(*x1, *y1));
                let stroke = Stroke::new(line_w, colour(*color));
                if *dashed {
                    painter.extend(Shape::dashed_line(
                        &[a, b],
                        stroke,
                        4.0 * scale.max(0.8),
                        3.0 * scale.max(0.8),
                    ));
                } else {
                    painter.line_segment([a, b], stroke);
                }
                if *arrows {
                    arrow_heads(&painter, a, b, (7.0 * scale).clamp(5.0, 12.0), colour(*color));
                }
            }
            SchemItem::Text { x, y, text, anchor, bold, color } => {
                painter.text(p(*x, *y), align(*anchor), text, font(*bold), colour(*color));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_centres_and_keeps_the_aspect_ratio() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), vec2(1000.0, 400.0));
        let (scale, origin) = fit(rect, 700.0, 340.0);
        // height is the limit: (400 - 48) / 340
        assert!((scale - 352.0 / 340.0).abs() < 1e-5);
        let centre = origin + vec2(350.0, 170.0) * scale;
        assert!((centre - rect.center()).length() < 1e-3);
        // a tiny area never gives a zero or negative scale
        let (s, _) = fit(Rect::from_min_size(Pos2::ZERO, vec2(10.0, 10.0)), 700.0, 340.0);
        assert!(s >= 0.05);
    }
}
