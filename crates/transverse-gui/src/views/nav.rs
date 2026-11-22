//! Mouse navigation shared by the plots: wheel zooms, left drag draws a zoom box, right drag pans.

use eframe::egui::{PointerButton, Vec2};
use egui_plot::{Plot, PlotUi};

/// Zoom per scroll point (exponential).
pub const SCROLL_ZOOM: f64 = 0.0015;

pub fn configure(plot: Plot<'_>) -> Plot<'_> {
    plot.allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(true)
        .boxed_zoom_pointer_button(PointerButton::Primary)
        .pan_pointer_button(PointerButton::Secondary)
}

/// Call inside the plot closure: zooms around the cursor with the mouse wheel.
pub fn wheel_zoom(pui: &mut PlotUi<'_>) {
    let scroll = pui.ctx().input(|i| i.smooth_scroll_delta.y);
    if scroll != 0.0 && pui.response().hovered() {
        pui.zoom_bounds_around_hovered(Vec2::splat((scroll as f64 * SCROLL_ZOOM).exp() as f32));
    }
}

/// x value [data units] under the cursor if it is inside the plot frame.
pub fn hover_x(resp: &egui_plot::PlotResponse<()>) -> Option<f64> {
    let pos = resp.response.hover_pos().filter(|p| resp.transform.frame().contains(*p))?;
    Some(resp.transform.value_from_position(pos).x)
}
