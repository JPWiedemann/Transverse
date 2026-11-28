//! Line types. Each module turns user parameters into a geometry for the solvers.

pub mod mesh_field;
pub mod model;
pub mod planar;
pub mod render;
pub mod section_solve;

use std::sync::Arc;

use model::LineModel;

/// All available line types, in the order a frontend lists them.
pub fn registry() -> Vec<Arc<dyn LineModel>> {
    vec![Arc::new(planar::PlanarLine)]
}

/// Line type by id.
pub fn find_model(id: &str) -> Option<Arc<dyn LineModel>> {
    registry().into_iter().find(|m| m.id() == id)
}
