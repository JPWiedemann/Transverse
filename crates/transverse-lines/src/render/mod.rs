//! Field images for the frontends: colour maps, the colour-mapped raster of |E|, potential,
//! permittivity and mesh, and the cursor probe. Pure functions on [`crate::model::FieldData`], so
//! the desktop and the web frontend draw the same pixels.

pub mod colormap;
mod colormap_data;
pub mod raster;
