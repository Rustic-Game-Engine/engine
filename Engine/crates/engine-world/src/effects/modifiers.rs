use super::{
    BlendMode, EffectFrame, EffectInput, EffectKind, LightModifier, QualityLevel, RenderDecal,
    RenderDistortion, RenderFogVolume, RenderLensFlare, RenderLightShaft, RenderOutline,
    RenderReflectionProbe, RenderScreenOverlay, SurfaceModifier, VolumeShape,
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
            if p.layers & frame.camera_visibility_layers.unwrap_or(u32::MAX) == 0 {
                return;
            }
            let distance = volume_distance(input.transform, p.shape, p.dimensions, camera);
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
            mix(&mut out.motion_blur, p.motion_blur, weight);
            mix(&mut out.focus_distance, p.focus_distance, weight);
            mix(&mut out.focus_range, p.focus_range, weight);
            mix(&mut out.bloom_threshold, p.bloom_threshold, weight);
            mix(&mut out.bloom_radius, p.bloom_radius, weight);
            mix(&mut out.temporal_weight, p.temporal_weight, weight);
            if weight >= 0.5 {
                out.motion_blur_samples = p.motion_blur_samples;
            }
            for i in 0..3 {
                mix(&mut out.tint[i], p.tint[i], weight);
            }
        }
        EffectKind::HeatHaze(p) => {
            frame.distortions.push(RenderDistortion {
                entity: input.entity,
                inverse_transform: input.transform.inverse(),
                radius: p.radius,
                strength: p.strength,
                noise_scale: p.noise_scale,
                speed: p.speed,
                edge_falloff: p.edge_falloff,
                noise_texture: p.noise_texture,
                refraction_offset: p.refraction_offset,
                depth_masking: p.depth_masking,
                screen_space: p.screen_space,
                time,
            });
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
            frame.overlays.push(RenderScreenOverlay {
                entity: input.entity,
                color: p.color,
                opacity: alpha,
                vignette: p.vignette,
                blend: p.blend,
                screen_mask: p.screen_mask,
                stacking: p.stacking,
            });
            match p.blend {
                BlendMode::Additive => {
                    for i in 0..3 {
                        frame.post.overlay_add[i] += p.color[i] * alpha;
                    }
                    return;
                }
                BlendMode::Multiply => {
                    for i in 0..3 {
                        frame.post.overlay_multiply[i] *= 1.0 + (p.color[i] - 1.0) * alpha;
                    }
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
        }
        EffectKind::CameraShake(p) => {
            let (position, rotation) = camera_shake_impulse(input, camera, time, progress);
            blend_camera_impulse(frame, p.blend_mode, position, rotation);
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
                    intensity: (p.intensity.sample(phase)
                        * jitter
                        * fade_envelope(time, input.effect.common.duration, p.fade_in, p.fade_out))
                    .max(0.0),
                });
            }
        }
        EffectKind::AnimatedSurface(p) => {
            if let Some(target) = p.target.or_else(|| p.target_material.map(|_| input.entity)) {
                frame.surfaces.push(SurfaceModifier {
                    is_dissolve: false,
                    source: input.entity,
                    target,
                    target_material: p.target_material,
                    noise_texture: None,
                    mask_texture: p.mask_texture,
                    distortion_texture: p.distortion_map,
                    distortion_strength: p.distortion_strength,
                    direction: [0.0; 3],
                    phase: p.phase + time * p.speed,
                    particle_density: 0.0,
                    uv_offset: [
                        p.uv_scroll[0] * (time * p.speed + p.phase),
                        p.uv_scroll[1] * (time * p.speed + p.phase),
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
            if let Some(target) = p.target.or_else(|| p.target_material.map(|_| input.entity)) {
                frame.surfaces.push(SurfaceModifier {
                    is_dissolve: true,
                    source: input.entity,
                    target,
                    target_material: p.target_material,
                    noise_texture: p.noise_texture,
                    mask_texture: p.direction_mask,
                    distortion_texture: None,
                    distortion_strength: 0.0,
                    direction: p.direction,
                    phase: time,
                    particle_density: p.particle_density,
                    uv_offset: [0.0; 2],
                    uv_scale: [1.0; 2],
                    uv_rotation: 0.0,
                    dissolve_noise_scale: p.noise_scale,
                    color: [1.0; 4],
                    emission: p.edge_emission,
                    dissolve: (p.progress + p.threshold + p.curve.sample(progress)).clamp(0.0, 1.0),
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
            noise_texture: p.noise_texture,
            anisotropy: p.anisotropy,
            light_scattering: p.light_scattering,
            max_distance: p.max_distance,
            temporal_filtering: p.temporal_filtering,
            time,
            steps: quality_samples(p.steps, input.effect.common.quality),
        }),
        EffectKind::LightShaft(p) => frame.shafts.push(RenderLightShaft {
            entity: input.entity,
            light: p.light,
            position: center,
            color: p.color,
            intensity: p.intensity,
            density: p.density,
            samples: quality_samples(p.samples, input.effect.common.quality),
            max_distance: p.max_distance,
            anisotropy: p.anisotropy,
            occlusion: p.occlusion,
            temporal_filtering: p.temporal_filtering,
            localized: p.localized,
            inverse_transform: input.transform.inverse(),
            shape: p.shape,
            dimensions: p.dimensions,
        }),
        EffectKind::ReflectionProbe(p) => {
            if p.cubemap.is_some()
                || p.refresh != super::ProbeRefresh::Baked
                || p.refresh_revision > 0
            {
                frame.probes.push(RenderReflectionProbe {
                    entity: input.entity,
                    position: center,
                    capture_position: input
                        .transform
                        .transform_point3(Vec3::from_array(p.capture_position)),
                    inverse_transform: input.transform.inverse(),
                    shape: p.shape,
                    dimensions: p.dimensions,
                    cubemap: if p.refresh == super::ProbeRefresh::Baked && p.refresh_revision == 0 {
                        p.cubemap
                            .unwrap_or_else(|| crate::AssetId::from(*input.entity.as_uuid()))
                    } else {
                        crate::AssetId::from(*input.entity.as_uuid())
                    },
                    blend_distance: p.blend_distance,
                    priority: input.effect.common.priority,
                    resolution: p.resolution,
                    refresh: p.refresh,
                    update_interval: p.update_interval,
                    parallax_correction: p.parallax_correction,
                    visibility_layers: p.visibility_layers,
                    receiver_layers: input.effect.common.visibility_layers,
                    quality: p.quality,
                    refresh_revision: p.refresh_revision,
                    time,
                });
            }
        }
        EffectKind::Decal(p) => frame.decals.push(RenderDecal {
            shape: p.shape,
            emissive_map: p.emissive_map,
            distance_fade: p.distance_fade,
            render_distance: input.effect.common.render_distance,
            priority: input.effect.common.priority,
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
            entity: input.entity,
            light: p.light,
            position: center,
            intensity: p.intensity,
            scale: p.scale,
            color: p.color,
            ghosts: p.ghosts.min(16),
            ghost_spacing: p.ghost_spacing,
            streak_length: p.streak_length,
            streak_orientation: p.streak_orientation,
            texture: p.texture,
            occlusion: p.occlusion,
            max_distance: p.max_distance,
            fade_speed: p.fade_speed,
        }),
        _ => {}
    }
    frame.post.time = time;
}
fn quality_samples(samples: u32, quality: QualityLevel) -> u32 {
    match quality {
        QualityLevel::Low => samples.div_ceil(2),
        QualityLevel::Medium => samples,
        QualityLevel::High => samples.saturating_mul(3) / 2,
    }
    .clamp(4, 128)
}
fn volume_distance(
    transform: glam::Mat4,
    shape: VolumeShape,
    dimensions: [f32; 3],
    point: Vec3,
) -> f32 {
    let local = transform.inverse().transform_point3(point);
    let half = Vec3::from_array(dimensions) * 0.5;
    let closest = match shape {
        VolumeShape::Sphere => {
            let radius = (local / half).length();
            if radius <= 1. { local } else { local / radius }
        }
        VolumeShape::Cylinder => {
            let radius = glam::Vec2::new(local.x / half.x, local.z / half.z)
                .length()
                .max(1.);
            Vec3::new(
                local.x / radius,
                local.y.clamp(-half.y, half.y),
                local.z / radius,
            )
        }
        VolumeShape::Box | VolumeShape::Plane => local.clamp(-half, half),
    };
    transform.transform_vector3(local - closest).length()
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
fn fade_envelope(time: f32, duration: f32, attack: f32, release: f32) -> f32 {
    let fade_in = if attack > 0.0 {
        (time / attack).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let fade_out = if release > 0.0 {
        ((duration - time) / release).clamp(0.0, 1.0)
    } else {
        1.0
    };
    fade_in * fade_out
}
#[allow(
    clippy::cast_precision_loss,
    reason = "cosmetic seed-to-phase conversion"
)]
pub(super) fn camera_shake_impulse(
    input: &EffectInput,
    camera: Vec3,
    time: f32,
    progress: f32,
) -> (Vec3, Vec3) {
    let EffectKind::CameraShake(p) = &input.effect.kind else {
        return (Vec3::ZERO, Vec3::ZERO);
    };
    let center = input.transform.transform_point3(Vec3::ZERO);
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
    (
        n * Vec3::from_array(p.position_amplitude),
        n * Vec3::from_array(p.rotation_amplitude),
    )
}
pub(super) fn blend_camera_impulse(
    frame: &mut EffectFrame,
    mode: super::CameraShakeBlendMode,
    position: Vec3,
    rotation: Vec3,
) {
    match mode {
        super::CameraShakeBlendMode::Additive => {
            frame.camera_offset += position;
            frame.camera_rotation += rotation;
        }
        super::CameraShakeBlendMode::Replace => {
            frame.camera_offset = position;
            frame.camera_rotation = rotation;
        }
        super::CameraShakeBlendMode::Maximum => {
            frame.camera_offset = Vec3::select(
                position.abs().cmpgt(frame.camera_offset.abs()),
                position,
                frame.camera_offset,
            );
            frame.camera_rotation = Vec3::select(
                rotation.abs().cmpgt(frame.camera_rotation.abs()),
                rotation,
                frame.camera_rotation,
            );
        }
    }
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
    fn light_pulse_explicit_fades_validate_and_preserve_authored_light() {
        let target = EntityId::new();
        let mut i = input(EffectKind::LightPulse(LightPulse {
            target: Some(target),
            intensity: crate::effects::Curve::linear(2.0, 2.0),
            fade_in: 1.0,
            fade_out: 1.0,
            ..LightPulse::default()
        }));
        i.effect.common.duration = 4.0;
        let authored = i.effect.clone();
        for (time, expected) in [(0.0, 0.0), (0.5, 1.0), (2.0, 2.0), (3.5, 1.0), (4.0, 0.0)] {
            let mut frame = EffectFrame::default();
            extract_modifier(&i, Vec3::ZERO, time, time / 4.0, &mut frame);
            assert_eq!(frame.lights[0].intensity, expected);
        }
        assert_eq!(i.effect, authored);
        if let EffectKind::LightPulse(pulse) = &mut i.effect.kind {
            pulse.fade_in = -1.0;
        }
        assert!(i.effect.validate().is_err());
    }
    #[test]
    fn camera_blend_modes_replace_or_select_largest_signed_component() {
        use crate::effects::CameraShakeBlendMode;
        let mut frame = EffectFrame::default();
        blend_camera_impulse(
            &mut frame,
            CameraShakeBlendMode::Additive,
            Vec3::new(2.0, -3.0, 1.0),
            Vec3::X,
        );
        blend_camera_impulse(
            &mut frame,
            CameraShakeBlendMode::Maximum,
            Vec3::new(-4.0, 1.0, -2.0),
            Vec3::Y,
        );
        assert_eq!(frame.camera_offset, Vec3::new(-4.0, -3.0, -2.0));
        assert_eq!(frame.camera_rotation, Vec3::new(1.0, 1.0, 0.0));
        blend_camera_impulse(
            &mut frame,
            CameraShakeBlendMode::Replace,
            Vec3::Z,
            Vec3::ZERO,
        );
        assert_eq!(frame.camera_offset, Vec3::Z);
        assert_eq!(frame.camera_rotation, Vec3::ZERO);
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
