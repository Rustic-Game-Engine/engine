//! Optional surface maps shared by authoring, persistence and rendering.
use bevy_ecs::prelude::Component;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable layer order used by the GPU material atlas.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum TextureMap {
    BaseColor,
    Normal,
    Roughness,
    Metallic,
    AmbientOcclusion,
    Height,
    Emissive,
    Clearcoat,
    ClearcoatRoughness,
    ClearcoatNormal,
    Transmission,
    Thickness,
    SheenColor,
    SheenRoughness,
    Specular,
    SpecularColor,
    Anisotropy,
}
impl TextureMap {
    pub const ALL: [Self; 17] = [
        Self::BaseColor,
        Self::Normal,
        Self::Roughness,
        Self::Metallic,
        Self::AmbientOcclusion,
        Self::Height,
        Self::Emissive,
        Self::Clearcoat,
        Self::ClearcoatRoughness,
        Self::ClearcoatNormal,
        Self::Transmission,
        Self::Thickness,
        Self::SheenColor,
        Self::SheenRoughness,
        Self::Specular,
        Self::SpecularColor,
        Self::Anisotropy,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::BaseColor => "Base Color (Albedo)",
            Self::Normal => "Normal",
            Self::Roughness => "Roughness",
            Self::Metallic => "Metallic",
            Self::AmbientOcclusion => "Ambient Occlusion (AO)",
            Self::Height => "Height / Displacement",
            Self::Emissive => "Emissive",
            Self::Clearcoat => "Clearcoat",
            Self::ClearcoatRoughness => "Clearcoat Roughness",
            Self::ClearcoatNormal => "Clearcoat Normal",
            Self::Transmission => "Transmission",
            Self::Thickness => "Thickness",
            Self::SheenColor => "Sheen Color",
            Self::SheenRoughness => "Sheen Roughness",
            Self::Specular => "Specular",
            Self::SpecularColor => "Specular Color",
            Self::Anisotropy => "Anisotropy",
        }
    }
    pub fn is_color(self) -> bool {
        matches!(
            self,
            Self::BaseColor | Self::Emissive | Self::SheenColor | Self::SpecularColor
        )
    }
    /// Infer common PBR filenames; unlabelled images are albedo maps.
    pub fn from_filename(path: &std::path::Path) -> Self {
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase()
            .replace(['-', ' '], "_");
        if name == "ao" {
            return Self::AmbientOcclusion;
        }
        for (tokens, kind) in [
            (
                &["clearcoatroughness", "clearcoat_roughness"][..],
                Self::ClearcoatRoughness,
            ),
            (
                &["clearcoatnormal", "clearcoat_normal"][..],
                Self::ClearcoatNormal,
            ),
            (
                &["sheenroughness", "sheen_roughness"][..],
                Self::SheenRoughness,
            ),
            (
                &["specularcolor", "specular_color"][..],
                Self::SpecularColor,
            ),
            (&["normal", "_nor", "_nrm"][..], Self::Normal),
            (&["rough"][..], Self::Roughness),
            (&["metal"][..], Self::Metallic),
            (&["occlusion", "_ao"][..], Self::AmbientOcclusion),
            (&["height", "displacement", "_disp"][..], Self::Height),
            (&["emiss", "glow"][..], Self::Emissive),
            (&["clearcoat"][..], Self::Clearcoat),
            (&["transmission"][..], Self::Transmission),
            (&["thickness"][..], Self::Thickness),
            (&["sheen"][..], Self::SheenColor),
            (&["specular"][..], Self::Specular),
            (&["anisotrop"][..], Self::Anisotropy),
        ] {
            if tokens.iter().any(|token| name.contains(token)) {
                return kind;
            }
        }
        Self::BaseColor
    }
}

/// A texture component on a mesh or a non-rendering Texture child entity.
/// Map paths are project-relative; absent slots retain neutral material defaults.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SurfaceTextures {
    pub maps: BTreeMap<TextureMap, String>,
    pub uv_scale: [f32; 2],
    pub uv_offset: [f32; 2],
    pub height_scale: f32,
    pub emissive_strength: f32,
}
impl Default for SurfaceTextures {
    fn default() -> Self {
        Self {
            maps: BTreeMap::new(),
            uv_scale: [1.0; 2],
            uv_offset: [0.0; 2],
            height_scale: 0.05,
            emissive_strength: 1.0,
        }
    }
}
impl SurfaceTextures {
    /// # Errors
    /// Rejects non-finite controls and paths that leave the project.
    pub fn validate(&self) -> Result<(), String> {
        if self
            .uv_scale
            .iter()
            .chain(&self.uv_offset)
            .chain([&self.height_scale, &self.emissive_strength])
            .any(|v| !v.is_finite())
            || self.height_scale.abs() > 100.0
            || !(0.0..=10000.0).contains(&self.emissive_strength)
        {
            return Err("Invalid texture scale, offset or intensity".into());
        }
        for path in self.maps.values() {
            if path.is_empty()
                || path.len() > 4096
                || path.contains('\\')
                || path.contains(':')
                || std::path::Path::new(path)
                    .components()
                    .any(|p| !matches!(p, std::path::Component::Normal(_)))
            {
                return Err("Texture paths must be relative to the project".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EntitySnapshot, Primitive, SceneDocument, SceneEdit, UndoStack, WorldCommand, load_scene,
    };
    #[test]
    fn texture_children_round_trip_resolve_reparent_and_undo() {
        let mut scene = SceneDocument::new("Textures");
        let a = EntitySnapshot {
            primitive: Some(Primitive::Cube { size: 1.0 }),
            ..Default::default()
        };
        let b = EntitySnapshot {
            primitive: Some(Primitive::Cube { size: 1.0 }),
            ..Default::default()
        };
        let mut maps = SurfaceTextures::default();
        for kind in TextureMap::ALL {
            maps.maps.insert(kind, format!("assets/{kind:?}.hdr"));
        }
        maps.uv_scale = [2.0, 3.0];
        maps.height_scale = 0.2;
        let child = EntitySnapshot {
            parent: Some(a.id),
            textures: Some(maps.clone()),
            ..Default::default()
        };
        scene.entities = vec![a.clone(), b.clone(), child.clone()];
        let loaded = load_scene(&scene.to_bytes().unwrap()).unwrap();
        assert!(!loaded.read_only);
        let mut world = loaded.document.create_world().unwrap();
        assert_eq!(world.resolved_textures(a.id), maps);
        assert!(world.resolved_textures(b.id).maps.is_empty());
        let mut undo = UndoStack::new();
        undo.execute(
            &mut world,
            SceneEdit::Reparent {
                entity: child.id,
                before: Some(a.id),
                after: Some(b.id),
            },
        )
        .unwrap();
        assert!(world.resolved_textures(a.id).maps.is_empty());
        assert_eq!(world.resolved_textures(b.id), maps);
        undo.undo(&mut world).unwrap();
        assert_eq!(world.resolved_textures(a.id), maps);
        undo.redo(&mut world).unwrap();
        assert_eq!(world.resolved_textures(b.id), maps);
        undo.execute(
            &mut world,
            SceneEdit::Textures {
                entity: child.id,
                before: Some(maps.clone()),
                after: None,
            },
        )
        .unwrap();
        assert!(world.resolved_textures(b.id).maps.is_empty());
        undo.undo(&mut world).unwrap();
        assert_eq!(world.resolved_textures(b.id), maps);
        // Invalid texture data must reject the whole batch, including earlier edits.
        let mut invalid = maps;
        invalid
            .maps
            .insert(TextureMap::BaseColor, "../outside.png".into());
        assert!(
            world
                .apply_commands(&[
                    WorldCommand::SetName {
                        entity: a.id,
                        value: Some("changed".into())
                    },
                    WorldCommand::SetTextures {
                        entity: child.id,
                        value: Some(invalid)
                    }
                ])
                .is_err()
        );
        assert_eq!(world.snapshot(a.id).unwrap().name, a.name);
    }
    #[test]
    fn optional_slots_preserve_direct_maps_and_reject_unsafe_paths() {
        let mut scene = SceneDocument::new("Overrides");
        let mut direct = SurfaceTextures::default();
        direct
            .maps
            .insert(TextureMap::BaseColor, "assets/color.png".into());
        let owner = EntitySnapshot {
            textures: Some(direct.clone()),
            ..Default::default()
        };
        let empty = EntitySnapshot {
            parent: Some(owner.id),
            textures: Some(SurfaceTextures::default()),
            ..Default::default()
        };
        scene.entities = vec![owner.clone(), empty];
        assert_eq!(
            scene
                .create_world()
                .unwrap()
                .resolved_textures(owner.id)
                .maps,
            direct.maps
        );
        for bad in [
            "",
            "/tmp/file.png",
            "../file.png",
            "C:/file.png",
            "assets/../file.png",
            "assets\\file.png",
        ] {
            direct.maps.insert(TextureMap::Normal, bad.into());
            assert!(direct.validate().is_err(), "accepted {bad}");
        }
        assert_eq!(
            TextureMap::from_filename(Path::new("paint_clearcoat_roughness.png")),
            TextureMap::ClearcoatRoughness
        );
        assert_eq!(
            TextureMap::from_filename(Path::new("stone_normal.hdr")),
            TextureMap::Normal
        );
        assert_eq!(
            TextureMap::from_filename(Path::new("wall.hdr")),
            TextureMap::BaseColor
        );
    }
    use std::path::Path;
}
