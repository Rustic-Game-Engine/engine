use crate::components::{
    Camera, Children, Folder, Light, LocalTransform, Material, Mesh, Name, Parent, PartAttributes,
    ScriptComponents, StableEntity, WorldTransform,
};
use crate::primitive::Primitive;
use crate::scene::{EntitySnapshot, SceneDocument, SceneInstance};
use crate::{EntityId, RuntimeEntityRef, SceneInstanceId, WorldId};
use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::World;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use thiserror::Error;

/// Errors from world mutation, reference validation, or schedule execution.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum WorldError {
    #[error("invalid friction for {0}: expected a finite coefficient between 0 and 10")]
    InvalidFriction(EntityId),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("invalid scene environment settings")]
    InvalidEnvironment,
    #[error("entity {0} does not exist")]
    EntityNotFound(EntityId),
    #[error("entity {0} already exists")]
    DuplicateEntity(EntityId),
    #[error("entity {0} was both spawned and despawned in one transaction")]
    ConflictingStructuralChange(EntityId),
    #[error("parent {parent} for entity {child} does not exist")]
    ParentNotFound { child: EntityId, parent: EntityId },
    #[error("hierarchy cycle would include entity {0}")]
    HierarchyCycle(EntityId),
    #[error("runtime entity reference belongs to another world")]
    WrongWorld,
    #[error("runtime entity reference is stale")]
    StaleReference,
    #[error("entity {0} has a non-finite or singular local transform")]
    InvalidTransform(EntityId),
    #[error("entity {entity} has invalid primitive data: {message}")]
    InvalidPrimitive { entity: EntityId, message: String },
    #[error("entity {entity} has invalid script component data: {message}")]
    InvalidScript { entity: EntityId, message: String },
    #[error("entity {0} has invalid camera or light attributes")]
    InvalidViewComponent(EntityId),
    #[error("system `{system}` failed: {message}")]
    System { system: String, message: String },
}

/// Structural/component mutations collected by systems and applied at a schedule barrier.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldCommand {
    SetEnvironment(crate::SceneEnvironment),
    Spawn(Box<EntitySnapshot>),
    Despawn(EntityId),
    SetParent {
        child: EntityId,
        parent: Option<EntityId>,
    },
    SetLocalTransform {
        entity: EntityId,
        value: LocalTransform,
    },
    SetName {
        entity: EntityId,
        value: Option<String>,
    },
    SetMesh {
        entity: EntityId,
        value: Option<Mesh>,
    },
    SetTextures {
        entity: EntityId,
        value: Option<crate::SurfaceTextures>,
    },
    SetMaterial {
        entity: EntityId,
        value: Option<Material>,
    },
    SetCamera {
        entity: EntityId,
        value: Option<Camera>,
    },
    SetLight {
        entity: EntityId,
        value: Option<Light>,
    },
    SetPrimitive {
        entity: EntityId,
        value: Option<Primitive>,
    },
    SetEffect {
        entity: EntityId,
        value: Option<Box<crate::EffectComponent>>,
    },
    SetEffectGroup {
        entity: EntityId,
        value: Option<Box<crate::effects::EffectGroup>>,
    },
    SetPartAttributes {
        entity: EntityId,
        value: PartAttributes,
    },
    SetScripts {
        entity: EntityId,
        value: Vec<crate::ScriptComponent>,
    },
}

/// Deterministic schedule phases. Deferred mutations apply after every phase.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SchedulePhase {
    FixedUpdate,
    Update,
    PostUpdate,
}

impl SchedulePhase {
    const fn index(self) -> usize {
        match self {
            Self::FixedUpdate => 0,
            Self::Update => 1,
            Self::PostUpdate => 2,
        }
    }
}

/// Engine-owned system boundary; Bevy system and entity types remain private.
pub trait WorldSystem: Send {
    /// Stable diagnostic name.
    fn name(&self) -> &str;

    /// Runs against the live world. Structural edits should be sent through
    /// [`SceneWorld::defer`].
    ///
    /// # Errors
    ///
    /// Returns a diagnostic message when the system cannot complete its phase.
    fn run(&mut self, world: &mut SceneWorld) -> Result<(), String>;
}

/// Named closure adapter for small systems and tests.
pub struct EngineSystem<F> {
    name: String,
    callback: F,
}

impl<F> EngineSystem<F> {
    pub fn new(name: impl Into<String>, callback: F) -> Self {
        Self {
            name: name.into(),
            callback,
        }
    }
}

impl<F> WorldSystem for EngineSystem<F>
where
    F: FnMut(&mut SceneWorld) -> Result<(), String> + Send,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn run(&mut self, world: &mut SceneWorld) -> Result<(), String> {
        (self.callback)(world)
    }
}

#[derive(Debug)]
struct Slot {
    generation: u32,
    entity: Option<Entity>,
    stable: Option<EntityId>,
}

#[derive(Default)]
struct GpuFeedback {
    context: Option<u64>,
    acknowledgements: BTreeMap<EntityId, u64>,
    last_context: u64,
    pending: bool,
}

/// Live ECS world with stable identities, generational references, and a scene graph.
pub struct SceneWorld {
    effect_groups: crate::effects::EffectGroupRuntime,
    gpu_feedback: std::sync::Mutex<Option<GpuFeedback>>,
    effects_runtime: crate::effects::EffectRuntime,
    effect_assets: BTreeMap<crate::AssetId, crate::effects::EffectAsset>,
    temporary_effects: BTreeMap<EntityId, (u8, f32)>,
    // Runtime ownership preserves source clocks without inheriting transform shear.
    temporary_effect_owners: BTreeMap<EntityId, EntityId>,
    completed_effect_groups: BTreeSet<EntityId>,
    pub(crate) services: crate::foundations::EntityServices,
    environment: crate::SceneEnvironment,
    id: WorldId,
    ecs: World,
    lookup: BTreeMap<EntityId, Entity>,
    runtime_slots: Vec<Slot>,
    free_slots: Vec<u32>,
    entity_slots: BTreeMap<EntityId, u32>,
    deferred: Vec<WorldCommand>,
    schedules: [Vec<Box<dyn WorldSystem>>; 3],
    hierarchy_order: Vec<EntityId>,
    hierarchy_scratch: Vec<EntityId>,
    hierarchy_dirty: bool,
    transform_dirty: BTreeSet<EntityId>,
    instances: Vec<SceneInstance>,
}

impl fmt::Debug for SceneWorld {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SceneWorld")
            .field("id", &self.id)
            .field("entity_count", &self.lookup.len())
            .field("deferred_count", &self.deferred.len())
            .finish_non_exhaustive()
    }
}

impl Default for SceneWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl SceneWorld {
    pub fn new() -> Self {
        Self {
            effect_groups: crate::effects::EffectGroupRuntime::default(),
            gpu_feedback: std::sync::Mutex::new(None),
            effects_runtime: crate::effects::EffectRuntime::default(),
            effect_assets: BTreeMap::new(),
            temporary_effects: BTreeMap::new(),
            temporary_effect_owners: BTreeMap::new(),
            completed_effect_groups: BTreeSet::new(),
            services: crate::foundations::EntityServices::default(),
            environment: crate::SceneEnvironment::default(),
            id: WorldId::new(),
            ecs: World::new(),
            lookup: BTreeMap::new(),
            runtime_slots: Vec::new(),
            free_slots: Vec::new(),
            entity_slots: BTreeMap::new(),
            deferred: Vec::new(),
            schedules: std::array::from_fn(|_| Vec::new()),
            hierarchy_order: Vec::new(),
            hierarchy_scratch: Vec::new(),
            hierarchy_dirty: true,
            transform_dirty: BTreeSet::new(),
            instances: Vec::new(),
        }
    }

    pub fn environment(&self) -> &crate::SceneEnvironment {
        &self.environment
    }

    pub const fn id(&self) -> WorldId {
        self.id
    }

    pub fn entity_count(&self) -> usize {
        self.lookup.len()
    }

    pub fn contains(&self, entity: EntityId) -> bool {
        self.lookup.contains_key(&entity)
    }

    pub fn entity_ids(&self) -> impl ExactSizeIterator<Item = EntityId> + '_ {
        self.lookup.keys().copied()
    }

    /// Resolves a dot- or slash-separated name path (for example `Workspace.Room.Table`).
    /// # Errors
    /// This lookup currently returns `Ok` even when no entity matches.
    pub fn find_path(&self, path: &str) -> Result<Option<EntityId>, WorldError> {
        let parts: Vec<_> = path
            .split(['.', '/'])
            .filter(|part| {
                !part.is_empty() && *part != "Game" && *part != "scene" && *part != "Workspace"
            })
            .collect();
        let mut parent = None;
        for part in parts {
            let mut matches = self.entity_ids().filter(|id| {
                self.ecs_entity(*id).is_ok_and(|entity| {
                    self.ecs
                        .get::<Name>(entity)
                        .is_some_and(|name| name.0 == part)
                        && self.ecs.get::<Parent>(entity).map(|value| value.0) == parent
                })
            });
            let Some(found) = matches.next() else {
                return Ok(None);
            };
            parent = Some(found);
        }
        Ok(parent)
    }

    /// # Errors
    /// Propagates errors from resolving the parent path.
    pub fn list_path(&self, path: &str) -> Result<Vec<EntityId>, WorldError> {
        let parent = if path.is_empty() || matches!(path, "Game.scene" | "scene" | "Workspace") {
            None
        } else {
            self.find_path(path)?
        };
        Ok(self
            .entity_ids()
            .filter(|id| self.parent(*id).ok().flatten() == parent)
            .collect())
    }

    /// # Errors
    /// Returns an error if the entity or an ancestor is absent.
    pub fn entity_path(&self, id: EntityId) -> Result<String, WorldError> {
        let mut names = Vec::new();
        let mut current = Some(id);
        while let Some(entity_id) = current {
            let snapshot = self.snapshot(entity_id)?;
            names.push(snapshot.name.unwrap_or_else(|| entity_id.to_string()));
            current = snapshot.parent;
        }
        names.reverse();
        Ok(names.join("."))
    }

    pub fn instances(&self) -> &[SceneInstance] {
        &self.instances
    }

    pub(crate) fn replace_instances(&mut self, instances: Vec<SceneInstance>) {
        self.instances = instances;
        self.instances.sort_by_key(|value| value.id);
    }

    pub fn add_system(&mut self, phase: SchedulePhase, system: impl WorldSystem + 'static) {
        self.schedules[phase.index()].push(Box::new(system));
    }

    pub fn defer(&mut self, command: WorldCommand) {
        self.deferred.push(command);
    }

    pub fn deferred_count(&self) -> usize {
        self.deferred.len()
    }

    /// Runs a schedule and applies all structural commands at its barrier.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError`] when a system fails or the deferred batch is invalid.
    pub fn run_schedule(&mut self, phase: SchedulePhase) -> Result<(), WorldError> {
        let index = phase.index();
        let mut systems = std::mem::take(&mut self.schedules[index]);
        for system in &mut systems {
            if let Err(message) = system.run(self) {
                self.deferred.clear();
                let error = WorldError::System {
                    system: system.name().to_owned(),
                    message,
                };
                self.schedules[index] = systems;
                return Err(error);
            }
        }
        self.schedules[index] = systems;
        self.apply_deferred()?;
        self.propagate_transforms();
        Ok(())
    }

    /// Applies and clears the pending deferred command buffer.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError`] without partial mutation when validation fails.
    pub fn apply_deferred(&mut self) -> Result<(), WorldError> {
        let commands = std::mem::take(&mut self.deferred);
        match self.apply_commands(&commands) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.deferred.clear();
                Err(error)
            }
        }
    }

    /// Validates the complete final hierarchy before performing any mutation.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError`] when an identity, component, or prospective hierarchy is
    /// invalid. Validation completes before any command is committed.
    #[allow(
        clippy::too_many_lines,
        reason = "validation and commit are kept adjacent to make the transaction boundary explicit"
    )]
    pub fn apply_commands(&mut self, commands: &[WorldCommand]) -> Result<(), WorldError> {
        if commands.is_empty() {
            return Ok(());
        }
        let removed: BTreeSet<_> = commands
            .iter()
            .filter_map(|command| {
                if let WorldCommand::Despawn(entity) = command {
                    Some(*entity)
                } else {
                    None
                }
            })
            .collect();
        let mut normalized = commands.to_vec();
        if !removed.is_empty() {
            let mut groups: BTreeMap<_, _> = self
                .entity_ids()
                .filter_map(|entity| {
                    self.effect_group(entity)
                        .ok()
                        .flatten()
                        .map(|group| (entity, group))
                })
                .collect();
            for command in commands {
                match command {
                    WorldCommand::Spawn(snapshot) => {
                        if let Some(group) = &snapshot.effect_group {
                            groups.insert(snapshot.id, group.clone());
                        }
                    }
                    WorldCommand::SetEffectGroup { entity, value } => {
                        groups.remove(entity);
                        if let Some(group) = value {
                            groups.insert(*entity, group.as_ref().clone());
                        }
                    }
                    _ => {}
                }
            }
            for (entity, mut group) in groups {
                if !removed.contains(&entity)
                    && group
                        .child_entities
                        .iter()
                        .any(|child| removed.contains(child))
                {
                    group.remove_children(&removed);
                    normalized.push(WorldCommand::SetEffectGroup {
                        entity,
                        value: Some(Box::new(group)),
                    });
                }
            }
        }
        let commands = normalized.as_slice();
        for command in commands {
            if let WorldCommand::SetEnvironment(value) = command
                && !value.is_valid()
            {
                return Err(WorldError::InvalidEnvironment);
            }
        }
        let mut entities: BTreeSet<_> = self.lookup.keys().copied().collect();
        let mut parents = self.parent_map();
        let mut spawned = BTreeSet::new();
        let mut despawned = BTreeSet::new();

        for command in commands {
            let (entity, camera, light) = match command {
                WorldCommand::Spawn(snapshot) => (snapshot.id, snapshot.camera, snapshot.light),
                WorldCommand::SetCamera { entity, value } => (*entity, *value, None),
                WorldCommand::SetLight { entity, value } => (*entity, None, *value),
                _ => continue,
            };
            if camera.is_some_and(|value| !value.is_valid())
                || light.is_some_and(|value| !value.is_valid())
            {
                return Err(WorldError::InvalidViewComponent(entity));
            }
        }
        for command in commands {
            match command {
                WorldCommand::Spawn(snapshot) => {
                    snapshot.validate()?;
                    if entities.contains(&snapshot.id) || !spawned.insert(snapshot.id) {
                        return Err(WorldError::DuplicateEntity(snapshot.id));
                    }
                    entities.insert(snapshot.id);
                    parents.insert(snapshot.id, snapshot.parent);
                }
                WorldCommand::Despawn(entity) => {
                    if !entities.contains(entity) {
                        return Err(WorldError::EntityNotFound(*entity));
                    }
                    if spawned.contains(entity) || !despawned.insert(*entity) {
                        return Err(WorldError::ConflictingStructuralChange(*entity));
                    }
                    entities.remove(entity);
                    parents.remove(entity);
                }
                WorldCommand::SetParent { child, parent } => {
                    parents.insert(*child, *parent);
                }
                WorldCommand::SetPartAttributes { entity, value } => {
                    if !value.friction.is_finite() || !(0.0..=10.0).contains(&value.friction) {
                        return Err(WorldError::InvalidFriction(*entity));
                    }
                }
                WorldCommand::SetLocalTransform { entity, value } => {
                    if !value.is_finite() {
                        return Err(WorldError::InvalidTransform(*entity));
                    }
                }
                WorldCommand::SetPrimitive {
                    entity,
                    value: Some(value),
                } => value
                    .validate()
                    .map_err(|error| WorldError::InvalidPrimitive {
                        entity: *entity,
                        message: error.to_string(),
                    })?,
                _ => {}
            }
        }

        for command in commands {
            if let WorldCommand::SetEffectGroup {
                value: Some(value), ..
            } = command
            {
                value.validate().map_err(WorldError::InvalidArgument)?;
            }
            if let WorldCommand::SetEffect {
                value: Some(value), ..
            } = command
            {
                value.validate().map_err(WorldError::InvalidArgument)?;
            }
            if let WorldCommand::SetTextures {
                value: Some(value), ..
            } = command
            {
                value.validate().map_err(WorldError::InvalidArgument)?;
            }
            let referenced = match command {
                WorldCommand::Spawn(_) | WorldCommand::SetEnvironment(_) => None,
                WorldCommand::Despawn(entity)
                | WorldCommand::SetLocalTransform { entity, .. }
                | WorldCommand::SetName { entity, .. }
                | WorldCommand::SetMesh { entity, .. }
                | WorldCommand::SetTextures { entity, .. }
                | WorldCommand::SetMaterial { entity, .. }
                | WorldCommand::SetCamera { entity, .. }
                | WorldCommand::SetLight { entity, .. }
                | WorldCommand::SetPrimitive { entity, .. }
                | WorldCommand::SetEffect { entity, .. }
                | WorldCommand::SetEffectGroup { entity, .. }
                | WorldCommand::SetPartAttributes { entity, .. }
                | WorldCommand::SetScripts { entity, .. } => Some(*entity),
                WorldCommand::SetParent { child, .. } => Some(*child),
            };
            if let Some(entity) = referenced
                && !entities.contains(&entity)
                && !despawned.contains(&entity)
            {
                return Err(WorldError::EntityNotFound(entity));
            }
        }

        for (child, parent) in &mut parents {
            if let Some(parent_id) = parent
                && !entities.contains(parent_id)
            {
                if despawned.contains(parent_id) {
                    *parent = None;
                } else {
                    return Err(WorldError::ParentNotFound {
                        child: *child,
                        parent: *parent_id,
                    });
                }
            }
        }
        validate_acyclic(&entities, &parents)?;
        // Validate bindings against the final transaction, including new children.
        // No ECS mutation occurs before all typed values and track references pass.
        let mut prospective_effects: BTreeMap<_, _> = self
            .entity_ids()
            .filter_map(|id| self.effect(id).ok().flatten().map(|effect| (id, effect)))
            .collect();
        let mut prospective_groups: BTreeMap<_, _> = self
            .entity_ids()
            .filter_map(|id| {
                self.effect_group(id)
                    .ok()
                    .flatten()
                    .map(|group| (id, group))
            })
            .collect();
        for command in commands {
            match command {
                WorldCommand::Spawn(snapshot) => {
                    if let Some(effect) = &snapshot.effect {
                        prospective_effects.insert(snapshot.id, effect.clone());
                    }
                    if let Some(group) = &snapshot.effect_group {
                        prospective_groups.insert(snapshot.id, group.clone());
                    }
                }
                WorldCommand::SetEffect { entity, value } => {
                    prospective_effects.remove(entity);
                    if let Some(value) = value {
                        prospective_effects.insert(*entity, value.as_ref().clone());
                    }
                }
                WorldCommand::SetEffectGroup { entity, value } => {
                    prospective_groups.remove(entity);
                    if let Some(value) = value {
                        prospective_groups.insert(*entity, value.as_ref().clone());
                    }
                }
                _ => {}
            }
        }
        for (entity, group) in prospective_groups
            .iter()
            .filter(|(entity, _)| entities.contains(entity))
        {
            group.validate().map_err(WorldError::InvalidArgument)?;
            let mut children = Vec::with_capacity(group.child_entities.len());
            let mut seen = BTreeSet::new();
            for child in &group.child_entities {
                if !seen.insert(*child) || !entities.contains(child) {
                    return Err(WorldError::InvalidArgument(
                        "composite track is missing or duplicated".into(),
                    ));
                }
                let mut ancestor = parents.get(child).copied().flatten();
                while ancestor.is_some() && ancestor != Some(*entity) {
                    ancestor = ancestor.and_then(|id| parents.get(&id).copied().flatten());
                }
                if ancestor != Some(*entity) {
                    return Err(WorldError::InvalidArgument(
                        "composite tracks must belong to its subtree".into(),
                    ));
                }
                children.push(prospective_effects.get(child).cloned().ok_or_else(|| {
                    WorldError::InvalidArgument("composite track has no effect".into())
                })?);
            }
            crate::effects::validate_group_bindings(&group.parameters, &group.bindings, &children)
                .map_err(WorldError::InvalidArgument)?;
            crate::effects::validate_group_triggers(&group.trigger_bindings, children.len())
                .map_err(WorldError::InvalidArgument)?;
        }

        for command in commands {
            if let WorldCommand::Spawn(snapshot) = command {
                self.spawn_snapshot((**snapshot).clone());
            }
        }
        for command in commands {
            match command {
                WorldCommand::SetLocalTransform { entity, value } => {
                    self.insert_or_remove(*entity, Some(*value));
                    self.mark_subtree_dirty(*entity);
                }
                WorldCommand::SetEnvironment(value) => self.environment = value.clone(),
                WorldCommand::SetName { entity, value } => {
                    self.insert_or_remove(*entity, value.clone().map(Name));
                }
                WorldCommand::SetMesh { entity, value } => self.insert_or_remove(*entity, *value),
                WorldCommand::SetTextures { entity, value } => {
                    self.insert_or_remove(*entity, value.clone());
                }
                WorldCommand::SetMaterial { entity, value } => {
                    self.insert_or_remove(*entity, *value);
                }
                WorldCommand::SetCamera { entity, value } => {
                    self.insert_or_remove(*entity, *value);
                }
                WorldCommand::SetLight { entity, value } => self.insert_or_remove(*entity, *value),
                WorldCommand::SetPrimitive { entity, value } => {
                    self.insert_or_remove(*entity, value.clone());
                }
                WorldCommand::SetEffect { entity, value } => {
                    self.insert_or_remove(*entity, value.as_deref().cloned());
                    if value.is_none() {
                        self.effects_runtime.remove(*entity);
                    }
                }
                WorldCommand::SetEffectGroup { entity, value } => {
                    self.insert_or_remove(*entity, value.as_deref().cloned());
                    if value.is_none() {
                        self.effect_groups.remove(*entity);
                    }
                }
                WorldCommand::SetPartAttributes { entity, value } => {
                    self.insert_or_remove(*entity, Some(*value));
                }
                WorldCommand::SetScripts { entity, value } => {
                    for script in value {
                        script
                            .validate()
                            .map_err(|message| WorldError::InvalidScript {
                                entity: *entity,
                                message,
                            })?;
                    }
                    self.insert_or_remove(
                        *entity,
                        (!value.is_empty()).then(|| ScriptComponents(value.clone())),
                    );
                }
                _ => {}
            }
        }
        for command in commands {
            if let WorldCommand::Despawn(entity) = command {
                self.effects_runtime.remove(*entity);
                self.effect_groups.remove(*entity);
                self.temporary_effects.remove(entity);
                self.temporary_effect_owners.remove(entity);
                self.completed_effect_groups.remove(entity);
                self.despawn_now(*entity);
            }
        }
        self.install_parent_map(&parents);
        self.hierarchy_dirty = true;
        if commands.iter().any(|command| {
            matches!(
                command,
                WorldCommand::Spawn(_) | WorldCommand::Despawn(_) | WorldCommand::SetParent { .. }
            )
        }) {
            self.transform_dirty.extend(self.lookup.keys().copied());
        }
        Ok(())
    }

    /// Creates a generational live reference for a stable entity.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError::EntityNotFound`] when the entity is absent.
    pub fn reference(&self, entity: EntityId) -> Result<RuntimeEntityRef, WorldError> {
        let slot = *self
            .entity_slots
            .get(&entity)
            .ok_or(WorldError::EntityNotFound(entity))?;
        let generation = self.runtime_slots[slot as usize].generation;
        Ok(RuntimeEntityRef {
            world: self.id,
            slot,
            generation,
        })
    }

    /// Resolves a live reference after checking world identity and generation.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError::WrongWorld`] or [`WorldError::StaleReference`] as applicable.
    pub fn resolve(&self, reference: RuntimeEntityRef) -> Result<EntityId, WorldError> {
        if reference.world != self.id {
            return Err(WorldError::WrongWorld);
        }
        let slot = self
            .runtime_slots
            .get(reference.slot as usize)
            .ok_or(WorldError::StaleReference)?;
        if slot.generation != reference.generation || slot.entity.is_none() {
            return Err(WorldError::StaleReference);
        }
        slot.stable.ok_or(WorldError::StaleReference)
    }

    /// Returns an entity's local transform.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError::EntityNotFound`] when the entity is absent.
    pub fn local_transform(&self, id: EntityId) -> Result<LocalTransform, WorldError> {
        self.component::<LocalTransform>(id).copied()
    }

    /// Returns an entity's derived world transform.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError::EntityNotFound`] when the entity is absent.
    pub fn world_transform(&self, id: EntityId) -> Result<WorldTransform, WorldError> {
        self.component::<WorldTransform>(id).copied()
    }

    /// Returns an entity's optional stable parent.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError::EntityNotFound`] when the entity is absent.
    pub fn parent(&self, id: EntityId) -> Result<Option<EntityId>, WorldError> {
        let entity = self.ecs_entity(id)?;
        Ok(self.ecs.get::<Parent>(entity).map(|parent| parent.0))
    }

    /// Returns an entity's deterministically ordered children.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError::EntityNotFound`] when the entity is absent.
    pub fn children(&self, id: EntityId) -> Result<&[EntityId], WorldError> {
        Ok(&self.component::<Children>(id)?.0)
    }

    /// Captures an entity's authored components.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError::EntityNotFound`] when the entity is absent.
    pub fn snapshot(&self, id: EntityId) -> Result<EntitySnapshot, WorldError> {
        let entity = self.ecs_entity(id)?;
        Ok(EntitySnapshot {
            id,
            name: self.ecs.get::<Name>(entity).map(|value| value.0.clone()),
            folder: self.ecs.get::<Folder>(entity).is_some(),
            local_transform: self
                .ecs
                .get::<LocalTransform>(entity)
                .copied()
                .unwrap_or_default(),
            parent: self.ecs.get::<Parent>(entity).map(|value| value.0),
            mesh: self.ecs.get::<Mesh>(entity).copied(),
            material: self.ecs.get::<Material>(entity).copied(),
            textures: self.ecs.get::<crate::SurfaceTextures>(entity).cloned(),
            camera: self.ecs.get::<Camera>(entity).copied(),
            light: self.ecs.get::<Light>(entity).copied(),
            primitive: self.ecs.get::<Primitive>(entity).cloned(),
            effect: self.ecs.get::<crate::EffectComponent>(entity).cloned(),
            effect_group: self.ecs.get::<crate::effects::EffectGroup>(entity).cloned(),
            part_attributes: self
                .ecs
                .get::<PartAttributes>(entity)
                .copied()
                .unwrap_or_default(),
            scripts: self
                .ecs
                .get::<ScriptComponents>(entity)
                .map_or_else(Vec::new, |value| value.0.clone()),
            unknown_components: Vec::new(),
        })
    }

    /// Returns the optional mesh component.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError::EntityNotFound`] when the entity is absent.
    pub fn mesh(&self, id: EntityId) -> Result<Option<Mesh>, WorldError> {
        let entity = self.ecs_entity(id)?;
        Ok(self.ecs.get::<Mesh>(entity).copied())
    }

    /// Returns the authored scene effect, independent of playback state.
    ///
    /// # Errors
    /// Returns [`WorldError::EntityNotFound`] when the entity is absent.
    pub fn effect(&self, id: EntityId) -> Result<Option<crate::EffectComponent>, WorldError> {
        let entity = self.ecs_entity(id)?;
        Ok(self.ecs.get::<crate::EffectComponent>(entity).cloned())
    }

    /// Returns the persisted composite timeline without exposing its playback state.
    /// # Errors
    /// Returns an error when the entity does not exist.
    pub fn effect_group(
        &self,
        id: EntityId,
    ) -> Result<Option<crate::effects::EffectGroup>, WorldError> {
        let entity = self.ecs_entity(id)?;
        Ok(self.ecs.get::<crate::effects::EffectGroup>(entity).cloned())
    }

    /// Returns a composite's own timeline state, independently of its children.
    /// # Errors
    /// Returns an error when the entity does not exist.
    pub fn effect_group_state(
        &self,
        id: EntityId,
    ) -> Result<Option<crate::effects::PlaybackState>, WorldError> {
        Ok(self
            .effect_group(id)?
            .map(|group| self.effect_groups.state(id, &group)))
    }

    /// Drains bounded composite cues into the engine's existing audio mixer.
    pub fn take_effect_audio_events(&mut self) -> Vec<crate::effects::EffectAudioEvent> {
        self.effect_groups.take_events()
    }

    /// Submits successful renderer progress; the simulation consumes it at its next tick.
    /// The interior mailbox lets immutable render callbacks acknowledge GPU work.
    pub fn submit_gpu_particle_feedback(
        &self,
        context: Option<u64>,
        acknowledgements: &[(EntityId, u64)],
    ) {
        if let Ok(mut mailbox) = self.gpu_feedback.lock() {
            let feedback = mailbox.get_or_insert_with(GpuFeedback::default);
            if context.is_some_and(|context| context < feedback.last_context) {
                return;
            }
            if feedback.context != context {
                feedback.acknowledgements.clear();
            }
            feedback.context = context;
            if let Some(context) = context {
                feedback.last_context = feedback.last_context.max(context);
            }
            feedback.pending = true;
            for &(entity, sequence) in acknowledgements.iter().take(4096) {
                if feedback.acknowledgements.len() < 4096
                    || feedback.acknowledgements.contains_key(&entity)
                {
                    feedback
                        .acknowledgements
                        .entry(entity)
                        .and_modify(|last| *last = (*last).max(sequence))
                        .or_insert(sequence);
                }
            }
        }
    }

    /// Advances cosmetic effects after transforms and physics. Authored data is unchanged.
    pub fn update_effects(&mut self, delta: f32) {
        let camera_entity = self
            .entity_ids()
            .filter_map(|entity| {
                let camera = self.camera(entity).ok()??;
                camera.active.then_some((camera.order, entity))
            })
            .max()
            .map(|(_, entity)| entity);
        let camera = camera_entity
            .and_then(|entity| self.world_transform(entity).ok())
            .map_or(glam::Vec3::ZERO, |transform| transform.0.w_axis.truncate());
        self.update_effects_from_camera(delta, camera, camera_entity);
    }

    /// Advances the shared simulation using an editor or custom viewport camera.
    /// A missing target previews camera effects independently of game-camera assignment.
    pub fn update_effects_from_camera(
        &mut self,
        delta: f32,
        camera: glam::Vec3,
        camera_entity: Option<EntityId>,
    ) {
        self.consume_gpu_particle_feedback();
        let transforms: BTreeMap<_, _> = self
            .entity_ids()
            .filter_map(|entity| {
                self.world_transform(entity)
                    .ok()
                    .map(|transform| (entity, transform.0))
            })
            .collect();
        let groups = self.effect_group_inputs(&transforms);
        self.effect_groups.update(delta, &groups);
        let restarted = self.effect_groups.take_restarted();
        let completed = self.effect_groups.take_completed();
        for group in restarted.keys() {
            self.completed_effect_groups.remove(group);
            for entity in self.descendants(*group).unwrap_or_default() {
                if let Some(child_group) = self.effect_group(entity).ok().flatten() {
                    let position = transforms
                        .get(&entity)
                        .map_or([0.0; 3], |transform| transform.w_axis.truncate().to_array());
                    let relative_scale = self
                        .composite_ancestors(entity)
                        .iter()
                        .take_while(|(id, _)| id != group)
                        .fold(1.0_f32, |scale, (_, group)| {
                            (scale * group.time_scale).min(1000.0)
                        });
                    let elapsed = restarted.get(group).copied().unwrap_or(0.0) * relative_scale;
                    self.effect_groups
                        .restart_at(entity, &child_group, position, elapsed);
                }
                if let Some(effect) = self.effect(entity).ok().flatten() {
                    let transform = transforms
                        .get(&entity)
                        .copied()
                        .unwrap_or(glam::Mat4::IDENTITY);
                    self.effects_runtime.ensure(&crate::effects::EffectInput {
                        entity,
                        transform,
                        effect,
                    });
                    self.effects_runtime
                        .control(entity, crate::effects::PlaybackCommand::Restart);
                }
            }
        }
        let inputs = self.composite_effect_inputs(delta, &transforms, &restarted, &completed);
        self.effects_runtime.set_camera_target(camera_entity);
        let layers = camera_entity
            .and_then(|entity| self.camera(entity).ok().flatten())
            .map_or(u32::MAX, |camera| camera.visibility_layers);
        self.effects_runtime.set_visibility_layers(layers);
        let parents: BTreeMap<_, _> = inputs
            .iter()
            .map(|input| {
                let transform = self
                    .parent(input.entity)
                    .ok()
                    .flatten()
                    .and_then(|parent| transforms.get(&parent).copied())
                    .unwrap_or(glam::Mat4::IDENTITY);
                (input.entity, transform)
            })
            .collect();
        self.effects_runtime.set_parent_transforms(&parents);
        self.effects_runtime
            .update_with_targets(delta, &inputs, camera, &transforms);
        for group in &completed {
            for entity in self.descendants(*group).unwrap_or_default() {
                self.effects_runtime
                    .control(entity, crate::effects::PlaybackCommand::Stop);
            }
        }
        self.update_effect_children(delta);
        self.trigger_effect_group_events(&groups);
        self.update_effect_group_events();
    }

    fn effect_group_inputs(
        &self,
        transforms: &BTreeMap<EntityId, glam::Mat4>,
    ) -> Vec<(EntityId, [f32; 3], crate::effects::EffectGroup)> {
        self.entity_ids()
            .filter_map(|entity| {
                let mut group = self.effect_group(entity).ok()??;
                let ancestors = self.composite_ancestors(entity);
                group.time_scale = ancestors.iter().fold(1.0_f32, |scale, (_, group)| {
                    (scale * group.time_scale).min(1000.0)
                });
                if ancestors.iter().any(|(id, ancestor)| {
                    *id != entity && {
                        let state = self.effect_groups.state(*id, ancestor);
                        match state {
                            crate::effects::PlaybackState::Playing => false,
                            crate::effects::PlaybackState::Completed => {
                                self.is_physical_effect_ancestor(entity, *id)
                            }
                            _ => true,
                        }
                    }
                }) {
                    group.time_scale = 0.0;
                }
                Some((
                    entity,
                    transforms.get(&entity)?.w_axis.truncate().to_array(),
                    group,
                ))
            })
            .collect()
    }

    fn trigger_effect_group_events(
        &mut self,
        groups: &[(EntityId, [f32; 3], crate::effects::EffectGroup)],
    ) {
        let events = self.effects_runtime.frame().events.clone();
        for (entity, _, group) in groups {
            if self.effect_groups.state(*entity, group) == crate::effects::PlaybackState::Playing {
                for (target, command) in crate::effects::group_trigger_commands(group, &events) {
                    let _ = self.control_effect(target, command);
                }
            }
        }
    }

    fn consume_gpu_particle_feedback(&mut self) {
        let feedback = self.gpu_feedback.lock().ok().and_then(|mut mailbox| {
            let feedback = mailbox.as_mut()?;
            if !feedback.pending {
                return None;
            }
            feedback.pending = false;
            Some((
                feedback.context,
                std::mem::take(&mut feedback.acknowledgements),
            ))
        });
        if let Some((context, acknowledgements)) = feedback {
            self.effects_runtime.set_gpu_particle_context(context);
            let acknowledgements: Vec<_> = acknowledgements.into_iter().collect();
            self.effects_runtime
                .acknowledge_gpu_particles(&acknowledgements);
        }
    }

    fn composite_effect_inputs(
        &self,
        delta: f32,
        transforms: &BTreeMap<EntityId, glam::Mat4>,
        restarted: &BTreeMap<EntityId, f32>,
        completed: &BTreeSet<EntityId>,
    ) -> Vec<crate::effects::EffectInput> {
        let mut inputs = Vec::new();
        for entity in self.entity_ids() {
            let Some(mut effect) = self.effect(entity).ok().flatten() else {
                continue;
            };
            let mut ancestors = self.composite_ancestors(entity);
            ancestors.reverse();
            let mut speed = 1.0_f32;
            for (group_id, group) in &ancestors {
                speed = (speed * group.time_scale).min(1000.0);
                let state = self.effect_groups.state(*group_id, group);
                if state == crate::effects::PlaybackState::Paused {
                    speed = 0.0;
                }
                if matches!(
                    state,
                    crate::effects::PlaybackState::Stopped
                        | crate::effects::PlaybackState::Disposed
                ) || (state == crate::effects::PlaybackState::Completed
                    && !completed.contains(group_id)
                    && self.is_physical_effect_ancestor(entity, *group_id))
                {
                    effect.common.enabled = false;
                }
                if let Some(elapsed) = restarted.get(group_id) {
                    let scale_delta = delta.max(0.0) * group.time_scale;
                    if scale_delta > 0.0 {
                        speed *= (*elapsed / scale_delta).clamp(0.0, 1.0);
                    }
                }
                // Group edits and assets are validated before mutation. Re-check
                // the cloned result defensively when loading recovered scenes.
                if crate::effects::apply_group_bindings(group, entity, &mut effect).is_err() {
                    effect.common.enabled = false;
                }
                if group.trigger_bindings.iter().any(|binding| {
                    group.child_entities.get(binding.source_child as usize) == Some(&entity)
                }) {
                    match &mut effect.kind {
                        crate::effects::EffectKind::Particle(p)
                        | crate::effects::EffectKind::MeshParticle(p) => p.generate_events = true,
                        crate::effects::EffectKind::Weather(w) => w.emitter.generate_events = true,
                        _ => {}
                    }
                }
            }
            effect.common.time_scale = (effect.common.time_scale * speed).min(1000.0);
            let Some(transform) = transforms.get(&entity).copied() else {
                continue;
            };
            inputs.push(crate::effects::EffectInput {
                entity,
                transform,
                effect,
            });
        }
        inputs
    }

    fn is_physical_effect_ancestor(&self, entity: EntityId, ancestor: EntityId) -> bool {
        let mut current = Some(entity);
        while let Some(entity) = current {
            if entity == ancestor {
                return true;
            }
            current = self.parent(entity).ok().flatten();
        }
        false
    }

    fn composite_ancestors(
        &self,
        entity: EntityId,
    ) -> Vec<(EntityId, crate::effects::EffectGroup)> {
        let mut current = Some(entity);
        let mut groups = Vec::new();
        let mut visited = BTreeSet::new();
        while let Some(entity) = current {
            if !visited.insert(entity) {
                break;
            }
            if let Some(group) = self.effect_group(entity).ok().flatten() {
                groups.push((entity, group));
            }
            current = self
                .parent(entity)
                .ok()
                .flatten()
                .or_else(|| self.temporary_effect_owners.get(&entity).copied());
        }
        groups
    }

    /// Returns local composite playback scaling, excluding the global scene clock.
    /// # Errors
    /// Returns an error when the entity is absent.
    pub fn effective_effect_time_scale(&self, entity: EntityId) -> Result<f32, WorldError> {
        self.ecs_entity(entity)?;
        Ok(self
            .composite_ancestors(entity)
            .iter()
            .fold(1.0_f32, |scale, (_, group)| {
                (scale * group.time_scale).min(1000.0)
            }))
    }

    fn update_effect_group_events(&mut self) {
        let mut groups: BTreeSet<_> = self
            .entity_ids()
            .filter(|entity| self.effect_group(*entity).ok().flatten().is_some())
            .collect();
        for event in &self.effects_runtime.frame().events {
            if event.kind != crate::effects::EffectEventKind::Completed {
                continue;
            }
            let mut ancestor = self.parent(event.entity).ok().flatten();
            while let Some(entity) = ancestor {
                if self
                    .ecs_entity(entity)
                    .is_ok_and(|entity| self.ecs.get::<Folder>(entity).is_some())
                {
                    groups.insert(entity);
                }
                ancestor = self.parent(entity).ok().flatten();
            }
        }
        for entity in groups {
            if self.effect_state(entity).ok().flatten()
                == Some(crate::effects::PlaybackState::Completed)
                && self.completed_effect_groups.insert(entity)
            {
                let position = self
                    .world_transform(entity)
                    .map_or(glam::Vec3::ZERO, |transform| transform.0.w_axis.truncate());
                self.effects_runtime
                    .emit_event(crate::effects::EffectEvent {
                        entity,
                        kind: crate::effects::EffectEventKind::Completed,
                        position,
                        child_effect: None,
                        velocity: glam::Vec3::ZERO,
                    });
            }
        }
    }

    /// Registers a validated asset under the existing project sidecar identity.
    ///
    /// # Errors
    /// Returns an error for an invalid asset or incompatible instance overrides.
    pub fn register_effect_asset(
        &mut self,
        id: crate::AssetId,
        asset: crate::effects::EffectAsset,
    ) -> Result<(), WorldError> {
        asset.validate().map_err(WorldError::InvalidArgument)?;
        let mut commands = Vec::new();
        for entity in self.entity_ids() {
            if let Some(group) = self.effect_group(entity)?
                && group.source == Some(id)
            {
                let mut defaults = asset.clone();
                let mapping: BTreeMap<_, _> = group
                    .child_entities
                    .iter()
                    .enumerate()
                    .filter_map(|(index, child)| {
                        let source = self.effect(*child).ok()??.source?;
                        (source.asset == id).then_some((source.child, u32::try_from(index).ok()?))
                    })
                    .collect();
                defaults.bindings.retain_mut(|binding| {
                    if let Some(child) = mapping.get(&binding.child) {
                        binding.child = *child;
                        true
                    } else {
                        false
                    }
                });
                defaults.trigger_bindings.retain_mut(|binding| {
                    match (
                        mapping.get(&binding.source_child),
                        mapping.get(&binding.target_child),
                    ) {
                        (Some(source), Some(target)) => {
                            binding.source_child = *source;
                            binding.target_child = *target;
                            true
                        }
                        _ => false,
                    }
                });
                let updated = group.refresh_defaults(&defaults);
                if updated != group {
                    commands.push(WorldCommand::SetEffectGroup {
                        entity,
                        value: Some(Box::new(updated)),
                    });
                }
            }
            let Some(instance) = self.effect(entity)? else {
                continue;
            };
            let Some(source) = &instance.source else {
                continue;
            };
            if source.asset != id {
                continue;
            }
            let Some(new_default) = asset.effects.get(source.child as usize) else {
                continue;
            };
            let mut new_default = new_default.clone();
            let parent = self.parent(entity)?;
            let remap: BTreeMap<_, _> = self
                .entity_ids()
                .filter_map(|sibling| {
                    if self.parent(sibling).ok()? != parent {
                        return None;
                    }
                    let source = self.effect(sibling).ok()??.source?;
                    if source.asset != id {
                        return None;
                    }
                    Some((*asset.entity_ids.get(source.child as usize)?, sibling))
                })
                .collect();
            new_default.remap_entities(&remap);
            let Some(baseline) = &source.baseline else {
                continue;
            };
            let old_default = baseline.as_ref().clone().into_component();
            let mut merged =
                crate::effects::merge_instance_defaults(&instance, &old_default, &new_default)
                    .map_err(WorldError::InvalidArgument)?;
            if let Some(source) = &mut merged.source {
                source.baseline = Some(Box::new(crate::effects::EffectDefaults::from_component(
                    &new_default,
                )));
            }
            if merged != instance {
                commands.push(WorldCommand::SetEffect {
                    entity,
                    value: Some(Box::new(merged)),
                });
            }
        }
        if !commands.is_empty() {
            self.apply_commands(&commands)?;
        }
        self.effect_assets.insert(id, asset);
        Ok(())
    }

    pub fn has_effect_asset(&self, id: crate::AssetId) -> bool {
        self.effect_assets.contains_key(&id)
    }

    /// Instantiates a reusable or composite asset as an ordinary parented scene subtree.
    ///
    /// # Errors
    /// Returns an error for a missing asset, invalid parent or invalid transform.
    pub fn instantiate_effect_asset(
        &mut self,
        asset_id: crate::AssetId,
        parent: Option<EntityId>,
        transform: LocalTransform,
    ) -> Result<EntityId, WorldError> {
        let asset = self.effect_assets.get(&asset_id).ok_or_else(|| {
            WorldError::InvalidArgument(format!("effect asset {asset_id} is unavailable"))
        })?;
        let children = asset.instances_with_ids(Some(asset_id));
        let root = EntitySnapshot {
            name: Some(asset.name.clone()),
            folder: true,
            parent,
            local_transform: transform,
            effect_group: Some(crate::effects::EffectGroup::from_asset_with_children(
                Some(asset_id),
                asset,
                &children,
            )),
            ..Default::default()
        };
        let root_id = root.id;
        let mut commands = vec![WorldCommand::Spawn(Box::new(root))];
        for mut snapshot in children {
            snapshot.parent = Some(root_id);
            commands.push(WorldCommand::Spawn(Box::new(snapshot)));
        }
        self.apply_commands(&commands)?;
        self.propagate_transforms();
        Ok(root_id)
    }

    /// Samples global and local wind using the same fields as particle simulation.
    pub fn sample_effect_wind(&self, position: glam::Vec3, time: f32, layers: u32) -> glam::Vec3 {
        let inputs: Vec<_> = self
            .entity_ids()
            .filter_map(|entity| {
                Some(crate::effects::EffectInput {
                    entity,
                    transform: self.world_transform(entity).ok()?.0,
                    effect: self.effect(entity).ok()??,
                })
            })
            .collect();
        crate::effects::sample_wind(&inputs, position, time, layers)
    }

    fn expired_effect_children(&mut self, delta: f32) -> BTreeSet<EntityId> {
        let clocks: BTreeMap<_, _> = self
            .temporary_effect_owners
            .keys()
            .filter_map(|entity| {
                let state = self.effect_state(*entity).ok().flatten()?;
                let scale = self.effective_effect_time_scale(*entity).ok()?;
                let enabled = self.composite_ancestors(*entity).iter().all(|(id, group)| {
                    let state = self.effect_groups.state(*id, group);
                    state == crate::effects::PlaybackState::Playing
                        || (state == crate::effects::PlaybackState::Completed
                            && !self.is_physical_effect_ancestor(*entity, *id))
                });
                Some((*entity, (state, scale, enabled)))
            })
            .collect();
        let mut expired = BTreeSet::new();
        for (entity, (_, remaining)) in &mut self.temporary_effects {
            // Retain authored tracks until the temporary composite ends, so event
            // links can restart completed or initially stopped tracks.
            if !self.temporary_effect_owners.contains_key(entity) {
                continue;
            }
            let Some((state, scale, enabled)) = clocks.get(entity) else {
                expired.insert(*entity);
                continue;
            };
            if matches!(
                state,
                crate::effects::PlaybackState::Completed
                    | crate::effects::PlaybackState::Disposed
                    | crate::effects::PlaybackState::Stopped
            ) {
                expired.insert(*entity);
            } else if *state == crate::effects::PlaybackState::Playing && *enabled {
                *remaining -= delta * scale;
                if *remaining <= 0.0 {
                    expired.insert(*entity);
                }
            }
        }
        for _ in 0..8 {
            let before = expired.len();
            for (root, owner) in &self.temporary_effect_owners {
                if !self.contains(*owner) || expired.contains(owner) || expired.contains(root) {
                    expired.insert(*root);
                    expired.extend(self.descendants(*root).unwrap_or_default());
                }
            }
            if expired.len() == before {
                break;
            }
        }
        expired
    }

    fn update_effect_children(&mut self, delta: f32) {
        const MAX_CHILDREN: usize = 1024;
        const MAX_SPAWNS_PER_TICK: usize = 128;
        let delta = if delta.is_finite() {
            delta.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let expired = self.expired_effect_children(delta);
        let events = self.effects_runtime.frame().events.clone();
        let mut commands = Vec::new();
        let mut pending = Vec::new();
        let mut owners = Vec::new();
        for event in events {
            let Some(asset_id) = event.child_effect else {
                continue;
            };
            let generation = self
                .temporary_effects
                .get(&event.entity)
                .map_or(0, |(generation, _)| *generation);
            let maximum = self
                .effect(event.entity)
                .ok()
                .flatten()
                .map_or(0, |effect| match effect.kind {
                    crate::effects::EffectKind::Particle(p)
                    | crate::effects::EffectKind::MeshParticle(p) => p.max_generations.min(8),
                    crate::effects::EffectKind::Weather(w) => w.emitter.max_generations.min(8),
                    crate::effects::EffectKind::CollisionVolume(_) => 3,
                    _ => 0,
                });
            if generation >= maximum {
                continue;
            }
            let Some(asset) = self.effect_assets.get(&asset_id) else {
                continue;
            };
            let count = asset.effects.len() + 1;
            if commands.len() + count > MAX_SPAWNS_PER_TICK
                || self.temporary_effects.len() + pending.len() + count > MAX_CHILDREN
            {
                continue;
            }
            let mut children = asset.instances_with_ids(Some(asset_id));
            let mut group = crate::effects::EffectGroup::from_asset_with_children(
                Some(asset_id),
                asset,
                &children,
            );
            group.looping = false;
            group.auto_play = true;
            let root = EntitySnapshot {
                name: Some(format!("{} (sub-emitter)", asset.name)),
                folder: true,
                local_transform: LocalTransform {
                    translation: event.position,
                    ..Default::default()
                },
                effect_group: Some(group.clone()),
                ..Default::default()
            };
            let root_id = root.id;
            owners.push((root_id, event.entity));
            pending.push((root_id, (generation + 1, group.duration)));
            commands.push(WorldCommand::Spawn(Box::new(root)));
            for child in &mut children {
                child.parent = Some(root_id);
                // Event velocity is already scaled by the sub-emitter inheritance control.
                let inverse = child.local_transform.matrix().inverse();
                let inherited = inverse.transform_vector3(event.velocity).to_array();
                if let Some(effect) = &mut child.effect {
                    let emitter = match &mut effect.kind {
                        crate::effects::EffectKind::Particle(p)
                        | crate::effects::EffectKind::MeshParticle(p) => Some(p),
                        crate::effects::EffectKind::Weather(w) => Some(&mut w.emitter),
                        _ => None,
                    };
                    if let Some(emitter) = emitter {
                        for (value, inherited) in emitter.initial_velocity.iter_mut().zip(inherited)
                        {
                            *value += inherited;
                        }
                    }
                }
                pending.push((child.id, (generation + 1, group.duration)));
                commands.push(WorldCommand::Spawn(Box::new(child.clone())));
            }
        }
        commands.extend(
            expired
                .into_iter()
                .filter(|entity| self.contains(*entity))
                .map(WorldCommand::Despawn),
        );
        if !commands.is_empty() && self.apply_commands(&commands).is_ok() {
            self.temporary_effects.extend(pending);
            self.temporary_effect_owners.extend(owners);
            self.propagate_transforms();
        }
    }

    /// Immutable render extraction and events for the latest simulation tick.
    pub fn effect_frame(&self) -> &crate::effects::EffectFrame {
        self.effects_runtime.frame()
    }

    /// Controls every effect in an entity subtree.
    ///
    /// # Errors
    /// Returns an error if the entity is absent or the subtree has no effects.
    pub fn control_effect(
        &mut self,
        id: EntityId,
        command: crate::effects::PlaybackCommand,
    ) -> Result<(), WorldError> {
        let mut targets = self.descendants(id)?;
        targets.insert(0, id);
        loop {
            let roots: Vec<_> = self
                .temporary_effect_owners
                .iter()
                .filter_map(|(root, owner)| {
                    (targets.contains(owner) && !targets.contains(root)).then_some(*root)
                })
                .collect();
            if roots.is_empty() {
                break;
            }
            for root in roots {
                targets.push(root);
                targets.extend(self.descendants(root)?);
            }
        }
        let mut found = false;
        for entity in targets {
            if matches!(
                command,
                crate::effects::PlaybackCommand::Play
                    | crate::effects::PlaybackCommand::Restart
                    | crate::effects::PlaybackCommand::Reset
            ) {
                self.completed_effect_groups.remove(&entity);
            }
            if let Some(group) = self.effect_group(entity)? {
                found = true;
                self.effect_groups.control(entity, &group, command);
                if command == crate::effects::PlaybackCommand::Dispose {
                    self.insert_or_remove::<crate::effects::EffectGroup>(entity, None);
                }
            }
            let Some(effect) = self.effect(entity)? else {
                continue;
            };
            found = true;
            let transform = self.world_transform(entity)?.0;
            self.effects_runtime.ensure(&crate::effects::EffectInput {
                entity,
                transform,
                effect,
            });
            self.effects_runtime.control(entity, command);
        }
        if !found {
            return Err(WorldError::InvalidArgument(
                "entity subtree has no scene effect".into(),
            ));
        }
        Ok(())
    }

    /// Returns an aggregate playback state for an entity subtree.
    ///
    /// # Errors
    /// Returns an error if the entity is absent.
    pub fn effect_state(
        &self,
        id: EntityId,
    ) -> Result<Option<crate::effects::PlaybackState>, WorldError> {
        use crate::effects::PlaybackState;
        if let Some(group) = self.effect_group(id)? {
            return Ok(Some(self.effect_groups.state(id, &group)));
        }
        let mut targets = self.descendants(id)?;
        targets.insert(0, id);
        let mut states = Vec::new();
        for entity in targets {
            if self
                .effect(entity)?
                .is_some_and(|effect| effect.common.enabled)
            {
                states.push(self.effects_runtime.state(entity));
            }
            if let Some(group) = self.effect_group(entity)? {
                states.push(self.effect_groups.state(entity, &group));
            }
        }
        Ok(if states.is_empty() {
            None
        } else if states.contains(&PlaybackState::Playing) {
            Some(PlaybackState::Playing)
        } else if states.contains(&PlaybackState::Paused) {
            Some(PlaybackState::Paused)
        } else if states
            .iter()
            .all(|state| *state == PlaybackState::Completed)
        {
            Some(PlaybackState::Completed)
        } else if states.iter().all(|state| *state == PlaybackState::Disposed) {
            Some(PlaybackState::Disposed)
        } else {
            Some(PlaybackState::Stopped)
        })
    }

    /// Returns the optional surface texture component.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError::EntityNotFound`] when the entity is absent.
    pub fn surface_textures(
        &self,
        id: EntityId,
    ) -> Result<Option<crate::SurfaceTextures>, WorldError> {
        let entity = self.ecs_entity(id)?;
        Ok(self.ecs.get::<crate::SurfaceTextures>(entity).cloned())
    }

    /// Resolves direct maps followed by immediate Texture children in stable ID order.
    /// Children override only slots they supply; maps never leak to siblings or descendants.
    pub fn resolved_textures(&self, id: EntityId) -> crate::SurfaceTextures {
        let mut result = self.surface_textures(id).ok().flatten().unwrap_or_default();
        if let Ok(children) = self.children(id) {
            for child in children {
                if self.primitive(*child).ok().flatten().is_some()
                    || self.mesh(*child).ok().flatten().is_some()
                {
                    continue;
                }
                if let Ok(Some(value)) = self.surface_textures(*child) {
                    if value.maps.is_empty() {
                        continue;
                    }
                    result.maps.extend(value.maps);
                    result.uv_scale = value.uv_scale;
                    result.uv_offset = value.uv_offset;
                    result.height_scale = value.height_scale;
                    result.emissive_strength = value.emissive_strength;
                }
            }
        }
        result
    }

    /// Returns the optional material asset component.
    ///
    /// # Errors
    /// Returns an error if the entity does not exist.
    pub fn material(&self, id: EntityId) -> Result<Option<Material>, WorldError> {
        let entity = self.ecs_entity(id)?;
        Ok(self.ecs.get::<Material>(entity).copied())
    }

    /// Returns the optional parametric primitive component.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError::EntityNotFound`] when the entity is absent.
    pub fn primitive(&self, id: EntityId) -> Result<Option<&Primitive>, WorldError> {
        let entity = self.ecs_entity(id)?;
        Ok(self.ecs.get::<Primitive>(entity))
    }

    /// # Errors
    /// Returns `WorldError::EntityNotFound` if the entity is absent.
    pub fn part_attributes(&self, id: EntityId) -> Result<PartAttributes, WorldError> {
        let entity = self.ecs_entity(id)?;
        Ok(self
            .ecs
            .get::<PartAttributes>(entity)
            .copied()
            .unwrap_or_default())
    }

    /// Returns the optional camera component.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError::EntityNotFound`] when the entity is absent.
    pub fn camera(&self, id: EntityId) -> Result<Option<Camera>, WorldError> {
        let entity = self.ecs_entity(id)?;
        Ok(self.ecs.get::<Camera>(entity).copied())
    }

    /// Returns the optional light component.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError::EntityNotFound`] when the entity is absent.
    pub fn light(&self, id: EntityId) -> Result<Option<Light>, WorldError> {
        let entity = self.ecs_entity(id)?;
        Ok(self.ecs.get::<Light>(entity).copied())
    }

    /// Updates dirty world transforms in parent-before-child order.
    pub fn propagate_transforms(&mut self) -> usize {
        if self.hierarchy_dirty {
            self.rebuild_hierarchy_order();
        }
        if self.transform_dirty.is_empty() {
            return 0;
        }
        let mut changed = 0;
        for &id in &self.hierarchy_order {
            if !self.transform_dirty.contains(&id) {
                continue;
            }
            let Ok(entity) = self.ecs_entity(id) else {
                continue;
            };
            // Folders only organize the scene tree. They deliberately contribute no
            // spatial transform to themselves or their descendants.
            let local = if self.ecs.get::<Folder>(entity).is_some() {
                LocalTransform::IDENTITY
            } else {
                self.ecs
                    .get::<LocalTransform>(entity)
                    .copied()
                    .unwrap_or_default()
            };
            let parent_matrix = self
                .ecs
                .get::<Parent>(entity)
                .and_then(|parent| self.lookup.get(&parent.0))
                .and_then(|parent| self.ecs.get::<WorldTransform>(*parent))
                .map_or(glam::Mat4::IDENTITY, |value| value.0);
            if let Some(mut world) = self.ecs.get_mut::<WorldTransform>(entity) {
                world.0 = parent_matrix * local.matrix();
                changed += 1;
            }
        }
        self.transform_dirty.clear();
        changed
    }

    /// Creates an additive instance and remaps every internal entity reference.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError`] when the source scene or optional root parent would produce
    /// invalid world data.
    pub fn instantiate_scene(
        &mut self,
        document: &SceneDocument,
        instance_id: SceneInstanceId,
        root_parent: Option<EntityId>,
    ) -> Result<SceneInstance, WorldError> {
        let mut entity_map = BTreeMap::new();
        for source in &document.entities {
            entity_map.insert(source.id, EntityId::new());
        }
        let mut commands = Vec::with_capacity(document.entities.len());
        for source in &document.entities {
            let mut entity = source.clone();
            entity.id = entity_map[&source.id];
            entity.parent = source
                .parent
                .and_then(|parent| entity_map.get(&parent).copied())
                .or(root_parent);
            if let Some(effect) = &mut entity.effect {
                effect.remap_entities(&entity_map);
            }
            commands.push(WorldCommand::Spawn(Box::new(entity)));
        }
        self.apply_commands(&commands)?;
        self.propagate_transforms();
        let instance = SceneInstance {
            id: instance_id,
            source_scene: document.id,
            entity_map,
        };
        self.instances.push(instance.clone());
        self.instances.sort_by_key(|value| value.id);
        Ok(instance)
    }

    fn component<T: bevy_ecs::prelude::Component>(&self, id: EntityId) -> Result<&T, WorldError> {
        let entity = self.ecs_entity(id)?;
        self.ecs
            .get::<T>(entity)
            .ok_or(WorldError::EntityNotFound(id))
    }

    fn ecs_entity(&self, id: EntityId) -> Result<Entity, WorldError> {
        self.lookup
            .get(&id)
            .copied()
            .ok_or(WorldError::EntityNotFound(id))
    }

    fn spawn_snapshot(&mut self, snapshot: EntitySnapshot) {
        let mut entity = self.ecs.spawn((
            StableEntity(snapshot.id),
            snapshot.local_transform,
            WorldTransform::default(),
            Children::default(),
            snapshot.part_attributes,
        ));
        if let Some(name) = snapshot.name {
            entity.insert(Name(name));
        }
        if snapshot.folder {
            entity.insert(Folder);
        }
        if let Some(mesh) = snapshot.mesh {
            entity.insert(mesh);
        }
        if let Some(textures) = snapshot.textures {
            entity.insert(textures);
        }
        if let Some(material) = snapshot.material {
            entity.insert(material);
        }
        if let Some(camera) = snapshot.camera {
            entity.insert(camera);
        }
        if let Some(light) = snapshot.light {
            entity.insert(light);
        }
        if let Some(primitive) = snapshot.primitive {
            entity.insert(primitive);
        }
        if let Some(effect) = snapshot.effect {
            entity.insert(effect);
        }
        if let Some(group) = snapshot.effect_group {
            entity.insert(group);
        }
        if !snapshot.scripts.is_empty() {
            entity.insert(ScriptComponents(snapshot.scripts));
        }
        let ecs_entity = entity.id();
        self.lookup.insert(snapshot.id, ecs_entity);
        let slot = if let Some(slot) = self.free_slots.pop() {
            let record = &mut self.runtime_slots[slot as usize];
            record.entity = Some(ecs_entity);
            record.stable = Some(snapshot.id);
            slot
        } else {
            let slot = u32::try_from(self.runtime_slots.len()).expect("entity slot overflow");
            self.runtime_slots.push(Slot {
                generation: 1,
                entity: Some(ecs_entity),
                stable: Some(snapshot.id),
            });
            slot
        };
        self.entity_slots.insert(snapshot.id, slot);
        self.transform_dirty.insert(snapshot.id);
    }

    fn despawn_now(&mut self, id: EntityId) {
        self.services.tags.remove(&id);
        self.services.deadlines.remove(&id);
        for members in self.services.groups.values_mut() {
            members.remove(&id);
        }
        if let Some(entity) = self.lookup.remove(&id) {
            let _ = self.ecs.despawn(entity);
        }
        if let Some(slot) = self.entity_slots.remove(&id) {
            let record = &mut self.runtime_slots[slot as usize];
            record.entity = None;
            record.stable = None;
            record.generation = record.generation.wrapping_add(1).max(1);
            self.free_slots.push(slot);
        }
        self.transform_dirty.remove(&id);
    }

    fn insert_or_remove<T>(&mut self, id: EntityId, value: Option<T>)
    where
        T: bevy_ecs::prelude::Component,
    {
        if let Some(entity) = self.lookup.get(&id).copied() {
            let mut entity_mut = self.ecs.entity_mut(entity);
            if let Some(value) = value {
                entity_mut.insert(value);
            } else {
                entity_mut.remove::<T>();
            }
        }
    }

    fn parent_map(&self) -> BTreeMap<EntityId, Option<EntityId>> {
        self.lookup
            .iter()
            .map(|(id, entity)| (*id, self.ecs.get::<Parent>(*entity).map(|parent| parent.0)))
            .collect()
    }

    fn install_parent_map(&mut self, parents: &BTreeMap<EntityId, Option<EntityId>>) {
        for entity in self.lookup.values().copied() {
            let mut entity_mut = self.ecs.entity_mut(entity);
            entity_mut.remove::<Parent>();
            if let Some(mut children) = entity_mut.get_mut::<Children>() {
                children.0.clear();
            }
        }
        for (child, parent) in parents {
            let Some(parent) = parent else { continue };
            if let Some(entity) = self.lookup.get(child).copied() {
                self.ecs.entity_mut(entity).insert(Parent(*parent));
            }
            if let Some(entity) = self.lookup.get(parent).copied()
                && let Some(mut children) = self.ecs.get_mut::<Children>(entity)
            {
                children.0.push(*child);
            }
        }
        for entity in self.lookup.values().copied() {
            if let Some(mut children) = self.ecs.get_mut::<Children>(entity) {
                children.0.sort_unstable();
            }
        }
    }

    fn mark_subtree_dirty(&mut self, root: EntityId) {
        self.hierarchy_scratch.clear();
        self.hierarchy_scratch.push(root);
        while let Some(id) = self.hierarchy_scratch.pop() {
            if !self.transform_dirty.insert(id) {
                continue;
            }
            let Some(entity) = self.lookup.get(&id).copied() else {
                continue;
            };
            if let Some(children) = self.ecs.get::<Children>(entity) {
                self.hierarchy_scratch
                    .extend(children.0.iter().rev().copied());
            }
        }
    }

    fn rebuild_hierarchy_order(&mut self) {
        self.hierarchy_order.clear();
        self.hierarchy_scratch.clear();
        for (id, entity) in self.lookup.iter().rev() {
            if self.ecs.get::<Parent>(*entity).is_none() {
                self.hierarchy_scratch.push(*id);
            }
        }
        while let Some(id) = self.hierarchy_scratch.pop() {
            self.hierarchy_order.push(id);
            let Some(entity) = self.lookup.get(&id).copied() else {
                continue;
            };
            if let Some(children) = self.ecs.get::<Children>(entity) {
                self.hierarchy_scratch
                    .extend(children.0.iter().rev().copied());
            }
        }
        self.hierarchy_dirty = false;
    }
}

fn validate_acyclic(
    entities: &BTreeSet<EntityId>,
    parents: &BTreeMap<EntityId, Option<EntityId>>,
) -> Result<(), WorldError> {
    for start in entities {
        let mut slow = Some(*start);
        let mut fast = Some(*start);
        loop {
            slow = slow.and_then(|id| parents.get(&id).copied().flatten());
            fast = fast
                .and_then(|id| parents.get(&id).copied().flatten())
                .and_then(|id| parents.get(&id).copied().flatten());
            match (slow, fast) {
                (Some(left), Some(right)) if left == right => {
                    return Err(WorldError::HierarchyCycle(left));
                }
                (None, _) | (_, None) => break,
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    fn entity(id: EntityId, parent: Option<EntityId>) -> EntitySnapshot {
        EntitySnapshot {
            id,
            parent,
            ..EntitySnapshot::default()
        }
    }

    #[test]
    fn deferred_commands_apply_only_at_schedule_barrier() {
        let id = EntityId::new();
        let mut world = SceneWorld::new();
        world.add_system(
            SchedulePhase::Update,
            EngineSystem::new("spawn", move |world: &mut SceneWorld| {
                world.defer(WorldCommand::Spawn(Box::new(entity(id, None))));
                assert!(!world.contains(id));
                Ok(())
            }),
        );
        world.run_schedule(SchedulePhase::Update).unwrap();
        assert!(world.contains(id));
    }

    #[test]
    fn stale_and_cross_world_references_are_rejected() {
        let id = EntityId::new();
        let mut first = SceneWorld::new();
        first
            .apply_commands(&[WorldCommand::Spawn(Box::new(entity(id, None)))])
            .unwrap();
        let reference = first.reference(id).unwrap();
        let second = SceneWorld::new();
        assert_eq!(second.resolve(reference), Err(WorldError::WrongWorld));
        first.apply_commands(&[WorldCommand::Despawn(id)]).unwrap();
        assert_eq!(first.resolve(reference), Err(WorldError::StaleReference));
    }

    #[test]
    fn cycle_rejection_is_transactional_for_a_command_batch() {
        let a = EntityId::new();
        let b = EntityId::new();
        let c = EntityId::new();
        let mut world = SceneWorld::new();
        world
            .apply_commands(&[
                WorldCommand::Spawn(Box::new(entity(a, None))),
                WorldCommand::Spawn(Box::new(entity(b, Some(a)))),
                WorldCommand::Spawn(Box::new(entity(c, Some(b)))),
            ])
            .unwrap();
        let before = (
            world.parent(a).unwrap(),
            world.parent(b).unwrap(),
            world.parent(c).unwrap(),
        );
        let error = world
            .apply_commands(&[
                WorldCommand::SetParent {
                    child: a,
                    parent: Some(c),
                },
                WorldCommand::SetName {
                    entity: b,
                    value: Some("must not apply".to_owned()),
                },
            ])
            .unwrap_err();
        assert!(matches!(error, WorldError::HierarchyCycle(_)));
        assert_eq!(
            before,
            (
                world.parent(a).unwrap(),
                world.parent(b).unwrap(),
                world.parent(c).unwrap()
            )
        );
        assert_eq!(world.snapshot(b).unwrap().name, None);
    }

    #[test]
    fn local_change_propagates_to_all_descendants() {
        let root = EntityId::new();
        let child = EntityId::new();
        let grandchild = EntityId::new();
        let mut world = SceneWorld::new();
        world
            .apply_commands(&[
                WorldCommand::Spawn(Box::new(entity(root, None))),
                WorldCommand::Spawn(Box::new(entity(child, Some(root)))),
                WorldCommand::Spawn(Box::new(entity(grandchild, Some(child)))),
            ])
            .unwrap();
        assert_eq!(world.propagate_transforms(), 3);
        world
            .apply_commands(&[WorldCommand::SetLocalTransform {
                entity: root,
                value: LocalTransform {
                    translation: Vec3::X,
                    ..LocalTransform::IDENTITY
                },
            }])
            .unwrap();
        assert_eq!(world.propagate_transforms(), 3);
        assert_eq!(
            world
                .world_transform(grandchild)
                .unwrap()
                .0
                .transform_point3(Vec3::ZERO),
            Vec3::X
        );
        assert_eq!(world.propagate_transforms(), 0);
    }

    #[test]
    fn folders_do_not_affect_descendant_world_transforms() {
        let folder = EntityId::new();
        let child = EntityId::new();
        let mut folder_snapshot = entity(folder, None);
        folder_snapshot.folder = true;
        folder_snapshot.local_transform.translation = Vec3::new(100.0, 200.0, 300.0);
        let mut child_snapshot = entity(child, Some(folder));
        child_snapshot.local_transform.translation = Vec3::new(1.0, 2.0, 3.0);
        let mut world = SceneWorld::new();
        world
            .apply_commands(&[
                WorldCommand::Spawn(Box::new(folder_snapshot)),
                WorldCommand::Spawn(Box::new(child_snapshot)),
            ])
            .unwrap();

        world.propagate_transforms();
        assert_eq!(
            world
                .world_transform(child)
                .unwrap()
                .0
                .transform_point3(Vec3::ZERO),
            Vec3::new(1.0, 2.0, 3.0)
        );
    }

    #[test]
    fn named_paths_and_part_attributes_round_trip_through_world_commands() {
        let root = EntityId::new();
        let child = EntityId::new();
        let mut root_snapshot = entity(root, None);
        root_snapshot.name = Some("Room".into());
        let mut child_snapshot = entity(child, Some(root));
        child_snapshot.name = Some("Table".into());
        let mut world = SceneWorld::new();
        world
            .apply_commands(&[
                WorldCommand::Spawn(Box::new(root_snapshot)),
                WorldCommand::Spawn(Box::new(child_snapshot)),
            ])
            .unwrap();
        assert_eq!(
            world.find_path("Game.scene.Room.Table").unwrap(),
            Some(child)
        );
        assert_eq!(world.list_path("Room").unwrap(), vec![child]);
        assert_eq!(world.entity_path(child).unwrap(), "Room.Table");

        let attributes = PartAttributes {
            color: [0.1, 0.2, 0.3, 1.0],
            can_touch: false,
            can_collide: true,
            anchored: true,
            friction: 0.5,
            visibility_layers: u32::MAX,
        };
        world
            .apply_commands(&[WorldCommand::SetPartAttributes {
                entity: child,
                value: attributes,
            }])
            .unwrap();
        assert_eq!(world.snapshot(child).unwrap().part_attributes, attributes);
    }
    #[test]
    fn composite_parameters_are_atomic_and_survive_track_deletion_and_reload() {
        use crate::effects::*;
        let id = crate::AssetId::new();
        let mut asset = EffectAsset::new(
            "bound",
            vec![EffectComponent::default(), EffectComponent::default()],
        );
        asset
            .parameters
            .insert("power".into(), EffectParameter::Scalar(2.0));
        asset.bindings.push(GroupParameterBinding {
            parameter: "power".into(),
            child: 1,
            property_path: "common.time_scale".into(),
            scale: 1.0,
            offset: 0.0,
        });
        let mut world = SceneWorld::new();
        world.register_effect_asset(id, asset.clone()).unwrap();
        let root = world
            .instantiate_effect_asset(id, None, LocalTransform::default())
            .unwrap();
        let group = world.effect_group(root).unwrap().unwrap();
        assert_eq!(group.child_entities.len(), 2);
        let before = world.snapshot(root).unwrap();
        let mut invalid = group.clone();
        invalid.bindings[0].property_path = "kind.nonexistent".into();
        assert!(
            world
                .apply_commands(&[
                    WorldCommand::SetEffectGroup {
                        entity: root,
                        value: Some(Box::new(invalid))
                    },
                    WorldCommand::SetName {
                        entity: root,
                        value: Some("must not commit".into())
                    }
                ])
                .is_err()
        );
        assert_eq!(world.snapshot(root).unwrap(), before);
        world
            .apply_commands(&[WorldCommand::Despawn(group.child_entities[0])])
            .unwrap();
        let group = world.effect_group(root).unwrap().unwrap();
        assert_eq!(group.child_entities.len(), 1);
        assert_eq!(group.bindings[0].child, 0);
        asset
            .parameters
            .insert("power".into(), EffectParameter::Scalar(3.0));
        world.register_effect_asset(id, asset).unwrap();
        let group = world.effect_group(root).unwrap().unwrap();
        assert_eq!(group.bindings[0].child, 0);
        assert_eq!(group.parameters["power"], EffectParameter::Scalar(3.0));
        assert!(
            (world
                .effect(group.child_entities[0])
                .unwrap()
                .unwrap()
                .common
                .time_scale
                - 1.0)
                .abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn composite_timeline_loops_pause_and_complete_with_canonical_state() {
        use crate::effects::*;
        let mut effect = EffectComponent::default();
        effect.common.looping = false;
        effect.common.duration = 0.05;
        let mut asset = EffectAsset::new("clock", vec![effect]);
        asset.group_settings = Some(EffectGroupSettings {
            auto_play: true,
            looping: true,
            duration: 0.25,
            time_scale: 2.0,
        });
        let asset_id = crate::AssetId::new();
        let mut world = SceneWorld::new();
        world.register_effect_asset(asset_id, asset).unwrap();
        let root = world
            .instantiate_effect_asset(asset_id, None, LocalTransform::default())
            .unwrap();
        world.update_effects(0.05);
        assert_eq!(
            world.effect_state(root).unwrap(),
            Some(PlaybackState::Playing)
        );
        world.control_effect(root, PlaybackCommand::Pause).unwrap();
        world.update_effects(0.5);
        assert_eq!(
            world.effect_state(root).unwrap(),
            Some(PlaybackState::Paused)
        );
        world.control_effect(root, PlaybackCommand::Resume).unwrap();
        world.update_effects(0.1);
        assert_eq!(
            world.effect_state(root).unwrap(),
            Some(PlaybackState::Playing)
        );
        let mut group = world.effect_group(root).unwrap().unwrap();
        group.looping = false;
        world
            .apply_commands(&[WorldCommand::SetEffectGroup {
                entity: root,
                value: Some(Box::new(group)),
            }])
            .unwrap();
        world.update_effects(0.2);
        assert_eq!(
            world.effect_state(root).unwrap(),
            Some(PlaybackState::Completed)
        );
        world
            .control_effect(root, PlaybackCommand::Restart)
            .unwrap();
        assert_eq!(
            world.effect_state(root).unwrap(),
            Some(PlaybackState::Playing)
        );
    }
    #[test]
    fn sub_emitter_retains_composite_audio_light_bindings_and_owner_controls() {
        use crate::effects::*;
        let mut world = SceneWorld::new();
        let source = EntitySnapshot {
            effect: Some(EffectComponent::default()),
            ..Default::default()
        };
        let source_id = source.id;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(source))])
            .unwrap();
        let asset_id = crate::AssetId::new();
        let sound = crate::AssetId::new();
        let mut asset = EffectAsset::new("child", vec![EffectComponent::default()]);
        asset.transforms =
            vec![glam::Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2).to_cols_array()];
        asset.lights = vec![Some(Light::default())];
        asset.sound_cues.push(EffectSoundCue {
            asset: sound,
            delay: 0.0,
            volume: 1.0,
            pitch: 1.0,
            spatial: true,
            looping: false,
        });
        asset
            .parameters
            .insert("speed".into(), EffectParameter::Scalar(2.0));
        asset.bindings.push(GroupParameterBinding {
            parameter: "speed".into(),
            child: 0,
            property_path: "common.time_scale".into(),
            scale: 1.0,
            offset: 0.0,
        });
        world.register_effect_asset(asset_id, asset).unwrap();
        world.effects_runtime.emit_event(EffectEvent {
            entity: source_id,
            kind: EffectEventKind::Birth,
            position: Vec3::new(3.0, 4.0, 5.0),
            velocity: Vec3::X * 4.0,
            child_effect: Some(asset_id),
        });
        world.update_effect_children(0.0);
        let root = *world.temporary_effect_owners.keys().next().unwrap();
        let group = world.effect_group(root).unwrap().unwrap();
        assert_eq!(group.sound_cues[0].asset, sound);
        assert_eq!(group.bindings.len(), 1);
        let child = group.child_entities[0];
        assert!(world.light(child).unwrap().is_some());
        let EffectKind::Particle(emitter) = world.effect(child).unwrap().unwrap().kind else {
            panic!("particle child")
        };
        assert!(
            (glam::Vec3::from_array(emitter.initial_velocity) - Vec3::NEG_Y * 4.0).length()
                < 0.0001
        );
        world.update_effects(0.0);
        assert!(world.take_effect_audio_events().iter().any(|event| event.group == root && matches!(event.command, EffectAudioCommand::Play { asset, .. } if asset == sound)));
        world
            .control_effect(source_id, PlaybackCommand::Pause)
            .unwrap();
        assert_eq!(
            world.effect_group_state(root).unwrap(),
            Some(PlaybackState::Paused)
        );
        world
            .control_effect(source_id, PlaybackCommand::Resume)
            .unwrap();
        assert_eq!(
            world.effect_group_state(root).unwrap(),
            Some(PlaybackState::Playing)
        );
        world
            .apply_commands(&[WorldCommand::Despawn(source_id)])
            .unwrap();
        world.update_effect_children(0.0);
        assert!(!world.contains(root));
        assert!(!world.contains(child));
    }
    #[test]
    fn composite_completed_track_restarts_target_on_the_next_update() {
        use crate::effects::*;
        let mut source = EffectComponent::new(EffectKind::ScreenFlash(ScreenFlash::default()));
        source.common.looping = false;
        source.common.duration = 0.05;
        let mut target = EffectComponent::new(EffectKind::ScreenFlash(ScreenFlash::default()));
        target.common.auto_play = false;
        target.common.looping = false;
        let mut asset = EffectAsset::new("event tracks", vec![source, target]);
        asset.group_settings = Some(EffectGroupSettings {
            duration: 2.0,
            ..Default::default()
        });
        asset.trigger_bindings.push(GroupTriggerBinding {
            source_child: 0,
            event: CompositeTriggerEvent::Completed,
            target_child: 1,
            action: CompositeTriggerAction::Restart,
        });
        let mut world = SceneWorld::new();
        let asset_id = crate::AssetId::new();
        world.register_effect_asset(asset_id, asset).unwrap();
        let root = world
            .instantiate_effect_asset(asset_id, None, LocalTransform::default())
            .unwrap();
        let target = world.effect_group(root).unwrap().unwrap().child_entities[1];
        world.update_effects(0.1);
        assert_eq!(
            world.effect_state(target).unwrap(),
            Some(PlaybackState::Playing)
        );
        assert!(
            !world
                .effect_frame()
                .overlays
                .iter()
                .any(|overlay| overlay.entity == target)
        );
        world.update_effects(0.1);
        assert!(
            world
                .effect_frame()
                .overlays
                .iter()
                .any(|overlay| overlay.entity == target)
        );
    }
    #[test]
    fn temporary_composite_retains_replay_tracks_and_outlives_natural_owner_completion() {
        use crate::effects::*;
        let mut source_track =
            EffectComponent::new(EffectKind::ScreenFlash(ScreenFlash::default()));
        source_track.common.looping = false;
        source_track.common.duration = 0.05;
        let mut target_track =
            EffectComponent::new(EffectKind::ScreenFlash(ScreenFlash::default()));
        target_track.common.auto_play = false;
        target_track.common.looping = false;
        let mut child_asset =
            EffectAsset::new("temporary event tracks", vec![source_track, target_track]);
        child_asset.group_settings = Some(EffectGroupSettings {
            duration: 1.0,
            ..Default::default()
        });
        child_asset.trigger_bindings.push(GroupTriggerBinding {
            source_child: 0,
            event: CompositeTriggerEvent::Completed,
            target_child: 1,
            action: CompositeTriggerAction::Restart,
        });
        let mut owner_asset = EffectAsset::new("short owner", vec![EffectComponent::default()]);
        owner_asset.group_settings = Some(EffectGroupSettings {
            duration: 0.05,
            ..Default::default()
        });
        let mut world = SceneWorld::new();
        let child_asset_id = crate::AssetId::new();
        let owner_asset_id = crate::AssetId::new();
        world
            .register_effect_asset(child_asset_id, child_asset)
            .unwrap();
        world
            .register_effect_asset(owner_asset_id, owner_asset)
            .unwrap();
        let owner = world
            .instantiate_effect_asset(owner_asset_id, None, LocalTransform::default())
            .unwrap();
        let emitter = world.effect_group(owner).unwrap().unwrap().child_entities[0];
        world.effects_runtime.emit_event(EffectEvent {
            entity: emitter,
            kind: EffectEventKind::Birth,
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            child_effect: Some(child_asset_id),
        });
        world.update_effect_children(0.0);
        let temporary = *world.temporary_effect_owners.keys().next().unwrap();
        let tracks = world
            .effect_group(temporary)
            .unwrap()
            .unwrap()
            .child_entities;
        world.update_effects(0.1);
        assert_eq!(
            world.effect_group_state(owner).unwrap(),
            Some(PlaybackState::Completed)
        );
        assert!(tracks.iter().all(|track| world.contains(*track)));
        assert_eq!(
            world.effect_state(tracks[1]).unwrap(),
            Some(PlaybackState::Playing)
        );
        world.update_effects(0.1);
        assert!(
            world
                .effect_frame()
                .overlays
                .iter()
                .any(|overlay| overlay.entity == tracks[1])
        );
        world.control_effect(owner, PlaybackCommand::Stop).unwrap();
        world.update_effect_children(0.0);
        assert!(!world.contains(temporary));
        assert!(tracks.iter().all(|track| !world.contains(*track)));
    }
}
