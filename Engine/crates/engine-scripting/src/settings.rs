use engine_platform::AtomicFileService;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const GAME_SETTINGS_FILE: &str = "settings.json";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GameSettings {
    #[serde(default)]
    pub global_scripts: Vec<crate::ScriptReference>,
    #[serde(default = "autosave_enabled_by_default")]
    pub autosave: bool,
}

const fn autosave_enabled_by_default() -> bool {
    true
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            global_scripts: Vec::new(),
            autosave: autosave_enabled_by_default(),
        }
    }
}

impl GameSettings {
    /// # Errors
    /// Returns an I/O, JSON, or validation error.
    pub fn load(project_root: &Path) -> Result<Self, String> {
        let path = project_root.join(GAME_SETTINGS_FILE);
        let settings: Self = serde_json::from_slice(
            &std::fs::read(&path)
                .map_err(|error| format!("could not read {}: {error}", path.display()))?,
        )
        .map_err(|error| format!("invalid {}: {error}", path.display()))?;
        settings.validate()?;
        Ok(settings)
    }

    /// # Errors
    /// Returns a validation, serialization, or atomic-write error.
    pub fn save(&self, project_root: &Path) -> Result<(), String> {
        self.validate()?;
        let mut bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        bytes.push(b'\n');
        AtomicFileService::new()
            .write(&project_root.join(GAME_SETTINGS_FILE), &bytes)
            .map_err(|error| error.to_string())
    }

    /// # Errors
    /// Returns an error when settings are invalid.
    pub fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let settings = GameSettings::default();
        settings.save(temp.path()).unwrap();
        assert_eq!(GameSettings::load(temp.path()).unwrap(), settings);
    }

    #[test]
    fn settings_without_autosave_use_the_enabled_default() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join(GAME_SETTINGS_FILE), br"{}").unwrap();

        assert!(GameSettings::load(temp.path()).unwrap().autosave);
    }
}
