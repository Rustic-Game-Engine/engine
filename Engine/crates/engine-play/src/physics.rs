//! Runtime-only translational physics for authored primitives.
use engine_world::{EntityId, Primitive, SceneWorld, WorldCommand, WorldError};
use glam::Vec3;
use std::collections::BTreeMap;

const GRAVITY: f32 = -9.81;
const SKIN: f32 = 0.025;

#[derive(Default)]
pub(crate) struct PhysicsWorld {
    velocities: BTreeMap<EntityId, Vec3>,
}

struct Body {
    id: EntityId,
    start: Vec3,
    original: Vec3,
    center: Vec3,
    half: Vec3,
    velocity: Vec3,
    dynamic: bool,
    solid: bool,
}

impl PhysicsWorld {
    pub fn colliders(
        world: &mut SceneWorld,
    ) -> Result<Vec<engine_core::gameplay::physics::Collider>, String> {
        world.propagate_transforms();
        let mut result = Vec::new();
        for id in world.entity_ids() {
            let Some(primitive) = world.primitive(id).map_err(|e| e.to_string())? else {
                continue;
            };
            if !world
                .part_attributes(id)
                .map_err(|e| e.to_string())?
                .can_collide
            {
                continue;
            }
            let (min, max) = bounds(primitive);
            let m = world.world_transform(id).map_err(|e| e.to_string())?.0;
            let center = m.transform_point3((min + max) * 0.5);
            let h = (max - min) * 0.5;
            let half = m.x_axis.truncate().abs() * h.x
                + m.y_axis.truncate().abs() * h.y
                + m.z_axis.truncate().abs() * h.z;
            result.push(engine_core::gameplay::physics::Collider {
                entity: id,
                center: center.as_dvec3().to_array(),
                half: half.as_dvec3().to_array(),
            });
        }
        Ok(result)
    }
    pub fn impulse(
        &mut self,
        world: &SceneWorld,
        id: EntityId,
        impulse: [f64; 3],
    ) -> Result<(), String> {
        let v = glam::DVec3::from_array(impulse).as_vec3();
        if !v.is_finite() {
            return Err("impulse must be finite and representable".into());
        }
        if world.primitive(id).map_err(|e| e.to_string())?.is_none() {
            return Err("physics requires an authored primitive collider".into());
        }
        if world
            .part_attributes(id)
            .map_err(|e| e.to_string())?
            .anchored
        {
            return Err("cannot impulse an anchored body".into());
        }
        let velocity = self.velocities.entry(id).or_default();
        let next = *velocity + v;
        if !next.is_finite() {
            return Err("velocity overflow".into());
        }
        *velocity = next;
        Ok(())
    }
    pub fn launch(
        &mut self,
        world: &SceneWorld,
        id: EntityId,
        velocity: [f64; 3],
    ) -> Result<(), String> {
        self.impulse(world, id, velocity)?;
        self.velocities
            .insert(id, glam::DVec3::from_array(velocity).as_vec3());
        Ok(())
    }
    pub fn step(&mut self, world: &mut SceneWorld, delta: f64) -> Result<(), WorldError> {
        if !delta.is_finite() || delta <= 0.0 {
            return Ok(());
        }
        world.propagate_transforms();
        let mut bodies = Vec::new();
        for id in world.entity_ids() {
            let Some(primitive) = world.primitive(id)? else {
                continue;
            };
            let (min, max) = bounds(primitive);
            let matrix = world.world_transform(id)?.0;
            let center = matrix.transform_point3((min + max) * 0.5);
            let half = (max - min) * 0.5;
            let half = matrix.x_axis.truncate().abs() * half.x
                + matrix.y_axis.truncate().abs() * half.y
                + matrix.z_axis.truncate().abs() * half.z;
            let attributes = world.part_attributes(id)?;
            bodies.push(Body {
                id,
                start: center,
                original: center,
                center,
                half: half.max(Vec3::splat(SKIN)),
                velocity: if attributes.anchored {
                    Vec3::ZERO
                } else {
                    self.velocities.get(&id).copied().unwrap_or(Vec3::ZERO)
                },
                dynamic: !attributes.anchored,
                solid: attributes.can_collide,
            });
        }
        // Bound large caller intervals; normal runtime steps are 1/60 second.
        let dt = delta.min(0.1) as f32 / 4.0;
        for _ in 0..4 {
            for body in &mut bodies {
                body.start = body.center;
                if body.dynamic {
                    body.velocity.y = (body.velocity.y + GRAVITY * dt).max(-100.0);
                    body.center += body.velocity * dt;
                }
            }
            // Sweep before resolving overlaps so fast falls cannot cross thin floors.
            for i in 0..bodies.len() {
                let (left, right) = bodies.split_at_mut(i + 1);
                for b in right {
                    let a = &mut left[i];
                    if !a.solid || !b.solid || (!a.dynamic && !b.dynamic) {
                        continue;
                    }
                    if let Some((time, normal)) = sweep(a, b) {
                        let relative_move = (a.center - a.start) - (b.center - b.start);
                        let correction = normal * (-relative_move.dot(normal) * (1.0 - time));
                        separate(a, b, correction, normal);
                    }
                }
            }
            // Sequential impulses settle equal-mass stacks and initial intersections.
            for _ in 0..12 {
                for i in 0..bodies.len() {
                    let (left, right) = bodies.split_at_mut(i + 1);
                    for b in right {
                        let a = &mut left[i];
                        if !a.solid || !b.solid || (!a.dynamic && !b.dynamic) {
                            continue;
                        }
                        let distance = a.center - b.center;
                        let overlap = a.half + b.half - distance.abs();
                        if overlap.min_element() <= 0.0 {
                            continue;
                        }
                        let axis = if overlap.y <= overlap.x && overlap.y <= overlap.z {
                            1
                        } else if overlap.x <= overlap.z {
                            0
                        } else {
                            2
                        };
                        let mut normal = Vec3::ZERO;
                        normal[axis] = if distance[axis] >= 0.0 { 1.0 } else { -1.0 };
                        separate(a, b, normal * overlap[axis], normal);
                    }
                }
            }
        }
        self.velocities.clear();
        // Parents must be written first, then convert each absolute target through
        // the updated parent matrix. Otherwise moving parents double-move children.
        let mut targets = Vec::new();
        for body in bodies {
            if !body.dynamic {
                continue;
            }
            self.velocities.insert(body.id, body.velocity);
            let mut depth = 0;
            let mut parent = world.parent(body.id)?;
            while let Some(id) = parent {
                depth += 1;
                parent = world.parent(id)?;
            }
            let translation =
                world.world_transform(body.id)?.0.w_axis.truncate() + body.center - body.original;
            targets.push((depth, body.id, translation));
        }
        targets.sort_by_key(|&(depth, id, _)| (depth, id));
        for (_, id, translation) in targets {
            world.propagate_transforms();
            let mut local = world.local_transform(id)?;
            local.translation = match world.parent(id)? {
                Some(parent) => world
                    .world_transform(parent)?
                    .0
                    .inverse()
                    .transform_point3(translation),
                None => translation,
            };
            world.apply_commands(&[WorldCommand::SetLocalTransform {
                entity: id,
                value: local,
            }])?;
        }
        world.propagate_transforms();
        Ok(())
    }
}

fn separate(a: &mut Body, b: &mut Body, correction: Vec3, normal: Vec3) {
    let wa = if a.dynamic { 1.0 } else { 0.0 };
    let wb = if b.dynamic { 1.0 } else { 0.0 };
    let total = wa + wb;
    a.center += correction * (wa / total);
    b.center -= correction * (wb / total);
    let closing = (a.velocity - b.velocity).dot(normal);
    if closing < 0.0 {
        a.velocity -= normal * (closing * wa / total);
        b.velocity += normal * (closing * wb / total);
    }
}

fn sweep(a: &Body, b: &Body) -> Option<(f32, Vec3)> {
    let origin = a.start - b.start;
    let half = a.half + b.half;
    if (half - origin.abs()).min_element() > 0.0 {
        return None;
    }
    let movement = (a.center - a.start) - (b.center - b.start);
    let mut enter = f32::NEG_INFINITY;
    let mut exit = f32::INFINITY;
    let mut normal = Vec3::ZERO;
    for axis in 0..3 {
        if movement[axis].abs() < 1e-8 {
            if origin[axis].abs() >= half[axis] {
                return None;
            }
            continue;
        }
        let t1 = (-half[axis] - origin[axis]) / movement[axis];
        let t2 = (half[axis] - origin[axis]) / movement[axis];
        let near = t1.min(t2);
        if near > enter {
            enter = near;
            normal = Vec3::ZERO;
            normal[axis] = -movement[axis].signum();
        }
        exit = exit.min(t1.max(t2));
    }
    (enter >= 0.0 && enter <= 1.0 && enter <= exit).then_some((enter, normal))
}

fn bounds(primitive: &Primitive) -> (Vec3, Vec3) {
    let half = match primitive {
        Primitive::Cube { size } => Vec3::splat(*size * 0.5),
        Primitive::RectangularPrism { size } => Vec3::from_array(*size) * 0.5,
        Primitive::Plane { size } => Vec3::new(size[0] * 0.5, SKIN, size[1] * 0.5),
        Primitive::Sphere { radius, .. } => Vec3::splat(*radius),
        Primitive::Cylinder { radius, height, .. } | Primitive::Cone { radius, height, .. } => {
            Vec3::new(*radius, *height * 0.5, *radius)
        }
        Primitive::Capsule { radius, height, .. } => {
            Vec3::new(*radius, (*height * 0.5).max(*radius), *radius)
        }
        Primitive::Torus {
            major_radius,
            minor_radius,
            ..
        } => Vec3::new(
            major_radius + minor_radius,
            *minor_radius,
            major_radius + minor_radius,
        ),
        Primitive::Rectangle2d { size } => Vec3::new(size[0] * 0.5, size[1] * 0.5, SKIN),
        Primitive::Circle2d { radius, .. } => Vec3::new(*radius, *radius, SKIN),
        Primitive::Polygon2d { points } => {
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            for point in points {
                let p = Vec3::new(point[0], point[1], 0.0);
                min = min.min(p);
                max = max.max(p);
            }
            min.z = -SKIN;
            max.z = SKIN;
            return (min, max);
        }
    };
    (-half, half)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_world::{EntitySnapshot, LocalTransform};
    use glam::Quat;

    fn cube(world: &mut SceneWorld, position: Vec3, anchored: bool) -> EntityId {
        let mut entity = EntitySnapshot::default();
        entity.primitive = Some(Primitive::Cube { size: 1.0 });
        entity.local_transform.translation = position;
        entity.part_attributes.anchored = anchored;
        let id = entity.id;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(entity))])
            .unwrap();
        id
    }

    fn tick(physics: &mut PhysicsWorld, world: &mut SceneWorld, count: usize) {
        for _ in 0..count {
            physics.step(world, 1.0 / 60.0).unwrap();
        }
    }

    #[test]
    fn gravity_and_anchoring() {
        let mut world = SceneWorld::new();
        let falling = cube(&mut world, Vec3::Y * 10.0, false);
        let fixed = cube(&mut world, Vec3::X * 4.0, true);
        let mut physics = PhysicsWorld::default();
        tick(&mut physics, &mut world, 60);
        assert!((world.local_transform(falling).unwrap().translation.y - 5.075).abs() < 0.03);
        assert_eq!(
            world.local_transform(fixed).unwrap().translation,
            Vec3::X * 4.0
        );
        let mut attributes = world.part_attributes(falling).unwrap();
        attributes.anchored = true;
        world
            .apply_commands(&[WorldCommand::SetPartAttributes {
                entity: falling,
                value: attributes,
            }])
            .unwrap();
        let position = world.local_transform(falling).unwrap().translation;
        tick(&mut physics, &mut world, 30);
        assert_eq!(
            world.local_transform(falling).unwrap().translation,
            position
        );
        attributes.anchored = false;
        world
            .apply_commands(&[WorldCommand::SetPartAttributes {
                entity: falling,
                value: attributes,
            }])
            .unwrap();
        tick(&mut physics, &mut world, 1);
        assert!(world.local_transform(falling).unwrap().translation.y > position.y - 0.01);
    }

    #[test]
    fn boxes_settle_in_a_stack() {
        let mut world = SceneWorld::new();
        cube(&mut world, Vec3::ZERO, true);
        let bottom = cube(&mut world, Vec3::Y * 3.0, false);
        let top = cube(&mut world, Vec3::Y * 5.0, false);
        tick(&mut PhysicsWorld::default(), &mut world, 300);
        assert!((world.local_transform(bottom).unwrap().translation.y - 1.0).abs() < 0.01);
        assert!((world.local_transform(top).unwrap().translation.y - 2.0).abs() < 0.01);
    }

    #[test]
    fn collision_flags_on_either_body_allow_pass_through() {
        for disable_floor in [false, true] {
            let mut world = SceneWorld::new();
            let floor = cube(&mut world, Vec3::ZERO, true);
            let falling = cube(&mut world, Vec3::Y * 3.0, false);
            let id = if disable_floor { floor } else { falling };
            let mut attributes = world.part_attributes(id).unwrap();
            attributes.can_collide = false;
            world
                .apply_commands(&[WorldCommand::SetPartAttributes {
                    entity: id,
                    value: attributes,
                }])
                .unwrap();
            tick(&mut PhysicsWorld::default(), &mut world, 90);
            assert!(world.local_transform(falling).unwrap().translation.y < -5.0);
        }
    }

    #[test]
    fn fast_fall_hits_thin_plane_and_touch_flag_does_not_disable_solids() {
        let mut world = SceneWorld::new();
        let mut floor = EntitySnapshot::default();
        floor.primitive = Some(Primitive::Plane { size: [20.0, 20.0] });
        floor.part_attributes.anchored = true;
        floor.part_attributes.can_touch = false;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(floor))])
            .unwrap();
        let falling = cube(&mut world, Vec3::Y * 0.8, false);
        let mut physics = PhysicsWorld::default();
        physics.velocities.insert(falling, Vec3::Y * -100.0);
        tick(&mut physics, &mut world, 60);
        assert!((world.local_transform(falling).unwrap().translation.y - 0.525).abs() < 0.001);
    }

    #[test]
    fn world_gravity_respects_rotated_scaled_parent() {
        let mut world = SceneWorld::new();
        let mut parent = EntitySnapshot::default();
        parent.local_transform = LocalTransform {
            translation: Vec3::new(5.0, 10.0, 0.0),
            rotation: Quat::from_rotation_z(0.7),
            scale: Vec3::splat(2.0),
        };
        let parent_id = parent.id;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(parent))])
            .unwrap();
        let child = cube(&mut world, Vec3::ZERO, false);
        world
            .apply_commands(&[WorldCommand::SetParent {
                child,
                parent: Some(parent_id),
            }])
            .unwrap();
        tick(&mut PhysicsWorld::default(), &mut world, 60);
        let position = world.world_transform(child).unwrap().0.w_axis.truncate();
        assert!((position.x - 5.0).abs() < 0.001);
        assert!((position.y - 5.075).abs() < 0.03);
    }

    #[test]
    fn scaled_floor_and_2d_platform_use_authored_dimensions() {
        for is_2d in [false, true] {
            let mut world = SceneWorld::new();
            let mut floor = EntitySnapshot::default();
            floor.primitive = Some(if is_2d {
                Primitive::Rectangle2d { size: [10.0, 1.0] }
            } else {
                Primitive::RectangularPrism {
                    size: [10.0, 1.0, 10.0],
                }
            });
            floor.local_transform.scale = Vec3::splat(2.0);
            floor.part_attributes.anchored = true;
            world
                .apply_commands(&[WorldCommand::Spawn(Box::new(floor))])
                .unwrap();
            let falling = cube(&mut world, Vec3::Y * 4.0, false);
            tick(&mut PhysicsWorld::default(), &mut world, 120);
            assert!((world.local_transform(falling).unwrap().translation.y - 1.5).abs() < 0.001);
        }
    }

    #[test]
    fn fast_dynamic_bodies_collide_without_crossing() {
        let mut world = SceneWorld::new();
        let left = cube(&mut world, Vec3::X * -1.0, false);
        let right = cube(&mut world, Vec3::X, false);
        let mut physics = PhysicsWorld::default();
        physics.velocities.insert(left, Vec3::X * 100.0);
        physics.velocities.insert(right, Vec3::X * -100.0);
        tick(&mut physics, &mut world, 1);
        let a = world.local_transform(left).unwrap().translation;
        let b = world.local_transform(right).unwrap().translation;
        assert!((b.x - a.x - 1.0).abs() < 0.001);
        assert!(physics.velocities[&left].x.abs() < 0.001);
        assert!(physics.velocities[&right].x.abs() < 0.001);
    }

    #[test]
    fn moving_parent_does_not_double_apply_child_gravity() {
        let mut world = SceneWorld::new();
        let parent = cube(&mut world, Vec3::Y * 10.0, false);
        let child = cube(&mut world, Vec3::X * 3.0, false);
        world
            .apply_commands(&[WorldCommand::SetParent {
                child,
                parent: Some(parent),
            }])
            .unwrap();
        tick(&mut PhysicsWorld::default(), &mut world, 60);
        let parent_pos = world.world_transform(parent).unwrap().0.w_axis.truncate();
        let child_pos = world.world_transform(child).unwrap().0.w_axis.truncate();
        assert!((child_pos - parent_pos - Vec3::X * 3.0).length() < 0.001);
    }

    #[test]
    fn removed_bodies_and_non_physical_nodes_have_no_velocity() {
        let mut world = SceneWorld::new();
        let id = cube(&mut world, Vec3::ZERO, false);
        let empty = EntitySnapshot::default();
        let empty_id = empty.id;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(empty))])
            .unwrap();
        let mut physics = PhysicsWorld::default();
        tick(&mut physics, &mut world, 1);
        world.apply_commands(&[WorldCommand::Despawn(id)]).unwrap();
        tick(&mut physics, &mut world, 1);
        assert!(physics.velocities.is_empty());
        assert_eq!(
            world.local_transform(empty_id).unwrap(),
            LocalTransform::IDENTITY
        );
    }
}
