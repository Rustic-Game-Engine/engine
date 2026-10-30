//! Viewport-normalized pointer input, shared by IPC and the script runtime.
use engine_core::gameplay::{Payload, Signal, Signals, Value};
use engine_world::{EntityId, RenderWorldBuffer, SceneWorld};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerDevice {
    Mouse,
    Touch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerPhase {
    Start,
    Move,
    End,
    Cancel,
}

/// IDs are strings so 64-bit platform touch IDs survive JavaScript transport.
/// Position is normalized to the displayed game image, with top-left [0, 0].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointerEvent {
    pub id: String,
    pub device: PointerDevice,
    pub phase: PointerPhase,
    pub position: [f32; 2],
    pub pressure: f32,
    pub aspect: f32,
}

#[derive(Clone, Debug, Serialize)]
pub struct ObjectClick {
    pub entity: EntityId,
    pub pointer: PointerEvent,
    pub point: [f64; 3],
}

#[derive(Default)]
pub(crate) struct PointerInput {
    pending: VecDeque<PointerEvent>,
    active: BTreeMap<(PointerDevice, String), PointerEvent>,
    pressed: BTreeMap<(PointerDevice, String), EntityId>,
    pub events: Vec<PointerEvent>,
    pub clicks: Vec<ObjectClick>,
}

impl PointerInput {
    pub fn enqueue(&mut self, events: Vec<PointerEvent>) {
        // Overflow cancels captures rather than leaving fingers permanently held.
        if events.len() > 1024 || self.pending.len() + events.len() > 1024 {
            self.reset();
            return;
        }
        self.pending.extend(events.into_iter().filter(|e| {
            !e.id.is_empty()
                && e.id.len() <= 64
                && e.position.iter().all(|v| v.is_finite())
                && e.pressure.is_finite()
                && (0.0..=1.0).contains(&e.pressure)
                && e.aspect.is_finite()
                && e.aspect > 0.0
        }));
    }

    pub fn reset(&mut self) {
        self.pending.clear();
        for mut event in self.active.values().cloned() {
            event.phase = PointerPhase::Cancel;
            self.pending.push_back(event);
        }
        self.pressed.clear();
        self.events.clear();
        self.clicks.clear();
    }

    pub fn touches(&self) -> Vec<&PointerEvent> {
        self.active
            .values()
            .filter(|e| e.device == PointerDevice::Touch)
            .collect()
    }

    pub fn begin_frame(&mut self, world: &mut SceneWorld, signals: &mut Signals) {
        self.events.clear();
        self.clicks.clear();
        world.propagate_transforms();
        for event in self.pending.drain(..) {
            let key = (event.device, event.id.clone());
            let hit = || pick(world, &event);
            match event.phase {
                PointerPhase::Start => {
                    if self.active.len() >= 64 && !self.active.contains_key(&key) {
                        continue;
                    }
                    self.pressed.remove(&key);
                    if let Some(hit) = hit() {
                        self.pressed.insert(key.clone(), hit.entity);
                    }
                    self.active.insert(key, event.clone());
                }
                PointerPhase::Move => {
                    let Some(active) = self.active.get_mut(&key) else {
                        continue;
                    };
                    *active = event.clone();
                }
                PointerPhase::End | PointerPhase::Cancel => {
                    if self.active.remove(&key).is_none() {
                        continue;
                    }
                    if let Some(target) = self.pressed.remove(&key)
                        && event.phase == PointerPhase::End
                        && let Some(hit) = hit()
                        && hit.entity == target
                    {
                        let _ = signals.emit(
                            &Signal {
                                name: "Click".into(),
                                source: Some(target),
                            },
                            arguments(&event),
                        );
                        self.clicks.push(ObjectClick {
                            entity: target,
                            pointer: event.clone(),
                            point: hit.point,
                        });
                    }
                }
            }
            if event.device == PointerDevice::Touch {
                let name = match event.phase {
                    PointerPhase::Start => "TouchStart",
                    PointerPhase::Move => "TouchMove",
                    PointerPhase::End => "TouchEnd",
                    PointerPhase::Cancel => "TouchCancel",
                };
                let _ = signals.emit(
                    &Signal {
                        name: name.into(),
                        source: None,
                    },
                    arguments(&event),
                );
            }
            self.events.push(event);
        }
    }
}

fn arguments(event: &PointerEvent) -> Vec<Payload> {
    vec![
        Payload::String(event.id.clone()),
        Value::Vector([
            f64::from(event.position[0]),
            f64::from(event.position[1]),
            0.0,
        ])
        .into(),
        Payload::String(
            match event.device {
                PointerDevice::Mouse => "mouse",
                PointerDevice::Touch => "touch",
            }
            .into(),
        ),
        Value::Number(f64::from(event.pressure)).into(),
    ]
}

fn pick(world: &SceneWorld, event: &PointerEvent) -> Option<engine_core::gameplay::physics::Hit> {
    if !event.position.iter().all(|v| (0.0..=1.0).contains(v)) {
        return None;
    }
    let mut buffer = RenderWorldBuffer::new();
    let rendered = buffer.extract(world);
    let camera = rendered.cameras.last()?;
    let inverse = camera.view_projection(event.aspect).inverse();
    let ndc = glam::Vec2::new(event.position[0] * 2.0 - 1.0, 1.0 - event.position[1] * 2.0);
    let near = inverse.project_point3(ndc.extend(0.0));
    let far = inverse.project_point3(ndc.extend(1.0));
    if !near.is_finite() || !far.is_finite() {
        return None;
    }
    let colliders = rendered
        .primitives
        .iter()
        .map(|p| {
            let (min, max) = crate::physics::bounds(&p.primitive);
            let m = glam::Mat4::from_cols_array(&p.transform);
            let h = (max - min) * 0.5;
            let half = m.x_axis.truncate().abs() * h.x
                + m.y_axis.truncate().abs() * h.y
                + m.z_axis.truncate().abs() * h.z;
            engine_core::gameplay::physics::Collider {
                entity: p.entity,
                center: m.transform_point3((min + max) * 0.5).as_dvec3().to_array(),
                half: half.as_dvec3().to_array(),
            }
        })
        .collect::<Vec<_>>();
    let hit = engine_core::gameplay::physics::cast(
        &colliders,
        near.as_dvec3().to_array(),
        (far - near).as_dvec3().to_array(),
        f64::from(near.distance(far)),
        0.0,
        &[],
    )
    .ok()??;
    // A non-touchable front object occludes objects behind it. CanCollide is independent.
    world
        .part_attributes(hit.entity)
        .ok()?
        .can_touch
        .then_some(hit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_world::{Camera, EntitySnapshot, Primitive, WorldCommand};
    pub(super) fn scene() -> (SceneWorld, EntityId) {
        let mut world = SceneWorld::new();
        let camera = EntitySnapshot {
            camera: Some(Camera::default()),
            ..EntitySnapshot::default()
        };
        let mut cube = EntitySnapshot {
            primitive: Some(Primitive::Cube { size: 2.0 }),
            ..EntitySnapshot::default()
        };
        cube.local_transform.translation.z = 5.0;
        cube.part_attributes.can_collide = false;
        let id = cube.id;
        world
            .apply_commands(&[
                WorldCommand::Spawn(Box::new(camera)),
                WorldCommand::Spawn(Box::new(cube)),
            ])
            .unwrap();
        (world, id)
    }
    fn event(id: &str, phase: PointerPhase) -> PointerEvent {
        PointerEvent {
            id: id.into(),
            device: PointerDevice::Touch,
            phase,
            position: [0.5, 0.5],
            pressure: 0.5,
            aspect: 16.0 / 9.0,
        }
    }
    fn set_touch(world: &mut SceneWorld, id: EntityId, enabled: bool) {
        let mut attrs = world.part_attributes(id).unwrap();
        attrs.can_touch = enabled;
        world
            .apply_commands(&[WorldCommand::SetPartAttributes {
                entity: id,
                value: attrs,
            }])
            .unwrap();
    }
    #[test]
    fn multitouch_and_mouse_clicks_are_once_per_frame_and_independent_of_collision() {
        let (mut world, target) = scene();
        let mut input = PointerInput::default();
        let mut signals = Signals::default();
        input.enqueue(vec![
            event("1", PointerPhase::Start),
            event("2", PointerPhase::Start),
        ]);
        input.begin_frame(&mut world, &mut signals);
        assert_eq!(input.touches().len(), 2);
        assert_eq!(input.events.len(), 2);
        input.enqueue(vec![
            event("2", PointerPhase::End),
            event("1", PointerPhase::Cancel),
        ]);
        input.begin_frame(&mut world, &mut signals);
        assert!(input.touches().is_empty());
        assert_eq!(input.clicks.len(), 1);
        assert_eq!(input.clicks[0].entity, target);
        assert_eq!(input.clicks[0].pointer.id, "2");
        input.begin_frame(&mut world, &mut signals);
        assert!(input.events.is_empty());
        assert!(input.clicks.is_empty());
        let mut start = event("mouse", PointerPhase::Start);
        start.device = PointerDevice::Mouse;
        let mut end = start.clone();
        end.phase = PointerPhase::End;
        input.enqueue(vec![start, end]);
        input.begin_frame(&mut world, &mut signals);
        assert_eq!(input.clicks.len(), 1);
        assert!(input.touches().is_empty());
    }
    #[test]
    fn can_touch_is_required_at_press_and_release_and_front_object_occludes() {
        let (mut world, target) = scene();
        let mut input = PointerInput::default();
        let mut signals = Signals::default();
        set_touch(&mut world, target, false);
        input.enqueue(vec![event("1", PointerPhase::Start)]);
        input.begin_frame(&mut world, &mut signals);
        set_touch(&mut world, target, true);
        input.enqueue(vec![event("1", PointerPhase::End)]);
        input.begin_frame(&mut world, &mut signals);
        assert!(input.clicks.is_empty());
        input.enqueue(vec![event("1", PointerPhase::Start)]);
        input.begin_frame(&mut world, &mut signals);
        set_touch(&mut world, target, false);
        input.enqueue(vec![event("1", PointerPhase::End)]);
        input.begin_frame(&mut world, &mut signals);
        assert!(input.clicks.is_empty());
        let mut behind = EntitySnapshot {
            primitive: Some(Primitive::Cube { size: 2.0 }),
            ..EntitySnapshot::default()
        };
        behind.local_transform.translation.z = 9.0;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(behind))])
            .unwrap();
        input.enqueue(vec![
            event("1", PointerPhase::Start),
            event("1", PointerPhase::End),
        ]);
        input.begin_frame(&mut world, &mut signals);
        assert!(input.clicks.is_empty());
    }
    #[test]
    fn cancellation_invalid_events_outside_release_and_missing_camera_do_not_click() {
        let (mut world, _) = scene();
        let mut input = PointerInput::default();
        let mut signals = Signals::default();
        input.enqueue(vec![event("1", PointerPhase::Start)]);
        input.begin_frame(&mut world, &mut signals);
        input.reset();
        input.begin_frame(&mut world, &mut signals);
        assert!(input.touches().is_empty());
        assert_eq!(input.events[0].phase, PointerPhase::Cancel);
        assert!(input.clicks.is_empty());
        let mut end = event("1", PointerPhase::End);
        end.position = [1.2, 0.5];
        input.enqueue(vec![event("1", PointerPhase::Start), end]);
        input.begin_frame(&mut world, &mut signals);
        assert!(input.clicks.is_empty());
        let mut invalid = event("bad", PointerPhase::Start);
        invalid.position[0] = f32::NAN;
        input.enqueue(vec![invalid, event("orphan", PointerPhase::End)]);
        input.begin_frame(&mut world, &mut signals);
        assert!(input.events.is_empty());
        let camera = world
            .entity_ids()
            .find(|id| world.camera(*id).unwrap().is_some())
            .unwrap();
        world
            .apply_commands(&[WorldCommand::Despawn(camera)])
            .unwrap();
        input.enqueue(vec![
            event("1", PointerPhase::Start),
            event("1", PointerPhase::End),
        ]);
        input.begin_frame(&mut world, &mut signals);
        assert!(input.clicks.is_empty());
    }
    #[test]
    fn camera_priority_and_orthographic_parent_transforms_match_rendering() {
        let (mut world, target) = scene();
        let parent = EntitySnapshot::default();
        let parent_id = parent.id;
        let mut camera = EntitySnapshot {
            camera: Some(Camera {
                order: 100,
                projection: engine_world::CameraProjection::Orthographic {
                    vertical_size: 10.0,
                    near: 0.1,
                    far: 100.0,
                },
                ..Camera::default()
            }),
            parent: Some(parent_id),
            ..EntitySnapshot::default()
        };
        camera.local_transform.translation.x = 10.0;
        world
            .apply_commands(&[
                WorldCommand::Spawn(Box::new(parent)),
                WorldCommand::Spawn(Box::new(camera)),
            ])
            .unwrap();
        let mut input = PointerInput::default();
        let mut signals = Signals::default();
        input.enqueue(vec![
            event("1", PointerPhase::Start),
            event("1", PointerPhase::End),
        ]);
        input.begin_frame(&mut world, &mut signals);
        assert!(input.clicks.is_empty());
        let mut transform = world.local_transform(target).unwrap();
        transform.translation.x = 10.0;
        world
            .apply_commands(&[WorldCommand::SetLocalTransform {
                entity: target,
                value: transform,
            }])
            .unwrap();
        input.enqueue(vec![
            event("1", PointerPhase::Start),
            event("1", PointerPhase::End),
        ]);
        input.begin_frame(&mut world, &mut signals);
        assert_eq!(input.clicks[0].entity, target);
    }
}
