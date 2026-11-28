//! Persistent user inputs: `settings.json` next to the executable.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::form::SavedField;

const VERSION: u32 = 1;

/// Saved inputs of one line type.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LineSettings {
    pub fields: BTreeMap<String, SavedField>,
    /// Further per-line-type values (e.g. synthesis target), free-form.
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    /// Line type selected last.
    pub line_type: String,
    /// Inputs per line-type id.
    pub lines: BTreeMap<String, LineSettings>,
    /// Frontend view options (field mode, zoom, ...), free-form.
    pub ui: BTreeMap<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { version: VERSION, line_type: String::new(), lines: BTreeMap::new(), ui: BTreeMap::new() }
    }
}

/// Ids that line types had before: settings saved under them are read under the current id.
const RENAMED_LINES: [(&str, &str); 1] = [("cpwg_inner", "planar")];

impl Settings {
    fn migrate(&mut self) {
        for (old, new) in RENAMED_LINES {
            if let Some(saved) = self.lines.remove(old) {
                self.lines.entry(new.to_string()).or_insert(saved);
            }
            if self.line_type == old {
                self.line_type = new.to_string();
            }
        }
    }
}

/// `settings.json` in the folder of the running executable.
pub fn default_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_default()
        .join("settings.json")
}

/// Load settings. A missing file gives the defaults silently; an unreadable one gives the defaults
/// and a note for the status line. Unknown fields are ignored, missing ones default.
pub fn load(path: &Path) -> (Settings, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Settings::default(), None),
        Err(e) => return (Settings::default(), Some(format!("could not read settings: {e}"))),
    };
    match serde_json::from_str::<Settings>(&text) {
        Ok(mut s) => {
            s.migrate();
            (s, Some(format!("Settings loaded from {}", path.display())))
        }
        Err(e) => (Settings::default(), Some(format!("ignored invalid settings file: {e}"))),
    }
}

/// Write settings (via a temporary file so a crash cannot leave a half-written one).
pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form::FormState;
    use transverse_lines::model::LineModel;
    use transverse_lines::planar::PlanarLine;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("transverse-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join("settings.json")
    }

    #[test]
    fn save_and_load_round_trip() {
        let path = temp("roundtrip");
        let m = PlanarLine;
        let mut form = FormState::new(&m);
        form.change_unit(&m, "w", "mil");
        let mut s = Settings { line_type: m.id().into(), ..Default::default() };
        s.lines.insert(m.id().into(), LineSettings { fields: form.to_saved(), ..Default::default() });
        s.lines.get_mut(m.id()).unwrap().extra.insert("z_target".into(), serde_json::json!("35"));
        s.ui.insert("zoom".into(), serde_json::json!(true));
        save(&path, &s).unwrap();
        assert!(!path.with_extension("json.tmp").exists());
        let (back, note) = load(&path);
        assert_eq!(back, s);
        assert!(note.unwrap().starts_with("Settings loaded"));
        let mut restored = FormState::new(&m);
        restored.load_saved(&m, &back.lines[m.id()].fields);
        assert_eq!(restored, form);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn missing_and_broken_files_give_defaults() {
        let path = temp("broken");
        let (s, note) = load(&path);
        assert_eq!(s, Settings::default());
        assert!(note.is_none());
        std::fs::write(&path, "{ not json").unwrap();
        let (s, note) = load(&path);
        assert_eq!(s, Settings::default());
        assert!(note.unwrap().contains("invalid"));
        // unknown and missing fields are tolerated
        std::fs::write(&path, r#"{"version": 1, "future_field": 3, "line_type": "planar"}"#).unwrap();
        let (s, _) = load(&path);
        assert_eq!(s.line_type, "planar");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn default_path_is_next_to_the_executable() {
        let p = default_path();
        assert_eq!(p.file_name().unwrap(), "settings.json");
        assert_eq!(p.parent(), std::env::current_exe().unwrap().parent());
    }

    #[test]
    fn settings_saved_under_the_old_line_id_are_found_under_the_new_one() {
        let dir = std::env::temp_dir().join(format!("transverse-migrate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{"version": 1, "line_type": "cpwg_inner",
            "lines": {"cpwg_inner": {"extra": {"z_target": "42"}}}}"#,
        )
        .unwrap();
        let (s, _) = load(&path);
        assert_eq!(s.line_type, "planar");
        assert_eq!(s.lines.keys().collect::<Vec<_>>(), ["planar"]);
        assert_eq!(s.lines["planar"].extra["z_target"], "42");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
