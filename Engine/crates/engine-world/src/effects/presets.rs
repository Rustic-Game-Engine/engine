//! Editable, versioned effect assets. A composite is an ordered set of delayed
//! effects; every child has the same configuration as an ordinary scene effect.
use super::{
    BeamRenderer, BlendMode, Burst, CameraShake, Curve, DissolveEffect, EffectAppearance,
    EffectComponent, EffectDefaults, EffectKind, EffectSource, FogVolume, Gradient, LightPulse,
    OutlineEffect, ParticleEmitter, RibbonRenderer, RingEffect, ScreenFlash, SpawnShape,
    TrailRenderer, WeatherType, WeatherVolume,
};
use crate::{
    AssetId, EntityId, EntitySnapshot, Light, LightKind, LocalTransform, PartAttributes, Primitive,
    SurfaceTextures,
};
use glam::Mat4;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A cue on the composite timeline, resolved through the existing audio asset/mixer path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectSoundCue {
    pub asset: AssetId,
    pub delay: f32,
    pub volume: f32,
    pub pitch: f32,
    pub spatial: bool,
    #[serde(default)]
    pub looping: bool,
}
/// Stable identity of the generated, redistributable built-in explosion sound.
/// The matching `.rmeta` is installed by `engine_assets::install_builtin_effect_resources`.
pub fn explosion_sound_asset() -> AssetId {
    AssetId::from(uuid::Uuid::from_u128(
        0x7275_7374_6963_5f76_6678_5f73_6e64_5f31,
    ))
}
/// Stable identity of the generated PBR debris source installed with built-in resources.
pub fn explosion_mesh_asset() -> AssetId {
    AssetId::from(uuid::Uuid::from_bytes(*b"rustic_vfx_mesh1"))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectAsset {
    pub version: u32,
    pub name: String,
    pub effects: Vec<EffectComponent>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, super::EffectParameter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<super::GroupParameterBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trigger_bindings: Vec<super::GroupTriggerBinding>,
    /// Timed cues use the scene's existing audio service and shared playback clock.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sound_cues: Vec<EffectSoundCue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_settings: Option<super::EffectGroupSettings>,
    /// Optional light components belonging to the corresponding effect children.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lights: Vec<Option<Light>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub primitives: Vec<Option<Primitive>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meshes: Vec<Option<AssetId>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<Option<AssetId>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub textures: Vec<Option<SurfaceTextures>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub part_attributes: Vec<Option<PartAttributes>>,
    /// Original child identities, used only to remap internal entity references.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_ids: Vec<EntityId>,
    /// Column-major affine child transforms relative to the asset group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transforms: Vec<[f32; 16]>,
}
impl EffectAsset {
    pub fn new(name: impl Into<String>, effects: Vec<EffectComponent>) -> Self {
        Self {
            version: 1,
            name: name.into(),
            effects,
            parameters: BTreeMap::new(),
            bindings: Vec::new(),
            trigger_bindings: Vec::new(),
            sound_cues: Vec::new(),
            group_settings: None,
            lights: Vec::new(),
            primitives: Vec::new(),
            meshes: Vec::new(),
            materials: Vec::new(),
            textures: Vec::new(),
            part_attributes: Vec::new(),
            entity_ids: Vec::new(),
            transforms: Vec::new(),
        }
    }
    /// Checks the asset schema, metadata and child configuration.
    /// # Errors
    /// Returns a diagnostic for incompatible versions or invalid child data.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(format!("unsupported effect asset version {}", self.version));
        }
        if self.name.is_empty() || self.name.len() > 256 {
            return Err("effect asset name must have 1..256 bytes".into());
        }
        if self.effects.is_empty() || self.effects.len() > 128 {
            return Err("effect asset requires 1..128 child effects".into());
        }
        for length in [
            self.entity_ids.len(),
            self.transforms.len(),
            self.lights.len(),
            self.primitives.len(),
            self.meshes.len(),
            self.materials.len(),
            self.textures.len(),
            self.part_attributes.len(),
        ] {
            if length != 0 && length != self.effects.len() {
                return Err("effect child metadata length must match the effects list".into());
            }
        }
        if self.entity_ids.iter().collect::<BTreeSet<_>>().len() != self.entity_ids.len() {
            return Err("effect asset child identities must be unique".into());
        }
        for matrix in &self.transforms {
            let matrix = Mat4::from_cols_array(matrix);
            let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
            let transform = LocalTransform {
                translation,
                rotation,
                scale,
            };
            if !matrix.is_finite()
                || !transform.is_finite()
                || (matrix - transform.matrix())
                    .to_cols_array()
                    .iter()
                    .any(|v| v.abs() > 0.0001)
            {
                return Err("effect child transform must be a finite nonsingular translation/rotation/scale matrix".into());
            }
        }
        if let Some(settings) = &self.group_settings {
            settings.validate()?;
        }
        if self.sound_cues.len() > 128 {
            return Err("effect assets support at most128 sound cues".into());
        }
        for cue in &self.sound_cues {
            if !cue.delay.is_finite()
                || cue.delay < 0.0
                || !cue.volume.is_finite()
                || !(0.0..=16.0).contains(&cue.volume)
                || !cue.pitch.is_finite()
                || !(0.01..=16.0).contains(&cue.pitch)
            {
                return Err("sound cue delay/volume/pitch are outside their finite ranges".into());
            }
        }
        self.validate_lights()?;
        for effect in &self.effects {
            effect.validate()?;
        }
        for primitive in self.primitives.iter().flatten() {
            primitive.validate().map_err(|error| error.to_string())?;
        }
        for textures in self.textures.iter().flatten() {
            textures.validate()?;
        }
        for attributes in self.part_attributes.iter().flatten() {
            if !attributes
                .color
                .iter()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
                || !attributes.friction.is_finite()
                || !(0.0..=10.0).contains(&attributes.friction)
            {
                return Err("effect child attributes require finite color and friction within their supported ranges".into());
            }
        }
        let timeline = super::EffectGroup::from_asset(None, self);
        if self
            .sound_cues
            .iter()
            .any(|cue| cue.delay >= timeline.duration)
        {
            return Err("sound cue delay must occur before the composite duration".into());
        }
        super::validate_group_bindings(&self.parameters, &self.bindings, &self.effects)?;
        super::validate_group_triggers(&self.trigger_bindings, self.effects.len())?;
        Ok(())
    }
    fn validate_lights(&self) -> Result<(), String> {
        for light in self.lights.iter().flatten() {
            if !light.color.is_finite()
                || light.color.min_element() < 0.0
                || !light.intensity.is_finite()
                || light.intensity < 0.0
                || !light.range.is_finite()
                || light.range <= 0.0
                || !light.spot_outer_angle_radians.is_finite()
                || !(0.0..std::f32::consts::PI).contains(&light.spot_outer_angle_radians)
            {
                return Err("effect asset light configuration is invalid".into());
            }
        }
        Ok(())
    }

    /// Encodes the validated versioned asset.
    /// # Errors
    /// Returns validation or RON encoding errors.
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map(String::into_bytes)
            .map_err(|error| error.to_string())
    }
    /// Reads and validates a bounded asset file.
    /// # Errors
    /// Returns size, schema, RON decoding or child validation errors.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 4 * 1024 * 1024 {
            return Err("effect asset exceeds 4 MiB".into());
        }
        let asset: Self = ron::de::from_bytes(bytes).map_err(|error| error.to_string())?;
        asset.validate()?;
        Ok(asset)
    }
    /// Returns independent authored instances: editing a child cannot mutate the
    /// library asset or another instance. Delays are relative to the group start.
    pub fn instantiate(&self) -> Vec<EffectComponent> {
        self.effects.clone()
    }
    /// Retains the existing asset system's stable source identity on every child.
    /// Embedded configuration is an instance override until explicitly restored.
    pub fn instantiate_from(&self, asset: crate::AssetId) -> Vec<EffectComponent> {
        self.effects
            .iter()
            .enumerate()
            .map(|(child, effect)| {
                let mut instance = effect.clone();
                instance.source = Some(EffectSource {
                    asset,
                    child: u32::try_from(child).unwrap_or(u32::MAX),
                    baseline: Some(Box::new(EffectDefaults::from_component(&instance))),
                });
                instance
            })
            .collect()
    }
    /// Allocates independent child identities and preserves authored relative transforms.
    /// Internal effect references and source baselines use the newly allocated IDs.
    /// Legacy assets without metadata instantiate at the group origin.
    pub fn instances_with_ids(&self, asset_id: Option<crate::AssetId>) -> Vec<EntitySnapshot> {
        let effects = asset_id.map_or_else(|| self.instantiate(), |id| self.instantiate_from(id));
        let ids: Vec<_> = effects.iter().map(|_| EntityId::new()).collect();
        let mapping: BTreeMap<_, _> = self
            .entity_ids
            .iter()
            .copied()
            .zip(ids.iter().copied())
            .collect();
        effects
            .into_iter()
            .enumerate()
            .map(|(index, mut effect)| {
                effect.remap_entities(&mapping);
                let baseline = EffectDefaults::from_component(&effect);
                if let Some(source) = &mut effect.source {
                    source.baseline = Some(Box::new(baseline));
                }
                let local_transform =
                    self.transforms
                        .get(index)
                        .map_or_else(LocalTransform::default, |matrix| {
                            let (scale, rotation, translation) =
                                Mat4::from_cols_array(matrix).to_scale_rotation_translation();
                            LocalTransform {
                                translation,
                                rotation,
                                scale,
                            }
                        });
                EntitySnapshot {
                    id: ids[index],
                    name: Some(format!("{} {}", self.name, index + 1)),
                    local_transform,
                    mesh: self
                        .meshes
                        .get(index)
                        .copied()
                        .flatten()
                        .map(|asset| crate::Mesh { asset }),
                    material: self
                        .materials
                        .get(index)
                        .copied()
                        .flatten()
                        .map(|asset| crate::Material { asset }),
                    textures: self.textures.get(index).cloned().flatten(),
                    light: self.lights.get(index).copied().flatten(),
                    primitive: self.primitives.get(index).cloned().flatten(),
                    effect: Some(effect),
                    part_attributes: self
                        .part_attributes
                        .get(index)
                        .copied()
                        .flatten()
                        .unwrap_or_default(),
                    ..EntitySnapshot::default()
                }
            })
            .collect()
    }
    /// Captures the ordinary components belonging to already ordered effect children.
    /// IDs, effect references and transforms remain under the caller's remapping policy.
    /// # Errors
    /// Rejects a child count that differs from the effect list.
    pub fn capture_child_components(&mut self, children: &[EntitySnapshot]) -> Result<(), String> {
        if children.len() != self.effects.len() {
            return Err("effect component metadata must match ordered children".into());
        }
        self.lights = compact(children.iter().map(|child| child.light).collect());
        self.primitives = compact(
            children
                .iter()
                .map(|child| child.primitive.clone())
                .collect(),
        );
        self.meshes = compact(
            children
                .iter()
                .map(|child| child.mesh.map(|mesh| mesh.asset))
                .collect(),
        );
        self.materials = compact(
            children
                .iter()
                .map(|child| child.material.map(|material| material.asset))
                .collect(),
        );
        self.textures = compact(
            children
                .iter()
                .map(|child| child.textures.clone())
                .collect(),
        );
        self.part_attributes = children
            .iter()
            .map(|child| Some(child.part_attributes))
            .collect();
        Ok(())
    }
    /// Restores one child resolved through authoritative `.rmeta` data.
    /// # Errors
    /// Returns a diagnostic when no source or matching child exists.
    pub fn restore(&self, instance: &mut EffectComponent) -> Result<(), String> {
        let mut source = instance
            .source
            .clone()
            .ok_or("effect instance has no source asset")?;
        let defaults = self
            .effects
            .get(source.child as usize)
            .ok_or("effect source child no longer exists")?;
        let mut restored = defaults.clone();
        source.baseline = Some(Box::new(EffectDefaults::from_component(defaults)));
        restored.source = Some(source);
        *instance = restored;
        Ok(())
    }
}
fn compact<T>(values: Vec<Option<T>>) -> Vec<Option<T>> {
    if values.iter().any(Option::is_some) {
        values
    } else {
        Vec::new()
    }
}

fn particle(
    name: &str,
    color: [f32; 4],
    rate: f32,
    lifetime: [f32; 2],
    speed: [f32; 2],
    size: [f32; 2],
) -> EffectAsset {
    let emitter = ParticleEmitter {
        rate,
        lifetime,
        speed,
        start_size: size,
        appearance: EffectAppearance {
            color: Gradient::constant(color),
            blend: BlendMode::Additive,
            ..Default::default()
        },
        ..Default::default()
    };
    EffectAsset::new(
        name,
        vec![EffectComponent::new(EffectKind::Particle(emitter))],
    )
}
fn trail(name: &str, color: [f32; 4], width: f32, lifetime: f32) -> EffectAsset {
    EffectAsset::new(
        name,
        vec![EffectComponent::new(EffectKind::Trail(TrailRenderer {
            width,
            lifetime,
            appearance: EffectAppearance {
                color: Gradient::constant(color),
                ..Default::default()
            },
            ..Default::default()
        }))],
    )
}
fn one(name: &str, kind: EffectKind) -> EffectAsset {
    EffectAsset::new(name, vec![EffectComponent::new(kind)])
}

/// Built-in presets contain authored configuration, never live simulation state
/// or external hotlinked resources. They can be serialized as `.effect` assets.
#[expect(
    clippy::too_many_lines,
    reason = "the preset library is one ordered table of authored configurations"
)]
pub fn presets() -> Vec<EffectAsset> {
    let mut assets = vec![
        particle(
            "Campfire",
            [1.0, 0.25, 0.025, 0.8],
            65.0,
            [0.5, 1.2],
            [0.8, 2.0],
            [0.12, 0.3],
        ),
        particle(
            "Fire sparks",
            [1.0, 0.55, 0.05, 1.0],
            25.0,
            [0.4, 1.5],
            [2.0, 5.0],
            [0.015, 0.04],
        ),
        particle(
            "Smoke plume",
            [0.25, 0.28, 0.32, 0.25],
            18.0,
            [3.0, 5.0],
            [0.2, 0.6],
            [0.3, 0.6],
        ),
        particle(
            "Muzzle flash",
            [1.0, 0.7, 0.15, 1.0],
            0.0,
            [0.03, 0.08],
            [2.0, 4.0],
            [0.1, 0.2],
        ),
        particle(
            "Embers",
            [1.0, 0.18, 0.02, 0.9],
            12.0,
            [2.0, 4.0],
            [0.4, 0.8],
            [0.02, 0.05],
        ),
        particle(
            "Firework burst",
            [0.4, 0.5, 1.0, 1.0],
            0.0,
            [1.0, 2.0],
            [4.0, 8.0],
            [0.03, 0.06],
        ),
        particle(
            "Magic projectile",
            [0.25, 0.1, 1.0, 1.0],
            60.0,
            [0.2, 0.5],
            [0.2, 0.5],
            [0.05, 0.12],
        ),
        one(
            "Lightning beam",
            EffectKind::Beam(BeamRenderer {
                width: 0.06,
                noise_amplitude: 0.35,
                appearance: EffectAppearance {
                    color: Gradient::constant([0.3, 0.5, 1.0, 1.0]),
                    blend: BlendMode::Additive,
                    ..Default::default()
                },
                ..Default::default()
            }),
        ),
        one(
            "Energy shield",
            EffectKind::Ribbon(RibbonRenderer {
                points: (0_u16..=48)
                    .map(|i| {
                        let t = f32::from(i) / 48.0 * std::f32::consts::TAU;
                        [t.cos() * 2.0, t.sin() * 2.0, 0.0]
                    })
                    .collect(),
                width: 0.08,
                appearance: EffectAppearance {
                    color: Gradient::constant([0.1, 0.6, 1.0, 0.8]),
                    ..Default::default()
                },
                ..Default::default()
            }),
        ),
        particle(
            "Healing aura",
            [0.15, 1.0, 0.3, 0.5],
            35.0,
            [1.0, 2.0],
            [0.2, 0.5],
            [0.04, 0.1],
        ),
        particle(
            "Teleport",
            [0.6, 0.1, 1.0, 0.8],
            0.0,
            [0.5, 1.0],
            [1.0, 4.0],
            [0.08, 0.18],
        ),
        one(
            "Portal",
            EffectKind::Ribbon(RibbonRenderer {
                points: (0_u16..=48)
                    .map(|i| {
                        let t = f32::from(i) / 48.0 * std::f32::consts::TAU;
                        [t.cos() * 1.4, t.sin() * 2.0, 0.0]
                    })
                    .collect(),
                width: 0.2,
                tube: true,
                ..Default::default()
            }),
        ),
        trail("Glowing trail", [0.2, 0.7, 1.0, 0.9], 0.18, 1.0),
        one("Shockwave", EffectKind::Shockwave(RingEffect::default())),
        particle(
            "Tire smoke",
            [0.6, 0.6, 0.65, 0.2],
            40.0,
            [0.5, 1.5],
            [0.2, 0.6],
            [0.15, 0.35],
        ),
        trail("Skid mark", [0.025, 0.025, 0.025, 0.9], 0.22, 15.0),
        particle(
            "Exhaust smoke",
            [0.3, 0.3, 0.35, 0.25],
            20.0,
            [0.8, 1.8],
            [0.5, 1.0],
            [0.08, 0.2],
        ),
        trail("Speed boost trail", [0.1, 0.4, 1.0, 1.0], 0.3, 0.5),
        particle(
            "Collision sparks",
            [1.0, 0.6, 0.15, 1.0],
            0.0,
            [0.2, 0.6],
            [2.0, 6.0],
            [0.02, 0.04],
        ),
        particle(
            "Rocket exhaust",
            [1.0, 0.35, 0.04, 0.9],
            100.0,
            [0.1, 0.4],
            [2.0, 5.0],
            [0.08, 0.2],
        ),
        one("Rain", EffectKind::Weather(WeatherVolume::default())),
        one(
            "Snow",
            EffectKind::Weather(WeatherVolume {
                weather: WeatherType::Snow,
                emitter: ParticleEmitter {
                    shape: SpawnShape::Box,
                    dimensions: [15.0, 10.0, 15.0],
                    direction: [0.0, -1.0, 0.0],
                    speed: [0.5, 1.2],
                    gravity: [0.0; 3],
                    rate: 80.0,
                    lifetime: [5.0, 8.0],
                    noise: 0.25,
                    ..Default::default()
                },
                ..Default::default()
            }),
        ),
        particle(
            "Dust",
            [0.6, 0.5, 0.3, 0.15],
            12.0,
            [3.0, 6.0],
            [0.05, 0.15],
            [0.08, 0.15],
        ),
        one(
            "Leaves",
            EffectKind::Weather(WeatherVolume {
                weather: WeatherType::Leaves,
                emitter: ParticleEmitter {
                    shape: SpawnShape::Box,
                    dimensions: [10.0, 8.0, 10.0],
                    direction: [0.0, -1.0, 0.0],
                    speed: [0.2, 0.6],
                    gravity: [0.0, -0.1, 0.0],
                    rate: 8.0,
                    lifetime: [5.0, 8.0],
                    angular_velocity: 2.0,
                    appearance: EffectAppearance {
                        color: Gradient::constant([0.6, 0.3, 0.05, 1.0]),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            }),
        ),
        particle(
            "Fireflies",
            [0.6, 1.0, 0.1, 0.7],
            8.0,
            [3.0, 5.0],
            [0.05, 0.2],
            [0.02, 0.05],
        ),
        one(
            "Mist",
            EffectKind::Fog(FogVolume {
                density: 0.05,
                dimensions: [10.0, 2.0, 10.0],
                ..Default::default()
            }),
        ),
        particle(
            "Water splash",
            [0.4, 0.7, 1.0, 0.6],
            0.0,
            [0.4, 1.0],
            [2.0, 4.0],
            [0.04, 0.09],
        ),
        one(
            "Ground ripple",
            EffectKind::WaterRipple(RingEffect::default()),
        ),
        trail("Sword trail", [0.7, 0.85, 1.0, 0.65], 0.6, 0.2),
        particle(
            "Footstep dust",
            [0.55, 0.45, 0.3, 0.25],
            0.0,
            [0.2, 0.6],
            [0.3, 0.8],
            [0.1, 0.2],
        ),
        particle(
            "Bullet impact",
            [1.0, 0.6, 0.2, 1.0],
            0.0,
            [0.1, 0.4],
            [1.0, 3.0],
            [0.02, 0.05],
        ),
        one(
            "Damage flash",
            EffectKind::ScreenFlash(ScreenFlash::default()),
        ),
        one(
            "Object dissolve",
            EffectKind::Dissolve(DissolveEffect::default()),
        ),
        one(
            "Character highlight",
            EffectKind::Outline(OutlineEffect::default()),
        ),
    ];
    for asset in &mut assets {
        if matches!(
            asset.name.as_str(),
            "Object dissolve" | "Character highlight"
        ) {
            let target =
                EntityId::from(uuid::Uuid::from_bytes(if asset.name == "Object dissolve" {
                    *b"rustic_dissolve1"
                } else {
                    *b"rustic_outline_1"
                }));
            asset.entity_ids = vec![target];
            asset.primitives = vec![Some(if asset.name == "Object dissolve" {
                crate::Primitive::Sphere {
                    radius: 0.7,
                    segments: 32,
                    rings: 24,
                }
            } else {
                crate::Primitive::Capsule {
                    radius: 0.35,
                    height: 1.8,
                    segments: 24,
                    rings: 12,
                }
            })];
            asset.transforms =
                vec![Mat4::from_translation(glam::Vec3::new(0.0, 1.0, 0.0)).to_cols_array()];
            asset.part_attributes = vec![Some(crate::PartAttributes {
                anchored: true,
                can_collide: false,
                color: [0.22, 0.42, 0.75, 1.0],
                ..Default::default()
            })];
            match &mut asset.effects[0].kind {
                EffectKind::Dissolve(effect) => effect.target = Some(target),
                EffectKind::Outline(effect) => effect.targets = vec![target],
                _ => unreachable!(),
            }
        }
        for effect in &mut asset.effects {
            if let EffectKind::Particle(p) = &mut effect.kind {
                if asset.name == "Fire sparks" {
                    p.simulation_mode = super::ParticleSimulationMode::Auto;
                    p.generate_events = false;
                    p.sorting = super::ParticleSortMode::None;
                    p.wind_influence = 0.0;
                }
                if p.rate == 0.0 {
                    p.bursts = vec![Burst {
                        count: if asset.name == "Firework burst" {
                            150
                        } else {
                            30
                        },
                        ..Default::default()
                    }];
                    effect.common.looping = false;
                    effect.common.duration = 2.0;
                }
                if asset.name.contains("smoke")
                    || asset.name == "Smoke plume"
                    || asset.name == "Dust"
                    || asset.name == "Footstep dust"
                {
                    p.appearance.blend = BlendMode::Alpha;
                    p.gravity = [0.0, 0.2, 0.0];
                }
                if matches!(
                    asset.name.as_str(),
                    "Firework burst" | "Collision sparks" | "Bullet impact" | "Water splash"
                ) {
                    p.shape = SpawnShape::Hemisphere;
                    p.gravity = [0.0, -9.81, 0.0];
                    p.collision = true;
                }
                if asset.name == "Fireflies" || asset.name == "Dust" {
                    p.shape = SpawnShape::Box;
                    p.dimensions = [8.0, 4.0, 8.0];
                    p.gravity = [0.0; 3];
                    p.noise = 0.3;
                }
            }
        }
    }
    let mut explosion = particle(
        "Explosion",
        [1.0, 0.3, 0.025, 1.0],
        0.0,
        [0.2, 0.8],
        [2.0, 6.0],
        [0.15, 0.45],
    );
    if let EffectKind::Particle(p) = &mut explosion.effects[0].kind {
        p.bursts = vec![Burst {
            count: 100,
            ..Default::default()
        }];
        p.shape = SpawnShape::Sphere;
    }
    explosion.effects[0].common.looping = false;
    explosion.effects[0].common.duration = 3.0;
    let mut smoke = assets
        .iter()
        .find(|a| a.name == "Smoke plume")
        .and_then(|asset| asset.effects.first())
        .cloned()
        .unwrap_or_default();
    smoke.common.delay = 0.15;
    smoke.common.duration = 2.0;
    smoke.common.looping = false;
    let mut ring = EffectComponent::new(EffectKind::Shockwave(RingEffect::default()));
    ring.common.duration = 0.6;
    ring.common.looping = false;
    let mut shake = EffectComponent::new(EffectKind::CameraShake(CameraShake::default()));
    shake.common.duration = 0.4;
    shake.common.looping = false;
    explosion.effects.extend([smoke, ring, shake]);
    let mut debris = EffectComponent::new(EffectKind::MeshParticle(ParticleEmitter {
        mesh: Some(explosion_mesh_asset()),
        max_particles: 48,
        rate: 0.0,
        bursts: vec![Burst {
            count: 24,
            ..Burst::default()
        }],
        speed: [2.0, 5.0],
        start_size: [0.05, 0.15],
        lifetime: [0.6, 1.4],
        shape: SpawnShape::Sphere,
        gravity: [0.0, -9.81, 0.0],
        collision: true,
        appearance: EffectAppearance {
            color: Gradient::constant([0.4, 0.2, 0.07, 1.0]),
            emission: 0.0,
            ..EffectAppearance::default()
        },
        ..ParticleEmitter::default()
    }));
    debris.common.looping = false;
    debris.common.duration = 1.5;
    explosion.effects.push(debris);
    explosion.entity_ids = explosion.effects.iter().map(|_| EntityId::new()).collect();
    let light_id = EntityId::new();
    let mut flash = EffectComponent::new(EffectKind::LightPulse(LightPulse {
        target: Some(light_id),
        intensity: Curve {
            keys: vec![
                super::CurveKey {
                    time: 0.0,
                    value: 12.0,
                },
                super::CurveKey {
                    time: 0.15,
                    value: 8.0,
                },
                super::CurveKey {
                    time: 1.0,
                    value: 0.0,
                },
            ],
        },
        color: Gradient::constant([1.0, 0.5, 0.12, 1.0]),
        ..LightPulse::default()
    }));
    flash.common.looping = false;
    flash.common.duration = 0.35;
    explosion.effects.push(flash);
    explosion.entity_ids.push(light_id);
    explosion.lights = vec![None; explosion.effects.len()];
    if let Some(light) = explosion.lights.last_mut() {
        *light = Some(Light {
            kind: LightKind::Point,
            color: glam::Vec3::new(1.0, 0.5, 0.12),
            intensity: 0.0,
            range: 8.0,
            ..Light::default()
        });
    }
    explosion.sound_cues.push(EffectSoundCue {
        asset: explosion_sound_asset(),
        delay: 0.0,
        volume: 0.75,
        pitch: 1.0,
        spatial: true,
        looping: false,
    });
    explosion
        .parameters
        .insert("power".into(), super::EffectParameter::Scalar(1.0));
    explosion.bindings = [
        (0, "kind.Particle.appearance.emission", 1.0),
        (1, "kind.Particle.rate", 18.0),
        (2, "kind.Shockwave.final_radius", 4.0),
        (4, "kind.MeshParticle.appearance.emission", 0.15),
        (5, "kind.LightPulse.intensity.keys.0.value", 12.0),
        (5, "kind.LightPulse.intensity.keys.1.value", 8.0),
    ]
    .into_iter()
    .map(|(child, path, scale)| super::GroupParameterBinding {
        parameter: "power".into(),
        child,
        property_path: path.into(),
        scale,
        offset: if child == 2 { 0.1 } else { 0.0 },
    })
    .collect();
    explosion.trigger_bindings.push(super::GroupTriggerBinding {
        source_child: 5,
        event: super::CompositeTriggerEvent::Completed,
        target_child: 3,
        action: super::CompositeTriggerAction::Restart,
    });
    assets.push(explosion);
    assets
}

#[cfg(test)]
mod tests {
    use super::{EffectAsset, EffectSoundCue, explosion_sound_asset, presets};
    use crate::EntityId;
    use crate::effects::{BeamRenderer, EffectComponent, EffectKind, ParticleEmitter};
    use glam::Mat4;
    #[test]
    fn composite_child_references_and_relative_transforms_survive_instances() {
        let first = EntityId::new();
        let second = EntityId::new();
        let mut asset = EffectAsset::new(
            "linked beam",
            vec![
                EffectComponent::new(EffectKind::Particle(ParticleEmitter::default())),
                EffectComponent::new(EffectKind::Beam(BeamRenderer {
                    end_target: Some(first),
                    ..BeamRenderer::default()
                })),
            ],
        );
        asset.entity_ids = vec![first, second];
        asset.transforms = vec![
            Mat4::from_translation(glam::Vec3::X).to_cols_array(),
            Mat4::from_translation(glam::Vec3::Y * 2.0).to_cols_array(),
        ];
        let asset = EffectAsset::from_bytes(&asset.to_bytes().unwrap()).unwrap();
        let instances = asset.instances_with_ids(Some(crate::AssetId::new()));
        let other = asset.instances_with_ids(None);
        assert_ne!(instances[0].id, other[0].id);
        assert_eq!(instances[0].local_transform.translation, glam::Vec3::X);
        assert_eq!(
            instances[1].local_transform.translation,
            glam::Vec3::Y * 2.0
        );
        let effect = instances[1].effect.as_ref().unwrap();
        let EffectKind::Beam(beam) = &effect.kind else {
            panic!("beam")
        };
        assert_eq!(beam.end_target, Some(instances[0].id));
        assert_eq!(
            effect
                .source
                .as_ref()
                .unwrap()
                .baseline
                .as_ref()
                .unwrap()
                .kind,
            effect.kind
        );
        let mut invalid = asset;
        invalid.entity_ids.pop();
        assert!(invalid.validate().is_err());
    }
    #[test]
    fn instance_restores_source_child_without_losing_stable_identity() {
        let asset = presets().pop().unwrap();
        let id = crate::AssetId::new();
        let mut instances = asset.instantiate_from(id);
        let source = instances[1].source.clone();
        instances[1].common.duration = 99.0;
        asset.restore(&mut instances[1]).unwrap();
        assert_eq!(instances[1].common, asset.effects[1].common);
        assert_eq!(instances[1].source, source);
        assert_eq!(instances[1].source.as_ref().unwrap().asset, id);
        instances[1].source.as_mut().unwrap().child = 999;
        assert!(asset.restore(&mut instances[1]).is_err());
    }
    #[test]
    fn library_assets_validate_and_round_trip() {
        let library = presets();
        assert_eq!(library.len(), 35);
        for asset in library {
            assert_eq!(
                EffectAsset::from_bytes(&asset.to_bytes().unwrap()).unwrap(),
                asset
            );
        }
    }
    #[test]
    fn composite_instances_are_independent_and_reject_future_versions() {
        let asset = presets().pop().unwrap();
        let mut instance = asset.instantiate();
        instance[0].common.duration = 1.0;
        assert_ne!(instance, asset.effects);
        let mut invalid = asset;
        invalid.version = 2;
        assert!(invalid.to_bytes().is_err());
    }
    #[test]
    fn explicit_group_duration_rejects_audio_outside_timeline_before_registration() {
        let mut asset = EffectAsset::new("invalid timeline", vec![EffectComponent::default()]);
        asset.group_settings = Some(super::super::EffectGroupSettings {
            duration: 1.0,
            ..Default::default()
        });
        asset.sound_cues.push(EffectSoundCue {
            asset: explosion_sound_asset(),
            delay: 2.0,
            volume: 1.0,
            pitch: 1.0,
            spatial: false,
            looping: false,
        });
        assert!(
            asset
                .validate()
                .unwrap_err()
                .contains("before the composite duration")
        );
        let mut world = crate::SceneWorld::new();
        let id = crate::AssetId::new();
        assert!(world.register_effect_asset(id, asset.clone()).is_err());
        assert!(!world.has_effect_asset(id));
        asset.sound_cues[0].delay = 0.5;
        assert!(asset.validate().is_ok());
        world.register_effect_asset(id, asset).unwrap();
        assert!(world.has_effect_asset(id));
    }
}
