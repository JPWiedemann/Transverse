//! Finite elements (P1 and P2) for the quasi-static Laplace problem on a triangle mesh with N signal conductors.
//!
//! The functions take plain slices (nodes, triangles, per-triangle permittivity, fixed potentials), so
//! this module does not depend on the mesher. Lengths are in mm; capacitances come out per metre,
//! exactly as in the finite-difference solver.

mod adapt;
mod converge;
mod element;
mod field;
mod flux;
mod solve;

pub use adapt::{converge_adaptive, error_indicators, mark, residual_indicators};
pub use converge::{Level, LevelRun, auto_margin_with, converge};
pub use field::{corner_gradients, element_dofs, p2_values};
pub use flux::{BoundaryFlux, boundary_fluxes};
pub use solve::{FemInput, Lumped, Order, Solved, solve_signals};
