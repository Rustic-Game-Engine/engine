use engine_core::EntityId;
use engine_world::LocalTransform;
use glam::{Mat4, Quat, Vec2, Vec3, Vec4};

const MIN_DISTANCE: f32 = 0.05;
const MAX_DISTANCE: f32 = 100_000.0;

/// Backend-neutral perspective camera used by native authoring viewports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorCamera {
    pub focus: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub vertical_fov_radians: f32,
    pub near: f32,
    pub far: f32,
}

impl Default for EditorCamera {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            yaw: 135.0_f32.to_radians(),
            pitch: 24.0_f32.to_radians(),
            distance: 7.0,
            vertical_fov_radians: 55.0_f32.to_radians(),
            near: 0.05,
            far: 10_000.0,
        }
    }
}

impl EditorCamera {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn position(self) -> Vec3 {
        self.focus
            + Vec3::new(
                self.yaw.sin() * self.pitch.cos(),
                self.pitch.sin(),
                self.yaw.cos() * self.pitch.cos(),
            ) * self.distance
    }

    pub fn forward(self) -> Vec3 {
        (self.focus - self.position()).normalize_or_zero()
    }

    pub fn right(self) -> Vec3 {
        self.forward().cross(Vec3::Y).normalize_or_zero()
    }

    pub fn up(self) -> Vec3 {
        self.right().cross(self.forward()).normalize_or_zero()
    }

    pub fn view_matrix(self) -> Mat4 {
        glam::camera::rh::view::look_at_mat4(self.position(), self.focus, Vec3::Y)
    }

    pub fn projection_matrix(self, aspect: f32) -> Mat4 {
        glam::camera::rh::proj::directx::perspective(
            self.vertical_fov_radians,
            aspect.max(0.001),
            self.near,
            self.far,
        )
    }

    pub fn view_projection(self, aspect: f32) -> Mat4 {
        self.projection_matrix(aspect) * self.view_matrix()
    }

    pub fn orbit(&mut self, delta: Vec2) {
        self.yaw -= delta.x * 0.008;
        self.pitch = (self.pitch + delta.y * 0.008).clamp(-1.553, 1.553);
    }

    pub fn pan(&mut self, delta: Vec2, viewport_height: f32) {
        let units_per_pixel = 2.0 * self.distance * (self.vertical_fov_radians * 0.5).tan()
            / viewport_height.max(1.0);
        self.focus += (-self.right() * delta.x + self.up() * delta.y) * units_per_pixel;
    }

    pub fn dolly(&mut self, amount: f32) {
        self.distance = (self.distance * (-amount * 0.12).exp()).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    pub fn fly(&mut self, local_motion: Vec3, delta_seconds: f32, speed: f32) {
        let world = self.right() * local_motion.x
            + Vec3::Y * local_motion.y
            + self.forward() * local_motion.z;
        self.focus += world * delta_seconds.max(0.0) * speed.max(0.0);
    }

    pub fn frame_sphere(&mut self, center: Vec3, radius: f32) {
        self.focus = center;
        let half_fov = (self.vertical_fov_radians * 0.5).max(0.01);
        self.distance = (radius.max(0.1) / half_fov.sin() * 1.25).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    pub fn world_ray(self, pixel: Vec2, extent: Vec2) -> Option<WorldRay> {
        if extent.x <= 0.0 || extent.y <= 0.0 || !pixel.is_finite() || !extent.is_finite() {
            return None;
        }
        let ndc = Vec2::new(
            pixel.x / extent.x * 2.0 - 1.0,
            1.0 - pixel.y / extent.y * 2.0,
        );
        let inverse = self.view_projection(extent.x / extent.y).inverse();
        if !inverse.is_finite() {
            return None;
        }
        let near = inverse * Vec4::new(ndc.x, ndc.y, 0.0, 1.0);
        let far = inverse * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
        let near = near.truncate() / near.w;
        let far = far.truncate() / far.w;
        let direction = (far - near).normalize_or_zero();
        (direction.length_squared() > 0.0).then_some(WorldRay {
            origin: near,
            direction,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldRay {
    pub origin: Vec3,
    pub direction: Vec3,
}

#[derive(Clone, Debug)]
pub struct PickMesh {
    pub entity: EntityId,
    pub transform: Mat4,
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

/// Returns the nearest visible triangle hit.
pub fn pick_meshes(ray: WorldRay, meshes: &[PickMesh]) -> Option<EntityId> {
    let mut nearest = f32::INFINITY;
    let mut picked = None;
    for mesh in meshes {
        for &[index_a, index_b, index_c] in mesh.indices.as_chunks::<3>().0 {
            let (Some(point_a), Some(point_b), Some(point_c)) = (
                mesh.positions.get(index_a as usize).copied(),
                mesh.positions.get(index_b as usize).copied(),
                mesh.positions.get(index_c as usize).copied(),
            ) else {
                continue;
            };
            let point_a = mesh.transform.transform_point3(Vec3::from_array(point_a));
            let point_b = mesh.transform.transform_point3(Vec3::from_array(point_b));
            let point_c = mesh.transform.transform_point3(Vec3::from_array(point_c));
            if let Some(distance) = ray_triangle(ray, point_a, point_b, point_c)
                && distance < nearest
            {
                nearest = distance;
                picked = Some(mesh.entity);
            }
        }
    }
    picked
}

fn ray_triangle(ray: WorldRay, point_a: Vec3, point_b: Vec3, point_c: Vec3) -> Option<f32> {
    let edge1 = point_b - point_a;
    let edge2 = point_c - point_a;
    let cross_direction_edge = ray.direction.cross(edge2);
    let determinant = edge1.dot(cross_direction_edge);
    if determinant.abs() < 1.0e-7 {
        return None;
    }
    let inverse = determinant.recip();
    let origin_delta = ray.origin - point_a;
    let barycentric_u = origin_delta.dot(cross_direction_edge) * inverse;
    if !(0.0..=1.0).contains(&barycentric_u) {
        return None;
    }
    let cross_delta_edge = origin_delta.cross(edge1);
    let barycentric_v = ray.direction.dot(cross_delta_edge) * inverse;
    if barycentric_v < 0.0 || barycentric_u + barycentric_v > 1.0 {
        return None;
    }
    let distance = edge2.dot(cross_delta_edge) * inverse;
    (distance >= 0.0 && distance.is_finite()).then_some(distance)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GizmoOperation {
    Translate,
    Rotate,
    Scale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GizmoSpace {
    Local,
    World,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GizmoAxis {
    X,
    Y,
    Z,
}

impl GizmoAxis {
    pub const fn vector(self) -> Vec3 {
        match self {
            Self::X => Vec3::X,
            Self::Y => Vec3::Y,
            Self::Z => Vec3::Z,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GizmoSettings {
    pub operation: GizmoOperation,
    pub space: GizmoSpace,
    pub snapping: bool,
    pub translation_snap: f32,
    pub rotation_snap_radians: f32,
    pub scale_snap: f32,
}

impl Default for GizmoSettings {
    fn default() -> Self {
        Self {
            operation: GizmoOperation::Translate,
            space: GizmoSpace::World,
            snapping: false,
            translation_snap: 0.5,
            rotation_snap_radians: 15.0_f32.to_radians(),
            scale_snap: 0.1,
        }
    }
}

pub fn apply_gizmo_delta(
    before: LocalTransform,
    axis: GizmoAxis,
    amount: f32,
    settings: GizmoSettings,
) -> Option<LocalTransform> {
    if !amount.is_finite() || !before.is_finite() {
        return None;
    }
    let snap = |value: f32, step: f32| {
        if settings.snapping && step.is_finite() && step > 0.0 {
            (value / step).round() * step
        } else {
            value
        }
    };
    let mut after = before;
    let local_axis = axis.vector();
    let world_axis = if settings.space == GizmoSpace::Local {
        before.rotation * local_axis
    } else {
        local_axis
    };
    match settings.operation {
        GizmoOperation::Translate => {
            after.translation += world_axis * snap(amount, settings.translation_snap);
        }
        GizmoOperation::Rotate => {
            let angle = snap(amount, settings.rotation_snap_radians);
            let delta = Quat::from_axis_angle(local_axis, angle);
            after.rotation = if settings.space == GizmoSpace::Local {
                (before.rotation * delta).normalize()
            } else {
                (Quat::from_axis_angle(world_axis, angle) * before.rotation).normalize()
            };
        }
        GizmoOperation::Scale => {
            let factor = snap(1.0 + amount, settings.scale_snap).max(0.001);
            after.scale *= Vec3::ONE + local_axis * (factor - 1.0);
        }
    }
    after.is_finite().then_some(after)
}

/// Generates deterministic XZ grid line endpoints.
pub fn grid_lines(half_extent: i16, spacing: f32) -> Vec<[f32; 3]> {
    let half_extent = half_extent.max(0);
    let count = usize::from(u16::try_from(half_extent).unwrap_or_default());
    let mut output = Vec::with_capacity((count * 2 + 1) * 4);
    let extent = f32::from(half_extent) * spacing;
    for line in -half_extent..=half_extent {
        let value = f32::from(line) * spacing;
        output.extend([
            [-extent, 0.0, value],
            [extent, 0.0, value],
            [value, 0.0, -extent],
            [value, 0.0, extent],
        ]);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_navigation_and_matrices_are_finite() {
        let mut camera = EditorCamera::default();
        assert!(camera.view_projection(16.0 / 9.0).is_finite());
        let original = camera;
        camera.orbit(Vec2::new(20.0, -10.0));
        assert_ne!(camera.position(), original.position());
        camera.pan(Vec2::new(10.0, 4.0), 720.0);
        camera.dolly(2.0);
        camera.fly(Vec3::new(1.0, 1.0, 1.0), 0.5, 3.0);
        camera.frame_sphere(Vec3::new(4.0, 5.0, 6.0), 2.0);
        assert_eq!(camera.focus, Vec3::new(4.0, 5.0, 6.0));
        camera.reset();
        assert_eq!(camera, original);
    }

    #[test]
    fn viewport_center_ray_points_at_focus() {
        let camera = EditorCamera::default();
        let ray = camera
            .world_ray(Vec2::new(400.0, 300.0), Vec2::new(800.0, 600.0))
            .unwrap();
        assert!(ray.direction.dot(camera.forward()) > 0.999);
        assert!(camera.world_ray(Vec2::ZERO, Vec2::ZERO).is_none());
    }

    #[test]
    fn picking_uses_transforms_and_nearest_depth() {
        let entity_near = EntityId::new();
        let entity_far = EntityId::new();
        let triangle = |entity, z| PickMesh {
            entity,
            transform: Mat4::from_translation(Vec3::new(0.0, 0.0, z)),
            positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]],
            indices: vec![0, 1, 2],
        };
        let ray = WorldRay {
            origin: Vec3::new(0.0, 0.0, -5.0),
            direction: Vec3::Z,
        };
        assert_eq!(
            pick_meshes(
                ray,
                &[triangle(entity_far, 2.0), triangle(entity_near, 0.0)]
            ),
            Some(entity_near)
        );
    }

    #[test]
    fn gizmo_local_world_snap_and_invalid_values() {
        let before = LocalTransform {
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            ..LocalTransform::IDENTITY
        };
        let mut settings = GizmoSettings {
            snapping: true,
            ..GizmoSettings::default()
        };
        let world = apply_gizmo_delta(before, GizmoAxis::X, 0.74, settings).unwrap();
        assert_eq!(world.translation, Vec3::new(0.5, 0.0, 0.0));
        settings.space = GizmoSpace::Local;
        let local = apply_gizmo_delta(before, GizmoAxis::X, 0.74, settings).unwrap();
        assert!((local.translation - Vec3::new(0.0, 0.0, -0.5)).length() < 1.0e-5);
        assert!(apply_gizmo_delta(before, GizmoAxis::X, f32::NAN, settings).is_none());
    }

    #[test]
    fn grid_has_expected_center_axes() {
        let grid = grid_lines(1, 1.0);
        assert_eq!(grid.len(), 12);
        assert!(grid.contains(&[-1.0, 0.0, 0.0]));
        assert!(grid.contains(&[1.0, 0.0, 0.0]));
    }
}
