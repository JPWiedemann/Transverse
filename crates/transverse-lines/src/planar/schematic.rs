//! Sketch of the CPWG cross-section (not to scale), ported from the Python tool's canvas drawing.

use super::{GapMode, PlanarParams};
use crate::model::{Anchor, SchemColor, SchemItem, Schematic};

/// What the sketch shows (all other parameters do not change the picture).
#[derive(Clone, Copy, Debug)]
pub struct SchematicFlags {
    pub coplanar: bool,
    pub upper: bool,
    /// The upper dielectric has the material of the lower one.
    pub upper_same: bool,
    pub side_walls: bool,
    pub plane1: bool,
    pub plane2: bool,
    pub fence1: bool,
    pub fence2: bool,
    pub gap_mode: GapMode,
}

impl SchematicFlags {
    /// The flags of the structure that `p` describes (switches resolved).
    pub fn of(p: &PlanarParams) -> Self {
        let e = p.effective();
        Self {
            coplanar: e.coplanar,
            upper: e.upper,
            upper_same: e.upper_same,
            side_walls: e.side_walls,
            plane1: e.plane1,
            plane2: e.plane2,
            fence1: e.fence1,
            fence2: e.fence2,
            gap_mode: e.gap_mode,
        }
    }
}

pub fn schematic(f: &SchematicFlags) -> Schematic {
    use SchemColor::*;
    let (x0, x1) = (40.0, 610.0);
    let (xf0, xf1) = (95.0, 555.0); // fences / coplanar outer ends
    let (yt, y1, yc, y2, yb) = (62.0, 72.0, 162.0, 176.0, 266.0);
    let (cx, hw, s) = (325.0, 70.0, 45.0);
    // the same material is drawn in the same colour
    let upper_col = if f.upper_same { LowerDielectric } else { UpperDielectric };
    let gap_col = match f.gap_mode {
        GapMode::Upper => upper_col,
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

    text(&mut it, 330.0, 16.0, "Cross-section (not to scale)", Anchor::Center, true, Text);
    if f.upper {
        rect(&mut it, x0, y1, x1, yc, upper_col, None);
    }
    rect(&mut it, x0, y2, x1, yb, LowerDielectric, None);
    // copper layer: what is not copper is filled with the gap material (air for the upper one without an upper half)
    if f.upper || f.gap_mode != GapMode::Upper {
        rect(&mut it, x0, yc, x1, y2, gap_col, None);
    }
    if !f.upper {
        text(&mut it, 330.0, y1 + 20.0, "air", Anchor::Center, false, Muted);
    } else if f.plane1 {
        rect(&mut it, x0, yt, x1, y1, Conductor, None);
        text(&mut it, x1, yt - 8.0, "Ground plane 1", Anchor::Right, false, Muted);
    } else {
        line(&mut it, x0, y1, x1, y1, Muted, true, false);
        text(&mut it, 330.0, y1 - 22.0, "no ground plane 1  (air)", Anchor::Center, false, Muted);
    }
    if f.plane2 {
        rect(&mut it, x0, yb, x1, yb + 10.0, Conductor, None);
        text(&mut it, x1, yb + 22.0, "Ground plane 2", Anchor::Right, false, Muted);
    } else {
        line(&mut it, x0, yb, x1, yb, Muted, true, false);
        text(&mut it, 330.0, yb + 22.0, "no ground plane 2  (air)", Anchor::Center, false, Muted);
    }
    if f.coplanar {
        rect(&mut it, x0, yc, cx - hw - s, y2, Conductor, None);
        rect(&mut it, cx + hw + s, yc, x1, y2, Conductor, None);
    }
    rect(&mut it, cx - hw, yc, cx + hw, y2, Signal, None);
    for xv in [xf0, xf1] {
        if f.fence1 {
            rect(&mut it, xv - 4.0, y1, xv + 4.0, yc, Conductor, Some(ConductorEdge));
        }
        if f.fence2 {
            rect(&mut it, xv - 4.0, y2, xv + 4.0, yb, Conductor, Some(ConductorEdge));
        }
        if f.side_walls {
            rect(&mut it, xv - 4.0, y1, xv + 4.0, yb, Conductor, Some(ConductorEdge));
        }
    }
    if f.side_walls {
        text(&mut it, xf1 - 8.0, y1 + 14.0, "Via fences", Anchor::Right, false, Muted);
    }
    if f.fence1 {
        text(&mut it, xf1 - 8.0, y1 + 14.0, "Via fence 1", Anchor::Right, false, Muted);
    }
    if f.fence2 {
        text(&mut it, xf1 - 8.0, yb - 14.0, "Via fence 2", Anchor::Right, false, Muted);
    }
    if f.upper {
        let label = if f.upper_same {
            "Upper dielectric:  h1, er2, tan d2 (as lower)"
        } else {
            "Upper dielectric:  h1, er1, tan d1"
        };
        text(&mut it, x0 + 70.0, y1 + 20.0, label, Anchor::Left, false, Text);
    }
    text(&mut it, x0 + 70.0, yb - 20.0, "Lower dielectric:  h2, er2, tan d2", Anchor::Left, false, Text);
    text(&mut it, cx, y2 + 16.0, "Signal", Anchor::Center, true, Text);
    if f.coplanar {
        text(&mut it, (xf0 + cx - hw - s) / 2.0, y2 + 14.0, "Coplanar ground", Anchor::Center, false, Text);
    }

    // dimensions
    let ya = yc - 14.0;
    // the edges next to the trace: the gap edges of a coplanar ground, or the trace edges alone
    let spans: &[(f64, f64, &str)] = if f.coplanar {
        &[(cx - hw - s, cx - hw, "s"), (cx - hw, cx + hw, "w"), (cx + hw, cx + hw + s, "s")]
    } else {
        &[(cx - hw, cx + hw, "w")]
    };
    for &(xa, xb, lab) in spans {
        line(&mut it, xa, ya, xb, ya, Text, false, true);
        text(&mut it, (xa + xb) / 2.0, ya - 9.0, lab, Anchor::Center, false, Text);
        for xv in [xa, xb] {
            line(&mut it, xv, ya - 6.0, xv, yc, Muted, true, false);
        }
    }
    if f.fence1 || f.fence2 || f.side_walls {
        let inner = if f.coplanar { cx + hw + s } else { cx + hw };
        let yg = y2 + 34.0;
        line(&mut it, inner, y2, inner, yg + 4.0, Muted, true, false);
        line(&mut it, xf1, y2, xf1, yg + 4.0, Muted, true, false);
        line(&mut it, inner, yg, xf1, yg, Text, false, true);
        text(&mut it, (inner + xf1) / 2.0, yg - 9.0, "g", Anchor::Center, false, Text);
    }
    let xd = x1 + 28.0;
    // reference levels of the heights: h1 and h2 count from the copper surfaces to the planes
    let mut level = |from: f64, y: f64| line(&mut it, from, y, xd + 6.0, y, Muted, true, false);
    level(cx + hw, yc);
    level(cx + hw, y2);
    level(x1, yb);
    if f.upper {
        level(x1, y1);
    }
    if f.upper {
        line(&mut it, xd, y1, xd, yc, Text, false, true);
        text(&mut it, xd + 14.0, (y1 + yc) / 2.0, "h1", Anchor::Center, false, Text);
    }
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
        (Conductor, "conductor"),
        (Signal, "signal conductor"),
    ]
    .into_iter()
    .enumerate()
    {
        let lx = 16.0 + i as f64 * 138.0;
        rect(&mut it, lx, ly - 6.0, lx + 14.0, ly + 6.0, col, Some(Muted));
        text(&mut it, lx + 20.0, ly, txt, Anchor::Left, false, Text);
    }
    Schematic { width: 700.0, height: 340.0, items: it }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(plane1: bool, plane2: bool, fence1: bool, fence2: bool, gap_mode: GapMode) -> SchematicFlags {
        SchematicFlags {
            coplanar: true,
            upper: true,
            upper_same: false,
            side_walls: false,
            plane1,
            plane2,
            fence1,
            fence2,
            gap_mode,
        }
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
    fn legend_names_the_conductors_and_the_signal() {
        let s = schematic(&flags(true, true, true, true, GapMode::Upper));
        let legend: Vec<(&str, f64)> = s
            .items
            .iter()
            .filter_map(|i| match i {
                SchemItem::Text { text, y, x, .. } if *y == 316.0 => Some((text.as_str(), *x)),
                _ => None,
            })
            .collect();
        let names: Vec<&str> = legend.iter().map(|l| l.0).collect();
        assert_eq!(
            names,
            ["upper dielectric", "lower dielectric", "custom gap material", "conductor", "signal conductor"]
        );
        // each entry has its own swatch colour and the signal swatch uses the signal colour
        let swatches: Vec<SchemColor> = s
            .items
            .iter()
            .filter_map(|i| match i {
                SchemItem::Rect { y0, fill, outline: Some(SchemColor::Muted), .. } if *y0 == 310.0 => {
                    Some(*fill)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            swatches,
            [
                SchemColor::UpperDielectric,
                SchemColor::LowerDielectric,
                SchemColor::CustomGap,
                SchemColor::Conductor,
                SchemColor::Signal
            ]
        );
        // entries do not overlap: an 11 pt proportional font is about 6 points per character
        for w in legend.windows(2) {
            let end = w[0].1 + 6.0 * (w[0].0.len() as f64);
            assert!(end < w[1].1 - 20.0, "{w:?}");
        }
        assert!(
            !s.items.iter().any(|i| matches!(i, SchemItem::Text { text, .. } if text.contains("copper")))
        );
    }

    #[test]
    fn upper_half_with_the_lower_material_looks_like_the_lower_one() {
        let s =
            schematic(&SchematicFlags { upper_same: true, ..flags(true, true, true, true, GapMode::Upper) });
        // the legend swatches are the only other rectangles and have an outline
        assert_eq!(
            count(&s, |i| matches!(
                i,
                SchemItem::Rect { fill: SchemColor::UpperDielectric, outline: None, .. }
            )),
            0
        );
        // upper half, lower half and the gap material beside the trace
        assert_eq!(
            count(&s, |i| matches!(
                i,
                SchemItem::Rect { fill: SchemColor::LowerDielectric, outline: None, .. }
            )),
            3
        );
        assert!(s.items.iter().any(|i| matches!(i, SchemItem::Text { text, .. }
            if text == "Upper dielectric:  h1, er2, tan d2 (as lower)")));
    }

    #[test]
    fn planes_fences_and_gap_material_change_the_picture() {
        let all = schematic(&flags(true, true, true, true, GapMode::Upper));
        let none = schematic(&flags(false, false, false, false, GapMode::Upper));
        let fences = |s: &Schematic| {
            count(s, |i| matches!(i, SchemItem::Rect { outline: Some(SchemColor::ConductorEdge), .. }))
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
                    if *y0 == 162.0 && *y1 == 176.0 && *x0 == 40.0 && *fill != SchemColor::Conductor =>
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
