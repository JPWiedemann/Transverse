//! Line types. Each module turns user parameters into a geometry for the solvers.

pub mod cpwg_inner;
pub mod model;
pub mod render;

use std::sync::Arc;

use model::LineModel;

/// All available line types, in the order a frontend lists them.
pub fn registry() -> Vec<Arc<dyn LineModel>> {
    vec![Arc::new(cpwg_inner::CpwgInner)]
}

/// Line type by id.
pub fn find_model(id: &str) -> Option<Arc<dyn LineModel>> {
    registry().into_iter().find(|m| m.id() == id)
}
