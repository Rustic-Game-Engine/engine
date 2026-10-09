use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;
use thiserror::Error;

/// Validated UTF-8 virtual path in `mount://relative/path` form.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VfsPath {
    mount: String,
    relative: PathBuf,
}

impl VfsPath {
    /// Constructs a validated virtual path.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid mount name, absolute path, traversal, or platform prefix.
    pub fn new(mount: impl Into<String>, relative: impl Into<PathBuf>) -> Result<Self, VfsError> {
        let mount = mount.into();
        let relative = relative.into();
        validate_mount_name(&mount)?;
        validate_relative(&relative)?;
        Ok(Self { mount, relative })
    }

    pub fn mount(&self) -> &str {
        &self.mount
    }

    pub fn relative(&self) -> &Path {
        &self.relative
    }
}

impl fmt::Display for VfsPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}://{}",
            self.mount,
            self.relative.to_string_lossy().replace('\\', "/")
        )
    }
}

impl FromStr for VfsPath {
    type Err = VfsError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (mount, relative) = value
            .split_once("://")
            .ok_or_else(|| VfsError::InvalidSyntax(value.to_owned()))?;
        Self::new(mount, PathBuf::from(relative))
    }
}

/// One canonical filesystem mount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsMount {
    root: PathBuf,
    read_only: bool,
}

impl VfsMount {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }
}

/// Safe mount-table resolution failure.
#[derive(Debug, Error)]
pub enum VfsError {
    #[error("invalid VFS syntax: {0}")]
    InvalidSyntax(String),
    #[error("invalid VFS mount name: {0}")]
    InvalidMountName(String),
    #[error("unsafe VFS relative path: {0}")]
    UnsafeRelativePath(PathBuf),
    #[error("VFS mount `{0}` is not registered")]
    UnknownMount(String),
    #[error("VFS mount `{0}` already exists")]
    DuplicateMount(String),
    #[error("VFS mount root is not a directory: {0}")]
    InvalidMountRoot(PathBuf),
    #[error("VFS path escapes mount `{mount}`: {path}")]
    EscapesMount { mount: String, path: PathBuf },
    #[error("VFS mount `{0}` is read-only")]
    ReadOnly(String),
    #[error("VFS I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Validated mapping from logical names to canonical native directories.
#[derive(Debug, Clone, Default)]
pub struct MountTable {
    mounts: BTreeMap<String, VfsMount>,
}

impl MountTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an existing directory.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid or duplicate mount name, or when the root cannot be
    /// canonicalized.
    pub fn mount(
        &mut self,
        name: impl Into<String>,
        root: impl AsRef<Path>,
        read_only: bool,
    ) -> Result<(), VfsError> {
        let name = name.into();
        validate_mount_name(&name)?;
        if self.mounts.contains_key(&name) {
            return Err(VfsError::DuplicateMount(name));
        }
        let root = root.as_ref();
        if !root.is_dir() {
            return Err(VfsError::InvalidMountRoot(root.to_path_buf()));
        }
        let canonical = fs::canonicalize(root).map_err(|source| VfsError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        self.mounts.insert(
            name,
            VfsMount {
                root: canonical,
                read_only,
            },
        );
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&VfsMount> {
        self.mounts.get(name)
    }

    /// Resolves an existing path and checks its final canonical location.
    ///
    /// # Errors
    ///
    /// Returns an error when the mount is unknown, the path is missing, or canonical resolution
    /// escapes the mount root.
    pub fn resolve_read(&self, path: &VfsPath) -> Result<PathBuf, VfsError> {
        let mount = self
            .mounts
            .get(path.mount())
            .ok_or_else(|| VfsError::UnknownMount(path.mount().to_owned()))?;
        let joined = mount.root.join(path.relative());
        let canonical = fs::canonicalize(&joined).map_err(|source| VfsError::Io {
            path: joined.clone(),
            source,
        })?;
        ensure_contained(path.mount(), &mount.root, &canonical)?;
        Ok(native_display_path(&canonical))
    }

    /// Resolves a prospective write through its deepest existing ancestor.
    ///
    /// # Errors
    ///
    /// Returns an error for a read-only or unknown mount, or when an existing ancestor resolves
    /// outside the mount root.
    pub fn resolve_write(&self, path: &VfsPath) -> Result<PathBuf, VfsError> {
        let mount = self
            .mounts
            .get(path.mount())
            .ok_or_else(|| VfsError::UnknownMount(path.mount().to_owned()))?;
        if mount.read_only {
            return Err(VfsError::ReadOnly(path.mount().to_owned()));
        }
        let joined = mount.root.join(path.relative());
        let mut ancestor = joined.as_path();
        while !ancestor.exists() {
            ancestor = ancestor.parent().ok_or_else(|| VfsError::EscapesMount {
                mount: path.mount().to_owned(),
                path: joined.clone(),
            })?;
        }
        let canonical = fs::canonicalize(ancestor).map_err(|source| VfsError::Io {
            path: ancestor.to_path_buf(),
            source,
        })?;
        ensure_contained(path.mount(), &mount.root, &canonical)?;
        Ok(native_display_path(&joined))
    }
}

#[cfg(windows)]
fn native_display_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\")
        .map_or_else(|| path.to_path_buf(), PathBuf::from)
}

#[cfg(not(windows))]
fn native_display_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

fn validate_mount_name(name: &str) -> Result<(), VfsError> {
    if name.is_empty()
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(VfsError::InvalidMountName(name.to_owned()));
    }
    Ok(())
}

fn validate_relative(path: &Path) -> Result<(), VfsError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(VfsError::UnsafeRelativePath(path.to_path_buf()));
    }
    Ok(())
}

fn ensure_contained(mount: &str, root: &Path, path: &Path) -> Result<(), VfsError> {
    if !path.starts_with(root) {
        return Err(VfsError::EscapesMount {
            mount: mount.to_owned(),
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn traversal_and_absolute_paths_are_rejected() {
        for value in ["project://../outside", "project:///absolute"] {
            assert!(value.parse::<VfsPath>().is_err(), "accepted {value}");
        }
    }

    #[test]
    fn read_only_mount_rejects_writes() {
        let directory = tempdir().unwrap();
        let mut mounts = MountTable::new();
        mounts.mount("package", directory.path(), true).unwrap();
        let path = VfsPath::new("package", "asset.bin").unwrap();
        assert!(matches!(
            mounts.resolve_write(&path),
            Err(VfsError::ReadOnly(name)) if name == "package"
        ));
    }

    #[test]
    fn nested_write_resolves_beneath_mount() {
        let directory = tempdir().unwrap();
        let mut mounts = MountTable::new();
        mounts.mount("project", directory.path(), false).unwrap();
        let path = VfsPath::new("project", "assets/new/item.bin").unwrap();
        let resolved = mounts.resolve_write(&path).unwrap();
        fs::create_dir_all(resolved.parent().unwrap()).unwrap();
        fs::write(&resolved, b"asset").unwrap();
        assert_eq!(
            resolved.canonicalize().unwrap(),
            directory
                .path()
                .canonicalize()
                .unwrap()
                .join("assets/new/item.bin")
        );
        assert_eq!(mounts.resolve_read(&path).unwrap(), resolved);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_rejected() {
        use std::os::unix::fs::symlink;
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        symlink(outside.path(), root.path().join("link")).unwrap();
        fs::write(outside.path().join("secret"), "x").unwrap();
        let mut mounts = MountTable::new();
        mounts.mount("project", root.path(), false).unwrap();
        let path = VfsPath::new("project", "link/secret").unwrap();
        assert!(matches!(
            mounts.resolve_read(&path),
            Err(VfsError::EscapesMount { .. })
        ));
    }
}
