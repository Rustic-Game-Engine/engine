//! Bounded CPU geometry for authored trails, beams, rings and scattering surfaces.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "validated tessellation and cosmetic instance budgets bound these numerical conversions"
)]
use super::{
    Random, make_geometry, make_path_geometry, ribbon_mesh, smooth_path, sweep_box, sweep_cylinder,
    sweep_sphere, weighted_mesh,
};
use crate::effects::{
    BeamRenderer, CollisionVolume, EffectGeometry, EffectInput, EffectKind, FoliageScatter,
    RingEffect, TrailRenderer, VolumeShape,
};
use crate::{EntityId, PrimitiveMesh};
use glam::{Mat4, Quat, Vec3};
use std::collections::{BTreeSet, VecDeque};
use std::f32::consts::{PI, TAU};

pub(super) fn trail_geometry(
    entity: EntityId,
    history: &VecDeque<(Vec3, f32)>,
    trail: &TrailRenderer,
    parent: Mat4,
    owner: Mat4,
    camera: Vec3,
    time: f32,
) -> Option<EffectGeometry> {
    if history.len() < 2 {
        return None;
    }
    let count = history.len().min(1024);
    let samples: Vec<_> = (0..count)
        .map(|index| history[index * (history.len() - 1) / (count - 1)])
        .collect();
    let path: Vec<_> = samples
        .iter()
        .map(|(point, _)| parent.transform_point3(*point))
        .collect();
    let subdivisions = 1 + trail.smoothing.min(16) + trail.corner_vertices.min(8);
    let path = smooth_path(&path, subdivisions as u16);
    let ages: Vec<_> = (0..path.len())
        .map(|index| {
            let sample = index as f32 / (path.len() - 1) as f32 * (samples.len() - 1) as f32;
            let before = (sample.floor() as usize).min(samples.len() - 1);
            let after = (before + 1).min(samples.len() - 1);
            samples[before].1 + (samples[after].1 - samples[before].1) * sample.fract()
        })
        .collect();
    let width_scale = owner.to_scale_rotation_translation().0.abs().max_element();
    let widths: Vec<_> = ages
        .iter()
        .map(|age| {
            let life = (*age / trail.lifetime).clamp(0.0, 1.0);
            let start = trail.start_width.unwrap_or(trail.width);
            let end = trail.end_width.unwrap_or(trail.width);
            (start + (end - start) * life)
                * trail.appearance.size.sample(life).max(0.0)
                * width_scale
        })
        .collect();
    let view = if trail.camera_facing {
        camera
    } else {
        owner.transform_point3(Vec3::ZERO) + owner.transform_vector3(Vec3::Z) * 1000.0
    };
    let mut mesh = ribbon_mesh(&path, &widths, 0, view);
    let color = |age: f32| {
        let t = (age / trail.lifetime).clamp(0.0, 1.0);
        let mut value = trail
            .appearance
            .sample_color(t, super::appearance_random(entity));
        value[3] *= trail.appearance.opacity.sample(t).clamp(0.0, 1.0);
        value
    };
    let mut colors: Vec<_> = ages.iter().flat_map(|age| [color(*age); 2]).collect();
    if trail.cap_vertices > 0 {
        for (index, outward) in [
            (0, (path[0] - path[1]).normalize_or_zero()),
            (
                path.len() - 1,
                (path[path.len() - 1] - path[path.len() - 2]).normalize_or_zero(),
            ),
        ] {
            let side =
                (Vec3::from(mesh.positions[index * 2 + 1]) - path[index]).normalize_or_zero();
            let normal = Vec3::from(mesh.normals[index * 2]);
            let base = mesh.positions.len() as u32;
            let segments = trail.cap_vertices.clamp(1, 16) + 1;
            mesh.positions.push(path[index].to_array());
            mesh.normals.push(normal.to_array());
            mesh.tex_coords.push([index as f32, 0.5]);
            colors.push(color(ages[index]));
            for step in 0..=segments {
                let angle = PI * step as f32 / segments as f32;
                mesh.positions.push(
                    (path[index]
                        + (side * angle.cos() + outward * angle.sin()) * widths[index] * 0.5)
                        .to_array(),
                );
                mesh.normals.push(normal.to_array());
                mesh.tex_coords
                    .push([index as f32, angle.cos().midpoint(1.0)]);
                colors.push(color(ages[index]));
                if step > 0 {
                    mesh.indices.extend([base, base + step, base + step + 1]);
                }
            }
        }
    }
    let mut geometry = make_geometry(entity, mesh, &trail.appearance, 0.0, time);
    geometry.colors = colors;
    Some(geometry)
}

/// Returns the nearest finite explicit collision-volume hit along a segment.
pub(super) fn raycast_volumes(
    start: Vec3,
    end: Vec3,
    inputs: &[EffectInput],
    active: &BTreeSet<EntityId>,
    layers: u32,
) -> Option<(Vec3, Vec3)> {
    inputs
        .iter()
        .filter_map(|input| {
            let EffectKind::CollisionVolume(volume) = &input.effect.kind else {
                return None;
            };
            if !active.contains(&input.entity) || volume.layers & layers == 0 {
                return None;
            }
            let inverse = input.transform.inverse();
            if !inverse.is_finite() {
                return None;
            }
            let a = inverse.transform_point3(start);
            let b = inverse.transform_point3(end);
            let (fraction, normal) = segment_volume(a, b, volume)?;
            Some((
                fraction,
                start.lerp(end, fraction),
                inverse
                    .transpose()
                    .transform_vector3(normal)
                    .normalize_or_zero(),
            ))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, point, normal)| (point, normal))
}
fn segment_volume(start: Vec3, end: Vec3, volume: &CollisionVolume) -> Option<(f32, Vec3)> {
    let half = Vec3::from(volume.dimensions) * 0.5;
    match volume.shape {
        VolumeShape::Box => sweep_box(start, end, half),
        VolumeShape::Sphere => sweep_sphere(start, end, half.max_element()),
        VolumeShape::Cylinder => sweep_cylinder(start, end, half.x.max(half.z), half.y),
        VolumeShape::Plane => {
            let delta = start.y - end.y;
            if delta.abs() < 1e-8 {
                return None;
            }
            let fraction = start.y / delta;
            let point = start.lerp(end, fraction);
            ((0.0..=1.0).contains(&fraction) && point.x.abs() <= half.x && point.z.abs() <= half.z)
                .then_some((fraction, Vec3::Y * delta.signum()))
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "resolved beam render inputs are supplied together by runtime dispatch"
)]
pub(super) fn beam_geometry(
    entity: EntityId,
    beam: &BeamRenderer,
    start: Vec3,
    end: Vec3,
    seed: u64,
    camera: Vec3,
    time: f32,
    normalized: f32,
) -> Vec<EffectGeometry> {
    let count = beam.segments.clamp(1, 4096) as usize;
    let direction = (end - start).normalize_or_zero();
    let side = direction
        .cross(if direction.y.abs() > 0.9 {
            Vec3::X
        } else {
            Vec3::Y
        })
        .normalize_or_zero();
    let up = direction.cross(side);
    let points: Vec<_> = (0..=count)
        .map(|index| {
            let phase = index as f32 / count as f32;
            let noise = phase * beam.noise_frequency * TAU + time * beam.noise_speed;
            start.lerp(end, phase)
                + (side * noise.sin() + up * (noise * 1.73 + 1.0).sin())
                    * beam.noise_amplitude
                    * (phase * PI).sin()
        })
        .collect();
    let mut meshes = vec![ribbon_mesh(
        &points,
        &vec![beam.width; points.len()],
        0,
        camera,
    )];
    let epoch = (time * beam.noise_speed.abs()).floor().max(0.0) as u64;
    for branch in 0..beam.branch_count.min(32) {
        let mut rng = Random::new(seed ^ u64::from(branch).wrapping_mul(0x9e37_79b9) ^ epoch);
        if rng.unit() >= beam.branch_probability {
            continue;
        }
        let index =
            ((branch as usize + 1) * count / (beam.branch_count.min(32) as usize + 1)).min(count);
        let a = points[index];
        let b = a
            + side
                * ((branch as f32 * 2.1 + time * beam.noise_speed).sin()
                    * beam.noise_amplitude
                    * 5.0)
            + direction * (end - start).length() * 0.2;
        meshes.push(ribbon_mesh(
            &[a, b],
            &[beam.width * beam.branch_width_scale, 0.0],
            0,
            camera,
        ));
    }
    meshes
        .into_iter()
        .map(|mesh| {
            let mut geometry = make_path_geometry(entity, mesh, &beam.appearance, normalized, time);
            if beam.pulse_speed != 0.0 {
                for (color, uv) in geometry.colors.iter_mut().zip(&geometry.mesh.tex_coords) {
                    color[3] *= 0.5 + 0.5 * (time * beam.pulse_speed * TAU - uv[0] * TAU).sin();
                }
            }
            geometry
        })
        .collect()
}

#[allow(
    clippy::too_many_arguments,
    reason = "resolved ring render and collision inputs are supplied together by runtime dispatch"
)]
pub(super) fn shockwave_geometry(
    entity: EntityId,
    ring: &RingEffect,
    transform: Mat4,
    elapsed: f32,
    normalized: f32,
    time: f32,
    inputs: &[EffectInput],
    active: &BTreeSet<EntityId>,
) -> EffectGeometry {
    let radius = ring.expansion_speed.map_or(
        ring.initial_radius + (ring.final_radius - ring.initial_radius) * normalized,
        |speed| {
            let sign = (ring.final_radius - ring.initial_radius).signum();
            (ring.initial_radius + sign * speed * elapsed).clamp(
                ring.initial_radius.min(ring.final_radius),
                ring.initial_radius.max(ring.final_radius),
            )
        },
    );
    let origin = transform.transform_point3(Vec3::ZERO);
    let owner_normal = transform
        .inverse()
        .transpose()
        .transform_vector3(Vec3::Y)
        .normalize_or_zero();
    let hit = ring
        .align_to_surface
        .then(|| {
            raycast_volumes(
                origin + Vec3::Y * radius.max(1.0),
                origin - Vec3::Y * radius.max(1.0),
                inputs,
                active,
                ring.surface_layers,
            )
        })
        .flatten();
    let (center, normal) = hit.unwrap_or((origin, owner_normal));
    let rotation = Quat::from_rotation_arc(owner_normal, normal);
    let inner = (radius - ring.thickness * 0.5).max(0.0);
    let outer = (radius + ring.thickness * 0.5).max(inner + 0.0001);
    let feather = (outer - inner) * ring.edge_softness.clamp(0.0, 1.0) * 0.5;
    let bands = if feather > 0.0 {
        vec![
            (inner, 0.0),
            (inner + feather, 1.0),
            (outer - feather, 1.0),
            (outer, 0.0),
        ]
    } else {
        vec![(inner, 1.0), (outer, 1.0)]
    };
    let mut mesh = PrimitiveMesh::default();
    let mut colors = Vec::new();
    for segment in 0..=96 {
        let angle = segment as f32 / 96.0 * TAU;
        let displacement =
            (angle * ring.displacement_frequency + time).sin() * ring.displacement_strength;
        for (r, alpha) in &bands {
            let local = Vec3::new(angle.cos() * *r, displacement, angle.sin() * *r);
            mesh.positions.push(
                (center
                    + rotation * transform.transform_vector3(local)
                    + normal * ring.vertical_offset)
                    .to_array(),
            );
            mesh.normals.push(normal.to_array());
            mesh.tex_coords
                .push([segment as f32 / 96.0, (*r - inner) / (outer - inner)]);
            let mut color = ring
                .appearance
                .sample_color(normalized, super::appearance_random(entity));
            color[3] *= ring.appearance.opacity.sample(normalized) * *alpha;
            colors.push(color);
        }
        if segment > 0 {
            for band in 0..bands.len() - 1 {
                let a = ((segment - 1) * bands.len() + band) as u32;
                let b = (segment * bands.len() + band) as u32;
                mesh.indices.extend([a, b, a + 1, a + 1, b, b + 1]);
            }
        }
    }
    let mut geometry = make_geometry(entity, mesh, &ring.appearance, normalized, time);
    geometry.colors = colors;
    geometry
}

pub(super) fn animate_ribbon_points(points: &mut [Vec3], speed: f32, time: f32) {
    if speed == 0.0 || points.len() < 2 {
        return;
    }
    let count = points.len();
    for (index, point) in points.iter_mut().enumerate() {
        let progress = index as f32 / (count - 1) as f32;
        point.y += (time * speed * TAU + progress * TAU).sin() * 0.1 * (progress * PI).sin();
    }
}

pub(super) struct ScatterSample {
    pub position: Vec3,
    pub rotation: Vec3,
    pub size: f32,
    pub mesh: Option<crate::AssetId>,
    pub material: Option<crate::AssetId>,
}

fn clip(mut polygon: Vec<Vec3>, distance: impl Fn(Vec3) -> f32) -> Vec<Vec3> {
    if polygon.is_empty() {
        return polygon;
    }
    let mut output = Vec::with_capacity(polygon.len() + 1);
    let mut previous = *polygon.last().expect("nonempty polygon");
    let mut before = distance(previous);
    for point in polygon.drain(..) {
        let after = distance(point);
        if (before >= 0.0) != (after >= 0.0) {
            output.push(previous.lerp(point, before / (before - after)));
        }
        if after >= 0.0 {
            output.push(point);
        }
        previous = point;
        before = after;
    }
    output
}

/// Deterministic area-weighted surface placement, clipped to authored volume,
/// world height and slope restrictions before generating any instances.
#[allow(
    clippy::too_many_lines,
    reason = "surface clipping, area sampling and stable LOD admission preserve a single deterministic RNG sequence"
)]
pub(super) fn scatter_samples(
    config: &FoliageScatter,
    transform: Mat4,
    surface: Mat4,
    seed: u64,
    cameras: &[Vec3],
    render_distance: f32,
) -> Vec<ScatterSample> {
    let inverse = transform.inverse();
    let half = Vec3::from(config.dimensions) * 0.5;
    let mut triangles = Vec::new();
    let mut cumulative = Vec::new();
    let mut total = 0.0;
    for triangle in &config.surface_triangles {
        let mut polygon: Vec<_> = triangle
            .iter()
            .map(|point| surface.transform_point3(Vec3::from(*point)))
            .collect();
        for axis in 0..3 {
            for sign in [-1.0, 1.0] {
                polygon = clip(polygon, |point| {
                    half[axis] - inverse.transform_point3(point)[axis] * sign
                });
            }
        }
        polygon = clip(polygon, |point| point.y - config.height[0]);
        polygon = clip(polygon, |point| config.height[1] - point.y);
        for index in 1..polygon.len().saturating_sub(1) {
            let points = [polygon[0], polygon[index], polygon[index + 1]];
            let cross = (points[1] - points[0]).cross(points[2] - points[0]);
            let normal = cross.normalize_or_zero();
            let slope = normal.dot(Vec3::Y).clamp(-1.0, 1.0).acos().to_degrees();
            if slope < config.slope[0] || slope > config.slope[1] {
                continue;
            }
            let area = cross.length() * 0.5;
            if area <= 1.0e-10 {
                continue;
            }
            total += area;
            triangles.push((points, normal));
            cumulative.push(total);
        }
    }
    let flat = config.surface_triangles.is_empty();
    if flat {
        total = (transform
            .transform_vector3(Vec3::X * config.dimensions[0])
            .cross(transform.transform_vector3(Vec3::Z * config.dimensions[2])))
        .length();
    }
    if total <= 0.0 {
        return Vec::new();
    }
    let count = (total * config.density)
        .max(0.0)
        .min(config.max_instances.min(100_000) as f32) as usize;
    let mut rng = Random::new(seed);
    let mut result = Vec::with_capacity(count);
    for index in 0..count {
        let (point, normal) = if flat {
            let point = transform.transform_point3(Vec3::new(
                rng.range(-0.5, 0.5) * config.dimensions[0],
                0.0,
                rng.range(-0.5, 0.5) * config.dimensions[2],
            ));
            (
                point,
                transform
                    .inverse()
                    .transpose()
                    .transform_vector3(Vec3::Y)
                    .normalize_or_zero(),
            )
        } else {
            let selected = rng.unit() * total;
            let triangle = &triangles[cumulative
                .partition_point(|area| *area < selected)
                .min(triangles.len() - 1)];
            let u = rng.unit().sqrt();
            let v = rng.unit();
            (
                triangle.0[0] * (1.0 - u) + triangle.0[1] * u * (1.0 - v) + triangle.0[2] * u * v,
                triangle.1,
            )
        };
        let yaw = rng.range(config.rotation[0], config.rotation[1]);
        let size = rng.range(config.scale[0], config.scale[1]);
        let choice = weighted_mesh(&config.weighted_meshes, &mut rng);
        let mut mesh = choice.map(|value| value.0).or_else(|| {
            config
                .meshes
                .get(index % config.meshes.len().max(1))
                .copied()
        });
        let material = choice
            .and_then(|value| value.1)
            .or(config.appearance.material);
        let density_sample = rng.unit();
        let distance = cameras
            .iter()
            .map(|camera| point.distance(*camera))
            .fold(f32::INFINITY, f32::min);
        let slope = normal.dot(Vec3::Y).clamp(-1.0, 1.0).acos().to_degrees();
        if point.y < config.height[0]
            || point.y > config.height[1]
            || slope < config.slope[0]
            || slope > config.slope[1]
            || distance > render_distance
        {
            continue;
        }
        if let Some(lod) = config
            .lod
            .iter()
            .filter(|lod| distance >= lod.distance)
            .max_by(|a, b| a.distance.total_cmp(&b.distance))
        {
            if density_sample >= lod.density_scale {
                continue;
            }
            if !lod.meshes.is_empty() {
                mesh = lod.meshes.get(index % lod.meshes.len()).copied();
            }
        }
        let orientation = if config.align_to_surface {
            Quat::from_rotation_arc(Vec3::Y, normal) * Quat::from_rotation_y(yaw)
        } else {
            Quat::from_rotation_y(yaw)
        };
        let (x, y, z) = orientation.to_euler(glam::EulerRot::XYZ);
        result.push(ScatterSample {
            position: point,
            rotation: Vec3::new(x, y, z),
            size,
            mesh,
            material,
        });
    }
    result
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::field_reassign_with_default,
    reason = "tests compare deterministic geometry and adjust individual authored fixture controls"
)]
mod tests {
    use super::*;

    #[test]
    fn trails_transform_history_and_tessellate_caps() {
        let history = VecDeque::from([(Vec3::ZERO, 0.8), (Vec3::X, 0.0)]);
        let mut trail = TrailRenderer::default();
        trail.start_width = Some(0.4);
        trail.end_width = Some(0.1);
        let entity = EntityId::new();
        let parent = Mat4::from_translation(Vec3::Y * 5.0);
        let plain = trail_geometry(
            entity,
            &history,
            &trail,
            parent,
            Mat4::IDENTITY,
            Vec3::Z * 10.0,
            0.0,
        )
        .unwrap();
        trail.smoothing = 3;
        trail.corner_vertices = 2;
        trail.cap_vertices = 4;
        let smooth = trail_geometry(
            entity,
            &history,
            &trail,
            parent,
            Mat4::IDENTITY,
            Vec3::Z * 10.0,
            0.0,
        )
        .unwrap();
        assert!(smooth.mesh.positions.len() > plain.mesh.positions.len());
        assert_eq!(smooth.colors.len(), smooth.mesh.positions.len());
        assert!(smooth.mesh.positions.iter().all(|p| p[1] > 4.5));
        assert!(
            (Vec3::from(plain.mesh.positions[0]) - Vec3::from(plain.mesh.positions[1])).length()
                < 0.4
        );
    }

    #[test]
    fn beam_probability_and_pulse_change_geometry() {
        let mut beam = BeamRenderer::default();
        beam.branch_count = 4;
        beam.branch_probability = 0.0;
        let entity = EntityId::new();
        let plain = beam_geometry(
            entity,
            &beam,
            Vec3::ZERO,
            Vec3::X * 5.0,
            42,
            Vec3::Z * 10.0,
            0.0,
            0.5,
        );
        assert_eq!(plain.len(), 1);
        beam.branch_probability = 1.0;
        beam.pulse_speed = 1.0;
        let branched = beam_geometry(
            entity,
            &beam,
            Vec3::ZERO,
            Vec3::X * 5.0,
            42,
            Vec3::Z * 10.0,
            0.25,
            0.5,
        );
        assert_eq!(branched.len(), 5);
        assert!(
            branched[0]
                .colors
                .iter()
                .any(|color| color[3] < plain[0].colors[0][3])
        );
    }

    #[test]
    fn scatter_samples_surface_area_height_and_lod_deterministically() {
        let mut scatter = FoliageScatter::default();
        scatter.dimensions = [4.0, 4.0, 4.0];
        scatter.density = 20.0;
        scatter.surface_triangles = vec![[[-1.0, 0.0, -1.0], [0.0, 0.0, 1.0], [1.0, 0.0, -1.0]]];
        let first = scatter_samples(
            &scatter,
            Mat4::IDENTITY,
            Mat4::IDENTITY,
            7,
            &[Vec3::ZERO],
            100.0,
        );
        let second = scatter_samples(
            &scatter,
            Mat4::IDENTITY,
            Mat4::IDENTITY,
            7,
            &[Vec3::ZERO],
            100.0,
        );
        assert_eq!(first.len(), 40);
        assert!(
            first
                .iter()
                .zip(&second)
                .all(|(a, b)| a.position == b.position && a.rotation == b.rotation)
        );
        assert!(first.iter().all(|sample| sample.position.y.abs() < 1e-6));
        scatter.height = [1.0, 2.0];
        assert!(
            scatter_samples(
                &scatter,
                Mat4::IDENTITY,
                Mat4::IDENTITY,
                7,
                &[Vec3::ZERO],
                100.0
            )
            .is_empty()
        );
        scatter.height = [-1.0, 1.0];
        scatter.lod.push(crate::effects::ScatterLod {
            distance: 0.0,
            density_scale: 0.0,
            meshes: Vec::new(),
        });
        assert!(
            scatter_samples(
                &scatter,
                Mat4::IDENTITY,
                Mat4::IDENTITY,
                7,
                &[Vec3::ZERO],
                100.0
            )
            .is_empty()
        );
    }

    #[test]
    fn explicit_volume_raycast_uses_nearest_hit_and_layer_mask() {
        let make = |y: f32, layers: u32| EffectInput {
            entity: EntityId::new(),
            transform: Mat4::from_translation(Vec3::Y * y),
            effect: crate::effects::EffectComponent::new(EffectKind::CollisionVolume(
                CollisionVolume {
                    layers,
                    ..Default::default()
                },
            )),
        };
        let near = make(2.0, 1);
        let far = make(0.0, 2);
        let active = BTreeSet::from([near.entity, far.entity]);
        let inputs = [far, near];
        let (point, normal) =
            raycast_volumes(Vec3::Y * 5.0, -Vec3::Y * 5.0, &inputs, &active, 3).unwrap();
        assert!((point.y - 2.0).abs() < 1e-5);
        assert_eq!(normal, Vec3::Y);
        assert!(raycast_volumes(Vec3::Y * 5.0, -Vec3::Y * 5.0, &inputs, &active, 4).is_none());
        let (point, _) =
            raycast_volumes(Vec3::Y * 5.0, -Vec3::Y * 5.0, &inputs, &active, 2).unwrap();
        assert!(point.y.abs() < 1e-5);
    }

    #[test]
    fn ring_feather_and_independent_expansion_are_applied() {
        let mut ring = RingEffect::default();
        ring.edge_softness = 0.5;
        ring.expansion_speed = Some(0.0);
        let geometry = shockwave_geometry(
            EntityId::new(),
            &ring,
            Mat4::IDENTITY,
            1.0,
            1.0,
            1.0,
            &[],
            &BTreeSet::new(),
        );
        assert!(geometry.colors.iter().any(|color| color[3] == 0.0));
        let max = geometry
            .mesh
            .positions
            .iter()
            .map(|position| Vec3::from(*position).length())
            .fold(0.0, f32::max);
        assert!(max <= ring.initial_radius + ring.thickness + 1e-5);
    }
}
