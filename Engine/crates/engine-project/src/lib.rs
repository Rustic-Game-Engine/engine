//! Project descriptors and safe project-directory management.
//!
//! A project is rooted at a directory containing a human-readable RON file named
//! [`PROJECT_DESCRIPTOR_FILE`]. All virtual directories are relative to that root.

mod catalog;
mod template;

pub use catalog::{
    CURRENT_CATALOG_VERSION, CatalogError, CatalogLoad, CatalogLoadStatus, CatalogSort,
    ProjectCatalog, ProjectRecord, ThumbnailCache, ThumbnailError, ThumbnailRef,
};
pub use template::{CURRENT_TEMPLATE_MANIFEST_VERSION, TEMPLATE_MANIFEST_FILE, TemplateManifest};

pub use engine_core::ProjectId;
use engine_platform::AtomicFileService;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use uuid::Uuid;

/// The descriptor filename at the root of every engine project.
pub const PROJECT_DESCRIPTOR_FILE: &str = "project.engine";
/// The project descriptor format understood by this version of the crate.
pub const CURRENT_FORMAT_VERSION: u32 = 1;

fn default_ui_directory() -> PathBuf {
    "ui".into()
}

/// Errors produced while creating, opening, or validating projects.
#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("project descriptor is not valid RON: {0}")]
    InvalidDescriptor(String),
    #[error("could not serialize project descriptor: {0}")]
    Serialize(String),
    #[error("unsupported project format version {found}; expected {expected}")]
    UnsupportedFormat { found: u32, expected: u32 },
    #[error("invalid project name: {0}")]
    InvalidName(String),
    #[error("unsafe path for virtual directory `{directory}`: {path}")]
    UnsafeVirtualPath {
        directory: VirtualDirectory,
        path: PathBuf,
    },
    #[error("virtual directory `{directory}` escapes the project root: {path}")]
    PathEscapesProject {
        directory: VirtualDirectory,
        path: PathBuf,
    },
    #[error("project destination already contains data: {0}")]
    DestinationNotEmpty(PathBuf),
    #[error("a project already exists at {0}")]
    ProjectAlreadyExists(PathBuf),
    #[error("project descriptor was not found at {0}")]
    DescriptorNotFound(PathBuf),
    #[error("project root is not a directory: {0}")]
    RootNotDirectory(PathBuf),
    #[error("virtual directory `{directory}` is missing or is not a directory: {path}")]
    MissingVirtualDirectory {
        directory: VirtualDirectory,
        path: PathBuf,
    },
    #[error("transactional project write failed: {0}")]
    Atomic(#[from] engine_platform::AtomicWriteError),
    #[error("project template scene failed: {0}")]
    TemplateScene(#[from] engine_world::SceneError),
    #[error("project template manifest failed: {0}")]
    TemplateManifest(String),
}

fn io_error(path: impl Into<PathBuf>, source: std::io::Error) -> ProjectError {
    ProjectError::Io {
        path: path.into(),
        source,
    }
}

/// A small set of starting points. Templates only supply metadata defaults; they
/// never overwrite files in an existing directory.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTemplate {
    /// A minimal project configured for 3D content.
    #[default]
    Empty3d,
    /// A minimal project configured for 2D content.
    Empty2d,
    /// A first-person 3D game starting point.
    FirstPerson,
    /// A third-person 3D game starting point.
    ThirdPerson,
    /// A side-view platform game starting point.
    Platformer,
    /// A top-down game starting point.
    TopDown,
    /// A non-game graphical application starting point.
    UiApplication,
    /// Directory structure only, with no gameplay assumptions.
    Blank,
}

impl ProjectTemplate {
    pub fn brief(self) -> &'static str {
        match self {
            Self::Empty3d => "A minimal 3D project ready for scenes, scripts, and materials.",
            Self::Empty2d => "A minimal 2D project ready for sprites, scenes, and scripts.",
            Self::FirstPerson => "A first-person 3D project with gameplay-oriented defaults.",
            Self::ThirdPerson => "A third-person 3D project with gameplay-oriented defaults.",
            Self::Platformer => "A platform game project with side-view gameplay defaults.",
            Self::TopDown => "A top-down game project with overhead gameplay defaults.",
            Self::UiApplication => "A graphical application centered on user-interface scenes.",
            Self::Blank => "A blank project with only the standard directory layout.",
        }
    }

    fn default_tags(self) -> Vec<String> {
        match self {
            Self::Empty3d => vec!["3d".to_owned()],
            Self::Empty2d => vec!["2d".to_owned()],
            Self::FirstPerson => vec!["3d".to_owned(), "first-person".to_owned()],
            Self::ThirdPerson => vec!["3d".to_owned(), "third-person".to_owned()],
            Self::Platformer => vec!["2d".to_owned(), "platformer".to_owned()],
            Self::TopDown => vec!["2d".to_owned(), "top-down".to_owned()],
            Self::UiApplication => vec!["ui".to_owned(), "application".to_owned()],
            Self::Blank => Vec::new(),
        }
    }
}

/// User-facing project metadata persisted in the descriptor.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectMetadata {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_unix_seconds: u64,
    pub updated_unix_seconds: u64,
}

impl ProjectMetadata {
    pub fn new(name: impl Into<String>) -> Self {
        let now = unix_seconds();
        Self {
            name: name.into(),
            description: None,
            author: None,
            engine_version: None,
            tags: Vec::new(),
            created_unix_seconds: now,
            updated_unix_seconds: now,
        }
    }
}

/// Logical directories available to engine systems.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum VirtualDirectory {
    Assets,
    Ui,
    Scenes,
    Scripts,
    Shaders,
    Materials,
    Plugins,
    Config,
    Cache,
    Temp,
    Builds,
    Logs,
}

impl VirtualDirectory {
    pub const ALL: [Self; 12] = [
        Self::Assets,
        Self::Ui,
        Self::Scenes,
        Self::Scripts,
        Self::Shaders,
        Self::Materials,
        Self::Plugins,
        Self::Config,
        Self::Cache,
        Self::Temp,
        Self::Builds,
        Self::Logs,
    ];
}

impl fmt::Display for VirtualDirectory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Assets => "assets",
            Self::Ui => "ui",
            Self::Scenes => "scenes",
            Self::Scripts => "scripts",
            Self::Shaders => "shaders",
            Self::Materials => "materials",
            Self::Plugins => "plugins",
            Self::Config => "config",
            Self::Cache => "cache",
            Self::Temp => "temp",
            Self::Builds => "builds",
            Self::Logs => "logs",
        })
    }
}

/// Configurable relative paths for the engine's virtual directories.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VirtualDirectoryMap {
    pub assets: PathBuf,
    #[serde(default = "default_ui_directory")]
    pub ui: PathBuf,
    pub scenes: PathBuf,
    pub scripts: PathBuf,
    pub shaders: PathBuf,
    pub materials: PathBuf,
    pub plugins: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,
    pub temp: PathBuf,
    pub builds: PathBuf,
    pub logs: PathBuf,
}

impl Default for VirtualDirectoryMap {
    fn default() -> Self {
        Self {
            assets: "assets".into(),
            ui: default_ui_directory(),
            // Legacy descriptors that omitted the directory map used this path.
            scenes: "scenes".into(),
            scripts: "scripts".into(),
            shaders: "shaders".into(),
            materials: "materials".into(),
            plugins: "plugins".into(),
            config: "config".into(),
            cache: "cache".into(),
            temp: "temp".into(),
            builds: "builds".into(),
            logs: "logs".into(),
        }
    }
}

impl VirtualDirectoryMap {
    pub fn get(&self, directory: VirtualDirectory) -> &Path {
        match directory {
            VirtualDirectory::Assets => &self.assets,
            VirtualDirectory::Ui => &self.ui,
            VirtualDirectory::Scenes => &self.scenes,
            VirtualDirectory::Scripts => &self.scripts,
            VirtualDirectory::Shaders => &self.shaders,
            VirtualDirectory::Materials => &self.materials,
            VirtualDirectory::Plugins => &self.plugins,
            VirtualDirectory::Config => &self.config,
            VirtualDirectory::Cache => &self.cache,
            VirtualDirectory::Temp => &self.temp,
            VirtualDirectory::Builds => &self.builds,
            VirtualDirectory::Logs => &self.logs,
        }
    }

    pub fn get_mut(&mut self, directory: VirtualDirectory) -> &mut PathBuf {
        match directory {
            VirtualDirectory::Assets => &mut self.assets,
            VirtualDirectory::Ui => &mut self.ui,
            VirtualDirectory::Scenes => &mut self.scenes,
            VirtualDirectory::Scripts => &mut self.scripts,
            VirtualDirectory::Shaders => &mut self.shaders,
            VirtualDirectory::Materials => &mut self.materials,
            VirtualDirectory::Plugins => &mut self.plugins,
            VirtualDirectory::Config => &mut self.config,
            VirtualDirectory::Cache => &mut self.cache,
            VirtualDirectory::Temp => &mut self.temp,
            VirtualDirectory::Builds => &mut self.builds,
            VirtualDirectory::Logs => &mut self.logs,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (VirtualDirectory, &Path)> {
        VirtualDirectory::ALL
            .into_iter()
            .map(|directory| (directory, self.get(directory)))
    }
}

/// The versioned data stored in [`PROJECT_DESCRIPTOR_FILE`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectDescriptor {
    pub format_version: u32,
    pub project_id: ProjectId,
    pub template: ProjectTemplate,
    pub metadata: ProjectMetadata,
    #[serde(default)]
    pub directories: VirtualDirectoryMap,
}

impl ProjectDescriptor {
    pub fn new(name: impl Into<String>, template: ProjectTemplate) -> Self {
        let mut metadata = ProjectMetadata::new(name);
        metadata.description = Some(template.brief().to_owned());
        metadata.tags = template.default_tags();
        let directories = VirtualDirectoryMap {
            scenes: "scene".into(),
            ..VirtualDirectoryMap::default()
        };
        Self {
            format_version: CURRENT_FORMAT_VERSION,
            project_id: ProjectId::new(),
            template,
            metadata,
            directories,
        }
    }

    /// Validate descriptor data without accessing the filesystem.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError`] when the format version, project name, or a configured
    /// virtual path is invalid.
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.format_version != CURRENT_FORMAT_VERSION {
            return Err(ProjectError::UnsupportedFormat {
                found: self.format_version,
                expected: CURRENT_FORMAT_VERSION,
            });
        }
        validate_name(&self.metadata.name)?;
        for (directory, path) in self.directories.iter() {
            validate_relative_path(directory, path)?;
        }
        Ok(())
    }
}

/// Options for creating a project. Callers may customize all virtual paths before
/// creation; unsafe paths are rejected before the destination is modified.
#[derive(Clone, Debug)]
pub struct CreateProjectOptions {
    pub name: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub engine_version: Option<String>,
    pub template: ProjectTemplate,
    pub directories: VirtualDirectoryMap,
}

impl CreateProjectOptions {
    pub fn new(name: impl Into<String>, template: ProjectTemplate) -> Self {
        let directories = VirtualDirectoryMap {
            scenes: "scene".into(),
            ..VirtualDirectoryMap::default()
        };
        Self {
            name: name.into(),
            description: None,
            author: None,
            engine_version: None,
            template,
            directories,
        }
    }

    fn into_descriptor(self) -> ProjectDescriptor {
        let mut descriptor = ProjectDescriptor::new(self.name, self.template);
        if self.description.is_some() {
            descriptor.metadata.description = self.description;
        }
        descriptor.metadata.author = self.author;
        descriptor.metadata.engine_version = self.engine_version;
        descriptor.directories = self.directories;
        descriptor
    }
}

/// An opened project and its parsed descriptor.
#[derive(Clone, Debug)]
pub struct Project {
    root: PathBuf,
    descriptor: ProjectDescriptor,
}

impl Project {
    /// Create a project with the default virtual-directory layout.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError`] if the destination is unsafe or non-empty, descriptor
    /// validation fails, or a directory/descriptor cannot be written.
    pub fn create(
        root: impl AsRef<Path>,
        name: impl Into<String>,
        template: ProjectTemplate,
    ) -> Result<Self, ProjectError> {
        Self::create_with_options(root, CreateProjectOptions::new(name, template))
    }

    /// Create a project without overwriting any pre-existing data.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError`] if the options are invalid, the destination contains
    /// existing data, a path escapes the project root, or filesystem I/O fails.
    pub fn create_with_options(
        root: impl AsRef<Path>,
        options: CreateProjectOptions,
    ) -> Result<Self, ProjectError> {
        let descriptor = options.into_descriptor();
        descriptor.validate()?;

        let root = root.as_ref().to_path_buf();
        let root_preexisted = prepare_empty_destination(&root)?;
        let parent = root.parent().ok_or_else(|| {
            io_error(
                &root,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "project root has no parent",
                ),
            )
        })?;
        fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
        let staging = project_creation_staging_path(&root);
        fs::create_dir(&staging).map_err(|error| io_error(&staging, error))?;

        let create_result = (|| -> Result<(), ProjectError> {
            for (directory, relative) in descriptor.directories.iter() {
                let target = checked_join(&staging, directory, relative)?;
                ensure_existing_ancestor_contained(&staging, directory, &target)?;
                fs::create_dir_all(&target).map_err(|error| io_error(&target, error))?;
                ensure_existing_ancestor_contained(&staging, directory, &target)?;
            }
            write_descriptor(&staging, &descriptor, false)?;
            template::materialize_template(&staging, &descriptor)?;
            Ok(())
        })();
        if let Err(error) = create_result {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        if root_preexisted {
            fs::remove_dir(&root).map_err(|error| io_error(&root, error))?;
        }
        if let Err(error) = fs::rename(&staging, &root) {
            if root_preexisted {
                let _ = fs::create_dir(&root);
            }
            let _ = fs::remove_dir_all(&staging);
            return Err(io_error(&root, error));
        }
        let root = std::path::absolute(&root).map_err(|error| io_error(&root, error))?;
        let project = Self { root, descriptor };
        project.validate()?;
        Ok(project)
    }

    /// Open and fully validate an existing project.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError`] when the descriptor is absent, malformed, incompatible,
    /// or any configured project directory is invalid or missing.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let supplied_root = root.as_ref().to_path_buf();
        let root = supplied_root.clone();
        if !root.exists() {
            return Err(ProjectError::DescriptorNotFound(
                root.join(PROJECT_DESCRIPTOR_FILE),
            ));
        }
        if !root.is_dir() {
            return Err(ProjectError::RootNotDirectory(root));
        }
        let root = std::path::absolute(&root).map_err(|error| io_error(&supplied_root, error))?;
        let descriptor_path = root.join(PROJECT_DESCRIPTOR_FILE);
        if !descriptor_path.is_file() {
            return Err(ProjectError::DescriptorNotFound(descriptor_path));
        }
        let files = AtomicFileService::new();
        files.recover(&descriptor_path)?;
        let descriptor = match read_descriptor(&descriptor_path) {
            Ok(descriptor) => descriptor,
            Err(error @ ProjectError::UnsupportedFormat { .. }) => return Err(error),
            Err(primary_error) => {
                let backup = AtomicFileService::backup_path(&descriptor_path);
                let backup_descriptor = read_descriptor(&backup);
                let Ok(backup_descriptor) = backup_descriptor else {
                    return Err(primary_error);
                };
                let quarantine = descriptor_path.with_file_name(format!(
                    "{PROJECT_DESCRIPTOR_FILE}.corrupt-{}",
                    Uuid::new_v4()
                ));
                fs::rename(&descriptor_path, &quarantine)
                    .map_err(|error| io_error(&descriptor_path, error))?;
                if !files.restore_backup(&descriptor_path)? {
                    return Err(primary_error);
                }
                backup_descriptor
            }
        };
        let project = Self { root, descriptor };
        project.validate()?;
        Ok(project)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn descriptor(&self) -> &ProjectDescriptor {
        &self.descriptor
    }

    pub fn metadata(&self) -> &ProjectMetadata {
        &self.descriptor.metadata
    }

    pub fn metadata_mut(&mut self) -> &mut ProjectMetadata {
        &mut self.descriptor.metadata
    }

    pub fn id(&self) -> ProjectId {
        self.descriptor.project_id
    }

    /// Resolve a virtual directory after validating its configured relative path.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError`] if the configured path is absolute or contains a parent,
    /// root, or platform-prefix component.
    pub fn directory(&self, directory: VirtualDirectory) -> Result<PathBuf, ProjectError> {
        checked_join(
            &self.root,
            directory,
            self.descriptor.directories.get(directory),
        )
    }

    /// Validate descriptor values, directory existence, and physical containment.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError`] if descriptor validation fails, a directory is missing,
    /// or canonical path resolution shows that a directory escapes the project root.
    pub fn validate(&self) -> Result<(), ProjectError> {
        self.descriptor.validate()?;
        if !self.root.is_dir() {
            return Err(ProjectError::RootNotDirectory(self.root.clone()));
        }
        let canonical_root =
            fs::canonicalize(&self.root).map_err(|error| io_error(&self.root, error))?;
        for (directory, relative) in self.descriptor.directories.iter() {
            let path = checked_join(&self.root, directory, relative)?;
            if !path.is_dir() {
                return Err(ProjectError::MissingVirtualDirectory { directory, path });
            }
            let canonical = fs::canonicalize(&path).map_err(|error| io_error(&path, error))?;
            if !canonical.starts_with(&canonical_root) {
                return Err(ProjectError::PathEscapesProject { directory, path });
            }
        }
        Ok(())
    }

    /// Persist metadata changes using a same-directory temporary file and rename.
    /// The previous descriptor is restored if the replacement rename fails.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectError`] if validation or the atomic replacement sequence fails.
    pub fn save(&mut self) -> Result<(), ProjectError> {
        self.descriptor.validate()?;
        self.validate()?;
        self.descriptor.metadata.updated_unix_seconds = unix_seconds();
        write_descriptor(&self.root, &self.descriptor, true)
    }
}

/// Convenience wrapper around [`Project::create`].
///
/// # Errors
///
/// Returns any project validation, safety, or filesystem error from [`Project::create`].
pub fn create_project(
    root: impl AsRef<Path>,
    name: impl Into<String>,
    template: ProjectTemplate,
) -> Result<Project, ProjectError> {
    Project::create(root, name, template)
}

/// Convenience wrapper around [`Project::open`].
///
/// # Errors
///
/// Returns any descriptor, validation, or filesystem error from [`Project::open`].
pub fn open_project(root: impl AsRef<Path>) -> Result<Project, ProjectError> {
    Project::open(root)
}

/// Open and validate a project in one operation.
///
/// # Errors
///
/// Returns any descriptor, validation, or filesystem error from [`Project::open`].
pub fn validate_project(root: impl AsRef<Path>) -> Result<(), ProjectError> {
    Project::open(root).map(|_| ())
}

fn validate_name(name: &str) -> Result<(), ProjectError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ProjectError::InvalidName("name cannot be empty".to_owned()));
    }
    if matches!(trimmed, "." | "..") {
        return Err(ProjectError::InvalidName(
            "name cannot be a relative path component".to_owned(),
        ));
    }
    if trimmed.ends_with(['.', ' ']) {
        return Err(ProjectError::InvalidName(
            "name cannot end with a dot or space".to_owned(),
        ));
    }
    if trimmed
        .chars()
        .any(|character| character.is_control() || r#"<>:"/\|?*"#.contains(character))
    {
        return Err(ProjectError::InvalidName(
            "name contains a reserved path character".to_owned(),
        ));
    }
    let device_stem = trimmed
        .split('.')
        .next()
        .unwrap_or(trimmed)
        .to_ascii_uppercase();
    let reserved_device = matches!(device_stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || device_stem
            .strip_prefix("COM")
            .or_else(|| device_stem.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            });
    if reserved_device {
        return Err(ProjectError::InvalidName(
            "name is reserved by Windows".to_owned(),
        ));
    }
    Ok(())
}

fn validate_relative_path(directory: VirtualDirectory, path: &Path) -> Result<(), ProjectError> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(ProjectError::UnsafeVirtualPath {
            directory,
            path: path.to_path_buf(),
        });
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(ProjectError::UnsafeVirtualPath {
            directory,
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

fn checked_join(
    root: &Path,
    directory: VirtualDirectory,
    relative: &Path,
) -> Result<PathBuf, ProjectError> {
    validate_relative_path(directory, relative)?;
    Ok(root.join(relative))
}

fn prepare_empty_destination(root: &Path) -> Result<bool, ProjectError> {
    if !root.exists() {
        return Ok(false);
    }
    if !root.is_dir() {
        return Err(ProjectError::RootNotDirectory(root.to_path_buf()));
    }
    if fs::symlink_metadata(root)
        .map_err(|error| io_error(root, error))?
        .file_type()
        .is_symlink()
    {
        return Err(ProjectError::PathEscapesProject {
            directory: VirtualDirectory::Config,
            path: root.to_path_buf(),
        });
    }
    let descriptor = root.join(PROJECT_DESCRIPTOR_FILE);
    if descriptor.exists() {
        return Err(ProjectError::ProjectAlreadyExists(root.to_path_buf()));
    }
    let mut entries = fs::read_dir(root).map_err(|error| io_error(root, error))?;
    if entries.next().is_some() {
        return Err(ProjectError::DestinationNotEmpty(root.to_path_buf()));
    }
    Ok(true)
}

fn ensure_existing_ancestor_contained(
    root: &Path,
    directory: VirtualDirectory,
    target: &Path,
) -> Result<(), ProjectError> {
    let canonical_root = fs::canonicalize(root).map_err(|error| io_error(root, error))?;
    let mut existing = target;
    while !existing.exists() {
        existing = existing
            .parent()
            .ok_or_else(|| ProjectError::PathEscapesProject {
                directory,
                path: target.to_path_buf(),
            })?;
    }
    let canonical_existing =
        fs::canonicalize(existing).map_err(|error| io_error(existing, error))?;
    if !canonical_existing.starts_with(canonical_root) {
        return Err(ProjectError::PathEscapesProject {
            directory,
            path: target.to_path_buf(),
        });
    }
    Ok(())
}

fn write_descriptor(
    root: &Path,
    descriptor: &ProjectDescriptor,
    replace: bool,
) -> Result<(), ProjectError> {
    let descriptor_path = root.join(PROJECT_DESCRIPTOR_FILE);
    if descriptor_path.exists() && !replace {
        return Err(ProjectError::ProjectAlreadyExists(root.to_path_buf()));
    }

    let pretty = ron::ser::PrettyConfig::new()
        .depth_limit(8)
        .separate_tuple_members(true)
        .enumerate_arrays(true);
    let serialized = ron::ser::to_string_pretty(descriptor, pretty)
        .map_err(|error| ProjectError::Serialize(error.to_string()))?;
    let mut bytes = serialized.into_bytes();
    bytes.push(b'\n');
    AtomicFileService::new().write(&descriptor_path, &bytes)?;
    Ok(())
}

fn read_descriptor(path: &Path) -> Result<ProjectDescriptor, ProjectError> {
    let text = fs::read_to_string(path).map_err(|error| io_error(path, error))?;
    let descriptor: ProjectDescriptor =
        ron::from_str(&text).map_err(|error| ProjectError::InvalidDescriptor(error.to_string()))?;
    descriptor.validate()?;
    Ok(descriptor)
}

fn project_creation_staging_path(root: &Path) -> PathBuf {
    let mut name = root
        .file_name()
        .map_or_else(|| "project".into(), std::ffi::OsStr::to_os_string);
    name.push(format!(".create-{}.tmp", Uuid::new_v4()));
    root.with_file_name(name)
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_id_round_trips_as_text() {
        let id = ProjectId::new();
        assert_eq!(id.to_string().parse::<ProjectId>().unwrap(), id);
    }

    #[test]
    fn descriptor_rejects_parent_components() {
        let mut descriptor = ProjectDescriptor::new("Unsafe", ProjectTemplate::Blank);
        descriptor.directories.assets = "../outside".into();
        assert!(matches!(
            descriptor.validate(),
            Err(ProjectError::UnsafeVirtualPath {
                directory: VirtualDirectory::Assets,
                ..
            })
        ));
    }

    #[test]
    fn every_launcher_template_has_brief_defaults() {
        let templates = [
            ProjectTemplate::Empty3d,
            ProjectTemplate::Empty2d,
            ProjectTemplate::FirstPerson,
            ProjectTemplate::ThirdPerson,
            ProjectTemplate::Platformer,
            ProjectTemplate::TopDown,
            ProjectTemplate::UiApplication,
            ProjectTemplate::Blank,
        ];

        for template in templates {
            let descriptor = ProjectDescriptor::new("Template", template);
            assert!(!template.brief().is_empty());
            assert_eq!(
                descriptor.metadata.description.as_deref(),
                Some(template.brief())
            );
        }
    }

    #[test]
    fn project_name_rejects_path_and_device_names() {
        for name in [
            "../outside",
            "nested/game",
            r"nested\game",
            "CON",
            "LPT1.txt",
        ] {
            let descriptor = ProjectDescriptor::new(name, ProjectTemplate::Blank);
            assert!(
                matches!(descriptor.validate(), Err(ProjectError::InvalidName(_))),
                "unsafe project name was accepted: {name}"
            );
        }
    }
}
