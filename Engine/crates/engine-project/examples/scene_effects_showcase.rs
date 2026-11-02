//! Generate an editable Rustic project using the public project/scene APIs.
//! cargo run -p engine-project --example scene_effects_showcase -- /tmp/effects-showcase
use engine_project::{Project, ProjectTemplate, VirtualDirectory};
use engine_scripting::{
    ScriptApiVersion, ScriptId, ScriptLanguage, ScriptManifest, ScriptManifestEntry,
    save_manifest_atomic,
};
use engine_world::effects::{EffectComponent, EffectKind, Gradient, GradientKey, presets};
use engine_world::{
    Camera, EntitySnapshot, Light, LightKind, LocalTransform, PartAttributes, Primitive,
    SceneDocument, save_scene_atomic,
};
use glam::{Quat, Vec3};
use std::error::Error;
use std::path::PathBuf;

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
            radius: 2.0,
            height: 0.2,
            segments: 48,
        }),
        part_attributes: PartAttributes {
            color: [0.07, 0.09, 0.13, 1.0],
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
    let destination = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/rustic-effects-showcase"));
    let mut project = Project::create(
        &destination,
        "Scene Effects Showcase",
        ProjectTemplate::Blank,
    )?;
    let mut scene = SceneDocument::new("Scene Effects Showcase");
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

    let orbit_script = ScriptId::new();
    std::fs::write(
        destination.join("scripts/orbit.lua"),
        r#"local origin, elapsed
return {
 Start = function() local x,y,z=rustic.get_translation();origin={x,y,z};elapsed=0 end,
 FixedUpdate = function(dt)
  elapsed=elapsed+dt
  rustic.set_translation(origin[1]+math.cos(elapsed*1.8)*1.1,origin[2]+1+math.sin(elapsed*2.0)*0.2,origin[3]+math.sin(elapsed*1.8)*1.1)
 end,
}
"#,
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
            translation: Vec3::new(12.5, 18.0, -25.0),
            rotation: Quat::from_rotation_x(0.38),
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
            intensity: 2.0,
            ..Default::default()
        }),
        ..Default::default()
    });
    scene.entities.push(EntitySnapshot {
        name: Some("Showcase floor".into()),
        local_transform: transform(Vec3::new(12.5, -0.3, 10.0)),
        primitive: Some(Primitive::Plane { size: [45.0, 40.0] }),
        part_attributes: PartAttributes {
            color: [0.12, 0.14, 0.18, 1.0],
            anchored: true,
            ..Default::default()
        },
        ..Default::default()
    });
    for (index, mut kind) in EffectKind::all_defaults().into_iter().enumerate() {
        let position = Vec3::new(
            f32::from(u16::try_from(index % 6).unwrap_or_default()) * 5.0,
            0.0,
            f32::from(u16::try_from(index / 6).unwrap_or_default()) * 5.0,
        );
        let name = format!("{:02} {}", index + 1, kind.name());
        scene
            .entities
            .push(pedestal(format!("{name} pedestal"), position));
        let target = EntitySnapshot {
            name: Some(format!("{name} target")),
            local_transform: transform(position + Vec3::Y),
            primitive: Some(Primitive::Sphere {
                radius: 0.6,
                segments: 24,
                rings: 16,
            }),
            part_attributes: PartAttributes {
                color: [0.2, 0.45, 0.65, 1.0],
                anchored: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let target_id = target.id;
        let light = EntitySnapshot {
            name: Some(format!("{name} point light")),
            local_transform: transform(position + Vec3::Y * 2.0),
            light: Some(Light {
                kind: LightKind::Point,
                intensity: 4.0,
                range: 5.0,
                ..Default::default()
            }),
            ..Default::default()
        };
        let light_id = light.id;
        match &mut kind {
            EffectKind::Beam(v) => {
                v.end = [2.0, 2.0, 0.0];
                v.appearance.color = Gradient::constant([0.25, 0.5, 1.0, 1.0]);
            }
            EffectKind::ForceField(v) | EffectKind::Wind(v) => {
                v.dimensions = [3.0; 3];
                v.strength = 2.0;
            }
            EffectKind::Weather(v) => {
                v.emitter.dimensions = [3.0, 4.0, 3.0];
                v.emitter.max_particles = 300;
            }
            EffectKind::Fog(v) => {
                v.dimensions = [3.0, 2.0, 3.0];
                v.density = 0.08;
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
                v.global = false;
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
            EffectKind::ScreenFlash(v) => v.opacity = 0.03,
            EffectKind::CameraShake(v) => {
                v.position_amplitude = [0.015; 3];
                v.rotation_amplitude = [0.002; 3];
            }
            _ => {}
        }
        scene.entities.extend([target, light]);
        let mut component = EffectComponent::new(kind);
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
    let asset_directory = project.directory(VirtualDirectory::Assets)?.join("effects");
    std::fs::create_dir_all(&asset_directory)?;
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
    assert_eq!(
        loaded
            .document
            .entities
            .iter()
            .filter(|e| e.effect.is_some())
            .count(),
        26
    );
    println!(
        "Generated {} with 26 effects and 35 editable preset assets",
        destination.display()
    );
    Ok(())
}
