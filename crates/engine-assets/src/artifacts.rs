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

/// One import may yield several meshes/materials while retaining one source ID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelArtifact {
    /// Static mesh primitives.
    pub meshes: Vec<StaticMeshArtifact>,
    /// Baseline materials.
    pub materials: Vec<PbrMaterialArtifact>,
    /// External source URIs that must resolve through the asset VFS.
    pub external_dependencies: Vec<String>,
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
