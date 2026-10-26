use eframe::egui;
use engine_editor::{EditorCamera, GizmoOperation, GizmoSettings, GizmoSpace};
use engine_world::{EntityId, LocalTransform};
use glam::{Mat4, Quat, Vec3};
use transform_gizmo_egui::{
    Gizmo, GizmoConfig, GizmoExt, GizmoMode, GizmoOrientation,
    math::{DMat4, DQuat, DVec3, Transform},
};

#[derive(Clone, Copy)]
pub struct Target {
    pub entity: EntityId,
    pub world: Mat4,
    pub parent: Mat4,
}

#[derive(Default)]
pub struct TransformWidget {
    gizmo: Gizmo,
    entity: Option<EntityId>,
}

impl TransformWidget {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn is_focused(&self) -> bool {
        self.gizmo.is_focused()
    }

    pub fn interact(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        camera: EditorCamera,
        target: Target,
        settings: GizmoSettings,
    ) -> Option<LocalTransform> {
        let Target {
            entity,
            world,
            parent,
        } = target;
        if self.entity != Some(entity) {
            self.reset();
            self.entity = Some(entity);
        }
        if !world.is_finite() || !parent.inverse().is_finite() {
            return None;
        }
        let (scale, rotation, translation) = world.to_scale_rotation_translation();
        self.gizmo.update_config(GizmoConfig {
            view_matrix: DMat4::from_cols_array(
                &camera.view_matrix().to_cols_array().map(f64::from),
            )
            .into(),
            projection_matrix: DMat4::from_cols_array(
                &camera
                    .projection_matrix(rect.aspect_ratio())
                    .to_cols_array()
                    .map(f64::from),
            )
            .into(),
            viewport: rect,
            modes: match settings.operation {
                GizmoOperation::Translate => GizmoMode::all_translate(),
                GizmoOperation::Rotate => GizmoMode::all_rotate(),
                GizmoOperation::Scale => GizmoMode::all_scale(),
            },
            orientation: match settings.space {
                GizmoSpace::World => GizmoOrientation::Global,
                GizmoSpace::Local => GizmoOrientation::Local,
            },
            snapping: settings.snapping,
            snap_angle: settings.rotation_snap_radians.max(0.001),
            snap_distance: settings.translation_snap.max(0.001),
            snap_scale: settings.scale_snap.max(0.001),
            ..Default::default()
        });
        let target = Transform::from_scale_rotation_translation(
            DVec3::from_array(scale.to_array().map(f64::from)),
            DQuat::from_array(rotation.to_array().map(f64::from)),
            DVec3::from_array(translation.to_array().map(f64::from)),
        );
        let (_, transforms) = self.gizmo.interact(ui, &[target])?;
        let after = transforms.first()?;
        let translation = Vec3::from_array(DVec3::from(after.translation).as_vec3().to_array());
        let result_world = if settings.operation == GizmoOperation::Translate {
            // Preserve the original linear transform, including parent-induced
            // shear, when only moving the pivot.
            let mut moved = world;
            moved.w_axis = translation.extend(1.0);
            moved
        } else {
            Mat4::from_scale_rotation_translation(
                Vec3::from_array(DVec3::from(after.scale).as_vec3().to_array()),
                Quat::from_array(DQuat::from(after.rotation).as_quat().to_array()),
                translation,
            )
        };
        local_from_world(parent, result_world)
    }
}

fn local_from_world(parent: Mat4, world: Mat4) -> Option<LocalTransform> {
    let local = parent.inverse() * world;
    if !local.is_finite() {
        return None;
    }
    let (scale, rotation, translation) = local.to_scale_rotation_translation();
    let result = LocalTransform {
        scale,
        rotation,
        translation,
    };
    // A TRS-only scene cannot represent shear from a rotated, nonuniform parent.
    // Reject it rather than silently corrupting the authored local transform.
    (result.is_finite() && result.matrix().abs_diff_eq(local, 0.001)).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_handle_owns_pointer_and_translates_in_camera_projection() {
        let context = egui::Context::default();
        let mut widget = TransformWidget::default();
        let entity = EntityId::new();
        let camera = EditorCamera {
            yaw: 0.0,
            pitch: 0.0,
            ..Default::default()
        };
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let frame = |widget: &mut TransformWidget, events| {
            let mut result = None;
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.allocate_rect(rect, egui::Sense::click_and_drag());
                    result = widget.interact(
                        ui,
                        rect,
                        camera,
                        Target {
                            entity,
                            world: Mat4::IDENTITY,
                            parent: Mat4::IDENTITY,
                        },
                        GizmoSettings::default(),
                    );
                },
            );
            result
        };
        let start = egui::pos2(440.0, 300.0);
        frame(&mut widget, vec![egui::Event::PointerMoved(start)]);
        frame(&mut widget, vec![]);
        frame(
            &mut widget,
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(widget.is_focused());
        let after = frame(
            &mut widget,
            vec![egui::Event::PointerMoved(egui::pos2(470.0, 300.0))],
        )
        .expect("the upstream X handle should produce a transform");
        assert!(after.translation.x > 0.0);
        assert!(after.translation.y.abs() < 0.001);
        assert!(after.translation.z.abs() < 0.001);
        widget.reset();
        assert!(!widget.is_focused());
    }

    #[test]
    fn parented_world_transform_round_trips_to_local() {
        let parent = Mat4::from_scale_rotation_translation(
            Vec3::splat(2.0),
            Quat::from_rotation_y(0.7),
            Vec3::new(3.0, 4.0, 5.0),
        );
        let local = LocalTransform {
            translation: Vec3::new(1.0, 2.0, 3.0),
            rotation: Quat::from_rotation_x(0.3),
            scale: Vec3::splat(0.5),
        };
        let actual = local_from_world(parent, parent * local.matrix()).unwrap();
        assert!(actual.matrix().abs_diff_eq(local.matrix(), 0.0001));
    }

    #[test]
    fn singular_parents_and_sheared_results_are_rejected() {
        assert!(local_from_world(Mat4::from_scale(Vec3::ZERO), Mat4::IDENTITY).is_none());
        let parent = Mat4::from_scale(Vec3::new(2.0, 1.0, 1.0));
        assert!(local_from_world(parent, Mat4::from_rotation_z(0.7)).is_none());
    }
}
