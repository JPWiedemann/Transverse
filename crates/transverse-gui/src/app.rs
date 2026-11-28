//! Main window: parameter form on the left, result tabs on the right.

use std::path::PathBuf;
use std::sync::Arc;

use crate::views::cross_section;
use crate::views::field::FieldView;
use crate::views::loss::{LossData, LossView};
use crate::views::return_current::ReturnView;
use eframe::egui::{self, Color32, RichText};
use transverse_core::units::{UnitSystem, parse_number};
use transverse_gui::form::{FieldState, FormState, unit_options};
use transverse_gui::prefs::{Prefs, ThemeChoice};
use transverse_gui::settings::{self, LineSettings, Settings};
use transverse_gui::worker::{Job, Msg, Worker};
use transverse_lines::model::{Analysis, LineModel, ParamKind, ParamSet, ParamSpec, is_visible};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    CrossSection,
    Results,
    Field,
    ReturnCurrent,
    Loss,
}

fn egui_theme(t: ThemeChoice) -> egui::ThemePreference {
    match t {
        ThemeChoice::System => egui::ThemePreference::System,
        ThemeChoice::Light => egui::ThemePreference::Light,
        ThemeChoice::Dark => egui::ThemePreference::Dark,
    }
}

/// Width of all entry boxes [points]: room for about 12 characters.
const ENTRY_WIDTH: f32 = 110.0;

const TABS: [(Tab, &str); 5] = [
    (Tab::CrossSection, "Cross-section"),
    (Tab::Results, "Results"),
    (Tab::Field, "Field"),
    (Tab::ReturnCurrent, "Return current"),
    (Tab::Loss, "Loss vs f"),
];

pub struct App {
    model: Arc<dyn LineModel>,
    form: FormState,
    synth_target: usize,
    /// The mode shown in the return-current tab (of a pair).
    split_mode: usize,
    z_target: String,
    /// The second target of a search in two dimensions (Zcomm of a pair).
    z_second: String,
    /// Electrical length to reach in degrees; empty keeps the line length.
    phase_target: String,
    settings: Settings,
    settings_path: PathBuf,
    worker: Option<Worker>,
    status: String,
    result: Option<Analysis>,
    field_view: FieldView,
    loss_view: LossView,
    /// Unit system and theme, applied to all tabs.
    prefs: Prefs,
    return_view: ReturnView,
    /// Counts results; plots reset their bounds when a new one arrives.
    generation: u64,
    tab: Tab,
    error: Option<String>,
}

/// Fixed-size entry box (red text while the content is not a number).
/// Like [`entry`], but an empty text is fine.
fn entry_optional(ui: &mut egui::Ui, text: &mut String) {
    let bad = !text.trim().is_empty() && parse_number(text).is_none();
    let edit = egui::TextEdit::singleline(text).text_color_opt(bad.then_some(Color32::RED));
    ui.add_sized([ENTRY_WIDTH, ui.spacing().interact_size.y], edit);
}

fn entry(ui: &mut egui::Ui, text: &mut String, enabled: bool) {
    let bad = parse_number(text).is_none();
    let edit = egui::TextEdit::singleline(text).text_color_opt(bad.then_some(Color32::RED));
    let size = [ENTRY_WIDTH, ui.spacing().interact_size.y];
    ui.add_enabled_ui(enabled, |ui| ui.add_sized(size, edit));
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings_path = settings::default_path();
        let (settings, note) = settings::load(&settings_path);
        Self::with_settings(&cc.egui_ctx, settings, settings_path, note)
    }

    fn with_settings(
        ctx: &egui::Context,
        settings: Settings,
        settings_path: PathBuf,
        note: Option<String>,
    ) -> Self {
        let model = transverse_lines::find_model(&settings.line_type)
            .unwrap_or_else(|| transverse_lines::registry().remove(0));
        let mut form = FormState::new(&*model);
        let prefs = Prefs::load(&settings.ui);
        ctx.set_theme(egui_theme(prefs.theme));
        let mut field_view = FieldView::default();
        field_view.load(&settings.ui);
        let mut loss_view = LossView::default();
        loss_view.load(&settings.ui);
        let mut synth_target = 0;
        let mut z_target = "35".to_string();
        let mut z_second = "30".to_string();
        let mut phase_target = String::new();
        if let Some(ls) = settings.lines.get(model.id()) {
            form.load_saved(&*model, &ls.fields);
            if let Some(id) = ls.extra.get("synth_target").and_then(|v| v.as_str()) {
                synth_target = model.synth_targets().iter().position(|t| t.id == id).unwrap_or(0);
            }
            if let Some(z) = ls.extra.get("z_target").and_then(|v| v.as_str()) {
                z_target = z.to_string();
            }
            if let Some(z) = ls.extra.get("z_second").and_then(|v| v.as_str()) {
                z_second = z.to_string();
            }
            if let Some(p) = ls.extra.get("phase_target").and_then(|v| v.as_str()) {
                phase_target = p.to_string();
            }
        }
        Self {
            model,
            form,
            synth_target,
            split_mode: 0,
            z_target,
            z_second,
            phase_target,
            settings,
            settings_path,
            worker: None,
            status: note.unwrap_or_else(|| "Ready".into()),
            result: None,
            field_view,
            loss_view,
            prefs,
            return_view: ReturnView::default(),
            generation: 0,
            tab: Tab::CrossSection,
            error: None,
        }
    }

    /// The frequency currently typed in the parameter form [Hz], if it parses.
    fn form_frequency(&self) -> Option<f64> {
        let spec = self.model.params().iter().find(|p| matches!(p.kind, ParamKind::Frequency { .. }))?;
        self.form.to_params(&*self.model).ok()?.num(spec.key).ok()
    }

    /// Global preferences: unit system (all tabs) and colour theme.
    fn toolbar_ui(&mut self, ui: &mut egui::Ui) {
        let before = self.prefs;
        ui.label("Units");
        egui::ComboBox::from_id_salt("unit_system")
            .selected_text(self.prefs.units.label())
            .width(80.0)
            .show_ui(ui, |ui| {
                for u in [UnitSystem::Metric, UnitSystem::Imperial] {
                    ui.selectable_value(&mut self.prefs.units, u, u.label());
                }
            })
            .response
            .on_hover_text("Lengths on the plot axes and readouts, losses and field strengths");
        ui.add_space(12.0);
        let detected = match ui.ctx().system_theme() {
            Some(egui::Theme::Dark) => "dark",
            Some(egui::Theme::Light) => "light",
            None => "not reported by the window system",
        };
        ui.label("Theme");
        egui::ComboBox::from_id_salt("theme")
            .selected_text(self.prefs.theme.label())
            .width(80.0)
            .show_ui(ui, |ui| {
                for t in ThemeChoice::ALL {
                    ui.selectable_value(&mut self.prefs.theme, t, t.label());
                }
            })
            .response
            .on_hover_text(format!("System follows the desktop theme. Detected: {detected}."));
        if self.prefs != before {
            ui.ctx().set_theme(egui_theme(self.prefs.theme));
            self.save_settings();
        }
    }

    fn busy(&self) -> bool {
        self.worker.is_some()
    }

    fn save_settings(&mut self) {
        let mut ls = LineSettings { fields: self.form.to_saved(), ..Default::default() };
        let target = self.model.synth_targets()[self.synth_target].id;
        ls.extra.insert("synth_target".into(), target.into());
        ls.extra.insert("z_target".into(), self.z_target.clone().into());
        ls.extra.insert("z_second".into(), self.z_second.clone().into());
        ls.extra.insert("phase_target".into(), self.phase_target.clone().into());
        self.field_view.save(&mut self.settings.ui);
        self.prefs.save(&mut self.settings.ui);
        self.loss_view.save(&mut self.settings.ui);
        self.settings.line_type = self.model.id().to_string();
        self.settings.lines.insert(self.model.id().to_string(), ls);
        if let Err(e) = settings::save(&self.settings_path, &self.settings) {
            self.status = format!("Could not save settings: {e}");
        }
    }

    fn start(&mut self, ctx: &egui::Context, synth: bool) {
        let params = match self.form.to_params(&*self.model) {
            Ok(p) => p,
            Err(e) => {
                self.error = Some(e.to_string());
                return;
            }
        };
        let job = if synth {
            let Some(z0) = parse_number(&self.z_target).filter(|z| *z > 0.0) else {
                self.error = Some("Target impedance: enter a positive number".into());
                return;
            };
            // the phase is optional: the line length follows from it (and eps_eff) after the Z0 synthesis
            let phase_deg = if self.phase_target.trim().is_empty() {
                None
            } else {
                let Some(ph) = parse_number(&self.phase_target).filter(|v| *v > 0.0) else {
                    self.error =
                        Some("Target phase: enter a positive number of degrees or leave it empty".into());
                    return;
                };
                Some(ph)
            };
            let target = self.shown_target(&self.form.control_set(&*self.model));
            let target = &self.model.synth_targets()[target];
            let second = match target.second {
                Some(name) => {
                    let Some(z) = parse_number(&self.z_second).filter(|z| *z > 0.0) else {
                        self.error = Some(format!("Target {name}: enter a positive number"));
                        return;
                    };
                    Some(z)
                }
                None => None,
            };
            let target_id = target.id.to_string();
            Job::Synthesize { params, target_id, z0, second, phase_deg }
        } else {
            Job::Analyze { params }
        };
        self.save_settings();
        let ctx = ctx.clone();
        self.worker = Some(Worker::start(self.model.clone(), job, move || ctx.request_repaint()));
        self.status = "Starting ...".into();
    }

    fn poll_worker(&mut self) {
        let Some(w) = &mut self.worker else { return };
        let msgs = w.poll();
        let el = w.elapsed().as_secs_f64();
        let mut finished = false;
        for m in msgs {
            match m {
                Msg::Progress(p) => self.status = format!("[{el:5.1} s]  {p}"),
                Msg::Cancelled => {
                    self.status = format!("Cancelled after {el:.1} s");
                    finished = true;
                }
                Msg::Error(e) => {
                    self.status = "Error".into();
                    self.error = Some(e);
                    finished = true;
                }
                Msg::Done(out) => {
                    if let Some(p) = &out.new_params {
                        self.form.set_params(&*self.model, p);
                    }
                    let nodes = out.analysis.accuracy.as_ref().map_or(0, |a| a.nodes);
                    let headline = match &out.analysis.pair {
                        Some(pair) => format!("Zdiff = {:.3} Ohm", pair.z_diff),
                        None => format!("Z0 = {:.3} Ohm", out.analysis.modes[0].z0),
                    };
                    self.status = format!("Done in {el:.1} s: {headline}, {nodes} nodes");
                    self.result = Some(out.analysis);
                    self.field_view.new_result();
                    self.generation += 1;
                    if self.tab == Tab::CrossSection {
                        self.tab = Tab::Results;
                    }
                    finished = true;
                }
            }
        }
        if finished {
            self.worker = None;
            self.save_settings();
        }
    }

    // ------------------------------------------------------------------ form

    fn form_ui(&mut self, ui: &mut egui::Ui) {
        let model = self.model.clone();
        let controls = self.form.control_set(&*model);
        let mut preset = None;
        let mut unit_change: Option<(&'static str, String)> = None;
        let busy = self.busy();

        // only what the structure uses; a group without such a field is left out
        let mut groups: Vec<(&str, Vec<&ParamSpec>)> = Vec::new();
        for spec in model.params().iter().filter(|s| is_visible(s.visible_when, &controls)) {
            match groups.last_mut() {
                Some((g, v)) if *g == spec.group => v.push(spec),
                _ => groups.push((spec.group, vec![spec])),
            }
        }

        ui.add_enabled_ui(!busy, |ui| {
            for (index, (group, specs)) in groups.into_iter().enumerate() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(group).strong());
                    // the presets head the first group, which holds the switches they set
                    if index == 0 && !model.presets().is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            for p in model.presets() {
                                if ui.button(p.label).clicked() {
                                    preset = Some(p);
                                }
                            }
                        });
                    }
                    egui::Grid::new(group).num_columns(3).spacing([6.0, 3.0]).show(ui, |ui| {
                        for spec in specs {
                            self.param_row(ui, spec, &mut unit_change);
                            ui.end_row();
                        }
                    });
                });
            }
        });
        if let Some(p) = preset {
            self.form.apply_preset(p);
        }
        if let Some((key, unit)) = unit_change {
            self.form.change_unit(&*model, key, &unit);
        }
    }

    fn param_row(
        &mut self,
        ui: &mut egui::Ui,
        spec: &'static ParamSpec,
        unit_change: &mut Option<(&'static str, String)>,
    ) {
        let options = unit_options(spec);
        let Some(state) = self.form.fields.get_mut(spec.key) else {
            return;
        };
        match state {
            FieldState::Quantity { text, unit } => {
                ui.label(spec.label);
                entry(ui, text, true);
                if options.is_empty() {
                    ui.label(&*unit);
                } else {
                    ui.scope(|ui| {
                        egui::ComboBox::from_id_salt((spec.key, "unit"))
                            .selected_text(&*unit)
                            .width(48.0)
                            .show_ui(ui, |ui| {
                                for o in &options {
                                    if ui.selectable_label(unit == o, *o).clicked() && unit != o {
                                        *unit_change = Some((spec.key, o.to_string()));
                                    }
                                }
                            });
                    });
                }
            }
            FieldState::Flag(b) => {
                ui.checkbox(b, spec.label);
                ui.label("");
                ui.label("");
            }
            FieldState::Choice(i) => {
                let ParamKind::Choice(opts, _) = spec.kind else { return };
                ui.label(spec.label);
                ui.scope(|ui| {
                    egui::ComboBox::from_id_salt((spec.key, "choice"))
                        .selected_text(opts[(*i).min(opts.len() - 1)])
                        .show_ui(ui, |ui| {
                            for (k, o) in opts.iter().enumerate() {
                                ui.selectable_value(i, k, *o);
                            }
                        });
                });
                ui.label("");
            }
        }
    }

    /// The chosen synthesis target, or the first one the structure offers if it does not offer the chosen one.
    fn shown_target(&self, controls: &ParamSet) -> usize {
        let targets = self.model.synth_targets();
        match targets.get(self.synth_target) {
            Some(t) if is_visible(t.visible_when, controls) => self.synth_target,
            _ => targets.iter().position(|t| is_visible(t.visible_when, controls)).unwrap_or(0),
        }
    }

    fn synthesis_ui(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        let targets = self.model.synth_targets();
        let controls = self.form.control_set(&*self.model);
        self.synth_target = self.shown_target(&controls);
        ui.add_enabled_ui(!busy, |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Synthesis").strong());
                egui::Grid::new("synthesis").num_columns(3).spacing([6.0, 3.0]).show(ui, |ui| {
                    // first, because it decides which targets there are
                    ui.label("Vary");
                    egui::ComboBox::from_id_salt("synth_target")
                        .selected_text(targets[self.synth_target].label)
                        .show_ui(ui, |ui| {
                            for (k, t) in targets
                                .iter()
                                .enumerate()
                                .filter(|(_, t)| is_visible(t.visible_when, &controls))
                            {
                                ui.selectable_value(&mut self.synth_target, k, t.label);
                            }
                        });
                    ui.end_row();
                    // the combo box may just have changed it: draw the targets it has now
                    ui.label(format!("Target {}", targets[self.synth_target].quantity));
                    entry(ui, &mut self.z_target, true);
                    ui.label("Ohm");
                    ui.end_row();
                    if let Some(name) = targets[self.synth_target].second {
                        ui.label(format!("Target {name}"));
                        entry(ui, &mut self.z_second, true);
                        ui.label("Ohm");
                        ui.end_row();
                    }
                    // optional: the line length is set from the phase, it does not change the search
                    ui.label("Phase");
                    entry_optional(ui, &mut self.phase_target);
                    ui.label("deg (empty: keep length)");
                    ui.end_row();
                });
            });
        });
    }

    fn buttons_ui(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Analyze")).clicked() {
                self.start(ui.ctx(), false);
            }
            if ui.add_enabled(!busy, egui::Button::new("Synthesize")).clicked() {
                self.start(ui.ctx(), true);
            }
            if ui.add_enabled(busy, egui::Button::new("Cancel")).clicked() {
                if let Some(w) = &self.worker {
                    w.cancel();
                }
                self.status = "Cancelling (after the current linear solve) ...".into();
            }
        });
    }

    // ------------------------------------------------------------------ tabs

    fn tab_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (t, name) in TABS {
                ui.selectable_value(&mut self.tab, t, name);
            }
        });
        ui.separator();
        match self.tab {
            Tab::Results => match &self.result {
                Some(a) => {
                    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(&a.report).monospace()).selectable(true));
                    });
                }
                None => {
                    ui.label("No result yet. Press Analyze or Synthesize.");
                }
            },
            Tab::Field => {
                match self.result.as_ref().and_then(|a| a.field.as_ref().map(|f| (f, a.accuracy.as_ref()))) {
                    Some((f, acc)) => self.field_view.ui(ui, f, acc, self.prefs.units),
                    None => {
                        ui.label("No result yet. Press Analyze or Synthesize.");
                    }
                }
            }
            Tab::CrossSection => match self.model.schematic(&self.form.control_set(&*self.model)) {
                Some(sch) => cross_section::show(ui, &sch),
                None => {
                    ui.label("This line type has no sketch.");
                }
            },
            Tab::ReturnCurrent => {
                // a pair has one split per mode
                if let Some(a) = self.result.as_ref().filter(|a| a.modes.len() > 1) {
                    ui.horizontal(|ui| {
                        ui.label("Shown mode");
                        for (k, m) in a.modes.iter().enumerate() {
                            ui.selectable_value(&mut self.split_mode, k, mode_label(&m.name));
                        }
                    });
                }
                let split = self
                    .result
                    .as_ref()
                    .and_then(|a| a.modes.get(self.split_mode).or_else(|| a.modes.first())?.split.as_ref());
                match split {
                    // each mode keeps its own zoom only until the other one is shown
                    Some(split) => {
                        let shown = self.generation * 2 + self.split_mode as u64;
                        self.return_view.ui(ui, split, shown, self.prefs.units)
                    }
                    None => {
                        ui.label("No result yet. Press Analyze or Synthesize.");
                    }
                }
            }
            Tab::Loss => match &self.result {
                Some(a) => {
                    let live_f = self.form_frequency();
                    if a.pair.is_some() {
                        ui.label("Odd (differential) mode, values per trace:");
                    }
                    let data = LossData {
                        freq: &*a.freq,
                        z0: a.modes[0].z0,
                        f_hz: a.modes[0].f_hz,
                        form_f_hz: live_f,
                        units: self.prefs.units,
                        generation: self.generation,
                    };
                    self.loss_view.ui(ui, &data)
                }
                None => {
                    ui.label("No result yet. Press Analyze or Synthesize.");
                }
            },
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_worker();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.busy() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| self.toolbar_ui(ui));
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if self.busy() {
                    ui.spinner();
                }
                ui.label(&self.status);
            });
        });

        egui::Panel::left("form").resizable(true).default_size(420.0).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.heading(self.model.name());
                ui.add_space(4.0);
                self.form_ui(ui);
                self.synthesis_ui(ui);
                ui.add_space(4.0);
                self.buttons_ui(ui);
            });
        });

        egui::CentralPanel::default().show(ui, |ui| self.tab_ui(ui));

        if let Some(msg) = self.error.clone() {
            egui::Window::new("Error")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    ui.label(msg);
                    if ui.button("OK").clicked() {
                        self.error = None;
                    }
                });
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(w) = &self.worker {
            w.cancel();
        }
        self.save_settings();
    }
}

/// The mode of a pair, for the selector of the return-current tab.
fn mode_label(name: &str) -> &'static str {
    if name == "even" { "Even (common)" } else { "Odd (differential)" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Pos2, Rect, Shape, vec2};
    use transverse_lines::model::ParamValue;

    fn texts(shape: &Shape, out: &mut Vec<String>) {
        match shape {
            Shape::Text(t) => out.push(t.galley.text().to_string()),
            Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
            _ => {}
        }
    }

    /// The texts the synthesis panel draws.
    fn synthesis_texts(app: &mut App, ctx: &egui::Context) -> Vec<String> {
        let mut out = Vec::new();
        // a new row of a grid is laid out invisibly in its first frame
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(420.0, 700.0))),
                ..Default::default()
            };
            let frame = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.synthesis_ui(ui));
            });
            out.clear();
            frame.shapes.iter().for_each(|s| texts(&s.shape, &mut out));
        }
        out
    }

    #[test]
    fn the_second_target_is_shown_for_width_and_gap_of_a_pair() {
        let ctx = egui::Context::default();
        let mut app = App::with_settings(&ctx, Settings::default(), PathBuf::new(), None);
        let mut set = app.model.defaults();
        set.set("signals", ParamValue::Num(1.0));
        let model = app.model.clone();
        app.form.set_params(&*model, &set);
        let before = synthesis_texts(&mut app, &ctx);
        assert!(before.iter().any(|t| t == "Target Zdiff"), "{before:?}");
        assert!(!before.iter().any(|t| t == "Target Zcomm"), "{before:?}");
        app.synth_target = model.synth_targets().iter().position(|t| t.id == "pair_wd").unwrap();
        let after = synthesis_texts(&mut app, &ctx);
        for want in ["Target Zdiff", "Target Zcomm", "30", "w and d (Zdiff + Zcomm)"] {
            assert!(after.iter().any(|t| t == want), "{want}: {after:?}");
        }
        // the choice comes before the targets it decides about
        let row = |text: &str| after.iter().position(|t| t == text).unwrap();
        assert!(row("Vary") < row("Target Zdiff") && row("Target Zdiff") < row("Target Zcomm"), "{after:?}");
    }
}
