//! Redistributable generated resources used by built-in effect presets.
use crate::{AssetError, AssetMeta, io_error, load_meta, save_meta, sidecar_path};
use engine_core::AssetId;
use std::{fs, path::Path};

/// Installs the generated explosion PCM and authoritative metadata without replacing
/// existing user files. Returns its stable source identity for cue references.
/// # Errors
/// Returns path, identity, metadata or filesystem errors. Conflicting files are retained.
pub fn install_builtin_effect_resources(project_root: &Path) -> Result<AssetId, AssetError> {
    let asset_id = engine_world::effects::explosion_sound_asset();
    let mut directory = project_root.join("assets");
    let assets = fs::symlink_metadata(&directory).map_err(|error| io_error(&directory, error))?;
    if assets.file_type().is_symlink() || !assets.is_dir() {
        return Err(AssetError::UnsafePath(directory));
    }
    for part in ["effects", "builtin"] {
        directory = directory.join(part);
        match fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(AssetError::UnsafePath(directory));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&directory).map_err(|error| io_error(&directory, error))?;
            }
            Err(error) => return Err(io_error(&directory, error)),
        }
    }
    install_resource(
        &directory,
        "explosion-debris.obj",
        engine_world::effects::explosion_mesh_asset(),
        "rustic.obj",
        1,
        || {
            b"v 0 0.65 0\nv -0.4 0 -0.3\nv 0.5 -0.1 -0.25\nv 0.1 0 0.5\nv 0 -0.35 0\nf 1 3 2\nf 1 4 3\nf 1 2 4\nf 5 2 3\nf 5 3 4\nf 5 4 2\n".to_vec()
        },
    )?;
    install_resource(
        &directory,
        "explosion.wav",
        asset_id,
        "rustic.audio",
        2,
        explosion_wav,
    )?;
    Ok(asset_id)
}

fn install_resource(
    directory: &Path,
    name: &str,
    asset_id: AssetId,
    importer: &str,
    version: u32,
    bytes: impl FnOnce() -> Vec<u8>,
) -> Result<(), AssetError> {
    let source = directory.join(name);
    let sidecar = sidecar_path(&source);
    for path in [&source, &sidecar] {
        if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(AssetError::UnsafePath(path.clone()));
        }
    }
    if source.exists() || sidecar.exists() {
        if source.is_file() && sidecar.is_file() {
            let meta = load_meta(&sidecar)?;
            if meta.asset_id == asset_id && meta.importer_id == importer {
                return Ok(());
            }
        }
        return Err(AssetError::InvalidData(format!(
            "built-in resource {name} already exists with another identity; it was preserved"
        )));
    }
    crate::meta::atomic_write(&source, &bytes())?;
    let mut meta = AssetMeta::new(importer, version);
    meta.asset_id = asset_id;
    save_meta(&sidecar, &meta)?;
    Ok(())
}

fn explosion_wav() -> Vec<u8> {
    // IEEE-float mono WAV avoids integer quantization and preserves bounded PCM.
    const RATE: u32 = 24_000;
    const FRAMES: u32 = 16_800;
    let data_size = FRAMES * 4;
    let mut bytes = Vec::with_capacity(44 + usize::try_from(data_size).unwrap_or_default());
    bytes.extend(b"RIFF");
    bytes.extend((36 + data_size).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend(3_u16.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(RATE.to_le_bytes());
    bytes.extend((RATE * 4).to_le_bytes());
    bytes.extend(4_u16.to_le_bytes());
    bytes.extend(32_u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data_size.to_le_bytes());
    let mut random = 0x45A1_8C27_u32;
    for frame in 0..FRAMES {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        let noise = f64::from(random) / f64::from(u32::MAX) * 2.0 - 1.0;
        let t = f64::from(frame) / f64::from(RATE);
        let tone = (std::f64::consts::TAU * (90.0 * t - 35.0 * t * t)).sin();
        let envelope = (1.0 - (-t * 1200.0).exp()) * (-t * 9.0).exp();
        #[expect(
            clippy::cast_possible_truncation,
            reason = "bounded generated PCM uses the engine's f32 audio format"
        )]
        let sample = ((noise * 0.55 + tone * 0.35) * envelope) as f32;
        bytes.extend(sample.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::install_builtin_effect_resources;
    use crate::{DerivedArtifact, ImportRequest, ImporterRegistry, load_meta, sidecar_path};
    #[test]
    fn builtin_audio_is_real_importable_pcm_and_preserves_identity() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("assets")).unwrap();
        let id = install_builtin_effect_resources(temp.path()).unwrap();
        assert_eq!(install_builtin_effect_resources(temp.path()).unwrap(), id);
        let source = temp.path().join("assets/effects/builtin/explosion.wav");
        assert_eq!(load_meta(&sidecar_path(&source)).unwrap().asset_id, id);
        let artifact = ImporterRegistry::import(&ImportRequest::new(
            "effects/builtin/explosion.wav",
            std::fs::read(source).unwrap(),
        ))
        .unwrap();
        let DerivedArtifact::Audio(audio) = artifact else {
            panic!("audio")
        };
        assert_eq!(audio.sample_rate, 24_000);
        assert_eq!(audio.channels, 1);
        assert!(audio.samples.iter().any(|value| value.abs() > 0.1));
        assert!(audio.samples.iter().all(|value| value.abs() <= 1.0));
        let mesh_source = temp
            .path()
            .join("assets/effects/builtin/explosion-debris.obj");
        assert_eq!(
            load_meta(&sidecar_path(&mesh_source)).unwrap().asset_id,
            engine_world::effects::explosion_mesh_asset()
        );
        let mesh = ImporterRegistry::import(&ImportRequest::new(
            "assets/effects/builtin/explosion-debris.obj",
            std::fs::read(mesh_source).unwrap(),
        ))
        .unwrap();
        let DerivedArtifact::Model(model) = mesh else {
            panic!("debris mesh")
        };
        assert!(
            model
                .meshes
                .iter()
                .any(|mesh| !mesh.positions.is_empty() && !mesh.indices.is_empty())
        );
    }

    #[test]
    fn conflicting_resources_are_retained_without_rewriting_user_files() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("assets/effects/builtin");
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("explosion.wav");
        std::fs::write(&source, b"user-created sound").unwrap();
        assert!(install_builtin_effect_resources(temp.path()).is_err());
        assert_eq!(std::fs::read(source).unwrap(), b"user-created sound");
    }

    #[cfg(unix)]
    #[test]
    fn builtin_resource_install_rejects_symlinked_asset_roots() {
        let temp = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(external.path(), temp.path().join("assets")).unwrap();
        assert!(install_builtin_effect_resources(temp.path()).is_err());
        assert_eq!(std::fs::read_dir(external.path()).unwrap().count(), 0);
    }
}
