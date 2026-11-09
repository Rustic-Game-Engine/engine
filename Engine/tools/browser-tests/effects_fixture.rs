//! Focused scene exported through the shipped website packager for real WebGL tests.
use super::{export_website, runtime_directory};
use engine_assets::{AssetMeta, sidecar_path};
use engine_play::{PlayMode, SnapshotBuilder, SnapshotInput};
use engine_scripting::{
    ScriptApiVersion, ScriptId, ScriptLanguage, ScriptManifest, ScriptManifestEntry,
};
use engine_world::effects::{
    BeamRenderer, BlendMode, Burst, Curve, EffectAppearance, EffectAsset, EffectComponent,
    EffectKind, Gradient, ParticleEmitter, RingEffect, ScreenFlash, TrailRenderer,
};
use engine_world::{
    Camera, EntitySnapshot, Light, LocalTransform, PartAttributes, Primitive, SceneDocument,
    ScriptComponent,
};
use glam::Vec3;
use std::{fs, path::PathBuf};

#[test]
#[ignore = "requires compiled browser runtime and Playwright; invoked by tools/browser-tests/effects.mjs"]
#[allow(
    clippy::too_many_lines,
    reason = "one end-to-end fixture keeps authored scene, script, and export contract reviewable together"
)]
fn write_browser_effects_fixture() {
    let root = PathBuf::from(
        std::env::var_os("RUSTIC_BROWSER_EFFECTS_DIR").expect("set RUSTIC_BROWSER_EFFECTS_DIR"),
    );
    fs::create_dir_all(&root).unwrap();
    let mut scene = SceneDocument::new("Browser Effects");
    scene.environment.enabled = true;
    scene.environment.ambient_intensity = 0.4;
    scene.environment.sky_color = [0.025, 0.04, 0.075];
    scene.entities.push(EntitySnapshot {
        name: Some("Camera".into()),
        local_transform: LocalTransform {
            translation: Vec3::new(0.0, 1.1, -6.0),
            ..LocalTransform::IDENTITY
        },
        camera: Some(Camera::default()),
        ..EntitySnapshot::default()
    });
    scene.entities.push(EntitySnapshot {
        light: Some(Light::default()),
        ..EntitySnapshot::default()
    });
    scene.entities.push(EntitySnapshot {
        name: Some("Floor".into()),
        local_transform: LocalTransform {
            translation: Vec3::new(0.0, -0.3, 0.0),
            ..LocalTransform::IDENTITY
        },
        primitive: Some(Primitive::Plane { size: [8.0, 8.0] }),
        part_attributes: PartAttributes {
            color: [0.08, 0.1, 0.15, 1.0],
            anchored: true,
            ..PartAttributes::default()
        },
        ..EntitySnapshot::default()
    });
    let appearance = |color| EffectAppearance {
        color: Gradient::constant(color),
        opacity: Curve::linear(1.0, 0.0),
        blend: BlendMode::Additive,
        ..EffectAppearance::default()
    };
    let mut particle = EffectComponent::new(EffectKind::Particle(ParticleEmitter {
        max_particles: 100,
        rate: 45.0,
        lifetime: [0.7, 1.2],
        speed: [0.6, 1.3],
        start_size: [0.07, 0.14],
        gravity: [0.0, -0.2, 0.0],
        appearance: appearance([1.0, 0.2, 0.02, 1.0]),
        ..ParticleEmitter::default()
    }));
    particle.common.random_seed = 456;
    let particle = EntitySnapshot {
        name: Some("Orange particles".into()),
        local_transform: LocalTransform {
            translation: Vec3::new(-1.2, 0.0, 0.0),
            ..LocalTransform::IDENTITY
        },
        effect: Some(particle),
        ..EntitySnapshot::default()
    };
    let particle_id = particle.id;
    scene.entities.push(particle);
    let script_id = ScriptId::new();
    let trail = EntitySnapshot {
        name: Some("Cyan trail".into()),
        local_transform: LocalTransform {
            translation: Vec3::new(0.0, 1.0, 0.0),
            ..LocalTransform::IDENTITY
        },
        effect: Some(EffectComponent::new(EffectKind::Trail(TrailRenderer {
            width: 0.09,
            lifetime: 0.6,
            appearance: appearance([0.02, 0.8, 1.0, 1.0]),
            ..TrailRenderer::default()
        }))),
        scripts: vec![ScriptComponent::new(script_id)],
        ..EntitySnapshot::default()
    };
    let trail_id = trail.id;
    scene.entities.push(trail);
    let mut beam = BeamRenderer {
        start: [1.2, 0.0, 0.0],
        end: [1.2, 2.0, 0.0],
        width: 0.07,
        noise_amplitude: 0.0,
        appearance: appearance([0.6, 0.08, 1.0, 1.0]),
        ..BeamRenderer::default()
    };
    beam.appearance.opacity = Curve::constant(1.0);
    scene.entities.push(EntitySnapshot {
        name: Some("Violet beam".into()),
        effect: Some(EffectComponent::new(EffectKind::Beam(beam))),
        ..EntitySnapshot::default()
    });
    let mut flash = EffectComponent::new(EffectKind::ScreenFlash(ScreenFlash {
        color: [0.05, 0.2, 1.0, 1.0],
        opacity: 0.65,
        fade_in: 0.0,
        fade_out: 0.2,
        vignette: 0.0,
        ..ScreenFlash::default()
    }));
    flash.common.auto_play = false;
    flash.common.looping = false;
    flash.common.duration = 1.2;
    let flash = EntitySnapshot {
        name: Some("Replay flash".into()),
        effect: Some(flash),
        ..EntitySnapshot::default()
    };
    let flash_id = flash.id;
    scene.entities.push(flash);
    let mut burst = EffectComponent::new(EffectKind::Particle(ParticleEmitter {
        rate: 0.0,
        bursts: vec![Burst {
            count: 30,
            ..Burst::default()
        }],
        lifetime: [0.2, 0.4],
        appearance: appearance([0.1, 1.0, 0.2, 1.0]),
        ..ParticleEmitter::default()
    }));
    burst.common.looping = false;
    burst.common.duration = 0.5;
    let mut ring = EffectComponent::new(EffectKind::Shockwave(RingEffect {
        final_radius: 1.0,
        appearance: appearance([0.1, 1.0, 0.4, 1.0]),
        ..RingEffect::default()
    }));
    ring.common.looping = false;
    ring.common.duration = 0.5;
    let asset = EffectAsset::new("Browser impact", vec![burst, ring]);
    let asset_bytes = asset.to_bytes().unwrap();
    let metadata = AssetMeta::new("rustic.effect", 1);
    let asset_id = metadata.asset_id;
    let script = format!(
        r"local elapsed,paused,was_replay,was_pause,warm=0,false,false,false,false
local particle='{particle_id}'
local trail='{trail_id}'
local flash='{flash_id}'
return {{
 Start=function()
  local config=Effects.get(particle)
  assert(config.kind.Particle.rate==45)
  local root=Effects.spawn('{asset_id}',{{0,0,0}},0.75)
  assert(root~=nil)
  rustic.log('info','vfx-ready:rate=45:asset-loaded')
 end,
 FixedUpdate=function(dt)
  local replay=rustic.key('ArrowUp').held
  if replay and not was_replay then
   Effects.restart(particle);Effects.restart(trail);Effects.restart(flash)
   assert(Effects.get(particle).kind.Particle.rate==45)
   rustic.log('info','vfx-replayed:rate=45')
  end
  was_replay=replay
  local toggle=rustic.key('ArrowDown').held
  if toggle and not was_pause then
   paused=not paused
   if paused then Effects.pause(particle);Effects.pause(trail);rustic.log('info','vfx-paused')
   else Effects.resume(particle);Effects.resume(trail);rustic.log('info','vfx-resumed') end
  end
  was_pause=toggle
  if not paused then
   elapsed=elapsed+dt
   if elapsed>1.25 and not warm then rustic.log('info','vfx-warm');warm=true end
   rustic.set_translation(math.cos(elapsed*2)*0.5,0.9+math.sin(elapsed*2)*0.4,0)
  end
 end,
}}
"
    );
    engine_scripting::validate_lua(script.as_bytes(), "browser-effects.lua", 1024 * 1024).unwrap();
    let manifest = ScriptManifest {
        format_version: engine_scripting::CURRENT_SCRIPT_MANIFEST_VERSION,
        scripts: vec![ScriptManifestEntry {
            id: script_id,
            language: ScriptLanguage::Lua54,
            relative_path: "scripts/effects.lua".into(),
            api_version: ScriptApiVersion::CURRENT,
            public_properties: vec![],
        }],
    };
    let asset_relative = PathBuf::from("assets/effects/impact.effect");
    let inputs = vec![
        SnapshotInput::new(
            "config/scripts.ron",
            ron::ser::to_string(&manifest).unwrap().into_bytes(),
        ),
        SnapshotInput::new("scripts/effects.lua", script.into_bytes()),
        SnapshotInput::new(&asset_relative, asset_bytes),
        SnapshotInput::new(
            sidecar_path(&asset_relative),
            ron::ser::to_string(&metadata).unwrap().into_bytes(),
        ),
    ];
    let snapshot = SnapshotBuilder::new(root.join("stage"))
        .stage(
            PlayMode::Standalone,
            SnapshotInput::new("scene/effects.scene", scene.to_bytes().unwrap()),
            &inputs,
        )
        .unwrap();
    export_website(
        snapshot.root(),
        &root.join("website"),
        &runtime_directory(&std::env::current_exe().unwrap()),
        "Browser Effects",
    )
    .unwrap();
    snapshot.remove().unwrap();
}
