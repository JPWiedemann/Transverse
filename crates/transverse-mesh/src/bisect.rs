//! Local refinement of an existing mesh: only the marked triangles are split, plus as few neighbours as
//! needed to keep the mesh conforming. All old nodes and all other triangles stay as they are, so the
//! refined mesh is nested in the old one (and the finite-element spaces with it: with every round the
//! capacitance can only fall towards its limit).
//!
//! A marked triangle has all three edges halved (four triangles similar to it). A neighbour that gets a
//! halved edge always halves its longest edge too and is cut into two, three or four triangles through the
//! midpoint of that longest edge first, which keeps the angles bounded over any number of rounds
//! (red-green-blue refinement with longest-edge bisection).

use std::collections::{HashMap, HashSet};

use transverse_core::{TvError, TvResult};

use crate::{BoundaryEdge, Mesh};

type Key = (u32, u32);

fn key(a: u32, b: u32) -> Key {
    (a.min(b), a.max(b))
}

/// The edge of `t` opposite to its vertex `m`.
fn edge(t: [u32; 3], m: usize) -> Key {
    key(t[(m + 1) % 3], t[(m + 2) % 3])
}

/// Index of the vertex opposite to the longest edge; ties are broken by the node numbers, so that the two
/// triangles at an edge agree on it.
fn longest(mesh: &Mesh, t: [u32; 3]) -> usize {
    let len = |m: usize| {
        let (a, b) = edge(t, m);
        let (p, q) = (mesh.pts[a as usize], mesh.pts[b as usize]);
        ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2), (a, b))
    };
    (0..3)
        .max_by(|&i, &j| {
            let (x, y) = (len(i), len(j));
            x.0.total_cmp(&y.0).then(x.1.cmp(&y.1))
        })
        .unwrap_or(0)
}

/// Split the triangles for which `marked` is true. `max_nodes` bounds the number of nodes of the result.
pub fn refine(mesh: &Mesh, marked: &[bool], max_nodes: usize) -> TvResult<Mesh> {
    if marked.len() != mesh.tris.len() {
        return Err(TvError::InvalidParam("refine: one mark per triangle is needed".into()));
    }
    // edges to halve: all edges of the marked triangles, then the closure over the longest edges
    let mut at_edge: HashMap<Key, Vec<usize>> = HashMap::new();
    for (k, &t) in mesh.tris.iter().enumerate() {
        for m in 0..3 {
            at_edge.entry(edge(t, m)).or_default().push(k);
        }
    }
    let mut split: HashSet<Key> = HashSet::new();
    let mut stack: Vec<Key> = Vec::new();
    for (&t, _) in mesh.tris.iter().zip(marked).filter(|(_, m)| **m) {
        stack.extend((0..3).map(|m| edge(t, m)));
    }
    while let Some(e) = stack.pop() {
        if !split.insert(e) {
            continue;
        }
        for &k in at_edge.get(&e).map_or(&[][..], Vec::as_slice) {
            let t = mesh.tris[k];
            let l = edge(t, longest(mesh, t));
            if !split.contains(&l) {
                stack.push(l);
            }
        }
    }
    let nodes = mesh.pts.len() + split.len();
    if nodes > max_nodes {
        return Err(TvError::MeshTooLarge { nodes, max: max_nodes });
    }

    let mut out = Mesh { pts: mesh.pts.clone(), node_cond: mesh.node_cond.clone(), ..Mesh::default() };
    // midpoints, numbered in a fixed order (the set iterates in a random one)
    let mut ordered: Vec<Key> = split.iter().copied().collect();
    ordered.sort_unstable();
    let on_boundary: HashMap<Key, &BoundaryEdge> = mesh.edges.iter().map(|e| (key(e.a, e.b), e)).collect();
    let mut mid: HashMap<Key, u32> = HashMap::with_capacity(ordered.len());
    for (a, b) in ordered {
        let (p, q) = (mesh.pts[a as usize], mesh.pts[b as usize]);
        mid.insert((a, b), out.pts.len() as u32);
        out.pts.push([0.5 * (p[0] + q[0]), 0.5 * (p[1] + q[1])]);
        // on a conductor only if the edge is part of its boundary (not just a chord between two of its nodes)
        out.node_cond.push(on_boundary.get(&(a, b)).map(|e| e.id));
    }
    for e in &mesh.edges {
        match mid.get(&key(e.a, e.b)) {
            Some(&m) => out.edges.extend([BoundaryEdge { b: m, ..*e }, BoundaryEdge { a: m, ..*e }]),
            None => out.edges.push(*e),
        }
    }

    for (k, &t) in mesh.tris.iter().enumerate() {
        let mat = mesh.tri_mat[k];
        let mut push = |tri: [u32; 3]| {
            out.tris.push(tri);
            out.tri_mat.push(mat);
        };
        let l = longest(mesh, t);
        // rotate so that the longest edge is (a, b), opposite to c; the orientation is kept
        let (c, a, b) = (t[l], t[(l + 1) % 3], t[(l + 2) % 3]);
        let Some(&mab) = mid.get(&key(a, b)) else {
            push(t); // no edge of it is halved (the closure halves the longest one first)
            continue;
        };
        let (mbc, mca) = (mid.get(&key(b, c)).copied(), mid.get(&key(c, a)).copied());
        if let (Some(mbc), Some(mca)) = (mbc, mca) {
            // all three: four triangles similar to the old one
            for tri in [[a, mab, mca], [mab, b, mbc], [mca, mbc, c], [mab, mbc, mca]] {
                push(tri);
            }
            continue;
        }
        match mca {
            Some(m) => {
                push([a, mab, m]);
                push([m, mab, c]);
            }
            None => push([a, mab, c]),
        }
        match mbc {
            Some(m) => {
                push([mab, b, m]);
                push([mab, m, c]);
            }
            None => push([mab, b, c]),
        }
    }
    Ok(out)
}
