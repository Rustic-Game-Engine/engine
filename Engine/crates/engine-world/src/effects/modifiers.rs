use super::{
    BlendMode, EffectFrame, EffectInput, EffectKind, LightModifier, RenderDecal, RenderFogVolume,
    RenderLensFlare, RenderLightShaft, RenderOutline, RenderReflectionProbe, SurfaceModifier,
};
use glam::Vec3;

/// Extract one active modifier without mutating authored transforms, lights or materials.
/// Call in ascending priority order so higher priority post volumes blend last.
// Variant dispatch follows the authored model. Cosmetic seed-to-phase conversion is intentional.
#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
pub(super) fn extract_modifier(
    input: &EffectInput,
    camera: Vec3,
    time: f32,
    progress: f32,
    frame: &mut EffectFrame,
) {
    let center = input.transform.transform_point3(Vec3::ZERO);
    match &input.effect.kind {
        EffectKind::PostProcess(p) => {
            let local = input.transform.inverse().transform_point3(camera).abs()
                - Vec3::from_array(p.dimensions) * 0.5;
            let distance = local.max(Vec3::ZERO).length();
            let weight = if p.global {
                p.weight
            } else {
                p.weight * (1.0 - distance / p.blend_distance.max(0.0001)).clamp(0.0, 1.0)
            }
            .clamp(0.0, 1.0);
            let out = &mut frame.post;
            mix(&mut out.exposure, p.exposure, weight);
            mix(&mut out.saturation, p.saturation, weight);
            mix(&mut out.contrast, p.contrast, weight);
            mix(&mut out.brightness, p.brightness, weight);
            mix(&mut out.bloom, p.bloom, weight);
            mix(&mut out.vignette, p.vignette, weight);
            mix(
                &mut out.chromatic_aberration,
                p.chromatic_aberration,
                weight,
            );
            mix(&mut out.film_grain, p.grain, weight);
            mix(&mut out.depth_of_field, p.depth_of_field, weight);
            for i in 0..3 {
                mix(&mut out.tint[i], p.tint[i], weight);
            }
        }
        EffectKind::HeatHaze(p) => {
            let attenuation = (1.0 - center.distance(camera) / p.radius.max(0.001))
                .clamp(0.0, 1.0)
                .powf(p.edge_falloff.max(0.001));
            frame.post.distortion += p.strength * attenuation;
            frame.post.distortion_scale = p.noise_scale;
            frame.post.distortion_speed = p.speed;
        }
        EffectKind::ScreenFlash(p) => {
            let attack = if p.fade_in > 0.0 {
                (time / p.fade_in).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let release = if p.fade_out > 0.0 {
                ((input.effect.common.duration - time) / p.fade_out).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let alpha = (p.opacity * p.color[3] * attack * release).clamp(0.0, 1.0);
            match p.blend {
                BlendMode::Additive => {
                    for i in 0..3 {
                        frame.post.overlay_add[i] += p.color[i] * alpha;
                    }
                    frame.post.vignette += p.vignette * alpha;
                    return;
                }
                BlendMode::Multiply => {
                    for i in 0..3 {
                        frame.post.overlay_multiply[i] *= 1.0 + (p.color[i] - 1.0) * alpha;
                    }
                    frame.post.vignette += p.vignette * alpha;
                    return;
                }
                BlendMode::Alpha => {}
            }
            let old = frame.post.overlay;
            let a = alpha + old[3] * (1.0 - alpha);
            if a > 0.0 {
                for (i, c) in p.color[..3].iter().enumerate() {
                    frame.post.overlay[i] = (*c * alpha + old[i] * old[3] * (1.0 - alpha)) / a;
                }
            }
            frame.post.overlay[3] = a;
            frame.post.vignette += p.vignette * alpha;
        }
        EffectKind::CameraShake(p) => {
            let attenuation = if p.radius > 0.0 {
                (1.0 - center.distance(camera) / p.radius).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let strength = p.falloff.sample(progress) * attenuation;
            let seed = input.effect.common.random_seed as f32 * 0.001;
            let phase = time * p.frequency * std::f32::consts::TAU;
            let n = Vec3::new(
                (phase + seed).sin(),
                (phase * 1.173 + seed * 2.0).sin(),
                (phase * 0.793 + seed * 3.0).sin(),
            ) * strength;
            frame.camera_offset += n * Vec3::from_array(p.position_amplitude);
            frame.camera_rotation += n * Vec3::from_array(p.rotation_amplitude);
        }
        EffectKind::LightPulse(p) => {
            if let Some(target) = p.target {
                let phase = (time * p.frequency + p.phase).rem_euclid(1.0);
                let jitter = 1.0
                    + p.randomness
                        * (hash_noise(time * p.frequency * 10.0, input.effect.common.random_seed)
                            - 0.5)
                        * 2.0;
                frame.lights.push(LightModifier {
                    target,
                    color: p.color.sample(phase)[..3].try_into().unwrap_or([1.0; 3]),
                    intensity: (p.intensity.sample(phase) * jitter).max(0.0),
                });
            }
        }
        EffectKind::AnimatedSurface(p) => {
            if let Some(target) = p.target {
                frame.surfaces.push(SurfaceModifier {
                    target,
                    uv_offset: [
                        p.uv_scroll[0] * time * p.speed,
                        p.uv_scroll[1] * time * p.speed,
                    ],
                    uv_scale: p.uv_scale,
                    uv_rotation: p.uv_rotation,
                    dissolve_noise_scale: 10.0,
                    color: p.color.sample(progress),
                    emission: p.emission,
                    dissolve: 0.0,
                    edge_width: 0.0,
                    edge_color: [0.0; 3],
                });
            }
        }
        EffectKind::Dissolve(p) => {
            if let Some(target) = p.target {
                frame.surfaces.push(SurfaceModifier {
                    target,
                    uv_offset: [0.0; 2],
                    uv_scale: [1.0; 2],
                    uv_rotation: 0.0,
                    dissolve_noise_scale: p.noise_scale,
                    color: [1.0; 4],
                    emission: p.edge_emission,
                    dissolve: (p.progress + p.curve.sample(progress)).clamp(0.0, 1.0),
                    edge_width: p.edge_width,
                    edge_color: p.edge_color,
                });
            }
        }
        EffectKind::Fog(p) => frame.fog.push(RenderFogVolume {
            entity: input.entity,
            inverse_transform: input.transform.inverse(),
            dimensions: p.dimensions,
            shape: p.shape,
            density: p.density,
            color: p.color,
            height_falloff: p.height_falloff,
            edge_falloff: p.edge_falloff,
            noise_scale: p.noise_scale,
            noise_speed: p.noise_speed,
            steps: p.steps.clamp(4, 128),
        }),
        EffectKind::LightShaft(p) => frame.shafts.push(RenderLightShaft {
            light: p.light,
            position: center,
            color: p.color,
            intensity: p.intensity,
            density: p.density,
            samples: p.samples.clamp(4, 128),
            max_distance: p.max_distance,
        }),
        EffectKind::ReflectionProbe(p) => {
            if let Some(cubemap) = p.cubemap {
                frame.probes.push(RenderReflectionProbe {
                    entity: input.entity,
                    position: center,
                    dimensions: p.dimensions,
                    cubemap,
                    blend_distance: p.blend_distance,
                    priority: input.effect.common.priority,
                });
            }
        }
        EffectKind::Decal(p) => frame.decals.push(RenderDecal {
            entity: input.entity,
            transform: input.transform,
            dimensions: p.dimensions,
            appearance: p.appearance.clone(),
            normal_map: p.normal_map,
            orm_map: p.orm_map,
            roughness: p.roughness,
            metallic: p.metallic,
            edge_fade: p.edge_fade,
            angle_fade: p.angle_fade,
            layers: p.layers,
        }),
        EffectKind::Outline(p) => {
            let mut color = p.color;
            if p.pulse_speed > 0.0 {
                color[3] *= 0.5 + 0.5 * (time * p.pulse_speed * std::f32::consts::TAU).sin();
            }
            frame.outlines.push(RenderOutline {
                targets: p.targets.clone(),
                color,
                thickness: p.thickness,
                through_walls: p.through_walls,
            });
        }
        EffectKind::LensFlare(p) => frame.flares.push(RenderLensFlare {
            light: p.light,
            position: center,
            intensity: p.intensity * (time * p.fade_speed.max(0.0)).clamp(0.0, 1.0),
            scale: p.scale,
            color: p.color,
            ghosts: p.ghosts.min(16),
            ghost_spacing: p.ghost_spacing,
            streak_length: p.streak_length,
        }),
        _ => {}
    }
    frame.post.time = time;
}
fn mix(a: &mut f32, b: f32, w: f32) {
    *a += (b - *a) * w;
}
// Intentional wrapping integer hash and normalized floating point output.
#[allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]
fn hash_noise(t: f32, seed: u64) -> f32 {
    let n = (t.floor() as i64 as u64) ^ seed;
    let n = n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    ((n ^ (n >> 32)) as u32) as f32 / u32::MAX as f32
}
#[cfg(test)]
#[allow(clippy::float_cmp)] // Exact compositing identities are intentional here.
mod tests {
    use super::*;
    use crate::EntityId;
    use crate::effects::{
        CameraShake, EffectComponent, LightPulse, PostProcessVolume, ScreenFlash,
    };
    use glam::Mat4;
    fn input(kind: EffectKind) -> EffectInput {
        EffectInput {
            entity: EntityId::new(),
            transform: Mat4::IDENTITY,
            effect: EffectComponent::new(kind),
        }
    }
    #[test]
    fn local_post_volume_blends_at_border() {
        let p = PostProcessVolume {
            global: false,
            dimensions: [2.0; 3],
            blend_distance: 2.0,
            exposure: 4.0,
            ..Default::default()
        };
        let mut frame = EffectFrame::default();
        extract_modifier(
            &input(EffectKind::PostProcess(p)),
            Vec3::new(2.0, 0.0, 0.0),
            0.0,
            0.0,
            &mut frame,
        );
        assert_eq!(frame.post.exposure, 2.0);
    }
    #[test]
    fn overlay_composes_alpha_and_respects_attack_release() {
        let i = input(EffectKind::ScreenFlash(ScreenFlash {
            fade_in: 1.0,
            fade_out: 1.0,
            opacity: 0.5,
            ..Default::default()
        }));
        let mut f = EffectFrame::default();
        extract_modifier(&i, Vec3::ZERO, 0.0, 0.0, &mut f);
        assert_eq!(f.post.overlay[3], 0.0);
        extract_modifier(&i, Vec3::ZERO, 2.0, 0.4, &mut f);
        assert_eq!(f.post.overlay[3], 0.5);
        extract_modifier(&i, Vec3::ZERO, 2.0, 0.4, &mut f);
        assert_eq!(f.post.overlay[3], 0.75);
    }
    #[test]
    fn shake_is_additive_and_bounded_by_radius() {
        let i = input(EffectKind::CameraShake(CameraShake::default()));
        let mut f = EffectFrame::default();
        extract_modifier(&i, Vec3::new(100.0, 0.0, 0.0), 0.123, 0.1, &mut f);
        assert_eq!(f.camera_offset, Vec3::ZERO);
        extract_modifier(&i, Vec3::ZERO, 0.123, 0.1, &mut f);
        let first = f.camera_offset;
        assert!(first.length() > 0.0);
        extract_modifier(&i, Vec3::ZERO, 0.123, 0.1, &mut f);
        assert!(f.camera_offset.abs_diff_eq(first * 2.0, 1e-6));
    }
    #[test]
    fn modifiers_leave_authored_properties_unchanged() {
        let target = EntityId::new();
        let i = input(EffectKind::LightPulse(LightPulse {
            target: Some(target),
            ..Default::default()
        }));
        let original = i.effect.clone();
        let mut frame = EffectFrame::default();
        extract_modifier(&i, Vec3::ZERO, 0.25, 0.25, &mut frame);
        assert_eq!(i.effect, original);
        assert_eq!(frame.lights[0].target, target);
        assert_eq!(frame.lights[0].intensity, 0.25);
    }
    #[test]
    fn flash_add_and_multiply_have_distinct_output_channels() {
        let mut frame = EffectFrame::default();
        let add = input(EffectKind::ScreenFlash(ScreenFlash {
            color: [0.5, 0.25, 0.0, 1.0],
            opacity: 0.5,
            fade_in: 0.0,
            fade_out: 0.0,
            blend: BlendMode::Additive,
            ..Default::default()
        }));
        extract_modifier(&add, Vec3::ZERO, 0.2, 0.2, &mut frame);
        assert_eq!(frame.post.overlay_add, [0.25, 0.125, 0.0]);
        let mul = input(EffectKind::ScreenFlash(ScreenFlash {
            color: [0.0, 0.5, 1.0, 1.0],
            opacity: 0.5,
            fade_in: 0.0,
            fade_out: 0.0,
            blend: BlendMode::Multiply,
            ..Default::default()
        }));
        extract_modifier(&mul, Vec3::ZERO, 0.2, 0.2, &mut frame);
        assert_eq!(frame.post.overlay_multiply, [0.5, 0.75, 1.0]);
        assert_eq!(frame.post.overlay[3], 0.0);
    }
}
