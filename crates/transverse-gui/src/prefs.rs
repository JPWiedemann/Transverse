//! Global display preferences that apply to every tab: unit system and colour theme.

use std::collections::BTreeMap;

use transverse_core::units::UnitSystem;

/// Colour theme choice. `System` follows the desktop, which the toolkit can only do when the window
/// system reports it; `Light` and `Dark` always work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeChoice {
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [Self::System, Self::Light, Self::Dark];

    pub fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == k)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub units: UnitSystem,
    pub theme: ThemeChoice,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { units: UnitSystem::Metric, theme: ThemeChoice::System }
    }
}

impl Prefs {
    /// From the settings `ui` map; missing or invalid entries keep the defaults.
    pub fn load(ui: &BTreeMap<String, serde_json::Value>) -> Self {
        let mut p = Self::default();
        if let Some(u) = ui.get("length_system").and_then(|v| v.as_str()).and_then(UnitSystem::from_key) {
            p.units = u;
        }
        if let Some(t) = ui.get("theme").and_then(|v| v.as_str()).and_then(ThemeChoice::from_key) {
            p.theme = t;
        }
        p
    }

    pub fn save(&self, ui: &mut BTreeMap<String, serde_json::Value>) {
        ui.insert("length_system".into(), self.units.key().into());
        ui.insert("theme".into(), self.theme.key().into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_system_theme_and_use_metric_units() {
        let p = Prefs::default();
        assert_eq!((p.units, p.theme), (UnitSystem::Metric, ThemeChoice::System));
        assert_eq!(Prefs::load(&BTreeMap::new()), p);
    }

    #[test]
    fn round_trip_and_garbage() {
        let p = Prefs { units: UnitSystem::Imperial, theme: ThemeChoice::Light };
        let mut ui = BTreeMap::new();
        p.save(&mut ui);
        assert_eq!(ui["theme"], "light");
        assert_eq!(Prefs::load(&ui), p);
        // an unknown value keeps the default of that one entry only
        ui.insert("theme".into(), "solarized".into());
        assert_eq!(Prefs::load(&ui), Prefs { theme: ThemeChoice::System, ..p });
        ui.insert("length_system".into(), 3.into());
        assert_eq!(Prefs::load(&ui), Prefs::default());
        for t in ThemeChoice::ALL {
            assert_eq!(ThemeChoice::from_key(t.key()), Some(t));
        }
    }
}
