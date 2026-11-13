//! Mesh-clipped decal projection: clips receiver triangles to all six projector planes,
//! preserving the receiving surface rather than placing an unrelated floating quad.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "validated effect and GPU dimension budgets bound numeric conversions"
)]
#![allow(
    clippy::too_many_lines,
    reason = "GPU descriptor construction and effect extraction remain auditable transactions"
)]

use super::{ViewportMesh, ViewportScene, ViewportVertex};
use engine_world::effects::RenderDecal;
use glam::{Mat4, Vec3};
use std::hash::{Hash, Hasher};
#[derive(Clone, Copy)]
struct ClipVertex {
    position: Vec3,
    normal: Vec3,
}
pub(super) fn key(entity: engine_world::EntityId) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (entity, "decal").hash(&mut h);
    h.finish()
}
pub(super) fn project(scene: &ViewportScene, decal: &RenderDecal) -> Option<ViewportMesh> {
    let inverse = decal.transform.inverse();
    if !inverse.is_finite() {
        return None;
    }
    let half = Vec3::from_array(decal.dimensions).abs() * 0.5;
    let planes = projection_planes(decal.shape, half);
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let projection_normal = decal
        .transform
        .transform_vector3(Vec3::Y)
        .normalize_or_zero();
    for receiver in &scene.meshes {
        if receiver.textures.visibility_layers & decal.layers == 0 {
            continue;
        }
        let model = Mat4::from_cols_array(&receiver.model);
        let normals = model.inverse().transpose();
        for triangle in receiver.indices.as_chunks::<3>().0.iter().take(200_000) {
            let mut polygon: Vec<ClipVertex> = triangle
                .iter()
                .filter_map(|i| receiver.vertices.get(*i as usize))
                .map(|v| {
                    let position = inverse
                        .transform_point3(model.transform_point3(Vec3::from_array(v.position)));
                    let normal = normals
                        .transform_vector3(Vec3::from_array(v.normal))
                        .normalize_or_zero();
                    ClipVertex { position, normal }
                })
                .collect();
            if polygon.len() != 3
                || polygon
                    .iter()
                    .all(|v| v.normal.dot(projection_normal) < decal.angle_fade)
            {
                continue;
            }
            for (normal, extent) in &planes {
                if polygon.is_empty() {
                    break;
                }
                let mut output = Vec::new();
                let mut previous = *polygon.last().unwrap();
                let mut previous_distance = *extent - previous.position.dot(*normal);
                for current in &polygon {
                    let distance = *extent - current.position.dot(*normal);
                    if (distance >= 0.) != (previous_distance >= 0.) {
                        let t = previous_distance / (previous_distance - distance);
                        output.push(ClipVertex {
                            position: previous.position.lerp(current.position, t),
                            normal: previous.normal.lerp(current.normal, t).normalize_or_zero(),
                        });
                    }
                    if distance >= 0. {
                        output.push(*current);
                    }
                    previous = *current;
                    previous_distance = distance;
                }
                polygon = output;
            }
            if polygon.len() < 3 {
                continue;
            }
            let first = vertices.len() as u32;
            for v in &polygon {
                vertices.push(ViewportVertex {
                    position: (decal.transform.transform_point3(v.position) + v.normal * 0.002)
                        .to_array(),
                    normal: v.normal.to_array(),
                    uv: [
                        v.position.x / (half.x * 2.).max(0.001) + 0.5,
                        v.position.z / (half.z * 2.).max(0.001) + 0.5,
                    ],
                });
            }
            for index in 1..polygon.len() - 1 {
                indices.extend_from_slice(&[first, first + index as u32, first + index as u32 + 1]);
            }
            if vertices.len() > 1_000_000 {
                break;
            }
        }
    }
    if indices.is_empty() {
        return None;
    }
    Some(ViewportMesh {
        textures: engine_world::SurfaceTextures::default(),
        instance_key: key(decal.entity),
        mesh_key: key(decal.entity),
        vertices,
        indices,
        model: Mat4::IDENTITY.to_cols_array(),
        color: decal.appearance.color.sample(0.),
        selected: false,
    })
}

fn projection_planes(shape: engine_world::effects::VolumeShape, half: Vec3) -> Vec<(Vec3, f32)> {
    use engine_world::effects::VolumeShape;
    let mut planes = Vec::new();
    for axis in 0..3 {
        for sign in [-1.0, 1.0] {
            let mut normal = Vec3::ZERO;
            normal[axis] = sign;
            planes.push((
                normal,
                if shape == VolumeShape::Plane && axis == 1 {
                    half[axis].min(0.005)
                } else {
                    half[axis]
                },
            ));
        }
    }
    if matches!(shape, VolumeShape::Sphere | VolumeShape::Cylinder) {
        let rings = if shape == VolumeShape::Sphere {
            8_u16
        } else {
            2
        };
        for ring in 1..rings {
            let polar = f32::from(ring) / f32::from(rings) * std::f32::consts::PI;
            for segment in 0_u16..24 {
                let angle = f32::from(segment) / 24.0 * std::f32::consts::TAU;
                let direction = Vec3::new(
                    angle.cos() * polar.sin(),
                    if shape == VolumeShape::Sphere {
                        polar.cos()
                    } else {
                        0.0
                    },
                    angle.sin() * polar.sin(),
                );
                planes.push((direction / half.max(Vec3::splat(0.0001)), 1.0));
            }
        }
    }
    planes
}
/// Per-camera fading is applied to a capture/main clone after shared projection.
pub(super) fn apply_view_fade(
    scene: &mut ViewportScene,
    frame: &engine_world::effects::EffectFrame,
    camera: Vec3,
) {
    let settings: std::collections::HashMap<_, _> = frame
        .decals
        .iter()
        .map(|decal| (key(decal.entity), decal))
        .collect();
    for mesh in &mut scene.meshes {
        if let Some(decal) = settings.get(&mesh.instance_key) {
            let distance = camera.distance(decal.transform.transform_point3(Vec3::ZERO));
            let fade = if decal.distance_fade > 0.0 {
                ((decal.render_distance - distance) / decal.distance_fade).clamp(0.0, 1.0)
            } else if distance <= decal.render_distance {
                1.0
            } else {
                0.0
            };
            mesh.color[3] *= fade;
        }
    }
    scene.meshes.sort_by_key(|mesh| {
        settings
            .get(&mesh.instance_key)
            .map_or((false, 0), |decal| (true, decal.priority))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projector_clips_receiver_triangles_and_offsets_along_surface() {
        let mesh = ViewportMesh {
            textures: engine_world::SurfaceTextures::default(),
            instance_key: 1,
            mesh_key: 1,
            vertices: vec![
                ViewportVertex {
                    position: [-4., 0., -4.],
                    normal: [0., 1., 0.],
                    uv: [0.; 2],
                },
                ViewportVertex {
                    position: [4., 0., -4.],
                    normal: [0., 1., 0.],
                    uv: [0.; 2],
                },
                ViewportVertex {
                    position: [0., 0., 4.],
                    normal: [0., 1., 0.],
                    uv: [0.; 2],
                },
            ],
            indices: vec![0, 1, 2],
            model: Mat4::IDENTITY.to_cols_array(),
            color: [1.; 4],
            selected: false,
        };
        let scene = ViewportScene {
            environment: engine_world::SceneEnvironment::default(),
            camera_position: [0.; 3],
            view_projection: Mat4::IDENTITY.to_cols_array(),
            meshes: vec![mesh],
            lights: vec![],
            guides: vec![],
            grid_vertices: vec![],
            clear_color: [0.; 4],
        };
        let decal = RenderDecal {
            shape: engine_world::effects::VolumeShape::Box,
            emissive_map: None,
            distance_fade: 5.0,
            render_distance: 1000.0,
            priority: 0,
            entity: engine_world::EntityId::new(),
            transform: Mat4::IDENTITY,
            dimensions: [2., 1., 2.],
            appearance: engine_world::effects::EffectAppearance::default(),
            normal_map: None,
            orm_map: None,
            roughness: 0.5,
            metallic: 0.,
            edge_fade: 0.1,
            angle_fade: 0.,
            layers: u32::MAX,
        };
        let mesh = project(&scene, &decal).unwrap();
        assert!(!mesh.indices.is_empty());
        assert!(mesh.vertices.iter().all(|v| v.position[0].abs() <= 1.001
            && v.position[2].abs() <= 1.001
            && (v.position[1] - 0.002).abs() < 0.00001));
        for shape in [
            engine_world::effects::VolumeShape::Sphere,
            engine_world::effects::VolumeShape::Cylinder,
            engine_world::effects::VolumeShape::Plane,
        ] {
            let mut shaped = decal.clone();
            shaped.shape = shape;
            let projection = project(&scene, &shaped).unwrap();
            if shape != engine_world::effects::VolumeShape::Plane {
                assert!(
                    projection
                        .vertices
                        .iter()
                        .all(|vertex| vertex.position[0].hypot(vertex.position[2]) <= 1.02)
                );
            }
        }
        let mut rejected = decal.clone();
        rejected.layers = 0;
        assert!(project(&scene, &rejected).is_none());
        let mut low = decal.clone();
        low.priority = -1;
        low.render_distance = 20.0;
        low.distance_fade = 10.0;
        let mut high = low.clone();
        high.entity = engine_world::EntityId::new();
        high.priority = 5;
        let mut view = scene.clone();
        view.meshes = vec![
            project(&scene, &high).unwrap(),
            project(&scene, &low).unwrap(),
        ];
        let frame = engine_world::effects::EffectFrame {
            decals: vec![low.clone(), high],
            ..engine_world::effects::EffectFrame::default()
        };
        apply_view_fade(&mut view, &frame, Vec3::new(0.0, 0.0, 15.0));
        assert_eq!(view.meshes[0].instance_key, key(low.entity));
        assert!((view.meshes[0].color[3] - 0.5).abs() < f32::EPSILON);
        apply_view_fade(&mut view, &frame, Vec3::new(0.0, 0.0, 25.0));
        assert!(view.meshes[0].color[3].abs() < f32::EPSILON);
    }
}
