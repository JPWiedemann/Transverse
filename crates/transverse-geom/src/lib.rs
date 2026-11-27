//! Geometry of a transmission-line cross-section: what a line type hands to the mesher.
//!
//! A [`CrossSection`] is a rectangular domain with shapes painted in order (later wins): dielectric
//! regions with a [`Material`] and conductors with an id and a tag. Lengths are in mm. Nothing here
//! knows about meshes or solvers.

pub mod section;
pub mod shape;
pub mod size;

pub use section::{ConductorId, CrossSection, GROUND, Item, Material, MaterialId, Paint, SIGNAL, Tag};
pub use shape::{P2, Shape};
pub use size::{Far, SizeSpec};
