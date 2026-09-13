use crate::components::{
    Camera, Children, Light, LocalTransform, Material, Mesh, Name, Parent, PartAttributes,
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

/// Live ECS world with stable identities, generational references, and a scene graph.
pub struct SceneWorld {
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
                self.ecs_entity(*id).ok().is_some_and(|entity| {
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
            let referenced = match command {
                WorldCommand::Spawn(_) => None,
                WorldCommand::Despawn(entity)
                | WorldCommand::SetLocalTransform { entity, .. }
                | WorldCommand::SetName { entity, .. }
                | WorldCommand::SetMesh { entity, .. }
                | WorldCommand::SetMaterial { entity, .. }
                | WorldCommand::SetCamera { entity, .. }
                | WorldCommand::SetLight { entity, .. }
                | WorldCommand::SetPrimitive { entity, .. }
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
                WorldCommand::SetName { entity, value } => {
                    self.insert_or_remove(*entity, value.clone().map(Name));
                }
                WorldCommand::SetMesh { entity, value } => self.insert_or_remove(*entity, *value),
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
            local_transform: self
                .ecs
                .get::<LocalTransform>(entity)
                .copied()
                .unwrap_or_default(),
            parent: self.ecs.get::<Parent>(entity).map(|value| value.0),
            mesh: self.ecs.get::<Mesh>(entity).copied(),
            material: self.ecs.get::<Material>(entity).copied(),
            camera: self.ecs.get::<Camera>(entity).copied(),
            light: self.ecs.get::<Light>(entity).copied(),
            primitive: self.ecs.get::<Primitive>(entity).cloned(),
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

    /// Returns the optional material component.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError::EntityNotFound`] when the entity is absent.
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
            let local = self
                .ecs
                .get::<LocalTransform>(entity)
                .copied()
                .unwrap_or_default();
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
        if let Some(mesh) = snapshot.mesh {
            entity.insert(mesh);
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
        };
        world
            .apply_commands(&[WorldCommand::SetPartAttributes {
                entity: child,
                value: attributes,
            }])
            .unwrap();
        assert_eq!(world.snapshot(child).unwrap().part_attributes, attributes);
    }
}
