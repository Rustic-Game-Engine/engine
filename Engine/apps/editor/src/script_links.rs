use engine_platform::AtomicFileService;
use engine_scripting::{
    GameSettings, ScriptId, ScriptManifest, ScriptManifestEntry, load_manifest,
    save_manifest_atomic,
};
use engine_world::{SceneDocument, load_scene, save_scene_atomic};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub fn script_label(entries: &[ScriptManifestEntry], id: ScriptId) -> String {
    entries.iter().find(|entry| entry.id == id).map_or_else(
        || format!("Missing script ({id})"),
        |entry| entry.relative_path.display().to_string(),
    )
}

enum UpdatedLinks {
    Manifest(ScriptManifest),
    Settings(GameSettings),
    Scene(SceneDocument),
}

struct LinkFile {
    path: PathBuf,
    original: Vec<u8>,
    updated: UpdatedLinks,
}

impl LinkFile {
    fn prepare(path: PathBuf, updated: UpdatedLinks) -> Result<Self, String> {
        let original = std::fs::read(&path).map_err(|error| error.to_string())?;
        Ok(Self {
            path,
            original,
            updated,
        })
    }

    fn save(&self) -> Result<(), String> {
        match &self.updated {
            UpdatedLinks::Manifest(manifest) => {
                save_manifest_atomic(&self.path, manifest).map_err(|error| error.to_string())
            }
            UpdatedLinks::Settings(settings) => settings.save(self.path.parent().unwrap()),
            UpdatedLinks::Scene(scene) => {
                save_scene_atomic(&self.path, scene).map_err(|error| error.to_string())
            }
        }
    }

    fn restore(&self) -> Result<(), String> {
        // A failed writer can leave its original file untouched, including when
        // a blocked temporary path caused the failure. Do not rewrite that file.
        if std::fs::read(&self.path).is_ok_and(|bytes| bytes == self.original) {
            return Ok(());
        }
        AtomicFileService::new()
            .write(&self.path, &self.original)
            .map_err(|error| error.to_string())
    }
}

/// Prepare every affected file before moving the source into trash. Persisted
/// scenes are updated separately from the live document so unsaved edits survive.
#[derive(Default)]
pub struct ScriptLinkDeletion {
    pub deleted_ids: BTreeSet<ScriptId>,
    files: Vec<LinkFile>,
}

impl ScriptLinkDeletion {
    pub fn prepare(root: &Path, source: &Path) -> Result<Self, String> {
        let manifest_path = root.join("config/scripts.ron");
        if !manifest_path.exists() {
            return Ok(Self::default());
        }
        super::checked_project_path(root, Path::new("config/scripts.ron"), false)?;
        let mut manifest =
            load_manifest(&std::fs::read(&manifest_path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?
                .manifest;
        let deleted_ids: BTreeSet<_> = manifest
            .scripts
            .iter()
            .filter(|entry| entry.relative_path.starts_with(source))
            .map(|entry| entry.id)
            .collect();
        if deleted_ids.is_empty() {
            return Ok(Self::default());
        }
        if Path::new("config/scripts.ron").starts_with(source) {
            return Err(
                "Cannot delete a folder containing the script registry and registered scripts"
                    .into(),
            );
        }
        manifest
            .scripts
            .retain(|entry| !deleted_ids.contains(&entry.id));
        let mut files = vec![LinkFile::prepare(
            manifest_path,
            UpdatedLinks::Manifest(manifest),
        )?];
        let settings_path = root.join(engine_scripting::GAME_SETTINGS_FILE);
        if settings_path.exists()
            && !Path::new(engine_scripting::GAME_SETTINGS_FILE).starts_with(source)
        {
            super::checked_project_path(
                root,
                Path::new(engine_scripting::GAME_SETTINGS_FILE),
                false,
            )?;
            let mut settings = GameSettings::load(root)?;
            let before = settings.global_scripts.len();
            settings
                .global_scripts
                .retain(|script| !deleted_ids.contains(&script.asset_id));
            if before != settings.global_scripts.len() {
                files.push(LinkFile::prepare(
                    settings_path,
                    UpdatedLinks::Settings(settings),
                )?);
            }
        }
        let mut scenes = Vec::new();
        collect_scenes(root, root, &root.join(source), &mut scenes)?;
        scenes.sort();
        for path in scenes {
            let loaded = load_scene(&std::fs::read(&path).map_err(|error| error.to_string())?)
                .map_err(|error| format!("Could not update {}: {error}", path.display()))?;
            if loaded.read_only {
                return Err(format!(
                    "Cannot update script links in read-only or corrupt scene {}",
                    path.display()
                ));
            }
            let mut scene = loaded.document;
            let before = scene.startup_scripts.len();
            scene
                .startup_scripts
                .retain(|script| !deleted_ids.contains(&script.asset_id));
            let mut changed = before != scene.startup_scripts.len();
            for entity in &mut scene.entities {
                let before = entity.scripts.len();
                entity
                    .scripts
                    .retain(|script| !deleted_ids.contains(&script.script_id));
                changed |= before != entity.scripts.len();
            }
            if changed {
                files.push(LinkFile::prepare(path, UpdatedLinks::Scene(scene))?);
            }
        }
        Ok(Self { deleted_ids, files })
    }

    pub fn save(&self) -> Result<(), String> {
        for (index, file) in self.files.iter().enumerate() {
            if let Err(error) = file.save() {
                let mut errors = vec![error];
                // Include the failed write in case it installed data before
                // reporting a durability error. Restore in reverse write order.
                for previous in self.files[..=index].iter().rev() {
                    if let Err(rollback) = previous.restore() {
                        errors.push(format!(
                            "could not restore {}: {rollback}",
                            previous.path.display()
                        ));
                    }
                }
                return Err(errors.join("; "));
            }
        }
        Ok(())
    }
}

fn collect_scenes(
    root: &Path,
    directory: &Path,
    deleted: &Path,
    scenes: &mut Vec<PathBuf>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if path.starts_with(deleted) || kind.is_symlink() {
            continue;
        }
        if kind.is_dir() && !super::is_hidden_project_directory(&entry.file_name()) {
            collect_scenes(root, &path, deleted, scenes)?;
        } else if kind.is_file()
            && path.extension().is_some_and(|extension| {
                extension.eq_ignore_ascii_case("scene") || extension.eq_ignore_ascii_case("rscene")
            })
        {
            super::checked_project_path(
                root,
                path.strip_prefix(root).map_err(|error| error.to_string())?,
                false,
            )?;
            scenes.push(path);
        }
    }
    Ok(())
}
