//! Editable, versioned effect assets. A composite is an ordered set of delayed
//! effects; every child has the same configuration as an ordinary scene effect.
use super::{
    BeamRenderer, BlendMode, Burst, CameraShake, DissolveEffect, EffectAppearance, EffectComponent,
    EffectDefaults, EffectKind, EffectSource, FogVolume, Gradient, OutlineEffect, ParticleEmitter,
    RibbonRenderer, RingEffect, ScreenFlash, SpawnShape, TrailRenderer, WeatherType, WeatherVolume,
};
use crate::{EntityId, EntitySnapshot, LocalTransform};
use glam::Mat4;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectAsset {
    pub version: u32,
    pub name: String,
    pub effects: Vec<EffectComponent>,
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
        for length in [self.entity_ids.len(), self.transforms.len()] {
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
        for effect in &self.effects {
            effect.validate()?;
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
                    effect: Some(effect),
                    ..EntitySnapshot::default()
                }
            })
            .collect()
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
        for effect in &mut asset.effects {
            if let EffectKind::Particle(p) = &mut effect.kind {
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
    assets.push(explosion);
    assets
}

#[cfg(test)]
mod tests {
    use super::{EffectAsset, presets};
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
}
