//! Engine-owned imported artifact types.

use engine_core::AssetId;
use serde::{Deserialize, Serialize};

/// Texture pixels retained in a backend-independent representation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TexturePixels {
    /// Eight-bit RGBA, suitable for standard color/data textures.
    Rgba8(Vec<u8>),
    /// Linear 32-bit float RGBA, used by HDR sources.
    Rgba32Float(Vec<f32>),
}

/// Imported texture resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextureArtifact {
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// Whether RGB channels are authored in sRGB color space.
    pub srgb: bool,
    /// Decoded, bounded pixels.
    pub pixels: TexturePixels,
}

/// Imported static mesh resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StaticMeshArtifact {
    #[serde(default)]
    pub material_index: Option<usize>,
    #[serde(default)]
    pub source_node: Option<usize>,
    #[serde(default)]
    pub skin: Option<ModelSkin>,
    #[serde(default)]
    pub joints: Vec<[u16; 4]>,
    #[serde(default)]
    pub weights: Vec<[f32; 4]>,
    /// Vertex positions.
    pub positions: Vec<[f32; 3]>,
    /// Vertex normals, when present.
    pub normals: Vec<[f32; 3]>,
    /// First UV channel, when present.
    pub texcoords: Vec<[f32; 2]>,
    /// Triangle-list indices.
    pub indices: Vec<u32>,
}

/// Baseline physically based material imported from glTF/MTL data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PbrMaterialArtifact {
    /// Linear base color and alpha.
    pub base_color: [f32; 4],
    /// Metallic factor.
    pub metallic: f32,
    /// Perceptual roughness factor.
    pub roughness: f32,
    /// Optional base-color texture source URI until dependency ID resolution.
    pub base_color_texture_uri: Option<String>,
}

/// Decoded floating-point PCM audio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioArtifact {
    /// Interleaved channel count.
    pub channels: u16,
    /// Samples per second.
    pub sample_rate: u32,
    /// Interleaved normalized samples.
    pub samples: Vec<f32>,
}

/// Validated font metadata. Glyph rasterization remains a renderer/UI concern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontArtifact {
    /// Number of glyphs declared by the face.
    pub glyph_count: u16,
    /// Font units per em.
    pub units_per_em: u16,
    /// Face index in a collection.
    pub face_index: u32,
}

/// Linear skin palette in source node order, with column-major inverse binds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelSkin {
    pub joints: Vec<usize>,
    pub inverse_bind: Vec<[f32; 16]>,
}

/// Source model node; indexes remain stable across clip and skin data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelNode {
    pub name: String,
    pub parent: Option<usize>,
    pub translation: [f64; 3],
    pub rotation: [f64; 4],
    pub scale: [f64; 3],
}

/// One import may yield several meshes/materials while retaining one source ID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelArtifact {
    /// Imported animation clips with node-relative tracks.
    #[serde(default)]
    pub animations: Vec<engine_core::gameplay::animation::Clip>,
    /// Source node hierarchy, retained for animation binding and skin evaluation.
    #[serde(default)]
    pub nodes: Vec<ModelNode>,
    /// Rigid or skinned mesh primitives.
    pub meshes: Vec<StaticMeshArtifact>,
    /// Baseline materials.
    pub materials: Vec<PbrMaterialArtifact>,
    /// External source URIs that must resolve through the asset VFS.
    pub external_dependencies: Vec<String>,
}

impl ModelArtifact {
    /// Validate the hierarchy and skin palette before binding or rendering.
    /// # Errors
    /// Returns an error for invalid transforms, hierarchy, skin palettes, or size limits.
    pub fn validate(&self) -> Result<(), crate::AssetError> {
        let invalid = |message: &str| crate::AssetError::Decode(message.into());
        if self.nodes.len() > 65_536 || self.animations.len() > 4096 {
            return Err(invalid("model node/clip limit exceeded"));
        }
        let mut colors = vec![0u8; self.nodes.len()];
        for (index, node) in self.nodes.iter().enumerate() {
            if node
                .translation
                .iter()
                .chain(&node.rotation)
                .chain(&node.scale)
                .any(|v| !v.is_finite() || v.abs() > f64::from(f32::MAX))
                || node.rotation.iter().map(|v| v * v).sum::<f64>() < 1e-20
            {
                return Err(invalid("model node has invalid transform"));
            }
            let mut chain = Vec::new();
            let mut current = Some(index);
            while let Some(i) = current {
                let Some(node) = self.nodes.get(i) else {
                    return Err(invalid("model parent outside node palette"));
                };
                if colors[i] == 2 {
                    break;
                }
                if colors[i] == 1 {
                    return Err(invalid("model hierarchy contains a cycle"));
                }
                colors[i] = 1;
                chain.push(i);
                current = node.parent;
            }
            for i in chain {
                colors[i] = 2;
            }
        }
        for clip in &self.animations {
            clip.validate().map_err(crate::AssetError::Decode)?;
        }
        for mesh in &self.meshes {
            if mesh.source_node.is_some_and(|i| i >= self.nodes.len()) {
                return Err(invalid("mesh source node outside palette"));
            }
            if let Some(skin) = &mesh.skin {
                if skin.joints.is_empty()
                    || skin.joints.len() != skin.inverse_bind.len()
                    || skin.joints.iter().any(|i| *i >= self.nodes.len())
                    || skin.inverse_bind.iter().flatten().any(|v| !v.is_finite())
                    || mesh.joints.len() != mesh.positions.len()
                    || mesh.weights.len() != mesh.positions.len()
                {
                    return Err(invalid("invalid skin palette or vertex weights"));
                }
                for (joints, weights) in mesh.joints.iter().zip(&mesh.weights) {
                    if weights.iter().any(|v| !v.is_finite() || *v < 0.0)
                        || joints
                            .iter()
                            .zip(weights)
                            .any(|(j, w)| *w > 0.0 && usize::from(*j) >= skin.joints.len())
                    {
                        return Err(invalid("invalid skin influence"));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Typed imported output stored in the derived-data cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DerivedArtifact {
    /// Raster texture.
    Texture(TextureArtifact),
    /// Static model, possibly with multiple primitives/materials.
    Model(ModelArtifact),
    /// Audio clip.
    Audio(AudioArtifact),
    /// Font face.
    Font(FontArtifact),
    /// Deterministic placeholder used before a first successful import.
    Placeholder(PlaceholderArtifact),
}

/// Placeholder/error resource used without blocking the editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceholderArtifact {
    /// Requested stable identity.
    pub asset_id: AssetId,
    /// Expected broad type.
    pub kind: AssetKind,
    /// Actionable failure context, if import failed.
    pub reason: Option<String>,
}

/// Broad runtime asset family used by handles and placeholders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    /// Texture source.
    Texture,
    /// Static mesh/model source.
    Model,
    /// Audio source.
    Audio,
    /// Font source.
    Font,
}

impl DerivedArtifact {
    /// Broad family for runtime type checks.
    pub const fn kind(&self) -> AssetKind {
        match self {
            Self::Texture(_) => AssetKind::Texture,
            Self::Model(_) => AssetKind::Model,
            Self::Audio(_) => AssetKind::Audio,
            Self::Font(_) => AssetKind::Font,
            Self::Placeholder(value) => value.kind,
        }
    }
}
