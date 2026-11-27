//! Generate an editable Rustic project using the public project/scene APIs.
//! `cargo run -p engine-project --example scene_effects_showcase -- /tmp/effects-showcase`
use engine_project::{Project, ProjectTemplate, VirtualDirectory};
use engine_scripting::{
    ScriptApiVersion, ScriptId, ScriptLanguage, ScriptManifest, ScriptManifestEntry,
    save_manifest_atomic,
};
use engine_world::effects::{
    BlendMode, Curve, EffectAppearance, EffectComponent, EffectKind, ForceType, Gradient,
    GradientKey, ParticleEmitter, SpawnShape, presets,
};
use engine_world::{
    Camera, EntitySnapshot, Light, LightKind, LocalTransform, PartAttributes, Primitive,
    SceneDocument, SurfaceTextures, TextureMap, save_scene_atomic,
};
use glam::{Quat, Vec3};
use std::error::Error;
use std::path::PathBuf;

fn appearance(color: [f32; 4], emission: f32, blend: BlendMode) -> EffectAppearance {
    EffectAppearance {
        color: Gradient::constant(color),
        opacity: Curve::linear(1.0, 0.0),
        size: Curve::linear(1.0, 0.25),
        emission,
        blend,
        ..EffectAppearance::default()
    }
}
fn transform(position: Vec3) -> LocalTransform {
    LocalTransform {
        translation: position,
        ..Default::default()
    }
}
fn pedestal(name: String, position: Vec3) -> EntitySnapshot {
    EntitySnapshot {
        name: Some(name),
        local_transform: transform(position - Vec3::Y * 0.15),
        primitive: Some(Primitive::Cylinder {
            radius: 1.6,
            height: 0.2,
            segments: 48,
        }),
        part_attributes: PartAttributes {
            color: [0.16, 0.2, 0.28, 1.0],
            anchored: true,
            ..Default::default()
        },
        ..Default::default()
    }
}
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "RGBE encoding intentionally quantizes bounded positive generated color channels to bytes"
)]
fn panorama_bytes() -> Vec<u8> {
    let mut hdr = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y 64 +X 128\n".to_vec();
    for y in 0_u8..64 {
        for x in 0_u8..128 {
            let horizon = 1.0 - ((f32::from(y) / 63.0 - 0.5) * 2.0).abs();
            let sun = if (i32::from(x) - 38).abs() < 4 && (i32::from(y) - 22).abs() < 4 {
                1.0
            } else {
                0.0
            };
            let rgb = [
                0.08 + 0.2 * horizon + 3.0 * sun,
                0.12 + 0.3 * horizon + 2.4 * sun,
                0.2 + 0.5 * horizon + 1.5 * sun,
            ];
            let maximum = rgb.into_iter().fold(0.0_f32, f32::max);
            let exponent = maximum.log2().floor() as i32 + 1;
            let scale = 256.0 / 2.0f32.powi(exponent);
            hdr.extend([
                (rgb[0] * scale) as u8,
                (rgb[1] * scale) as u8,
                (rgb[2] * scale) as u8,
                (exponent + 128) as u8,
            ]);
        }
    }
    hdr
}
#[expect(
    clippy::too_many_lines,
    reason = "this example keeps the 26 authored showcase configurations and their source references together"
)]
fn main() -> Result<(), Box<dyn Error>> {
    let destination = std::env::args_os().nth(1).map_or_else(
        || PathBuf::from("/tmp/rustic-effects-showcase"),
        PathBuf::from,
    );
    let mut project = Project::create(
        &destination,
        "Scene Effects Showcase",
        ProjectTemplate::Blank,
    )?;
    let mut scene = SceneDocument::new("Scene Effects Showcase");
    scene.environment = engine_world::SceneEnvironment {
        enabled: true,
        sky_color: [0.055, 0.08, 0.14],
        ambient_color: [0.8, 0.88, 1.0],
        ambient_intensity: 0.48,
        sun_color: [1.0, 0.88, 0.72],
        sun_intensity: 0.8,
        sun_direction: [0.3, 0.9, -0.2],
        ..Default::default()
    };
    // A locally generated equirectangular HDR panorama exercises baked probes.
    let panorama = destination.join("assets/showcase-probe.hdr");
    let hdr = panorama_bytes();
    std::fs::write(&panorama, hdr)?;
    let debris = destination.join("assets/debris.obj");
    std::fs::write(
        &debris,
        "v 0 0.7 0\nv -0.4 0 -0.3\nv 0.4 0 -0.3\nv 0 0 0.5\nf 1 3 2\nf 1 4 3\nf 1 2 4\nf 2 3 4\n",
    )?;
    let debris_meta = engine_assets::AssetMeta::new("rustic.obj", 1);
    engine_assets::save_meta(&engine_assets::sidecar_path(&debris), &debris_meta)?;
    let leaf = destination.join("assets/foliage.obj");
    std::fs::write(
        &leaf,
        "v 0 0 0\nv -0.35 0.5 0\nv 0 1.5 0\nv 0.35 0.5 0\nvt 0.5 0\nvt 0 0.5\nvt 0.5 1\nvt 1 0.5\nf 1/1 2/2 3/3\nf 1/1 3/3 4/4\nf 3/3 2/2 1/1\nf 4/4 3/3 1/1\n",
    )?;
    let leaf_meta = engine_assets::AssetMeta::new("rustic.obj", 1);
    engine_assets::save_meta(&engine_assets::sidecar_path(&leaf), &leaf_meta)?;
    let probe_meta = engine_assets::AssetMeta::new("rustic.image", 1);
    engine_assets::save_meta(&engine_assets::sidecar_path(&panorama), &probe_meta)?;

    let mut image = vec![0_u8; 18];
    image[2] = 2;
    image[12] = 32;
    image[14] = 32;
    image[16] = 24;
    image[17] = 0x20;
    for y in 0..32 {
        for x in 0..32 {
            let bright = x % 8 == 0 || y % 8 == 0;
            image.extend(if bright { [255, 180, 35] } else { [45, 20, 8] });
        }
    }
    let energy_path = destination.join("assets/energy-grid.tga");
    std::fs::write(&energy_path, image)?;
    engine_assets::save_meta(
        &engine_assets::sidecar_path(&energy_path),
        &engine_assets::AssetMeta::new("rustic.image", 1),
    )?;
    let orbit_script = ScriptId::new();
    std::fs::write(
        destination.join("scripts/orbit.lua"),
        r"local origin, elapsed
return {
 Start = function() local x,y,z=rustic.get_translation();origin={x,y,z};elapsed=0 end,
 FixedUpdate = function(dt)
  elapsed=elapsed+dt
  rustic.set_translation(origin[1]+math.cos(elapsed*1.8)*1.1,origin[2]+1+math.sin(elapsed*2.0)*0.2,origin[3]+math.sin(elapsed*1.8)*1.1)
 end,
}
",
    )?;
    save_manifest_atomic(
        &destination.join("config/scripts.ron"),
        &ScriptManifest {
            format_version: engine_scripting::CURRENT_SCRIPT_MANIFEST_VERSION,
            scripts: vec![ScriptManifestEntry {
                id: orbit_script,
                language: ScriptLanguage::Lua54,
                relative_path: "scripts/orbit.lua".into(),
                api_version: ScriptApiVersion::CURRENT,
                public_properties: vec![],
            }],
        },
    )?;
    scene.entities.push(EntitySnapshot {
        name: Some("Showcase Camera".into()),
        local_transform: LocalTransform {
            translation: Vec3::new(11.0, 14.0, -17.0),
            rotation: Quat::from_rotation_x(0.5),
            ..Default::default()
        },
        camera: Some(Camera::default()),
        ..Default::default()
    });
    scene.entities.push(EntitySnapshot {
        name: Some("Key light".into()),
        local_transform: LocalTransform {
            rotation: Quat::from_euler(glam::EulerRot::XYZ, -0.7, -0.4, 0.0),
            ..Default::default()
        },
        light: Some(Light {
            kind: LightKind::Directional,
            color: Vec3::new(1.0, 0.9, 0.75),
            intensity: 1.4,
            ..Default::default()
        }),
        ..Default::default()
    });
    scene.entities.push(EntitySnapshot {
        name: Some("Showcase floor".into()),
        local_transform: transform(Vec3::new(11.0, -0.3, 8.8)),
        primitive: Some(Primitive::Plane { size: [30.0, 29.0] }),
        part_attributes: PartAttributes {
            color: [0.2, 0.23, 0.3, 1.0],
            anchored: true,
            ..Default::default()
        },
        ..Default::default()
    });
    for (index, mut kind) in EffectKind::all_defaults().into_iter().enumerate() {
        let position = Vec3::new(
            f32::from(u16::try_from(index % 6).unwrap_or_default()) * 4.4,
            0.0,
            f32::from(u16::try_from(index / 6).unwrap_or_default()) * 4.4,
        );
        let name = format!("{:02} {}", index + 1, kind.name());
        scene
            .entities
            .push(pedestal(format!("{name} pedestal"), position));
        let needs_target = matches!(
            kind,
            EffectKind::Dissolve(_)
                | EffectKind::AnimatedSurface(_)
                | EffectKind::Outline(_)
                | EffectKind::ReflectionProbe(_)
        );
        let target_shape = match &kind {
            EffectKind::Dissolve(_) => Primitive::Cube { size: 1.3 },
            EffectKind::AnimatedSurface(_) => Primitive::RectangularPrism {
                size: [2.2, 0.25, 2.2],
            },
            EffectKind::Outline(_) => Primitive::Capsule {
                radius: 0.45,
                height: 1.0,
                segments: 24,
                rings: 12,
            },
            _ => Primitive::Sphere {
                radius: 0.8,
                segments: 32,
                rings: 24,
            },
        };
        let target = EntitySnapshot {
            name: Some(format!("{name} target")),
            local_transform: transform(position + Vec3::Y),
            textures: matches!(kind, EffectKind::AnimatedSurface(_)).then(|| SurfaceTextures {
                maps: [(TextureMap::BaseColor, "assets/energy-grid.tga".into())]
                    .into_iter()
                    .collect(),
                ..Default::default()
            }),
            primitive: Some(target_shape),
            part_attributes: PartAttributes {
                color: [0.2, 0.45, 0.65, 1.0],
                anchored: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let target_id = target.id;
        let needs_light = matches!(
            kind,
            EffectKind::LightPulse(_) | EffectKind::LightShaft(_) | EffectKind::LensFlare(_)
        );
        let light = EntitySnapshot {
            name: Some(format!("{name} point light")),
            local_transform: transform(position + Vec3::Y * 2.0),
            light: Some(Light {
                kind: LightKind::Point,
                intensity: 3.0,
                range: 4.0,
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut light = light;
        light.primitive = Some(Primitive::Sphere {
            radius: 0.16,
            segments: 16,
            rings: 10,
        });
        light.part_attributes = PartAttributes {
            color: [1.0, 0.76, 0.32, 1.0],
            anchored: true,
            ..Default::default()
        };
        let light_id = light.id;
        match &mut kind {
            EffectKind::Particle(v) => {
                v.rate = 5000.0;
                v.max_particles = 10_000;
                v.simulation_mode = engine_world::effects::ParticleSimulationMode::Auto;
                v.generate_events = false;
                v.sorting = engine_world::effects::ParticleSortMode::None;
                v.wind_influence = 0.0;
                v.dimensions = [0.4; 3];
                v.lifetime = [0.5, 1.3];
                v.speed = [1.2, 2.8];
                v.start_size = [0.06, 0.16];
                v.gravity = [0.0, -0.4, 0.0];
                v.noise = 0.0;
                v.appearance = appearance([1.0, 0.32, 0.035, 0.9], 1.5, BlendMode::Additive);
            }
            EffectKind::Trail(v) => {
                v.width = 0.16;
                v.lifetime = 1.2;
                v.appearance = appearance([0.1, 0.8, 1.0, 0.9], 1.4, BlendMode::Additive);
            }
            EffectKind::Shockwave(v) => {
                v.final_radius = 1.55;
                v.thickness = 0.12;
                v.appearance = appearance([0.1, 1.0, 0.65, 0.85], 1.2, BlendMode::Additive);
            }
            EffectKind::WaterRipple(v) => {
                v.final_radius = 1.4;
                v.thickness = 0.2;
                v.wavelength = 0.45;
                v.amplitude = 0.09;
                v.appearance = appearance([0.08, 0.55, 0.7, 0.8], 0.3, BlendMode::Alpha);
            }
            EffectKind::Ribbon(v) => {
                v.points = vec![
                    [-1.2, 0.4, 0.0],
                    [-0.6, 1.8, 0.2],
                    [0.0, 0.6, 0.0],
                    [0.6, 1.8, -0.2],
                    [1.2, 0.4, 0.0],
                ];
                v.width = 0.1;
                v.tube = true;
                v.appearance = appearance([0.6, 0.18, 1.0, 1.0], 1.1, BlendMode::Additive);
            }
            EffectKind::ProjectileTracer(v) => {
                v.velocity = [0.35, 0.1, 0.0];
                v.length = 1.2;
                v.width = 0.065;
                v.appearance = appearance([1.0, 0.6, 0.08, 1.0], 1.8, BlendMode::Additive);
            }
            EffectKind::Decal(v) => {
                v.dimensions = [2.0, 1.2, 2.0];
                v.appearance = appearance([0.12, 0.65, 0.95, 0.8], 0.35, BlendMode::Alpha);
            }
            EffectKind::Beam(v) => {
                v.end = [1.3, 2.2, 0.0];
                v.width = 0.05;
                v.appearance.blend = BlendMode::Additive;
                v.appearance.emission = 1.3;
                v.appearance.color = Gradient::constant([0.5, 0.25, 1.0, 1.0]);
            }
            EffectKind::ForceField(v) => {
                v.dimensions = [3.0; 3];
                v.layers = 1 << 28;
                v.force_type = ForceType::Vortex;
                v.direction = [0.0, 1.0, 0.0];
                v.strength = 3.0;
            }
            EffectKind::Wind(v) => {
                v.dimensions = [3.0; 3];
                v.layers = 1 << 29;
                v.direction = [1.0, 0.0, 0.2];
                v.strength = 1.4;
                v.turbulence = 0.25;
            }
            EffectKind::CollisionVolume(v) => {
                v.dimensions = [3.0; 3];
                v.layers = 1 << 30;
                v.bounce = 0.8;
                v.friction = 0.12;
            }
            EffectKind::Weather(v) => {
                v.emitter.dimensions = [3.0, 4.0, 3.0];
                v.emitter.max_particles = 300;
                v.emitter.start_size = [0.015, 0.03];
                v.emitter.stretch = 0.025;
                v.emitter.rate = 90.0;
                v.emitter.appearance = appearance([0.35, 0.65, 1.0, 0.6], 0.5, BlendMode::Alpha);
            }
            EffectKind::Fog(v) => {
                v.dimensions = [3.0, 2.0, 3.0];
                v.density = 0.11;
                v.color = [0.24, 0.38, 0.58];
            }
            EffectKind::Dissolve(v) => v.target = Some(target_id),
            EffectKind::AnimatedSurface(v) => {
                v.target = Some(target_id);
                v.color = Gradient {
                    keys: vec![
                        GradientKey {
                            time: 0.0,
                            color: [0.1, 0.4, 1.0, 1.0],
                        },
                        GradientKey {
                            time: 1.0,
                            color: [1.0, 0.2, 0.5, 1.0],
                        },
                    ],
                };
            }
            EffectKind::LightPulse(v) => v.target = Some(light_id),
            EffectKind::LightShaft(v) => v.light = Some(light_id),
            EffectKind::LensFlare(v) => v.light = Some(light_id),
            EffectKind::Outline(v) => v.targets = vec![target_id],
            EffectKind::PostProcess(v) => {
                v.global = true;
                v.weight = 0.18;
                v.bloom = 0.12;
                v.dimensions = [3.0; 3];
                v.vignette = 0.2;
                v.saturation = 1.1;
            }
            EffectKind::MeshParticle(v) => {
                v.mesh = Some(debris_meta.asset_id);
                v.start_size = [0.15, 0.35];
                v.speed = [2.0, 4.0];
                v.gravity = [0.0, -5.0, 0.0];
                v.rate = 12.0;
                v.collision = true;
                v.appearance = appearance([1.0, 0.55, 0.16, 1.0], 0.4, BlendMode::Alpha);
            }
            EffectKind::FoliageScatter(v) => {
                v.meshes = vec![leaf_meta.asset_id];
                v.dimensions = [3.0, 1.0, 3.0];
                v.max_instances = 40;
                v.appearance.color = Gradient::constant([0.2, 0.6, 0.12, 1.0]);
            }
            EffectKind::ReflectionProbe(v) => {
                v.cubemap = Some(probe_meta.asset_id);
                v.dimensions = [5.0; 3];
                v.parallax_correction = true;
            }
            EffectKind::HeatHaze(v) => {
                v.radius = 80.0;
                v.strength = 0.002;
            }
            EffectKind::ScreenFlash(v) => v.opacity = 0.015,
            EffectKind::CameraShake(v) => {
                v.position_amplitude = [0.015; 3];
                v.rotation_amplitude = [0.002; 3];
                v.radius = 0.0;
            }
        }
        if needs_target {
            scene.entities.push(target);
        }
        if needs_light {
            scene.entities.push(light);
        }
        let mut component = EffectComponent::new(kind);
        component.common.visibility_layers = 1;
        if matches!(
            component.kind,
            EffectKind::ScreenFlash(_) | EffectKind::CameraShake(_)
        ) {
            component.common.looping = false;
            component.common.duration = 1.0;
        }
        let companion = match &component.kind {
            EffectKind::ForceField(_) => Some((
                1_u32 << 28,
                [1.0, 0.2, 0.75, 0.9],
                "Vortex particles",
                false,
                Vec3::Y * 0.5,
            )),
            EffectKind::Wind(_) => Some((
                1_u32 << 29,
                [0.5, 1.0, 0.3, 0.9],
                "Wind particles",
                false,
                Vec3::Y * 0.7,
            )),
            EffectKind::CollisionVolume(_) => Some((
                1_u32 << 30,
                [1.0, 0.65, 0.12, 1.0],
                "Collision particles",
                true,
                Vec3::Y * 2.2,
            )),
            _ => None,
        };
        if let Some((mask, color, label, collision, offset)) = companion {
            let mut companion = EffectComponent::new(EffectKind::Particle(ParticleEmitter {
                max_particles: 100,
                rate: 30.0,
                shape: SpawnShape::Sphere,
                dimensions: [0.35; 3],
                lifetime: [1.5, 2.0],
                speed: if collision { [0.1, 0.2] } else { [0.05, 0.15] },
                direction: if collision {
                    [0.0, -1.0, 0.0]
                } else {
                    [0.0, 1.0, 0.0]
                },
                start_size: [0.025, 0.06],
                gravity: if collision {
                    [0.0, -4.0, 0.0]
                } else {
                    [0.0; 3]
                },
                drag: 0.05,
                wind_influence: 1.0,
                appearance: appearance(color, 1.0, BlendMode::Additive),
                collision,
                collision_layers: mask,
                bounce: 0.8,
                ..ParticleEmitter::default()
            }));
            companion.common.visibility_layers = mask;
            companion.common.random_seed = 500 + u64::try_from(index).unwrap_or_default();
            scene.entities.push(EntitySnapshot {
                name: Some(label.into()),
                local_transform: transform(position + offset),
                effect: Some(companion),
                ..EntitySnapshot::default()
            });
        }
        component.common.random_seed = 100 + u64::try_from(index).unwrap_or_default();
        let moving = matches!(
            component.kind,
            EffectKind::Trail(_) | EffectKind::WaterRipple(_)
        );
        let effect = EntitySnapshot {
            name: Some(name),
            local_transform: transform(position),
            effect: Some(component),
            scripts: if moving {
                vec![engine_world::ScriptComponent::new(orbit_script)]
            } else {
                vec![]
            },
            ..Default::default()
        };
        scene.entities.push(effect);
    }
    let controller_id = ScriptId::new();
    let ids = scene
        .entities
        .iter()
        .filter(|entity| entity.effect.is_some())
        .map(|entity| format!("'{}'", entity.id))
        .collect::<Vec<_>>()
        .join(",");
    let controller = format!(
        "local ids={{{ids}}}\nlocal was_held=false\nreturn {{FixedUpdate=function() local held=rustic.key('ArrowUp').held;if held and not was_held then for _,id in ipairs(ids) do Effects.restart(id) end end;was_held=held end}}\n"
    );
    std::fs::write(destination.join("scripts/replay.lua"), &controller)?;
    let mut manifest =
        engine_scripting::load_manifest(&std::fs::read(destination.join("config/scripts.ron"))?)?
            .manifest;
    manifest.scripts.push(ScriptManifestEntry {
        id: controller_id,
        language: ScriptLanguage::Lua54,
        relative_path: "scripts/replay.lua".into(),
        api_version: ScriptApiVersion::CURRENT,
        public_properties: vec![],
    });
    save_manifest_atomic(&destination.join("config/scripts.ron"), &manifest)?;
    scene.entities.push(EntitySnapshot {
        name: Some("Replay all effects — ArrowUp".into()),
        scripts: vec![engine_world::ScriptComponent::new(controller_id)],
        ..EntitySnapshot::default()
    });
    for relative in ["scripts/orbit.lua", "scripts/replay.lua"] {
        engine_scripting::validate_lua(
            &std::fs::read(destination.join(relative))?,
            relative,
            1024 * 1024,
        )?;
    }
    let asset_directory = project.directory(VirtualDirectory::Assets)?.join("effects");
    std::fs::create_dir_all(&asset_directory)?;
    engine_assets::install_builtin_effect_resources(&destination)?;
    for asset in presets() {
        let slug = asset.name.to_lowercase().replace([' ', '/'], "-");
        let source = asset_directory.join(format!("{slug}.effect"));
        std::fs::write(&source, asset.to_bytes()?)?;
        let meta = engine_assets::AssetMeta::new("rustic.effect", 1);
        engine_assets::save_meta(&engine_assets::sidecar_path(&source), &meta)?;
    }
    // Validate generated geometry and HDR through the real importer contracts.
    for path in [&panorama, &debris, &leaf] {
        engine_assets::ImporterRegistry::import(&engine_assets::ImportRequest::new(
            path.strip_prefix(&destination)?,
            std::fs::read(path)?,
        ))?;
    }
    let scene_path = project
        .directory(VirtualDirectory::Scenes)?
        .join("showcase.scene");
    save_scene_atomic(&scene_path, &scene)?;
    project.set_start_scene("showcase.scene")?;
    let loaded = engine_world::load_scene(&std::fs::read(&scene_path)?)?;
    let loaded_effects: Vec<_> = loaded
        .document
        .entities
        .iter()
        .filter_map(|entity| entity.effect.as_ref())
        .collect();
    assert!(loaded_effects.len() >= 26);
    assert_eq!(
        loaded_effects
            .iter()
            .map(|effect| effect.kind.name())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        26
    );
    assert_eq!(presets().len(), 35);
    println!(
        "Generated {} with 29 effect components covering26 types and 35 editable preset assets",
        destination.display()
    );
    Ok(())
}
