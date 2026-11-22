//! Finite-volume (5-point) solver on a non-uniform tensor grid.

pub mod axis;
pub mod grid;
pub mod solve;

pub use grid::{CAT_FREE, CAT_SIGNAL, Grid, TOL_GEO};
pub use solve::{LevelFields, solve_grid};
