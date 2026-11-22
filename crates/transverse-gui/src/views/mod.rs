//! Result tabs.

pub mod cross_section;
pub mod field;
pub mod loss;
mod nav;
pub mod return_current;

#[cfg(test)]
mod tests {
    use eframe::egui::{self, Pos2, Rect, vec2};
    use transverse_core::Ctl;
    use transverse_lines::cpwg_inner::CpwgInner;
    use transverse_lines::model::{Analysis, LineModel, ParamValue};

    use super::{cross_section, loss::LossView, return_current};

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
        let m = CpwgInner;
        let closed = m.analyze(&m.defaults(), &Ctl::new()).unwrap();
        let mut set = m.defaults();
        set.set("plane1", ParamValue::Bool(false));
        set.set("fence1", ParamValue::Bool(false));
        vec![closed, m.analyze(&set, &Ctl::new()).unwrap()]
    }

    #[test]
    fn return_current_tab_renders_for_closed_and_open_structures() {
        let ctx = egui::Context::default();
        for (gen_, a) in analyses().iter().enumerate() {
            let split = a.modes[0].split.as_ref().unwrap();
            for _ in 0..2 {
                frame(&ctx, |ui| return_current::show(ui, split, gen_ as u64));
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
            frame(&ctx, |ui| view.ui(ui, &*a.freq, m.z0, m.f_hz, 0));
        }
        let mut ui_state = std::collections::BTreeMap::new();
        ui_state.insert("fmax_text".into(), "500".into());
        ui_state.insert("fmax_unit".into(), "MHz".into());
        view.load(&ui_state); // analysis frequency (1 GHz) is now beyond the plot
        for _ in 0..2 {
            frame(&ctx, |ui| view.ui(ui, &*a.freq, m.z0, m.f_hz, 0));
        }
    }

    #[test]
    fn cross_section_tab_renders_every_combination_of_switches() {
        let ctx = egui::Context::default();
        let m = CpwgInner;
        for mask in 0..32u32 {
            let mut set = m.defaults();
            for (i, k) in ["plane1", "plane2", "fence1", "fence2"].into_iter().enumerate() {
                set.set(k, ParamValue::Bool(mask & (1 << i) != 0));
            }
            set.set("gap_mode", ParamValue::Num((mask % 3) as f64));
            let sch = m.schematic(&set).unwrap();
            frame(&ctx, |ui| cross_section::show(ui, &sch));
        }
    }
}
