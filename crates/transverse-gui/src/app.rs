//! Main window: parameter form on the left, result tabs on the right.

use std::path::PathBuf;
use std::sync::Arc;

use crate::views::cross_section;
use crate::views::field::FieldView;
use crate::views::loss::{LossData, LossView};
use crate::views::return_current::ReturnView;
use eframe::egui::{self, Color32, RichText};
use transverse_core::units::{self, UnitSystem, parse_number};
use transverse_gui::form::{FieldState, FormState, unit_options};
use transverse_gui::prefs::{Prefs, ThemeChoice};
use transverse_gui::settings::{self, LineSettings, Settings};
use transverse_gui::worker::{Job, Msg, Worker};
use transverse_lines::model::{Analysis, LineModel, ParamKind, ParamSpec, is_enabled};

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
    /// Value to reach, as typed, for every synthesis target (Z0 in Ohm, phase in degrees).
    synth_values: Vec<String>,
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
        let model = transverse_lines::find_model(&settings.line_type)
            .unwrap_or_else(|| transverse_lines::registry().remove(0));
        let mut form = FormState::new(&*model);
        let prefs = Prefs::load(&settings.ui);
        cc.egui_ctx.set_theme(egui_theme(prefs.theme));
        let mut field_view = FieldView::default();
        field_view.load(&settings.ui);
        let mut loss_view = LossView::default();
        loss_view.load(&settings.ui);
        let mut synth_target = 0;
        let mut synth_values: Vec<String> =
            model.synth_targets().iter().map(|t| units::format_g(t.default)).collect();
        if let Some(ls) = settings.lines.get(model.id()) {
            form.load_saved(&*model, &ls.fields);
            if let Some(id) = ls.extra.get("synth_target").and_then(|v| v.as_str()) {
                synth_target = model.synth_targets().iter().position(|t| t.id == id).unwrap_or(0);
            }
            for (t, v) in model.synth_targets().iter().zip(&mut synth_values) {
                if let Some(s) = ls.extra.get(&format!("synth_value_{}", t.id)).and_then(|s| s.as_str()) {
                    *v = s.to_string();
                }
            }
            // settings of older versions: one Z0 for all targets
            if let Some(z) = ls.extra.get("z_target").and_then(|v| v.as_str()) {
                for (t, v) in model.synth_targets().iter().zip(&mut synth_values) {
                    if t.unit == "Ohm" && !ls.extra.contains_key(&format!("synth_value_{}", t.id)) {
                        *v = z.to_string();
                    }
                }
            }
        }
        Self {
            model,
            form,
            synth_target,
            synth_values,
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
        for (t, v) in self.model.synth_targets().iter().zip(&self.synth_values) {
            ls.extra.insert(format!("synth_value_{}", t.id), v.clone().into());
        }
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
            let t = &self.model.synth_targets()[self.synth_target];
            let Some(value) = parse_number(&self.synth_values[self.synth_target]).filter(|v| *v > 0.0) else {
                self.error = Some(format!("Target {}: enter a positive number", t.goal));
                return;
            };
            Job::Synthesize { params, target_id: t.id.to_string(), value }
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
                    self.status =
                        format!("Done in {el:.1} s: Z0 = {:.3} Ohm, {nodes} nodes", out.analysis.modes[0].z0);
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
        let enabled_set = self.form.control_set(&*model);
        let mut unit_change: Option<(&'static str, String)> = None;
        let busy = self.busy();

        let mut groups: Vec<(&str, Vec<&ParamSpec>)> = Vec::new();
        for spec in model.params() {
            match groups.last_mut() {
                Some((g, v)) if *g == spec.group => v.push(spec),
                _ => groups.push((spec.group, vec![spec])),
            }
        }

        ui.add_enabled_ui(!busy, |ui| {
            for (group, specs) in groups {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(group).strong());
                    egui::Grid::new(group).num_columns(3).spacing([6.0, 3.0]).show(ui, |ui| {
                        for spec in specs {
                            let enabled = is_enabled(spec, &enabled_set);
                            self.param_row(ui, spec, enabled, &mut unit_change);
                            ui.end_row();
                        }
                    });
                });
            }
        });
        if let Some((key, unit)) = unit_change {
            self.form.change_unit(&*model, key, &unit);
        }
    }

    fn param_row(
        &mut self,
        ui: &mut egui::Ui,
        spec: &'static ParamSpec,
        enabled: bool,
        unit_change: &mut Option<(&'static str, String)>,
    ) {
        let options = unit_options(spec);
        let Some(state) = self.form.fields.get_mut(spec.key) else {
            return;
        };
        match state {
            FieldState::Quantity { text, unit } => {
                ui.label(spec.label);
                entry(ui, text, enabled);
                if options.is_empty() {
                    ui.label(&*unit);
                } else {
                    ui.add_enabled_ui(enabled, |ui| {
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
                ui.add_enabled(enabled, egui::Checkbox::new(b, spec.label));
                ui.label("");
                ui.label("");
            }
            FieldState::Choice(i) => {
                let ParamKind::Choice(opts, _) = spec.kind else { return };
                ui.label(spec.label);
                ui.add_enabled_ui(enabled, |ui| {
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

    fn synthesis_ui(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        let targets = self.model.synth_targets();
        ui.add_enabled_ui(!busy, |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Synthesis").strong());
                egui::Grid::new("synthesis").num_columns(3).spacing([6.0, 3.0]).show(ui, |ui| {
                    // the value to reach follows the target: Z0 for a width or gap, phase for the length
                    ui.label(format!("Target {}", targets[self.synth_target].goal));
                    entry(ui, &mut self.synth_values[self.synth_target], true);
                    ui.label(targets[self.synth_target].unit);
                    ui.end_row();
                    ui.label("Vary");
                    egui::ComboBox::from_id_salt("synth_target")
                        .selected_text(targets[self.synth_target].label)
                        .show_ui(ui, |ui| {
                            for (k, t) in targets.iter().enumerate() {
                                ui.selectable_value(&mut self.synth_target, k, t.label);
                            }
                        });
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
            Tab::ReturnCurrent => match self.result.as_ref().and_then(|a| a.modes.first()?.split.as_ref()) {
                Some(split) => self.return_view.ui(ui, split, self.generation, self.prefs.units),
                None => {
                    ui.label("No result yet. Press Analyze or Synthesize.");
                }
            },
            Tab::Loss => match &self.result {
                Some(a) => {
                    let live_f = self.form_frequency();
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
