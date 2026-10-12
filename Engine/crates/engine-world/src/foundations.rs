//! Runtime entity services. Authored scene formats remain unchanged.
use crate::{EntityId, LocalTransform, RuntimeEntityRef, SceneWorld, WorldCommand, WorldError};
use glam::{Mat4, Vec3};
use std::collections::{BTreeMap, BTreeSet};

const MAX_NAMES: usize = 4096;
const MAX_ENTITY_TAGS: usize = 128;

#[derive(Default)]
pub(crate) struct EntityServices {
    pub tags: BTreeMap<EntityId, BTreeSet<String>>,
    pub groups: BTreeMap<String, BTreeSet<EntityId>>,
    pub deadlines: BTreeMap<EntityId, (RuntimeEntityRef, f64)>,
}

fn name(value: &str) -> Result<(), WorldError> {
    if value.trim().is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(WorldError::InvalidArgument(
            "name must be 1..128 bytes without control characters".into(),
        ));
    }
    Ok(())
}

impl SceneWorld {
    /// Adds a case-sensitive runtime tag; repeated additions are idempotent.
    /// # Errors
    /// Rejects absent entities, invalid names and per-entity tag limits.
    pub fn add_tag(&mut self, entity: EntityId, tag: &str) -> Result<bool, WorldError> {
        self.reference(entity)?;
        name(tag)?;
        let tags = self.services.tags.entry(entity).or_default();
        if tags.len() >= MAX_ENTITY_TAGS && !tags.contains(tag) {
            return Err(WorldError::InvalidArgument(
                "entity tag limit reached".into(),
            ));
        }
        Ok(tags.insert(tag.into()))
    }

    /// Removes a runtime tag, returning whether it existed.
    /// # Errors
    /// Rejects absent entities and invalid names.
    pub fn remove_tag(&mut self, entity: EntityId, tag: &str) -> Result<bool, WorldError> {
        self.reference(entity)?;
        name(tag)?;
        Ok(self
            .services
            .tags
            .get_mut(&entity)
            .is_some_and(|tags| tags.remove(tag)))
    }

    /// Returns matching live entities in stable ID order.
    /// # Errors
    /// Rejects invalid tag names.
    pub fn find_by_tag(&self, tag: &str) -> Result<Vec<EntityId>, WorldError> {
        name(tag)?;
        Ok(self
            .services
            .tags
            .iter()
            .filter(|(_, tags)| tags.contains(tag))
            .map(|(id, _)| *id)
            .collect())
    }

    /// Creates an empty runtime group; repeated creation is idempotent.
    /// # Errors
    /// Rejects invalid names and the group count limit.
    pub fn create_group(&mut self, group: &str) -> Result<bool, WorldError> {
        name(group)?;
        if self.services.groups.contains_key(group) {
            return Ok(false);
        }
        if self.services.groups.len() >= MAX_NAMES {
            return Err(WorldError::InvalidArgument("group limit reached".into()));
        }
        self.services.groups.insert(group.into(), BTreeSet::new());
        Ok(true)
    }

    /// Adds a live entity to an existing group.
    /// # Errors
    /// Rejects absent entities and unknown groups.
    pub fn add_to_group(&mut self, group: &str, entity: EntityId) -> Result<bool, WorldError> {
        self.reference(entity)?;
        name(group)?;
        let members = self
            .services
            .groups
            .get_mut(group)
            .ok_or_else(|| WorldError::InvalidArgument("group does not exist".into()))?;
        Ok(members.insert(entity))
    }

    /// Returns a group's live members in stable ID order.
    /// # Errors
    /// Rejects unknown or invalid group names.
    pub fn group_members(&self, group: &str) -> Result<Vec<EntityId>, WorldError> {
        name(group)?;
        Ok(self
            .services
            .groups
            .get(group)
            .ok_or_else(|| WorldError::InvalidArgument("group does not exist".into()))?
            .iter()
            .copied()
            .collect())
    }

    /// Reparents using the existing command transaction. By default script bindings
    /// preserve world position, rotation and scale; non-TRS results are rejected.
    /// # Errors
    /// Rejects missing objects, cycles and transforms requiring shear.
    pub fn reparent(
        &mut self,
        child: EntityId,
        parent: Option<EntityId>,
        keep_world: bool,
    ) -> Result<(), WorldError> {
        self.reference(child)?;
        if let Some(parent) = parent {
            self.reference(parent)?;
        }
        self.propagate_transforms();
        let mut commands = vec![WorldCommand::SetParent { child, parent }];
        if keep_world {
            let world = self.world_transform(child)?.0;
            let local = match parent {
                Some(parent) => self.world_transform(parent)?.0.inverse() * world,
                None => world,
            };
            let (scale, rotation, translation) = local.to_scale_rotation_translation();
            let value = LocalTransform {
                translation,
                rotation,
                scale,
            };
            let reconstructed = Mat4::from_scale_rotation_translation(scale, rotation, translation);
            let tolerance = local
                .to_cols_array()
                .iter()
                .fold(1.0_f32, |m, v| m.max(v.abs()))
                * 1e-4;
            if !value.is_finite()
                || local
                    .to_cols_array()
                    .iter()
                    .zip(reconstructed.to_cols_array())
                    .any(|(a, b)| (*a - b).abs() > tolerance)
            {
                return Err(WorldError::InvalidArgument(
                    "world-preserving reparent requires a representable TRS transform (no shear)"
                        .into(),
                ));
            }
            commands.push(WorldCommand::SetLocalTransform {
                entity: child,
                value,
            });
        }
        self.apply_commands(&commands)?;
        self.propagate_transforms();
        Ok(())
    }

    /// Returns descendants in stable ID order, excluding the root.
    /// # Errors
    /// Rejects absent roots.
    pub fn descendants(&self, root: EntityId) -> Result<Vec<EntityId>, WorldError> {
        self.reference(root)?;
        let mut pending = vec![root];
        let mut result = Vec::new();
        while let Some(id) = pending.pop() {
            for &child in self.children(id)? {
                result.push(child);
                pending.push(child);
            }
        }
        result.sort_unstable();
        Ok(result)
    }

    /// Destroys an entire hierarchy through one validated batch; references become stale.
    /// # Errors
    /// Rejects absent entities or invalid world commands.
    pub fn destroy_entity(&mut self, entity: EntityId) -> Result<(), WorldError> {
        let mut ids = self.descendants(entity)?;
        ids.push(entity);
        self.apply_commands(
            &ids.into_iter()
                .map(WorldCommand::Despawn)
                .collect::<Vec<_>>(),
        )?;
        Ok(())
    }

    /// Schedules destruction on the scaled fixed simulation clock. Replaces a prior
    /// deadline; zero destroys immediately. Deadlines retain generational references.
    /// # Errors
    /// Rejects stale entities and negative/non-finite delays.
    pub fn destroy_after(&mut self, entity: EntityId, delay: f64) -> Result<(), WorldError> {
        let reference = self.reference(entity)?;
        if !delay.is_finite() || delay < 0.0 {
            return Err(WorldError::InvalidArgument(
                "delay must be finite and nonnegative".into(),
            ));
        }
        if delay == 0.0 {
            return self.destroy_entity(entity);
        }
        self.services.deadlines.insert(entity, (reference, delay));
        Ok(())
    }

    /// Advances runtime lifetime deadlines once per fixed step.
    /// # Errors
    /// Rejects invalid deltas and propagates destruction errors.
    pub fn tick_entity_lifetimes(&mut self, delta: f64) -> Result<(), WorldError> {
        if !delta.is_finite() || delta < 0.0 {
            return Err(WorldError::InvalidArgument(
                "delta must be finite and nonnegative".into(),
            ));
        }
        let mut due = Vec::new();
        for (&id, (reference, remaining)) in &mut self.services.deadlines {
            *remaining -= delta;
            if *remaining <= 0.0 {
                due.push((id, *reference));
            }
        }
        for (id, reference) in due {
            self.services.deadlines.remove(&id);
            if self.resolve(reference).is_ok() {
                self.destroy_entity(id)?;
            }
        }
        Ok(())
    }

    /// Finds entity origins within a radius (not a collider overlap).
    /// # Errors
    /// Rejects non-finite centers and negative/non-finite radii.
    pub fn find_in_radius(
        &mut self,
        center: Vec3,
        radius: f64,
    ) -> Result<Vec<EntityId>, WorldError> {
        if !center.is_finite() || !radius.is_finite() || radius < 0.0 {
            return Err(WorldError::InvalidArgument(
                "center and nonnegative radius must be finite".into(),
            ));
        }
        self.propagate_transforms();
        Ok(self
            .entity_ids()
            .filter(|id| {
                self.world_transform(*id).is_ok_and(|t| {
                    t.0.w_axis.truncate().as_dvec3().distance(center.as_dvec3()) <= radius
                })
            })
            .collect())
    }

    /// Lists present built-in component schemas, then script IDs (`script:<uuid>`).
    /// # Errors
    /// Rejects absent entities.
    pub fn component_names(&self, entity: EntityId) -> Result<Vec<String>, WorldError> {
        let s = self.snapshot(entity)?;
        let mut names: Vec<String> = ["rustic.transform", "rustic.part_attributes"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        for (present, schema) in [
            (s.name.is_some(), "rustic.name"),
            (s.folder, "rustic.folder"),
            (s.parent.is_some(), "rustic.parent"),
            (s.mesh.is_some(), "rustic.mesh"),
            (s.material.is_some(), "rustic.material"),
            (s.camera.is_some(), "rustic.camera"),
            (s.light.is_some(), "rustic.light"),
            (s.primitive.is_some(), "rustic.primitive"),
            (!s.scripts.is_empty(), "rustic.scripts"),
        ] {
            if present {
                names.push(schema.into());
            }
        }
        for script in s.scripts {
            names.push(format!("script:{}", script.script_id));
        }
        names.sort_unstable();
        names.dedup();
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntitySnapshot, SceneDocument};
    use glam::Quat;

    fn spawn(world: &mut SceneWorld, parent: Option<EntityId>, translation: Vec3) -> EntityId {
        let snapshot = EntitySnapshot {
            parent,
            local_transform: LocalTransform {
                translation,
                ..LocalTransform::IDENTITY
            },
            ..EntitySnapshot::default()
        };
        let id = snapshot.id;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(snapshot))])
            .unwrap();
        id
    }

    #[test]
    fn tags_groups_and_descendants_are_cleaned_for_every_despawn_path() {
        let mut world = SceneWorld::new();
        let root = spawn(&mut world, None, Vec3::ZERO);
        let child = spawn(&mut world, Some(root), Vec3::X);
        let reference = world.reference(child).unwrap();
        assert!(world.create_group("enemies").unwrap());
        assert!(!world.create_group("enemies").unwrap());
        for id in [root, child] {
            assert!(world.add_tag(id, "enemy").unwrap());
            assert!(!world.add_tag(id, "enemy").unwrap());
            world.add_to_group("enemies", id).unwrap();
        }
        assert_eq!(world.find_by_tag("enemy").unwrap().len(), 2);
        world.destroy_entity(root).unwrap();
        assert_eq!(world.entity_count(), 0);
        assert!(world.resolve(reference).is_err());
        assert!(world.find_by_tag("enemy").unwrap().is_empty());
        assert!(world.group_members("enemies").unwrap().is_empty());
        let id = spawn(&mut world, None, Vec3::ZERO);
        world.add_tag(id, "enemy").unwrap();
        world.add_to_group("enemies", id).unwrap();
        world.apply_commands(&[WorldCommand::Despawn(id)]).unwrap();
        assert!(world.find_by_tag("enemy").unwrap().is_empty());
        assert!(world.group_members("enemies").unwrap().is_empty());
    }

    #[test]
    fn hierarchy_preserves_world_pose_and_rejects_cycles_atomically() {
        let mut world = SceneWorld::new();
        let parent = spawn(&mut world, None, Vec3::new(10., 20., 30.));
        let child = spawn(&mut world, None, Vec3::new(1., 2., 3.));
        world.propagate_transforms();
        let original = world.world_transform(child).unwrap();
        world.reparent(child, Some(parent), true).unwrap();
        assert_eq!(world.world_transform(child).unwrap(), original);
        assert_eq!(
            world.local_transform(child).unwrap().translation,
            Vec3::new(-9., -18., -27.)
        );
        let before = world.local_transform(parent).unwrap();
        assert!(world.reparent(parent, Some(child), true).is_err());
        assert_eq!(world.local_transform(parent).unwrap(), before);
        assert_eq!(world.parent(parent).unwrap(), None);
        world.reparent(child, None, true).unwrap();
        assert_eq!(world.world_transform(child).unwrap(), original);
        world.reparent(child, Some(parent), false).unwrap();
        assert_eq!(
            world.local_transform(child).unwrap().translation,
            Vec3::new(1., 2., 3.)
        );
    }

    #[test]
    fn hierarchy_rejects_shear_without_mutating_parent_or_local_transform() {
        let mut world = SceneWorld::new();
        let parent = spawn(&mut world, None, Vec3::ZERO);
        let child = spawn(&mut world, None, Vec3::ZERO);
        world
            .apply_commands(&[
                WorldCommand::SetLocalTransform {
                    entity: parent,
                    value: LocalTransform {
                        scale: Vec3::new(2., 1., 1.),
                        ..LocalTransform::IDENTITY
                    },
                },
                WorldCommand::SetLocalTransform {
                    entity: child,
                    value: LocalTransform {
                        rotation: Quat::from_rotation_z(0.5),
                        ..LocalTransform::IDENTITY
                    },
                },
            ])
            .unwrap();
        let before = world.local_transform(child).unwrap();
        assert!(world.reparent(child, Some(parent), true).is_err());
        assert_eq!(world.local_transform(child).unwrap(), before);
        assert_eq!(world.parent(child).unwrap(), None);
    }

    #[test]
    fn deadlines_replace_pause_and_do_not_destroy_recreated_ids() {
        let mut world = SceneWorld::new();
        let id = spawn(&mut world, None, Vec3::ZERO);
        world.destroy_after(id, 1.).unwrap();
        world.destroy_after(id, 2.).unwrap();
        world.tick_entity_lifetimes(0.).unwrap();
        world.tick_entity_lifetimes(1.).unwrap();
        assert!(world.contains(id));
        world.apply_commands(&[WorldCommand::Despawn(id)]).unwrap();
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(EntitySnapshot {
                id,
                ..EntitySnapshot::default()
            }))])
            .unwrap();
        world.tick_entity_lifetimes(5.).unwrap();
        assert!(world.contains(id));
        world.destroy_after(id, 0.25).unwrap();
        world.tick_entity_lifetimes(0.25).unwrap();
        assert!(!world.contains(id));
        assert!(world.services.deadlines.is_empty());
    }

    #[test]
    fn radius_queries_use_propagated_world_origins_including_parents() {
        let mut world = SceneWorld::new();
        let parent = spawn(&mut world, None, Vec3::new(10., 0., 0.));
        let child = spawn(&mut world, Some(parent), Vec3::X);
        assert_eq!(
            world.find_in_radius(Vec3::new(11., 0., 0.), 0.).unwrap(),
            vec![child]
        );
        assert_eq!(
            world
                .find_in_radius(Vec3::new(11., 0., 0.), 1.)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn invalid_inputs_are_rejected_and_runtime_metadata_is_not_serialized() {
        let mut world = SceneWorld::new();
        let id = spawn(&mut world, None, Vec3::ZERO);
        for tag in ["", "  ", "bad\ntag"] {
            assert!(world.add_tag(id, tag).is_err());
        }
        assert!(world.add_tag(EntityId::new(), "enemy").is_err());
        assert!(world.add_to_group("unknown", id).is_err());
        assert!(world.find_in_radius(Vec3::ZERO, -1.).is_err());
        assert!(world.find_in_radius(Vec3::NAN, 1.).is_err());
        for delay in [-1., f64::NAN, f64::INFINITY] {
            assert!(world.destroy_after(id, delay).is_err());
        }
        assert!(world.tick_entity_lifetimes(-1.).is_err());
        let before = SceneDocument::from_world(crate::SceneId::new(), "test", &world).unwrap();
        world.add_tag(id, "runtime").unwrap();
        world.create_group("runtime").unwrap();
        world.destroy_after(id, 10.).unwrap();
        let after = SceneDocument::from_world(before.id, "test", &world).unwrap();
        assert_eq!(before, after);
        assert_eq!(
            world.component_names(id).unwrap(),
            vec!["rustic.part_attributes", "rustic.transform"]
        );
        for index in 0..MAX_ENTITY_TAGS - 1 {
            world.add_tag(id, &format!("tag{index}")).unwrap();
        }
        assert!(world.add_tag(id, "overflow").is_err());
        assert!(world.remove_tag(id, "runtime").unwrap());
        assert!(!world.remove_tag(id, "runtime").unwrap());
    }
}
