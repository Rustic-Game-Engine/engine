use crate::{AtomicFileService, AtomicWriteError};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;

/// Version envelope used by per-user engine configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct ConfigDocument<T> {
    /// Schema generation of `data`.
    pub format_version: u32,
    /// Schema-owned configuration value.
    pub data: T,
}

/// Contract implemented by each versioned configuration schema.
pub trait ConfigSchema: Sized + Serialize + DeserializeOwned {
    /// Current schema generation.
    const FORMAT_VERSION: u32;

    /// Migrates an older RON value. Implementations must reject unknown generations.
    ///
    /// # Errors
    ///
    /// Returns an explanation when `from_version` is unsupported or `value` is invalid.
    fn migrate(from_version: u32, _value: ron::Value) -> Result<Self, String> {
        Err(format!(
            "configuration version {from_version} has no migration to {}",
            Self::FORMAT_VERSION
        ))
    }
}

/// How a configuration value was obtained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigLoad<T> {
    /// No config exists yet.
    Missing,
    /// Current schema parsed normally.
    Current(T),
    /// An older schema was migrated in memory and should be saved.
    Migrated { from_version: u32, value: T },
    /// Corrupt current bytes were quarantined and the retained backup restored.
    RecoveredBackup { value: T, quarantine: PathBuf },
}

impl<T> ConfigLoad<T> {
    /// Extracts the value, using `default` only for the missing state.
    pub fn value_or_else(self, default: impl FnOnce() -> T) -> T {
        match self {
            Self::Missing => default(),
            Self::Current(value)
            | Self::Migrated { value, .. }
            | Self::RecoveredBackup { value, .. } => value,
        }
    }
}

/// Configuration persistence failure.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error(transparent)]
    Atomic(#[from] AtomicWriteError),
    #[error("configuration I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("configuration at {path} exceeds the {MAX_CONFIG_BYTES}-byte limit")]
    TooLarge { path: PathBuf },
    #[error("configuration at {path} is corrupt: {message}")]
    Corrupt { path: PathBuf, message: String },
    #[error("configuration version {found} is newer than supported version {supported}")]
    FutureVersion { found: u32, supported: u32 },
    #[error("configuration migration from version {from_version} failed: {message}")]
    Migration { from_version: u32, message: String },
    #[error("configuration serialization failed: {0}")]
    Serialize(String),
}

/// Transactional store for one or more configuration schema types.
#[derive(Debug, Clone, Default)]
pub struct ConfigStore {
    files: AtomicFileService,
}

impl ConfigStore {
    pub fn new(files: AtomicFileService) -> Self {
        Self { files }
    }

    /// Loads, migrates, or recovers a versioned config without accepting future data.
    ///
    /// # Errors
    ///
    /// Returns an error for I/O failures, malformed or oversized data, unsupported versions, or a
    /// failed migration.
    pub fn load<T: ConfigSchema>(&self, path: &Path) -> Result<ConfigLoad<T>, ConfigError> {
        self.files.recover(path)?;
        if !path.exists() {
            return Ok(ConfigLoad::Missing);
        }
        match read_and_parse::<T>(path) {
            Ok(value) => Ok(value),
            Err(error @ ConfigError::FutureVersion { .. }) => Err(error),
            Err(primary_error) => {
                let backup = AtomicFileService::backup_path(path);
                let backup_value = if backup.is_file() {
                    read_and_parse::<T>(&backup).ok()
                } else {
                    None
                };
                let Some(backup_value) = backup_value else {
                    return Err(primary_error);
                };
                let quarantine = quarantine_path(path);
                fs::rename(path, &quarantine).map_err(|source| ConfigError::Io {
                    path: path.to_path_buf(),
                    source,
                })?;
                if !self.files.restore_backup(path)? {
                    return Err(ConfigError::Corrupt {
                        path: path.to_path_buf(),
                        message: "valid backup disappeared during recovery".to_owned(),
                    });
                }
                Ok(ConfigLoad::RecoveredBackup {
                    value: backup_value.value_or_else(|| unreachable!()),
                    quarantine,
                })
            }
        }
    }

    /// Saves a current-version document transactionally and retains the prior bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization or the transactional write fails.
    pub fn save<T: ConfigSchema>(&self, path: &Path, value: &T) -> Result<(), ConfigError> {
        let document = ConfigDocument {
            format_version: T::FORMAT_VERSION,
            data: value,
        };
        let mut serialized = ron::ser::to_string_pretty(
            &document,
            ron::ser::PrettyConfig::new()
                .depth_limit(16)
                .separate_tuple_members(true),
        )
        .map_err(|error| ConfigError::Serialize(error.to_string()))?;
        serialized.push('\n');
        self.files.write(path, serialized.as_bytes())?;
        Ok(())
    }
}

fn read_and_parse<T: ConfigSchema>(path: &Path) -> Result<ConfigLoad<T>, ConfigError> {
    let metadata = fs::metadata(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err(ConfigError::TooLarge {
            path: path.to_path_buf(),
        });
    }
    let text = fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let raw: ConfigDocument<ron::Value> =
        ron::from_str(&text).map_err(|error| ConfigError::Corrupt {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    if raw.format_version > T::FORMAT_VERSION {
        return Err(ConfigError::FutureVersion {
            found: raw.format_version,
            supported: T::FORMAT_VERSION,
        });
    }
    if raw.format_version == T::FORMAT_VERSION {
        let parsed: ConfigDocument<T> =
            ron::from_str(&text).map_err(|error| ConfigError::Corrupt {
                path: path.to_path_buf(),
                message: error.to_string(),
            })?;
        return Ok(ConfigLoad::Current(parsed.data));
    }
    let from_version = raw.format_version;
    let value = T::migrate(from_version, raw.data).map_err(|message| ConfigError::Migration {
        from_version,
        message,
    })?;
    Ok(ConfigLoad::Migrated {
        from_version,
        value,
    })
}

fn quarantine_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map_or_else(|| "config".into(), std::ffi::OsStr::to_os_string);
    name.push(format!(".corrupt-{}", Uuid::new_v4()));
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct Settings {
        name: String,
        count: u32,
    }

    impl ConfigSchema for Settings {
        const FORMAT_VERSION: u32 = 2;

        fn migrate(from_version: u32, value: ron::Value) -> Result<Self, String> {
            #[derive(Deserialize)]
            struct Old {
                name: String,
            }

            if from_version != 1 {
                return Err("unsupported fixture version".to_owned());
            }
            let old: Old = value.into_rust().map_err(|error| error.to_string())?;
            Ok(Self {
                name: old.name,
                count: 0,
            })
        }
    }

    #[test]
    fn current_config_round_trips() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("settings.ron");
        let store = ConfigStore::default();
        let settings = Settings {
            name: "Rustic".to_owned(),
            count: 3,
        };
        store.save(&path, &settings).unwrap();
        assert_eq!(store.load(&path).unwrap(), ConfigLoad::Current(settings));
    }

    #[test]
    fn older_config_migrates_and_future_config_is_rejected() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("settings.ron");
        fs::write(&path, "(format_version:1,data:(name:\"old\"))").unwrap();
        assert!(matches!(
            ConfigStore::default().load::<Settings>(&path).unwrap(),
            ConfigLoad::Migrated {
                from_version: 1,
                ..
            }
        ));
        fs::write(&path, "(format_version:99,data:(name:\"future\"))").unwrap();
        assert!(matches!(
            ConfigStore::default().load::<Settings>(&path),
            Err(ConfigError::FutureVersion { found: 99, .. })
        ));
    }

    #[test]
    fn corrupt_current_config_restores_valid_backup() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("settings.ron");
        let store = ConfigStore::default();
        let first = Settings {
            name: "first".to_owned(),
            count: 1,
        };
        store.save(&path, &first).unwrap();
        store
            .save(
                &path,
                &Settings {
                    name: "second".to_owned(),
                    count: 2,
                },
            )
            .unwrap();
        fs::write(&path, "not ron").unwrap();
        let ConfigLoad::RecoveredBackup { value, quarantine } =
            store.load::<Settings>(&path).unwrap()
        else {
            panic!("expected backup recovery");
        };
        assert_eq!(value, first);
        assert!(quarantine.is_file());
    }
}
