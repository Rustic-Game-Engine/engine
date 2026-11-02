//! Shared authored effects, deterministic simulation, and renderer-neutral extraction.
mod model;
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
    pub size: f32,
    pub color: [f32; 4],
    pub mesh: Option<AssetId>,
    pub material: Option<AssetId>,
    pub texture: Option<AssetId>,
    pub emission: f32,
    pub blend: BlendMode,
}
#[derive(Clone, Debug)]
pub struct EffectGeometry {
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
            bloom_threshold: 0.8,
            bloom_radius: 1.0,
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
    pub target: EntityId,
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
    pub water: Vec<RenderWaterDisturbance>,
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
}
#[derive(Clone, Debug)]
pub struct RenderLightShaft {
    pub light: Option<EntityId>,
    pub position: Vec3,
    pub color: [f32; 3],
    pub intensity: f32,
    pub density: f32,
    pub samples: u32,
    pub max_distance: f32,
}
#[derive(Clone, Debug)]
pub struct RenderReflectionProbe {
    pub entity: EntityId,
    pub position: Vec3,
    pub dimensions: [f32; 3],
    pub cubemap: AssetId,
    pub blend_distance: f32,
    pub priority: i32,
}
#[derive(Clone, Debug)]
pub struct RenderDecal {
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
    pub light: Option<EntityId>,
    pub position: Vec3,
    pub intensity: f32,
    pub scale: f32,
    pub color: [f32; 3],
    pub ghosts: u32,
    pub ghost_spacing: f32,
    pub streak_length: f32,
}

#[derive(Clone, Debug)]
pub struct RenderWaterDisturbance {
    pub entity: EntityId,
    pub target: EntityId,
    pub disturbance: WaterDisturbance,
}
