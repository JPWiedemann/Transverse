//! Result tabs.

pub mod cross_section;
pub mod field;
mod legend;
pub mod loss;
mod nav;
pub mod return_current;

#[cfg(test)]
mod tests {
    use eframe::egui::{self, Pos2, Rect, vec2};
    use transverse_core::Ctl;
    use transverse_core::units::UnitSystem;
    use transverse_lines::model::{Analysis, LineModel, ParamValue};
    use transverse_lines::planar::PlanarLine;

    use super::{
        cross_section,
        loss::{LossData, LossView},
        return_current::ReturnView,
    };

    fn frame(ctx: &egui::Context, mut f: impl FnMut(&mut egui::Ui)) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| f(ui));
        });
    }

    fn analyses() -> Vec<Analysis> {
        let m = PlanarLine;
        let closed = m.analyze(&m.defaults(), &Ctl::new()).unwrap();
        let mut set = m.defaults();
        set.set("plane1", ParamValue::Bool(false));
        set.set("fence1", ParamValue::Bool(false));
        let mut pair = m.defaults();
        pair.set("signals", ParamValue::Num(1.0));
        pair.set("w", ParamValue::Num(0.2));
        vec![closed, m.analyze(&set, &Ctl::new()).unwrap(), m.analyze(&pair, &Ctl::new()).unwrap()]
    }

    #[test]
    fn return_current_tab_renders_for_closed_and_open_structures() {
        let ctx = egui::Context::default();
        for (gen_, a) in analyses().iter().enumerate() {
            let split = a.modes[0].split.as_ref().unwrap();
            let mut view = ReturnView::default();
            for _ in 0..2 {
                frame(&ctx, |ui| view.ui(ui, split, gen_ as u64, UnitSystem::Metric));
            }
        }
    }

    #[test]
    fn loss_tab_renders_and_accepts_a_changed_frequency_range() {
        let ctx = egui::Context::default();
        let a = &analyses()[0];
        let mut view = LossView::default();
        let m = &a.modes[0];
        for _ in 0..2 {
            frame(&ctx, |ui| {
                view.ui(
                    ui,
                    &LossData {
                        freq: &*a.freq,
                        z0: m.z0,
                        f_hz: m.f_hz,
                        form_f_hz: None,
                        units: UnitSystem::Metric,
                        generation: 0,
                    },
                )
            });
        }
        let mut ui_state = std::collections::BTreeMap::new();
        ui_state.insert("fmax_text".into(), "500".into());
        ui_state.insert("fmax_unit".into(), "MHz".into());
        view.load(&ui_state); // analysis frequency (1 GHz) is now beyond the entry: the plot is extended
        for _ in 0..2 {
            frame(&ctx, |ui| {
                view.ui(
                    ui,
                    &LossData {
                        freq: &*a.freq,
                        z0: m.z0,
                        f_hz: m.f_hz,
                        form_f_hz: None,
                        units: UnitSystem::Metric,
                        generation: 0,
                    },
                )
            });
        }
        // a frequency typed in the form (but not analysed yet) extends the plot too
        for _ in 0..2 {
            frame(&ctx, |ui| {
                view.ui(
                    ui,
                    &LossData {
                        freq: &*a.freq,
                        z0: m.z0,
                        f_hz: m.f_hz,
                        form_f_hz: Some(30e9),
                        units: UnitSystem::Imperial,
                        generation: 0,
                    },
                )
            });
        }
    }

    #[test]
    fn cross_section_tab_renders_every_combination_of_switches() {
        let ctx = egui::Context::default();
        let m = PlanarLine;
        for mask in 0..32u32 {
            let mut set = m.defaults();
            for (i, k) in ["plane1", "plane2", "fence1", "fence2"].into_iter().enumerate() {
                set.set(k, ParamValue::Bool(mask & (1 << i) != 0));
            }
            set.set("gap_mode", ParamValue::Num((mask % 3) as f64));
            let sch = m.schematic(&set).unwrap();
            frame(&ctx, |ui| cross_section::show(ui, &sch));
            // and in the dark theme
            ctx.set_visuals(egui::Visuals::dark());
            frame(&ctx, |ui| cross_section::show(ui, &sch));
            ctx.set_visuals(egui::Visuals::light());
        }
    }
}
