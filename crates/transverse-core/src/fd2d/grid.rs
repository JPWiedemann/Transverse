//! Tensor grid with per-node categories and per-cell material.

/// Geometric tolerance [mm] for matching coordinates to conductor edges.
pub const TOL_GEO: f64 = 1e-9;
/// Node category: free (solved for).
pub const CAT_FREE: u8 = 0;
/// Node category: signal conductor (1 V). Every other category > 0 is a grounded conductor.
pub const CAT_SIGNAL: u8 = 1;

/// Row-major storage: node (i, j) at `j * nx + i`, cell (i, j) at `j * (nx - 1) + i`.
pub struct Grid {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    /// Node categories, `nx * ny`. 0 = free, 1 = signal, > 1 = grounded conductor (line-type specific).
    pub cat: Vec<u8>,
    /// Relative permittivity per cell, `(nx-1) * (ny-1)`.
    pub er: Vec<f64>,
    /// Loss tangent per cell.
    pub tand: Vec<f64>,
}

impl Grid {
    pub fn nx(&self) -> usize {
        self.x.len()
    }
    pub fn ny(&self) -> usize {
        self.y.len()
    }
    pub fn nodes(&self) -> usize {
        self.x.len() * self.y.len()
    }
}
