use directories::{ProjectDirs, UserDirs};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Failures reported by native path discovery.
#[derive(Debug, Error)]
pub enum PlatformError {
    /// No platform-specific application data directory could be resolved.
    #[error("the operating system did not provide an application data directory")]
    ApplicationDataUnavailable,
    /// The current executable could not be located.
    #[error("could not locate the current executable: {0}")]
    CurrentExecutable(#[source] std::io::Error),
}

/// Locations owned by the installed engine rather than by a game project.
pub trait PlatformPaths {
    fn project_registry_file(&self) -> &Path;
    fn launcher_settings_file(&self) -> &Path;
    fn launcher_log_directory(&self) -> &Path;
    fn thumbnail_cache_directory(&self) -> &Path;
    fn launcher_startup_marker(&self) -> &Path;
    fn default_project_parent(&self) -> &Path;
    /// Resolves an executable installed beside the engine binaries.
    ///
    /// # Errors
    ///
    /// Returns an error when the executable stem is empty or invalid.
    fn sibling_executable(&self, stem: &str) -> Result<PathBuf, PlatformError>;
}

/// Operating-system path discovery for the current user and installation.
#[derive(Debug, Clone)]
pub struct NativePlatformPaths {
    project_registry_file: PathBuf,
    launcher_settings_file: PathBuf,
    launcher_log_directory: PathBuf,
    thumbnail_cache_directory: PathBuf,
    launcher_startup_marker: PathBuf,
    default_project_parent: PathBuf,
    executable_directory: PathBuf,
}

impl NativePlatformPaths {
    /// Discovers paths without creating or mutating directories.
    ///
    /// # Errors
    ///
    /// Returns an error when the operating system does not expose an application-data location
    /// or the current executable path cannot be determined.
    pub fn discover() -> Result<Self, PlatformError> {
        let project_dirs = ProjectDirs::from("org", "RusticEngine", "RusticGameEngine")
            .ok_or(PlatformError::ApplicationDataUnavailable)?;
        let data = project_dirs.data_local_dir();
        let config = project_dirs.config_local_dir();
        let cache = project_dirs.cache_dir();
        let default_project_parent = UserDirs::new()
            .and_then(|directories| directories.document_dir().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("Projects"))
            .join("RusticProjects");
        let current_executable =
            std::env::current_exe().map_err(PlatformError::CurrentExecutable)?;
        let executable_directory = current_executable
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| {
                PlatformError::CurrentExecutable(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "current executable has no parent directory",
                ))
            })?;
        Ok(Self {
            project_registry_file: data.join("projects.ron"),
            launcher_settings_file: config.join("launcher-settings.ron"),
            launcher_log_directory: data.join("logs"),
            thumbnail_cache_directory: cache.join("thumbnails"),
            launcher_startup_marker: data.join("launcher.startup"),
            default_project_parent,
            executable_directory,
        })
    }

    #[cfg(test)]
    fn from_root(root: &Path) -> Self {
        Self {
            project_registry_file: root.join("data/projects.ron"),
            launcher_settings_file: root.join("config/settings.ron"),
            launcher_log_directory: root.join("data/logs"),
            thumbnail_cache_directory: root.join("cache/thumbnails"),
            launcher_startup_marker: root.join("data/launcher.startup"),
            default_project_parent: root.join("projects"),
            executable_directory: root.join("bin"),
        }
    }
}

impl PlatformPaths for NativePlatformPaths {
    fn project_registry_file(&self) -> &Path {
        &self.project_registry_file
    }

    fn launcher_settings_file(&self) -> &Path {
        &self.launcher_settings_file
    }

    fn launcher_log_directory(&self) -> &Path {
        &self.launcher_log_directory
    }

    fn thumbnail_cache_directory(&self) -> &Path {
        &self.thumbnail_cache_directory
    }

    fn launcher_startup_marker(&self) -> &Path {
        &self.launcher_startup_marker
    }

    fn default_project_parent(&self) -> &Path {
        &self.default_project_parent
    }

    fn sibling_executable(&self, stem: &str) -> Result<PathBuf, PlatformError> {
        let executable_name = if cfg!(windows) {
            format!("{stem}.exe")
        } else {
            stem.to_owned()
        };
        Ok(self.executable_directory.join(executable_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_engine_owned_path_is_below_the_fixture_root() {
        let root = Path::new("fixture-root");
        let paths = NativePlatformPaths::from_root(root);
        for path in [
            paths.project_registry_file(),
            paths.launcher_settings_file(),
            paths.launcher_log_directory(),
            paths.thumbnail_cache_directory(),
            paths.launcher_startup_marker(),
            paths.default_project_parent(),
        ] {
            assert!(path.starts_with(root));
        }
    }

    #[test]
    fn sibling_executable_uses_the_native_suffix() {
        let paths = NativePlatformPaths::from_root(Path::new("fixture"));
        let expected = if cfg!(windows) {
            PathBuf::from("fixture/bin/rustic-editor.exe")
        } else {
            PathBuf::from("fixture/bin/rustic-editor")
        };
        assert_eq!(paths.sibling_executable("rustic-editor").unwrap(), expected);
    }
}
