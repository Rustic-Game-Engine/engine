use crate::{AssetId, CameraProjection, EntityId, LightKind, Primitive, SceneWorld};

/// Immutable mesh instance consumed by renderer implementations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderMesh {
    pub entity: EntityId,
    pub transform: [f32; 16],
    pub mesh: AssetId,
    pub material: Option<AssetId>,
}

/// Immutable parametric primitive instance consumed by editor/runtime renderers.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderPrimitive {
    pub entity: EntityId,
    pub transform: [f32; 16],
    pub primitive: Primitive,
    pub material: Option<AssetId>,
}

/// Immutable camera data, containing no live ECS or backend references.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderCamera {
    pub entity: EntityId,
    pub transform: [f32; 16],
    pub projection: CameraProjection,
    pub zoom: f32,
    pub order: i32,
}

/// Immutable light data, containing no backend-specific representation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderLight {
    pub entity: EntityId,
    pub transform: [f32; 16],
    pub kind: LightKind,
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
    pub spot_outer_angle_radians: f32,
    pub casts_shadows: bool,
}

/// Borrowed immutable render snapshot.
#[derive(Clone, Copy, Debug)]
pub struct RenderWorld<'a> {
    pub meshes: &'a [RenderMesh],
    pub primitives: &'a [RenderPrimitive],
    pub cameras: &'a [RenderCamera],
    pub lights: &'a [RenderLight],
}

/// Reusable extraction storage. `allocation_epoch` changes only when a backing vector
/// must grow and therefore makes the unchanged-frame allocation gate observable.
#[derive(Debug, Default)]
pub struct RenderWorldBuffer {
    meshes: Vec<RenderMesh>,
    primitives: Vec<RenderPrimitive>,
    cameras: Vec<RenderCamera>,
    lights: Vec<RenderLight>,
    allocation_epoch: u64,
}

impl RenderWorldBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub const fn allocation_epoch(&self) -> u64 {
        self.allocation_epoch
    }

    pub fn capacities(&self) -> (usize, usize, usize) {
        (
            self.meshes.capacity(),
            self.cameras.capacity(),
            self.lights.capacity(),
        )
    }

    /// Extracts stable-ID-sorted frame data into retained capacity.
    pub fn extract<'a>(&'a mut self, world: &SceneWorld) -> RenderWorld<'a> {
        self.meshes.clear();
        self.primitives.clear();
        self.cameras.clear();
        self.lights.clear();
        self.reserve_meshes(world.entity_count());

        for entity in world.entity_ids() {
            let Ok(transform) = world.world_transform(entity) else {
                continue;
            };
            let transform = transform.0.to_cols_array();
            if let Ok(Some(mesh)) = world.mesh(entity) {
                self.meshes.push(RenderMesh {
                    entity,
                    transform,
                    mesh: mesh.asset,
                    material: world
                        .material(entity)
                        .ok()
                        .flatten()
                        .map(|value| value.asset),
                });
            }
            if let Ok(Some(primitive)) = world.primitive(entity) {
                self.primitives.push(RenderPrimitive {
                    entity,
                    transform,
                    primitive: primitive.clone(),
                    material: world
                        .material(entity)
                        .ok()
                        .flatten()
                        .map(|value| value.asset),
                });
            }
            if let Ok(Some(camera)) = world.camera(entity)
                && camera.active
            {
                self.reserve_camera();
                self.cameras.push(RenderCamera {
                    entity,
                    transform,
                    projection: camera.projection,
                    zoom: camera.zoom,
                    order: camera.order,
                });
            }
            if let Ok(Some(light)) = world.light(entity) {
                self.reserve_light();
                self.lights.push(RenderLight {
                    entity,
                    transform,
                    kind: light.kind,
                    color: light.color.to_array(),
                    intensity: light.intensity,
                    range: light.range,
                    spot_outer_angle_radians: light.spot_outer_angle_radians,
                    casts_shadows: light.casts_shadows,
                });
            }
        }
        self.cameras
            .sort_by_key(|camera| (camera.order, camera.entity));
        RenderWorld {
            meshes: &self.meshes,
            primitives: &self.primitives,
            cameras: &self.cameras,
            lights: &self.lights,
        }
    }

    fn reserve_meshes(&mut self, required: usize) {
        if self.meshes.capacity() < required {
            self.meshes.reserve(required);
            self.primitives.reserve(required);
            self.allocation_epoch = self.allocation_epoch.saturating_add(1);
        }
    }

    fn reserve_camera(&mut self) {
        if self.cameras.len() == self.cameras.capacity() {
            self.cameras.reserve(1);
            self.allocation_epoch = self.allocation_epoch.saturating_add(1);
        }
    }

    fn reserve_light(&mut self) {
        if self.lights.len() == self.lights.capacity() {
            self.lights.reserve(1);
            self.allocation_epoch = self.allocation_epoch.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntitySnapshot, Mesh, WorldCommand};

    #[test]
    fn ten_thousand_static_transforms_reuse_capacity_after_warm_up() {
        let mut world = SceneWorld::new();
        let mesh = AssetId::new();
        let commands: Vec<_> = (0..10_000)
            .map(|_| {
                WorldCommand::Spawn(Box::new(EntitySnapshot {
                    mesh: Some(Mesh { asset: mesh }),
                    ..EntitySnapshot::default()
                }))
            })
            .collect();
        world.apply_commands(&commands).unwrap();
        assert_eq!(world.propagate_transforms(), 10_000);

        let mut buffer = RenderWorldBuffer::new();
        assert_eq!(buffer.extract(&world).meshes.len(), 10_000);
        let epoch = buffer.allocation_epoch();
        let capacities = buffer.capacities();
        for _ in 0..120 {
            assert_eq!(world.propagate_transforms(), 0);
            assert_eq!(buffer.extract(&world).meshes.len(), 10_000);
        }
        assert_eq!(buffer.allocation_epoch(), epoch);
        assert_eq!(buffer.capacities(), capacities);
    }

    #[test]
    fn extraction_contains_transformed_primitive() {
        let mut world = SceneWorld::new();
        let snapshot = EntitySnapshot {
            primitive: Some(Primitive::Cube { size: 1.0 }),
            ..EntitySnapshot::default()
        };
        let id = snapshot.id;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(snapshot))])
            .unwrap();
        world.propagate_transforms();
        let mut buffer = RenderWorldBuffer::new();
        let extracted = buffer.extract(&world);
        assert_eq!(extracted.primitives.len(), 1);
        assert_eq!(extracted.primitives[0].entity, id);
        assert_eq!(
            extracted.primitives[0].primitive,
            Primitive::Cube { size: 1.0 }
        );
    }
}

impl RenderCamera {
    /// Cameras look along local +Z, with local +Y up. Scale does not change the lens.
    pub fn view_projection(&self, aspect: f32) -> glam::Mat4 {
        use glam::{Mat4, Vec3};
        let transform = Mat4::from_cols_array(&self.transform);
        let (_, rotation, position) = transform.to_scale_rotation_translation();
        let view = glam::camera::rh::view::look_at_mat4(
            position,
            position + rotation * Vec3::Z,
            rotation * Vec3::Y,
        );
        let projection = match self.projection {
            CameraProjection::Perspective {
                vertical_fov_radians,
                near,
                far,
            } => glam::camera::rh::proj::directx::perspective(
                2.0 * ((vertical_fov_radians * 0.5).tan() / self.zoom).atan(),
                aspect.max(0.001),
                near,
                far,
            ),
            CameraProjection::Orthographic {
                vertical_size,
                near,
                far,
            } => {
                let half = vertical_size * 0.5 / self.zoom;
                // Right-handed projection with WebGPU depth in 0..1.
                Mat4::from_cols_array(&[
                    1.0 / (half * aspect.max(0.001)),
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0 / half,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0 / (near - far),
                    0.0,
                    0.0,
                    0.0,
                    near / (near - far),
                    1.0,
                ])
            }
        };
        projection * view
    }
}

#[cfg(test)]
mod camera_projection_tests {
    use super::*;

    #[test]
    fn zoom_changes_the_render_projection() {
        let base = RenderCamera {
            entity: EntityId::new(),
            transform: glam::Mat4::IDENTITY.to_cols_array(),
            projection: CameraProjection::default(),
            zoom: 1.0,
            order: 0,
        };
        let zoomed = RenderCamera { zoom: 2.0, ..base };

        let base_projection = base.view_projection(1.0);
        let zoomed_projection = zoomed.view_projection(1.0);
        assert!(zoomed_projection.x_axis.x.abs() > base_projection.x_axis.x.abs());
        assert!(zoomed_projection.y_axis.y.abs() > base_projection.y_axis.y.abs());
    }
}
