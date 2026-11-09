#[path = "cpu_geometry.rs"]
mod cpu_geometry;
use super::{
    CollisionResponse, EffectAppearance, EffectComponent, EffectEvent, EffectEventKind,
    EffectFrame, EffectGeometry, EffectInput, EffectKind, ForceField, ForceType,
    GpuParticleCollider, GpuParticleEmitterFrame, GpuParticleSpawn, GpuParticleState,
    ParticleBackend, ParticleBackendStatus, ParticleCollisionQuality, ParticleEmitter,
    ParticleInstance, ParticleOffscreenBehavior, ParticleSimulationMode, ParticleTrigger,
    PlaybackCommand, PlaybackState, PostProcessSettings, QualityLevel, RenderWaterDisturbance,
    RippleWaterSurface, SimulationSpace, SpawnShape, VolumeShape, WaterDisturbance, WeatherType,
    water_surface_mesh,
};
use crate::{EntityId, PrimitiveMesh};
use glam::{Mat4, Vec3, Vec3Swizzles};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::f32::consts::TAU;

const MAX_EVENTS: usize = 8192;
const MAX_PENDING_GPU_STEPS: usize = 120;
static NEXT_GPU_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static NEXT_GPU_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
fn gpu_epoch() -> u64 {
    NEXT_GPU_EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

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
    let sampled;
    let points = if points.len() > 2048 {
        sampled = (0..2048)
            .map(|i| points[i * (points.len() - 1) / 2047])
            .collect::<Vec<_>>();
        &sampled
    } else {
        points
    };
    let count = points.len();
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
    color_random: f32,
    position: Vec3,
    velocity: Vec3,
    age: f32,
    lifetime: f32,
    size: f32,
    rotation: f32,
    rotation3: Vec3,
    angular_velocity3: Vec3,
    scale: Vec3,
    mesh: Option<crate::AssetId>,
    material: Option<crate::AssetId>,
    gpu_slot: u32,
    stuck: bool,
}

#[derive(Clone, Debug)]
struct WeatherDeposit {
    position: Vec3,
    normal: Vec3,
    age: f32,
    size: f32,
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "independent lifecycle and backend state flags"
)]
#[derive(Clone, Debug)]
struct Instance {
    state: PlaybackState,
    time: f32,
    emission: f32,
    rng: Random,
    seed: u64,
    particles: Vec<Particle>,
    deposits: Vec<WeatherDeposit>,
    points: VecDeque<(Vec3, f32)>,
    previous: Option<Vec3>,
    began: bool,
    completed_event: bool,
    delay: f32,
    kind: std::mem::Discriminant<EffectKind>,
    prewarmed: bool,
    backend: ParticleBackend,
    gpu_epoch: u64,
    gpu_sequence: u64,
    gpu_pending: VecDeque<GpuParticleEmitterFrame>,
    gpu_free_slots: Vec<u32>,
    gpu_next_slot: u32,
    gpu_capacity: u32,
    gpu_overflow: bool,
    gpu_last_frame: Option<GpuParticleEmitterFrame>,
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
            deposits: Vec::new(),
            points: VecDeque::new(),
            previous: None,
            began: false,
            completed_event: false,
            delay: effect.common.delay,
            kind: std::mem::discriminant(&effect.kind),
            prewarmed: false,
            backend: ParticleBackend::Cpu,
            gpu_epoch: gpu_epoch(),
            gpu_sequence: 0,
            gpu_pending: VecDeque::new(),
            gpu_free_slots: Vec::new(),
            gpu_next_slot: 0,
            gpu_capacity: 0,
            gpu_overflow: false,
            gpu_last_frame: None,
        }
    }
    fn reset(&mut self) {
        self.time = -self.delay;
        self.emission = 0.0;
        self.rng = Random::new(self.seed);
        self.particles.clear();
        self.deposits.clear();
        self.points.clear();
        self.previous = None;
        self.began = false;
        self.completed_event = false;
        self.prewarmed = false;
        self.gpu_epoch = gpu_epoch();
        self.gpu_sequence = 0;
        self.gpu_pending.clear();
        self.gpu_free_slots.clear();
        self.gpu_next_slot = 0;
        self.gpu_overflow = false;
        self.gpu_last_frame = None;
    }
}

/// Retain at most 64 empty buffers and 100,000 particle slots per scene.
fn recycle_particle_buffer(pool: &mut Vec<Vec<Particle>>, mut particles: Vec<Particle>) {
    let capacity = particles.capacity();
    if capacity == 0
        || pool.len() >= 64
        || pool
            .iter()
            .map(Vec::capacity)
            .sum::<usize>()
            .saturating_add(capacity)
            > 100_000
    {
        return;
    }
    particles.clear();
    pool.push(particles);
}

/// Retained CPU simulation state for one scene. Cosmetic collisions never modify
/// authoritative gameplay physics. Destroying an entity removes all its state on
/// the next update; `clear` releases it immediately at scene unload.
#[derive(Debug)]
pub struct EffectRuntime {
    instances: BTreeMap<EntityId, Instance>,
    particle_pool: Vec<Vec<Particle>>,
    frame: EffectFrame,
    time: f32,
    camera_target: Option<EntityId>,
    visibility_layers: u32,
    gpu_context: Option<u64>,
    parent_transforms: BTreeMap<EntityId, Mat4>,
}
impl Default for EffectRuntime {
    fn default() -> Self {
        Self {
            instances: BTreeMap::new(),
            particle_pool: Vec::new(),
            frame: EffectFrame::default(),
            time: 0.0,
            camera_target: None,
            visibility_layers: u32::MAX,
            gpu_context: None,
            parent_transforms: BTreeMap::new(),
        }
    }
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
    /// Supplies the current parent reference frames used by local trail history.
    pub fn set_parent_transforms(&mut self, transforms: &BTreeMap<EntityId, Mat4>) {
        self.parent_transforms.clone_from(transforms);
    }
    /// Selects camera-specific impulses and overlays. `None` previews all effects.
    pub fn set_camera_target(&mut self, target: Option<EntityId>) {
        self.camera_target = target;
    }
    /// Changes render visibility only; cosmetic forces and emission keep running.
    pub fn set_visibility_layers(&mut self, layers: u32) {
        self.visibility_layers = layers;
    }
    /// Capability selection restarts GPU-owned instances when the renderer changes.
    pub fn set_gpu_particle_context(&mut self, context: Option<u64>) {
        if self.gpu_context != context {
            for instance in self.instances.values_mut() {
                if instance.backend == ParticleBackend::Gpu {
                    instance.reset();
                    instance.backend = ParticleBackend::Cpu;
                }
            }
        }
        self.gpu_context = context;
    }
    pub fn set_gpu_particle_support(&mut self, supported: bool) {
        self.set_gpu_particle_context(supported.then_some(1));
    }
    /// Releases queued fixed steps only after the renderer confirms dispatch.
    pub fn acknowledge_gpu_particles(&mut self, acknowledgements: &[(EntityId, u64)]) {
        for (entity, sequence) in acknowledgements {
            if let Some(instance) = self.instances.get_mut(entity) {
                while instance
                    .gpu_pending
                    .front()
                    .is_some_and(|command| command.sequence <= *sequence)
                {
                    instance.gpu_pending.pop_front();
                }
            }
        }
    }
    pub fn remove(&mut self, entity: EntityId) {
        let removed = self.instances.remove(&entity);
        let contributed = removed.as_ref().is_some_and(|instance| {
            matches!(
                instance.state,
                PlaybackState::Playing | PlaybackState::Paused
            )
        });
        if let Some(instance) = removed {
            recycle_particle_buffer(&mut self.particle_pool, instance.particles);
        }
        self.frame.geometry.retain(|v| v.entity != entity);
        self.frame.particles.retain(|v| v.entity != entity);
        self.frame.gpu_emitters.retain(|v| v.entity != entity);
        self.frame.particle_backends.retain(|v| v.entity != entity);
        self.frame.events.retain(|v| v.entity != entity);
        self.frame.fog.retain(|v| v.entity != entity);
        self.frame.probes.retain(|v| v.entity != entity);
        self.frame.decals.retain(|v| v.entity != entity);
        self.frame.water.retain(|v| v.entity != entity);
        self.frame.distortions.retain(|v| v.entity != entity);
        self.frame.overlays.retain(|v| v.entity != entity);
        self.frame.effect_layers.remove(&entity);
        self.frame.effect_render_distances.remove(&entity);
        self.frame.wind_zones.retain(|v| v.entity != entity);
        // Inactive sources and composite roots contributed no aggregate modifiers.
        // Preserve adjustments still owned by other active effects.
        if !contributed {
            return;
        }
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
        let pool = &mut self.particle_pool;
        let current = self.instances.entry(input.entity).or_insert_with(|| {
            let mut instance = Instance::new(&input.effect);
            if let Some(particles) = pool.pop() {
                instance.particles = particles;
            }
            instance
        });
        if current.seed != input.effect.common.random_seed
            || current.kind != std::mem::discriminant(&input.effect.kind)
        {
            let mut particles = std::mem::take(&mut current.particles);
            particles.clear();
            *current = Instance::new(&input.effect);
            current.particles = particles;
        }
        current.delay = input.effect.common.delay;
    }
    pub fn clear(&mut self) {
        self.instances.clear();
        self.particle_pool.clear();
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
        self.frame.effect_layers = inputs
            .iter()
            .map(|input| (input.entity, input.effect.common.visibility_layers))
            .collect();
        self.frame.geometry.clear();
        self.frame.particles.clear();
        self.frame.gpu_emitters.clear();
        self.frame.particle_backends.clear();
        self.frame.effect_render_distances = inputs
            .iter()
            .map(|input| {
                (
                    input.entity,
                    (
                        input.transform.transform_point3(Vec3::ZERO),
                        0.0,
                        input.effect.common.render_distance,
                    ),
                )
            })
            .collect();
        self.frame.particle_budget_exhausted = false;
        self.frame.camera_visibility_layers = Some(self.visibility_layers);
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
        self.frame.wind_zones.clear();
        self.frame.distortions.clear();
        self.frame.overlays.clear();
        self.frame.post = PostProcessSettings::default();
        self.frame.camera_offset = Vec3::ZERO;
        self.frame.camera_rotation = Vec3::ZERO;
        let alive: BTreeSet<_> = inputs.iter().map(|i| i.entity).collect();
        let pool = &mut self.particle_pool;
        self.instances.retain(|id, instance| {
            if alive.contains(id) {
                true
            } else {
                recycle_particle_buffer(pool, std::mem::take(&mut instance.particles));
                false
            }
        });
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
        let mut global_particle_budget = 100_000usize.saturating_sub(
            self.instances
                .values()
                .map(|instance| instance.particles.len())
                .sum::<usize>(),
        );
        let mut gpu_budget = GpuParticleEmitterFrame::MAX_TOTAL_PARTICLES;
        for input in inputs {
            let instance = self
                .instances
                .entry(input.entity)
                .or_insert_with(|| Instance::new(&input.effect));
            let common = &input.effect.common;
            if instance.state == PlaybackState::Disposed {
                continue;
            }
            if !common.enabled {
                if instance.backend == ParticleBackend::Gpu {
                    gpu_budget = gpu_budget.saturating_sub(instance.gpu_capacity);
                    append_gpu_commands(instance, &mut self.frame, false);
                }
                continue;
            }
            let mut gpu_selected = false;
            if let Some(emitter) = match &input.effect.kind {
                EffectKind::Particle(p) | EffectKind::MeshParticle(p) => Some(p),
                EffectKind::Weather(w) => Some(&w.emitter),
                _ => None,
            } {
                let capacity = match common.quality {
                    QualityLevel::Low => (emitter.max_particles / 2).max(1),
                    _ => emitter.max_particles,
                }
                .min(GpuParticleEmitterFrame::MAX_PARTICLES);
                let queue_overflow = instance.gpu_pending.len() >= MAX_PENDING_GPU_STEPS
                    || instance
                        .gpu_pending
                        .iter()
                        .map(|command| command.spawns.len())
                        .sum::<usize>()
                        >= 100_000;
                let mut reason = if instance.gpu_overflow || queue_overflow {
                    Some("GPU renderer did not acknowledge the bounded command queue; restart to retry GPU simulation".into())
                } else if capacity == 0 {
                    Some("GPU particles require positive capacity".into())
                } else {
                    gpu_fallback_reason(emitter, input, inputs, &active_fields, self.gpu_context)
                };
                if capacity > gpu_budget {
                    reason =
                        Some("GPU scene particle budget exceeded; using CPU simulation".into());
                }
                let target = if emitter.simulation_mode != ParticleSimulationMode::Cpu
                    && reason.is_none()
                    && matches!(
                        instance.state,
                        PlaybackState::Playing | PlaybackState::Paused
                    ) {
                    ParticleBackend::Gpu
                } else {
                    ParticleBackend::Cpu
                };
                if instance.backend != target
                    || (target == ParticleBackend::Gpu && instance.gpu_capacity != capacity)
                {
                    instance.reset();
                    instance.backend = target;
                    instance.gpu_capacity = capacity;
                    if queue_overflow {
                        instance.gpu_overflow = true;
                    }
                }
                gpu_selected = target == ParticleBackend::Gpu;
                if gpu_selected {
                    gpu_budget = gpu_budget.saturating_sub(capacity);
                }
                self.frame.particle_backends.push(ParticleBackendStatus {
                    entity: input.entity,
                    requested: emitter.simulation_mode,
                    active: target,
                    reason: if emitter.simulation_mode == ParticleSimulationMode::Cpu {
                        None
                    } else {
                        reason
                    },
                });
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
            self.frame.effect_render_distances.insert(
                input.entity,
                (render_center, render_radius, common.render_distance),
            );
            let in_render_distance = render_center.distance(camera) - render_radius
                <= common.render_distance
                || inputs.iter().any(|probe_input| {
                    let EffectKind::ReflectionProbe(probe) = &probe_input.effect.kind else {
                        return false;
                    };
                    if !active_fields.contains(&probe_input.entity)
                        || !probe_input.effect.common.visible
                        || probe.visibility_layers & common.visibility_layers == 0
                    {
                        return false;
                    }
                    let capture = probe_input
                        .transform
                        .transform_point3(Vec3::from(probe.capture_position));
                    render_center.distance(capture) - render_radius
                        <= common
                            .render_distance
                            .min(probe_input.effect.common.render_distance)
                });
            let render_visible = common.visible && in_render_distance;
            let offscreen_behavior = match &input.effect.kind {
                EffectKind::Particle(p) | EffectKind::MeshParticle(p) => p.offscreen_behavior,
                EffectKind::Weather(w) => w.emitter.offscreen_behavior,
                _ => ParticleOffscreenBehavior::Simulate,
            };
            let has_visible_view = (common.visibility_layers & self.visibility_layers != 0
                && render_center.distance(camera) - render_radius <= common.render_distance)
                || inputs.iter().any(|probe_input| {
                    if let EffectKind::ReflectionProbe(probe) = &probe_input.effect.kind {
                        active_fields.contains(&probe_input.entity)
                            && probe.visibility_layers & common.visibility_layers != 0
                            && render_center.distance(
                                probe_input
                                    .transform
                                    .transform_point3(Vec3::from(probe.capture_position)),
                            ) - render_radius
                                <= common
                                    .render_distance
                                    .min(probe_input.effect.common.render_distance)
                    } else {
                        false
                    }
                });
            let offscreen_paused =
                !has_visible_view && offscreen_behavior == ParticleOffscreenBehavior::Pause;
            let offscreen_clear =
                !has_visible_view && offscreen_behavior == ParticleOffscreenBehavior::Clear;
            if offscreen_clear {
                instance.deposits.clear();
                global_particle_budget = global_particle_budget
                    .saturating_add(instance.particles.len())
                    .min(100_000);
                instance.particles.clear();
            }
            let active_dt = if instance.state == PlaybackState::Playing && !offscreen_paused {
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
            if instance.time < 0.0 || offscreen_paused || offscreen_clear {
                emitting = false;
            }
            let mesh_emitter = match &input.effect.kind {
                EffectKind::Particle(p) | EffectKind::MeshParticle(p) => Some(Cow::Borrowed(p)),
                EffectKind::Weather(w) => {
                    let mut p = w.emitter.clone();
                    p.rate *= w.intensity.max(0.0);
                    if let Some(asset) = w.splash_effect {
                        p.sub_emitters.push(super::ParticleSubEmitter {
                            effect: asset,
                            trigger: ParticleTrigger::Collision,
                            probability: 1.0,
                            inherit_velocity: 0.0,
                        });
                    }
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
                let mut gpu_spawns = Vec::new();
                if offscreen_clear && gpu_selected {
                    instance.gpu_epoch = gpu_epoch();
                    instance.gpu_pending.clear();
                    instance.gpu_free_slots.clear();
                    instance.gpu_next_slot = 0;
                    instance.gpu_last_frame = None;
                }
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
                            false,
                            &mut gpu_spawns,
                            &mut global_particle_budget,
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
                    gpu_selected,
                    &mut gpu_spawns,
                    &mut global_particle_budget,
                    &mut self.frame,
                );
                if gpu_selected
                    && instance
                        .gpu_pending
                        .iter()
                        .map(|command| command.spawns.len())
                        .sum::<usize>()
                        + gpu_spawns.len()
                        > 100_000
                {
                    instance.reset();
                    instance.backend = ParticleBackend::Cpu;
                    instance.gpu_overflow = true;
                    gpu_selected = false;
                    if let Some(status) = self.frame.particle_backends.last_mut() {
                        status.active = ParticleBackend::Cpu;
                        status.reason = Some(
                            "unacknowledged GPU birth budget exceeded; CPU simulation restarted"
                                .into(),
                        );
                    }
                }
                if gpu_selected {
                    let mut command = GpuParticleEmitterFrame {
                        entity: input.entity,
                        epoch: instance.gpu_epoch,
                        sequence: instance.gpu_sequence,
                        capacity: instance.gpu_capacity,
                        live_count: u32::try_from(instance.particles.len()).unwrap_or(u32::MAX),
                        delta: emission_dt,
                        visible: render_visible,
                        visibility_layers: common.visibility_layers,
                        render_bounds: (render_center, render_radius, common.render_distance),
                        transform,
                        simulation_space: common.simulation_space,
                        gravity: emitter.gravity,
                        force: emitter.force,
                        drag: emitter.drag,
                        velocity_limit: emitter.velocity_limit,
                        angular_velocity: emitter.angular_velocity,
                        stretch: emitter.stretch,
                        appearance: emitter.appearance.clone(),
                        flipbook: emitter.flipbook,
                        flipbook_fps: emitter.flipbook_fps,
                        spawns: gpu_spawns,
                        colliders: gpu_colliders(emitter, inputs, &active_fields),
                    };
                    let render_changed = instance.gpu_last_frame.as_ref().is_some_and(|last| {
                        last.transform != command.transform
                            || last.visibility_layers != command.visibility_layers
                            || last.render_bounds != command.render_bounds
                            || last.appearance != command.appearance
                            || last.stretch.to_bits() != command.stretch.to_bits()
                    });
                    if emission_dt > 0.0
                        || !command.spawns.is_empty()
                        || instance.gpu_last_frame.is_none()
                        || render_changed
                    {
                        // Acknowledgements can arrive after restart/seed edits. Never
                        // reuse a sequence even when the instance or epoch is replaced.
                        instance.gpu_sequence =
                            NEXT_GPU_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        command.sequence = instance.gpu_sequence;
                        instance.gpu_last_frame = Some(command.clone());
                        instance.gpu_pending.push_back(command);
                    }
                    append_gpu_commands(instance, &mut self.frame, render_visible);
                }
                if render_visible && !gpu_selected {
                    let mut generated_mesh = make_geometry(
                        input.entity,
                        PrimitiveMesh::default(),
                        &emitter.appearance,
                        0.0,
                        self.time,
                    );
                    generated_mesh.pbr = matches!(input.effect.kind, EffectKind::MeshParticle(_));
                    generated_mesh.cast_shadows = emitter.cast_shadows;
                    generated_mesh.receive_shadows = emitter.receive_shadows;
                    for p in &instance.particles {
                        let normalized = (p.age / p.lifetime).clamp(0.0, 1.0);
                        let mut color = emitter.appearance.sample_color(normalized, p.color_random);
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
                            && p.mesh.is_none()
                        {
                            append_cube(
                                &mut generated_mesh,
                                particle_position,
                                size,
                                p.rotation3
                                    + Vec3::Z * emitter.rotation_over_lifetime.sample(normalized),
                                p.scale,
                                color,
                            );
                            continue;
                        }
                        self.frame.particles.push(ParticleInstance {
                            entity: input.entity,
                            position: particle_position,
                            velocity: if common.simulation_space == SimulationSpace::Local {
                                transform.transform_vector3(movement_velocity(p, emitter))
                            } else {
                                movement_velocity(p, emitter)
                            },
                            stretch: emitter.stretch.max(0.0),
                            uv_rect: flipbook_rect(emitter.flipbook, emitter.flipbook_fps, p.age),
                            rotation: p.rotation
                                + emitter.rotation_over_lifetime.sample(normalized),
                            rotation3: p.rotation3
                                + Vec3::Z * emitter.rotation_over_lifetime.sample(normalized),
                            scale: p.scale,
                            size: p.size * emitter.appearance.size.sample(normalized).max(0.0),
                            color,
                            mesh: p.mesh,
                            material: p.material.or(emitter.appearance.material),
                            texture: emitter.appearance.texture,
                            emission: emitter.appearance.emission,
                            blend: emitter.appearance.blend,
                            soft_particle_distance: emitter.soft_particle_distance,
                            cast_shadows: emitter.cast_shadows,
                            receive_shadows: emitter.receive_shadows,
                            wind: Vec3::ZERO,
                            sort_mode: emitter.sorting,
                            sort_key: p.age,
                        });
                    }
                    if let EffectKind::Weather(weather) = &input.effect.kind {
                        for deposit in &instance.deposits {
                            let progress =
                                (deposit.age / weather.accumulation_lifetime).clamp(0.0, 1.0);
                            let mut color = emitter
                                .appearance
                                .sample_color(progress, appearance_random(input.entity));
                            color[3] *= (1.0 - progress) * weather.accumulation;
                            let rotation = glam::Quat::from_rotation_arc(Vec3::Y, deposit.normal);
                            let tangent = rotation * Vec3::X * deposit.size;
                            let bitangent = rotation * Vec3::Z * deposit.size;
                            let base = generated_mesh.mesh.positions.len() as u32;
                            for offset in [
                                -tangent - bitangent,
                                tangent - bitangent,
                                tangent + bitangent,
                                -tangent + bitangent,
                            ] {
                                generated_mesh
                                    .mesh
                                    .positions
                                    .push((deposit.position + offset).to_array());
                                generated_mesh.mesh.normals.push(deposit.normal.to_array());
                                generated_mesh.mesh.tex_coords.push([0.0, 0.0]);
                                generated_mesh.colors.push(color);
                            }
                            generated_mesh.mesh.indices.extend([
                                base,
                                base + 2,
                                base + 1,
                                base,
                                base + 3,
                                base + 2,
                            ]);
                        }
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
                        let parent = if common.simulation_space == SimulationSpace::Local {
                            self.parent_transforms
                                .get(&input.entity)
                                .copied()
                                .unwrap_or(Mat4::IDENTITY)
                        } else {
                            Mat4::IDENTITY
                        };
                        let local_position = parent.inverse().transform_point3(position);
                        if emitting && trail.emitting {
                            if instance.points.back().is_some_and(|(point, _)| {
                                parent.transform_point3(*point).distance(position)
                                    > trail.teleport_distance
                            }) {
                                instance.points.clear();
                            }
                            if instance.points.back().is_none_or(|(point, _)| {
                                parent.transform_point3(*point).distance(position)
                                    >= trail.min_vertex_distance.max(0.001)
                            }) {
                                instance.points.push_back((local_position, 0.0));
                            }
                            while instance.points.len() > trail.max_points.min(65536) as usize {
                                instance.points.pop_front();
                            }
                        }
                        if render_visible
                            && let Some(geometry) = cpu_geometry::trail_geometry(
                                input.entity,
                                &instance.points,
                                trail,
                                parent,
                                transform,
                                camera,
                                self.time,
                            )
                        {
                            self.frame.geometry.push(geometry);
                        }
                    }
                    EffectKind::Beam(beam) if render_visible => {
                        let endpoint = |target: Option<EntityId>, offset: [f32; 3]| {
                            target
                                .and_then(|id| {
                                    targets.get(&id).copied().or_else(|| {
                                        inputs
                                            .iter()
                                            .find(|input| input.entity == id)
                                            .map(|input| input.transform)
                                    })
                                })
                                .unwrap_or(transform)
                                .transform_point3(Vec3::from(offset))
                        };
                        let start = endpoint(beam.start_target, beam.start);
                        let mut end = endpoint(beam.end_target, beam.end);
                        if beam.endpoint_collision
                            && let Some((hit, _)) = cpu_geometry::raycast_volumes(
                                start,
                                end,
                                inputs,
                                &active_fields,
                                beam.collision_layers,
                            )
                        {
                            end = hit;
                        }
                        self.frame.geometry.extend(cpu_geometry::beam_geometry(
                            input.entity,
                            beam,
                            start,
                            end,
                            common.random_seed,
                            camera,
                            self.time,
                            t,
                        ));
                    }
                    EffectKind::Shockwave(ring) if render_visible => {
                        self.frame.geometry.push(cpu_geometry::shockwave_geometry(
                            input.entity,
                            ring,
                            transform,
                            age,
                            t,
                            self.time,
                            inputs,
                            &active_fields,
                        ));
                        let radius = ring.final_radius.max(ring.initial_radius).max(0.01);
                        let influence =
                            (1.0 - position.distance(camera) / (radius * 4.0)).clamp(0.0, 1.0);
                        if common.visibility_layers & self.visibility_layers != 0
                            && render_center.distance(camera) - render_radius
                                <= common.render_distance
                        {
                            self.frame.post.distortion += ring.screen_distortion
                                * influence
                                * ring.appearance.opacity.sample(t);
                        }
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
                        let mut local_points: Vec<_> =
                            ribbon.points.iter().copied().map(Vec3::from).collect();
                        cpu_geometry::animate_ribbon_points(
                            &mut local_points,
                            ribbon.animation_speed,
                            self.time,
                        );
                        let points: Vec<_> = local_points
                            .into_iter()
                            .map(|point| transform.transform_point3(point))
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
                        let start = transform.transform_point3(Vec3::from(tracer.start_position));
                        let endpoint = tracer
                            .end_target
                            .and_then(|entity| targets.get(&entity))
                            .map(|matrix| matrix.transform_point3(Vec3::ZERO))
                            .or_else(|| {
                                tracer
                                    .end_position
                                    .map(|point| transform.transform_point3(Vec3::from(point)))
                            });
                        let head =
                            endpoint.map_or(start + velocity * age, |end| start.lerp(end, t));
                        let direction = endpoint
                            .map_or(velocity, |end| end - start)
                            .normalize_or_zero();
                        let length = tracer.length + velocity.length() * tracer.velocity_stretch;
                        let tail = head - direction * length;
                        let points: Vec<_> = (0..=8)
                            .map(|index| tail.lerp(head, index as f32 / 8.0))
                            .collect();
                        let widths: Vec<_> = (0..=8)
                            .map(|index| tracer.width * index as f32 / 8.0)
                            .collect();
                        let mut geometry = make_geometry(
                            input.entity,
                            ribbon_mesh(&points, &widths, 0, camera),
                            &tracer.appearance,
                            t,
                            self.time,
                        );
                        geometry.colors = vec![geometry.color; geometry.mesh.positions.len()];
                        for (index, color) in geometry.colors.iter_mut().enumerate() {
                            color[3] *= ((index / 2) as f32 / 8.0).powf(tracer.tail_fade);
                        }
                        self.frame.geometry.push(geometry);
                    }
                    EffectKind::FoliageScatter(scatter) if render_visible => {
                        let surface = scatter
                            .surface_target
                            .and_then(|entity| targets.get(&entity).copied())
                            .unwrap_or(transform);
                        let mut views = vec![camera];
                        views.extend(inputs.iter().filter_map(|probe_input| {
                            if let EffectKind::ReflectionProbe(probe) = &probe_input.effect.kind {
                                (active_fields.contains(&probe_input.entity)
                                    && probe.visibility_layers & common.visibility_layers != 0)
                                    .then(|| {
                                        probe_input
                                            .transform
                                            .transform_point3(Vec3::from(probe.capture_position))
                                    })
                            } else {
                                None
                            }
                        }));
                        let samples = cpu_geometry::scatter_samples(
                            scatter,
                            transform,
                            surface,
                            common.random_seed,
                            &views,
                            common.render_distance,
                        );
                        let mut foliage = make_geometry(
                            input.entity,
                            PrimitiveMesh::default(),
                            &scatter.appearance,
                            0.0,
                            self.time,
                        );
                        foliage.pbr = true;
                        foliage.cast_shadows = scatter.cast_shadows;
                        foliage.receive_shadows = scatter.receive_shadows;
                        for sample in samples {
                            let wind = sample_active_wind_for(
                                inputs,
                                &active_fields,
                                sample.position,
                                self.time,
                                common.visibility_layers,
                                true,
                            ) * scatter.wind_response;
                            if sample.mesh.is_none() {
                                append_foliage(
                                    &mut foliage,
                                    sample.position,
                                    sample.size,
                                    sample.rotation,
                                    wind * 0.03,
                                    scatter
                                        .appearance
                                        .sample_color(0.0, appearance_random(input.entity)),
                                );
                            } else {
                                self.frame.particles.push(ParticleInstance {
                                    entity: input.entity,
                                    position: sample.position,
                                    velocity: Vec3::ZERO,
                                    stretch: 0.0,
                                    uv_rect: [0.0, 0.0, 1.0, 1.0],
                                    rotation: sample.rotation.z,
                                    rotation3: sample.rotation,
                                    scale: Vec3::ONE,
                                    size: sample.size,
                                    color: scatter
                                        .appearance
                                        .sample_color(0.0, appearance_random(input.entity)),
                                    mesh: sample.mesh,
                                    material: sample.material,
                                    texture: scatter.appearance.texture,
                                    emission: scatter.appearance.emission,
                                    blend: scatter.appearance.blend,
                                    cast_shadows: scatter.cast_shadows,
                                    receive_shadows: scatter.receive_shadows,
                                    wind,
                                    sort_mode: super::ParticleSortMode::Distance,
                                    sort_key: 0.0,
                                    soft_particle_distance: 0.0,
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
                        velocity: Vec3::ZERO,
                        child_effect: None,
                    });
                    instance.completed_event = true;
                }
                if instance.particles.is_empty()
                    && instance.points.is_empty()
                    && instance.deposits.is_empty()
                {
                    instance.state = PlaybackState::Completed;
                }
            }
        }
        // All emitters sharing a water receiver share its strictest authored budget.
        let mut water_limits = BTreeMap::new();
        for water in &self.frame.water {
            water_limits
                .entry(water.target)
                .and_modify(|limit: &mut u32| {
                    *limit = (*limit).min(water.disturbance.settings.max_disturbances);
                })
                .or_insert(water.disturbance.settings.max_disturbances);
        }
        let mut water_counts = BTreeMap::new();
        self.frame.water.retain(|water| {
            let count = water_counts.entry(water.target).or_insert(0usize);
            *count += 1;
            *count <= water_limits[&water.target].clamp(1, 256) as usize
        });
        let mut ordered: Vec<_> = inputs.iter().collect();
        ordered.sort_by_key(|i| (i.effect.common.priority, i.entity));
        for input in ordered {
            let target = match &input.effect.kind {
                EffectKind::CameraShake(v) => v.target,
                EffectKind::ScreenFlash(v) => v.target,
                EffectKind::PostProcess(v) => v.camera,
                _ => None,
            };
            if target.is_some() && self.camera_target.is_some() && target != self.camera_target {
                continue;
            }
            if let Some(state) = self.instances.get(&input.entity)
                && input.effect.common.enabled
                && input.effect.common.visible
                && (!matches!(
                    input.effect.kind,
                    EffectKind::CameraShake(_)
                        | EffectKind::ScreenFlash(_)
                        | EffectKind::PostProcess(_)
                        | EffectKind::HeatHaze(_)
                ) || input.effect.common.visibility_layers & self.visibility_layers != 0)
                && state.time >= 0.0
                && matches!(state.state, PlaybackState::Playing | PlaybackState::Paused)
            {
                let elapsed = if input.effect.common.looping {
                    state.time % input.effect.common.duration.max(0.001)
                } else {
                    state.time
                };
                if let EffectKind::Wind(field) = &input.effect.kind
                    && field.affect_materials
                {
                    self.frame.wind_zones.push(super::RenderWindZone {
                        entity: input.entity,
                        transform: input.transform,
                        field: field.clone(),
                    });
                }
                super::modifiers::extract_modifier(
                    input,
                    camera,
                    elapsed,
                    (elapsed / input.effect.common.duration.max(0.001)).clamp(0.0, 1.0),
                    &mut self.frame,
                );
                if matches!(input.effect.kind, EffectKind::ReflectionProbe(_))
                    && let Some(probe) = self
                        .frame
                        .probes
                        .iter_mut()
                        .rev()
                        .find(|probe| probe.entity == input.entity)
                {
                    probe.time = state.time;
                }
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

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "24-bit stable entity hash is exactly representable as f32"
)]
fn appearance_random(entity: EntityId) -> f32 {
    ((entity.as_uuid().as_u128() & 0x00ff_ffff) as u32) as f32 / 16_777_215.0
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
            let mut c = appearance.sample_color(uv[0] / length, appearance_random(entity));
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
    let mut color = appearance.sample_color(t, appearance_random(entity));
    color[3] *= appearance.opacity.sample(t).clamp(0.0, 1.0);
    EffectGeometry {
        pbr: appearance.material.is_some(),
        cast_shadows: true,
        receive_shadows: true,
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
    rotation: Vec3,
    scale: Vec3,
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
        * Mat4::from_quat(glam::Quat::from_euler(
            glam::EulerRot::XYZ,
            rotation.x,
            rotation.y,
            rotation.z,
        ))
        * Mat4::from_scale(scale * size);
    let base =
        u32::try_from(geometry.mesh.positions.len()).expect("bounded effect geometry fits u32");
    geometry.mesh.positions.extend(
        mesh.positions
            .iter()
            .map(|p| transform.transform_point3(Vec3::from(*p)).to_array()),
    );
    geometry.mesh.normals.extend(mesh.normals.iter().map(|n| {
        Mat4::from_quat(glam::Quat::from_euler(
            glam::EulerRot::XYZ,
            rotation.x,
            rotation.y,
            rotation.z,
        ))
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
    rotation: Vec3,
    wind: Vec3,
    color: [f32; 4],
) {
    let orientation =
        glam::Quat::from_euler(glam::EulerRot::XYZ, rotation.x, rotation.y, rotation.z);
    let up = orientation * Vec3::Y;
    for angle in [0.0, std::f32::consts::FRAC_PI_2] {
        let blade_width = orientation * Vec3::new(angle.cos(), 0.0, angle.sin()) * size * 0.2;
        let top = position + up * size + wind;
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
        let normal = blade_width.cross(up).normalize_or_zero().to_array();
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
    sample_active_wind_for(inputs, active, position, time, layers, false)
}

fn sample_active_wind_for(
    inputs: &[EffectInput],
    active: &BTreeSet<EntityId>,
    position: Vec3,
    time: f32,
    layers: u32,
    foliage: bool,
) -> Vec3 {
    inputs
        .iter()
        .filter(|i| active.contains(&i.entity))
        .filter_map(|i| match &i.effect.kind {
            EffectKind::Wind(f)
                if f.layers & layers != 0
                    && if foliage {
                        f.affect_foliage
                    } else {
                        f.affect_particles
                    } =>
            {
                Some(field_force(f, i.transform, position, time))
            }
            _ => None,
        })
        .sum()
}

/// Samples render-material wind using the same spatial falloff and gusts as particles.
pub fn sample_render_wind(
    zones: &[super::RenderWindZone],
    position: Vec3,
    time: f32,
    layers: u32,
) -> Vec3 {
    zones
        .iter()
        .filter(|zone| zone.field.affect_materials && zone.field.layers & layers != 0)
        .map(|zone| field_force(&zone.field, zone.transform, position, time))
        .sum()
}

fn field_weight(field: &ForceField, transform: Mat4, position: Vec3) -> f32 {
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
        return 0.0;
    }
    if field.global {
        1.0
    } else {
        (1.0 - normalized).max(0.0).powf(field.falloff.max(0.0))
    }
}

fn field_force(field: &ForceField, transform: Mat4, position: Vec3, time: f32) -> Vec3 {
    let weight = field_weight(field, transform, position);
    let direction = transform
        .transform_vector3(Vec3::from(field.direction))
        .normalize_or_zero();
    let radial = (position - transform.transform_point3(Vec3::ZERO)).normalize_or_zero();
    let noise = Vec3::new(
        (position.z * field.frequency / field.noise_scale + time).sin(),
        (position.x * field.frequency / field.noise_scale + time * 1.37).sin(),
        (position.y * field.frequency / field.noise_scale - time * 0.93).cos(),
    );
    let force = match field.force_type {
        ForceType::Directional => direction,
        ForceType::Attraction => -radial,
        ForceType::Repulsion => radial,
        ForceType::Vortex => direction.cross(radial) * field.vortex_speed,
        ForceType::Turbulence => noise,
    };
    let gust = field.gust_strength * (0.5 + 0.5 * (time * field.gust_frequency * TAU).sin());
    (force * (field.strength + gust) + noise * field.turbulence) * weight
}

fn append_gpu_commands(instance: &Instance, frame: &mut EffectFrame, visible: bool) {
    if instance.gpu_pending.is_empty() {
        if let Some(last) = &instance.gpu_last_frame {
            let mut heartbeat = last.clone();
            heartbeat.spawns.clear();
            heartbeat.delta = 0.0;
            heartbeat.visible = visible;
            frame.gpu_emitters.push(heartbeat);
        }
    } else {
        frame
            .gpu_emitters
            .extend(instance.gpu_pending.iter().cloned().map(|mut command| {
                command.visible = visible;
                command
            }));
    }
}

fn gpu_fallback_reason(
    emitter: &ParticleEmitter,
    input: &EffectInput,
    inputs: &[EffectInput],
    active: &BTreeSet<EntityId>,
    context: Option<u64>,
) -> Option<String> {
    let reason = if context.is_none() {
        Some("renderer has no compute/storage particle support")
    } else if !matches!(input.effect.kind, EffectKind::Particle(_)) {
        Some("mesh particles and weather use the complete CPU simulator")
    } else if emitter.generate_events
        || emitter.sub_emitter.is_some()
        || !emitter.sub_emitters.is_empty()
    {
        Some("per-particle events and sub-emitters require CPU simulation")
    } else if emitter.appearance.color_parameter.is_some() {
        Some("random gradient parameters use complete CPU appearance evaluation")
    } else if emitter.prewarm {
        Some("prewarm requires CPU simulation")
    } else if emitter.noise != 0.0
        || emitter.turbulence != 0.0
        || inputs.iter().any(|field| {
            active.contains(&field.entity)
                && match &field.effect.kind {
                    EffectKind::Wind(wind) => {
                        emitter.wind_influence != 0.0
                            && wind.layers & input.effect.common.visibility_layers != 0
                    }
                    EffectKind::ForceField(force) => {
                        force.layers & input.effect.common.visibility_layers != 0
                    }
                    _ => false,
                }
        })
    {
        Some("noise, turbulence, force fields and wind require CPU simulation")
    } else if emitter.sorting != super::ParticleSortMode::None
        && emitter.appearance.blend == super::BlendMode::Alpha
    {
        Some(
            "per-particle sorting requires CPU simulation; use unsorted additive particles for GPU",
        )
    } else if emitter.soft_particle_distance > 0.0 {
        Some("soft scene-depth intersections use CPU billboard batching")
    } else if emitter.mesh.is_some() || !emitter.mesh_variants.is_empty() {
        Some("mesh particle instances require CPU simulation")
    } else if emitter.appearance.texture.is_some()
        || emitter.appearance.material.is_some()
        || emitter.flipbook != [1, 1]
    {
        Some("textured materials and flipbook sprites require CPU simulation")
    } else if emitter.appearance.color.keys.len() > GpuParticleEmitterFrame::MAX_CURVE_KEYS
        || emitter.appearance.size.keys.len() > GpuParticleEmitterFrame::MAX_CURVE_KEYS
        || emitter.appearance.opacity.keys.len() > GpuParticleEmitterFrame::MAX_CURVE_KEYS
    {
        Some("GPU appearance curves support at most 16 keys")
    } else if emitter
        .velocity_over_lifetime
        .iter()
        .flat_map(|c| &c.keys)
        .any(|key| key.value != 0.0)
        || emitter
            .rotation_over_lifetime
            .keys
            .iter()
            .any(|key| key.value != 0.0)
    {
        Some("lifetime motion curves require CPU simulation")
    } else if emitter.start_scale != [[1.0; 3]; 2]
        || emitter
            .start_rotation
            .iter()
            .any(|v| v[0] != 0.0 || v[1] != 0.0)
        || emitter.angular_velocity_range != [[0.0; 3]; 2]
    {
        Some("3D scale/rotation variation requires CPU simulation")
    } else if emitter.collision
        && (emitter.collision_quality == ParticleCollisionQuality::Discrete
            || emitter.collision_response == CollisionResponse::Kill)
    {
        Some("discrete and kill collisions require CPU simulation")
    } else {
        None
    };
    if let Some(reason) = reason {
        return Some(reason.into());
    }
    gpu_collision_fallback(emitter, inputs, active)
}

fn gpu_collision_fallback(
    emitter: &ParticleEmitter,
    inputs: &[EffectInput],
    active: &BTreeSet<EntityId>,
) -> Option<String> {
    if !emitter.collision {
        return None;
    }
    let mut count = 0;
    for field in inputs {
        if active.contains(&field.entity)
            && let EffectKind::CollisionVolume(volume) = &field.effect.kind
            && volume.layers & emitter.collision_layers != 0
        {
            count += 1;
            if volume.response == CollisionResponse::Kill
                || volume.quality == ParticleCollisionQuality::Discrete
                || volume.impact_effect.is_some()
            {
                return Some(
                    "collision events, discrete and kill volumes require CPU simulation".into(),
                );
            }
        }
    }
    (count > GpuParticleEmitterFrame::MAX_COLLIDERS)
        .then(|| "GPU particles support at most 16 collision volumes".into())
}

fn gpu_colliders(
    emitter: &ParticleEmitter,
    inputs: &[EffectInput],
    active: &BTreeSet<EntityId>,
) -> Vec<GpuParticleCollider> {
    if !emitter.collision {
        return Vec::new();
    }
    inputs
        .iter()
        .filter(|field| active.contains(&field.entity))
        .filter_map(|field| {
            let EffectKind::CollisionVolume(volume) = &field.effect.kind else {
                return None;
            };
            if volume.layers & emitter.collision_layers == 0 {
                return None;
            }
            Some(GpuParticleCollider {
                inverse_transform: field.transform.inverse(),
                transform: field.transform,
                dimensions: volume.dimensions,
                shape: volume.shape,
                bounce: emitter.bounce * volume.bounce,
                friction: 1.0 - (1.0 - emitter.friction) * (1.0 - volume.friction),
                response: if emitter.collision_response == CollisionResponse::Stick
                    || volume.response == CollisionResponse::Stick
                {
                    CollisionResponse::Stick
                } else {
                    CollisionResponse::Bounce
                },
            })
        })
        .collect()
}

fn random_vector_range(range: [[f32; 3]; 2], rng: &mut Random) -> Vec3 {
    Vec3::from(std::array::from_fn(|axis| {
        let minimum = range[0][axis];
        let maximum = range[1][axis];
        if minimum.to_bits() == maximum.to_bits() {
            minimum
        } else {
            rng.range(minimum, maximum)
        }
    }))
}

fn weighted_mesh(
    meshes: &[super::WeightedParticleMesh],
    rng: &mut Random,
) -> Option<(crate::AssetId, Option<crate::AssetId>)> {
    let total: f32 = meshes.iter().map(|v| v.weight.max(0.0)).sum();
    if total <= 0.0 {
        return None;
    }
    let mut choice = rng.unit() * total;
    for variant in meshes {
        choice -= variant.weight.max(0.0);
        if choice < 0.0 {
            return Some((variant.mesh, variant.material));
        }
    }
    meshes.last().map(|v| (v.mesh, v.material))
}

fn movement_velocity(particle: &Particle, emitter: &ParticleEmitter) -> Vec3 {
    if particle.stuck {
        return Vec3::ZERO;
    }
    let progress = (particle.age / particle.lifetime).clamp(0.0, 1.0);
    let added = Vec3::from(emitter.velocity_over_lifetime.sample(progress));
    (particle.velocity + added).clamp_length_max(emitter.velocity_limit.max(0.0))
}

fn discrete_contact(point: Vec3, shape: VolumeShape, half: Vec3) -> Option<Vec3> {
    match shape {
        VolumeShape::Plane => {
            (point.y <= 0.0 && point.x.abs() <= half.x && point.z.abs() <= half.z)
                .then_some(Vec3::Y)
        }
        VolumeShape::Sphere => {
            if point.length_squared() <= half.max_element().powi(2) {
                Some(if point.length_squared() > 1e-12 {
                    point.normalize()
                } else {
                    Vec3::Y
                })
            } else {
                None
            }
        }
        VolumeShape::Box => {
            if point.abs().cmple(half).all() {
                let distance = half - point.abs();
                let axis = if distance.x <= distance.y && distance.x <= distance.z {
                    0
                } else if distance.y <= distance.z {
                    1
                } else {
                    2
                };
                let mut normal = Vec3::ZERO;
                normal[axis] = if point[axis] < 0.0 { -1.0 } else { 1.0 };
                Some(normal)
            } else {
                None
            }
        }
        VolumeShape::Cylinder => {
            let radius = half.x.max(half.z);
            let radial = point.xz().length();
            if radial > radius || point.y.abs() > half.y {
                None
            } else if half.y - point.y.abs() < radius - radial {
                Some(Vec3::Y * if point.y < 0.0 { -1.0 } else { 1.0 })
            } else {
                Some(if radial > 1e-6 {
                    Vec3::new(point.x, 0.0, point.z) / radial
                } else {
                    Vec3::X
                })
            }
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "one bounded event dispatcher consistently handles all particle lifecycle triggers"
)]
fn particle_events(
    frame: &mut EffectFrame,
    entity: EntityId,
    kind: EffectEventKind,
    position: Vec3,
    velocity: Vec3,
    emitter: &ParticleEmitter,
    rng: &mut Random,
    impact: Option<crate::AssetId>,
) {
    let legacy = impact.or_else(|| {
        if rng.unit() < emitter.sub_emitter_probability {
            emitter.sub_emitter
        } else {
            None
        }
    });
    if (emitter.generate_events || legacy.is_some()) && frame.events.len() < MAX_EVENTS {
        frame.events.push(EffectEvent {
            entity,
            kind,
            position,
            velocity: Vec3::ZERO,
            child_effect: legacy,
        });
    }
    let trigger = match kind {
        EffectEventKind::Birth => ParticleTrigger::Birth,
        EffectEventKind::Collision => ParticleTrigger::Collision,
        EffectEventKind::Death => ParticleTrigger::Death,
        EffectEventKind::Completed => return,
    };
    for child in &emitter.sub_emitters {
        if child.trigger == trigger
            && rng.unit() < child.probability
            && frame.events.len() < MAX_EVENTS
        {
            frame.events.push(EffectEvent {
                entity,
                kind,
                position,
                velocity: velocity * child.inherit_velocity,
                child_effect: Some(child.effect),
            });
        }
    }
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
    gpu: bool,
    gpu_spawns: &mut Vec<GpuParticleSpawn>,
    global_particle_budget: &mut usize,
    frame: &mut EffectFrame,
) {
    let common = &input.effect.common;
    let local = common.simulation_space == SimulationSpace::Local;
    if let EffectKind::Weather(weather) = &input.effect.kind {
        for deposit in &mut instance.deposits {
            deposit.age += dt;
        }
        instance
            .deposits
            .retain(|deposit| deposit.age < weather.accumulation_lifetime);
    }
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
                        && instance.rng.unit() < burst.probability
                    {
                        spawn = spawn.saturating_add(burst.count as usize);
                    }
                }
            }
        }
        instance.began = true;
        let capacity = ((p.max_particles as f32 * quality.min(1.0)).min(100_000.0) as usize)
            .saturating_sub(instance.particles.len());
        if spawn.min(capacity) > *global_particle_budget {
            frame.particle_budget_exhausted = true;
        }
        for _ in 0..spawn.min(capacity).min(*global_particle_budget) {
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
            let velocity = direction * instance.rng.range(p.speed[0], p.speed[1])
                + Vec3::from(p.initial_velocity);
            let rotation3 = random_vector_range(p.start_rotation, &mut instance.rng);
            let angular_velocity3 =
                random_vector_range(p.angular_velocity_range, &mut instance.rng)
                    + Vec3::Z * p.angular_velocity;
            let scale = random_vector_range(p.start_scale, &mut instance.rng);
            let variant = weighted_mesh(&p.mesh_variants, &mut instance.rng);
            let particle = Particle {
                color_random: if p.appearance.color_parameter.is_some() {
                    instance.rng.unit()
                } else {
                    0.5
                },
                position: if local {
                    offset
                } else {
                    position + transform.transform_vector3(offset)
                },
                velocity: if local {
                    velocity
                        + transform.inverse().transform_vector3(emitter_velocity)
                            * p.inherit_velocity
                } else {
                    transform.transform_vector3(velocity) + emitter_velocity * p.inherit_velocity
                },
                age: 0.0,
                lifetime: instance.rng.range(p.lifetime[0], p.lifetime[1]).max(0.001),
                size: instance.rng.range(p.start_size[0], p.start_size[1]),
                rotation: rotation3.z,
                rotation3,
                angular_velocity3,
                scale,
                mesh: variant.map(|v| v.0).or(p.mesh),
                material: variant.and_then(|v| v.1),
                gpu_slot: if gpu {
                    instance.gpu_free_slots.pop().unwrap_or_else(|| {
                        let slot = instance.gpu_next_slot;
                        instance.gpu_next_slot = instance.gpu_next_slot.saturating_add(1);
                        slot
                    })
                } else {
                    0
                },
                stuck: false,
            };
            if let EffectKind::Weather(weather) = &input.effect.kind {
                let relative = if local {
                    particle.position
                } else {
                    transform.inverse().transform_point3(particle.position)
                };
                let radius = Vec3::from(p.dimensions).length().max(0.001) * 0.5;
                if weather.density_falloff > 0.0
                    && instance.rng.unit()
                        > (1.0 - relative.length() / radius)
                            .clamp(0.0, 1.0)
                            .powf(weather.density_falloff)
                {
                    continue;
                }
            }
            let world_position = if local {
                transform.transform_point3(particle.position)
            } else {
                particle.position
            };
            let world_velocity = if local {
                transform.transform_vector3(movement_velocity(&particle, p))
            } else {
                movement_velocity(&particle, p)
            };
            particle_events(
                frame,
                input.entity,
                EffectEventKind::Birth,
                world_position,
                world_velocity,
                p,
                &mut instance.rng,
                None,
            );
            if gpu {
                gpu_spawns.push(GpuParticleSpawn {
                    slot: particle.gpu_slot,
                    state: GpuParticleState::new(
                        particle.position,
                        particle.velocity,
                        0.0,
                        particle.lifetime,
                        particle.size,
                        particle.rotation,
                    ),
                });
            }
            *global_particle_budget = global_particle_budget.saturating_sub(1);
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
            if particle.stuck || gpu {
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
            let field_priority = inputs
                .iter()
                .filter_map(|field| {
                    if let EffectKind::ForceField(config) = &field.effect.kind {
                        (active_fields.contains(&field.entity)
                            && config.affect_particles
                            && config.layers & common.visibility_layers != 0
                            && field_weight(config, field.transform, world_position) > 0.0)
                            .then_some(field.effect.common.priority)
                    } else {
                        None
                    }
                })
                .max();
            for field in inputs {
                if let EffectKind::ForceField(f) = &field.effect.kind
                    && active_fields.contains(&field.entity)
                    && f.layers & common.visibility_layers != 0
                    && f.affect_particles
                    && Some(field.effect.common.priority) == field_priority
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
            force += Vec3::new(
                (particle.position.y * 1.7 + instance.time).cos()
                    - (particle.position.z * 1.3 - instance.time).sin(),
                (particle.position.z * 1.3 - instance.time).cos()
                    - (particle.position.x * 1.1 + instance.time).sin(),
                (particle.position.x * 1.1 + instance.time).cos()
                    - (particle.position.y * 1.7 + instance.time).sin(),
            ) * p.turbulence;
            let field_drag: f32 = inputs
                .iter()
                .filter(|field| active_fields.contains(&field.entity))
                .filter_map(|field| match &field.effect.kind {
                    EffectKind::ForceField(config) | EffectKind::Wind(config)
                        if config.affect_particles
                            && config.layers & common.visibility_layers != 0 =>
                    {
                        Some(
                            config.drag_influence
                                * field_weight(config, field.transform, world_position),
                        )
                    }
                    _ => None,
                })
                .sum();
            particle.velocity =
                (particle.velocity + force * step) * (-(p.drag + field_drag).max(0.0) * step).exp();
            particle.velocity = particle
                .velocity
                .clamp_length_max(p.velocity_limit.max(0.0));
            let old = particle.position;
            let next = old + movement_velocity(particle, p) * step;
            particle.position = next;
            particle.rotation3 += particle.angular_velocity3 * step;
            particle.rotation = particle.rotation3.z;
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
                    let contact = if p.collision_quality == ParticleCollisionQuality::Discrete
                        || c.quality == ParticleCollisionQuality::Discrete
                    {
                        discrete_contact(b, c.shape, half).map(|normal| (1.0, normal))
                    } else {
                        match c.shape {
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
                            VolumeShape::Cylinder => {
                                sweep_cylinder(a, b, half.x.max(half.z), half.y)
                            }
                        }
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
                        if let EffectKind::Weather(weather) = &input.effect.kind
                            && instance.deposits.len() < weather.max_accumulation as usize
                            && instance.rng.unit() < weather.accumulation
                        {
                            instance.deposits.push(WeatherDeposit {
                                position: hit,
                                normal,
                                age: 0.0,
                                size: particle.size.max(0.01),
                            });
                        }
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
                        let collision_velocity = if local {
                            transform.transform_vector3(movement_velocity(particle, p))
                        } else {
                            movement_velocity(particle, p)
                        };
                        match response {
                            CollisionResponse::Kill => particle.age = particle.lifetime,
                            CollisionResponse::Stick => {
                                particle.velocity = Vec3::ZERO;
                                particle.stuck = true;
                            }
                            CollisionResponse::Bounce => {
                                let v = bounce_velocity(
                                    collision_velocity,
                                    normal,
                                    p.bounce * c.bounce,
                                    1.0 - (1.0 - p.friction) * (1.0 - c.friction),
                                );
                                let reflected = if local {
                                    transform.inverse().transform_vector3(v)
                                } else {
                                    v
                                };
                                let progress = (particle.age / particle.lifetime).clamp(0.0, 1.0);
                                particle.velocity = reflected
                                    - Vec3::from(std::array::from_fn(|axis| {
                                        p.velocity_over_lifetime[axis].sample(progress)
                                    }));
                            }
                        }
                        particle_events(
                            frame,
                            input.entity,
                            EffectEventKind::Collision,
                            hit,
                            collision_velocity,
                            p,
                            &mut instance.rng,
                            c.impact_effect,
                        );
                        break;
                    }
                }
            }
        }
    }
    instance.particles.retain(|particle| {
        if particle.age >= particle.lifetime {
            *global_particle_budget = global_particle_budget.saturating_add(1).min(100_000);
            if gpu {
                instance.gpu_free_slots.push(particle.gpu_slot);
            }
            let world_position = if local {
                transform.transform_point3(particle.position)
            } else {
                particle.position
            };
            let world_velocity = if local {
                transform.transform_vector3(movement_velocity(particle, p))
            } else {
                movement_velocity(particle, p)
            };
            particle_events(
                frame,
                input.entity,
                EffectEventKind::Death,
                world_position,
                world_velocity,
                p,
                &mut instance.rng,
                None,
            );
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
    clippy::cast_precision_loss,
    reason = "tests check exact deterministic values and modify individual fixture controls"
)]
mod geometry_tests {
    use super::*;
    use crate::effects::{
        BeamRenderer, Burst, EffectParameter, FoliageScatter, ScreenFlash, TrailRenderer,
    };
    #[test]
    fn particle_buffers_are_recycled_between_entities_and_released_on_unload() {
        let first = emitter();
        let mut runtime = EffectRuntime::new();
        runtime.update(0.1, std::slice::from_ref(&first), Vec3::Z);
        let pointer = runtime.instances[&first.entity].particles.as_ptr();
        let capacity = runtime.instances[&first.entity].particles.capacity();
        runtime.remove(first.entity);
        assert_eq!(runtime.particle_pool.len(), 1);
        let second = emitter();
        runtime.ensure(&second);
        assert_eq!(
            runtime.instances[&second.entity].particles.capacity(),
            capacity
        );
        assert_eq!(
            runtime.instances[&second.entity].particles.as_ptr(),
            pointer
        );
        assert!(runtime.instances[&second.entity].particles.is_empty());
        runtime.update(0.1, std::slice::from_ref(&second), Vec3::Z);
        let mut fresh = EffectRuntime::new();
        fresh.update(0.1, std::slice::from_ref(&second), Vec3::Z);
        assert_eq!(
            runtime.frame().particles[0].position,
            fresh.frame().particles[0].position
        );
        assert_eq!(
            runtime.frame().particles[0].rotation3,
            fresh.frame().particles[0].rotation3
        );
        assert_eq!(
            runtime.frame().particles[0].size,
            fresh.frame().particles[0].size
        );
        runtime.update(0.0, &[], Vec3::Z);
        assert_eq!(runtime.particle_pool.len(), 1);
        runtime.clear();
        assert!(runtime.particle_pool.is_empty());
    }

    #[test]
    fn global_particle_admission_budget_preserves_existing_particles() {
        let make = || {
            let mut input = emitter();
            if let EffectKind::Particle(emitter) = &mut input.effect.kind {
                emitter.max_particles = 100_000;
                emitter.rate = 0.0;
                emitter.bursts = vec![Burst {
                    count: 60_000,
                    ..Default::default()
                }];
            }
            input
        };
        let first = make();
        let second = make();
        let mut runtime = EffectRuntime::new();
        runtime.update(0.0, &[first.clone(), second.clone()], Vec3::Z);
        assert_eq!(runtime.particle_count(), 100_000);
        assert!(runtime.frame().particle_budget_exhausted);
        assert_eq!(runtime.instances[&first.entity].particles.len(), 60_000);
        assert_eq!(runtime.instances[&second.entity].particles.len(), 40_000);
        let third = make();
        runtime.update(0.0, &[first.clone(), second.clone(), third], Vec3::Z);
        assert_eq!(runtime.particle_count(), 100_000);
        assert_eq!(runtime.instances[&first.entity].particles.len(), 60_000);
        assert_eq!(runtime.instances[&second.entity].particles.len(), 40_000);
    }

    #[test]
    fn per_pass_distance_metadata_resolves_attachments() {
        let mut input = emitter();
        let target = EntityId::new();
        input.effect.common.attachment_target = Some(target);
        input.effect.common.render_distance = 25.0;
        let targets = BTreeMap::from([(target, Mat4::from_translation(Vec3::X * 12.0))]);
        let mut runtime = EffectRuntime::new();
        let frame =
            runtime.update_with_targets(0.0, std::slice::from_ref(&input), Vec3::ZERO, &targets);
        assert_eq!(
            frame.effect_render_distances[&input.entity],
            (Vec3::X * 12.0, 0.0, 25.0)
        );
        runtime.update(0.0, &[], Vec3::ZERO);
        assert!(runtime.frame().effect_render_distances.is_empty());
    }

    #[test]
    fn weather_collision_spawns_splash_and_deposits_expire_without_events() {
        let splash = crate::AssetId::new();
        let mut weather = crate::effects::WeatherVolume::default();
        weather.accumulation = 1.0;
        weather.accumulation_lifetime = 0.2;
        weather.splash_effect = Some(splash);
        weather.emitter.shape = SpawnShape::Point;
        weather.emitter.rate = 10.0;
        weather.emitter.gravity = [0.0; 3];
        weather.emitter.speed = [10.0; 2];
        weather.emitter.collision = true;
        weather.emitter.generate_events = false;
        weather.emitter.collision_response = CollisionResponse::Kill;
        let input = EffectInput {
            entity: EntityId::new(),
            transform: Mat4::from_translation(Vec3::Y),
            effect: EffectComponent::new(EffectKind::Weather(weather)),
        };
        let collider = EffectInput {
            entity: EntityId::new(),
            transform: Mat4::IDENTITY,
            effect: EffectComponent::new(EffectKind::CollisionVolume(
                crate::effects::CollisionVolume::default(),
            )),
        };
        let mut runtime = EffectRuntime::new();
        let frame = runtime.update(0.11, &[input.clone(), collider.clone()], Vec3::Z * 10.0);
        assert!(
            frame
                .geometry
                .iter()
                .any(|geometry| geometry.entity == input.entity
                    && !geometry.mesh.positions.is_empty())
        );
        assert!(
            frame
                .events
                .iter()
                .any(|event| event.child_effect == Some(splash))
        );
        assert_eq!(runtime.instances[&input.entity].deposits.len(), 1);
        let mut stopped = input.clone();
        if let EffectKind::Weather(weather) = &mut stopped.effect.kind {
            weather.emitter.rate = 0.0;
        }
        runtime.update(0.3, &[stopped, collider], Vec3::Z * 10.0);
        assert!(runtime.instances[&input.entity].deposits.is_empty());
    }

    #[test]
    fn local_trails_move_history_with_parent_without_collapsing_head() {
        let mut input = EffectInput {
            entity: EntityId::new(),
            transform: Mat4::IDENTITY,
            effect: EffectComponent::new(EffectKind::Trail(TrailRenderer::default())),
        };
        input.effect.common.simulation_space = SimulationSpace::Local;
        let mut runtime = EffectRuntime::new();
        let mut parents = BTreeMap::new();
        parents.insert(input.entity, Mat4::IDENTITY);
        runtime.set_parent_transforms(&parents);
        runtime.update(0.1, std::slice::from_ref(&input), Vec3::Z * 10.0);
        input.transform = Mat4::from_translation(Vec3::X);
        runtime.update(0.1, std::slice::from_ref(&input), Vec3::Z * 10.0);
        parents.insert(input.entity, Mat4::from_translation(Vec3::Y * 3.0));
        runtime.set_parent_transforms(&parents);
        input.transform = Mat4::from_translation(Vec3::Y * 3.0 + Vec3::X * 2.0);
        let frame = runtime.update(0.1, &[input], Vec3::Z * 10.0);
        assert_eq!(frame.geometry.len(), 1);
        let positions = &frame.geometry[0].mesh.positions;
        assert_eq!(positions.len(), 6);
        for pair in positions.as_chunks::<2>().0 {
            assert!(((Vec3::from(pair[0]) + Vec3::from(pair[1])) * 0.5).y > 2.99);
        }
    }

    #[test]
    fn smoothing_large_paths_preserves_last_control_point() {
        let points: Vec<_> = (0..4096).map(|index| Vec3::X * index as f32).collect();
        let smoothed = smooth_path(&points, 2);
        assert_eq!(smoothed.first(), points.first());
        assert_eq!(smoothed.last(), points.last());
        assert!(smoothed.len() <= 4095);
    }

    #[test]
    fn tracer_endpoints_tail_alpha_and_velocity_stretch_are_consumed() {
        let mut tracer = crate::effects::ProjectileTracer::default();
        tracer.start_position = [1.0, 0.0, 0.0];
        tracer.end_position = Some([5.0, 0.0, 0.0]);
        tracer.velocity_stretch = 0.1;
        let mut input = EffectInput {
            entity: EntityId::new(),
            transform: Mat4::IDENTITY,
            effect: EffectComponent::new(EffectKind::ProjectileTracer(tracer)),
        };
        input.effect.common.duration = 2.0;
        let mut runtime = EffectRuntime::new();
        let frame = runtime.update(1.0, &[input], Vec3::Z * 10.0);
        let geometry = &frame.geometry[0];
        assert!(geometry.colors[0][3] == 0.0 && geometry.colors[16][3] > 0.0);
        let head = (Vec3::from(geometry.mesh.positions[16])
            + Vec3::from(geometry.mesh.positions[17]))
            * 0.5;
        let tail = Vec3::from(geometry.mesh.positions[0]);
        assert!((head.x - 3.0).abs() < 1e-5);
        assert!((head.distance(tail) - 3.0).abs() < 1e-5);
    }

    #[test]
    fn overlapping_particle_fields_use_highest_priority() {
        let input = emitter();
        let mut field = ForceField::default();
        field.global = true;
        field.strength = 10.0;
        field.direction = [1.0, 0.0, 0.0];
        let mut low = EffectInput {
            entity: EntityId::new(),
            transform: Mat4::IDENTITY,
            effect: EffectComponent::new(EffectKind::ForceField(field.clone())),
        };
        low.effect.common.priority = 0;
        field.direction = [0.0, 1.0, 0.0];
        let mut high = EffectInput {
            entity: EntityId::new(),
            transform: Mat4::IDENTITY,
            effect: EffectComponent::new(EffectKind::ForceField(field)),
        };
        high.effect.common.priority = 2;
        let mut runtime = EffectRuntime::new();
        let frame = runtime.update(0.1, &[input, low, high], Vec3::Z);
        assert!(
            frame
                .particles
                .iter()
                .all(|particle| particle.velocity.x.abs() < 1e-6 && particle.velocity.y > 0.9)
        );
    }

    #[test]
    fn material_wind_gusts_respect_recipient_mask_and_layers() {
        let mut field = ForceField::default();
        field.global = true;
        field.direction = [1.0, 0.0, 0.0];
        field.strength = 0.0;
        field.turbulence = 0.0;
        field.gust_strength = 4.0;
        field.gust_frequency = 1.0;
        field.layers = 2;
        let mut zone = super::super::RenderWindZone {
            entity: EntityId::new(),
            transform: Mat4::IDENTITY,
            field,
        };
        assert_eq!(
            sample_render_wind(&[zone.clone()], Vec3::ZERO, 0.25, 1),
            Vec3::ZERO
        );
        assert!((sample_render_wind(&[zone.clone()], Vec3::ZERO, 0.25, 2).x - 4.0).abs() < 1e-5);
        zone.field.affect_materials = false;
        assert_eq!(sample_render_wind(&[zone], Vec3::ZERO, 0.25, 2), Vec3::ZERO);
    }

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

    #[test]
    fn dual_gradients_keep_birth_random_stable_and_vector_curves_drive_motion() {
        let mut input = emitter();
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.rate = 40.0;
            p.appearance.opacity = crate::effects::Curve::constant(1.0);
            p.appearance.color_parameter =
                Some(crate::effects::GradientParameter::BetweenGradients {
                    min: crate::effects::Gradient::constant([1., 0., 0., 1.]),
                    max: crate::effects::Gradient::constant([0., 0., 1., 1.]),
                });
            p.velocity_over_lifetime = crate::effects::VectorCurve([
                crate::effects::Curve::constant(2.0),
                crate::effects::Curve::constant(0.0),
                crate::effects::Curve::constant(0.0),
            ]);
        }
        input.effect.validate().unwrap();
        let mut runtime = EffectRuntime::new();
        let first = runtime
            .update(0.1, std::slice::from_ref(&input), Vec3::Z * 10.)
            .particles
            .clone();
        assert_eq!(first.len(), 4);
        assert!(first.iter().all(|particle| particle.color[0] > 0.
            && particle.color[2] > 0.
            && (particle.color[0] + particle.color[2] - 1.).abs() < 0.0001));
        assert!(
            first
                .windows(2)
                .any(|pair| (pair[0].color[0] - pair[1].color[0]).abs() > 0.0001)
        );
        let mut repeated = EffectRuntime::new();
        let again = repeated.update(0.1, std::slice::from_ref(&input), Vec3::Z * 10.);
        assert_eq!(
            first
                .iter()
                .map(|particle| particle.color)
                .collect::<Vec<_>>(),
            again
                .particles
                .iter()
                .map(|particle| particle.color)
                .collect::<Vec<_>>()
        );
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.rate = 0.;
        }
        let later = runtime.update(0.1, &[input], Vec3::Z * 10.);
        assert_eq!(first.len(), later.particles.len());
        for (before, after) in first.iter().zip(&later.particles) {
            assert_eq!(before.color, after.color);
            assert!((after.position.x - before.position.x - 0.2).abs() < 0.0001);
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
        assert!(runtime.update(0.4, &[input], Vec3::Z).geometry.is_empty());
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
    #[test]
    fn local_space_inherits_world_emitter_velocity_and_reports_world_events() {
        let mut input = emitter();
        input.effect.common.simulation_space = SimulationSpace::Local;
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.inherit_velocity = 1.0;
        }
        let mut runtime = EffectRuntime::new();
        runtime.update(0.1, std::slice::from_ref(&input), Vec3::Z);
        input.transform = Mat4::from_translation(Vec3::X);
        let frame = runtime.update(0.1, &[input], Vec3::Z);
        assert!(
            frame
                .particles
                .iter()
                .any(|p| (p.velocity.x - 10.0).abs() < 0.0001)
        );
        assert!(
            frame
                .events
                .iter()
                .any(|e| e.kind == EffectEventKind::Birth && (e.position.x - 1.0).abs() < 0.0001)
        );
    }

    #[test]
    fn zero_subemitter_probability_blocks_birth_and_death_children() {
        let mut input = emitter();
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.lifetime = [0.01; 2];
            p.sub_emitter = Some(crate::AssetId::new());
            p.sub_emitter_probability = 0.0;
        }
        let mut runtime = EffectRuntime::new();
        let frame = runtime.update(0.1, &[input], Vec3::Z);
        assert!(
            frame
                .events
                .iter()
                .any(|e| e.kind == EffectEventKind::Birth)
        );
        assert!(
            frame
                .events
                .iter()
                .any(|e| e.kind == EffectEventKind::Death)
        );
        assert!(frame.events.iter().all(|e| e.child_effect.is_none()));
    }
    #[test]
    fn gpu_commands_queue_fixed_steps_until_renderer_acknowledges() {
        let mut input = emitter();
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.generate_events = false;
            p.sorting = super::super::ParticleSortMode::None;
            p.simulation_mode = ParticleSimulationMode::Gpu;
        }
        let mut runtime = EffectRuntime::new();
        runtime.set_gpu_particle_context(Some(7));
        let frame = runtime.update(0.1, std::slice::from_ref(&input), Vec3::Z);
        assert!(frame.particles.is_empty());
        assert_eq!(frame.gpu_emitters.len(), 1);
        assert_eq!(frame.gpu_emitters[0].spawns.len(), 1);
        let first_sequence = frame.gpu_emitters[0].sequence;
        let epoch = frame.gpu_emitters[0].epoch;
        let frame = runtime.update(0.1, std::slice::from_ref(&input), Vec3::Z);
        assert_eq!(frame.gpu_emitters.len(), 2);
        assert_ne!(
            frame.gpu_emitters[0].spawns[0].slot,
            frame.gpu_emitters[1].spawns[0].slot
        );
        runtime.acknowledge_gpu_particles(&[(input.entity, first_sequence)]);
        assert_eq!(
            runtime
                .update(0.0, std::slice::from_ref(&input), Vec3::Z)
                .gpu_emitters
                .len(),
            1
        );
        runtime.set_gpu_particle_context(Some(8));
        let frame = runtime.update(0.1, &[input], Vec3::Z);
        assert_ne!(frame.gpu_emitters[0].epoch, epoch);
        assert_eq!(frame.gpu_emitters[0].spawns.len(), 1);
    }

    #[test]
    fn discrete_collision_volume_selects_complete_cpu_fallback() {
        let mut input = emitter();
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.generate_events = false;
            p.sorting = super::super::ParticleSortMode::None;
            p.simulation_mode = ParticleSimulationMode::Gpu;
            p.collision = true;
        }
        let volume = EffectInput {
            entity: EntityId::new(),
            transform: Mat4::IDENTITY,
            effect: EffectComponent::new(EffectKind::CollisionVolume(
                super::super::CollisionVolume {
                    quality: ParticleCollisionQuality::Discrete,
                    ..super::super::CollisionVolume::default()
                },
            )),
        };
        let mut runtime = EffectRuntime::new();
        runtime.set_gpu_particle_context(Some(7));
        let frame = runtime.update(0.1, &[input, volume], Vec3::Z);
        assert!(frame.gpu_emitters.is_empty());
        assert_eq!(frame.particle_backends[0].active, ParticleBackend::Cpu);
        assert!(
            frame.particle_backends[0]
                .reason
                .as_deref()
                .unwrap()
                .contains("discrete")
        );
        assert!(!frame.particles.is_empty());
    }

    #[test]
    fn paused_gpu_emitters_publish_live_transform_without_advancing_particles() {
        let mut input = emitter();
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.generate_events = false;
            p.sorting = super::super::ParticleSortMode::None;
            p.simulation_mode = ParticleSimulationMode::Gpu;
        }
        input.effect.common.simulation_space = SimulationSpace::Local;
        let mut runtime = EffectRuntime::new();
        runtime.set_gpu_particle_context(Some(7));
        let first = runtime
            .update(0.1, std::slice::from_ref(&input), Vec3::Z)
            .gpu_emitters[0]
            .clone();
        runtime.acknowledge_gpu_particles(&[(input.entity, first.sequence)]);
        runtime.control(input.entity, PlaybackCommand::Pause);
        input.transform = Mat4::from_translation(Vec3::X * 5.0);
        let frame = runtime.update(0.1, &[input.clone()], Vec3::Z);
        assert_eq!(frame.gpu_emitters.len(), 1);
        assert_eq!(frame.gpu_emitters[0].transform, input.transform);
        assert!(frame.gpu_emitters[0].sequence > first.sequence);
        assert!(frame.gpu_emitters[0].delta.abs() < f32::EPSILON);
        assert!(frame.gpu_emitters[0].spawns.is_empty());
    }

    #[test]
    fn stale_gpu_acknowledgements_cannot_discard_restarted_births() {
        let mut input = emitter();
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.generate_events = false;
            p.sorting = super::super::ParticleSortMode::None;
            p.simulation_mode = ParticleSimulationMode::Gpu;
        }
        let mut runtime = EffectRuntime::new();
        runtime.set_gpu_particle_context(Some(7));
        let first = runtime
            .update(0.1, std::slice::from_ref(&input), Vec3::Z)
            .gpu_emitters[0]
            .clone();
        runtime.control(input.entity, PlaybackCommand::Restart);
        let restarted = runtime
            .update(0.1, std::slice::from_ref(&input), Vec3::Z)
            .gpu_emitters[0]
            .clone();
        assert_ne!(first.epoch, restarted.epoch);
        assert!(first.sequence < restarted.sequence);
        runtime.acknowledge_gpu_particles(&[(input.entity, first.sequence)]);
        let pending = runtime.update(0.0, std::slice::from_ref(&input), Vec3::Z);
        assert_eq!(pending.gpu_emitters.len(), 1);
        assert_eq!(pending.gpu_emitters[0].sequence, restarted.sequence);
        assert!(!pending.gpu_emitters[0].spawns.is_empty());
        input.effect.common.random_seed += 1;
        let reseeded = runtime
            .update(0.1, std::slice::from_ref(&input), Vec3::Z)
            .gpu_emitters[0]
            .clone();
        assert!(reseeded.sequence > restarted.sequence);
        runtime.acknowledge_gpu_particles(&[(input.entity, restarted.sequence)]);
        assert_eq!(
            runtime.update(0.0, &[input], Vec3::Z).gpu_emitters[0].sequence,
            reseeded.sequence
        );
    }

    #[test]
    fn gpu_fallback_is_explicit_and_render_masks_keep_simulation_running() {
        let mut input = emitter();
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.simulation_mode = ParticleSimulationMode::Gpu;
        }
        let mut runtime = EffectRuntime::new();
        runtime.set_gpu_particle_support(true);
        runtime.set_visibility_layers(0);
        let frame = runtime.update(0.1, std::slice::from_ref(&input), Vec3::Z);
        // Extraction includes geometry for probe views; each render pass applies its mask.
        assert_eq!(frame.particles.len(), 1);
        assert_eq!(frame.effect_layers[&input.entity], u32::MAX);
        assert_eq!(frame.particle_backends[0].active, ParticleBackend::Cpu);
        assert!(
            frame.particle_backends[0]
                .reason
                .as_ref()
                .unwrap()
                .contains("events")
        );
        assert_eq!(runtime.particle_count(), 1);
        runtime.set_visibility_layers(u32::MAX);
        assert_eq!(runtime.update(0.1, &[input], Vec3::Z).particles.len(), 2);
    }

    #[test]
    fn weighted_meshes_3d_variation_and_lifetime_motion_are_consumed() {
        let a = crate::AssetId::new();
        let b = crate::AssetId::new();
        let mut input = emitter();
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.rate = 0.0;
            p.bursts = vec![Burst {
                count: 1,
                ..Default::default()
            }];
            p.lifetime = [1.0; 2];
            p.mesh_variants = vec![
                super::super::WeightedParticleMesh {
                    mesh: a,
                    weight: 0.0,
                    material: None,
                },
                super::super::WeightedParticleMesh {
                    mesh: b,
                    weight: 1.0,
                    material: None,
                },
            ];
            p.start_scale = [[2.0, 1.0, 3.0]; 2];
            p.start_rotation = [[std::f32::consts::FRAC_PI_2, 0.0, 0.0]; 2];
            p.angular_velocity_range = [[0.0, 1.0, 0.0]; 2];
            p.velocity_over_lifetime[0] = super::super::Curve::constant(2.0);
            p.rotation_over_lifetime = super::super::Curve::linear(0.0, std::f32::consts::PI);
        }
        let mut runtime = EffectRuntime::new();
        let frame = runtime.update(0.1, &[input], Vec3::Z);
        let p = &frame.particles[0];
        assert_eq!(p.mesh, Some(b));
        assert_eq!(p.scale, Vec3::new(2.0, 1.0, 3.0));
        assert!((p.position.x - 0.2).abs() < 0.0001);
        assert!((p.rotation3.y - 0.1).abs() < 0.0001);
        assert!((p.rotation3.z - std::f32::consts::PI * 0.1).abs() < 0.0001);
    }

    #[test]
    fn explicit_subemitter_triggers_inherit_world_velocity() {
        let birth = crate::AssetId::new();
        let death = crate::AssetId::new();
        let mut input = emitter();
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.rate = 0.0;
            p.bursts = vec![Burst {
                count: 1,
                ..Default::default()
            }];
            p.lifetime = [0.05; 2];
            p.initial_velocity = [4.0, 0.0, 0.0];
            p.sub_emitters = vec![
                super::super::ParticleSubEmitter {
                    trigger: ParticleTrigger::Birth,
                    effect: birth,
                    probability: 1.0,
                    inherit_velocity: 0.5,
                },
                super::super::ParticleSubEmitter {
                    trigger: ParticleTrigger::Death,
                    effect: death,
                    probability: 1.0,
                    inherit_velocity: 1.0,
                },
            ];
        }
        let mut runtime = EffectRuntime::new();
        let frame = runtime.update(0.1, &[input], Vec3::Z);
        let b = frame
            .events
            .iter()
            .find(|event| event.child_effect == Some(birth))
            .unwrap();
        assert_eq!(b.kind, EffectEventKind::Birth);
        assert_eq!(b.velocity, Vec3::X * 2.0);
        let d = frame
            .events
            .iter()
            .find(|event| event.child_effect == Some(death))
            .unwrap();
        assert_eq!(d.kind, EffectEventKind::Death);
        assert_eq!(d.velocity, Vec3::X * 4.0);
    }

    #[test]
    fn offscreen_pause_and_clear_are_distinct_from_simulate() {
        let mut input = emitter();
        input.effect.common.render_distance = 1.0;
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.offscreen_behavior = ParticleOffscreenBehavior::Pause;
        }
        let mut runtime = EffectRuntime::new();
        runtime.update(0.1, std::slice::from_ref(&input), Vec3::Z * 10.0);
        assert_eq!(runtime.particle_count(), 0);
        runtime.update(0.1, std::slice::from_ref(&input), Vec3::ZERO);
        assert_eq!(runtime.particle_count(), 1);
        if let EffectKind::Particle(p) = &mut input.effect.kind {
            p.offscreen_behavior = ParticleOffscreenBehavior::Clear;
        }
        runtime.update(0.1, &[input], Vec3::Z * 10.0);
        assert_eq!(runtime.particle_count(), 0);
    }
}
