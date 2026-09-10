use crate::{ProjectDescriptor, ProjectError, ProjectTemplate};
use engine_core::SceneId;
use engine_platform::AtomicFileService;
use engine_scripting::{
    GameSettings, ScriptApiVersion, ScriptId, ScriptLanguage, ScriptManifest, ScriptManifestEntry,
    generate_programming_workspace, save_manifest_atomic,
};
use engine_world::{
    Camera, CameraProjection, EntitySnapshot, Light, LightKind, LocalTransform, Primitive,
    SceneDocument, ScriptComponent, save_scene_atomic,
};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const CURRENT_TEMPLATE_MANIFEST_VERSION: u32 = 1;
pub const TEMPLATE_MANIFEST_FILE: &str = "project-template.ron";

/// Versioned record of the materialized project seed and startup scene.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TemplateManifest {
    pub format_version: u32,
    pub template: ProjectTemplate,
    pub startup_scene: Option<PathBuf>,
}

pub(crate) fn materialize_template(
    staging_root: &Path,
    descriptor: &ProjectDescriptor,
) -> Result<(), ProjectError> {
    let example_script = (descriptor.template != ProjectTemplate::Blank).then(ScriptId::new);
    let startup_scene = match descriptor.template {
        ProjectTemplate::Blank => None,
        ProjectTemplate::Empty2d | ProjectTemplate::Platformer | ProjectTemplate::TopDown => {
            Some(empty_2d_scene(example_script))
        }
        _ => Some(empty_3d_scene(example_script)),
    };
    let startup_relative = startup_scene
        .as_ref()
        .map(|_| PathBuf::from("scenes/main.rscene"));
    if let Some(scene) = startup_scene {
        save_scene_atomic(staging_root.join("scenes/main.rscene"), &scene)?;
    }
    if let Some(script_id) = example_script {
        let relative = PathBuf::from("scripts/example_behavior.lua");
        AtomicFileService::new().write(
            &staging_root.join(&relative),
            br#"-- This entry script runs from top to bottom when the game starts.
rustic.log("info", "Example game started")
return {
  fixed_update = function(dt)
    local x, y, z = rustic.get_translation()
    rustic.set_translation(x + dt, y, z)
  end,
  on_stop = function() rustic.log("info", "Example behavior stopped") end,
}
"#,
        )?;
        let manifest = ScriptManifest {
            format_version: engine_scripting::CURRENT_SCRIPT_MANIFEST_VERSION,
            scripts: vec![ScriptManifestEntry {
                id: script_id,
                language: ScriptLanguage::Lua54,
                relative_path: relative,
                api_version: ScriptApiVersion::CURRENT,
                public_properties: Vec::new(),
            }],
        };
        save_manifest_atomic(&staging_root.join("config/scripts.ron"), &manifest)
            .map_err(|error| ProjectError::TemplateManifest(error.to_string()))?;
    }
    if example_script.is_none() {
        AtomicFileService::new().write(
            &staging_root.join("scripts/main.lua"),
            b"-- This entry script runs from top to bottom when the game starts.\nreturn {}\n",
        )?;
    }
    let game_settings = GameSettings {
        entry_script: if example_script.is_some() {
            PathBuf::from("scripts/example_behavior.lua")
        } else {
            PathBuf::from("scripts/main.lua")
        },
    };
    game_settings
        .save(staging_root)
        .map_err(ProjectError::TemplateManifest)?;
    generate_programming_workspace(staging_root, &descriptor.metadata.name)
        .map_err(|error| ProjectError::TemplateManifest(error.to_string()))?;
    let manifest = TemplateManifest {
        format_version: CURRENT_TEMPLATE_MANIFEST_VERSION,
        template: descriptor.template,
        startup_scene: startup_relative,
    };
    let mut text =
        ron::ser::to_string_pretty(&manifest, ron::ser::PrettyConfig::new().depth_limit(8))
            .map_err(|error| ProjectError::TemplateManifest(error.to_string()))?;
    text.push('\n');
    AtomicFileService::new().write(
        &staging_root.join("config").join(TEMPLATE_MANIFEST_FILE),
        text.as_bytes(),
    )?;
    Ok(())
}

fn empty_3d_scene(example_script: Option<ScriptId>) -> SceneDocument {
    let mut scene = SceneDocument {
        id: SceneId::new(),
        name: "Main".to_owned(),
        entities: Vec::new(),
        instances: Vec::new(),
    };
    scene.entities.push(EntitySnapshot {
        name: Some("Camera".to_owned()),
        local_transform: LocalTransform {
            translation: Vec3::new(0.0, 1.5, -5.0),
            ..LocalTransform::IDENTITY
        },
        camera: Some(Camera::default()),
        ..EntitySnapshot::default()
    });
    scene.entities.push(EntitySnapshot {
        name: Some("Directional Light".to_owned()),
        light: Some(Light {
            kind: LightKind::Directional,
            ..Light::default()
        }),
        ..EntitySnapshot::default()
    });
    scene.entities.push(EntitySnapshot {
        name: Some("Cube".to_owned()),
        primitive: Some(Primitive::Cube { size: 1.0 }),
        scripts: example_script
            .map(ScriptComponent::new)
            .into_iter()
            .collect(),
        ..EntitySnapshot::default()
    });
    scene
}

fn empty_2d_scene(example_script: Option<ScriptId>) -> SceneDocument {
    let mut scene = SceneDocument {
        id: SceneId::new(),
        name: "Main".to_owned(),
        entities: Vec::new(),
        instances: Vec::new(),
    };
    scene.entities.push(EntitySnapshot {
        name: Some("2D Camera".to_owned()),
        local_transform: LocalTransform {
            translation: Vec3::new(0.0, 0.0, -10.0),
            ..LocalTransform::IDENTITY
        },
        camera: Some(Camera {
            projection: CameraProjection::Orthographic {
                vertical_size: 10.0,
                near: 0.01,
                far: 100.0,
            },
            ..Camera::default()
        }),
        ..EntitySnapshot::default()
    });
    scene.entities.push(EntitySnapshot {
        name: Some("Rectangle".to_owned()),
        primitive: Some(Primitive::Rectangle2d { size: [2.0, 1.0] }),
        scripts: example_script
            .map(ScriptComponent::new)
            .into_iter()
            .collect(),
        ..EntitySnapshot::default()
    });
    scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Project;
    use engine_world::load_scene;
    use tempfile::tempdir;

    #[test]
    fn blank_and_empty_templates_materialize_real_versioned_content() {
        let temporary = tempdir().unwrap();
        let blank = Project::create(
            temporary.path().join("blank"),
            "Blank",
            ProjectTemplate::Blank,
        )
        .unwrap();
        let blank_manifest: TemplateManifest = ron::from_str(
            &std::fs::read_to_string(blank.root().join("config/project-template.ron")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            blank_manifest.format_version,
            CURRENT_TEMPLATE_MANIFEST_VERSION
        );
        assert!(blank_manifest.startup_scene.is_none());

        for template in [ProjectTemplate::Empty2d, ProjectTemplate::Empty3d] {
            let root = temporary.path().join(format!("{template:?}"));
            let project = Project::create(&root, format!("{template:?}"), template).unwrap();
            let bytes = std::fs::read(project.root().join("scenes/main.rscene")).unwrap();
            let scene = load_scene(&bytes).unwrap();
            assert!(!scene.document.entities.is_empty());
            assert!(
                scene
                    .document
                    .entities
                    .iter()
                    .any(|entity| entity.camera.is_some())
            );
            assert!(
                scene
                    .document
                    .entities
                    .iter()
                    .any(|entity| entity.primitive.is_some())
            );
            assert!(
                scene
                    .document
                    .entities
                    .iter()
                    .any(|entity| !entity.scripts.is_empty())
            );
            assert!(project.root().join("config/scripts.ron").is_file());
            assert!(
                project
                    .root()
                    .join("scripts/example_behavior.lua")
                    .is_file()
            );
        }
    }
}
