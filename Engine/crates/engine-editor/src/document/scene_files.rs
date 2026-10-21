use super::{AuthoringDocument, AuthoringError};
use engine_project::VirtualDirectory;
use std::path::{Component, Path, PathBuf};

impl AuthoringDocument {
    /// Saves a scene as the project's start scene without changing the active scene.
    /// # Errors
    /// Returns an error for read-only access, invalid scene data, or failed persistence.
    pub fn set_start_scene(&mut self, relative: &Path) -> Result<(), AuthoringError> {
        self.ensure_writable()?;
        let path = self.scene_file_path(relative)?;
        if same_file(&path, &self.scene_path) && (!path.is_file() || self.is_modified()) {
            self.save()?;
        }
        Self::open_scene(self.project.clone(), relative, self.project_read_only)?;
        self.project.set_start_scene(relative)?;
        Ok(())
    }

    /// Renames a scene file, retaining active edits, recovery files, asset metadata,
    /// and its start-scene reference. A bare name retains the original extension.
    /// # Errors
    /// Returns an error for read-only access, invalid names, collisions, or I/O failure.
    pub fn rename_scene(&mut self, relative: &Path, name: &str) -> Result<PathBuf, AuthoringError> {
        self.ensure_writable()?;
        let source = self.scene_file_path(relative)?;
        let name = scene_filename(relative, name)?;
        let renamed = relative.with_file_name(name);
        let destination =
            source.with_file_name(renamed.file_name().ok_or(AuthoringError::UnsafeScenePath)?);
        if source == destination {
            return Ok(renamed);
        }
        let moves: Vec<_> = ["", ".bak", ".rmeta"]
            .into_iter()
            .map(|suffix| {
                (
                    with_suffix(&source, suffix),
                    with_suffix(&destination, suffix),
                )
            })
            .collect();
        for (_, target) in &moves {
            match std::fs::symlink_metadata(target) {
                Ok(_) => {
                    return Err(operation_error(format!(
                        "An entry already exists at {}",
                        target.display()
                    )));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(operation_error(error.to_string())),
            }
        }
        let active = same_file(&source, &self.scene_path);
        let start = same_file(&source, &self.project.start_scene_path()?);
        if active && (self.is_modified() || !source.is_file()) {
            self.save()?;
        }
        // Reject links and directories for every file before beginning the transaction.
        let mut existing = Vec::new();
        for (from, to) in moves {
            match std::fs::symlink_metadata(&from) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    existing.push((from, to));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && from != source => {}
                _ => {
                    return Err(operation_error(format!(
                        "Not a regular scene file: {}",
                        from.display()
                    )));
                }
            }
        }
        let mut completed = Vec::new();
        for (from, to) in existing {
            if let Err(error) = std::fs::rename(&from, &to) {
                return Err(rollback(&completed, &error.to_string()));
            }
            completed.push((from, to));
        }
        if start && let Err(error) = self.project.set_start_scene(&renamed) {
            return Err(rollback(&completed, &error.to_string()));
        }
        if active {
            self.scene_path = destination;
        }
        Ok(renamed)
    }

    fn scene_file_path(&self, relative: &Path) -> Result<PathBuf, AuthoringError> {
        self.project.validate()?;
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            || !relative
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| {
                    value.eq_ignore_ascii_case("scene") || value.eq_ignore_ascii_case("rscene")
                })
        {
            return Err(AuthoringError::UnsafeScenePath);
        }
        let mut path = self.project.directory(VirtualDirectory::Scenes)?;
        for component in relative.components() {
            path.push(component);
            match std::fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(AuthoringError::UnsafeScenePath);
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(operation_error(error.to_string())),
            }
        }
        Ok(path)
    }
}

fn same_file(left: &Path, right: &Path) -> bool {
    left == right
        || left
            .canonicalize()
            .is_ok_and(|left| right.canonicalize().is_ok_and(|right| left == right))
}

fn scene_filename(relative: &Path, name: &str) -> Result<String, AuthoringError> {
    if name.is_empty()
        || name != name.trim()
        || name.ends_with('.')
        || name.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*'])
        || name.chars().any(char::is_control)
    {
        return Err(operation_error("Enter a single valid scene name"));
    }
    let extension = relative
        .extension()
        .and_then(|value| value.to_str())
        .ok_or(AuthoringError::UnsafeScenePath)?;
    let suffix = format!(".{extension}");
    if name
        .to_ascii_lowercase()
        .ends_with(&suffix.to_ascii_lowercase())
    {
        if name.len() == suffix.len() {
            return Err(operation_error("Enter a scene name before the extension"));
        }
        Ok(name.to_owned())
    } else if Path::new(name).extension().is_some_and(|value| {
        value.eq_ignore_ascii_case("scene") || value.eq_ignore_ascii_case("rscene")
    }) {
        Err(operation_error("Keep the original scene extension"))
    } else {
        Ok(format!("{name}{suffix}"))
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut path = path.as_os_str().to_owned();
    path.push(suffix);
    PathBuf::from(path)
}

fn operation_error(message: impl Into<String>) -> AuthoringError {
    AuthoringError::SceneFileOperation(message.into())
}

fn rollback(moved: &[(PathBuf, PathBuf)], cause: &str) -> AuthoringError {
    let mut message = cause.to_owned();
    for (from, to) in moved.iter().rev() {
        if let Err(error) = std::fs::rename(to, from) {
            use std::fmt::Write as _;
            let _ = write!(message, "; could not restore {}: {error}", from.display());
        }
    }
    operation_error(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_platform::AtomicFileService;
    use engine_project::{PROJECT_DESCRIPTOR_FILE, Project, ProjectTemplate};
    use engine_world::{SceneDocument, save_scene_atomic};

    fn fixture() -> (tempfile::TempDir, AuthoringDocument) {
        let temp = tempfile::tempdir().unwrap();
        let project =
            Project::create(temp.path().join("game"), "Game", ProjectTemplate::Blank).unwrap();
        let mut document = AuthoringDocument::open(project, false).unwrap();
        document.save().unwrap();
        (temp, document)
    }

    #[test]
    fn start_scene_persists_and_reopens_objects_and_environment_without_switching_now() {
        let (_temp, mut document) = fixture();
        let root = document.project.root().to_path_buf();
        let mut night = SceneDocument::new("Night");
        night.environment.enabled = true;
        night.environment.sun_intensity = 0.3;
        night.entities.push(engine_world::EntitySnapshot::default());
        let scenes = document
            .project
            .directory(VirtualDirectory::Scenes)
            .unwrap();
        save_scene_atomic(scenes.join("levels/night.scene"), &night).unwrap();
        let selected = document.add_folder(None).unwrap();
        document
            .set_start_scene(Path::new("levels/night.scene"))
            .unwrap();
        assert_eq!(document.selected(), Some(selected));
        assert!(document.is_modified());
        assert_eq!(document.scene_path(), scenes.join("main.scene"));
        let reopened = AuthoringDocument::open(Project::open(root).unwrap(), false).unwrap();
        assert_eq!(reopened.scene_path(), scenes.join("levels/night.scene"));
        assert_eq!(reopened.entity_snapshots().unwrap(), night.entities);
        assert_eq!(reopened.world().environment(), &night.environment);
    }

    #[test]
    fn active_scene_rename_keeps_edits_history_backups_metadata_and_default_start_reference() {
        let (_temp, mut document) = fixture();
        let source = document.scene_path().to_path_buf();
        let selected = document.add_folder(None).unwrap();
        std::fs::write(with_suffix(&source, ".rmeta"), "asset identity").unwrap();
        let renamed = document
            .rename_scene(Path::new("main.scene"), "Renamed")
            .unwrap();
        assert_eq!(renamed, Path::new("Renamed.scene"));
        let destination = source.with_file_name(&renamed);
        assert_eq!(document.scene_path(), destination);
        assert_eq!(document.selected(), Some(selected));
        assert!(destination.is_file());
        assert!(!source.exists());
        assert!(with_suffix(&destination, ".bak").is_file());
        assert!(!with_suffix(&source, ".bak").exists());
        assert_eq!(
            std::fs::read_to_string(with_suffix(&destination, ".rmeta")).unwrap(),
            "asset identity"
        );
        let reopened =
            AuthoringDocument::open(Project::open(document.project.root()).unwrap(), false)
                .unwrap();
        assert_eq!(reopened.scene_path(), destination);
        assert!(reopened.world().contains(selected));
        document.undo().unwrap();
        assert!(!document.world().contains(selected));
        document.redo().unwrap();
        document.save().unwrap();
        assert!(
            !source.exists(),
            "saving after rename must not recreate the old scene"
        );
    }

    #[test]
    fn renaming_inactive_start_scene_updates_start_without_touching_active_edits() {
        let (_temp, mut document) = fixture();
        let scenes = document
            .project
            .directory(VirtualDirectory::Scenes)
            .unwrap();
        save_scene_atomic(scenes.join("night.rscene"), &SceneDocument::new("Night")).unwrap();
        document.set_start_scene(Path::new("night.rscene")).unwrap();
        let selected = document.add_folder(None).unwrap();
        document
            .rename_scene(Path::new("night.rscene"), "Moon.rscene")
            .unwrap();
        assert_eq!(document.selected(), Some(selected));
        assert!(document.is_modified());
        assert_eq!(document.scene_path(), scenes.join("main.scene"));
        assert_eq!(
            Project::open(document.project.root())
                .unwrap()
                .start_scene_path()
                .unwrap(),
            scenes.join("Moon.rscene")
        );
    }

    #[test]
    fn invalid_rename_collision_and_read_only_actions_preserve_scene_and_configuration() {
        let (_temp, mut document) = fixture();
        let source = document.scene_path().to_path_buf();
        let bytes = std::fs::read(&source).unwrap();
        let descriptor =
            std::fs::read(document.project.root().join(PROJECT_DESCRIPTOR_FILE)).unwrap();
        save_scene_atomic(
            source.with_file_name("occupied.scene"),
            &SceneDocument::new("Occupied"),
        )
        .unwrap();
        std::fs::write(source.with_file_name("recovery.scene.bak"), "backup").unwrap();
        std::fs::write(source.with_file_name("asset.scene.rmeta"), "metadata").unwrap();
        for name in [
            "",
            "../escape",
            "bad/name",
            "occupied.scene",
            "recovery.scene",
            "asset.scene",
            ".scene",
            "main.rscene",
        ] {
            assert!(
                document
                    .rename_scene(Path::new("main.scene"), name)
                    .is_err(),
                "accepted {name}"
            );
        }
        assert!(
            document
                .set_start_scene(Path::new("missing.scene"))
                .is_err()
        );
        assert!(
            document
                .set_start_scene(Path::new("../main.scene"))
                .is_err()
        );
        document.read_only = true;
        assert!(
            document
                .rename_scene(Path::new("main.scene"), "Renamed")
                .is_err()
        );
        assert!(document.set_start_scene(Path::new("main.scene")).is_err());
        assert_eq!(std::fs::read(source).unwrap(), bytes);
        assert_eq!(
            std::fs::read(document.project.root().join(PROJECT_DESCRIPTOR_FILE)).unwrap(),
            descriptor
        );
    }

    #[test]
    fn failed_start_reference_save_rolls_back_renamed_files_and_keeps_active_path() {
        let (_temp, mut document) = fixture();
        let source = document.scene_path().to_path_buf();
        let descriptor_path = document.project.root().join(PROJECT_DESCRIPTOR_FILE);
        let descriptor = std::fs::read(&descriptor_path).unwrap();
        std::fs::create_dir(AtomicFileService::backup_path(&descriptor_path)).unwrap();
        let selected = document.add_folder(None).unwrap();
        assert!(
            document
                .rename_scene(Path::new("main.scene"), "Renamed")
                .is_err()
        );
        assert_eq!(document.scene_path(), source);
        assert!(source.is_file());
        assert!(!source.with_file_name("Renamed.scene").exists());
        assert!(document.world().contains(selected));
        assert_eq!(document.project.descriptor().start_scene, None);
        assert_eq!(std::fs::read(descriptor_path).unwrap(), descriptor);
    }

    #[test]
    fn missing_explicit_start_scene_does_not_silently_create_an_empty_scene() {
        let (_temp, mut document) = fixture();
        document.set_start_scene(Path::new("main.scene")).unwrap();
        std::fs::remove_file(document.scene_path()).unwrap();
        assert!(AuthoringDocument::open(document.project.clone(), false).is_err());
    }
}
