use engine_platform::ConfigSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LauncherSettings {
    pub theme: ThemePreference,
    pub background_workers: u8,
    pub thumbnail_cache_enabled: bool,
}

impl Default for LauncherSettings {
    fn default() -> Self {
        let workers = std::thread::available_parallelism().map_or(2, usize::from);
        Self {
            theme: ThemePreference::System,
            background_workers: u8::try_from(workers.clamp(1, 8)).unwrap_or(2),
            thumbnail_cache_enabled: true,
        }
    }
}

impl ConfigSchema for LauncherSettings {
    const FORMAT_VERSION: u32 = 1;
}
