//! Renderer-neutral commands for persistent GPU particle simulation.
//!
//! The world schedules seeded births and lifetime occupancy. A capable renderer
//! uploads only births, advances persistent storage with compute, and draws that
//! storage directly. Feature combinations requiring CPU events retain CPU simulation.
use super::{BlendMode, CollisionResponse, EffectAppearance, SimulationSpace, VolumeShape};
use crate::EntityId;
use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};

/// Requested simulation backend. Unsupported configurations use the full CPU path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParticleSimulationMode {
    #[default]
    Cpu,
    Auto,
    Gpu,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticleBackend {
    Cpu,
    Gpu,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleBackendStatus {
    pub entity: EntityId,
    pub requested: ParticleSimulationMode,
    pub active: ParticleBackend,
    pub reason: Option<String>,
}
/// Exactly four 16-byte vectors in the compute/storage ABI.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuParticleState {
    /// Position xyz and age seconds.
    pub position_age: [f32; 4],
    /// Velocity xyz and lifetime seconds; lifetime zero denotes a free slot.
    pub velocity_lifetime: [f32; 4],
    /// Initial size, rotation radians, collision-stuck flag, and one reserved value.
    pub size_rotation: [f32; 4],
    /// Reserved initial-color payload. Appearance curves are evaluated on GPU.
    pub color: [f32; 4],
}
impl GpuParticleState {
    pub fn new(
        position: Vec3,
        velocity: Vec3,
        age: f32,
        lifetime: f32,
        size: f32,
        rotation: f32,
    ) -> Self {
        Self {
            position_age: position.extend(age).to_array(),
            velocity_lifetime: velocity.extend(lifetime).to_array(),
            size_rotation: [size, rotation, 0.0, 0.0],
            color: [1.0; 4],
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuParticleSpawn {
    pub slot: u32,
    pub state: GpuParticleState,
}
#[derive(Clone, Debug, PartialEq)]
pub struct GpuParticleCollider {
    pub inverse_transform: Mat4,
    pub transform: Mat4,
    pub dimensions: [f32; 3],
    pub shape: VolumeShape,
    pub bounce: f32,
    pub friction: f32,
    pub response: CollisionResponse,
}
#[derive(Clone, Debug)]
pub struct GpuParticleEmitterFrame {
    pub entity: EntityId,
    /// Changes on reset, restart or backend transition; clears persistent storage.
    pub epoch: u64,
    /// Prevents a scene rendered by multiple viewports from advancing twice.
    pub sequence: u64,
    pub capacity: u32,
    /// Scheduler lifetime occupancy; supported GPU modules preserve these lifetimes.
    pub live_count: u32,
    pub visible: bool,
    pub visibility_layers: u32,
    /// World center, bounding radius, and per-camera maximum draw distance.
    pub render_bounds: (Vec3, f32, f32),
    pub delta: f32,
    pub transform: Mat4,
    pub simulation_space: SimulationSpace,
    pub gravity: [f32; 3],
    pub force: [f32; 3],
    pub drag: f32,
    pub velocity_limit: f32,
    pub angular_velocity: f32,
    pub stretch: f32,
    pub appearance: EffectAppearance,
    pub flipbook: [u32; 2],
    pub flipbook_fps: f32,
    pub spawns: Vec<GpuParticleSpawn>,
    pub colliders: Vec<GpuParticleCollider>,
}
impl GpuParticleEmitterFrame {
    pub const MAX_PARTICLES: u32 = 100_000;
    pub const MAX_COLLIDERS: usize = 16;
    pub const MAX_CURVE_KEYS: usize = 16;
    pub const MAX_TOTAL_PARTICLES: u32 = 1_000_000;
    pub fn blend(&self) -> BlendMode {
        self.appearance.blend
    }
    fn validate_render_metadata(&self) -> Result<(), String> {
        if self.appearance.color_parameter.is_some() {
            return Err("gradient parameters require CPU particle evaluation".into());
        }
        if self.live_count > self.capacity {
            return Err("GPU particle live count exceeds capacity".into());
        }
        if !self.render_bounds.0.is_finite()
            || !self.render_bounds.1.is_finite()
            || self.render_bounds.1 < 0.0
            || !self.render_bounds.2.is_finite()
            || self.render_bounds.2 < 0.0
        {
            return Err("GPU particle render bounds must be finite and nonnegative".into());
        }

        Ok(())
    }
    /// Validates commands before any GPU upload or dispatch.
    ///
    /// # Errors
    /// Returns a diagnostic for invalid dimensions, indices, values or unsupported GPU modules.
    pub fn validate(&self) -> Result<(), String> {
        self.validate_render_metadata()?;
        if self.capacity == 0 || self.capacity > Self::MAX_PARTICLES {
            return Err("GPU particle capacity must be 1..100000".into());
        }
        if !self.delta.is_finite()
            || self.delta < 0.0
            || !self.transform.is_finite()
            || !self.transform.inverse().is_finite()
            || self.transform.determinant().abs() < 1e-12
        {
            return Err("GPU particle step and transform must be finite and invertible".into());
        }
        if !self
            .gravity
            .iter()
            .chain(&self.force)
            .chain([
                &self.drag,
                &self.velocity_limit,
                &self.angular_velocity,
                &self.stretch,
            ])
            .all(|v| v.is_finite())
        {
            return Err("GPU particle forces and material inputs must be finite".into());
        }
        if self.colliders.len() > Self::MAX_COLLIDERS {
            return Err("GPU particle collision supports at most 16 explicit volumes".into());
        }
        if self.appearance.color.keys.len() > Self::MAX_CURVE_KEYS
            || self.appearance.opacity.keys.len() > Self::MAX_CURVE_KEYS
            || self.appearance.size.keys.len() > Self::MAX_CURVE_KEYS
        {
            return Err("GPU appearance supports at most 16 keys per curve or gradient".into());
        }
        if self.appearance.texture.is_some()
            || self.appearance.material.is_some()
            || self.flipbook != [1, 1]
        {
            return Err(
                "textured and material-backed particles require CPU billboard batching".into(),
            );
        }
        if !self.appearance.emission.is_finite()
            || self.appearance.color.keys.iter().any(|key| {
                !key.time.is_finite()
                    || !(0.0..=1.0).contains(&key.time)
                    || !key.color.iter().all(|value| value.is_finite())
            })
            || [&self.appearance.size, &self.appearance.opacity]
                .into_iter()
                .any(|curve| {
                    curve.keys.iter().any(|key| {
                        !key.time.is_finite()
                            || !(0.0..=1.0).contains(&key.time)
                            || !key.value.is_finite()
                    })
                })
        {
            return Err(
                "GPU appearance requires finite emission, colors, and normalized curve times"
                    .into(),
            );
        }
        if self.spawns.iter().any(|s| {
            s.slot >= self.capacity
                || s.state.velocity_lifetime[3] <= 0.0
                || s.state.position_age[3] < 0.0
                || s.state.size_rotation[0] < 0.0
                || !s
                    .state
                    .position_age
                    .iter()
                    .chain(&s.state.velocity_lifetime)
                    .chain(&s.state.size_rotation)
                    .chain(&s.state.color)
                    .all(|v| v.is_finite())
        }) {
            return Err("GPU birth slot or particle state is invalid".into());
        }
        if self.colliders.iter().any(|c| {
            c.response == CollisionResponse::Kill
                || !c.transform.is_finite()
                || !c.inverse_transform.is_finite()
                || !c.dimensions.iter().all(|v| v.is_finite() && *v > 0.0)
                || !c.bounce.is_finite()
                || !(0.0..=1.0).contains(&c.bounce)
                || !c.friction.is_finite()
                || !(0.0..=1.0).contains(&c.friction)
        }) {
            return Err(
                "GPU collision requires finite bounce/stick volumes; kill/event responses use CPU"
                    .into(),
            );
        }
        Ok(())
    }
}
