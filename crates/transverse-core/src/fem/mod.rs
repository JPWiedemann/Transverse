//! P1 (linear) finite elements for the quasi-static Laplace problem on a triangle mesh.
//!
//! The functions take plain slices (nodes, triangles, per-triangle permittivity, fixed potentials), so
//! this module does not depend on the mesher. Lengths are in mm; capacitances come out per metre,
//! exactly as in the finite-difference solver.

mod converge;
mod flux;
mod solve;

pub use converge::{Level, LevelRun, auto_margin_with, converge};
pub use flux::{BoundaryFlux, boundary_fluxes};
pub use solve::{FemInput, Lumped, Order, Solved, solve_lumped};
