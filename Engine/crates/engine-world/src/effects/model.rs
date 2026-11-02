//! Versioned authored effect configuration. Runtime state is never serialized.
use crate::{AssetId, EntityId};
use bevy_ecs::prelude::Component;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimulationSpace {
    Local,
    #[default]
    World,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlendMode {
    #[default]
    Alpha,
    Additive,
    Multiply,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityLevel {
    Low,
    #[default]
    Medium,
    High,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurveKey {
    pub time: f32,
    pub value: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub keys: Vec<CurveKey>,
}
impl Default for Curve {
    fn default() -> Self {
        Self::constant(1.0)
    }
}
impl Curve {
    pub fn constant(value: f32) -> Self {
        Self {
            keys: vec![CurveKey { time: 0.0, value }, CurveKey { time: 1.0, value }],
        }
    }
    pub fn linear(start: f32, end: f32) -> Self {
        Self {
            keys: vec![
                CurveKey {
                    time: 0.0,
                    value: start,
                },
                CurveKey {
                    time: 1.0,
                    value: end,
                },
            ],
        }
    }
    pub fn sample(&self, t: f32) -> f32 {
        let mut before: Option<&CurveKey> = None;
        let mut after: Option<&CurveKey> = None;
        for k in &self.keys {
            if k.time <= t && before.is_none_or(|v| k.time > v.time) {
                before = Some(k);
            }
            if k.time >= t && after.is_none_or(|v| k.time < v.time) {
                after = Some(k);
            }
        }
        match (before, after) {
            (Some(a), Some(b)) if b.time > a.time => {
                a.value + (b.value - a.value) * ((t - a.time) / (b.time - a.time))
            }
            (Some(a), _) => a.value,
            (_, Some(b)) => b.value,
            _ => 1.0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientKey {
    pub time: f32,
    pub color: [f32; 4],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    pub keys: Vec<GradientKey>,
}
impl Default for Gradient {
    fn default() -> Self {
        Self::constant([1.0; 4])
    }
}
impl Gradient {
    pub fn constant(color: [f32; 4]) -> Self {
        Self {
            keys: vec![GradientKey { time: 0.0, color }],
        }
    }
    pub fn sample(&self, t: f32) -> [f32; 4] {
        let mut before: Option<&GradientKey> = None;
        let mut after: Option<&GradientKey> = None;
        for k in &self.keys {
            if k.time <= t && before.is_none_or(|v| k.time > v.time) {
                before = Some(k);
            }
            if k.time >= t && after.is_none_or(|v| k.time < v.time) {
                after = Some(k);
            }
        }
        match (before, after) {
            (Some(a), Some(b)) if b.time > a.time => {
                let f = (t - a.time) / (b.time - a.time);
                std::array::from_fn(|i| a.color[i] + (b.color[i] - a.color[i]) * f)
            }
            (Some(a), _) => a.color,
            (_, Some(b)) => b.color,
            _ => [1.0; 4],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ScalarParameter {
    Constant(f32),
    Range { min: f32, max: f32 },
    Curve(Curve),
    BetweenCurves { min: Curve, max: Curve },
}
impl ScalarParameter {
    pub fn sample(&self, t: f32, random: f32) -> f32 {
        match self {
            Self::Constant(v) => *v,
            Self::Range { min, max } => min + (max - min) * random,
            Self::Curve(c) => c.sample(t),
            Self::BetweenCurves { min, max } => {
                min.sample(t) + (max.sample(t) - min.sample(t)) * random
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
// These independent authoring toggles mirror scene/editor lifecycle settings.
#[allow(clippy::struct_excessive_bools)]
pub struct EffectCommon {
    pub parameters: std::collections::BTreeMap<String, EffectParameter>,
    pub bounds: Option<EffectBounds>,
    pub enabled: bool,
    pub visible: bool,
    pub auto_play: bool,
    pub looping: bool,
    pub duration: f32,
    pub delay: f32,
    pub time_scale: f32,
    pub simulation_space: SimulationSpace,
    pub attachment_target: Option<EntityId>,
    pub visibility_layers: u32,
    pub render_distance: f32,
    pub quality: QualityLevel,
    pub random_seed: u64,
    pub priority: i32,
}
impl Default for EffectCommon {
    fn default() -> Self {
        Self {
            parameters: std::collections::BTreeMap::default(),
            bounds: None,
            enabled: true,
            visible: true,
            auto_play: true,
            looping: true,
            duration: 5.0,
            delay: 0.0,
            time_scale: 1.0,
            simulation_space: SimulationSpace::World,
            attachment_target: None,
            visibility_layers: u32::MAX,
            render_distance: 1000.0,
            quality: QualityLevel::Medium,
            random_seed: 1,
            priority: 0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectAppearance {
    pub color: Gradient,
    pub opacity: Curve,
    pub size: Curve,
    pub material: Option<AssetId>,
    pub texture: Option<AssetId>,
    pub emission: f32,
    pub blend: BlendMode,
    pub uv_scale: [f32; 2],
    pub uv_scroll: [f32; 2],
}
impl Default for EffectAppearance {
    fn default() -> Self {
        Self {
            color: Gradient::default(),
            opacity: Curve::linear(1.0, 0.0),
            size: Curve::constant(1.0),
            material: None,
            texture: None,
            emission: 1.0,
            blend: BlendMode::Alpha,
            uv_scale: [1.0; 2],
            uv_scroll: [0.0; 2],
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpawnShape {
    #[default]
    Point,
    Sphere,
    Hemisphere,
    Box,
    Cone,
    Circle,
    Cylinder,
    MeshSurface,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Burst {
    pub time: f32,
    pub count: u32,
    pub probability: f32,
    pub cycles: u32,
    pub interval: f32,
}
impl Default for Burst {
    fn default() -> Self {
        Self {
            time: 0.0,
            count: 20,
            probability: 1.0,
            cycles: 1,
            interval: 1.0,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollisionResponse {
    #[default]
    Bounce,
    Stick,
    Kill,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParticleEmitter {
    pub prewarm: bool,
    pub prewarm_time: f32,
    pub max_particles: u32,
    pub rate: f32,
    pub rate_over_distance: f32,
    pub bursts: Vec<Burst>,
    pub shape: SpawnShape,
    pub dimensions: [f32; 3],
    pub surface_only: bool,
    pub lifetime: [f32; 2],
    pub speed: [f32; 2],
    pub direction: [f32; 3],
    pub start_size: [f32; 2],
    pub gravity: [f32; 3],
    pub force: [f32; 3],
    pub drag: f32,
    pub wind_influence: f32,
    pub noise: f32,
    pub angular_velocity: f32,
    pub inherit_velocity: f32,
    pub velocity_limit: f32,
    pub appearance: EffectAppearance,
    pub mesh: Option<AssetId>,
    /// Local-space triangles supplied by a loaded mesh for surface emission.
    pub mesh_spawn_triangles: Vec<[[f32; 3]; 3]>,
    pub collision: bool,
    pub collision_layers: u32,
    pub bounce: f32,
    pub friction: f32,
    pub collision_response: CollisionResponse,
    pub sub_emitter: Option<AssetId>,
    pub sub_emitter_probability: f32,
    pub max_generations: u8,
    pub flipbook: [u32; 2],
    pub flipbook_fps: f32,
    /// Velocity stretch exposure in seconds; zero uses square billboards.
    pub stretch: f32,
}
impl Default for ParticleEmitter {
    fn default() -> Self {
        Self {
            prewarm: false,
            prewarm_time: 5.0,
            max_particles: 1000,
            rate: 30.0,
            rate_over_distance: 0.0,
            bursts: vec![],
            shape: SpawnShape::Cone,
            dimensions: [1.0; 3],
            surface_only: false,
            lifetime: [1.0, 2.0],
            speed: [1.0, 3.0],
            direction: [0.0, 1.0, 0.0],
            start_size: [0.1, 0.25],
            gravity: [0.0, -1.0, 0.0],
            force: [0.0; 3],
            drag: 0.1,
            wind_influence: 1.0,
            noise: 0.0,
            angular_velocity: 0.0,
            inherit_velocity: 0.0,
            velocity_limit: 1000.0,
            appearance: EffectAppearance::default(),
            mesh: None,
            mesh_spawn_triangles: Vec::new(),
            collision: false,
            collision_layers: u32::MAX,
            bounce: 0.5,
            friction: 0.1,
            collision_response: CollisionResponse::Bounce,
            sub_emitter: None,
            sub_emitter_probability: 1.0,
            max_generations: 3,
            flipbook: [1, 1],
            flipbook_fps: 0.0,
            stretch: 0.0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrailRenderer {
    pub emitting: bool,
    pub lifetime: f32,
    pub width: f32,
    pub min_vertex_distance: f32,
    pub max_points: u32,
    pub teleport_distance: f32,
    pub camera_facing: bool,
    pub appearance: EffectAppearance,
}
impl Default for TrailRenderer {
    fn default() -> Self {
        Self {
            emitting: true,
            lifetime: 1.0,
            width: 0.2,
            min_vertex_distance: 0.05,
            max_points: 256,
            teleport_distance: 20.0,
            camera_facing: true,
            appearance: EffectAppearance::default(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BeamRenderer {
    pub start_target: Option<EntityId>,
    pub end_target: Option<EntityId>,
    pub start: [f32; 3],
    pub end: [f32; 3],
    pub width: f32,
    pub segments: u32,
    pub noise_amplitude: f32,
    pub noise_frequency: f32,
    pub noise_speed: f32,
    pub branch_count: u32,
    pub appearance: EffectAppearance,
}
impl Default for BeamRenderer {
    fn default() -> Self {
        Self {
            start_target: None,
            end_target: None,
            start: [0.0; 3],
            end: [0.0, 3.0, 0.0],
            width: 0.1,
            segments: 24,
            noise_amplitude: 0.2,
            noise_frequency: 3.0,
            noise_speed: 5.0,
            branch_count: 0,
            appearance: EffectAppearance::default(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RingEffect {
    pub target: Option<EntityId>,
    pub source_velocity: [f32; 3],
    pub wake_angle: f32,
    pub foam_intensity: f32,
    pub foam_lifetime: f32,
    pub normal_strength: f32,
    pub max_disturbances: u32,
    pub initial_radius: f32,
    pub final_radius: f32,
    pub thickness: f32,
    pub vertical_offset: f32,
    pub amplitude: f32,
    pub wavelength: f32,
    pub damping: f32,
    pub appearance: EffectAppearance,
}
impl Default for RingEffect {
    fn default() -> Self {
        Self {
            target: None,
            source_velocity: [0.0; 3],
            wake_angle: 0.6,
            foam_intensity: 0.5,
            foam_lifetime: 1.0,
            normal_strength: 1.0,
            max_disturbances: 32,
            initial_radius: 0.1,
            final_radius: 4.0,
            thickness: 0.2,
            vertical_offset: 0.01,
            amplitude: 0.1,
            wavelength: 1.0,
            damping: 1.0,
            appearance: EffectAppearance::default(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RibbonRenderer {
    pub points: Vec<[f32; 3]>,
    pub width: f32,
    pub tube: bool,
    pub radial_segments: u32,
    pub subdivisions: u32,
    pub twist: f32,
    pub appearance: EffectAppearance,
}
impl Default for RibbonRenderer {
    fn default() -> Self {
        Self {
            points: vec![[0.0; 3], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0]],
            width: 0.2,
            tube: false,
            radial_segments: 8,
            subdivisions: 8,
            twist: 0.0,
            appearance: EffectAppearance::default(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VolumeShape {
    #[default]
    Box,
    Sphere,
    Plane,
    Cylinder,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForceType {
    #[default]
    Directional,
    Attraction,
    Repulsion,
    Vortex,
    Turbulence,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ForceField {
    pub shape: VolumeShape,
    pub dimensions: [f32; 3],
    pub force_type: ForceType,
    pub direction: [f32; 3],
    pub strength: f32,
    pub falloff: f32,
    pub turbulence: f32,
    pub frequency: f32,
    pub layers: u32,
    pub global: bool,
}
impl Default for ForceField {
    fn default() -> Self {
        Self {
            shape: VolumeShape::Sphere,
            dimensions: [5.0; 3],
            force_type: ForceType::Directional,
            direction: [1.0, 0.0, 0.0],
            strength: 1.0,
            falloff: 1.0,
            turbulence: 0.0,
            frequency: 1.0,
            layers: u32::MAX,
            global: false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CollisionVolume {
    pub shape: VolumeShape,
    pub dimensions: [f32; 3],
    pub layers: u32,
    pub bounce: f32,
    pub friction: f32,
    pub response: CollisionResponse,
    pub impact_effect: Option<AssetId>,
}
impl Default for CollisionVolume {
    fn default() -> Self {
        Self {
            shape: VolumeShape::Plane,
            dimensions: [10.0; 3],
            layers: u32::MAX,
            bounce: 0.5,
            friction: 0.1,
            response: CollisionResponse::Bounce,
            impact_effect: None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectileTracer {
    pub velocity: [f32; 3],
    pub length: f32,
    pub width: f32,
    pub appearance: EffectAppearance,
}
impl Default for ProjectileTracer {
    fn default() -> Self {
        Self {
            velocity: [0.0, 0.0, 10.0],
            length: 2.0,
            width: 0.05,
            appearance: EffectAppearance::default(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FogVolume {
    pub shape: VolumeShape,
    pub dimensions: [f32; 3],
    pub density: f32,
    pub color: [f32; 3],
    pub height_falloff: f32,
    pub edge_falloff: f32,
    pub noise_scale: f32,
    pub noise_speed: f32,
    pub anisotropy: f32,
    pub steps: u32,
}
impl Default for FogVolume {
    fn default() -> Self {
        Self {
            shape: VolumeShape::Box,
            dimensions: [10.0, 3.0, 10.0],
            density: 0.1,
            color: [0.6, 0.7, 0.8],
            height_falloff: 0.5,
            edge_falloff: 1.0,
            noise_scale: 1.0,
            noise_speed: 0.1,
            anisotropy: 0.0,
            steps: 32,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DecalProjector {
    pub dimensions: [f32; 3],
    pub appearance: EffectAppearance,
    pub normal_map: Option<AssetId>,
    pub orm_map: Option<AssetId>,
    pub roughness: f32,
    pub metallic: f32,
    pub edge_fade: f32,
    pub angle_fade: f32,
    pub layers: u32,
}
impl Default for DecalProjector {
    fn default() -> Self {
        Self {
            dimensions: [1.0, 1.0, 1.0],
            appearance: EffectAppearance::default(),
            normal_map: None,
            orm_map: None,
            roughness: 0.8,
            metallic: 0.0,
            edge_fade: 0.1,
            angle_fade: 0.5,
            layers: u32::MAX,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeatherType {
    #[default]
    Rain,
    Snow,
    Hail,
    Dust,
    Sand,
    Leaves,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeatherVolume {
    pub weather: WeatherType,
    pub emitter: ParticleEmitter,
    pub intensity: f32,
    pub camera_follow: bool,
}
impl Default for WeatherVolume {
    fn default() -> Self {
        let emitter = ParticleEmitter {
            shape: SpawnShape::Box,
            dimensions: [20.0, 10.0, 20.0],
            direction: [0.0, -1.0, 0.0],
            speed: [8.0, 10.0],
            gravity: [0.0; 3],
            rate: 200.0,
            lifetime: [2.0, 3.0],
            ..ParticleEmitter::default()
        };
        Self {
            weather: WeatherType::Rain,
            emitter,
            intensity: 1.0,
            camera_follow: false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DistortionEffect {
    pub strength: f32,
    pub noise_scale: f32,
    pub speed: f32,
    pub radius: f32,
    pub edge_falloff: f32,
}
impl Default for DistortionEffect {
    fn default() -> Self {
        Self {
            strength: 0.01,
            noise_scale: 10.0,
            speed: 1.0,
            radius: 3.0,
            edge_falloff: 1.0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightShaft {
    pub light: Option<EntityId>,
    pub intensity: f32,
    pub color: [f32; 3],
    pub density: f32,
    pub anisotropy: f32,
    pub max_distance: f32,
    pub samples: u32,
}
impl Default for LightShaft {
    fn default() -> Self {
        Self {
            light: None,
            intensity: 0.5,
            color: [1.0, 0.9, 0.6],
            density: 0.1,
            anisotropy: 0.5,
            max_distance: 100.0,
            samples: 32,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FoliageScatter {
    pub meshes: Vec<AssetId>,
    pub density: f32,
    pub dimensions: [f32; 3],
    pub scale: [f32; 2],
    pub max_instances: u32,
    pub wind_response: f32,
    pub appearance: EffectAppearance,
}
impl Default for FoliageScatter {
    fn default() -> Self {
        Self {
            meshes: vec![],
            density: 1.0,
            dimensions: [10.0, 1.0, 10.0],
            scale: [0.5, 1.5],
            max_instances: 1000,
            wind_response: 1.0,
            appearance: EffectAppearance::default(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DissolveEffect {
    pub target: Option<EntityId>,
    pub progress: f32,
    pub edge_width: f32,
    pub edge_color: [f32; 3],
    pub edge_emission: f32,
    pub noise_scale: f32,
    pub curve: Curve,
}
impl Default for DissolveEffect {
    fn default() -> Self {
        Self {
            target: None,
            progress: 0.0,
            edge_width: 0.05,
            edge_color: [1.0, 0.4, 0.0],
            edge_emission: 3.0,
            noise_scale: 10.0,
            curve: Curve::linear(0.0, 1.0),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimatedSurface {
    pub target: Option<EntityId>,
    pub uv_scroll: [f32; 2],
    pub uv_scale: [f32; 2],
    pub uv_rotation: f32,
    pub color: Gradient,
    pub emission: f32,
    pub speed: f32,
}
impl Default for AnimatedSurface {
    fn default() -> Self {
        Self {
            target: None,
            uv_scroll: [0.2, 0.0],
            uv_scale: [1.0; 2],
            uv_rotation: 0.0,
            color: Gradient::default(),
            emission: 1.0,
            speed: 1.0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LensFlare {
    pub light: Option<EntityId>,
    pub intensity: f32,
    pub scale: f32,
    pub color: [f32; 3],
    pub ghosts: u32,
    pub ghost_spacing: f32,
    pub streak_length: f32,
    pub fade_speed: f32,
    pub texture: Option<AssetId>,
}
impl Default for LensFlare {
    fn default() -> Self {
        Self {
            light: None,
            intensity: 1.0,
            scale: 0.1,
            color: [1.0, 0.8, 0.5],
            ghosts: 4,
            ghost_spacing: 0.2,
            streak_length: 0.3,
            fade_speed: 5.0,
            texture: None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightPulse {
    pub target: Option<EntityId>,
    pub intensity: Curve,
    pub color: Gradient,
    pub frequency: f32,
    pub randomness: f32,
    pub phase: f32,
}
impl Default for LightPulse {
    fn default() -> Self {
        Self {
            target: None,
            intensity: Curve::linear(0.0, 1.0),
            color: Gradient::default(),
            frequency: 1.0,
            randomness: 0.0,
            phase: 0.0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PostProcessVolume {
    pub global: bool,
    pub dimensions: [f32; 3],
    pub blend_distance: f32,
    pub weight: f32,
    pub exposure: f32,
    pub saturation: f32,
    pub contrast: f32,
    pub brightness: f32,
    pub tint: [f32; 3],
    pub bloom: f32,
    pub vignette: f32,
    pub chromatic_aberration: f32,
    pub grain: f32,
    pub depth_of_field: f32,
}
impl Default for PostProcessVolume {
    fn default() -> Self {
        Self {
            global: true,
            dimensions: [10.0; 3],
            blend_distance: 1.0,
            weight: 1.0,
            exposure: 0.0,
            saturation: 1.0,
            contrast: 1.0,
            brightness: 0.0,
            tint: [1.0; 3],
            bloom: 0.0,
            vignette: 0.0,
            chromatic_aberration: 0.0,
            grain: 0.0,
            depth_of_field: 0.0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutlineEffect {
    pub targets: Vec<EntityId>,
    pub color: [f32; 4],
    pub thickness: f32,
    pub pulse_speed: f32,
    pub through_walls: bool,
}
impl Default for OutlineEffect {
    fn default() -> Self {
        Self {
            targets: vec![],
            color: [1.0, 0.6, 0.0, 1.0],
            thickness: 0.03,
            pulse_speed: 0.0,
            through_walls: false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraShake {
    pub target: Option<EntityId>,
    pub position_amplitude: [f32; 3],
    pub rotation_amplitude: [f32; 3],
    pub frequency: f32,
    pub falloff: Curve,
    pub radius: f32,
}
impl Default for CameraShake {
    fn default() -> Self {
        Self {
            target: None,
            position_amplitude: [0.1; 3],
            rotation_amplitude: [0.02; 3],
            frequency: 20.0,
            falloff: Curve::linear(1.0, 0.0),
            radius: 20.0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenFlash {
    pub target: Option<EntityId>,
    pub color: [f32; 4],
    pub opacity: f32,
    pub fade_in: f32,
    pub fade_out: f32,
    pub vignette: f32,
    pub blend: BlendMode,
}
impl Default for ScreenFlash {
    fn default() -> Self {
        Self {
            target: None,
            color: [1.0, 0.0, 0.0, 1.0],
            opacity: 0.5,
            fade_in: 0.05,
            fade_out: 0.4,
            vignette: 0.5,
            blend: BlendMode::Alpha,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProbeRefresh {
    #[default]
    Baked,
    OnEnable,
    Interval,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReflectionProbe {
    pub dimensions: [f32; 3],
    pub cubemap: Option<AssetId>,
    pub resolution: u32,
    pub refresh: ProbeRefresh,
    pub blend_distance: f32,
    pub update_interval: f32,
    pub parallax_correction: bool,
}
impl Default for ReflectionProbe {
    fn default() -> Self {
        Self {
            dimensions: [10.0; 3],
            cubemap: None,
            resolution: 128,
            refresh: ProbeRefresh::Baked,
            blend_distance: 1.0,
            update_interval: 1.0,
            parallax_correction: false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EffectKind {
    Trail(TrailRenderer),
    Particle(ParticleEmitter),
    Beam(BeamRenderer),
    Shockwave(RingEffect),
    MeshParticle(ParticleEmitter),
    Ribbon(RibbonRenderer),
    ForceField(ForceField),
    CollisionVolume(CollisionVolume),
    ProjectileTracer(ProjectileTracer),
    Fog(FogVolume),
    Decal(DecalProjector),
    Weather(WeatherVolume),
    WaterRipple(RingEffect),
    Wind(ForceField),
    HeatHaze(DistortionEffect),
    LightShaft(LightShaft),
    FoliageScatter(FoliageScatter),
    Dissolve(DissolveEffect),
    AnimatedSurface(AnimatedSurface),
    LensFlare(LensFlare),
    LightPulse(LightPulse),
    PostProcess(PostProcessVolume),
    Outline(OutlineEffect),
    CameraShake(CameraShake),
    ScreenFlash(ScreenFlash),
    ReflectionProbe(ReflectionProbe),
}
impl Default for EffectKind {
    fn default() -> Self {
        Self::Particle(ParticleEmitter::default())
    }
}
impl EffectKind {
    pub fn all_defaults() -> Vec<Self> {
        vec![
            Self::Trail(TrailRenderer::default()),
            Self::Particle(ParticleEmitter::default()),
            Self::Beam(BeamRenderer::default()),
            Self::Shockwave(RingEffect::default()),
            Self::MeshParticle(ParticleEmitter::default()),
            Self::Ribbon(RibbonRenderer::default()),
            Self::ForceField(ForceField::default()),
            Self::CollisionVolume(CollisionVolume::default()),
            Self::ProjectileTracer(ProjectileTracer::default()),
            Self::Fog(FogVolume::default()),
            Self::Decal(DecalProjector::default()),
            Self::Weather(WeatherVolume::default()),
            Self::WaterRipple(RingEffect::default()),
            Self::Wind(ForceField::default()),
            Self::HeatHaze(DistortionEffect::default()),
            Self::LightShaft(LightShaft::default()),
            Self::FoliageScatter(FoliageScatter::default()),
            Self::Dissolve(DissolveEffect::default()),
            Self::AnimatedSurface(AnimatedSurface::default()),
            Self::LensFlare(LensFlare::default()),
            Self::LightPulse(LightPulse::default()),
            Self::PostProcess(PostProcessVolume::default()),
            Self::Outline(OutlineEffect::default()),
            Self::CameraShake(CameraShake::default()),
            Self::ScreenFlash(ScreenFlash::default()),
            Self::ReflectionProbe(ReflectionProbe::default()),
        ]
    }
    pub fn name(&self) -> &'static str {
        match self {
            Self::Trail(_) => "Trail Renderer",
            Self::Particle(_) => "Particle Emitter",
            Self::Beam(_) => "Beam / Lightning",
            Self::Shockwave(_) => "Shockwave",
            Self::MeshParticle(_) => "Mesh Particle Emitter",
            Self::Ribbon(_) => "Ribbon / Tube",
            Self::ForceField(_) => "Particle Force Field",
            Self::CollisionVolume(_) => "Particle Collision Volume",
            Self::ProjectileTracer(_) => "Projectile Tracer",
            Self::Fog(_) => "Fog Volume",
            Self::Decal(_) => "Decal Projector",
            Self::Weather(_) => "Weather Volume",
            Self::WaterRipple(_) => "Water Ripple / Wake",
            Self::Wind(_) => "Wind Zone",
            Self::HeatHaze(_) => "Heat Haze",
            Self::LightShaft(_) => "Light Shaft",
            Self::FoliageScatter(_) => "Foliage Scatter",
            Self::Dissolve(_) => "Dissolve",
            Self::AnimatedSurface(_) => "Animated Surface",
            Self::LensFlare(_) => "Lens Flare",
            Self::LightPulse(_) => "Light Pulse / Flicker",
            Self::PostProcess(_) => "Post-Processing Volume",
            Self::Outline(_) => "Outline / Highlight",
            Self::CameraShake(_) => "Camera Shake",
            Self::ScreenFlash(_) => "Screen Flash",
            Self::ReflectionProbe(_) => "Reflection Probe",
        }
    }
}
/// Stable reusable asset origin; authored fields are per-instance overrides.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectSource {
    pub asset: AssetId,
    pub child: u32,
    #[serde(default)]
    pub baseline: Option<Box<EffectDefaults>>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectDefaults {
    pub common: EffectCommon,
    pub kind: EffectKind,
}
impl EffectDefaults {
    pub fn from_component(effect: &EffectComponent) -> Self {
        Self {
            common: effect.common.clone(),
            kind: effect.kind.clone(),
        }
    }
    pub fn into_component(self) -> EffectComponent {
        EffectComponent {
            source: None,
            common: self.common,
            kind: self.kind,
        }
    }
}
#[derive(Component, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectComponent {
    pub source: Option<EffectSource>,
    pub common: EffectCommon,
    pub kind: EffectKind,
}
impl EffectComponent {
    pub fn new(kind: EffectKind) -> Self {
        Self {
            kind,
            ..Self::default()
        }
    }
    /// Validates finite values, supported modes and bounded resource capacities.
    ///
    /// # Errors
    /// Returns a property-specific diagnostic for invalid authored configuration.
    pub fn validate(&self) -> Result<(), String> {
        self.validate_values()?;
        let c = &self.common;
        if ![c.duration, c.delay, c.time_scale, c.render_distance]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0)
            || c.duration <= 0.0
        {
            return Err("effect duration must be positive; delay, speed and distance must be finite and nonnegative".into());
        }
        match &self.kind {
            EffectKind::Particle(p) | EffectKind::MeshParticle(p) => validate_particles(p),
            EffectKind::Weather(w) => validate_particles(&w.emitter),
            EffectKind::Trail(t)
                if t.max_points > 65536
                    || t.max_points < 2
                    || t.lifetime <= 0.0
                    || t.width <= 0.0 =>
            {
                Err("invalid trail capacity, width or lifetime".into())
            }
            EffectKind::Beam(b) if b.segments < 1 || b.segments > 4096 => {
                Err("beam segments must be 1..4096".into())
            }
            EffectKind::Ribbon(r)
                if r.points.len() < 2
                    || r.points.len() > 4096
                    || r.radial_segments < 3
                    || r.radial_segments > 64
                    || r.subdivisions > 64 =>
            {
                Err("invalid ribbon points or tessellation".into())
            }
            _ => Ok(()),
        }
    }
}
fn validate_particles(p: &ParticleEmitter) -> Result<(), String> {
    if p.max_particles > 100_000
        || ![
            p.rate,
            p.rate_over_distance,
            p.lifetime[0],
            p.lifetime[1],
            p.drag,
            p.start_size[0],
            p.start_size[1],
        ]
        .iter()
        .all(|x| x.is_finite() && *x >= 0.0)
        || p.lifetime[0] <= 0.0
        || p.lifetime[1] < p.lifetime[0]
        || p.bursts.len() > 1024
    {
        return Err("invalid particle limits, emission rate or lifetime".into());
    }
    Ok(())
}
impl EffectComponent {
    /// Remaps authored object references when instantiating or duplicating a scene.
    pub fn remap_entities(&mut self, map: &std::collections::BTreeMap<EntityId, EntityId>) {
        fn remap(id: &mut Option<EntityId>, map: &std::collections::BTreeMap<EntityId, EntityId>) {
            if let Some(value) = id.as_mut()
                && let Some(replacement) = map.get(value)
            {
                *value = *replacement;
            }
        }
        remap(&mut self.common.attachment_target, map);
        match &mut self.kind {
            EffectKind::Beam(p) => {
                remap(&mut p.start_target, map);
                remap(&mut p.end_target, map);
            }
            EffectKind::Shockwave(p) | EffectKind::WaterRipple(p) => remap(&mut p.target, map),
            EffectKind::Dissolve(p) => remap(&mut p.target, map),
            EffectKind::AnimatedSurface(p) => remap(&mut p.target, map),
            EffectKind::LightPulse(p) => remap(&mut p.target, map),
            EffectKind::CameraShake(p) => remap(&mut p.target, map),
            EffectKind::ScreenFlash(p) => remap(&mut p.target, map),
            EffectKind::LightShaft(p) => remap(&mut p.light, map),
            EffectKind::LensFlare(p) => remap(&mut p.light, map),
            EffectKind::Outline(p) => {
                for target in &mut p.targets {
                    if let Some(replacement) = map.get(target) {
                        *target = *replacement;
                    }
                }
            }
            _ => {}
        }
    }
}
impl EffectComponent {
    // One dispatch keeps validation rules next to all 26 serialized variants.
    #[allow(clippy::too_many_lines)]
    fn validate_values(&self) -> Result<(), String> {
        fn values(v: &[f32]) -> Result<(), String> {
            if v.iter().all(|x| x.is_finite() && x.abs() <= 1.0e8) {
                Ok(())
            } else {
                Err("effect values must be finite and within +/-100000000".into())
            }
        }
        fn dimensions(v: &[f32]) -> Result<(), String> {
            values(v)?;
            if v.iter().all(|x| *x > 0.0) {
                Ok(())
            } else {
                Err("effect dimensions must be positive".into())
            }
        }
        fn curve(c: &Curve) -> Result<(), String> {
            if c.keys.is_empty() || c.keys.len() > 1024 {
                return Err("curves require 1..1024 keys".into());
            }
            for key in &c.keys {
                values(&[key.time, key.value])?;
                if !(0.0..=1.0).contains(&key.time) {
                    return Err("curve times must be normalized to 0..1".into());
                }
            }
            Ok(())
        }
        fn gradient(g: &Gradient) -> Result<(), String> {
            if g.keys.is_empty() || g.keys.len() > 1024 {
                return Err("gradients require 1..1024 keys".into());
            }
            for k in &g.keys {
                values(&k.color)?;
                if !k.time.is_finite() || !(0.0..=1.0).contains(&k.time) {
                    return Err("gradient times must be normalized to 0..1".into());
                }
            }
            Ok(())
        }
        fn appearance(a: &EffectAppearance) -> Result<(), String> {
            gradient(&a.color)?;
            curve(&a.opacity)?;
            curve(&a.size)?;
            values(&[a.emission])?;
            values(&a.uv_scale)?;
            values(&a.uv_scroll)
        }
        fn particles(p: &ParticleEmitter) -> Result<(), String> {
            if p.mesh_spawn_triangles.len() > 100_000 {
                return Err("mesh surface emission supports at most 100_000 triangles".into());
            }
            if p.shape == SpawnShape::MeshSurface && p.mesh_spawn_triangles.is_empty() {
                return Err("mesh surface emission requires loaded mesh_spawn_triangles".into());
            }
            for triangle in &p.mesh_spawn_triangles {
                for point in triangle {
                    values(point)?;
                }
            }
            values(&[p.prewarm_time])?;
            if p.prewarm_time < 0.0 || p.prewarm_time > 10.0 {
                return Err("prewarm time must be 0..10 seconds".into());
            }
            values(&p.dimensions)?;
            values(&p.lifetime)?;
            values(&p.speed)?;
            values(&p.direction)?;
            values(&p.start_size)?;
            values(&p.gravity)?;
            values(&p.force)?;
            values(&[
                p.rate,
                p.rate_over_distance,
                p.drag,
                p.wind_influence,
                p.noise,
                p.angular_velocity,
                p.inherit_velocity,
                p.velocity_limit,
                p.bounce,
                p.friction,
                p.sub_emitter_probability,
                p.flipbook_fps,
                p.stretch,
            ])?;
            appearance(&p.appearance)?;
            if p.lifetime[1] < p.lifetime[0]
                || p.speed[1] < p.speed[0]
                || p.start_size[1] < p.start_size[0]
                || p.max_generations > 8
                || p.flipbook.contains(&0)
                || p.flipbook.iter().any(|n| *n > 1024)
            {
                return Err(
                    "invalid particle range, sub-emitter generations or flipbook dimensions".into(),
                );
            }
            for b in &p.bursts {
                values(&[b.time, b.probability, b.interval])?;
                if b.time < 0.0
                    || !(0.0..=1.0).contains(&b.probability)
                    || b.count > 100_000
                    || b.cycles > 1024
                    || b.interval < 0.0
                {
                    return Err("invalid timed burst".into());
                }
            }
            Ok(())
        }
        if let Some(bounds) = &self.common.bounds {
            values(&bounds.center)?;
            dimensions(&bounds.half_extents)?;
        }
        if self.common.parameters.len() > 256 {
            return Err("effect parameters are limited to 256 bindings".into());
        }
        for (key, value) in &self.common.parameters {
            if key.len() > 128 {
                return Err("effect parameter name exceeds128 bytes".into());
            }
            match value {
                EffectParameter::Scalar(v) => values(&[*v])?,
                EffectParameter::Vector(v) => values(v)?,
                EffectParameter::Color(v) => values(v)?,
                EffectParameter::Boolean(_) => {}
            }
        }
        match &self.kind {
            EffectKind::Trail(p) => {
                values(&[
                    p.lifetime,
                    p.width,
                    p.min_vertex_distance,
                    p.teleport_distance,
                ])?;
                appearance(&p.appearance)
            }
            EffectKind::Particle(p) | EffectKind::MeshParticle(p) => particles(p),
            EffectKind::Beam(p) => {
                values(&p.start)?;
                values(&p.end)?;
                values(&[p.width, p.noise_amplitude, p.noise_frequency, p.noise_speed])?;
                if p.branch_count > 32 {
                    return Err("beam branches must be <=32".into());
                }
                appearance(&p.appearance)
            }
            EffectKind::Shockwave(p) | EffectKind::WaterRipple(p) => {
                values(&p.source_velocity)?;
                values(&[
                    p.wake_angle,
                    p.foam_intensity,
                    p.foam_lifetime,
                    p.normal_strength,
                ])?;
                if p.max_disturbances == 0 || p.max_disturbances > 256 || p.foam_lifetime <= 0.0 {
                    return Err(
                        "water disturbances must be 1..256 and foam lifetime positive".into(),
                    );
                }
                values(&[
                    p.initial_radius,
                    p.final_radius,
                    p.thickness,
                    p.vertical_offset,
                    p.amplitude,
                    p.wavelength,
                    p.damping,
                ])?;
                if p.initial_radius < 0.0 || p.final_radius <= 0.0 || p.thickness <= 0.0 {
                    return Err("ring radius and thickness must be positive".into());
                }
                appearance(&p.appearance)
            }
            EffectKind::Ribbon(p) => {
                for v in &p.points {
                    values(v)?;
                }
                values(&[p.width, p.twist])?;
                appearance(&p.appearance)
            }
            EffectKind::ForceField(p) | EffectKind::Wind(p) => {
                dimensions(&p.dimensions)?;
                values(&p.direction)?;
                values(&[p.strength, p.falloff, p.turbulence, p.frequency])
            }
            EffectKind::CollisionVolume(p) => {
                dimensions(&p.dimensions)?;
                values(&[p.bounce, p.friction])
            }
            EffectKind::ProjectileTracer(p) => {
                values(&p.velocity)?;
                dimensions(&[p.length, p.width])?;
                appearance(&p.appearance)
            }
            EffectKind::Fog(p) => {
                dimensions(&p.dimensions)?;
                values(&p.color)?;
                values(&[
                    p.density,
                    p.height_falloff,
                    p.edge_falloff,
                    p.noise_scale,
                    p.noise_speed,
                    p.anisotropy,
                ])?;
                if p.steps < 4 || p.steps > 128 || p.density < 0.0 {
                    return Err("fog requires 4..128 steps and nonnegative density".into());
                }
                Ok(())
            }
            EffectKind::Decal(p) => {
                dimensions(&p.dimensions)?;
                appearance(&p.appearance)?;
                values(&[p.roughness, p.metallic, p.edge_fade, p.angle_fade])
            }
            EffectKind::Weather(p) => {
                values(&[p.intensity])?;
                validate_particles(&p.emitter)?;
                particles(&p.emitter)
            }
            EffectKind::HeatHaze(p) => {
                values(&[p.strength, p.noise_scale, p.speed, p.radius, p.edge_falloff])
            }
            EffectKind::LightShaft(p) => {
                values(&p.color)?;
                values(&[p.intensity, p.density, p.anisotropy, p.max_distance])?;
                if p.samples < 4 || p.samples > 128 {
                    return Err("light shaft samples must be 4..128".into());
                }
                Ok(())
            }
            EffectKind::FoliageScatter(p) => {
                dimensions(&p.dimensions)?;
                values(&p.scale)?;
                values(&[p.density, p.wind_response])?;
                if p.max_instances > 100_000 || p.meshes.len() > 256 {
                    return Err("scatter capacity exceeds supported limits".into());
                }
                appearance(&p.appearance)
            }
            EffectKind::Dissolve(p) => {
                values(&p.edge_color)?;
                values(&[p.progress, p.edge_width, p.edge_emission, p.noise_scale])?;
                curve(&p.curve)
            }
            EffectKind::AnimatedSurface(p) => {
                values(&p.uv_scroll)?;
                values(&p.uv_scale)?;
                values(&[p.uv_rotation, p.emission, p.speed])?;
                gradient(&p.color)
            }
            EffectKind::LensFlare(p) => {
                values(&p.color)?;
                values(&[
                    p.intensity,
                    p.scale,
                    p.ghost_spacing,
                    p.streak_length,
                    p.fade_speed,
                ])?;
                if p.ghosts > 16 {
                    return Err("lens flare ghosts must be <=16".into());
                }
                Ok(())
            }
            EffectKind::LightPulse(p) => {
                curve(&p.intensity)?;
                gradient(&p.color)?;
                values(&[p.frequency, p.randomness, p.phase])
            }
            EffectKind::PostProcess(p) => {
                dimensions(&p.dimensions)?;
                values(&p.tint)?;
                values(&[
                    p.blend_distance,
                    p.weight,
                    p.exposure,
                    p.saturation,
                    p.contrast,
                    p.brightness,
                    p.bloom,
                    p.vignette,
                    p.chromatic_aberration,
                    p.grain,
                    p.depth_of_field,
                ])
            }
            EffectKind::Outline(p) => {
                values(&p.color)?;
                values(&[p.thickness, p.pulse_speed])?;
                if p.targets.len() > 100_000 {
                    return Err("too many outline targets".into());
                }
                Ok(())
            }
            EffectKind::CameraShake(p) => {
                values(&p.position_amplitude)?;
                values(&p.rotation_amplitude)?;
                values(&[p.frequency, p.radius])?;
                curve(&p.falloff)
            }
            EffectKind::ScreenFlash(p) => {
                values(&p.color)?;
                values(&[p.opacity, p.fade_in, p.fade_out, p.vignette])
            }
            EffectKind::ReflectionProbe(p) => {
                if p.refresh != ProbeRefresh::Baked {
                    return Err("dynamic reflection probe capture is unsupported; select Baked and assign a panorama asset".into());
                }
                dimensions(&p.dimensions)?;
                values(&[p.blend_distance, p.update_interval])?;
                if !(16..=2048).contains(&p.resolution) || !p.resolution.is_power_of_two() {
                    return Err("probe resolution must be power-of-two 16..2048".into());
                }
                Ok(())
            }
        }
    }
}
#[cfg(test)]
#[allow(clippy::float_cmp)] // Exact interpolation and zero-state invariants.
mod tests {
    use super::*;
    #[test]
    fn every_effect_default_validates() {
        let all = EffectKind::all_defaults();
        assert_eq!(all.len(), 26);
        for kind in all {
            let effect = EffectComponent::new(kind);
            assert_eq!(effect.validate(), Ok(()), "{}", effect.kind.name());
        }
    }
    #[test]
    fn curves_accept_unsorted_keys_and_clamp() {
        let c = Curve {
            keys: vec![
                CurveKey {
                    time: 1.0,
                    value: 10.0,
                },
                CurveKey {
                    time: 0.0,
                    value: 0.0,
                },
            ],
        };
        assert_eq!(c.sample(0.25), 2.5);
        assert_eq!(c.sample(-1.0), 0.0);
        assert_eq!(c.sample(2.0), 10.0);
    }
    #[test]
    fn gradients_interpolate_rgba() {
        let g = Gradient {
            keys: vec![
                GradientKey {
                    time: 0.0,
                    color: [0.0; 4],
                },
                GradientKey {
                    time: 1.0,
                    color: [1.0; 4],
                },
            ],
        };
        assert_eq!(g.sample(0.5), [0.5; 4]);
    }
    #[test]
    fn rejects_nonfinite_and_unbounded_configs() {
        let mut e = EffectComponent::new(EffectKind::HeatHaze(DistortionEffect::default()));
        if let EffectKind::HeatHaze(p) = &mut e.kind {
            p.strength = f32::NAN;
        }
        assert!(e.validate().is_err());
        let mut e = EffectComponent::default();
        if let EffectKind::Particle(p) = &mut e.kind {
            p.max_particles = 1_000_000;
        }
        assert!(e.validate().is_err());
    }
    #[test]
    fn scene_references_remap() {
        let source = EntityId::new();
        let dest = EntityId::new();
        let mut e = EffectComponent::new(EffectKind::Beam(BeamRenderer {
            end_target: Some(source),
            ..Default::default()
        }));
        e.common.attachment_target = Some(source);
        e.remap_entities(&[(source, dest)].into_iter().collect());
        assert_eq!(e.common.attachment_target, Some(dest));
        if let EffectKind::Beam(b) = e.kind {
            assert_eq!(b.end_target, Some(dest));
        } else {
            panic!("expected beam")
        }
    }
}
/// A reusable gradient source with a stable per-particle random factor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ColorParameter {
    Gradient(Gradient),
    BetweenGradients { min: Gradient, max: Gradient },
}
impl ColorParameter {
    pub fn sample(&self, time: f32, random: f32) -> [f32; 4] {
        match self {
            Self::Gradient(g) => g.sample(time),
            Self::BetweenGradients { min, max } => {
                let a = min.sample(time);
                let b = max.sample(time);
                std::array::from_fn(|i| a[i] + (b[i] - a[i]) * random.clamp(0.0, 1.0))
            }
        }
    }
}
/// Independent, normalized curves for all three vector channels.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VectorCurve {
    pub x: Curve,
    pub y: Curve,
    pub z: Curve,
}
impl VectorCurve {
    pub fn sample(&self, time: f32) -> [f32; 3] {
        [
            self.x.sample(time),
            self.y.sample(time),
            self.z.sample(time),
        ]
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EffectParameter {
    Scalar(f32),
    Vector([f32; 3]),
    Color([f32; 4]),
    Boolean(bool),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectBounds {
    pub center: [f32; 3],
    pub half_extents: [f32; 3],
}
impl EffectComponent {
    /// Applies named instance parameters without altering shared asset defaults.
    /// Supported bindings: enabled, visible, `emission_rate`, intensity, color,
    /// speed, size, gravity, force, density, progress, width and radius.
    pub fn resolve_parameters(&mut self) {
        let parameters = std::mem::take(&mut self.common.parameters);
        for (name, value) in &parameters {
            match (name.as_str(), value) {
                ("enabled", EffectParameter::Boolean(v)) => self.common.enabled = *v,
                ("visible", EffectParameter::Boolean(v)) => self.common.visible = *v,
                ("emission_rate", EffectParameter::Scalar(v)) => match &mut self.kind {
                    EffectKind::Particle(p) | EffectKind::MeshParticle(p) => p.rate = v.max(0.0),
                    EffectKind::Weather(p) => p.emitter.rate = v.max(0.0),
                    _ => {}
                },
                ("gravity", EffectParameter::Vector(v)) => match &mut self.kind {
                    EffectKind::Particle(p) | EffectKind::MeshParticle(p) => p.gravity = *v,
                    EffectKind::Weather(p) => p.emitter.gravity = *v,
                    _ => {}
                },
                ("force", EffectParameter::Vector(v)) => match &mut self.kind {
                    EffectKind::Particle(p) | EffectKind::MeshParticle(p) => p.force = *v,
                    EffectKind::Weather(p) => p.emitter.force = *v,
                    _ => {}
                },
                ("speed", EffectParameter::Scalar(v)) => match &mut self.kind {
                    EffectKind::Particle(p) | EffectKind::MeshParticle(p) => p.speed = [*v; 2],
                    EffectKind::Weather(p) => p.emitter.speed = [*v; 2],
                    _ => {}
                },
                ("size", EffectParameter::Scalar(v)) => match &mut self.kind {
                    EffectKind::Particle(p) | EffectKind::MeshParticle(p) => {
                        p.start_size = [v.max(0.0); 2];
                    }
                    EffectKind::Weather(p) => p.emitter.start_size = [v.max(0.0); 2],
                    _ => {}
                },
                ("intensity", EffectParameter::Scalar(v)) => match &mut self.kind {
                    EffectKind::Weather(p) => p.intensity = v.max(0.0),
                    EffectKind::LightShaft(p) => p.intensity = v.max(0.0),
                    EffectKind::LensFlare(p) => p.intensity = v.max(0.0),
                    EffectKind::ForceField(p) | EffectKind::Wind(p) => p.strength = *v,
                    EffectKind::ScreenFlash(p) => p.opacity = v.clamp(0.0, 1.0),
                    _ => {
                        if let Some(a) = self.appearance_mut() {
                            a.emission = v.max(0.0);
                        }
                    }
                },
                ("color", EffectParameter::Color(v)) => {
                    if let Some(a) = self.appearance_mut() {
                        a.color = Gradient::constant(*v);
                    } else {
                        match &mut self.kind {
                            EffectKind::ScreenFlash(p) => p.color = *v,
                            EffectKind::Outline(p) => p.color = *v,
                            EffectKind::Fog(p) => p.color = [v[0], v[1], v[2]],
                            _ => {}
                        }
                    }
                }
                ("density", EffectParameter::Scalar(v)) => match &mut self.kind {
                    EffectKind::Fog(p) => p.density = v.max(0.0),
                    EffectKind::FoliageScatter(p) => p.density = v.max(0.0),
                    _ => {}
                },
                ("progress", EffectParameter::Scalar(v)) => {
                    if let EffectKind::Dissolve(p) = &mut self.kind {
                        p.progress = v.clamp(0.0, 1.0);
                    }
                }
                ("width", EffectParameter::Scalar(v)) => match &mut self.kind {
                    EffectKind::Trail(p) => p.width = v.max(0.0001),
                    EffectKind::Beam(p) => p.width = v.max(0.0001),
                    EffectKind::Ribbon(p) => p.width = v.max(0.0001),
                    EffectKind::ProjectileTracer(p) => p.width = v.max(0.0001),
                    _ => {}
                },
                ("radius", EffectParameter::Scalar(v)) => match &mut self.kind {
                    EffectKind::Shockwave(p) | EffectKind::WaterRipple(p) => {
                        p.final_radius = v.max(0.0001);
                    }
                    EffectKind::HeatHaze(p) => p.radius = v.max(0.0001),
                    EffectKind::CameraShake(p) => p.radius = v.max(0.0001),
                    _ => {}
                },
                _ => {}
            }
        }
        self.common.parameters = parameters;
    }
    fn appearance_mut(&mut self) -> Option<&mut EffectAppearance> {
        match &mut self.kind {
            EffectKind::Particle(p) | EffectKind::MeshParticle(p) => Some(&mut p.appearance),
            EffectKind::Trail(p) => Some(&mut p.appearance),
            EffectKind::Beam(p) => Some(&mut p.appearance),
            EffectKind::Shockwave(p) | EffectKind::WaterRipple(p) => Some(&mut p.appearance),
            EffectKind::Ribbon(p) => Some(&mut p.appearance),
            EffectKind::ProjectileTracer(p) => Some(&mut p.appearance),
            EffectKind::Decal(p) => Some(&mut p.appearance),
            EffectKind::Weather(p) => Some(&mut p.emitter.appearance),
            EffectKind::FoliageScatter(p) => Some(&mut p.appearance),
            _ => None,
        }
    }
}
#[cfg(test)]
#[allow(clippy::float_cmp)] // Exact parameter transfer is part of the API.
mod parameter_tests {
    use super::*;
    #[test]
    fn named_parameters_modify_instances_and_preserve_binding_values() {
        let mut effect = EffectComponent::default();
        effect
            .common
            .parameters
            .insert("emission_rate".into(), EffectParameter::Scalar(75.0));
        effect
            .common
            .parameters
            .insert("color".into(), EffectParameter::Color([0.0, 0.5, 1.0, 1.0]));
        effect.resolve_parameters();
        let EffectKind::Particle(p) = &effect.kind else {
            unreachable!()
        };
        assert_eq!(p.rate, 75.0);
        assert_eq!(p.appearance.color.sample(0.5), [0.0, 0.5, 1.0, 1.0]);
        assert_eq!(effect.common.parameters.len(), 2);
        assert!(effect.validate().is_ok());
    }
    #[test]
    fn bounds_and_prewarm_are_validated() {
        let mut effect = EffectComponent::default();
        effect.common.bounds = Some(EffectBounds {
            center: [0.0; 3],
            half_extents: [-1.0, 1.0, 1.0],
        });
        assert!(effect.validate().is_err());
        effect.common.bounds = None;
        if let EffectKind::Particle(p) = &mut effect.kind {
            p.prewarm_time = 100.0;
        }
        assert!(effect.validate().is_err());
    }
}
