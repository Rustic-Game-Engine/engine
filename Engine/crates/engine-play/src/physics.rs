//! Runtime-only translational physics for authored primitives.
use engine_world::{EntityId, Primitive, SceneWorld, WorldCommand, WorldError};
use glam::Vec3;
use std::collections::BTreeMap;

const GRAVITY: f32 = -9.81;
const SKIN: f32 = 0.025;

#[derive(Default)]
pub(crate) struct PhysicsWorld {
    velocities: BTreeMap<EntityId, Vec3>,
    contacts: BTreeMap<(EntityId, EntityId), Vec3>,
    pub events: Vec<ContactEvent>,
    notified: BTreeMap<(EntityId, EntityId), Vec3>,
}

pub(crate) struct ContactEvent {
    pub entity: EntityId,
    pub other: EntityId,
    pub name: &'static str,
    pub normal: Vec3,
}

struct Body {
    id: EntityId,
    start: Vec3,
    original: Vec3,
    center: Vec3,
    half: Vec3,
    velocity: Vec3,
    axes: [Vec3; 3],
    friction: f32,
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
    pub fn cast(
        world: &mut SceneWorld,
        origin: [f64; 3],
        direction: [f64; 3],
        distance: f64,
        radius: f64,
        ignore: &[EntityId],
    ) -> Result<Option<engine_core::gameplay::physics::Hit>, String> {
        use engine_core::gameplay::physics::{Collider, cast};
        // Validate even an empty scene.
        cast(&[], origin, direction, distance, radius, ignore)?;
        world.propagate_transforms();
        let mut best: Option<engine_core::gameplay::physics::Hit> = None;
        for id in world.entity_ids() {
            if ignore.contains(&id)
                || !world
                    .part_attributes(id)
                    .map_err(|e| e.to_string())?
                    .can_collide
            {
                continue;
            }
            let Some(primitive) = world.primitive(id).map_err(|e| e.to_string())? else {
                continue;
            };
            let (min, max) = bounds(primitive);
            let (center, half, axes) = shape(
                world.world_transform(id).map_err(|e| e.to_string())?.0,
                min,
                max,
            );
            let basis =
                glam::DMat3::from_cols(axes[0].as_dvec3(), axes[1].as_dvec3(), axes[2].as_dvec3());
            let local_origin =
                basis.transpose() * (glam::DVec3::from_array(origin) - center.as_dvec3());
            let local_direction = basis.transpose() * glam::DVec3::from_array(direction);
            let collider = Collider {
                entity: id,
                center: [0.0; 3],
                half: half.as_dvec3().to_array(),
            };
            if let Some(mut hit) = cast(
                &[collider],
                local_origin.to_array(),
                local_direction.to_array(),
                distance,
                radius,
                &[],
            )? && best.as_ref().is_none_or(|old| hit.distance < old.distance)
            {
                hit.point =
                    (basis * glam::DVec3::from_array(hit.point) + center.as_dvec3()).to_array();
                hit.normal = (basis * glam::DVec3::from_array(hit.normal)).to_array();
                best = Some(hit);
            }
        }
        Ok(best)
    }
    pub fn ground(&self, id: EntityId, max_angle: f64) -> Result<serde_json::Value, String> {
        if !max_angle.is_finite() || !(0.0..=90.0).contains(&max_angle) {
            return Err("maximum slope must be between 0 and 90 degrees".into());
        }
        let mut support: Option<(EntityId, Vec3)> = None;
        for (&(a, b), &normal) in &self.contacts {
            let (other, n) = if a == id {
                (b, normal)
            } else if b == id {
                (a, -normal)
            } else {
                continue;
            };
            if n.y > 0.0 && support.is_none_or(|(_, old)| n.y > old.y) {
                support = Some((other, n));
            }
        }
        Ok(support.map_or(serde_json::Value::Null, |(entity, normal)| {
            let angle = f64::from(normal.y.clamp(-1.0, 1.0).acos().to_degrees());
            serde_json::json!({"entity":entity,"normal":normal.to_array(),"angle":angle,"walkable":angle <= max_angle})
        }))
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
        if !glam::DVec3::from_array(velocity).as_vec3().is_finite() {
            return Err("velocity must be finite and representable".into());
        }
        self.impulse(world, id, [0.0; 3])?;
        self.velocities
            .insert(id, glam::DVec3::from_array(velocity).as_vec3());
        Ok(())
    }
    #[allow(
        clippy::too_many_lines,
        reason = "keep sweep, contact resolution, and transform updates in their integration order"
    )]
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
            let (center, half, axes) = shape(matrix, min, max);
            let attributes = world.part_attributes(id)?;
            bodies.push(Body {
                id,
                start: center,
                original: center,
                center,
                half,
                axes,
                friction: attributes.friction,
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
        #[allow(
            clippy::cast_possible_truncation,
            reason = "finite positive delta is clamped to 0.1 before converting to simulation precision"
        )]
        let dt = delta.min(0.1) as f32 / 4.0;
        let mut contacts = BTreeMap::new();
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
                        record_contact(&mut contacts, a, b, normal);
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
                        if let Some((depth, normal)) = penetration(a, b) {
                            record_contact(&mut contacts, a, b, normal);
                            separate(a, b, normal * depth.max(0.0), normal);
                        }
                    }
                }
            }
        }
        self.events.clear();
        let notified: BTreeMap<_, _> = contacts
            .iter()
            .filter(|((a, b), _)| {
                world.part_attributes(*a).is_ok_and(|p| p.can_touch)
                    && world.part_attributes(*b).is_ok_and(|p| p.can_touch)
            })
            .map(|(pair, normal)| (*pair, *normal))
            .collect();
        for (&(a, b), &normal) in &notified {
            let name = if self.notified.contains_key(&(a, b)) {
                "CollisionStay"
            } else {
                "CollisionEnter"
            };
            self.events.push(ContactEvent {
                entity: a,
                other: b,
                name,
                normal,
            });
            self.events.push(ContactEvent {
                entity: b,
                other: a,
                name,
                normal: -normal,
            });
        }
        for (&(a, b), &normal) in &self.notified {
            if !notified.contains_key(&(a, b)) {
                self.events.push(ContactEvent {
                    entity: a,
                    other: b,
                    name: "CollisionExit",
                    normal,
                });
                self.events.push(ContactEvent {
                    entity: b,
                    other: a,
                    name: "CollisionExit",
                    normal: -normal,
                });
            }
        }
        self.contacts = contacts;
        self.notified = notified;
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
        let relative = a.velocity - b.velocity;
        let tangent = relative - normal * relative.dot(normal);
        let speed = tangent.length();
        if speed > 1e-8 {
            // Coulomb friction cannot reverse the tangential relative velocity.
            let coefficient = (a.friction * b.friction).sqrt();
            let impulse = tangent / speed * speed.min(-closing * coefficient);
            a.velocity -= impulse * (wa / total);
            b.velocity += impulse * (wb / total);
        }
    }
}

// SAT handles oriented boxes without introducing angular dynamics. Continuous
// SAT uses the same axes, so fast bodies cannot tunnel through rotated ramps.
fn axes(a: &Body, b: &Body) -> impl Iterator<Item = Vec3> {
    let cross: [Vec3; 9] = std::array::from_fn(|i| a.axes[i / 3].cross(b.axes[i % 3]));
    a.axes
        .into_iter()
        .chain(b.axes)
        .chain(cross.into_iter().filter_map(Vec3::try_normalize))
}
fn radius(body: &Body, axis: Vec3) -> f32 {
    body.axes
        .iter()
        .enumerate()
        .map(|(i, a)| a.dot(axis).abs() * body.half[i])
        .sum()
}
fn penetration(a: &Body, b: &Body) -> Option<(f32, Vec3)> {
    let distance = a.center - b.center;
    let mut best = (f32::INFINITY, Vec3::Y);
    for axis in axes(a, b) {
        let depth = radius(a, axis) + radius(b, axis) - distance.dot(axis).abs();
        if depth < -1e-5 {
            return None;
        }
        if depth < best.0 {
            best = (
                depth,
                axis * if distance.dot(axis) >= 0.0 { 1.0 } else { -1.0 },
            );
        }
    }
    Some(best)
}
fn record_contact(
    contacts: &mut BTreeMap<(EntityId, EntityId), Vec3>,
    a: &Body,
    b: &Body,
    normal: Vec3,
) {
    contacts.insert((a.id, b.id), normal);
}
fn sweep(a: &Body, b: &Body) -> Option<(f32, Vec3)> {
    let origin = a.start - b.start;
    let movement = (a.center - a.start) - (b.center - b.start);
    let mut enter = f32::NEG_INFINITY;
    let mut exit = f32::INFINITY;
    let mut normal = Vec3::ZERO;
    for axis in axes(a, b) {
        let half = radius(a, axis) + radius(b, axis);
        let position = origin.dot(axis);
        let speed = movement.dot(axis);
        if speed.abs() < 1e-8 {
            if position.abs() > half {
                return None;
            }
            continue;
        }
        let t1 = (-half - position) / speed;
        let t2 = (half - position) / speed;
        if t1.min(t2) > enter {
            enter = t1.min(t2);
            normal = -axis * speed.signum();
        }
        exit = exit.min(t1.max(t2));
    }
    ((0.0..=1.0).contains(&enter) && enter <= exit).then_some((enter, normal))
}

// A nonuniformly scaled, rotated parent can shear its children. Conservatively
// enclose sheared transforms; orthogonal transforms retain their slope surfaces.
fn shape(matrix: glam::Mat4, min: Vec3, max: Vec3) -> (Vec3, Vec3, [Vec3; 3]) {
    let center = matrix.transform_point3((min + max) * 0.5);
    let local_half = (max - min) * 0.5;
    let columns = [
        matrix.x_axis.truncate(),
        matrix.y_axis.truncate(),
        matrix.z_axis.truncate(),
    ];
    let axes = columns.map(|v| v.try_normalize().unwrap_or(Vec3::ZERO));
    if axes.iter().all(|v| v.length_squared() > 0.5)
        && axes[0].dot(axes[1]).abs() < 1e-4
        && axes[0].dot(axes[2]).abs() < 1e-4
        && axes[1].dot(axes[2]).abs() < 1e-4
    {
        (
            center,
            (local_half
                * Vec3::new(
                    columns[0].length(),
                    columns[1].length(),
                    columns[2].length(),
                ))
            .max(Vec3::splat(SKIN)),
            axes,
        )
    } else {
        (
            center,
            (columns[0].abs() * local_half.x
                + columns[1].abs() * local_half.y
                + columns[2].abs() * local_half.z)
                .max(Vec3::splat(SKIN)),
            [Vec3::X, Vec3::Y, Vec3::Z],
        )
    }
}

pub(crate) fn bounds(primitive: &Primitive) -> (Vec3, Vec3) {
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
#[allow(
    clippy::field_reassign_with_default,
    reason = "fixtures start from defaults and vary only the authored values under test"
)]
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

    fn floor(world: &mut SceneWorld, rotation: f32, friction: f32) -> EntityId {
        let mut e = EntitySnapshot::default();
        e.primitive = Some(Primitive::RectangularPrism {
            size: [40.0, 1.0, 20.0],
        });
        e.local_transform.rotation = Quat::from_rotation_z(rotation);
        e.part_attributes.anchored = true;
        e.part_attributes.friction = friction;
        let id = e.id;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(e))])
            .unwrap();
        id
    }
    fn friction(world: &mut SceneWorld, id: EntityId, value: f32) {
        let mut attributes = world.part_attributes(id).unwrap();
        attributes.friction = value;
        world
            .apply_commands(&[WorldCommand::SetPartAttributes {
                entity: id,
                value: attributes,
            }])
            .unwrap();
    }
    #[test]
    fn friction_stops_sliding_and_zero_friction_preserves_speed() {
        for coefficient in [0.0, 1.0] {
            let mut world = SceneWorld::new();
            floor(&mut world, 0.0, coefficient);
            let id = cube(&mut world, Vec3::Y, false);
            friction(&mut world, id, coefficient);
            let mut physics = PhysicsWorld::default();
            physics.launch(&world, id, [3.0, 0.0, 0.0]).unwrap();
            tick(&mut physics, &mut world, 60);
            let speed = physics.velocities[&id].x;
            if coefficient == 0.0 {
                assert!((speed - 3.0).abs() < 0.001);
            } else {
                assert!(speed.abs() < 0.001);
            }
        }
    }
    #[test]
    fn slopes_slide_or_hold_using_the_surface_normal() {
        for coefficient in [0.0, 1.0] {
            let mut world = SceneWorld::new();
            let angle = 0.4_f32;
            let ramp = floor(&mut world, angle, coefficient);
            let id = cube(&mut world, Vec3::Y * 3.0, false);
            friction(&mut world, id, coefficient);
            let mut physics = PhysicsWorld::default();
            tick(&mut physics, &mut world, 100);
            let ground = physics.ground(id, 30.0).unwrap();
            assert_eq!(ground["entity"], serde_json::json!(ramp));
            assert!(
                (ground["angle"].as_f64().unwrap() - f64::from(angle.to_degrees())).abs() < 0.01
            );
            assert_eq!(ground["walkable"], true);
            assert_eq!(physics.ground(id, 10.0).unwrap()["walkable"], false);
            if coefficient == 0.0 {
                assert!(physics.velocities[&id].x < -1.0);
            } else {
                assert!(physics.velocities[&id].length() < 0.01);
            }
        }
    }
    #[test]
    fn casts_hit_rotated_surfaces_and_respect_ignore() {
        let mut world = SceneWorld::new();
        let angle = 0.4_f32;
        let id = floor(&mut world, angle, 0.5);
        let hit = PhysicsWorld::cast(
            &mut world,
            [2.0, 10.0, 0.0],
            [0.0, -2.0, 0.0],
            20.0,
            0.0,
            &[],
        )
        .unwrap()
        .unwrap();
        assert_eq!(hit.entity, id);
        let expected_y = 2.0 * angle.tan() + 0.5 / angle.cos();
        assert!((hit.point[1] - f64::from(expected_y)).abs() < 0.001);
        assert!((hit.normal[0] + f64::from(angle.sin())).abs() < 0.001);
        assert!(
            PhysicsWorld::cast(
                &mut world,
                [2.0, 10.0, 0.0],
                [0.0, -1.0, 0.0],
                20.0,
                0.0,
                &[id]
            )
            .unwrap()
            .is_none()
        );
        let sphere = PhysicsWorld::cast(
            &mut world,
            [2.0, 10.0, 0.0],
            [0.0, -1.0, 0.0],
            20.0,
            0.25,
            &[],
        )
        .unwrap()
        .unwrap();
        assert!((hit.distance - sphere.distance - 0.25 / f64::from(angle.cos())).abs() < 0.001);
    }
    #[test]
    fn contact_events_are_paired_and_despawn_produces_exit() {
        let mut world = SceneWorld::new();
        let floor = floor(&mut world, 0.0, 0.5);
        let id = cube(&mut world, Vec3::Y, false);
        let mut physics = PhysicsWorld::default();
        tick(&mut physics, &mut world, 1);
        assert_eq!(physics.events.len(), 2);
        assert!(physics.events.iter().all(|e| e.name == "CollisionEnter"));
        assert!(
            physics
                .events
                .iter()
                .find(|e| e.entity == id)
                .unwrap()
                .normal
                .y
                > 0.99
        );
        tick(&mut physics, &mut world, 1);
        assert!(physics.events.iter().all(|e| e.name == "CollisionStay"));
        world
            .apply_commands(&[WorldCommand::Despawn(floor)])
            .unwrap();
        tick(&mut physics, &mut world, 1);
        assert!(
            physics
                .events
                .iter()
                .any(|e| e.entity == id && e.other == floor && e.name == "CollisionExit")
        );
        tick(&mut physics, &mut world, 1);
        assert!(physics.events.is_empty());
    }
    #[test]
    fn can_touch_controls_events_without_losing_ground_support() {
        let mut world = SceneWorld::new();
        floor(&mut world, 0.0, 0.5);
        let id = cube(&mut world, Vec3::Y, false);
        let mut attributes = world.part_attributes(id).unwrap();
        attributes.can_touch = false;
        world
            .apply_commands(&[WorldCommand::SetPartAttributes {
                entity: id,
                value: attributes,
            }])
            .unwrap();
        let mut physics = PhysicsWorld::default();
        tick(&mut physics, &mut world, 2);
        assert!(physics.events.is_empty());
        assert_eq!(physics.ground(id, 45.0).unwrap()["walkable"], true);
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
