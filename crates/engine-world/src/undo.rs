use crate::{
    EntityId, EntitySnapshot, LocalTransform, PartAttributes, Primitive, SceneWorld,
    ScriptComponent, WorldCommand, WorldError,
};

/// Reversible scene mutation. Transactions are committed through one validated world
/// command batch and therefore cannot leave a partial hierarchy edit behind.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneEdit {
    Camera {
        entity: EntityId,
        before: Option<crate::Camera>,
        after: Option<crate::Camera>,
    },
    Light {
        entity: EntityId,
        before: Option<crate::Light>,
        after: Option<crate::Light>,
    },
    Name {
        entity: EntityId,
        before: Option<String>,
        after: Option<String>,
    },
    Transform {
        entity: EntityId,
        before: LocalTransform,
        after: LocalTransform,
    },
    Primitive {
        entity: EntityId,
        before: Option<Primitive>,
        after: Option<Primitive>,
    },
    PartAttributes {
        entity: EntityId,
        before: PartAttributes,
        after: PartAttributes,
    },
    Scripts {
        entity: EntityId,
        before: Vec<ScriptComponent>,
        after: Vec<ScriptComponent>,
    },
    Reparent {
        entity: EntityId,
        before: Option<EntityId>,
        after: Option<EntityId>,
    },
    Create {
        snapshot: EntitySnapshot,
    },
    Delete {
        snapshot: EntitySnapshot,
        children: Vec<EntityId>,
    },
    Transaction {
        label: String,
        edits: Vec<SceneEdit>,
    },
}

impl SceneEdit {
    /// Captures all data required to undo deletion of one entity and its child links.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError::EntityNotFound`] when the entity is absent.
    pub fn capture_delete(world: &SceneWorld, entity: EntityId) -> Result<Self, WorldError> {
        Ok(Self::Delete {
            snapshot: world.snapshot(entity)?,
            children: world.children(entity)?.to_vec(),
        })
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Camera { .. } => "Camera",
            Self::Light { .. } => "Light",
            Self::Name { .. } => "Name",
            Self::Transform { .. } => "Transform",
            Self::Primitive { .. } => "Primitive dimensions",
            Self::PartAttributes { .. } => "Part attributes",
            Self::Scripts { .. } => "Script components",
            Self::Reparent { .. } => "Reparent",
            Self::Create { .. } => "Create entity",
            Self::Delete { .. } => "Delete entity",
            Self::Transaction { label, .. } => label,
        }
    }

    fn append_forward(&self, output: &mut Vec<WorldCommand>) {
        match self {
            Self::Camera { entity, after, .. } => output.push(WorldCommand::SetCamera {
                entity: *entity,
                value: *after,
            }),
            Self::Light { entity, after, .. } => output.push(WorldCommand::SetLight {
                entity: *entity,
                value: *after,
            }),
            Self::Name { entity, after, .. } => output.push(WorldCommand::SetName {
                entity: *entity,
                value: after.clone(),
            }),
            Self::Transform { entity, after, .. } => output.push(WorldCommand::SetLocalTransform {
                entity: *entity,
                value: *after,
            }),
            Self::Primitive { entity, after, .. } => output.push(WorldCommand::SetPrimitive {
                entity: *entity,
                value: after.clone(),
            }),
            Self::PartAttributes { entity, after, .. } => {
                output.push(WorldCommand::SetPartAttributes {
                    entity: *entity,
                    value: *after,
                })
            }
            Self::Scripts { entity, after, .. } => output.push(WorldCommand::SetScripts {
                entity: *entity,
                value: after.clone(),
            }),
            Self::Reparent { entity, after, .. } => output.push(WorldCommand::SetParent {
                child: *entity,
                parent: *after,
            }),
            Self::Create { snapshot } => {
                output.push(WorldCommand::Spawn(Box::new(snapshot.clone())));
            }
            Self::Delete { snapshot, .. } => output.push(WorldCommand::Despawn(snapshot.id)),
            Self::Transaction { edits, .. } => {
                for edit in edits {
                    edit.append_forward(output);
                }
            }
        }
    }

    fn append_reverse(&self, output: &mut Vec<WorldCommand>) {
        match self {
            Self::Camera { entity, before, .. } => output.push(WorldCommand::SetCamera {
                entity: *entity,
                value: *before,
            }),
            Self::Light { entity, before, .. } => output.push(WorldCommand::SetLight {
                entity: *entity,
                value: *before,
            }),
            Self::Name { entity, before, .. } => output.push(WorldCommand::SetName {
                entity: *entity,
                value: before.clone(),
            }),
            Self::Transform { entity, before, .. } => {
                output.push(WorldCommand::SetLocalTransform {
                    entity: *entity,
                    value: *before,
                });
            }
            Self::Primitive { entity, before, .. } => output.push(WorldCommand::SetPrimitive {
                entity: *entity,
                value: before.clone(),
            }),
            Self::PartAttributes { entity, before, .. } => {
                output.push(WorldCommand::SetPartAttributes {
                    entity: *entity,
                    value: *before,
                })
            }
            Self::Scripts { entity, before, .. } => output.push(WorldCommand::SetScripts {
                entity: *entity,
                value: before.clone(),
            }),
            Self::Reparent { entity, before, .. } => output.push(WorldCommand::SetParent {
                child: *entity,
                parent: *before,
            }),
            Self::Create { snapshot } => output.push(WorldCommand::Despawn(snapshot.id)),
            Self::Delete { snapshot, children } => {
                output.push(WorldCommand::Spawn(Box::new(snapshot.clone())));
                for child in children {
                    output.push(WorldCommand::SetParent {
                        child: *child,
                        parent: Some(snapshot.id),
                    });
                }
            }
            Self::Transaction { edits, .. } => {
                for edit in edits.iter().rev() {
                    edit.append_reverse(output);
                }
            }
        }
    }

    fn apply(&self, world: &mut SceneWorld) -> Result<(), WorldError> {
        let mut commands = Vec::new();
        self.append_forward(&mut commands);
        world.apply_commands(&commands)?;
        world.propagate_transforms();
        Ok(())
    }

    fn revert(&self, world: &mut SceneWorld) -> Result<(), WorldError> {
        let mut commands = Vec::new();
        self.append_reverse(&mut commands);
        world.apply_commands(&commands)?;
        world.propagate_transforms();
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct UndoEntry {
    edit: SceneEdit,
    before_state: u64,
    after_state: u64,
}

/// Central scene edit history with explicit saved-scene checkpoint state.
#[derive(Clone, Debug)]
pub struct UndoStack {
    undo: Vec<UndoEntry>,
    redo: Vec<UndoEntry>,
    current_state: u64,
    saved_state: u64,
    next_state: u64,
}

impl Default for UndoStack {
    fn default() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            current_state: 0,
            saved_state: 0,
            next_state: 1,
        }
    }
}

impl UndoStack {
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies and records one reversible edit, clearing the redo branch.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError`] when the complete edit transaction is invalid.
    pub fn execute(&mut self, world: &mut SceneWorld, edit: SceneEdit) -> Result<(), WorldError> {
        edit.apply(world)?;
        let after_state = self.next_state;
        self.next_state = self.next_state.saturating_add(1);
        self.undo.push(UndoEntry {
            edit,
            before_state: self.current_state,
            after_state,
        });
        self.current_state = after_state;
        self.redo.clear();
        Ok(())
    }

    /// Reverts the most recent edit.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError`] if the reverse transaction no longer validates.
    pub fn undo(&mut self, world: &mut SceneWorld) -> Result<bool, WorldError> {
        let Some(entry) = self.undo.pop() else {
            return Ok(false);
        };
        if let Err(error) = entry.edit.revert(world) {
            self.undo.push(entry);
            return Err(error);
        }
        self.current_state = entry.before_state;
        self.redo.push(entry);
        Ok(true)
    }

    /// Reapplies the most recently undone edit.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError`] if the forward transaction no longer validates.
    pub fn redo(&mut self, world: &mut SceneWorld) -> Result<bool, WorldError> {
        let Some(entry) = self.redo.pop() else {
            return Ok(false);
        };
        if let Err(error) = entry.edit.apply(world) {
            self.redo.push(entry);
            return Err(error);
        }
        self.current_state = entry.after_state;
        self.undo.push(entry);
        Ok(true)
    }

    pub fn mark_saved(&mut self) {
        self.saved_state = self.current_state;
    }

    pub const fn is_modified(&self) -> bool {
        self.current_state != self.saved_state
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|entry| entry.edit.label())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|entry| entry.edit.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn transform_and_delete_round_trip_through_undo_redo() {
        let snapshot = EntitySnapshot::default();
        let id = snapshot.id;
        let mut world = SceneWorld::new();
        let mut undo = UndoStack::new();
        undo.execute(&mut world, SceneEdit::Create { snapshot })
            .unwrap();
        undo.mark_saved();
        let after = LocalTransform {
            translation: Vec3::new(1.0, 2.0, 3.0),
            ..LocalTransform::IDENTITY
        };
        undo.execute(
            &mut world,
            SceneEdit::Transform {
                entity: id,
                before: LocalTransform::IDENTITY,
                after,
            },
        )
        .unwrap();
        assert!(undo.is_modified());
        assert_eq!(world.local_transform(id).unwrap(), after);
        assert!(undo.undo(&mut world).unwrap());
        assert_eq!(world.local_transform(id).unwrap(), LocalTransform::IDENTITY);
        assert!(!undo.is_modified());
        assert!(undo.redo(&mut world).unwrap());
        assert_eq!(world.local_transform(id).unwrap(), after);

        let delete = SceneEdit::capture_delete(&world, id).unwrap();
        undo.execute(&mut world, delete).unwrap();
        assert!(!world.contains(id));
        assert!(undo.undo(&mut world).unwrap());
        assert!(world.contains(id));
        assert_eq!(world.local_transform(id).unwrap(), after);
    }
}

#[cfg(test)]
mod view_component_tests {
    use super::*;
    #[test]
    fn camera_and_light_edits_validate_undo_and_serialize() {
        let mut world = SceneWorld::new();
        let entity = EntitySnapshot {
            camera: Some(crate::Camera::default()),
            light: Some(crate::Light::default()),
            ..EntitySnapshot::default()
        };
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(entity.clone()))])
            .unwrap();
        let camera = crate::Camera {
            order: 5,
            active: false,
            ..crate::Camera::default()
        };
        let light = crate::Light {
            intensity: 3.0,
            kind: crate::LightKind::Spot,
            ..crate::Light::default()
        };
        let mut undo = UndoStack::default();
        undo.execute(
            &mut world,
            SceneEdit::Transaction {
                label: "View".into(),
                edits: vec![
                    SceneEdit::Camera {
                        entity: entity.id,
                        before: entity.camera,
                        after: Some(camera),
                    },
                    SceneEdit::Light {
                        entity: entity.id,
                        before: entity.light,
                        after: Some(light),
                    },
                ],
            },
        )
        .unwrap();
        assert_eq!(world.camera(entity.id).unwrap(), Some(camera));
        undo.undo(&mut world).unwrap();
        assert_eq!(world.light(entity.id).unwrap(), entity.light);
        undo.redo(&mut world).unwrap();
        let document = crate::SceneDocument {
            entities: vec![world.snapshot(entity.id).unwrap()],
            ..crate::SceneDocument::new("View test")
        };
        let loaded = crate::load_scene(&document.to_bytes().unwrap())
            .unwrap()
            .document
            .create_world()
            .unwrap();
        assert_eq!(loaded.camera(entity.id).unwrap(), Some(camera));
        assert_eq!(loaded.light(entity.id).unwrap(), Some(light));
        assert!(
            world
                .apply_commands(&[WorldCommand::SetLight {
                    entity: entity.id,
                    value: Some(crate::Light {
                        intensity: f32::NAN,
                        ..light
                    })
                }])
                .is_err()
        );
        assert_eq!(world.light(entity.id).unwrap(), Some(light));
    }
}
