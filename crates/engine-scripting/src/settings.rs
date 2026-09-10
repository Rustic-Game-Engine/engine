use engine_platform::AtomicFileService;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

pub const GAME_SETTINGS_FILE: &str = "settings.json";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GameSettings {
    pub entry_script: PathBuf,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            entry_script: PathBuf::from("scripts/main.lua"),
        }
    }
}

impl GameSettings {
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

    pub fn save(&self, project_root: &Path) -> Result<(), String> {
        self.validate()?;
        let mut bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        bytes.push(b'\n');
        AtomicFileService::new()
            .write(&project_root.join(GAME_SETTINGS_FILE), &bytes)
            .map_err(|error| error.to_string())
    }

    pub fn validate(&self) -> Result<(), String> {
        let path = &self.entry_script;
        if path.as_os_str().is_empty()
            || path.is_absolute()
            || !path
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
            || crate::ScriptLanguage::from_path(path).is_none()
        {
            return Err(format!(
                "entry script must be a safe project-relative script path: {}",
                path.display()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_reject_unsafe_entry_path() {
        let temp = tempfile::tempdir().unwrap();
        let settings = GameSettings::default();
        settings.save(temp.path()).unwrap();
        assert_eq!(GameSettings::load(temp.path()).unwrap(), settings);
        assert!(
            GameSettings {
                entry_script: "../main.lua".into()
            }
            .validate()
            .is_err()
        );
    }
}
