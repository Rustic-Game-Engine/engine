use engine_core::{EntityId, SceneId};
use engine_play::{
    ApplyRuntimeChangesTransaction, MapChangeStore, RuntimeChangeSet, RuntimeChangeTarget,
    RuntimeValue,
};
use engine_project::{Project, ProjectError, VirtualDirectory};
use engine_world::{
    EntitySnapshot, LocalTransform, PartAttributes, Primitive, RecoverySource, SceneDocument,
    SceneEdit, SceneError, SceneWorld, ScriptComponent, ScriptReference, UndoStack, WorldError,
    load_scene_recovering, save_scene_atomic,
};
use num_traits::ToPrimitive as _;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthoringError {
    #[error(transparent)]
    Project(#[from] ProjectError),
    #[error(transparent)]
    Scene(#[from] SceneError),
    #[error(transparent)]
    World(#[from] WorldError),
    #[error("authoring document is read-only")]
    ReadOnly,
    #[error("runtime changes could not be applied: {0}")]
    RuntimeChanges(String),
    #[error("scene I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("scene insertion path must stay inside the project scenes folder")]
    UnsafeScenePath,
}

/// One source scene and its centralized, checkpoint-aware edit history.
#[derive(Debug)]
pub struct AuthoringDocument {
    project: Project,
    scene_path: PathBuf,
    scene_id: SceneId,
    scene_name: String,
    scene_startup_scripts: Vec<ScriptReference>,
    world: SceneWorld,
    undo: UndoStack,
    selected: Option<EntityId>,
    read_only: bool,
    recovered_from_backup: bool,
}

impl AuthoringDocument {
    /// Opens the project's main scene, including backup recovery when required.
    ///
    /// # Errors
    ///
    /// Returns an error when project paths, scene recovery, migration, or world construction fails.
    pub fn open(project: Project, read_only: bool) -> Result<Self, AuthoringError> {
        let scene_directory = project.directory(VirtualDirectory::Scenes)?;
        // New projects use scene/{name}.scene. Keep opening the original
        // scenes/main.rscene location so existing projects need no migration.
        let preferred = scene_directory.join("main.scene");
        let legacy = scene_directory.join("main.rscene");
        let scene_path = if preferred.is_file() || !legacy.is_file() {
            preferred
        } else {
            legacy
        };
        let (document, recovery_read_only, recovered_from_backup) = if scene_path.is_file() {
            let loaded = load_scene_recovering(&scene_path)?;
            (
                loaded.document,
                loaded.read_only,
                loaded.source == RecoverySource::Backup,
            )
        } else {
            (SceneDocument::new("Main"), false, false)
        };
        let world = document.create_world()?;
        let mut undo = UndoStack::new();
        undo.mark_saved();
        Ok(Self {
            project,
            scene_path,
            scene_id: document.id,
            scene_name: document.name,
            scene_startup_scripts: document.startup_scripts,
            world,
            undo,
            selected: None,
            read_only: read_only || recovery_read_only,
            recovered_from_backup,
        })
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Returns the project for metadata edits when the document is writable.
    pub fn project_mut(&mut self) -> Result<&mut Project, AuthoringError> {
        if self.read_only {
            return Err(AuthoringError::ReadOnly);
        }
        Ok(&mut self.project)
    }

    pub fn scene_path(&self) -> &Path {
        &self.scene_path
    }

    pub fn world(&self) -> &SceneWorld {
        &self.world
    }

    pub fn selected(&self) -> Option<EntityId> {
        self.selected
    }

    pub fn scene_startup_scripts(&self) -> &[ScriptReference] {
        &self.scene_startup_scripts
    }

    pub fn set_scene_startup_scripts(
        &mut self,
        scripts: Vec<ScriptReference>,
    ) -> Result<(), AuthoringError> {
        self.ensure_writable()?;
        self.scene_startup_scripts = scripts;
        Ok(())
    }

    pub fn select(&mut self, entity: Option<EntityId>) {
        self.selected = entity.filter(|value| self.world.contains(*value));
    }

    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub const fn recovered_from_backup(&self) -> bool {
        self.recovered_from_backup
    }

    pub fn is_modified(&self) -> bool {
        self.undo.is_modified()
    }

    /// Returns stable snapshots for all entities.
    ///
    /// # Errors
    ///
    /// Returns an error when an entity cannot be snapshotted consistently.
    pub fn entity_snapshots(&self) -> Result<Vec<EntitySnapshot>, AuthoringError> {
        self.world
            .entity_ids()
            .map(|id| self.world.snapshot(id).map_err(AuthoringError::from))
            .collect()
    }

    /// Creates a primitive through the centralized undo stack.
    ///
    /// # Errors
    ///
    /// Returns an error for a read-only document, invalid primitive, or failed world edit.
    pub fn add_primitive(
        &mut self,
        name: impl Into<String>,
        primitive: Primitive,
        parent: Option<EntityId>,
    ) -> Result<EntityId, AuthoringError> {
        self.ensure_writable()?;
        primitive
            .validate()
            .map_err(|error| WorldError::InvalidPrimitive {
                entity: EntityId::new(),
                message: error.to_string(),
            })?;
        let snapshot = EntitySnapshot {
            name: Some(name.into()),
            primitive: Some(primitive),
            parent,
            ..EntitySnapshot::default()
        };
        let id = snapshot.id;
        self.undo
            .execute(&mut self.world, SceneEdit::Create { snapshot })?;
        self.selected = Some(id);
        Ok(id)
    }

    /// Updates one local transform through the centralized undo stack.
    ///
    /// # Errors
    ///
    /// Returns an error for a read-only document, unknown entity, or invalid edit.
    pub fn set_transform(
        &mut self,
        entity: EntityId,
        after: LocalTransform,
    ) -> Result<(), AuthoringError> {
        self.ensure_writable()?;
        let before = self.world.local_transform(entity)?;
        self.undo.execute(
            &mut self.world,
            SceneEdit::Transform {
                entity,
                before,
                after,
            },
        )?;
        Ok(())
    }

    /// Applies a transient transform during an interactive drag without adding history.
    /// The caller must finish with [`Self::commit_transform_drag`] or restore `before`.
    ///
    /// # Errors
    ///
    /// Returns an error for read-only state, an unknown entity, or an invalid transform.
    pub fn preview_transform(
        &mut self,
        entity: EntityId,
        after: LocalTransform,
    ) -> Result<(), AuthoringError> {
        self.ensure_writable()?;
        self.world
            .apply_commands(&[engine_world::WorldCommand::SetLocalTransform {
                entity,
                value: after,
            }])?;
        self.world.propagate_transforms();
        Ok(())
    }

    /// Records one complete interactive transform drag as exactly one undo entry.
    ///
    /// # Errors
    ///
    /// Returns an error for read-only state, an unknown entity, or an invalid transaction.
    pub fn commit_transform_drag(
        &mut self,
        entity: EntityId,
        before: LocalTransform,
        after: LocalTransform,
    ) -> Result<(), AuthoringError> {
        self.ensure_writable()?;
        if before == after {
            return Ok(());
        }
        self.undo.execute(
            &mut self.world,
            SceneEdit::Transaction {
                label: "Transform gizmo drag".to_owned(),
                edits: vec![SceneEdit::Transform {
                    entity,
                    before,
                    after,
                }],
            },
        )?;
        Ok(())
    }

    /// Updates one primitive through the centralized undo stack.
    ///
    /// # Errors
    ///
    /// Returns an error for a read-only document, unknown entity, or invalid primitive edit.
    pub fn set_primitive(
        &mut self,
        entity: EntityId,
        after: Primitive,
    ) -> Result<(), AuthoringError> {
        self.ensure_writable()?;
        let before = self.world.snapshot(entity)?.primitive;
        self.undo.execute(
            &mut self.world,
            SceneEdit::Primitive {
                entity,
                before,
                after: Some(after),
            },
        )?;
        Ok(())
    }

    pub fn add_camera_or_light(
        &mut self,
        camera: bool,
        transform: LocalTransform,
        parent: Option<EntityId>,
    ) -> Result<EntityId, AuthoringError> {
        self.ensure_writable()?;
        let snapshot = EntitySnapshot {
            name: Some(if camera { "Camera" } else { "Light" }.into()),
            local_transform: transform,
            camera: camera.then(engine_world::Camera::default),
            light: (!camera).then(engine_world::Light::default),
            parent,
            ..EntitySnapshot::default()
        };
        let id = snapshot.id;
        self.undo
            .execute(&mut self.world, SceneEdit::Create { snapshot })?;
        self.selected = Some(id);
        Ok(id)
    }

    /// Creates an organizational folder in the scene hierarchy.
    pub fn add_folder(&mut self, parent: Option<EntityId>) -> Result<EntityId, AuthoringError> {
        self.ensure_writable()?;
        let snapshot = EntitySnapshot {
            name: Some("New Folder".into()),
            folder: true,
            parent,
            ..EntitySnapshot::default()
        };
        let id = snapshot.id;
        self.undo
            .execute(&mut self.world, SceneEdit::Create { snapshot })?;
        self.selected = Some(id);
        Ok(id)
    }

    pub fn set_camera(
        &mut self,
        entity: EntityId,
        after: engine_world::Camera,
    ) -> Result<(), AuthoringError> {
        self.ensure_writable()?;
        let before = self.world.camera(entity)?;
        self.undo.execute(
            &mut self.world,
            SceneEdit::Camera {
                entity,
                before,
                after: Some(after),
            },
        )?;
        Ok(())
    }

    pub fn set_light(
        &mut self,
        entity: EntityId,
        after: engine_world::Light,
    ) -> Result<(), AuthoringError> {
        self.ensure_writable()?;
        let before = self.world.light(entity)?;
        self.undo.execute(
            &mut self.world,
            SceneEdit::Light {
                entity,
                before,
                after: Some(after),
            },
        )?;
        Ok(())
    }

    pub fn set_part_attributes(
        &mut self,
        entity: EntityId,
        after: PartAttributes,
    ) -> Result<(), AuthoringError> {
        self.ensure_writable()?;
        let before = self.world.part_attributes(entity)?;
        if before != after {
            self.undo.execute(
                &mut self.world,
                SceneEdit::PartAttributes {
                    entity,
                    before,
                    after,
                },
            )?;
        }
        Ok(())
    }

    pub fn set_name(&mut self, entity: EntityId, after: String) -> Result<(), AuthoringError> {
        self.ensure_writable()?;
        let before = self.world.snapshot(entity)?.name;
        self.undo.execute(
            &mut self.world,
            SceneEdit::Transaction {
                label: "Rename entity".into(),
                edits: vec![SceneEdit::Name {
                    entity,
                    before,
                    after: Some(after),
                }],
            },
        )?;
        Ok(())
    }

    /// Inserts a `.rscene` from the project's `scenes` folder as an additive instance.
    pub fn insert_scene(
        &mut self,
        relative_path: impl AsRef<Path>,
        parent: Option<EntityId>,
    ) -> Result<Vec<EntityId>, AuthoringError> {
        self.ensure_writable()?;
        let relative_path = relative_path.as_ref();
        if relative_path.is_absolute()
            || relative_path.components().any(|part| {
                matches!(
                    part,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            return Err(AuthoringError::UnsafeScenePath);
        }
        let scenes = self.project.directory(VirtualDirectory::Scenes)?;
        let path = scenes.join(relative_path);
        let loaded = load_scene_recovering(&path)?;
        let instance = self.world.instantiate_scene(
            &loaded.document,
            engine_world::SceneInstanceId::new(),
            parent,
        )?;
        Ok(instance.entity_map.values().copied().collect())
    }

    /// Replaces the ordered script list as one undoable authoring operation.
    ///
    /// # Errors
    /// Returns an error for read-only access, a missing entity, or invalid script data.
    pub fn set_scripts(
        &mut self,
        entity: EntityId,
        after: Vec<ScriptComponent>,
    ) -> Result<(), AuthoringError> {
        self.ensure_writable()?;
        let before = self.world.snapshot(entity)?.scripts;
        self.undo.execute(
            &mut self.world,
            SceneEdit::Scripts {
                entity,
                before,
                after,
            },
        )?;
        Ok(())
    }

    /// Reverts the most recent edit.
    ///
    /// # Errors
    ///
    /// Returns an error when the document is read-only or the inverse world edit fails.
    pub fn undo(&mut self) -> Result<bool, AuthoringError> {
        self.ensure_writable()?;
        self.undo.undo(&mut self.world).map_err(Into::into)
    }

    /// Reapplies the most recently reverted edit.
    ///
    /// # Errors
    ///
    /// Returns an error when the document is read-only or the world edit fails.
    pub fn redo(&mut self) -> Result<bool, AuthoringError> {
        self.ensure_writable()?;
        self.undo.redo(&mut self.world).map_err(Into::into)
    }

    /// Saves the current scene transactionally and advances the undo checkpoint.
    ///
    /// # Errors
    ///
    /// Returns an error for read-only state, invalid scene data, or failed persistence.
    pub fn save(&mut self) -> Result<(), AuthoringError> {
        self.ensure_writable()?;
        let mut document = SceneDocument::from_world(self.scene_id, &self.scene_name, &self.world)?;
        document.startup_scripts = self.scene_startup_scripts.clone();
        save_scene_atomic(&self.scene_path, &document)?;
        self.undo.mark_saved();
        Ok(())
    }

    /// Serializes the current in-memory scene exactly as a play snapshot consumes it.
    ///
    /// # Errors
    ///
    /// Returns an error when the world cannot be converted to a valid scene document.
    pub fn snapshot_bytes(&self) -> Result<Vec<u8>, AuthoringError> {
        let mut document = SceneDocument::from_world(self.scene_id, &self.scene_name, &self.world)?;
        document.startup_scripts = self.scene_startup_scripts.clone();
        Ok(document.to_bytes()?)
    }

    /// Computes the source-scene hash used to detect play-session authoring conflicts.
    ///
    /// # Errors
    ///
    /// Returns an error when the saved source cannot be read or the in-memory scene cannot be
    /// serialized.
    pub fn source_sha256(&self) -> Result<String, AuthoringError> {
        let bytes = self.snapshot_bytes()?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    /// Applies a reviewed runtime proposal as one centralized, undoable scene transaction.
    ///
    /// Currently the authoring adapter accepts whole-vector `translation`, `rotation`, and
    /// `scale` properties from the `transform.v1` component schema. Unknown schemas/properties
    /// are rejected without mutating the scene.
    ///
    /// # Errors
    ///
    /// Returns an error for read-only state, a changed source hash or property, unsupported
    /// targets/values, or an invalid world transaction.
    pub fn apply_runtime_changes(
        &mut self,
        change_set: &RuntimeChangeSet,
    ) -> Result<usize, AuthoringError> {
        self.ensure_writable()?;
        let source_hash = self.source_sha256()?;
        let mut review_store = MapChangeStore::new(source_hash);
        for change in &change_set.changes {
            review_store.insert(change.target.clone(), self.runtime_value(&change.target)?);
        }
        let mut transaction = ApplyRuntimeChangesTransaction::new(change_set.clone())
            .map_err(|error| AuthoringError::RuntimeChanges(error.to_string()))?;
        transaction
            .apply(&mut review_store)
            .map_err(|error| AuthoringError::RuntimeChanges(error.to_string()))?;

        let mut transforms = BTreeMap::new();
        for change in &change_set.changes {
            let entity = change
                .target
                .entity_id
                .parse::<EntityId>()
                .map_err(|error| AuthoringError::RuntimeChanges(error.to_string()))?;
            let current = self.world.local_transform(entity)?;
            let (_, after) = transforms.entry(entity).or_insert((current, current));
            let value = review_store.get(&change.target).cloned().ok_or_else(|| {
                AuthoringError::RuntimeChanges("reviewed runtime value disappeared".to_owned())
            })?;
            apply_transform_value(after, &change.target, value)?;
        }
        let edits = transforms
            .into_iter()
            .filter_map(|(entity, (before, after))| {
                (before != after).then_some(SceneEdit::Transform {
                    entity,
                    before,
                    after,
                })
            })
            .collect::<Vec<_>>();
        let changed = edits.len();
        if changed > 0 {
            self.undo.execute(
                &mut self.world,
                SceneEdit::Transaction {
                    label: "Apply runtime changes".to_owned(),
                    edits,
                },
            )?;
        }
        Ok(changed)
    }

    fn runtime_value(&self, target: &RuntimeChangeTarget) -> Result<RuntimeValue, AuthoringError> {
        let entity = target
            .entity_id
            .parse::<EntityId>()
            .map_err(|error| AuthoringError::RuntimeChanges(error.to_string()))?;
        if target.component_schema != "transform.v1" {
            return Err(AuthoringError::RuntimeChanges(format!(
                "unsupported component schema {}",
                target.component_schema
            )));
        }
        let transform = self.world.local_transform(entity)?;
        match target.property.as_str() {
            "translation" => Ok(RuntimeValue::Vector3(
                transform.translation.to_array().map(f64::from),
            )),
            "rotation" => Ok(RuntimeValue::Vector4(
                transform.rotation.to_array().map(f64::from),
            )),
            "scale" => Ok(RuntimeValue::Vector3(
                transform.scale.to_array().map(f64::from),
            )),
            property => Err(AuthoringError::RuntimeChanges(format!(
                "unsupported transform property {property}"
            ))),
        }
    }

    fn ensure_writable(&self) -> Result<(), AuthoringError> {
        if self.read_only {
            Err(AuthoringError::ReadOnly)
        } else {
            Ok(())
        }
    }
}

fn apply_transform_value(
    transform: &mut LocalTransform,
    target: &RuntimeChangeTarget,
    value: RuntimeValue,
) -> Result<(), AuthoringError> {
    if target.component_schema != "transform.v1" {
        return Err(AuthoringError::RuntimeChanges(format!(
            "unsupported component schema {}",
            target.component_schema
        )));
    }
    match (target.property.as_str(), value) {
        ("translation", RuntimeValue::Vector3(value)) => {
            transform.translation = glam::Vec3::from_array(f64_vector3_to_f32(value)?);
        }
        ("rotation", RuntimeValue::Vector4(value)) => {
            transform.rotation = glam::Quat::from_array(f64_vector4_to_f32(value)?).normalize();
        }
        ("scale", RuntimeValue::Vector3(value)) => {
            transform.scale = glam::Vec3::from_array(f64_vector3_to_f32(value)?);
        }
        (property, _) => {
            return Err(AuthoringError::RuntimeChanges(format!(
                "invalid value for transform property {property}"
            )));
        }
    }
    Ok(())
}

fn f64_vector3_to_f32(value: [f64; 3]) -> Result<[f32; 3], AuthoringError> {
    Ok([
        finite_f32(value[0])?,
        finite_f32(value[1])?,
        finite_f32(value[2])?,
    ])
}

fn f64_vector4_to_f32(value: [f64; 4]) -> Result<[f32; 4], AuthoringError> {
    Ok([
        finite_f32(value[0])?,
        finite_f32(value[1])?,
        finite_f32(value[2])?,
        finite_f32(value[3])?,
    ])
}

fn finite_f32(value: f64) -> Result<f32, AuthoringError> {
    value
        .is_finite()
        .then(|| value.to_f32())
        .flatten()
        .ok_or_else(|| AuthoringError::RuntimeChanges("non-finite/out-of-range number".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_play::{RuntimeChange, RuntimeChangeTarget};
    use engine_project::ProjectTemplate;
    use glam::Vec3;
    use tempfile::tempdir;

    #[test]
    fn gizmo_preview_drag_commits_exactly_one_undo_entry_and_read_only_rejects() {
        let temporary = tempdir().unwrap();
        let project = engine_project::Project::create(
            temporary.path().join("gizmo"),
            "Gizmo",
            ProjectTemplate::Empty3d,
        )
        .unwrap();
        let mut document = AuthoringDocument::open(project, false).unwrap();
        let entity = document
            .entity_snapshots()
            .unwrap()
            .into_iter()
            .find(|snapshot| snapshot.primitive.is_some())
            .unwrap()
            .id;
        let before = document.world().local_transform(entity).unwrap();
        let mid = LocalTransform {
            translation: Vec3::X,
            ..before
        };
        let after = LocalTransform {
            translation: Vec3::new(2.0, 0.0, 0.0),
            ..before
        };
        document.preview_transform(entity, mid).unwrap();
        document.preview_transform(entity, after).unwrap();
        document
            .commit_transform_drag(entity, before, after)
            .unwrap();
        assert!(document.undo().unwrap());
        assert_eq!(document.world().local_transform(entity).unwrap(), before);
        assert!(!document.undo().unwrap());

        let project = engine_project::Project::open(temporary.path().join("gizmo")).unwrap();
        let mut read_only = AuthoringDocument::open(project, true).unwrap();
        assert!(matches!(
            read_only.preview_transform(entity, after),
            Err(AuthoringError::ReadOnly)
        ));
    }

    #[test]
    fn create_edit_undo_redo_save_close_reopen_is_lossless() {
        let temporary = tempdir().unwrap();
        let root = temporary.path().join("authoring");
        let project = Project::create(&root, "Authoring", ProjectTemplate::Blank).unwrap();
        let mut document = AuthoringDocument::open(project, false).unwrap();
        let id = document
            .add_primitive("Cube", Primitive::Cube { size: 2.0 }, None)
            .unwrap();
        document
            .set_transform(
                id,
                LocalTransform {
                    translation: Vec3::new(1.0, 2.0, 3.0),
                    ..LocalTransform::IDENTITY
                },
            )
            .unwrap();
        assert!(document.undo().unwrap());
        assert_eq!(
            document.world().local_transform(id).unwrap(),
            LocalTransform::IDENTITY
        );
        assert!(document.redo().unwrap());
        document.save().unwrap();
        assert!(!document.is_modified());
        drop(document);

        let project = Project::open(&root).unwrap();
        let reopened = AuthoringDocument::open(project, false).unwrap();
        let snapshot = reopened.world().snapshot(id).unwrap();
        assert_eq!(snapshot.name.as_deref(), Some("Cube"));
        assert_eq!(
            snapshot.local_transform.translation,
            Vec3::new(1.0, 2.0, 3.0)
        );
        assert_eq!(snapshot.primitive, Some(Primitive::Cube { size: 2.0 }));
    }

    #[test]
    fn folders_persist_and_accept_new_scene_objects() {
        let temporary = tempdir().unwrap();
        let root = temporary.path().join("folders");
        let project = Project::create(&root, "Folders", ProjectTemplate::Blank).unwrap();
        let mut document = AuthoringDocument::open(project, false).unwrap();
        let folder = document.add_folder(None).unwrap();
        document.set_name(folder, "Environment".into()).unwrap();
        let child = document
            .add_primitive("Tree", Primitive::Cube { size: 1.0 }, Some(folder))
            .unwrap();
        document.save().unwrap();
        drop(document);

        let project = Project::open(&root).unwrap();
        let reopened = AuthoringDocument::open(project, false).unwrap();
        let folder_snapshot = reopened.world().snapshot(folder).unwrap();
        assert!(folder_snapshot.folder);
        assert_eq!(folder_snapshot.name.as_deref(), Some("Environment"));
        assert_eq!(reopened.world().parent(child).unwrap(), Some(folder));
    }

    #[test]
    fn read_only_session_refuses_all_writes() {
        let temporary = tempdir().unwrap();
        let project = Project::create(
            temporary.path().join("read-only"),
            "RO",
            ProjectTemplate::Blank,
        )
        .unwrap();
        let mut document = AuthoringDocument::open(project, true).unwrap();
        assert!(matches!(document.save(), Err(AuthoringError::ReadOnly)));
        assert!(matches!(
            document.add_primitive("Cube", Primitive::Cube { size: 1.0 }, None),
            Err(AuthoringError::ReadOnly)
        ));
    }

    #[test]
    fn reviewed_runtime_changes_apply_as_one_undoable_authoring_transaction() {
        let temporary = tempdir().unwrap();
        let project = Project::create(
            temporary.path().join("runtime-apply"),
            "Runtime Apply",
            ProjectTemplate::Blank,
        )
        .unwrap();
        let mut document = AuthoringDocument::open(project, false).unwrap();
        let entity = document
            .add_primitive("Cube", Primitive::Cube { size: 1.0 }, None)
            .unwrap();
        document.save().unwrap();
        let base_hash = document.source_sha256().unwrap();
        let target = RuntimeChangeTarget {
            entity_id: entity.to_string(),
            component_schema: "transform.v1".to_owned(),
            property: "translation".to_owned(),
        };
        let changes = RuntimeChangeSet::new(
            base_hash,
            vec![RuntimeChange {
                target,
                before: RuntimeValue::Vector3([0.0, 0.0, 0.0]),
                after: RuntimeValue::Vector3([4.0, 5.0, 6.0]),
            }],
        );
        assert_eq!(document.apply_runtime_changes(&changes).unwrap(), 1);
        assert_eq!(
            document
                .world()
                .local_transform(entity)
                .unwrap()
                .translation,
            Vec3::new(4.0, 5.0, 6.0)
        );
        assert!(document.undo().unwrap());
        assert_eq!(
            document
                .world()
                .local_transform(entity)
                .unwrap()
                .translation,
            Vec3::ZERO
        );
    }
}
