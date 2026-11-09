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
            for axis in 0..3 {
                for sign in [-1., 1.] {
                    if polygon.is_empty() {
                        break;
                    }
                    let mut output = Vec::new();
                    let mut previous = *polygon.last().unwrap();
                    let mut previous_distance = half[axis] - previous.position[axis] * sign;
                    for current in &polygon {
                        let distance = half[axis] - current.position[axis] * sign;
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
    }
}
