use super::{
    CollisionResponse, EffectAppearance, EffectComponent, EffectEvent, EffectEventKind,
    EffectFrame, EffectGeometry, EffectInput, EffectKind, ForceField, ForceType, ParticleEmitter,
    ParticleInstance, PlaybackCommand, PlaybackState, PostProcessSettings, QualityLevel,
    RenderWaterDisturbance, RippleWaterSurface, SimulationSpace, SpawnShape, VolumeShape,
    WaterDisturbance, WeatherType, water_surface_mesh,
};
use crate::{EntityId, PrimitiveMesh};
use glam::{Mat4, Vec3, Vec3Swizzles};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::f32::consts::TAU;

const MAX_EVENTS: usize = 8192;

/// Builds a camera-facing ribbon or a parallel-transport tube without creating
/// one draw call for every segment. Input points and output positions use the
/// same coordinate space. Degenerate points are removed before tangent creation.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "validated cosmetic capacities and tessellation bounds keep numerical conversions in range"
)]
pub fn ribbon_mesh(
    points: &[Vec3],
    widths: &[f32],
    radial_segments: u16,
    camera: Vec3,
) -> PrimitiveMesh {
    let points: Vec<_> = points
        .iter()
        .copied()
        .filter(|point| point.is_finite())
        .enumerate()
        .filter(|(index, point)| *index == 0 || point.distance_squared(points[index - 1]) > 1.0e-10)
        .collect();
    let mut mesh = PrimitiveMesh::default();
    if points.len() < 2 {
        return mesh;
    }
    let sides = if radial_segments < 3 {
        2
    } else {
        radial_segments.min(32) as usize
    };
    let stride = if sides == 2 { 2 } else { sides + 1 };
    let mut distance = 0.0;
    let mut previous_normal = Vec3::X;
    for (index, (_, point)) in points.iter().enumerate() {
        let before = points[index.saturating_sub(1)].1;
        let after = points[(index + 1).min(points.len() - 1)].1;
        let tangent = (after - before).normalize_or_zero();
        let toward_camera = (camera - *point).normalize_or_zero();
        let mut normal = if sides == 2 {
            tangent.cross(toward_camera).normalize_or_zero()
        } else {
            (previous_normal - tangent * previous_normal.dot(tangent)).normalize_or_zero()
        };
        if normal.length_squared() < 0.5 {
            normal = tangent
                .cross(if tangent.y.abs() > 0.9 {
                    Vec3::X
                } else {
                    Vec3::Y
                })
                .normalize_or_zero();
        }
        previous_normal = normal;
        let binormal = tangent.cross(normal).normalize_or_zero();
        if index > 0 {
            distance += point.distance(points[index - 1].1);
        }
        let width = widths.get(points[index].0).copied().unwrap_or(1.0).max(0.0) * 0.5;
        for side in 0..stride {
            let (offset, surface_normal, v) = if sides == 2 {
                let sign = if side == 0 { -1.0 } else { 1.0 };
                (normal * width * sign, toward_camera, side as f32)
            } else {
                let phase = side as f32 / sides as f32;
                let radial = normal * (phase * TAU).cos() + binormal * (phase * TAU).sin();
                (radial * width, radial, phase)
            };
            mesh.positions.push((*point + offset).to_array());
            mesh.normals.push(surface_normal.to_array());
            mesh.tex_coords.push([distance, v]);
        }
        if index > 0 {
            for side in 0..(stride - 1) {
                let a = ((index - 1) * stride + side) as u32;
                let b = (index * stride + side) as u32;
                mesh.indices
                    .extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
            }
        }
    }
    mesh
}

/// Uniform Catmull-Rom interpolation preserving both endpoints. Subdivision and
/// input limits keep authored curves from producing unbounded frame allocations.
#[allow(
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    reason = "validated cosmetic capacities and tessellation bounds keep numerical conversions in range"
)]
pub fn smooth_path(points: &[Vec3], subdivisions: u16) -> Vec<Vec3> {
    if points.len() < 2 {
        return points.to_vec();
    }
    let subdivisions = subdivisions.clamp(1, 32) as usize;
    let count = points.len().min(2048);
    let mut result = Vec::with_capacity((count - 1) * subdivisions + 1);
    for segment in 0..count - 1 {
        let a = points[segment.saturating_sub(1)];
        let b = points[segment];
        let c = points[segment + 1];
        let d = points[(segment + 2).min(count - 1)];
        for step in 0..subdivisions {
            let t = step as f32 / subdivisions as f32;
            result.push(
                0.5 * ((2.0 * b)
                    + (-a + c) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t * t
                    + (-a + 3.0 * b - 3.0 * c + d) * t * t * t),
            );
        }
    }
    result.push(points[count - 1]);
    result
}

/// An annulus in the XZ plane with continuous UVs and upward normals.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "validated cosmetic capacities and tessellation bounds keep numerical conversions in range"
)]
pub fn ring_mesh(radius: f32, thickness: f32, segments: u16) -> PrimitiveMesh {
    let mut mesh = PrimitiveMesh::default();
    let segments = segments.clamp(8, 256) as usize;
    for index in 0..=segments {
        let u = index as f32 / segments as f32;
        let direction = Vec3::new((u * TAU).cos(), 0.0, (u * TAU).sin());
        for edge in 0..2 {
            let r = (radius + (edge as f32 - 0.5) * thickness).max(0.0);
            mesh.positions.push((direction * r).to_array());
            mesh.normals.push(Vec3::Y.to_array());
            mesh.tex_coords.push([u, edge as f32]);
        }
        if index > 0 {
            let a = ((index - 1) * 2) as u32;
            mesh.indices
                .extend_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
        }
    }
    mesh
}

/// Earliest segment contact with a solid axis-aligned box. Swept tests prevent
/// fast particles tunneling through thin cosmetic collision volumes.
pub fn sweep_box(start: Vec3, end: Vec3, half_size: Vec3) -> Option<(f32, Vec3)> {
    let delta = end - start;
    let mut entering = 0.0_f32;
    let mut leaving = 1.0_f32;
    let mut normal = Vec3::ZERO;
    for axis in 0..3 {
        if delta[axis].abs() < 1.0e-8 {
            if start[axis].abs() > half_size[axis] {
                return None;
            }
            continue;
        }
        let a = (-half_size[axis] - start[axis]) / delta[axis];
        let b = (half_size[axis] - start[axis]) / delta[axis];
        let near = a.min(b);
        if near >= entering {
            entering = near;
            normal = Vec3::ZERO;
            normal[axis] = -delta[axis].signum();
        }
        leaving = leaving.min(a.max(b));
        if entering > leaving {
            return None;
        }
    }
    if normal == Vec3::ZERO {
        return None;
    }
    Some((entering, normal))
}

/// Earliest segment contact with a solid sphere centered at the origin.
pub fn sweep_sphere(start: Vec3, end: Vec3, radius: f32) -> Option<(f32, Vec3)> {
    let delta = end - start;
    let a = delta.length_squared();
    if a < 1.0e-12 {
        return None;
    }
    let b = start.dot(delta);
    let c = start.length_squared() - radius * radius;
    let discriminant = b * b - a * c;
    if discriminant < 0.0 {
        return None;
    }
    let time = (-b - discriminant.sqrt()) / a;
    if !(0.0..=1.0).contains(&time) {
        return None;
    }
    Some((time, (start + delta * time).normalize_or_zero()))
}

/// Earliest hit against a closed vertical cylinder. Side and cap contacts are
/// considered separately, so corners do not behave like an enclosing box.
pub fn sweep_cylinder(
    start: Vec3,
    end: Vec3,
    radius: f32,
    half_height: f32,
) -> Option<(f32, Vec3)> {
    let delta = end - start;
    let mut best: Option<(f32, Vec3)> = None;
    let mut accept = |time: f32, normal: Vec3| {
        if (0.0..=1.0).contains(&time) && best.is_none_or(|(t, _)| time < t) {
            best = Some((time, normal));
        }
    };
    let a = delta.x * delta.x + delta.z * delta.z;
    if a > 1.0e-12 {
        let b = start.x * delta.x + start.z * delta.z;
        let c = start.x * start.x + start.z * start.z - radius * radius;
        let discriminant = b * b - a * c;
        if discriminant >= 0.0 {
            let time = (-b - discriminant.sqrt()) / a;
            let hit = start + delta * time;
            if hit.y.abs() <= half_height {
                accept(time, Vec3::new(hit.x, 0.0, hit.z).normalize_or_zero());
            }
        }
    }
    if delta.y.abs() > 1.0e-12 {
        for sign in [-1.0, 1.0] {
            if delta.y * sign < 0.0 {
                let time = (sign * half_height - start.y) / delta.y;
                let hit = start + delta * time;
                if hit.x * hit.x + hit.z * hit.z <= radius * radius {
                    accept(time, Vec3::Y * sign);
                }
            }
        }
    }
    best
}

/// Reflects only the incoming normal component and damps tangential motion.
pub fn bounce_velocity(velocity: Vec3, normal: Vec3, bounce: f32, friction: f32) -> Vec3 {
    let incoming = velocity.dot(normal);
    if incoming >= 0.0 {
        return velocity;
    }
    (velocity - normal * incoming) * (1.0 - friction.clamp(0.0, 1.0))
        - normal * incoming * bounce.clamp(0.0, 1.0)
}

/// Deterministic `SplitMix64` stream. No dependency on process or thread RNG state.
#[derive(Clone, Debug)]
pub(crate) struct Random(u64);

impl Random {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "the top 24 random bits are represented exactly by the f32 mantissa"
    )]
    pub(crate) fn unit(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^= value >> 31;
        ((value >> 40) as f32) / 16_777_216.0
    }
    pub(crate) fn range(&mut self, minimum: f32, maximum: f32) -> f32 {
        minimum + (maximum - minimum) * self.unit()
    }
    pub(crate) fn direction(&mut self) -> Vec3 {
        let y = self.range(-1.0, 1.0);
        let azimuth = self.unit() * TAU;
        let radius = (1.0 - y * y).sqrt();
        Vec3::new(radius * azimuth.cos(), y, radius * azimuth.sin())
    }
}

#[derive(Clone, Debug)]
struct Particle {
    position: Vec3,
    velocity: Vec3,
    age: f32,
    lifetime: f32,
    size: f32,
    rotation: f32,
    stuck: bool,
}

#[derive(Clone, Debug)]
struct Instance {
    state: PlaybackState,
    time: f32,
    emission: f32,
    rng: Random,
    seed: u64,
    particles: Vec<Particle>,
    points: VecDeque<(Vec3, f32)>,
    previous: Option<Vec3>,
    began: bool,
    completed_event: bool,
    delay: f32,
    kind: std::mem::Discriminant<EffectKind>,
    prewarmed: bool,
}

impl Instance {
    fn new(effect: &EffectComponent) -> Self {
        Self {
            state: if effect.common.auto_play {
                PlaybackState::Playing
            } else {
                PlaybackState::Stopped
            },
            time: -effect.common.delay,
            emission: 0.0,
            rng: Random::new(effect.common.random_seed),
            seed: effect.common.random_seed,
            particles: Vec::new(),
            points: VecDeque::new(),
            previous: None,
            began: false,
            completed_event: false,
            delay: effect.common.delay,
            kind: std::mem::discriminant(&effect.kind),
            prewarmed: false,
        }
    }
    fn reset(&mut self) {
        self.time = -self.delay;
        self.emission = 0.0;
        self.rng = Random::new(self.seed);
        self.particles.clear();
        self.points.clear();
        self.previous = None;
        self.began = false;
        self.completed_event = false;
        self.prewarmed = false;
    }
}

/// Retained CPU simulation state for one scene. Cosmetic collisions never modify
/// authoritative gameplay physics. Destroying an entity removes all its state on
/// the next update; `clear` releases it immediately at scene unload.
#[derive(Debug, Default)]
pub struct EffectRuntime {
    instances: BTreeMap<EntityId, Instance>,
    frame: EffectFrame,
    time: f32,
    camera_target: Option<EntityId>,
}

impl EffectRuntime {
    pub fn new() -> Self {
        Self::default()
    }
    /// Adds a bounded scene-level event, including aggregate composite completion.
    pub(crate) fn emit_event(&mut self, event: EffectEvent) {
        if self.frame.events.len() < MAX_EVENTS {
            self.frame.events.push(event);
        }
    }
    pub fn frame(&self) -> &EffectFrame {
        &self.frame
    }
    /// Selects camera-specific impulses and overlays. `None` previews all effects.
    pub fn set_camera_target(&mut self, target: Option<EntityId>) {
        self.camera_target = target;
    }
    pub fn remove(&mut self, entity: EntityId) {
        self.instances.remove(&entity);
        self.frame.geometry.retain(|v| v.entity != entity);
        self.frame.particles.retain(|v| v.entity != entity);
        self.frame.events.retain(|v| v.entity != entity);
        self.frame.fog.retain(|v| v.entity != entity);
        self.frame.probes.retain(|v| v.entity != entity);
        self.frame.decals.retain(|v| v.entity != entity);
        self.frame.water.retain(|v| v.entity != entity);
        // Aggregated camera and material adjustments are regenerated on update.
        self.frame.lights.clear();
        self.frame.surfaces.clear();
        self.frame.shafts.clear();
        self.frame.outlines.clear();
        self.frame.flares.clear();
        self.frame.post = PostProcessSettings::default();
        self.frame.camera_offset = Vec3::ZERO;
        self.frame.camera_rotation = Vec3::ZERO;
    }
    /// Registers an authored component before a first-frame scripting command.
    pub fn ensure(&mut self, input: &EffectInput) {
        let current = self
            .instances
            .entry(input.entity)
            .or_insert_with(|| Instance::new(&input.effect));
        if current.seed != input.effect.common.random_seed
            || current.kind != std::mem::discriminant(&input.effect.kind)
        {
            *current = Instance::new(&input.effect);
        }
        current.delay = input.effect.common.delay;
    }
    pub fn clear(&mut self) {
        self.instances.clear();
        self.frame = EffectFrame::default();
        self.time = 0.0;
    }
    pub fn state(&self, entity: EntityId) -> PlaybackState {
        self.instances
            .get(&entity)
            .map_or(PlaybackState::Stopped, |s| s.state)
    }
    pub fn particle_count(&self) -> usize {
        self.instances.values().map(|s| s.particles.len()).sum()
    }
    pub fn control(&mut self, entity: EntityId, command: PlaybackCommand) {
        let Some(instance) = self.instances.get_mut(&entity) else {
            return;
        };
        match command {
            PlaybackCommand::Play => {
                if instance.state != PlaybackState::Playing {
                    instance.reset();
                    instance.state = PlaybackState::Playing;
                }
            }
            PlaybackCommand::Pause => {
                if instance.state == PlaybackState::Playing {
                    instance.state = PlaybackState::Paused;
                }
            }
            PlaybackCommand::Resume => {
                if instance.state == PlaybackState::Paused {
                    instance.state = PlaybackState::Playing;
                }
            }
            PlaybackCommand::Stop | PlaybackCommand::Reset => {
                instance.reset();
                instance.state = PlaybackState::Stopped;
            }
            PlaybackCommand::Restart => {
                instance.reset();
                instance.state = PlaybackState::Playing;
            }
            PlaybackCommand::Dispose => {
                instance.reset();
                instance.state = PlaybackState::Disposed;
            }
        }
    }

    pub fn update(&mut self, dt: f32, inputs: &[EffectInput], camera: Vec3) -> &EffectFrame {
        self.update_with_targets(dt, inputs, camera, &BTreeMap::new())
    }

    /// Updates effects with transforms of ordinary scene entities for attachment
    /// and moving beam endpoints. Targets do not need an effects component.
    #[allow(
        clippy::too_many_lines,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::float_cmp,
        reason = "one exhaustive dispatch preserves update ordering; bounded numerical conversions and exact authored-default sentinel comparisons are intentional"
    )]
    pub fn update_with_targets(
        &mut self,
        dt: f32,
        inputs: &[EffectInput],
        camera: Vec3,
        targets: &BTreeMap<EntityId, Mat4>,
    ) -> &EffectFrame {
        let resolved_inputs: Cow<'_, [EffectInput]> = if inputs
            .iter()
            .any(|i| !i.effect.common.parameters.is_empty())
        {
            let mut values = inputs.to_vec();
            for input in &mut values {
                input.effect.resolve_parameters();
            }
            Cow::Owned(values)
        } else {
            Cow::Borrowed(inputs)
        };
        let inputs = resolved_inputs.as_ref();
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.time += dt;
        self.frame.geometry.clear();
        self.frame.particles.clear();
        self.frame.events.clear();
        self.frame.lights.clear();
        self.frame.surfaces.clear();
        self.frame.fog.clear();
        self.frame.shafts.clear();
        self.frame.probes.clear();
        self.frame.decals.clear();
        self.frame.outlines.clear();
        self.frame.flares.clear();
        self.frame.water.clear();
        self.frame.post = PostProcessSettings::default();
        self.frame.camera_offset = Vec3::ZERO;
        self.frame.camera_rotation = Vec3::ZERO;
        let alive: BTreeSet<_> = inputs.iter().map(|i| i.entity).collect();
        self.instances.retain(|id, _| alive.contains(id));
        for input in inputs {
            self.ensure(input);
        }
        let active_fields: BTreeSet<_> = inputs
            .iter()
            .filter(|input| {
                let state = &self.instances[&input.entity];
                let common = &input.effect.common;
                let predicted = state.time
                    + if state.state == PlaybackState::Playing {
                        dt * common.time_scale
                    } else {
                        0.0
                    };
                common.enabled
                    && matches!(state.state, PlaybackState::Playing | PlaybackState::Paused)
                    && predicted >= 0.0
                    && (common.looping || state.time < common.duration)
            })
            .map(|input| input.entity)
            .collect();
        for input in inputs {
            let instance = self
                .instances
                .entry(input.entity)
                .or_insert_with(|| Instance::new(&input.effect));
            let common = &input.effect.common;
            if !common.enabled || instance.state == PlaybackState::Disposed {
                continue;
            }
            let transform = common
                .attachment_target
                .and_then(|id| {
                    targets
                        .get(&id)
                        .copied()
                        .or_else(|| inputs.iter().find(|i| i.entity == id).map(|i| i.transform))
                })
                .unwrap_or(input.transform);
            let position = transform.transform_point3(Vec3::ZERO);
            let (render_center, render_radius) =
                common.bounds.as_ref().map_or((position, 0.0), |bounds| {
                    let center = transform.transform_point3(Vec3::from(bounds.center));
                    let radius = transform
                        .transform_vector3(Vec3::from(bounds.half_extents))
                        .length();
                    (center, radius)
                });
            let render_visible = common.visible
                && render_center.distance(camera) - render_radius <= common.render_distance;

            let active_dt = if instance.state == PlaybackState::Playing {
                dt * common.time_scale
            } else {
                0.0
            };
            let previous_time = instance.time;
            if active_dt > 0.0 {
                instance.time += active_dt;
            }
            let emission_dt = (instance.time.max(0.0) - previous_time.max(0.0)).max(0.0);
            let age = if common.looping {
                instance.time.max(0.0) % common.duration.max(0.001)
            } else {
                instance.time.max(0.0).min(common.duration)
            };
            let t = (age / common.duration.max(0.001)).clamp(0.0, 1.0);
            let mut emitting = instance.state == PlaybackState::Playing
                && instance.time >= 0.0
                && (common.looping || previous_time < common.duration);
            if instance.time < 0.0 {
                emitting = false;
            }
            let mesh_emitter = match &input.effect.kind {
                EffectKind::Particle(p) | EffectKind::MeshParticle(p) => Some(Cow::Borrowed(p)),
                EffectKind::Weather(w) => {
                    let mut p = w.emitter.clone();
                    p.rate *= w.intensity.max(0.0);
                    match w.weather {
                        WeatherType::Snow => {
                            if p.speed == [8.0, 10.0] {
                                p.speed = [0.5, 1.0];
                            }
                            if p.noise == 0.0 {
                                p.noise = 0.3;
                            }
                            if p.start_size == [0.1, 0.25] {
                                p.start_size = [0.03, 0.08];
                            }
                        }
                        WeatherType::Hail => {
                            if p.bounce == 0.5 {
                                p.bounce = 0.7;
                            }
                        }
                        WeatherType::Dust | WeatherType::Sand => {
                            if p.direction == [0.0, -1.0, 0.0] {
                                p.direction = [1.0, -0.1, 0.0];
                            }
                            if p.start_size == [0.1, 0.25] {
                                p.start_size = [0.01, 0.04];
                            }
                        }
                        WeatherType::Leaves => {
                            if p.speed == [8.0, 10.0] {
                                p.speed = [0.5, 1.5];
                            }
                            if p.angular_velocity == 0.0 {
                                p.angular_velocity = 2.0;
                            }
                            if p.noise == 0.0 {
                                p.noise = 1.0;
                            }
                        }
                        WeatherType::Rain => {
                            if p.stretch == 0.0 {
                                p.stretch = 0.025;
                            }
                        }
                    }
                    Some(Cow::Owned(p))
                }
                _ => None,
            };
            if let Some(emitter) = &mesh_emitter {
                let emitter_position = if matches!(&input.effect.kind, EffectKind::Weather(w) if w.camera_follow)
                {
                    camera
                } else {
                    position
                };
                if !instance.prewarmed && emitter.prewarm && common.looping && emitting {
                    let saved = instance.time;
                    let events = self.frame.events.len();
                    let steps = (emitter.prewarm_time.clamp(0.0, 10.0) * 60.0).ceil() as usize;
                    instance.time = 0.0;
                    for _ in 0..steps {
                        let old = instance.time;
                        instance.time += 1.0 / 60.0;
                        simulate_particles(
                            instance,
                            emitter,
                            input,
                            transform,
                            emitter_position,
                            inputs,
                            &active_fields,
                            1.0 / 60.0,
                            old,
                            true,
                            &mut self.frame,
                        );
                    }
                    instance.time = saved;
                    self.frame.events.truncate(events);
                    instance.prewarmed = true;
                }
                simulate_particles(
                    instance,
                    emitter,
                    input,
                    transform,
                    emitter_position,
                    inputs,
                    &active_fields,
                    emission_dt,
                    previous_time,
                    emitting,
                    &mut self.frame,
                );
                if render_visible {
                    let mut generated_mesh = make_geometry(
                        input.entity,
                        PrimitiveMesh::default(),
                        &emitter.appearance,
                        0.0,
                        self.time,
                    );
                    for p in &instance.particles {
                        let normalized = (p.age / p.lifetime).clamp(0.0, 1.0);
                        let mut color = emitter.appearance.color.sample(normalized);
                        color[3] *= emitter
                            .appearance
                            .opacity
                            .sample(normalized)
                            .clamp(0.0, 1.0);
                        let particle_position = if common.simulation_space == SimulationSpace::Local
                        {
                            transform.transform_point3(p.position)
                        } else {
                            p.position
                        };
                        let size = p.size * emitter.appearance.size.sample(normalized).max(0.0);
                        if matches!(input.effect.kind, EffectKind::MeshParticle(_))
                            && emitter.mesh.is_none()
                        {
                            append_cube(
                                &mut generated_mesh,
                                particle_position,
                                size,
                                p.rotation,
                                color,
                            );
                            continue;
                        }
                        self.frame.particles.push(ParticleInstance {
                            entity: input.entity,
                            position: particle_position,
                            velocity: if common.simulation_space == SimulationSpace::Local {
                                transform.transform_vector3(p.velocity)
                            } else {
                                p.velocity
                            },
                            stretch: emitter.stretch.max(0.0),
                            uv_rect: flipbook_rect(emitter.flipbook, emitter.flipbook_fps, p.age),
                            rotation: p.rotation,
                            size: p.size * emitter.appearance.size.sample(normalized).max(0.0),
                            color,
                            mesh: emitter.mesh,
                            material: emitter.appearance.material,
                            texture: emitter.appearance.texture,
                            emission: emitter.appearance.emission,
                            blend: emitter.appearance.blend,
                        });
                    }
                    if !generated_mesh.mesh.positions.is_empty() {
                        self.frame.geometry.push(generated_mesh);
                    }
                }
            }
            if instance.time >= 0.0
                && instance.state != PlaybackState::Stopped
                && instance.state != PlaybackState::Disposed
            {
                match &input.effect.kind {
                    EffectKind::Trail(trail) => {
                        for (_, age) in &mut instance.points {
                            *age += active_dt;
                        }
                        while instance
                            .points
                            .front()
                            .is_some_and(|(_, age)| *age >= trail.lifetime)
                        {
                            instance.points.pop_front();
                        }
                        if emitting && trail.emitting {
                            if instance.points.back().is_some_and(|(p, _)| {
                                p.distance(position) > trail.teleport_distance
                            }) {
                                instance.points.clear();
                            }
                            if instance.points.back().is_none_or(|(p, _)| {
                                p.distance(position) >= trail.min_vertex_distance.max(0.001)
                            }) {
                                instance.points.push_back((position, 0.0));
                            }
                            while instance.points.len() > trail.max_points.min(65536) as usize {
                                instance.points.pop_front();
                            }
                        }
                        if render_visible {
                            let points: Vec<_> = instance.points.iter().map(|(p, _)| *p).collect();
                            let widths: Vec<_> = instance
                                .points
                                .iter()
                                .map(|(_, age)| {
                                    trail.width
                                        * trail.appearance.size.sample(*age / trail.lifetime)
                                })
                                .collect();
                            let mesh = ribbon_mesh(
                                &points,
                                &widths,
                                0,
                                if trail.camera_facing {
                                    camera
                                } else {
                                    position + transform.transform_vector3(Vec3::Z) * 1000.0
                                },
                            );
                            let mut geometry =
                                make_geometry(input.entity, mesh, &trail.appearance, t, self.time);
                            geometry.colors = instance
                                .points
                                .iter()
                                .flat_map(|(_, age)| {
                                    let fraction = (*age / trail.lifetime).clamp(0.0, 1.0);
                                    let mut c = trail.appearance.color.sample(fraction);
                                    c[3] *= trail.appearance.opacity.sample(fraction);
                                    [c, c]
                                })
                                .collect();
                            self.frame.geometry.push(geometry);
                        }
                    }
                    EffectKind::Beam(beam) if render_visible => {
                        let endpoint = |target: Option<EntityId>, offset: [f32; 3]| {
                            target
                                .and_then(|id| {
                                    targets.get(&id).copied().or_else(|| {
                                        inputs.iter().find(|i| i.entity == id).map(|i| i.transform)
                                    })
                                })
                                .unwrap_or(transform)
                                .transform_point3(Vec3::from(offset))
                        };
                        let start = endpoint(beam.start_target, beam.start);
                        let end = endpoint(beam.end_target, beam.end);
                        let n = beam.segments.clamp(1, 4096) as usize;
                        let direction = (end - start).normalize_or_zero();
                        let side = direction
                            .cross(if direction.y.abs() > 0.9 {
                                Vec3::X
                            } else {
                                Vec3::Y
                            })
                            .normalize_or_zero();
                        let up = direction.cross(side);
                        let points: Vec<_> = (0..=n)
                            .map(|i| {
                                let f = i as f32 / n as f32;
                                let phase =
                                    f * beam.noise_frequency * TAU + self.time * beam.noise_speed;
                                start.lerp(end, f)
                                    + (side * phase.sin() + up * (phase * 1.73 + 1.0).sin())
                                        * beam.noise_amplitude
                                        * (f * std::f32::consts::PI).sin()
                            })
                            .collect();
                        self.frame.geometry.push(make_path_geometry(
                            input.entity,
                            ribbon_mesh(&points, &vec![beam.width; points.len()], 0, camera),
                            &beam.appearance,
                            t,
                            self.time,
                        ));
                        for branch in 0..beam.branch_count.min(64) {
                            let index = ((branch as usize + 1) * n
                                / (beam.branch_count.min(64) as usize + 1))
                                .min(n);
                            let a = points[index];
                            let b = a
                                + side
                                    * ((branch as f32 * 2.1 + self.time * beam.noise_speed).sin()
                                        * beam.noise_amplitude
                                        * 5.0)
                                + direction * (end - start).length() * 0.2;
                            self.frame.geometry.push(make_path_geometry(
                                input.entity,
                                ribbon_mesh(&[a, b], &[beam.width * 0.5, 0.0], 0, camera),
                                &beam.appearance,
                                t,
                                self.time,
                            ));
                        }
                    }
                    EffectKind::Shockwave(ring) if render_visible => {
                        let radius =
                            ring.initial_radius + (ring.final_radius - ring.initial_radius) * t;
                        let mut mesh = ring_mesh(radius, ring.thickness, 96);
                        for p in &mut mesh.positions {
                            let mut v = Vec3::from(*p);
                            v.y += ring.vertical_offset;
                            *p = transform.transform_point3(v).to_array();
                        }
                        for normal in &mut mesh.normals {
                            *normal = transform
                                .inverse()
                                .transpose()
                                .transform_vector3(Vec3::from(*normal))
                                .normalize_or_zero()
                                .to_array();
                        }
                        self.frame.geometry.push(make_geometry(
                            input.entity,
                            mesh,
                            &ring.appearance,
                            t,
                            self.time,
                        ));
                    }
                    EffectKind::WaterRipple(ring) if render_visible => {
                        let disturbance = WaterDisturbance {
                            source: position,
                            elapsed: age,
                            duration: common.duration,
                            settings: ring.clone(),
                        };
                        if let Some(target) = ring.target {
                            self.frame.water.push(RenderWaterDisturbance {
                                entity: input.entity,
                                target,
                                disturbance,
                            });
                        } else {
                            let mut surface = RippleWaterSurface {
                                max_disturbances: ring.max_disturbances.max(1) as usize,
                                ..Default::default()
                            };
                            surface.disturb(disturbance);
                            let (mut mesh, foam) = water_surface_mesh(
                                &surface,
                                position + Vec3::Y * ring.vertical_offset,
                                ring.final_radius.max(0.1) * 2.0,
                                64,
                            );
                            let mut geometry = make_geometry(
                                input.entity,
                                std::mem::take(&mut mesh),
                                &ring.appearance,
                                t,
                                self.time,
                            );
                            geometry.colors = foam
                                .into_iter()
                                .map(|foam| {
                                    let mut color = geometry.color;
                                    for channel in &mut color[..3] {
                                        *channel += (1.0 - *channel) * foam;
                                    }
                                    color
                                })
                                .collect();
                            self.frame.geometry.push(geometry);
                        }
                    }
                    EffectKind::Ribbon(ribbon) if render_visible => {
                        let points: Vec<_> = ribbon
                            .points
                            .iter()
                            .map(|p| transform.transform_point3(Vec3::from(*p)))
                            .collect();
                        let points = smooth_path(&points, ribbon.subdivisions.min(32) as u16);
                        let widths: Vec<_> = (0..points.len())
                            .map(|i| {
                                ribbon.width
                                    * ribbon
                                        .appearance
                                        .size
                                        .sample(i as f32 / (points.len() - 1).max(1) as f32)
                            })
                            .collect();
                        self.frame.geometry.push(make_path_geometry(
                            input.entity,
                            twisted_ribbon_mesh(
                                &points,
                                &widths,
                                if ribbon.tube {
                                    ribbon.radial_segments.min(32) as u16
                                } else {
                                    0
                                },
                                camera,
                                ribbon.twist,
                            ),
                            &ribbon.appearance,
                            t,
                            self.time,
                        ));
                    }
                    EffectKind::ProjectileTracer(tracer) if render_visible => {
                        let velocity = transform.transform_vector3(Vec3::from(tracer.velocity));
                        let head = position + velocity * age;
                        let tail = head - velocity.normalize_or_zero() * tracer.length;
                        self.frame.geometry.push(make_geometry(
                            input.entity,
                            ribbon_mesh(&[tail, head], &[0.0, tracer.width], 0, camera),
                            &tracer.appearance,
                            t,
                            self.time,
                        ));
                    }
                    EffectKind::FoliageScatter(scatter) if render_visible => {
                        let mut rng = Random::new(common.random_seed);
                        let count =
                            (scatter.density * scatter.dimensions[0] * scatter.dimensions[2])
                                .max(0.0)
                                .min(scatter.max_instances.min(100_000) as f32)
                                as usize;
                        let mut foliage = make_geometry(
                            input.entity,
                            PrimitiveMesh::default(),
                            &scatter.appearance,
                            0.0,
                            self.time,
                        );
                        for i in 0..count {
                            let local = Vec3::new(
                                rng.range(-0.5, 0.5) * scatter.dimensions[0],
                                0.0,
                                rng.range(-0.5, 0.5) * scatter.dimensions[2],
                            );
                            let world = transform.transform_point3(local);
                            if world.distance(camera) > common.render_distance {
                                continue;
                            }
                            let wind = sample_active_wind(
                                inputs,
                                &active_fields,
                                world,
                                self.time,
                                common.visibility_layers,
                            ) * scatter.wind_response;
                            let rotation = rng.unit() * TAU;
                            let size = rng.range(scatter.scale[0], scatter.scale[1]);
                            if scatter.meshes.is_empty() {
                                append_foliage(
                                    &mut foliage,
                                    world,
                                    size,
                                    rotation,
                                    wind * 0.03,
                                    scatter.appearance.color.sample(0.0),
                                );
                            } else {
                                self.frame.particles.push(ParticleInstance {
                                    entity: input.entity,
                                    position: world + wind * 0.03,
                                    velocity: Vec3::ZERO,
                                    stretch: 0.0,
                                    uv_rect: [0.0, 0.0, 1.0, 1.0],
                                    rotation,
                                    size,
                                    color: scatter.appearance.color.sample(0.0),
                                    mesh: scatter
                                        .meshes
                                        .get(i % scatter.meshes.len().max(1))
                                        .copied(),
                                    material: scatter.appearance.material,
                                    texture: scatter.appearance.texture,
                                    emission: scatter.appearance.emission,
                                    blend: scatter.appearance.blend,
                                });
                            }
                        }
                        if !foliage.mesh.positions.is_empty() {
                            self.frame.geometry.push(foliage);
                        }
                    }
                    _ => {}
                }
            }
            instance.previous = Some(position);
            if !common.looping
                && instance.time >= common.duration
                && instance.state == PlaybackState::Playing
            {
                if !instance.completed_event {
                    self.frame.events.push(EffectEvent {
                        entity: input.entity,
                        kind: EffectEventKind::Completed,
                        position,
                        child_effect: None,
                    });
                    instance.completed_event = true;
                }
                if instance.particles.is_empty() && instance.points.is_empty() {
                    instance.state = PlaybackState::Completed;
                }
            }
        }
        let mut ordered: Vec<_> = inputs.iter().collect();
        ordered.sort_by_key(|i| (i.effect.common.priority, i.entity));
        for input in ordered {
            let target = match &input.effect.kind {
                EffectKind::CameraShake(v) => v.target,
                EffectKind::ScreenFlash(v) => v.target,
                _ => None,
            };
            if target.is_some() && self.camera_target.is_some() && target != self.camera_target {
                continue;
            }
            if let Some(state) = self.instances.get(&input.entity)
                && input.effect.common.enabled
                && input.effect.common.visible
                && state.time >= 0.0
                && matches!(state.state, PlaybackState::Playing | PlaybackState::Paused)
            {
                let elapsed = if input.effect.common.looping {
                    state.time % input.effect.common.duration.max(0.001)
                } else {
                    state.time
                };
                super::modifiers::extract_modifier(
                    input,
                    camera,
                    elapsed,
                    (elapsed / input.effect.common.duration.max(0.001)).clamp(0.0, 1.0),
                    &mut self.frame,
                );
            }
        }
        &self.frame
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "validated cosmetic capacities and tessellation bounds keep numerical conversions in range"
)]
fn twisted_ribbon_mesh(
    points: &[Vec3],
    widths: &[f32],
    radial: u16,
    camera: Vec3,
    twist: f32,
) -> PrimitiveMesh {
    let mut mesh = ribbon_mesh(points, widths, radial, camera);
    if twist.abs() < 0.00001 || points.len() < 2 {
        return mesh;
    }
    let stride = if radial < 3 {
        2
    } else {
        radial.min(32) as usize + 1
    };
    for (index, center) in points.iter().enumerate() {
        let tangent = (points[(index + 1).min(points.len() - 1)] - points[index.saturating_sub(1)])
            .normalize_or_zero();
        if tangent.length_squared() < 0.5 {
            continue;
        }
        let rotation =
            glam::Quat::from_axis_angle(tangent, twist * index as f32 / (points.len() - 1) as f32);
        for side in 0..stride {
            let vertex = index * stride + side;
            if let Some(p) = mesh.positions.get_mut(vertex) {
                *p = (*center + rotation * (Vec3::from(*p) - *center)).to_array();
            }
            if let Some(n) = mesh.normals.get_mut(vertex) {
                *n = (rotation * Vec3::from(*n)).to_array();
            }
        }
    }
    mesh
}

fn make_path_geometry(
    entity: EntityId,
    mesh: PrimitiveMesh,
    appearance: &EffectAppearance,
    t: f32,
    time: f32,
) -> EffectGeometry {
    let length = mesh
        .tex_coords
        .iter()
        .map(|uv| uv[0])
        .fold(0.0f32, f32::max)
        .max(0.00001);
    let colors = mesh
        .tex_coords
        .iter()
        .map(|uv| {
            let mut c = appearance.color.sample(uv[0] / length);
            c[3] *= appearance.opacity.sample(t).clamp(0.0, 1.0);
            c
        })
        .collect();
    let mut geometry = make_geometry(entity, mesh, appearance, t, time);
    geometry.colors = colors;
    geometry
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "validated cosmetic capacities and tessellation bounds keep numerical conversions in range"
)]
fn flipbook_rect(grid: [u32; 2], fps: f32, age: f32) -> [f32; 4] {
    let columns = grid[0].clamp(1, 1024);
    let rows = grid[1].clamp(1, 1024);
    let frame = ((age * fps.max(0.0)).floor() as u32) % (columns * rows);
    let x = (frame % columns) as f32 / columns as f32;
    let y = (frame / columns) as f32 / rows as f32;
    [x, y, x + 1.0 / columns as f32, y + 1.0 / rows as f32]
}

fn make_geometry(
    entity: EntityId,
    mut mesh: PrimitiveMesh,
    appearance: &EffectAppearance,
    t: f32,
    time: f32,
) -> EffectGeometry {
    for uv in &mut mesh.tex_coords {
        for (axis, value) in uv.iter_mut().enumerate() {
            *value = *value * appearance.uv_scale[axis] + appearance.uv_scroll[axis] * time;
        }
    }
    let mut color = appearance.color.sample(t);
    color[3] *= appearance.opacity.sample(t).clamp(0.0, 1.0);
    EffectGeometry {
        entity,
        mesh,
        colors: Vec::new(),
        color,
        emission: appearance.emission,
        material: appearance.material,
        texture: appearance.texture,
        blend: appearance.blend,
    }
}

fn append_cube(
    geometry: &mut EffectGeometry,
    position: Vec3,
    size: f32,
    rotation: f32,
    color: [f32; 4],
) {
    // One batched indexed mesh for the emitter's procedural debris. Asset-backed
    // mesh particles continue through the existing mesh/material asset path.
    static CUBE: std::sync::OnceLock<PrimitiveMesh> = std::sync::OnceLock::new();
    let mesh = CUBE.get_or_init(|| {
        crate::Primitive::Cube { size: 1.0 }
            .mesh()
            .expect("unit cube is valid")
    });
    let transform = Mat4::from_translation(position)
        * Mat4::from_rotation_z(rotation)
        * Mat4::from_scale(Vec3::splat(size));
    let base =
        u32::try_from(geometry.mesh.positions.len()).expect("bounded effect geometry fits u32");
    geometry.mesh.positions.extend(
        mesh.positions
            .iter()
            .map(|p| transform.transform_point3(Vec3::from(*p)).to_array()),
    );
    geometry.mesh.normals.extend(mesh.normals.iter().map(|n| {
        Mat4::from_rotation_z(rotation)
            .transform_vector3(Vec3::from(*n))
            .to_array()
    }));
    geometry.mesh.tex_coords.extend_from_slice(&mesh.tex_coords);
    geometry
        .mesh
        .indices
        .extend(mesh.indices.iter().map(|index| index + base));
    geometry
        .colors
        .extend(std::iter::repeat_n(color, mesh.positions.len()));
}

fn append_foliage(
    geometry: &mut EffectGeometry,
    position: Vec3,
    size: f32,
    rotation: f32,
    wind: Vec3,
    color: [f32; 4],
) {
    for angle in [rotation, rotation + std::f32::consts::FRAC_PI_2] {
        let blade_width = Vec3::new(angle.cos(), 0.0, angle.sin()) * size * 0.2;
        let top = position + Vec3::Y * size + wind;
        let base =
            u32::try_from(geometry.mesh.positions.len()).expect("bounded effect geometry fits u32");
        geometry.mesh.positions.extend(
            [
                position - blade_width,
                position + blade_width,
                top + blade_width * 0.25,
                top - blade_width * 0.25,
            ]
            .map(|v| v.to_array()),
        );
        let normal = blade_width.cross(Vec3::Y).normalize_or_zero().to_array();
        geometry.mesh.normals.extend([normal; 4]);
        geometry
            .mesh
            .tex_coords
            .extend([[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
        geometry
            .mesh
            .indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        geometry.colors.extend([color; 4]);
    }
}

/// Wind fields add in stable scene order. Masks select compatible consumers;
/// falloff is evaluated in the zone's local coordinates, including parent scale.
pub fn sample_wind(inputs: &[EffectInput], position: Vec3, time: f32, layers: u32) -> Vec3 {
    inputs
        .iter()
        .filter_map(|i| match &i.effect.kind {
            EffectKind::Wind(f) if i.effect.common.enabled && f.layers & layers != 0 => {
                Some(field_force(f, i.transform, position, time))
            }
            _ => None,
        })
        .sum()
}

fn sample_active_wind(
    inputs: &[EffectInput],
    active: &BTreeSet<EntityId>,
    position: Vec3,
    time: f32,
    layers: u32,
) -> Vec3 {
    inputs
        .iter()
        .filter(|i| active.contains(&i.entity))
        .filter_map(|i| match &i.effect.kind {
            EffectKind::Wind(f) if f.layers & layers != 0 => {
                Some(field_force(f, i.transform, position, time))
            }
            _ => None,
        })
        .sum()
}

fn field_force(field: &ForceField, transform: Mat4, position: Vec3, time: f32) -> Vec3 {
    let local = transform.inverse().transform_point3(position);
    let half = Vec3::from(field.dimensions).abs() * 0.5;
    let normalized = match field.shape {
        VolumeShape::Sphere => local.length() / half.max_element().max(0.001),
        VolumeShape::Cylinder => {
            (local.xz().length() / half.x.max(0.001)).max(local.y.abs() / half.y.max(0.001))
        }
        _ => (local.abs() / half.max(Vec3::splat(0.001))).max_element(),
    };
    if !field.global && normalized > 1.0 {
        return Vec3::ZERO;
    }
    let weight = if field.global {
        1.0
    } else {
        (1.0 - normalized).max(0.0).powf(field.falloff.max(0.0))
    };
    let direction = transform
        .transform_vector3(Vec3::from(field.direction))
        .normalize_or_zero();
    let radial = (position - transform.transform_point3(Vec3::ZERO)).normalize_or_zero();
    let noise = Vec3::new(
        (position.z * field.frequency + time).sin(),
        (position.x * field.frequency + time * 1.37).sin(),
        (position.y * field.frequency - time * 0.93).cos(),
    );
    let force = match field.force_type {
        ForceType::Directional => direction,
        ForceType::Attraction => -radial,
        ForceType::Repulsion => radial,
        ForceType::Vortex => direction.cross(radial),
        ForceType::Turbulence => noise,
    };
    (force * field.strength + noise * field.turbulence) * weight
}

fn sample_triangles(triangles: &[[[f32; 3]; 3]], rng: &mut Random) -> Vec3 {
    let area = |triangle: &[[f32; 3]; 3]| {
        let a = Vec3::from(triangle[0]);
        let b = Vec3::from(triangle[1]);
        let c = Vec3::from(triangle[2]);
        (b - a).cross(c - a).length() * 0.5
    };
    let total: f32 = triangles.iter().map(area).sum();
    if total <= f32::EPSILON {
        return Vec3::ZERO;
    }
    let mut selection = rng.unit() * total;
    let mut chosen = &triangles[triangles.len() - 1];
    for triangle in triangles {
        selection -= area(triangle);
        if selection <= 0.0 {
            chosen = triangle;
            break;
        }
    }
    let u = rng.unit().sqrt();
    let v = rng.unit();
    Vec3::from(chosen[0]) * (1.0 - u)
        + Vec3::from(chosen[1]) * u * (1.0 - v)
        + Vec3::from(chosen[2]) * u * v
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "validated cosmetic capacities and tessellation bounds keep numerical conversions in range"
)]
fn spawn_offset(shape: SpawnShape, dimensions: Vec3, surface: bool, rng: &mut Random) -> Vec3 {
    let radius = if surface { 1.0 } else { rng.unit().cbrt() };
    match shape {
        SpawnShape::Point | SpawnShape::MeshSurface => Vec3::ZERO,
        SpawnShape::Sphere => rng.direction() * dimensions * 0.5 * radius,
        SpawnShape::Hemisphere => {
            let mut p = rng.direction();
            p.y = p.y.abs();
            p * dimensions * 0.5 * radius
        }
        SpawnShape::Box => {
            let mut p = Vec3::new(
                rng.range(-0.5, 0.5),
                rng.range(-0.5, 0.5),
                rng.range(-0.5, 0.5),
            );
            if surface {
                let axis = (rng.unit() * 3.0) as usize;
                p[axis.min(2)] = if rng.unit() < 0.5 { -0.5 } else { 0.5 };
            }
            p * dimensions
        }
        SpawnShape::Circle => {
            let a = rng.unit() * TAU;
            let r = if surface {
                0.5
            } else {
                rng.unit().sqrt() * 0.5
            };
            Vec3::new(a.cos() * r * dimensions.x, 0.0, a.sin() * r * dimensions.z)
        }
        SpawnShape::Cylinder => {
            let a = rng.unit() * TAU;
            let r = if surface {
                0.5
            } else {
                rng.unit().sqrt() * 0.5
            };
            Vec3::new(
                a.cos() * r * dimensions.x,
                rng.range(-0.5, 0.5) * dimensions.y,
                a.sin() * r * dimensions.z,
            )
        }
        SpawnShape::Cone => {
            let h = rng.unit().cbrt();
            let a = rng.unit() * TAU;
            let r = if surface { h } else { h * rng.unit().sqrt() };
            Vec3::new(
                a.cos() * r * dimensions.x * 0.5,
                h * dimensions.y,
                a.sin() * r * dimensions.z * 0.5,
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "validated cosmetic capacities and tessellation bounds keep numerical conversions in range"
)]
fn simulate_particles(
    instance: &mut Instance,
    p: &ParticleEmitter,
    input: &EffectInput,
    transform: Mat4,
    position: Vec3,
    inputs: &[EffectInput],
    active_fields: &BTreeSet<EntityId>,
    dt: f32,
    previous_time: f32,
    emitting: bool,
    frame: &mut EffectFrame,
) {
    let common = &input.effect.common;
    let local = common.simulation_space == SimulationSpace::Local;
    let quality = match common.quality {
        QualityLevel::Low => 0.5,
        QualityLevel::Medium => 1.0,
        QualityLevel::High => 1.5,
    };
    let emitter_velocity = instance
        .previous
        .map_or(Vec3::ZERO, |old| (position - old) / dt.max(0.00001));
    if emitting {
        let clipped_dt = if common.looping {
            dt
        } else {
            dt.min((common.duration - previous_time.max(0.0)).max(0.0))
        };
        instance.emission += p.rate * quality * clipped_dt
            + p.rate_over_distance
                * quality
                * instance.previous.map_or(0.0, |old| old.distance(position));
        let mut spawn = instance.emission.floor().clamp(0.0, 100_000.0) as usize;
        instance.emission -= spawn as f32;
        let duration = common.duration.max(0.001);
        let first_cycle = (previous_time.max(0.0) / duration).floor() as u64;
        let last_cycle = (instance.time.max(0.0) / duration).floor() as u64;
        let mut burst_budget = 8192usize;
        'bursts: for cycle in first_cycle..=last_cycle.min(first_cycle + 100) {
            if cycle > 0 && !common.looping {
                break;
            }
            for burst in &p.bursts {
                for repetition in 0..burst.cycles.min(1024) {
                    if burst_budget == 0 {
                        break 'bursts;
                    }
                    burst_budget -= 1;
                    let when =
                        cycle as f32 * duration + burst.time + repetition as f32 * burst.interval;
                    if when > cycle as f32 * duration + duration {
                        break;
                    }
                    if (when > previous_time || (when == 0.0 && !instance.began))
                        && when <= instance.time
                        && instance.rng.unit() <= burst.probability
                    {
                        spawn = spawn.saturating_add(burst.count as usize);
                    }
                }
            }
        }
        instance.began = true;
        let capacity = ((p.max_particles as f32 * quality.min(1.0)).min(100_000.0) as usize)
            .saturating_sub(instance.particles.len());
        for _ in 0..spawn.min(capacity) {
            let offset = if p.shape == SpawnShape::MeshSurface {
                sample_triangles(&p.mesh_spawn_triangles, &mut instance.rng)
            } else {
                spawn_offset(
                    p.shape,
                    Vec3::from(p.dimensions),
                    p.surface_only,
                    &mut instance.rng,
                )
            };
            let direction = if p.shape == SpawnShape::Cone {
                (Vec3::from(p.direction).normalize_or_zero() + offset.normalize_or_zero() * 0.5)
                    .normalize_or_zero()
            } else {
                Vec3::from(p.direction).normalize_or_zero()
            };
            let velocity = direction * instance.rng.range(p.speed[0], p.speed[1]);
            let particle = Particle {
                position: if local {
                    offset
                } else {
                    position + transform.transform_vector3(offset)
                },
                velocity: if local {
                    velocity
                } else {
                    transform.transform_vector3(velocity) + emitter_velocity * p.inherit_velocity
                },
                age: 0.0,
                lifetime: instance.rng.range(p.lifetime[0], p.lifetime[1]).max(0.001),
                size: instance.rng.range(p.start_size[0], p.start_size[1]),
                rotation: instance.rng.unit() * TAU,
                stuck: false,
            };
            if frame.events.len() < MAX_EVENTS {
                frame.events.push(EffectEvent {
                    entity: input.entity,
                    kind: EffectEventKind::Birth,
                    position: particle.position,
                    child_effect: if instance.rng.unit() <= p.sub_emitter_probability {
                        p.sub_emitter
                    } else {
                        None
                    },
                });
            }
            instance.particles.push(particle);
        }
    }
    let steps = (dt / (1.0 / 60.0)).ceil().clamp(1.0, 120.0) as usize;
    let step = dt / steps as f32;
    for _ in 0..steps {
        for particle in &mut instance.particles {
            particle.age += step;
            if particle.age >= particle.lifetime {
                continue;
            }
            if particle.stuck {
                continue;
            }
            let world_position = if local {
                transform.transform_point3(particle.position)
            } else {
                particle.position
            };
            let mut force = Vec3::from(p.gravity)
                + Vec3::from(p.force)
                + sample_active_wind(
                    inputs,
                    active_fields,
                    world_position,
                    instance.time,
                    common.visibility_layers,
                ) * p.wind_influence;
            for field in inputs {
                if let EffectKind::ForceField(f) = &field.effect.kind
                    && active_fields.contains(&field.entity)
                    && f.layers & common.visibility_layers != 0
                {
                    force += field_force(f, field.transform, world_position, instance.time);
                }
            }
            force += Vec3::new(
                (particle.position.y + instance.time * 1.7).sin(),
                (particle.position.z + instance.time * 1.3).sin(),
                (particle.position.x - instance.time * 1.1).cos(),
            ) * p.noise;
            if local {
                force = transform.inverse().transform_vector3(force);
            }
            particle.velocity =
                (particle.velocity + force * step) * (-p.drag.max(0.0) * step).exp();
            particle.velocity = particle
                .velocity
                .clamp_length_max(p.velocity_limit.max(0.0));
            let old = particle.position;
            let next = old + particle.velocity * step;
            particle.position = next;
            particle.rotation += p.angular_velocity * step;
            if p.collision && step > 0.0 {
                let start = if local {
                    transform.transform_point3(old)
                } else {
                    old
                };
                let end = if local {
                    transform.transform_point3(next)
                } else {
                    next
                };
                for volume in inputs {
                    let EffectKind::CollisionVolume(c) = &volume.effect.kind else {
                        continue;
                    };
                    if !active_fields.contains(&volume.entity) || c.layers & p.collision_layers == 0
                    {
                        continue;
                    }
                    let inverse = volume.transform.inverse();
                    let a = inverse.transform_point3(start);
                    let b = inverse.transform_point3(end);
                    let half = Vec3::from(c.dimensions).abs() * 0.5;
                    let contact = match c.shape {
                        VolumeShape::Sphere => sweep_sphere(a, b, half.max_element()),
                        VolumeShape::Box => sweep_box(a, b, half),
                        VolumeShape::Plane => {
                            if a.y >= 0.0 && b.y <= 0.0 && (a.y - b.y) != 0.0 {
                                let fraction = a.y / (a.y - b.y);
                                let hit = a.lerp(b, fraction);
                                if hit.x.abs() <= half.x && hit.z.abs() <= half.z {
                                    Some((fraction, Vec3::Y))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        VolumeShape::Cylinder => sweep_cylinder(a, b, half.x.max(half.z), half.y),
                    };
                    if let Some((fraction, normal)) = contact {
                        let normal = volume
                            .transform
                            .inverse()
                            .transpose()
                            .transform_vector3(normal)
                            .normalize_or_zero();
                        let hit = start.lerp(end, fraction) + normal * 0.001;
                        particle.position = if local {
                            transform.inverse().transform_point3(hit)
                        } else {
                            hit
                        };
                        let response = if p.collision_response == CollisionResponse::Kill
                            || c.response == CollisionResponse::Kill
                        {
                            CollisionResponse::Kill
                        } else if p.collision_response == CollisionResponse::Stick
                            || c.response == CollisionResponse::Stick
                        {
                            CollisionResponse::Stick
                        } else {
                            CollisionResponse::Bounce
                        };
                        match response {
                            CollisionResponse::Kill => particle.age = particle.lifetime,
                            CollisionResponse::Stick => {
                                particle.velocity = Vec3::ZERO;
                                particle.stuck = true;
                            }
                            CollisionResponse::Bounce => {
                                let v = if local {
                                    transform.transform_vector3(particle.velocity)
                                } else {
                                    particle.velocity
                                };
                                let v = bounce_velocity(
                                    v,
                                    normal,
                                    p.bounce * c.bounce,
                                    1.0 - (1.0 - p.friction) * (1.0 - c.friction),
                                );
                                particle.velocity = if local {
                                    transform.inverse().transform_vector3(v)
                                } else {
                                    v
                                };
                            }
                        }
                        if frame.events.len() < MAX_EVENTS {
                            frame.events.push(EffectEvent {
                                entity: input.entity,
                                kind: EffectEventKind::Collision,
                                position: hit,
                                child_effect: c.impact_effect.or(p.sub_emitter),
                            });
                        }
                        break;
                    }
                }
            }
        }
    }
    instance.particles.retain(|particle| {
        if particle.age >= particle.lifetime {
            if frame.events.len() < MAX_EVENTS {
                frame.events.push(EffectEvent {
                    entity: input.entity,
                    kind: EffectEventKind::Death,
                    position: particle.position,
                    child_effect: p.sub_emitter,
                });
            }
            false
        } else {
            true
        }
    });
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::field_reassign_with_default,
    reason = "tests check exact deterministic values and modify individual fixture controls"
)]
mod geometry_tests {
    use super::*;
    use crate::effects::{
        BeamRenderer, Burst, EffectParameter, FoliageScatter, ScreenFlash, TrailRenderer,
    };
    #[test]
    fn ribbon_is_indexed_and_camera_facing() {
        let mesh = ribbon_mesh(&[Vec3::ZERO, Vec3::Y], &[2.0, 1.0], 0, Vec3::Z * 5.0);
        assert_eq!(mesh.positions.len(), 4);
        assert_eq!(mesh.indices.len(), 6);
        assert!(
            (Vec3::from(mesh.positions[0]).distance(Vec3::from(mesh.positions[1])) - 2.0).abs()
                < 1.0e-5
        );
        assert!(mesh.positions.iter().flatten().all(|v| v.is_finite()));
    }
    #[test]
    fn tube_closes_its_uv_seam_and_handles_parallel_tangents() {
        let mesh = ribbon_mesh(&[Vec3::ZERO, Vec3::X, Vec3::X * 2.0], &[1.0; 3], 8, Vec3::Z);
        assert_eq!(mesh.positions.len(), 27);
        assert_eq!(mesh.indices.len(), 96);
        assert!(Vec3::from(mesh.positions[0]).distance(Vec3::from(mesh.positions[8])) < 1.0e-5);
        assert!(mesh.normals.iter().flatten().all(|v| v.is_finite()));
    }
    #[test]
    fn random_directions_are_unit_and_repeatable() {
        let mut a = Random::new(123);
        let mut b = Random::new(123);
        for _ in 0..1000 {
            let direction = a.direction();
            assert_eq!(direction, b.direction());
            assert!((direction.length() - 1.0).abs() < 1.0e-5);
        }
    }

    fn emitter() -> EffectInput {
        let mut p = ParticleEmitter::default();
        p.shape = SpawnShape::Point;
        p.rate = 10.0;
        p.speed = [0.0; 2];
        p.lifetime = [10.0; 2];
        p.gravity = [0.0; 3];
        p.drag = 0.0;
        EffectInput {
            entity: EntityId::new(),
            transform: Mat4::IDENTITY,
            effect: EffectComponent::new(EffectKind::Particle(p)),
        }
    }

    #[test]
    fn lifecycle_pause_resume_restart_and_dispose_are_observable() {
        let input = emitter();
        let id = input.entity;
        let mut runtime = EffectRuntime::new();
        runtime.ensure(&input);
        runtime.control(id, PlaybackCommand::Pause);
        assert_eq!(runtime.state(id), PlaybackState::Paused);
        assert_eq!(
            runtime
                .update(0.5, std::slice::from_ref(&input), Vec3::Z)
                .particles
                .len(),
            0
        );
        runtime.control(id, PlaybackCommand::Resume);
        assert_eq!(
            runtime
                .update(0.5, std::slice::from_ref(&input), Vec3::Z)
                .particles
                .len(),
            5
        );
        runtime.control(id, PlaybackCommand::Pause);
        assert_eq!(
            runtime
                .update(0.5, std::slice::from_ref(&input), Vec3::Z)
                .particles
                .len(),
            5
        );
        runtime.control(id, PlaybackCommand::Restart);
        assert_eq!(
            runtime
                .update(0.2, std::slice::from_ref(&input), Vec3::Z)
                .particles
                .len(),
            2
        );
        runtime.control(id, PlaybackCommand::Dispose);
        assert!(runtime.update(1.0, &[input], Vec3::Z).particles.is_empty());
        assert_eq!(runtime.state(id), PlaybackState::Disposed);
    }

    #[test]
    fn deterministic_seed_and_maximum_particle_budget() {
        let mut input = emitter();
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.shape = SpawnShape::Sphere;
            p.speed = [1.0, 3.0];
            p.max_particles = 4;
            p.rate = 1000.0;
        }
        let mut a = EffectRuntime::new();
        let mut b = EffectRuntime::new();
        a.update(0.1, std::slice::from_ref(&input), Vec3::Z);
        b.update(0.1, &[input], Vec3::Z);
        assert_eq!(a.particle_count(), 4);
        for (a, b) in a.frame().particles.iter().zip(&b.frame().particles) {
            assert_eq!(a.position, b.position);
            assert_eq!(a.size, b.size);
        }
    }

    #[test]
    fn timed_bursts_do_not_repeat_during_pause_and_loop_crossing() {
        let mut input = emitter();
        input.effect.common.duration = 1.0;
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.rate = 0.0;
            p.bursts = vec![Burst {
                time: 0.25,
                count: 3,
                ..Burst::default()
            }];
        }
        let mut runtime = EffectRuntime::new();
        assert_eq!(
            runtime
                .update(0.2, std::slice::from_ref(&input), Vec3::Z)
                .particles
                .len(),
            0
        );
        assert_eq!(
            runtime
                .update(0.1, std::slice::from_ref(&input), Vec3::Z)
                .particles
                .len(),
            3
        );
        assert_eq!(
            runtime
                .update(1.0, std::slice::from_ref(&input), Vec3::Z)
                .particles
                .len(),
            6
        );
        runtime.control(input.entity, PlaybackCommand::Pause);
        assert_eq!(runtime.update(1.0, &[input], Vec3::Z).particles.len(), 6);
    }

    #[test]
    fn completion_event_fires_once_and_unload_removes_allocated_state() {
        let mut input = emitter();
        input.effect.common.duration = 0.25;
        input.effect.common.looping = false;
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.rate = 0.0;
        }
        let mut runtime = EffectRuntime::new();
        assert_eq!(
            runtime
                .update(0.5, std::slice::from_ref(&input), Vec3::Z)
                .events
                .iter()
                .filter(|e| e.kind == EffectEventKind::Completed)
                .count(),
            1
        );
        assert_eq!(runtime.state(input.entity), PlaybackState::Completed);
        assert!(
            runtime
                .update(0.1, std::slice::from_ref(&input), Vec3::Z)
                .events
                .is_empty()
        );
        runtime.update(0.1, &[], Vec3::Z);
        assert!(runtime.instances.is_empty());
    }

    #[test]
    fn ordinary_scene_targets_drive_beam_endpoints() {
        let target = EntityId::new();
        let input = EffectInput {
            entity: EntityId::new(),
            transform: Mat4::IDENTITY,
            effect: EffectComponent::new(EffectKind::Beam(BeamRenderer {
                end_target: Some(target),
                end: [0.0; 3],
                noise_amplitude: 0.0,
                ..Default::default()
            })),
        };
        let mut targets = BTreeMap::new();
        targets.insert(target, Mat4::from_translation(Vec3::Y * 5.0));
        let mut runtime = EffectRuntime::new();
        let frame = runtime.update_with_targets(0.1, &[input], Vec3::Z * 10.0, &targets);
        assert!(
            frame.geometry[0]
                .mesh
                .positions
                .iter()
                .any(|p| (p[1] - 5.0).abs() < 0.0001)
        );
    }

    #[test]
    fn swept_contacts_handle_thin_volumes_and_cylinder_corners() {
        let hit = sweep_box(Vec3::Y * 10.0, -Vec3::Y * 10.0, Vec3::new(2.0, 0.01, 2.0)).unwrap();
        assert!((hit.0 - 0.4995).abs() < 0.0001);
        assert_eq!(hit.1, Vec3::Y);
        assert!(
            sweep_cylinder(
                Vec3::new(0.9, 2.0, 0.9),
                Vec3::new(0.9, -2.0, 0.9),
                1.0,
                1.0
            )
            .is_none()
        );
        let cylinder = sweep_cylinder(Vec3::X * 2.0, -Vec3::X * 2.0, 1.0, 1.0).unwrap();
        assert_eq!(cylinder.0, 0.25);
        assert_eq!(cylinder.1, Vec3::X);
        assert_eq!(
            bounce_velocity(Vec3::new(2.0, -4.0, 0.0), Vec3::Y, 0.5, 0.25),
            Vec3::new(1.5, 2.0, 0.0)
        );
    }

    #[test]
    fn shape_samples_stay_in_bounds_and_triangle_samples_stay_on_surface() {
        let mut rng = Random::new(77);
        for shape in [
            SpawnShape::Sphere,
            SpawnShape::Hemisphere,
            SpawnShape::Box,
            SpawnShape::Cone,
            SpawnShape::Circle,
            SpawnShape::Cylinder,
        ] {
            for _ in 0..1000 {
                let p = spawn_offset(shape, Vec3::splat(2.0), false, &mut rng);
                assert!(p.is_finite());
                assert!(p.x.abs() <= 1.0001 && p.z.abs() <= 1.0001);
                if shape == SpawnShape::Hemisphere {
                    assert!(p.y >= 0.0);
                }
            }
        }
        for _ in 0..1000 {
            let p = sample_triangles(
                &[[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]],
                &mut rng,
            );
            assert_eq!(p.y, 0.0);
            assert!(p.x >= 0.0 && p.z >= 0.0 && p.x + p.z <= 1.00001);
        }
    }

    #[test]
    fn trailing_points_follow_attachment_and_expire() {
        let mut input = EffectInput {
            entity: EntityId::new(),
            transform: Mat4::IDENTITY,
            effect: EffectComponent::new(EffectKind::Trail(TrailRenderer {
                lifetime: 0.3,
                ..Default::default()
            })),
        };
        let mut runtime = EffectRuntime::new();
        runtime.update(0.1, std::slice::from_ref(&input), Vec3::Z);
        input.transform = Mat4::from_translation(Vec3::X);
        assert_eq!(
            runtime
                .update(0.1, std::slice::from_ref(&input), Vec3::Z)
                .geometry[0]
                .mesh
                .positions
                .len(),
            4
        );
        if let EffectKind::Trail(t) = &mut input.effect.kind {
            t.emitting = false;
        }
        assert!(
            runtime.update(0.4, &[input], Vec3::Z).geometry[0]
                .mesh
                .positions
                .is_empty()
        );
    }

    #[test]
    fn mesh_particles_and_foliage_generate_batched_triangles_without_asset() {
        let mut input = emitter();
        if let EffectKind::Particle(p) = input.effect.kind.clone() {
            input.effect.kind = EffectKind::MeshParticle(p);
        }
        let mut runtime = EffectRuntime::new();
        let frame = runtime.update(0.2, std::slice::from_ref(&input), Vec3::Z);
        assert!(frame.particles.is_empty());
        assert_eq!(frame.geometry.len(), 1);
        assert_eq!(frame.geometry[0].mesh.indices.len(), 72);
        input.effect.kind = EffectKind::FoliageScatter(FoliageScatter {
            dimensions: [1.0; 3],
            density: 3.0,
            ..Default::default()
        });
        let frame = runtime.update(0.1, &[input], Vec3::Z);
        assert!(frame.particles.is_empty());
        assert_eq!(frame.geometry[0].mesh.indices.len(), 36);
    }

    #[test]
    fn gravity_drag_and_field_stop_affect_real_particle_motion() {
        let mut emitter = emitter();
        if let EffectKind::Particle(p) = &mut emitter.effect.kind {
            p.rate = 0.0;
            p.bursts = vec![Burst {
                count: 1,
                ..Default::default()
            }];
            p.gravity = [0.0, -10.0, 0.0];
            p.drag = 1.0;
        }
        let wind = EffectInput {
            entity: EntityId::new(),
            transform: Mat4::IDENTITY,
            effect: EffectComponent::new(EffectKind::Wind(ForceField {
                global: true,
                strength: 5.0,
                ..Default::default()
            })),
        };
        let mut runtime = EffectRuntime::new();
        runtime.update(0.1, &[emitter.clone(), wind.clone()], Vec3::Z);
        let first = runtime.frame().particles[0].clone();
        assert!(first.position.y < 0.0);
        assert!(first.velocity.x > 0.0);
        runtime.control(wind.entity, PlaybackCommand::Stop);
        runtime.update(0.1, &[emitter, wind], Vec3::Z);
        assert!(runtime.frame().particles[0].velocity.x < first.velocity.x);
    }

    #[test]
    fn atlas_frames_and_camera_targets_follow_configuration() {
        assert_eq!(flipbook_rect([2, 2], 4.0, 0.75), [0.5, 0.5, 1.0, 1.0]);
        let camera = EntityId::new();
        let flash = EffectInput {
            entity: EntityId::new(),
            transform: Mat4::IDENTITY,
            effect: EffectComponent::new(EffectKind::ScreenFlash(ScreenFlash {
                target: Some(camera),
                ..Default::default()
            })),
        };
        let mut runtime = EffectRuntime::new();
        runtime.set_camera_target(Some(EntityId::new()));
        assert_eq!(
            runtime
                .update(0.1, std::slice::from_ref(&flash), Vec3::ZERO)
                .post
                .overlay[3],
            0.0
        );
        runtime.set_camera_target(Some(camera));
        assert!(runtime.update(0.1, &[flash], Vec3::ZERO).post.overlay[3] > 0.0);
    }
    #[test]
    #[ignore = "explicit performance observation; no timing threshold"]
    fn ten_thousand_particle_performance_smoke() {
        let mut input = emitter();
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.rate = 0.0;
            p.max_particles = 10000;
            p.lifetime = [60.0; 2];
            p.bursts = vec![Burst {
                count: 10000,
                ..Default::default()
            }];
        }
        let mut runtime = EffectRuntime::new();
        runtime.update(0.0, std::slice::from_ref(&input), Vec3::Z);
        let start = std::time::Instant::now();
        for _ in 0..120 {
            runtime.update(1.0 / 60.0, std::slice::from_ref(&input), Vec3::Z);
            assert_eq!(runtime.particle_count(), 10000);
        }
        eprintln!(
            "10,000 CPU particles, 120 frames: {:?}; final particle records: {} (renderer batches by blend)",
            start.elapsed(),
            runtime.frame().particles.len()
        );
    }

    #[test]
    fn prewarm_and_parameter_bindings_simulate_before_first_visible_frame() {
        let mut input = emitter();
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.prewarm = true;
            p.prewarm_time = 1.0;
            p.speed = [1.0; 2];
        }
        let mut runtime = EffectRuntime::new();
        let frame = runtime.update(0.0, std::slice::from_ref(&input), Vec3::Z);
        assert!(frame.particles.len() >= 9);
        assert!(frame.particles.iter().any(|p| p.position.y > 0.5));
        assert!(frame.events.is_empty());
        input
            .effect
            .common
            .parameters
            .insert("emission_rate".into(), EffectParameter::Scalar(100.0));
        let before = runtime.particle_count();
        runtime.update(0.1, &[input], Vec3::Z);
        assert!(runtime.particle_count() >= before + 9);
    }
}
