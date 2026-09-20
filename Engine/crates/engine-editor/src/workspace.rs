use engine_platform::{ConfigError, ConfigSchema, ConfigStore};
use engine_project::ProjectTemplate;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Durable editor tabs. The actual split/window geometry is owned by the docking widget,
/// while this schema keeps the visible set and safe fallback bounds stable across versions.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorTab {
    Viewport3d,
    Viewport2d,
    Hierarchy,
    Inspector,
    ContentBrowser,
    Console,
    Settings,
}

impl EditorTab {
    pub const ALL: [Self; 5] = [
        Self::Viewport3d,
        Self::Viewport2d,
        Self::Hierarchy,
        Self::Inspector,
        Self::Console,
    ];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Viewport3d => "3D Viewport",
            Self::Viewport2d => "2D Viewport",
            Self::Hierarchy => "Game Project Explorer",
            Self::Inspector => "Inspector",
            Self::ContentBrowser => "Content Browser",
            Self::Console => "Console",
            Self::Settings => "Settings",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceWindow {
    pub tab: EditorTab,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub monitor: i32,
}

/// Versioned layout preferences with conservative recovery for missing monitors.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditorWorkspace {
    pub tabs: Vec<EditorTab>,
    pub windows: Vec<WorkspaceWindow>,
    pub ui_scale: f32,
}

impl Default for EditorWorkspace {
    fn default() -> Self {
        Self {
            tabs: EditorTab::ALL.to_vec(),
            windows: Vec::new(),
            ui_scale: 1.0,
        }
    }
}

impl ConfigSchema for EditorWorkspace {
    const FORMAT_VERSION: u32 = 2;

    fn migrate(from_version: u32, value: ron::Value) -> Result<Self, String> {
        if from_version != 1 {
            return Err(format!("unsupported workspace version {from_version}"));
        }
        value.into_rust().map_err(|error| error.to_string())
    }
}

impl EditorWorkspace {
    /// Template-aware initial panel set. Blank is deliberately neutral and exposes both views.
    pub fn for_template(template: ProjectTemplate) -> Self {
        Self {
            tabs: match template {
                ProjectTemplate::Empty2d
                | ProjectTemplate::Platformer
                | ProjectTemplate::TopDown => vec![
                    EditorTab::Viewport2d,
                    EditorTab::Hierarchy,
                    EditorTab::Inspector,
                    EditorTab::Console,
                ],
                ProjectTemplate::Blank => EditorTab::ALL.to_vec(),
                _ => vec![
                    EditorTab::Viewport3d,
                    EditorTab::Hierarchy,
                    EditorTab::Inspector,
                    EditorTab::Console,
                ],
            },
            ..Self::default()
        }
    }

    /// Converts the old all-tabs default to a template-aware layout while preserving custom sets.
    pub fn migrate_legacy_default_for_template(&mut self, template: ProjectTemplate) -> bool {
        if self.tabs == EditorTab::ALL && template != ProjectTemplate::Blank {
            self.tabs = Self::for_template(template).tabs;
            true
        } else {
            false
        }
    }

    /// Consolidates the legacy content browser into the project explorer panel.
    pub fn consolidate_project_explorer(&mut self) -> bool {
        let had_content_browser = self.tabs.contains(&EditorTab::ContentBrowser);
        self.tabs.retain(|tab| *tab != EditorTab::ContentBrowser);
        if had_content_browser && !self.tabs.contains(&EditorTab::Hierarchy) {
            self.tabs.push(EditorTab::Hierarchy);
        }
        for window in &mut self.windows {
            if window.tab == EditorTab::ContentBrowser {
                window.tab = EditorTab::Hierarchy;
            }
        }
        let had_settings = self.tabs.contains(&EditorTab::Settings);
        self.tabs.retain(|tab| *tab != EditorTab::Settings);
        self.windows
            .retain(|window| window.tab != EditorTab::Settings);
        had_content_browser || had_settings
    }
    /// Loads settings, recovering missing data with defaults.
    ///
    /// # Errors
    ///
    /// Returns an error when settings cannot be read, recovered, or migrated.
    pub fn load(store: &ConfigStore, path: &Path) -> Result<Self, ConfigError> {
        store
            .load(path)
            .map(|load| load.value_or_else(Self::default))
    }

    /// Persists settings transactionally.
    ///
    /// # Errors
    ///
    /// Returns an error when settings cannot be serialized or written.
    pub fn save(&self, store: &ConfigStore, path: &Path) -> Result<(), ConfigError> {
        store.save(path, self)
    }

    /// Moves windows from monitors no longer present back onto the primary work area.
    pub fn recover_monitors(&mut self, monitor_count: usize, primary_size: [f32; 2]) -> usize {
        let mut recovered = 0;
        for window in &mut self.windows {
            let unavailable = window.monitor < 0
                || usize::try_from(window.monitor).map_or(true, |value| value >= monitor_count);
            if unavailable
                || !window.position.iter().all(|value| value.is_finite())
                || !window
                    .size
                    .iter()
                    .all(|value| value.is_finite() && *value > 0.0)
            {
                window.monitor = 0;
                let recovered_offset = u16::try_from(recovered).unwrap_or(u16::MAX);
                window.position = [32.0 + f32::from(recovered_offset) * 24.0; 2];
                window.size = [primary_size[0].min(960.0), primary_size[1].min(720.0)];
                recovered += 1;
            }
        }
        self.ui_scale = self.ui_scale.clamp(0.75, 2.5);
        recovered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_platform::AtomicFileService;
    use tempfile::tempdir;

    #[test]
    fn workspace_round_trip_and_missing_monitor_recovery() {
        let temporary = tempdir().unwrap();
        let path = temporary.path().join("workspace.ron");
        let store = ConfigStore::new(AtomicFileService::new());
        let mut workspace = EditorWorkspace::default();
        workspace.windows.push(WorkspaceWindow {
            tab: EditorTab::Inspector,
            position: [9000.0, 100.0],
            size: [500.0, 700.0],
            monitor: 3,
        });
        workspace.save(&store, &path).unwrap();
        let mut reopened = EditorWorkspace::load(&store, &path).unwrap();
        assert_eq!(reopened.recover_monitors(1, [1_440.0, 900.0]), 1);
        assert_eq!(reopened.windows[0].monitor, 0);
    }

    #[test]
    fn template_defaults_and_legacy_migration_choose_the_expected_viewport() {
        let two_d = EditorWorkspace::for_template(ProjectTemplate::Empty2d);
        assert!(two_d.tabs.contains(&EditorTab::Viewport2d));
        assert!(!two_d.tabs.contains(&EditorTab::Viewport3d));
        let three_d = EditorWorkspace::for_template(ProjectTemplate::Empty3d);
        assert!(three_d.tabs.contains(&EditorTab::Viewport3d));
        assert!(!three_d.tabs.contains(&EditorTab::Viewport2d));
        assert!(
            EditorWorkspace::for_template(ProjectTemplate::Blank)
                .tabs
                .contains(&EditorTab::Viewport2d)
        );

        let mut legacy = EditorWorkspace::default();
        assert!(legacy.migrate_legacy_default_for_template(ProjectTemplate::Empty3d));
        assert_eq!(legacy, three_d);
    }
}
