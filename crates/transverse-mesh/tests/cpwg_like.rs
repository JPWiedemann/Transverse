//! The mesher on CPWG-like cross-sections (built here by hand, the line crate is not needed):
//! area, conformity, angles, grading, size field, boundary tags, limits, cancel.

use transverse_core::{Ctl, TvError};
use transverse_geom::{CrossSection, Far, Item, Material, Paint, SIGNAL, Shape, SizeSpec};
use transverse_mesh::planar::build;
use transverse_mesh::quality::{max_neighbour_edge_ratio, min_angle_deg, total_area};
use transverse_mesh::sizefield::SizeField;
use transverse_mesh::{Mesh, Region, mesh, region_of};

// conductor tags
const T_SIGNAL: u8 = 1;
const T_PLANE1: u8 = 2;
const T_PLANE2: u8 = 3;
const T_COP: u8 = 4;
const T_FENCE1: u8 = 5;
const T_FENCE2: u8 = 6;
const T_BOX: u8 = 7;

#[derive(Clone, Copy)]
struct Geo {
    w: f64,
    s: f64,
    t: f64,
    h1: f64,
    h2: f64,
    g: f64,
    margin: f64,
    planes: (bool, bool),
    fences: (bool, bool),
}

impl Geo {
    fn stackup() -> Self {
        Self {
            w: 0.3605,
            s: 0.11,
            t: 0.018,
            h1: 0.204,
            h2: 0.254,
            g: 1.0,
            margin: 0.0,
            planes: (true, true),
            fences: (true, true),
        }
    }
    fn xe(&self) -> f64 {
        self.w / 2.0 + self.s + self.g
    }
    fn closed(&self) -> bool {
        self.planes.0 && self.planes.1 && self.fences.0 && self.fences.1
    }
    fn spec(&self) -> SizeSpec {
        let hmin = self.h1.min(self.h2);
        let dmin = (self.s / 30.0).min(self.w / 60.0).min(hmin / 60.0).min(if self.t > 0.0 {
            self.t / 6.0
        } else {
            f64::INFINITY
        });
        let dmax = (hmin / 20.0).min((self.w + 2.0 * self.s) / 20.0).max(2.0 * dmin);
        // like the FD grid: coarse cells (margin / 6) in the open space around the board
        let far = (!self.closed()).then(|| Far {
            core: [-self.xe(), self.xe(), -self.h2, self.t + self.h1],
            dfar: (self.margin / 6.0).max(dmax),
        });
        SizeSpec { dmin, dmax, growth: 1.25, far }
    }
    /// The cross-section like `CpwgParams` would produce it (same domain and painting rules as the FD grid).
    fn section(&self) -> CrossSection {
        let (xe, t) = (self.xe(), self.t);
        let (ytop, ybot) = (t + self.h1, -self.h2);
        let m = if self.closed() { 0.0 } else { self.margin };
        let xd = xe + m;
        let ymax = if self.planes.0 { ytop } else { ytop + m };
        let ymin = if self.planes.1 { ybot } else { ybot - m };
        let materials = vec![
            Material { er: 1.0, tand: 0.0 },
            Material { er: 3.52, tand: 0.02 },
            Material { er: 3.66, tand: 0.02 },
        ];
        let cond = |shape, id, tag| Item { shape, paint: Paint::Conductor { id, tag } };
        let mut items = vec![
            Item { shape: Shape::rect(-xd, ybot, xd, 0.0), paint: Paint::Dielectric(2) },
            Item { shape: Shape::rect(-xd, t, xd, ytop), paint: Paint::Dielectric(1) },
        ];
        if t > 0.0 {
            items.push(Item { shape: Shape::rect(-xd, 0.0, xd, t), paint: Paint::Dielectric(1) });
        }
        // painting order like the FD node categories: fences, planes, coplanar ground, signal
        if self.fences.0 {
            for sx in [-xe, xe] {
                items.push(cond(Shape::segment([sx, t], [sx, ytop]), 0, T_FENCE1));
            }
        }
        if self.fences.1 {
            for sx in [-xe, xe] {
                items.push(cond(Shape::segment([sx, ybot], [sx, 0.0]), 0, T_FENCE2));
            }
        }
        if self.planes.0 {
            items.push(cond(Shape::segment([-xd, ytop], [xd, ytop]), 0, T_PLANE1));
        }
        if self.planes.1 {
            items.push(cond(Shape::segment([-xd, ybot], [xd, ybot]), 0, T_PLANE2));
        }
        let (a, b) = (self.w / 2.0 + self.s, xd);
        if t > 0.0 {
            items.push(cond(Shape::rect(a, 0.0, b, t), 0, T_COP));
            items.push(cond(Shape::rect(-b, 0.0, -a, t), 0, T_COP));
            items.push(cond(Shape::rect(-self.w / 2.0, 0.0, self.w / 2.0, t), SIGNAL, T_SIGNAL));
        } else {
            items.push(cond(Shape::segment([a, 0.0], [b, 0.0]), 0, T_COP));
            items.push(cond(Shape::segment([-b, 0.0], [-a, 0.0]), 0, T_COP));
            items.push(cond(Shape::segment([-self.w / 2.0, 0.0], [self.w / 2.0, 0.0]), SIGNAL, T_SIGNAL));
        }
        CrossSection {
            domain: [-xd, xd, ymin, ymax],
            background: 0,
            materials,
            items,
            box_grounded: true,
            box_tag: T_BOX,
        }
    }
    /// Domain area minus the copper that has thickness.
    fn expected_area(&self) -> f64 {
        let m = if self.closed() { 0.0 } else { self.margin };
        let xd = self.xe() + m;
        let (ymax, ymin) = (
            if self.planes.0 { self.t + self.h1 } else { self.t + self.h1 + m },
            if self.planes.1 { -self.h2 } else { -self.h2 - m },
        );
        let copper = self.t * (self.w + 2.0 * (xd - self.w / 2.0 - self.s));
        2.0 * xd * (ymax - ymin) - copper
    }
}

fn rel(a: f64, b: f64) -> f64 {
    (a / b - 1.0).abs()
}

fn run(g: &Geo) -> Mesh {
    mesh(&g.section(), &g.spec(), 2_000_000, &Ctl::new()).unwrap()
}

/// Length of the boundary edges with a tag.
fn tag_length(m: &Mesh, tag: u8) -> f64 {
    m.edges
        .iter()
        .filter(|e| e.tag == tag)
        .map(|e| {
            let (a, b) = (m.pts[e.a as usize], m.pts[e.b as usize]);
            (a[0] - b[0]).hypot(a[1] - b[1])
        })
        .sum()
}

#[test]
fn closed_stackup_has_the_right_area_angles_and_grading() {
    let g = Geo::stackup();
    let m = run(&g);
    println!("final stackup: {} nodes, {} triangles", m.pts.len(), m.tris.len());
    assert!(rel(total_area(&m), g.expected_area()) < 1e-10, "{} vs {}", total_area(&m), g.expected_area());
    assert!(min_angle_deg(&m) >= 24.9, "min angle {}", min_angle_deg(&m));
    assert!(max_neighbour_edge_ratio(&m) < 6.0, "grading {}", max_neighbour_edge_ratio(&m));
    assert_eq!(m.tris.len(), m.tri_mat.len());
    assert_eq!(m.pts.len(), m.node_cond.len());
    // counter-clockwise triangles only
    assert!(m.tris.iter().enumerate().all(|(k, _)| transverse_mesh::quality::tri_area(&m, k) > 0.0));
}

#[test]
fn no_triangle_straddles_an_interface_and_materials_follow_the_painting() {
    let g = Geo::stackup();
    let cs = g.section();
    let m = run(&g);
    for (k, t) in m.tris.iter().enumerate() {
        let p = [m.pts[t[0] as usize], m.pts[t[1] as usize], m.pts[t[2] as usize]];
        let c = [(p[0][0] + p[1][0] + p[2][0]) / 3.0, (p[0][1] + p[1][1] + p[2][1]) / 3.0];
        let Region::Dielectric(mat) = region_of(&cs, c) else { panic!("triangle {k} lies in a conductor") };
        assert_eq!(mat, m.tri_mat[k]);
        // points pulled slightly towards each corner are in the same material
        for v in p {
            let q = [c[0] + 0.9 * (v[0] - c[0]), c[1] + 0.9 * (v[1] - c[1])];
            let Region::Dielectric(mq) = region_of(&cs, q) else {
                panic!("near-corner point of triangle {k} in a conductor")
            };
            assert_eq!(mq, mat, "triangle {k} straddles an interface");
        }
    }
}

#[test]
fn boundary_edges_cover_every_conductor_piece_with_the_right_tag() {
    let g = Geo::stackup();
    let m = run(&g);
    let (xe, t) = (g.xe(), g.t);
    let tol = 1e-12;
    assert!(rel(tag_length(&m, T_SIGNAL), 2.0 * g.w + 2.0 * t) < tol);
    assert!(rel(tag_length(&m, T_PLANE1), 2.0 * xe) < tol);
    assert!(rel(tag_length(&m, T_PLANE2), 2.0 * xe) < tol);
    assert!(rel(tag_length(&m, T_FENCE1), 2.0 * g.h1) < tol);
    assert!(rel(tag_length(&m, T_FENCE2), 2.0 * g.h2) < tol);
    // coplanar ground: top, bottom and flank on both sides; the outer edge on the box has no dielectric
    // next to it, so it is no boundary of the mesh
    assert!(rel(tag_length(&m, T_COP), 4.0 * g.g + 2.0 * t) < tol);
    // everything on the box is covered by something with a higher rank in a closed structure
    assert_eq!(tag_length(&m, T_BOX), 0.0);
    // Dirichlet nodes: the signal nodes carry the signal, everything else on a conductor is ground
    let signal_nodes = m.node_cond.iter().filter(|c| **c == Some(SIGNAL)).count();
    assert!(signal_nodes > 10);
    for e in &m.edges {
        let want = if e.tag == T_SIGNAL { Some(SIGNAL) } else { Some(0) };
        if e.tag != T_SIGNAL {
            // ground edges may share a corner node with the signal only if the geometry touches
            assert!(
                [e.a, e.b]
                    .iter()
                    .all(|n| m.node_cond[*n as usize] == want || m.node_cond[*n as usize] == Some(SIGNAL))
            );
        }
    }
}

#[test]
fn the_size_field_is_respected_everywhere_outside_conductors() {
    let g = Geo::stackup();
    let cs = g.section();
    let m = run(&g);
    let field = SizeField::new(&build(&cs).unwrap(), g.spec());
    for t in &m.tris {
        let p = [m.pts[t[0] as usize], m.pts[t[1] as usize], m.pts[t[2] as usize]];
        let c = [(p[0][0] + p[1][0] + p[2][0]) / 3.0, (p[0][1] + p[1][1] + p[2][1]) / 3.0];
        let longest = (0..3)
            .map(|k| (p[k][0] - p[(k + 1) % 3][0]).hypot(p[k][1] - p[(k + 1) % 3][1]))
            .fold(0.0, f64::max);
        assert!(longest <= field.at(c) * (1.0 + 1e-9), "triangle at {c:?}: {longest} > {}", field.at(c));
    }
}

#[test]
fn zero_thickness_conductors_work_too() {
    let g = Geo { t: 0.0, w: 0.248, s: 0.15, h1: 0.25, h2: 0.25, ..Geo::stackup() };
    let m = run(&g);
    assert!(rel(total_area(&m), g.expected_area()) < 1e-10);
    assert!(min_angle_deg(&m) >= 24.9);
    // the signal line is a chain of edges of total length w, with triangles on both sides
    assert!(rel(tag_length(&m, T_SIGNAL), g.w) < 1e-12);
    assert!(rel(tag_length(&m, T_COP), 2.0 * g.g) < 1e-12);
}

#[test]
fn open_structures_with_missing_planes_mesh_with_air_above() {
    let g = Geo { planes: (false, true), fences: (false, true), margin: 3.0, ..Geo::stackup() };
    let m = run(&g);
    assert!(rel(total_area(&m), g.expected_area()) < 1e-10);
    assert!(min_angle_deg(&m) >= 24.9);
    // some triangles are air (material 0), and the far field is coarser than near the strip
    assert!(m.tri_mat.contains(&0));
    println!("open: {} nodes", m.pts.len());
    assert!(m.pts.len() < 400_000);
}

#[test]
fn the_node_limit_and_cancel_are_honoured() {
    let g = Geo::stackup();
    match mesh(&g.section(), &g.spec(), 1000, &Ctl::new()) {
        Err(TvError::MeshTooLarge { max, .. }) => assert_eq!(max, 1000),
        other => panic!("expected MeshTooLarge, got {:?}", other.map(|m| m.pts.len())),
    }
    let ctl = Ctl::new();
    ctl.cancel();
    assert!(matches!(mesh(&g.section(), &g.spec(), 2_000_000, &ctl), Err(TvError::Cancelled)));
    // invalid input is reported, not panicked on
    let mut bad = g.section();
    bad.domain = [1.0, 1.0, 0.0, 1.0];
    assert!(mesh(&bad, &g.spec(), 1000, &Ctl::new()).is_err());
    assert!(
        mesh(&g.section(), &SizeSpec { dmin: 0.0, dmax: 1.0, growth: 1.25, far: None }, 1000, &Ctl::new())
            .is_err()
    );
}

#[test]
fn finer_size_specs_give_more_nodes_at_roughly_the_expected_rate() {
    let g = Geo::stackup();
    let n1 = mesh(&g.section(), &g.spec().scaled(1.0), 2_000_000, &Ctl::new()).unwrap().pts.len();
    let n2 = mesh(&g.section(), &g.spec().scaled(0.5), 2_000_000, &Ctl::new()).unwrap().pts.len();
    let ratio = n2 as f64 / n1 as f64;
    println!("nodes {n1} -> {n2}, ratio {ratio:.2}");
    // halving all sizes quadruples the node count in 2D (less where the geometry is fixed by features)
    assert!((2.5..=5.0).contains(&ratio), "ratio {ratio}");
}

/// Small deterministic generator, so the property test needs no extra crate.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self, lo: f64, hi: f64) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        lo + (hi - lo) * ((self.0 >> 11) as f64 / (1u64 << 53) as f64)
    }
}

#[test]
fn perturbed_geometries_keep_area_conformity_and_angles() {
    let mut r = Lcg(12345);
    for k in 0..12 {
        let w = r.next(0.15, 0.6);
        let g = Geo {
            w,
            s: r.next(0.06, 0.3),
            t: if k % 4 == 0 { 0.0 } else { r.next(0.01, 0.05) },
            h1: r.next(0.12, 0.4),
            h2: r.next(0.12, 0.4),
            g: r.next(0.4, 1.2),
            margin: r.next(1.0, 3.0),
            planes: (true, k % 3 != 0),
            fences: (true, k % 3 != 0),
        };
        // coarser than the production sizes, to keep the test fast
        let spec = g.spec().scaled(2.0);
        let cs = g.section();
        let m = mesh(&cs, &spec, 2_000_000, &Ctl::new()).unwrap();
        assert!(rel(total_area(&m), g.expected_area()) < 1e-9, "case {k}");
        assert!(min_angle_deg(&m) >= 24.9, "case {k}: min angle {}", min_angle_deg(&m));
        assert!(
            rel(tag_length(&m, T_SIGNAL), if g.t > 0.0 { 2.0 * g.w + 2.0 * g.t } else { g.w }) < 1e-10,
            "case {k}"
        );
        for t in m.tris.iter().step_by(37) {
            let p = [m.pts[t[0] as usize], m.pts[t[1] as usize], m.pts[t[2] as usize]];
            let c = [(p[0][0] + p[1][0] + p[2][0]) / 3.0, (p[0][1] + p[1][1] + p[2][1]) / 3.0];
            let Region::Dielectric(mat) = region_of(&cs, c) else {
                panic!("case {k}: triangle in a conductor")
            };
            for v in p {
                let q = [c[0] + 0.9 * (v[0] - c[0]), c[1] + 0.9 * (v[1] - c[1])];
                assert!(
                    matches!(region_of(&cs, q), Region::Dielectric(mq) if mq == mat),
                    "case {k}: straddles an interface"
                );
            }
        }
    }
}
