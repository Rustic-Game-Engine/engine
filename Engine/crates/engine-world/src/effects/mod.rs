//! Shared authored effects, deterministic simulation, and renderer-neutral extraction.
mod bindings;
mod model;
pub use bindings::*;
mod gpu;
mod group;
mod probe_control;
pub use gpu::*;
pub use group::*;
mod water;
pub use water::*;
mod defaults;
pub use defaults::merge_instance_defaults;
mod presets;
pub use presets::*;
mod modifiers;
mod simulation;
use crate::{AssetId, EntityId, PrimitiveMesh};
use glam::{Mat4, Vec3};
pub use model::*;
pub use simulation::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackCommand {
    Play,
    Pause,
    Resume,
    Stop,
    Restart,
    Reset,
    Dispose,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Playing,
    Paused,
    Completed,
    Disposed,
}
#[derive(Clone, Debug)]
pub struct EffectInput {
    pub entity: EntityId,
    pub transform: Mat4,
    pub effect: EffectComponent,
}
#[derive(Clone, Debug)]
pub struct ParticleInstance {
    pub entity: EntityId,
    pub position: Vec3,
    pub velocity: Vec3,
    pub stretch: f32,
    /// Sprite atlas rectangle: minimum UV followed by maximum UV.
    pub uv_rect: [f32; 4],
    pub rotation: f32,
    pub rotation3: Vec3,
    pub scale: Vec3,
    pub size: f32,
    pub color: [f32; 4],
    pub mesh: Option<AssetId>,
    pub material: Option<AssetId>,
    pub texture: Option<AssetId>,
    pub emission: f32,
    pub blend: BlendMode,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
    pub wind: Vec3,
    pub sort_mode: ParticleSortMode,
    pub sort_key: f32,
    pub soft_particle_distance: f32,
}
#[derive(Clone, Debug)]
pub struct EffectGeometry {
    /// Render through the retained mesh/PBR pipeline, including procedural meshes.
    pub pbr: bool,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
    pub entity: EntityId,
    pub mesh: PrimitiveMesh,
    pub colors: Vec<[f32; 4]>,
    pub color: [f32; 4],
    pub emission: f32,
    pub material: Option<AssetId>,
    pub texture: Option<AssetId>,
    pub blend: BlendMode,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostProcessSettings {
    pub exposure: f32,
    pub saturation: f32,
    pub contrast: f32,
    pub brightness: f32,
    pub tint: [f32; 3],
    pub bloom: f32,
    pub bloom_threshold: f32,
    pub bloom_radius: f32,
    pub vignette: f32,
    pub chromatic_aberration: f32,
    pub film_grain: f32,
    pub distortion: f32,
    pub distortion_scale: f32,
    pub distortion_speed: f32,
    pub time: f32,
    pub overlay: [f32; 4],
    pub overlay_add: [f32; 3],
    pub overlay_multiply: [f32; 3],
    pub depth_of_field: f32,
    pub motion_blur: f32,
    pub motion_blur_samples: u32,
    pub focus_distance: f32,
    pub focus_range: f32,
    pub temporal_weight: f32,
}
impl Default for PostProcessSettings {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            saturation: 1.0,
            contrast: 1.0,
            brightness: 0.0,
            tint: [1.0; 3],
            bloom: 0.0,
            bloom_threshold: 1.0,
            bloom_radius: 4.0,
            vignette: 0.0,
            chromatic_aberration: 0.0,
            film_grain: 0.0,
            distortion: 0.0,
            distortion_scale: 10.0,
            distortion_speed: 1.0,
            time: 0.0,
            overlay: [0.0; 4],
            overlay_add: [0.0; 3],
            overlay_multiply: [1.0; 3],
            depth_of_field: 0.0,
            motion_blur: 0.0,
            motion_blur_samples: 12,
            focus_distance: 0.0,
            focus_range: 5.0,
            temporal_weight: 0.0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectEventKind {
    Birth,
    Collision,
    Death,
    Completed,
}
#[derive(Clone, Debug)]
pub struct EffectEvent {
    pub entity: EntityId,
    pub kind: EffectEventKind,
    pub position: Vec3,
    /// World-space velocity inherited by a triggered child effect.
    pub velocity: Vec3,
    pub child_effect: Option<AssetId>,
}
#[derive(Clone, Debug)]
pub struct LightModifier {
    pub target: EntityId,
    pub color: [f32; 3],
    pub intensity: f32,
}
#[derive(Clone, Debug)]
pub struct SurfaceModifier {
    pub target_material: Option<AssetId>,
    pub is_dissolve: bool,
    pub source: EntityId,
    pub target: EntityId,
    pub noise_texture: Option<AssetId>,
    pub mask_texture: Option<AssetId>,
    pub distortion_texture: Option<AssetId>,
    pub distortion_strength: f32,
    pub direction: [f32; 3],
    pub phase: f32,
    pub particle_density: f32,
    pub uv_offset: [f32; 2],
    pub uv_scale: [f32; 2],
    pub uv_rotation: f32,
    pub dissolve_noise_scale: f32,
    pub color: [f32; 4],
    pub emission: f32,
    pub dissolve: f32,
    pub edge_width: f32,
    pub edge_color: [f32; 3],
}
#[derive(Clone, Debug, Default)]
pub struct EffectFrame {
    /// Per-owner render layers retained independently of the active camera.
    pub effect_layers: std::collections::BTreeMap<EntityId, u32>,
    /// Resolved world center, bounding radius and maximum view distance per effect.
    pub effect_render_distances: std::collections::BTreeMap<EntityId, (Vec3, f32, f32)>,
    /// New births were refused because the global 100,000 live-particle budget is full.
    pub particle_budget_exhausted: bool,
    pub camera_visibility_layers: Option<u32>,
    pub gpu_emitters: Vec<GpuParticleEmitterFrame>,
    pub particle_backends: Vec<ParticleBackendStatus>,
    pub geometry: Vec<EffectGeometry>,
    pub particles: Vec<ParticleInstance>,
    pub post: PostProcessSettings,
    pub camera_offset: Vec3,
    pub camera_rotation: Vec3,
    pub lights: Vec<LightModifier>,
    pub surfaces: Vec<SurfaceModifier>,
    pub events: Vec<EffectEvent>,
    pub fog: Vec<RenderFogVolume>,
    pub shafts: Vec<RenderLightShaft>,
    pub probes: Vec<RenderReflectionProbe>,
    pub decals: Vec<RenderDecal>,
    pub outlines: Vec<RenderOutline>,
    pub flares: Vec<RenderLensFlare>,
    pub distortions: Vec<RenderDistortion>,
    pub overlays: Vec<RenderScreenOverlay>,
    pub water: Vec<RenderWaterDisturbance>,
    pub wind_zones: Vec<RenderWindZone>,
}
#[derive(Clone, Debug)]
pub struct RenderWindZone {
    pub entity: EntityId,
    pub transform: Mat4,
    pub field: ForceField,
}
#[derive(Clone, Debug)]
pub struct RenderFogVolume {
    pub entity: EntityId,
    pub inverse_transform: Mat4,
    pub dimensions: [f32; 3],
    pub shape: VolumeShape,
    pub density: f32,
    pub color: [f32; 3],
    pub height_falloff: f32,
    pub edge_falloff: f32,
    pub noise_scale: f32,
    pub noise_speed: f32,
    pub steps: u32,
    pub noise_texture: Option<AssetId>,
    pub anisotropy: f32,
    pub light_scattering: f32,
    pub max_distance: f32,
    pub temporal_filtering: f32,
    pub time: f32,
}
#[derive(Clone, Debug)]
pub struct RenderLightShaft {
    pub entity: EntityId,
    pub light: Option<EntityId>,
    pub position: Vec3,
    pub color: [f32; 3],
    pub intensity: f32,
    pub density: f32,
    pub samples: u32,
    pub max_distance: f32,
    pub anisotropy: f32,
    pub occlusion: bool,
    pub temporal_filtering: f32,
    pub localized: bool,
    pub inverse_transform: Mat4,
    pub shape: VolumeShape,
    pub dimensions: [f32; 3],
}
#[derive(Clone, Debug)]
pub struct RenderReflectionProbe {
    pub entity: EntityId,
    pub position: Vec3,
    pub capture_position: Vec3,
    pub inverse_transform: Mat4,
    pub shape: VolumeShape,
    pub dimensions: [f32; 3],
    pub cubemap: AssetId,
    pub blend_distance: f32,
    pub priority: i32,
    pub resolution: u32,
    pub refresh: ProbeRefresh,
    pub update_interval: f32,
    pub parallax_correction: bool,
    pub visibility_layers: u32,
    pub receiver_layers: u32,
    pub quality: QualityLevel,
    pub refresh_revision: u64,
    pub time: f32,
}
#[derive(Clone, Debug)]
pub struct RenderDecal {
    pub shape: VolumeShape,
    pub emissive_map: Option<AssetId>,
    pub distance_fade: f32,
    pub render_distance: f32,
    pub priority: i32,
    pub entity: EntityId,
    pub transform: Mat4,
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
#[derive(Clone, Debug)]
pub struct RenderOutline {
    pub targets: Vec<EntityId>,
    pub color: [f32; 4],
    pub thickness: f32,
    pub through_walls: bool,
}
#[derive(Clone, Debug)]
pub struct RenderLensFlare {
    pub entity: EntityId,
    pub light: Option<EntityId>,
    pub position: Vec3,
    pub intensity: f32,
    pub scale: f32,
    pub color: [f32; 3],
    pub ghosts: u32,
    pub ghost_spacing: f32,
    pub streak_length: f32,
    pub streak_orientation: f32,
    pub texture: Option<AssetId>,
    pub occlusion: bool,
    pub max_distance: f32,
    pub fade_speed: f32,
}

#[derive(Clone, Debug)]
pub struct RenderDistortion {
    pub entity: EntityId,
    pub inverse_transform: Mat4,
    pub radius: f32,
    pub strength: f32,
    pub noise_scale: f32,
    pub speed: f32,
    pub edge_falloff: f32,
    pub noise_texture: Option<AssetId>,
    pub refraction_offset: [f32; 2],
    pub depth_masking: bool,
    pub screen_space: bool,
    pub time: f32,
}
#[derive(Clone, Debug)]
pub struct RenderScreenOverlay {
    pub entity: EntityId,
    pub color: [f32; 4],
    pub opacity: f32,
    pub vignette: f32,
    pub blend: BlendMode,
    pub screen_mask: Option<AssetId>,
    pub stacking: OverlayStacking,
}

#[derive(Clone, Debug)]
pub struct RenderWaterDisturbance {
    pub entity: EntityId,
    pub target: EntityId,
    pub disturbance: WaterDisturbance,
}
