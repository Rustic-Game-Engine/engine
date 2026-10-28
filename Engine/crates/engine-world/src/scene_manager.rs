//! Explicit ownership of loaded scene worlds; unloaded scenes have no cache entry.

use crate::{SceneDocument, SceneError, SceneId, SceneWorld, WorldError, load_scene};
use std::{collections::BTreeMap, path::Path};
use thiserror::Error;

/// Errors leave both the loaded set and the active scene unchanged.
#[derive(Debug, Error)]
pub enum SceneManagerError {
    #[error("scene {0} is already loaded")]
    AlreadyLoaded(SceneId),
    #[error("scene {0} is not loaded; load it before switching")]
    NotLoaded(SceneId),
    #[error("scene is damaged or uses an incompatible schema")]
    InvalidScene,
    #[error(transparent)]
    Scene(#[from] SceneError),
    #[error(transparent)]
    World(#[from] WorldError),
}

/// Owns only explicitly loaded worlds. Loading does not activate a scene.
///
/// Switching preserves all loaded worlds and their edits. Unloading drops the
/// world's entities and components immediately and clears the active selection
/// if necessary. No document, serialized bytes, or unloaded-world cache is kept.
/// Hosts decide when to tick/render the active world and manage script, audio,
/// physics and renderer resources separately; this is the Rust world API.
#[derive(Default)]
pub struct SceneManager {
    loaded: BTreeMap<SceneId, SceneWorld>,
    active: Option<SceneId>,
}

impl SceneManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Consumes a document and builds its live world without retaining a snapshot.
    ///
    /// # Errors
    /// Rejects duplicate IDs or invalid world data without changing existing worlds.
    pub fn load(&mut self, document: SceneDocument) -> Result<SceneId, SceneManagerError> {
        let id = document.id;
        if self.is_loaded(id) {
            return Err(SceneManagerError::AlreadyLoaded(id));
        }
        let world = document.create_world()?;
        drop(document);
        self.loaded.insert(id, world);
        Ok(id)
    }

    /// Reads a scene only on explicit load. Temporary file bytes are then dropped.
    ///
    /// # Errors
    /// Rejects I/O, parse, checksum, compatibility, world and duplicate-ID errors.
    /// Runtime loading never silently recovers stale data from a backup file.
    pub fn load_file(&mut self, path: impl AsRef<Path>) -> Result<SceneId, SceneManagerError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| SceneError::Io {
            path: path.to_owned(),
            source,
        })?;
        let scene = load_scene(&bytes)?;
        if scene.read_only || scene.has_errors() {
            return Err(SceneManagerError::InvalidScene);
        }
        self.load(scene.document)
    }

    /// Selects a loaded scene without implicitly loading anything.
    ///
    /// # Errors
    /// Returns [`SceneManagerError::NotLoaded`] if the scene is unloaded or unknown.
    pub fn switch(&mut self, id: SceneId) -> Result<(), SceneManagerError> {
        if !self.is_loaded(id) {
            return Err(SceneManagerError::NotLoaded(id));
        }
        self.active = Some(id);
        Ok(())
    }

    /// Drops a loaded world, discarding unsaved changes. Does not pick a fallback.
    ///
    /// # Errors
    /// Returns [`SceneManagerError::NotLoaded`] if no world is owned for this ID.
    pub fn unload(&mut self, id: SceneId) -> Result<(), SceneManagerError> {
        self.loaded
            .remove(&id)
            .ok_or(SceneManagerError::NotLoaded(id))?;
        if self.active == Some(id) {
            self.active = None;
        }
        Ok(())
    }

    pub fn is_loaded(&self, id: SceneId) -> bool {
        self.loaded.contains_key(&id)
    }

    pub fn loaded_ids(&self) -> impl Iterator<Item = SceneId> + '_ {
        self.loaded.keys().copied()
    }

    pub fn active_id(&self) -> Option<SceneId> {
        self.active
    }

    pub fn world(&self, id: SceneId) -> Option<&SceneWorld> {
        self.loaded.get(&id)
    }

    pub fn world_mut(&mut self, id: SceneId) -> Option<&mut SceneWorld> {
        self.loaded.get_mut(&id)
    }

    pub fn active_world(&self) -> Option<&SceneWorld> {
        self.active.and_then(|id| self.world(id))
    }

    pub fn active_world_mut(&mut self) -> Option<&mut SceneWorld> {
        self.active.and_then(|id| self.loaded.get_mut(&id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntitySnapshot, WorldCommand, save_scene_atomic};

    #[test]
    fn invalid_world_and_duplicate_load_preserve_existing_state() {
        let mut scenes = SceneManager::new();
        let document = SceneDocument::new("Active");
        let id = scenes.load(document.clone()).unwrap();
        scenes.switch(id).unwrap();
        assert!(matches!(
            scenes.load(document),
            Err(SceneManagerError::AlreadyLoaded(_))
        ));
        let mut invalid = SceneDocument::new("Invalid");
        let mut entity = EntitySnapshot::default();
        entity.parent = Some(entity.id);
        invalid.entities.push(entity);
        let invalid_id = invalid.id;
        assert!(matches!(
            scenes.load(invalid),
            Err(SceneManagerError::World(_))
        ));
        assert!(!scenes.is_loaded(invalid_id));
        assert_eq!(scenes.loaded_ids().collect::<Vec<_>>(), vec![id]);
        assert_eq!(scenes.active_id(), Some(id));
    }

    #[test]
    fn switching_preserves_edits_and_unload_removes_the_world() {
        let mut scenes = SceneManager::new();
        let first = scenes.load(SceneDocument::new("First")).unwrap();
        let second = scenes.load(SceneDocument::new("Second")).unwrap();
        assert_eq!(scenes.active_id(), None);
        scenes.switch(first).unwrap();
        let entity = EntitySnapshot::default();
        let entity_id = entity.id;
        scenes
            .active_world_mut()
            .unwrap()
            .apply_commands(&[WorldCommand::Spawn(Box::new(entity))])
            .unwrap();
        scenes.switch(second).unwrap();
        scenes.switch(first).unwrap();
        assert!(scenes.active_world().unwrap().contains(entity_id));
        scenes.unload(second).unwrap();
        assert_eq!(scenes.active_id(), Some(first));
        assert!(
            matches!(scenes.switch(second), Err(SceneManagerError::NotLoaded(id)) if id == second)
        );
        assert_eq!(scenes.active_id(), Some(first));
        scenes.unload(first).unwrap();
        assert!(scenes.world(first).is_none());
        assert!(scenes.active_world().is_none());
        assert_eq!(scenes.active_id(), None);
        assert_eq!(scenes.loaded_ids().count(), 0);
        assert!(scenes.unload(first).is_err());
    }

    #[test]
    fn file_load_is_explicit_atomic_and_reloads_from_disk() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("level.rscene");
        let mut document = SceneDocument::new("Level");
        let id = document.id;
        save_scene_atomic(&path, &document).unwrap();
        let mut scenes = SceneManager::new();
        assert!(scenes.switch(id).is_err());
        scenes.load_file(&path).unwrap();
        scenes.switch(id).unwrap();
        assert!(matches!(
            scenes.load_file(&path),
            Err(SceneManagerError::AlreadyLoaded(_))
        ));
        std::fs::write(&path, b"broken").unwrap();
        assert!(scenes.load_file(&path).is_err());
        assert_eq!(scenes.active_id(), Some(id));
        scenes.unload(id).unwrap();
        document.entities.push(EntitySnapshot::default());
        save_scene_atomic(&path, &document).unwrap();
        scenes.load_file(&path).unwrap();
        assert!(scenes.world(id).unwrap().contains(document.entities[0].id));
        assert_eq!(scenes.active_id(), None);
        let edited = String::from_utf8(document.to_bytes().unwrap())
            .unwrap()
            .replace("\"Level\"", "\"Changed\"");
        scenes.unload(id).unwrap();
        std::fs::write(&path, edited).unwrap();
        assert!(matches!(
            scenes.load_file(&path),
            Err(SceneManagerError::InvalidScene)
        ));
        assert_eq!(scenes.loaded_ids().count(), 0);
    }
}
