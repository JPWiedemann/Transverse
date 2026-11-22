//! Sketch of the CPWG cross-section (not to scale), ported from the Python tool's canvas drawing.

use super::GapMode;
use crate::model::{Anchor, SchemColor, SchemItem, Schematic};

/// What the sketch shows (all other parameters do not change the picture).
#[derive(Clone, Copy, Debug)]
pub struct SchematicFlags {
    pub plane1: bool,
    pub plane2: bool,
    pub fence1: bool,
    pub fence2: bool,
    pub gap_mode: GapMode,
}

pub fn schematic(f: &SchematicFlags) -> Schematic {
    use SchemColor::*;
    let (x0, x1) = (40.0, 610.0);
    let (xf0, xf1) = (95.0, 555.0); // fences / coplanar outer ends
    let (yt, y1, yc, y2, yb) = (62.0, 72.0, 162.0, 176.0, 266.0);
    let (cx, hw, s) = (325.0, 70.0, 45.0);
    let gap_col = match f.gap_mode {
        GapMode::Upper => UpperDielectric,
        GapMode::Lower => LowerDielectric,
        GapMode::Custom => CustomGap,
    };
    let mut it: Vec<SchemItem> = Vec::new();
    let rect = |it: &mut Vec<SchemItem>, a: f64, b: f64, c: f64, d: f64, fill, outline| {
        it.push(SchemItem::Rect { x0: a, y0: b, x1: c, y1: d, fill, outline });
    };
    let line = |it: &mut Vec<SchemItem>, a: f64, b: f64, c: f64, d: f64, color, dashed, arrows| {
        it.push(SchemItem::Line { x0: a, y0: b, x1: c, y1: d, color, dashed, arrows });
    };
    let text = |it: &mut Vec<SchemItem>, x: f64, y: f64, t: &str, anchor, bold, color| {
        it.push(SchemItem::Text { x, y, text: t.to_string(), anchor, bold, color });
    };

    text(&mut it, 330.0, 16.0, "CPWG in PCB inner layer (not to scale)", Anchor::Center, true, Text);
    rect(&mut it, x0, y1, x1, yc, UpperDielectric, None);
    rect(&mut it, x0, y2, x1, yb, LowerDielectric, None);
    // copper layer: gaps and the area beyond the coplanar ground are filled with the gap material
    rect(&mut it, x0, yc, x1, y2, gap_col, None);
    if f.plane1 {
        rect(&mut it, x0, yt, x1, y1, Copper, None);
        text(&mut it, x1, yt - 8.0, "Ground plane 1", Anchor::Right, false, Muted);
    } else {
        line(&mut it, x0, y1, x1, y1, Muted, true, false);
        text(&mut it, 330.0, y1 - 22.0, "no ground plane 1  (air)", Anchor::Center, false, Muted);
    }
    if f.plane2 {
        rect(&mut it, x0, yb, x1, yb + 10.0, Copper, None);
        text(&mut it, x1, yb + 22.0, "Ground plane 2", Anchor::Right, false, Muted);
    } else {
        line(&mut it, x0, yb, x1, yb, Muted, true, false);
        text(&mut it, 330.0, yb + 22.0, "no ground plane 2  (air)", Anchor::Center, false, Muted);
    }
    rect(&mut it, x0, yc, cx - hw - s, y2, Copper, None);
    rect(&mut it, cx + hw + s, yc, x1, y2, Copper, None);
    rect(&mut it, cx - hw, yc, cx + hw, y2, Signal, None);
    for xv in [xf0, xf1] {
        if f.fence1 {
            rect(&mut it, xv - 4.0, y1, xv + 4.0, yc, Copper, Some(CopperEdge));
        }
        if f.fence2 {
            rect(&mut it, xv - 4.0, y2, xv + 4.0, yb, Copper, Some(CopperEdge));
        }
    }
    if f.fence1 {
        text(&mut it, xf1 - 8.0, y1 + 14.0, "Via fence 1", Anchor::Right, false, Muted);
    }
    if f.fence2 {
        text(&mut it, xf1 - 8.0, yb - 14.0, "Via fence 2", Anchor::Right, false, Muted);
    }
    text(&mut it, x0 + 70.0, y1 + 20.0, "Upper dielectric:  h1, er1, tan d1", Anchor::Left, false, Text);
    text(&mut it, x0 + 70.0, yb - 20.0, "Lower dielectric:  h2, er2, tan d2", Anchor::Left, false, Text);
    text(&mut it, cx, y2 + 16.0, "Signal", Anchor::Center, true, Text);
    text(&mut it, (xf0 + cx - hw - s) / 2.0, y2 + 14.0, "Coplanar ground", Anchor::Center, false, Text);

    // dimensions
    let ya = yc - 14.0;
    for (xa, xb, lab) in [(cx - hw - s, cx - hw, "s"), (cx - hw, cx + hw, "w"), (cx + hw, cx + hw + s, "s")] {
        line(&mut it, xa, ya, xb, ya, Text, false, true);
        text(&mut it, (xa + xb) / 2.0, ya - 9.0, lab, Anchor::Center, false, Text);
    }
    for xv in [cx - hw - s, cx - hw, cx + hw, cx + hw + s] {
        line(&mut it, xv, ya - 6.0, xv, yc, Muted, true, false);
    }
    let yg = y2 + 34.0;
    line(&mut it, cx + hw + s, y2, cx + hw + s, yg + 4.0, Muted, true, false);
    line(&mut it, xf1, y2, xf1, yg + 4.0, Muted, true, false);
    line(&mut it, cx + hw + s, yg, xf1, yg, Text, false, true);
    text(&mut it, (cx + hw + s + xf1) / 2.0, yg - 9.0, "g", Anchor::Center, false, Text);
    let xd = x1 + 28.0;
    line(&mut it, xd, y1, xd, yc, Text, false, true);
    text(&mut it, xd + 14.0, (y1 + yc) / 2.0, "h1", Anchor::Center, false, Text);
    line(&mut it, xd, y2, xd, yb, Text, false, true);
    text(&mut it, xd + 14.0, (y2 + yb) / 2.0, "h2", Anchor::Center, false, Text);
    line(&mut it, x1 + 10.0, yc, x1 + 10.0, y2, Text, false, false);
    text(&mut it, x1 + 14.0, (yc + y2) / 2.0, "t", Anchor::Left, false, Text);
    line(&mut it, x0 - 6.0, y2, x0, y2, Muted, false, false);
    text(&mut it, x0 - 8.0, y2, "y=0", Anchor::Right, false, Muted);

    // legend
    let ly = 316.0;
    for (i, (col, txt)) in [
        (UpperDielectric, "upper dielectric"),
        (LowerDielectric, "lower dielectric"),
        (CustomGap, "custom gap material"),
        (Copper, "copper"),
    ]
    .into_iter()
    .enumerate()
    {
        let lx = 60.0 + i as f64 * 150.0;
        rect(&mut it, lx, ly - 6.0, lx + 14.0, ly + 6.0, col, Some(Muted));
        text(&mut it, lx + 20.0, ly, txt, Anchor::Left, false, Text);
    }
    Schematic { width: 700.0, height: 340.0, items: it }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(plane1: bool, plane2: bool, fence1: bool, fence2: bool, gap_mode: GapMode) -> SchematicFlags {
        SchematicFlags { plane1, plane2, fence1, fence2, gap_mode }
    }

    fn count(s: &Schematic, pred: impl Fn(&SchemItem) -> bool) -> usize {
        s.items.iter().filter(|i| pred(i)).count()
    }

    #[test]
    fn everything_stays_inside_the_canvas() {
        let s = schematic(&flags(true, true, true, true, GapMode::Custom));
        for i in &s.items {
            let (xs, ys): (Vec<f64>, Vec<f64>) = match i {
                SchemItem::Rect { x0, y0, x1, y1, .. } | SchemItem::Line { x0, y0, x1, y1, .. } => {
                    (vec![*x0, *x1], vec![*y0, *y1])
                }
                SchemItem::Text { x, y, .. } => (vec![*x], vec![*y]),
            };
            assert!(
                xs.iter().all(|x| (0.0..=s.width).contains(x))
                    && ys.iter().all(|y| (0.0..=s.height).contains(y)),
                "{i:?}"
            );
        }
    }

    #[test]
    fn planes_fences_and_gap_material_change_the_picture() {
        let all = schematic(&flags(true, true, true, true, GapMode::Upper));
        let none = schematic(&flags(false, false, false, false, GapMode::Upper));
        let fences = |s: &Schematic| {
            count(s, |i| matches!(i, SchemItem::Rect { outline: Some(SchemColor::CopperEdge), .. }))
        };
        assert_eq!((fences(&all), fences(&none)), (4, 0));
        let has = |s: &Schematic, t: &str| {
            s.items.iter().any(|i| matches!(i, SchemItem::Text { text, .. } if text == t))
        };
        assert!(
            has(&all, "Ground plane 1") && has(&all, "Via fence 2") && !has(&all, "no ground plane 1  (air)")
        );
        assert!(
            !has(&none, "Ground plane 1")
                && has(&none, "no ground plane 2  (air)")
                && !has(&none, "Via fence 1")
        );
        // the layer between signal and ground takes the colour of the selected gap material
        let gap_fill = |m| {
            let s = schematic(&flags(true, true, true, true, m));
            s.items.iter().find_map(|i| match i {
                SchemItem::Rect { y0, y1, x0, fill, .. }
                    if *y0 == 162.0 && *y1 == 176.0 && *x0 == 40.0 && *fill != SchemColor::Copper =>
                {
                    Some(*fill)
                }
                _ => None,
            })
        };
        assert_eq!(gap_fill(GapMode::Upper), Some(SchemColor::UpperDielectric));
        assert_eq!(gap_fill(GapMode::Lower), Some(SchemColor::LowerDielectric));
        assert_eq!(gap_fill(GapMode::Custom), Some(SchemColor::CustomGap));
        // dimension arrows for s, w, s, g, h1, h2
        assert_eq!(count(&all, |i| matches!(i, SchemItem::Line { arrows: true, .. })), 6);
    }
}
