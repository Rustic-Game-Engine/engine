use serde::{Deserialize, Serialize};

/// Scene-owned sky and atmosphere. Image paths are relative to the project root.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SceneEnvironment {
    pub enabled: bool,
    /// A 2:1 equirectangular HDR, PNG, JPEG or TGA panorama; empty uses `sky_color`.
    pub sky_image: String,
    pub sky_color: [f32; 3],
    pub rotation_degrees: f32,
    /// Exposure in stops, applied to the sky image.
    pub exposure: f32,
    pub ambient_color: [f32; 3],
    pub ambient_intensity: f32,
    pub sun_color: [f32; 3],
    pub sun_intensity: f32,
    /// Direction from the surface towards the sun.
    pub sun_direction: [f32; 3],
    pub haze_color: [f32; 3],
    pub haze_density: f32,
    pub haze_start: f32,
}

impl Default for SceneEnvironment {
    fn default() -> Self {
        Self {
            enabled: false,
            sky_image: String::new(),
            sky_color: [0.045, 0.06, 0.085],
            rotation_degrees: 0.0,
            exposure: 0.0,
            ambient_color: [1.0; 3],
            ambient_intensity: 0.12,
            sun_color: [1.0, 0.95, 0.85],
            sun_intensity: 0.0,
            sun_direction: [0.3, 0.8, 0.4],
            haze_color: [0.6, 0.7, 0.8],
            haze_density: 0.0,
            haze_start: 0.0,
        }
    }
}

impl SceneEnvironment {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    /// Reject non-finite settings, negative energy/distances and unsafe image paths.
    pub fn is_valid(&self) -> bool {
        let colors = [
            self.sky_color,
            self.ambient_color,
            self.sun_color,
            self.haze_color,
        ];
        let path = &self.sky_image;
        colors
            .into_iter()
            .flatten()
            .all(|v| v.is_finite() && v >= 0.0)
            && [
                self.ambient_intensity,
                self.sun_intensity,
                self.haze_density,
                self.haze_start,
            ]
            .into_iter()
            .all(|v| v.is_finite() && v >= 0.0)
            && self.rotation_degrees.is_finite()
            && self.exposure.is_finite()
            && (-20.0..=20.0).contains(&self.exposure)
            && self.sun_direction.into_iter().all(f32::is_finite)
            && glam::Vec3::from_array(self.sun_direction)
                .length_squared()
                .is_finite()
            && glam::Vec3::from_array(self.sun_direction).length_squared() > 0.000_001
            && !path.contains(['\\', ':', '\0'])
            && !path.starts_with('/')
            && (path.is_empty()
                || path
                    .split('/')
                    .all(|s| !s.is_empty() && s != "." && s != ".."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SceneDocument, SceneEdit, SceneWorld, UndoStack, WorldCommand, load_scene};

    #[test]
    fn environment_survives_save_load_world_capture_and_undo() {
        let mut world = SceneWorld::new();
        let env = SceneEnvironment {
            enabled: true,
            sky_image: "assets/sky.hdr".into(),
            sun_intensity: 2.0,
            haze_density: 0.03,
            haze_start: 4.0,
            ..Default::default()
        };
        let mut undo = UndoStack::new();
        undo.execute(
            &mut world,
            SceneEdit::Environment {
                before: SceneEnvironment::default(),
                after: env.clone(),
            },
        )
        .unwrap();
        let document = SceneDocument::from_world(crate::SceneId::new(), "Sky", &world).unwrap();
        let loaded = load_scene(&document.to_bytes().unwrap()).unwrap();
        assert!(!loaded.read_only);
        assert_eq!(loaded.document.environment, env);
        assert_eq!(loaded.document.create_world().unwrap().environment(), &env);
        undo.undo(&mut world).unwrap();
        assert_eq!(world.environment(), &SceneEnvironment::default());
        undo.redo(&mut world).unwrap();
        assert_eq!(world.environment(), &env);
    }

    #[test]
    fn legacy_scenes_keep_their_checksum_and_default_environment() {
        let document = SceneDocument::new("Legacy");
        let bytes = document.to_bytes().unwrap();
        assert!(
            !String::from_utf8(bytes.clone())
                .unwrap()
                .contains("environment:")
        );
        let loaded = load_scene(&bytes).unwrap();
        assert!(!loaded.read_only);
        assert_eq!(loaded.document.environment, SceneEnvironment::default());
        assert_eq!(loaded.document.to_bytes().unwrap(), bytes);
    }

    #[test]
    fn invalid_environment_cannot_partially_commit_a_transaction() {
        let mut world = SceneWorld::new();
        for env in [
            SceneEnvironment {
                haze_density: f32::NAN,
                ..Default::default()
            },
            SceneEnvironment {
                sky_image: "../sky.hdr".into(),
                ..Default::default()
            },
            SceneEnvironment {
                sky_image: "C:/sky.hdr".into(),
                ..Default::default()
            },
            SceneEnvironment {
                sun_direction: [0.0; 3],
                ..Default::default()
            },
        ] {
            let spawn = crate::EntitySnapshot::default();
            let id = spawn.id;
            assert!(
                world
                    .apply_commands(&[
                        WorldCommand::Spawn(Box::new(spawn)),
                        WorldCommand::SetEnvironment(env)
                    ])
                    .is_err()
            );
            assert!(!world.contains(id));
            assert_eq!(world.environment(), &SceneEnvironment::default());
        }
    }
}
