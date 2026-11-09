//! Reusable, renderer-independent PBR materials with stable texture identities.
use crate::AssetError;
use engine_core::AssetId;
use engine_world::{SurfaceTextures, TextureMap};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A project `.material` source. Texture slots use `.rmeta` identities rather
/// than renderer handles, and retain their meanings across import and export.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialAsset {
    pub version: u32,
    pub name: String,
    pub base_color: [f32; 4],
    pub roughness: f32,
    pub metallic: f32,
    pub emissive_color: [f32; 3],
    pub emissive_strength: f32,
    pub normal_scale: f32,
    pub occlusion_strength: f32,
    pub uv_scale: [f32; 2],
    pub uv_offset: [f32; 2],
    pub height_scale: f32,
    pub alpha_cutoff: Option<f32>,
    pub maps: BTreeMap<TextureMap, AssetId>,
    pub orm_map: Option<AssetId>,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
    /// Opt-in vertex bending strength for compatible wind zones.
    pub wind_response: f32,
}
impl Default for MaterialAsset {
    fn default() -> Self {
        Self {
            version: 1,
            name: "Material".into(),
            base_color: [1.0; 4],
            roughness: 0.5,
            metallic: 0.0,
            emissive_color: [0.0; 3],
            emissive_strength: 1.0,
            normal_scale: 1.0,
            occlusion_strength: 1.0,
            uv_scale: [1.0; 2],
            uv_offset: [0.0; 2],
            height_scale: 0.05,
            alpha_cutoff: None,
            maps: BTreeMap::new(),
            orm_map: None,
            cast_shadows: true,
            receive_shadows: true,
            wind_response: 0.0,
        }
    }
}
impl MaterialAsset {
    /// Validates material controls before persistence or GPU resource creation.
    /// # Errors
    /// Rejects incompatible versions, unbounded names and invalid PBR values.
    pub fn validate(&self) -> Result<(), AssetError> {
        if self.version != 1 || self.name.trim().is_empty() || self.name.len() > 256 {
            return Err(AssetError::InvalidData(
                "material requires version1 and a name of1–256 bytes".into(),
            ));
        }
        let controls = SurfaceTextures {
            base_color: self.base_color,
            roughness: Some(self.roughness),
            metallic: Some(self.metallic),
            emissive_color: self.emissive_color,
            emissive_strength: self.emissive_strength,
            normal_scale: self.normal_scale,
            occlusion_strength: self.occlusion_strength,
            uv_scale: self.uv_scale,
            uv_offset: self.uv_offset,
            height_scale: self.height_scale,
            alpha_cutoff: self.alpha_cutoff,
            pbr: true,
            wind_response: Some(self.wind_response),
            ..SurfaceTextures::default()
        };
        controls.validate().map_err(AssetError::InvalidData)
    }
    /// Encodes a validated source suitable for project assets and source control.
    /// # Errors
    /// Returns schema, value or RON encoding errors.
    pub fn to_bytes(&self) -> Result<Vec<u8>, AssetError> {
        self.validate()?;
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map(String::into_bytes)
            .map_err(|error| AssetError::InvalidData(error.to_string()))
    }
    /// Reads the bounded, versioned source.
    /// # Errors
    /// Returns a diagnostic for oversized, malformed or invalid material data.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AssetError> {
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(AssetError::Limit("material source exceeds4MiB".into()));
        }
        let value: Self = ron::de::from_bytes(bytes)
            .map_err(|error| AssetError::InvalidData(error.to_string()))?;
        value.validate()?;
        Ok(value)
    }
    /// Stable source dependencies for cache invalidation and packaging.
    pub fn dependencies(&self) -> Vec<AssetId> {
        let mut ids: Vec<_> = self
            .maps
            .values()
            .chain(self.orm_map.iter())
            .copied()
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DerivedArtifact, ImportRequest, ImporterRegistry};
    #[test]
    fn typed_material_import_roundtrips_stable_maps_and_rejects_invalid_controls() {
        let normal = AssetId::new();
        let orm = AssetId::new();
        let material = MaterialAsset {
            name: "Copper".into(),
            metallic: 1.0,
            roughness: 0.2,
            maps: [(TextureMap::Normal, normal)].into_iter().collect(),
            orm_map: Some(orm),
            ..MaterialAsset::default()
        };
        let bytes = material.to_bytes().unwrap();
        let artifact =
            ImporterRegistry::import(&ImportRequest::new("assets/copper.material", bytes)).unwrap();
        assert_eq!(artifact, DerivedArtifact::Material(material.clone()));
        assert_eq!(material.dependencies().len(), 2);
        let mut invalid = material;
        invalid.normal_scale = f32::NAN;
        assert!(invalid.to_bytes().is_err());
        assert!(MaterialAsset::from_bytes(b"(version:99,name:\"Future\")").is_err());
    }
}
