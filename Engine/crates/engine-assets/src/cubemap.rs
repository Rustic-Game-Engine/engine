//! Six explicitly oriented texture sources under one stable project asset identity.
use crate::{
    AssetError, DerivedArtifact, ImportRequest, ImporterRegistry, TextureArtifact,
    validate_asset_relative_path,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// `.cubemap` RON source. Faces use +X,-X,+Y,-Y,+Z,-Z order and project-relative
/// paths, so source control, play snapshots and exported projects retain them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CubemapAsset {
    pub version: u32,
    pub faces: [PathBuf; 6],
}
impl CubemapAsset {
    /// Checks the version and all six portable source references.
    /// # Errors
    /// Rejects unknown versions, unsafe paths, or missing filenames.
    pub fn validate(&self) -> Result<(), AssetError> {
        if self.version != 1 {
            return Err(AssetError::InvalidData("cubemap version must be 1".into()));
        }
        for face in &self.faces {
            validate_asset_relative_path(face)?;
        }
        Ok(())
    }
    /// Encodes the portable descriptor without copying image pixels.
    /// # Errors
    /// Returns descriptor validation or RON serialization errors.
    pub fn to_bytes(&self) -> Result<Vec<u8>, AssetError> {
        self.validate()?;
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map(String::into_bytes)
            .map_err(|e| AssetError::InvalidData(e.to_string()))
    }
    /// Decodes a bounded descriptor; actual image availability is checked on load.
    /// # Errors
    /// Rejects oversized, malformed, unsupported, or unsafe descriptors.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AssetError> {
        if bytes.len() > 64 * 1024 {
            return Err(AssetError::Limit("cubemap descriptor exceeds 64KiB".into()));
        }
        let value: Self =
            ron::de::from_bytes(bytes).map_err(|e| AssetError::InvalidData(e.to_string()))?;
        value.validate()?;
        Ok(value)
    }
    /// Resolves and decodes real face images using the existing bounded texture
    /// importers. All faces must be square, equally sized, and at most2048texels.
    /// # Errors
    /// Returns missing/escaped source paths, decoder errors or incompatible faces.
    pub fn decode_faces(&self, root: &Path) -> Result<[TextureArtifact; 6], AssetError> {
        self.validate()?;
        let root = root.canonicalize().map_err(|e| crate::io_error(root, e))?;
        let mut faces = Vec::with_capacity(6);
        let mut dimension = None;
        for source in &self.faces {
            let path = root
                .join(source)
                .canonicalize()
                .map_err(|e| crate::io_error(root.join(source), e))?;
            if !path.starts_with(&root) {
                return Err(AssetError::UnsafePath(source.clone()));
            }
            let metadata = std::fs::metadata(&path).map_err(|e| crate::io_error(&path, e))?;
            if metadata.len() > 64 * 1024 * 1024 {
                return Err(AssetError::Limit("cubemap face exceeds 64MiB".into()));
            }
            let bytes = std::fs::read(&path).map_err(|e| crate::io_error(&path, e))?;
            let DerivedArtifact::Texture(texture) =
                ImporterRegistry::import(&ImportRequest::new(source, bytes))?
            else {
                return Err(AssetError::InvalidData(
                    "cubemap faces must be texture sources".into(),
                ));
            };
            if texture.width != texture.height
                || texture.width > 2048
                || dimension.is_some_and(|size| size != texture.width)
            {
                return Err(AssetError::InvalidData(
                    "cubemap requires six equally sized square faces, maximum 2048 texels".into(),
                ));
            }
            dimension = Some(texture.width);
            faces.push(texture);
        }
        faces
            .try_into()
            .map_err(|_| AssetError::InvalidData("cubemap requires six faces".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_cubemap_round_trips_and_decodes_all_six_orientations() {
        let root = tempfile::tempdir().unwrap();
        let faces = std::array::from_fn(|i| PathBuf::from(format!("face{i}.png")));
        for (i, path) in faces.iter().enumerate() {
            let channel = u8::try_from(i * 30).unwrap();
            image::RgbaImage::from_pixel(2, 2, image::Rgba([channel, 0, 0, 255]))
                .save(root.path().join(path))
                .unwrap();
        }
        let source = CubemapAsset { version: 1, faces };
        assert_eq!(
            CubemapAsset::from_bytes(&source.to_bytes().unwrap()).unwrap(),
            source
        );
        let decoded = source.decode_faces(root.path()).unwrap();
        assert_eq!(decoded.len(), 6);
        assert!(decoded.iter().all(|f| f.width == 2 && f.height == 2));
        let imported = ImporterRegistry::import(&ImportRequest::new(
            "sky.cubemap",
            source.to_bytes().unwrap(),
        ))
        .unwrap();
        assert!(matches!(imported, DerivedArtifact::Cubemap(_)));
    }
    #[test]
    fn invalid_or_mismatched_face_sources_fail_without_substitution() {
        let root = tempfile::tempdir().unwrap();
        let mut source = CubemapAsset {
            version: 1,
            faces: std::array::from_fn(|_| PathBuf::from("missing.png")),
        };
        assert!(source.decode_faces(root.path()).is_err());
        source.faces[0] = PathBuf::from("../escape.png");
        assert!(source.validate().is_err());
        source.faces[0] = PathBuf::from("wide.png");
        image::RgbaImage::new(4, 2)
            .save(root.path().join("wide.png"))
            .unwrap();
        assert!(source.decode_faces(root.path()).is_err());
    }
}
