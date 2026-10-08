//! Bounded baseline format importers. Every decoder is wrapped by engine-owned contracts.

use crate::{
    AssetError, AssetKind, AudioArtifact, DerivedArtifact, FontArtifact, ModelArtifact,
    PbrMaterialArtifact, StaticMeshArtifact, TextureArtifact, TexturePixels,
    validate_asset_relative_path,
};
use base64::Engine as _;
use image::ImageFormat;
use lewton::inside_ogg::OggStreamReader;
use num_traits::ToPrimitive as _;
use serde::{Deserialize, Serialize};
use skrifa::{FontRef, MetadataProvider as _, instance::Size};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// Maximum source bytes accepted by an in-process decoder. Larger/riskier work must use the
/// isolated worker with a deliberately raised policy.
pub const MAX_SOURCE_BYTES: usize = 256 * 1024 * 1024;
/// Maximum decoded texture dimension.
pub const MAX_TEXTURE_DIMENSION: u32 = 16_384;
/// Maximum decoded pixels.
pub const MAX_TEXTURE_PIXELS: u64 = 128 * 1024 * 1024;
/// Maximum mesh vertices/indices per source.
pub const MAX_MESH_ELEMENTS: usize = 20_000_000;
/// Maximum decoded PCM sample count.
pub const MAX_AUDIO_SAMPLES: usize = 200_000_000;

/// Source plus explicitly VFS-resolved dependency bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRequest {
    /// Project-relative source path used only for extension selection and diagnostics.
    pub source_path: PathBuf,
    /// Bounded source file bytes.
    pub source_bytes: Vec<u8>,
    /// External glTF/OBJ dependency bytes keyed by normalized source-relative URI.
    pub external_bytes: BTreeMap<String, Vec<u8>>,
}

impl ImportRequest {
    /// Creates a request without external dependencies.
    pub fn new(source_path: impl Into<PathBuf>, source_bytes: Vec<u8>) -> Self {
        Self {
            source_path: source_path.into(),
            source_bytes,
            external_bytes: BTreeMap::new(),
        }
    }

    fn validate(&self) -> Result<(), AssetError> {
        validate_asset_relative_path(&self.source_path)?;
        if self.source_bytes.is_empty() {
            return Err(AssetError::Decode("source is empty".to_owned()));
        }
        if self.source_bytes.len() > MAX_SOURCE_BYTES {
            return Err(AssetError::Limit(format!(
                "source is {} bytes; maximum is {MAX_SOURCE_BYTES}",
                self.source_bytes.len()
            )));
        }
        for (uri, bytes) in &self.external_bytes {
            validate_dependency_uri(uri)?;
            if bytes.len() > MAX_SOURCE_BYTES {
                return Err(AssetError::Limit(format!(
                    "dependency {uri} exceeds {MAX_SOURCE_BYTES} bytes"
                )));
            }
        }
        Ok(())
    }
}

/// Stable contract selected for a source extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImporterContract {
    /// Stable importer identifier stored in `.rmeta`.
    pub id: &'static str,
    /// Version affecting recipe keys and DDC output.
    pub version: u32,
    /// Broad output family.
    pub kind: AssetKind,
}

/// Stateless baseline importer registry.
#[derive(Debug, Default, Clone, Copy)]
pub struct ImporterRegistry;

impl ImporterRegistry {
    /// Returns the versioned contract selected by a source extension.
    ///
    /// # Errors
    ///
    /// Returns an error when the extension has no baseline importer.
    pub fn contract(path: &Path) -> Result<ImporterContract, AssetError> {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match extension.as_str() {
            "gltf" | "glb" => Ok(ImporterContract {
                id: "rustic.gltf",
                version: 2,
                kind: AssetKind::Model,
            }),
            "fbx" => Ok(ImporterContract {
                id: "rustic.fbx",
                version: 1,
                kind: AssetKind::Model,
            }),
            "obj" => Ok(ImporterContract {
                id: "rustic.obj",
                version: 1,
                kind: AssetKind::Model,
            }),
            "png" | "jpg" | "jpeg" | "tga" | "hdr" => Ok(ImporterContract {
                id: "rustic.image",
                version: 1,
                kind: AssetKind::Texture,
            }),
            "wav" | "ogg" => Ok(ImporterContract {
                id: "rustic.audio",
                version: 1,
                kind: AssetKind::Audio,
            }),
            "ttf" | "otf" => Ok(ImporterContract {
                id: "rustic.font",
                version: 1,
                kind: AssetKind::Font,
            }),
            _ => Err(AssetError::UnsupportedFormat(extension)),
        }
    }

    /// Decodes one source. Panics in a third-party decoder are contained and converted to an
    /// error; production callers still route this operation through the isolated asset worker.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid requests, unsupported formats, policy-limit violations, or
    /// decoder failures and contained panics.
    pub fn import(request: &ImportRequest) -> Result<DerivedArtifact, AssetError> {
        request.validate()?;
        let contract = Self::contract(&request.source_path)?;
        let artifact =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match contract.id {
                "rustic.gltf" => import_gltf(request).map(DerivedArtifact::Model),
                "rustic.fbx" => import_fbx(request).map(DerivedArtifact::Model),
                "rustic.obj" => import_obj(request).map(DerivedArtifact::Model),
                "rustic.image" => import_image(request).map(DerivedArtifact::Texture),
                "rustic.audio" => import_audio(request).map(DerivedArtifact::Audio),
                "rustic.font" => import_font(request).map(DerivedArtifact::Font),
                _ => Err(AssetError::UnsupportedFormat(contract.id.to_owned())),
            }))
            .map_err(|_| AssetError::Decode("decoder panic was contained".to_owned()))??;
        if let DerivedArtifact::Model(model) = &artifact {
            model.validate()?;
        }
        Ok(artifact)
    }
}

fn import_image(request: &ImportRequest) -> Result<TextureArtifact, AssetError> {
    let extension = extension(&request.source_path);
    let format = match extension.as_str() {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "tga" => ImageFormat::Tga,
        "hdr" => ImageFormat::Hdr,
        _ => return Err(AssetError::UnsupportedFormat(extension)),
    };
    let image = image::load_from_memory_with_format(&request.source_bytes, format)
        .map_err(|error| AssetError::Decode(error.to_string()))?;
    let width = image.width();
    let height = image.height();
    validate_texture_extent(width, height)?;
    if format == ImageFormat::Hdr {
        let pixels = image.to_rgba32f().into_raw();
        if pixels.iter().any(|value| !value.is_finite()) {
            return Err(AssetError::Decode(
                "HDR contains non-finite pixels".to_owned(),
            ));
        }
        Ok(TextureArtifact {
            width,
            height,
            srgb: false,
            pixels: TexturePixels::Rgba32Float(pixels),
        })
    } else {
        Ok(TextureArtifact {
            width,
            height,
            srgb: true,
            pixels: TexturePixels::Rgba8(image.to_rgba8().into_raw()),
        })
    }
}

fn validate_texture_extent(width: u32, height: u32) -> Result<(), AssetError> {
    if width == 0
        || height == 0
        || width > MAX_TEXTURE_DIMENSION
        || height > MAX_TEXTURE_DIMENSION
        || u64::from(width).saturating_mul(u64::from(height)) > MAX_TEXTURE_PIXELS
    {
        return Err(AssetError::Limit(format!(
            "decoded texture extent {width}x{height} is outside policy"
        )));
    }
    Ok(())
}

fn import_obj(request: &ImportRequest) -> Result<ModelArtifact, AssetError> {
    let mut cursor = Cursor::new(&request.source_bytes);
    let options = tobj::LoadOptions {
        triangulate: true,
        single_index: true,
        ..tobj::LoadOptions::default()
    };
    let (models, materials) = tobj::load_obj_buf(&mut cursor, &options, |_path| {
        Err(tobj::LoadError::OpenFileFailed)
    })
    .map_err(|error| AssetError::Decode(error.to_string()))?;
    let _ = materials.map_err(|error| AssetError::Decode(error.to_string()))?;
    let mut meshes = Vec::new();
    for model in models {
        let mesh = model.mesh;
        if !mesh.positions.len().is_multiple_of(3)
            || !mesh.normals.len().is_multiple_of(3)
            || !mesh.texcoords.len().is_multiple_of(2)
            || mesh.positions.len() / 3 > MAX_MESH_ELEMENTS
            || mesh.indices.len() > MAX_MESH_ELEMENTS
        {
            return Err(AssetError::Limit(
                "OBJ element counts are invalid or exceed policy".to_owned(),
            ));
        }
        let positions = mesh
            .positions
            .as_chunks::<3>()
            .0
            .iter()
            .map(|value| [value[0], value[1], value[2]])
            .collect::<Vec<_>>();
        let normals = mesh
            .normals
            .as_chunks::<3>()
            .0
            .iter()
            .map(|value| [value[0], value[1], value[2]])
            .collect::<Vec<_>>();
        let texcoords = mesh
            .texcoords
            .as_chunks::<2>()
            .0
            .iter()
            .map(|value| [value[0], value[1]])
            .collect::<Vec<_>>();
        validate_mesh(&positions, &normals, &texcoords, &mesh.indices)?;
        meshes.push(StaticMeshArtifact {
            material_index: None,
            source_node: None,
            skin: None,
            joints: Vec::new(),
            weights: Vec::new(),
            positions,
            normals,
            texcoords,
            indices: mesh.indices,
        });
    }
    if meshes.is_empty() {
        return Err(AssetError::Decode("OBJ contains no meshes".to_owned()));
    }
    Ok(ModelArtifact {
        animations: Vec::new(),
        nodes: Vec::new(),
        meshes,
        materials: vec![default_material()],
        external_dependencies: Vec::new(),
    })
}

/// Validated external buffer/image dependencies without decoding mesh payloads.
/// # Errors
/// Returns an error for invalid glTF data or unsafe dependency paths.
pub fn model_dependencies(bytes: &[u8]) -> Result<Vec<String>, AssetError> {
    preflight_gltf_uris(bytes)?;
    let parsed = gltf::Gltf::from_slice(bytes).map_err(|e| AssetError::Decode(e.to_string()))?;
    let mut uris = BTreeSet::new();
    for buffer in parsed.buffers() {
        if let gltf::buffer::Source::Uri(uri) = buffer.source()
            && !uri.starts_with("data:")
        {
            validate_dependency_uri(uri)?;
            uris.insert(uri.to_owned());
        }
    }
    for image in parsed.images() {
        if let gltf::image::Source::Uri { uri, .. } = image.source()
            && !uri.starts_with("data:")
        {
            validate_dependency_uri(uri)?;
            uris.insert(uri.to_owned());
        }
    }
    Ok(uris.into_iter().collect())
}
#[allow(
    clippy::too_many_lines,
    reason = "decode glTF buffers, meshes, skins, and animation data from one parsed source"
)]
fn import_gltf(request: &ImportRequest) -> Result<ModelArtifact, AssetError> {
    preflight_gltf_uris(&request.source_bytes)?;
    let parsed = gltf::Gltf::from_slice(&request.source_bytes)
        .map_err(|error| AssetError::Decode(error.to_string()))?;
    let blob = parsed.blob.as_deref();
    let mut inline = std::collections::BTreeMap::new();
    for buffer in parsed.buffers() {
        if let gltf::buffer::Source::Uri(uri) = buffer.source()
            && uri.starts_with("data:")
        {
            let (_, encoded) = uri.split_once(";base64,").ok_or_else(|| {
                AssetError::Decode("embedded glTF buffers must be base64 data URIs".into())
            })?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|e| AssetError::Decode(e.to_string()))?;
            if bytes.len() > MAX_SOURCE_BYTES {
                return Err(AssetError::Limit(
                    "embedded buffer exceeds source limit".into(),
                ));
            }
            inline.insert(buffer.index(), bytes);
        }
    }
    let mut meshes = Vec::new();
    let mut vertex_count = 0usize;
    let mut external = BTreeSet::new();
    for buffer in parsed.document.buffers() {
        if let gltf::buffer::Source::Uri(uri) = buffer.source()
            && !uri.starts_with("data:")
        {
            validate_dependency_uri(uri)?;
            external.insert(uri.to_owned());
        }
    }
    for image in parsed.document.images() {
        if let gltf::image::Source::Uri { uri, .. } = image.source()
            && !uri.starts_with("data:")
        {
            validate_dependency_uri(uri)?;
            external.insert(uri.to_owned());
        }
    }
    for mesh in parsed.document.meshes() {
        for primitive in mesh.primitives() {
            let reader = primitive.reader(|buffer| match buffer.source() {
                gltf::buffer::Source::Bin => blob,
                gltf::buffer::Source::Uri(uri) if uri.starts_with("data:") => {
                    inline.get(&buffer.index()).map(Vec::as_slice)
                }
                gltf::buffer::Source::Uri(uri) => {
                    request.external_bytes.get(uri).map(Vec::as_slice)
                }
            });
            let positions = reader
                .read_positions()
                .ok_or_else(|| AssetError::Decode("glTF primitive lacks positions".to_owned()))?
                .collect::<Vec<_>>();
            let normals = reader
                .read_normals()
                .map_or_else(Vec::new, Iterator::collect);
            let texcoords = reader
                .read_tex_coords(0)
                .map_or_else(Vec::new, |values| values.into_f32().collect());
            let indices = reader.read_indices().map_or_else(
                || (0..u32::try_from(positions.len()).unwrap_or(u32::MAX)).collect::<Vec<_>>(),
                |values| values.into_u32().collect(),
            );
            validate_mesh(&positions, &normals, &texcoords, &indices)?;
            let mut source_nodes = parsed
                .nodes()
                .filter(|n| n.mesh().is_some_and(|m| m.index() == mesh.index()))
                .map(Some)
                .collect::<Vec<_>>();
            if source_nodes.is_empty() {
                source_nodes.push(None);
            }
            let joints = reader
                .read_joints(0)
                .map_or_else(Vec::new, |v| v.into_u16().collect());
            let weights = reader
                .read_weights(0)
                .map_or_else(Vec::new, |v| v.into_f32().collect());
            for source_node in source_nodes {
                let skin = source_node.as_ref().and_then(gltf::Node::skin).map(|skin| {
                    let reader = skin.reader(|buffer| match buffer.source() {
                        gltf::buffer::Source::Bin => blob,
                        gltf::buffer::Source::Uri(uri) if uri.starts_with("data:") => {
                            inline.get(&buffer.index()).map(Vec::as_slice)
                        }
                        gltf::buffer::Source::Uri(uri) => {
                            request.external_bytes.get(uri).map(Vec::as_slice)
                        }
                    });
                    let joints = skin.joints().map(|n| n.index()).collect::<Vec<_>>();
                    let inverse_bind = reader.read_inverse_bind_matrices().map_or_else(
                        || {
                            vec![
                                [
                                    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.
                                ];
                                joints.len()
                            ]
                        },
                        |v| {
                            v.map(|m| {
                                m.into_iter()
                                    .flatten()
                                    .collect::<Vec<_>>()
                                    .try_into()
                                    .unwrap_or([0.; 16])
                            })
                            .collect()
                        },
                    );
                    crate::ModelSkin {
                        joints,
                        inverse_bind,
                    }
                });
                vertex_count = vertex_count.saturating_add(positions.len());
                if vertex_count > MAX_MESH_ELEMENTS {
                    return Err(AssetError::Limit(
                        "instanced glTF vertex limit exceeded".into(),
                    ));
                }
                meshes.push(StaticMeshArtifact {
                    material_index: primitive.material().index(),
                    source_node: source_node.map(|n| n.index()),
                    skin,
                    joints: joints.clone(),
                    weights: weights.clone(),
                    positions: positions.clone(),
                    normals: normals.clone(),
                    texcoords: texcoords.clone(),
                    indices: indices.clone(),
                });
            }
        }
    }
    if meshes.is_empty() {
        return Err(AssetError::Decode(
            "glTF contains no mesh primitives".to_owned(),
        ));
    }
    let materials =
        parsed
            .document
            .materials()
            .map(|material| {
                let pbr = material.pbr_metallic_roughness();
                PbrMaterialArtifact {
                    base_color: pbr.base_color_factor(),
                    metallic: pbr.metallic_factor(),
                    roughness: pbr.roughness_factor(),
                    base_color_texture_uri: pbr.base_color_texture().and_then(|texture| {
                        match texture.texture().source().source() {
                            gltf::image::Source::Uri { uri, .. } => Some(uri.to_owned()),
                            gltf::image::Source::View { .. } => None,
                        }
                    }),
                }
            })
            .collect::<Vec<_>>();
    let mut parents = std::collections::BTreeMap::new();
    for node in parsed.nodes() {
        for child in node.children() {
            parents.insert(child.index(), node.index());
        }
    }
    let nodes = parsed
        .nodes()
        .map(|node| {
            let (t, r, s) = node.transform().decomposed();
            crate::ModelNode {
                name: node.name().unwrap_or("Node").into(),
                parent: parents.get(&node.index()).copied(),
                translation: t.map(f64::from),
                rotation: r.map(f64::from),
                scale: s.map(f64::from),
            }
        })
        .collect();
    let mut animations = Vec::new();
    for animation in parsed.animations() {
        use engine_core::gameplay::{
            Ease, Value,
            animation::{Clip, Keyframe, Track, TrackInterpolation},
        };
        let mut tracks = Vec::new();
        let mut duration = 0.0_f64;
        for channel in animation.channels() {
            let reader = channel.reader(|buffer| match buffer.source() {
                gltf::buffer::Source::Bin => blob,
                gltf::buffer::Source::Uri(uri) if uri.starts_with("data:") => {
                    inline.get(&buffer.index()).map(Vec::as_slice)
                }
                gltf::buffer::Source::Uri(uri) => {
                    request.external_bytes.get(uri).map(Vec::as_slice)
                }
            });
            let times = reader
                .read_inputs()
                .ok_or_else(|| AssetError::Decode("animation channel has no time accessor".into()))?
                .map(f64::from)
                .collect::<Vec<_>>();
            let outputs = reader.read_outputs().ok_or_else(|| {
                AssetError::Decode("animation channel has no value accessor".into())
            })?;
            let (property, values) = match outputs {
                gltf::animation::util::ReadOutputs::Translations(v) => (
                    "Position",
                    v.map(|v| Value::Vector(v.map(f64::from)))
                        .collect::<Vec<_>>(),
                ),
                gltf::animation::util::ReadOutputs::Rotations(v) => (
                    "Rotation",
                    v.into_f32()
                        .map(|v| Value::Rotation(v.map(f64::from)))
                        .collect(),
                ),
                gltf::animation::util::ReadOutputs::Scales(v) => (
                    "Scale",
                    v.map(|v| Value::Vector(v.map(f64::from))).collect(),
                ),
                gltf::animation::util::ReadOutputs::MorphTargetWeights(_) => continue,
            };
            let (interpolation, values) = match channel.sampler().interpolation() {
                gltf::animation::Interpolation::Linear => (TrackInterpolation::Linear, values),
                gltf::animation::Interpolation::Step => (TrackInterpolation::Step, values),
                gltf::animation::Interpolation::CubicSpline => {
                    if values.len() != times.len() * 3 {
                        return Err(AssetError::Decode(
                            "cubic animation accessor count differs".into(),
                        ));
                    }
                    let incoming = values.as_chunks::<3>().0.iter().map(|v| v[0]).collect();
                    let outgoing = values.as_chunks::<3>().0.iter().map(|v| v[2]).collect();
                    let values = values.as_chunks::<3>().0.iter().map(|v| v[1]).collect();
                    (
                        TrackInterpolation::CubicSpline { incoming, outgoing },
                        values,
                    )
                }
            };
            if times.len() != values.len() {
                return Err(AssetError::Decode(
                    "animation key/value counts differ".into(),
                ));
            }
            duration = duration.max(times.last().copied().unwrap_or(0.0));
            tracks.push(Track {
                interpolation,
                target: format!("node_{}/{property}", channel.target().node().index()),
                keys: times
                    .into_iter()
                    .zip(values)
                    .map(|(time, value)| Keyframe {
                        time,
                        value,
                        easing: Ease::Linear,
                    })
                    .collect(),
            });
        }
        if duration > 0.0 && !tracks.is_empty() {
            let clip = Clip {
                name: animation
                    .name()
                    .map_or_else(|| format!("Animation{}", animation.index()), str::to_owned),
                duration,
                tracks,
                markers: Vec::new(),
            };
            clip.validate().map_err(AssetError::Decode)?;
            animations.push(clip);
        }
    }
    Ok(ModelArtifact {
        animations,
        nodes,
        meshes,
        materials: if materials.is_empty() {
            vec![default_material()]
        } else {
            materials
        },
        external_dependencies: external.into_iter().collect(),
    })
}

fn preflight_gltf_uris(bytes: &[u8]) -> Result<(), AssetError> {
    if bytes.first().copied() != Some(b'{') {
        return Ok(());
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| AssetError::Decode(error.to_string()))?;
    visit_gltf_uris(&value)
}

fn visit_gltf_uris(value: &serde_json::Value) -> Result<(), AssetError> {
    match value {
        serde_json::Value::Object(object) => {
            if let Some(uri) = object.get("uri").and_then(serde_json::Value::as_str)
                && !uri.starts_with("data:")
            {
                validate_dependency_uri(uri)?;
            }
            for child in object.values() {
                visit_gltf_uris(child)?;
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                visit_gltf_uris(child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_mesh(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    texcoords: &[[f32; 2]],
    indices: &[u32],
) -> Result<(), AssetError> {
    if positions.is_empty()
        || positions.len() > MAX_MESH_ELEMENTS
        || indices.is_empty()
        || indices.len() > MAX_MESH_ELEMENTS
        || !indices.len().is_multiple_of(3)
        || indices
            .iter()
            .any(|index| *index as usize >= positions.len())
        || (!normals.is_empty() && normals.len() != positions.len())
        || (!texcoords.is_empty() && texcoords.len() != positions.len())
    {
        return Err(AssetError::Decode(
            "mesh topology or attribute counts are invalid".to_owned(),
        ));
    }
    if positions
        .iter()
        .flat_map(|value| value.iter())
        .chain(normals.iter().flat_map(|value| value.iter()))
        .chain(texcoords.iter().flat_map(|value| value.iter()))
        .any(|value| !value.is_finite())
    {
        return Err(AssetError::Decode(
            "mesh contains non-finite values".to_owned(),
        ));
    }
    Ok(())
}

const fn default_material() -> PbrMaterialArtifact {
    PbrMaterialArtifact {
        base_color: [1.0, 1.0, 1.0, 1.0],
        metallic: 0.0,
        roughness: 1.0,
        base_color_texture_uri: None,
    }
}

fn import_audio(request: &ImportRequest) -> Result<AudioArtifact, AssetError> {
    match extension(&request.source_path).as_str() {
        "wav" => import_wav(&request.source_bytes),
        "ogg" => import_ogg(&request.source_bytes),
        extension => Err(AssetError::UnsupportedFormat(extension.to_owned())),
    }
}

fn import_wav(bytes: &[u8]) -> Result<AudioArtifact, AssetError> {
    let mut reader = hound::WavReader::new(Cursor::new(bytes))
        .map_err(|error| AssetError::Decode(error.to_string()))?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.sample_rate == 0 {
        return Err(AssetError::Decode(
            "WAV has invalid stream metadata".to_owned(),
        ));
    }
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|sample| sample.map_err(|error| AssetError::Decode(error.to_string())))
            .collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample.clamp(1, 32);
            let scale = ((1_u64 << (bits - 1)) - 1)
                .max(1)
                .to_f32()
                .ok_or_else(|| AssetError::Decode("WAV normalization overflow".to_owned()))?;
            reader
                .samples::<i32>()
                .map(|sample| {
                    sample
                        .map_err(|error| AssetError::Decode(error.to_string()))
                        .and_then(|value| {
                            value.to_f32().map(|value| value / scale).ok_or_else(|| {
                                AssetError::Decode("WAV sample conversion failed".to_owned())
                            })
                        })
                })
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    validate_audio_samples(&samples)?;
    Ok(AudioArtifact {
        channels: spec.channels,
        sample_rate: spec.sample_rate,
        samples,
    })
}

fn import_ogg(bytes: &[u8]) -> Result<AudioArtifact, AssetError> {
    let mut reader = OggStreamReader::new(Cursor::new(bytes))
        .map_err(|error| AssetError::Decode(error.to_string()))?;
    let channels = u16::from(reader.ident_hdr.audio_channels);
    let sample_rate = reader.ident_hdr.audio_sample_rate;
    let mut samples = Vec::new();
    while let Some(packet) = reader
        .read_dec_packet_itl()
        .map_err(|error| AssetError::Decode(error.to_string()))?
    {
        if samples.len().saturating_add(packet.len()) > MAX_AUDIO_SAMPLES {
            return Err(AssetError::Limit(format!(
                "decoded audio exceeds {MAX_AUDIO_SAMPLES} samples"
            )));
        }
        samples.extend(
            packet
                .into_iter()
                .map(|sample| f32::from(sample) / 32_768.0),
        );
    }
    validate_audio_samples(&samples)?;
    Ok(AudioArtifact {
        channels,
        sample_rate,
        samples,
    })
}

fn validate_audio_samples(samples: &[f32]) -> Result<(), AssetError> {
    if samples.is_empty() || samples.len() > MAX_AUDIO_SAMPLES {
        return Err(AssetError::Limit(
            "decoded audio sample count is empty or exceeds policy".to_owned(),
        ));
    }
    if samples.iter().any(|sample| !sample.is_finite()) {
        return Err(AssetError::Decode(
            "audio contains non-finite samples".to_owned(),
        ));
    }
    Ok(())
}

fn import_font(request: &ImportRequest) -> Result<FontArtifact, AssetError> {
    let face = FontRef::new(&request.source_bytes)
        .map_err(|error| AssetError::Decode(format!("font parse error: {error:?}")))?;
    let location: &[skrifa::instance::NormalizedCoord] = &[];
    let metrics = face.metrics(Size::unscaled(), location);
    let glyph_count = metrics.glyph_count;
    if glyph_count == 0 {
        return Err(AssetError::Decode("font has no glyphs".to_owned()));
    }
    Ok(FontArtifact {
        glyph_count,
        units_per_em: metrics.units_per_em,
        face_index: 0,
    })
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn validate_dependency_uri(uri: &str) -> Result<(), AssetError> {
    if uri.contains("%2e%2e")
        || uri.contains("%2E%2E")
        || uri.contains(':')
        || uri.starts_with(['/', '\\'])
    {
        return Err(AssetError::UnsafePath(PathBuf::from(uri)));
    }
    validate_asset_relative_path(Path::new(uri))
}

#[allow(
    clippy::too_many_lines,
    reason = "decode FBX meshes, skins, and animation takes from one loaded scene"
)]
#[allow(
    clippy::cast_possible_truncation,
    reason = "FBX doubles become GPU floats; artifact validation rejects non-finite results"
)]
fn import_fbx(request: &ImportRequest) -> Result<ModelArtifact, AssetError> {
    use engine_core::gameplay::{
        Ease, Value,
        animation::{Clip, Keyframe, Track, TrackInterpolation},
    };
    let scene = ufbx::load_memory(
        &request.source_bytes,
        ufbx::LoadOpts {
            temp_allocator: ufbx::AllocatorOpts {
                memory_limit: MAX_SOURCE_BYTES * 2,
                ..Default::default()
            },
            result_allocator: ufbx::AllocatorOpts {
                memory_limit: MAX_SOURCE_BYTES * 2,
                ..Default::default()
            },
            load_external_files: false,
            ..Default::default()
        },
    )
    .map_err(|e| AssetError::Decode(format!("{e:?}")))?;
    let v3 = |v: ufbx::Vec3| [v.x, v.y, v.z];
    let q4 = |v: ufbx::Quat| [v.x, v.y, v.z, v.w];
    let nodes = scene
        .nodes
        .iter()
        .map(|n| crate::ModelNode {
            name: n.element.name.to_string(),
            parent: n.parent.as_ref().map(|p| p.element.typed_id as usize),
            translation: v3(n.local_transform.translation),
            rotation: q4(n.local_transform.rotation),
            scale: v3(n.local_transform.scale),
        })
        .collect();
    let mut meshes = Vec::new();
    for mesh in &scene.meshes {
        if mesh.num_indices > MAX_MESH_ELEMENTS || mesh.num_triangles > MAX_MESH_ELEMENTS / 3 {
            return Err(AssetError::Limit("FBX mesh exceeds element limit".into()));
        }
        let positions = (0..mesh.num_indices)
            .map(|i| v3(mesh.vertex_position[i]).map(|v| v as f32))
            .collect::<Vec<_>>();
        let normals = if mesh.vertex_normal.exists {
            (0..mesh.num_indices)
                .map(|i| v3(mesh.vertex_normal[i]).map(|v| v as f32))
                .collect()
        } else {
            Vec::new()
        };
        let texcoords = if mesh.vertex_uv.exists {
            (0..mesh.num_indices)
                .map(|i| {
                    let v = mesh.vertex_uv[i];
                    [v.x as f32, v.y as f32]
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut indices = Vec::new();
        let mut scratch = vec![0; mesh.max_face_triangles * 3];
        for face in &mesh.faces {
            let count = ufbx::triangulate_face(&mut scratch, mesh, *face) as usize;
            indices.extend_from_slice(&scratch[..count * 3]);
        }
        validate_mesh(&positions, &normals, &texcoords, &indices)?;
        let source_node = scene
            .nodes
            .iter()
            .find(|n| {
                n.mesh
                    .as_ref()
                    .is_some_and(|m| m.element.typed_id == mesh.element.typed_id)
            })
            .map(|n| n.element.typed_id as usize);
        let mut joints = Vec::new();
        let mut weights = Vec::new();
        let skin = if let Some(deformer) = mesh.skin_deformers.first() {
            if deformer.clusters.len() > usize::from(u16::MAX) {
                return Err(AssetError::Limit("FBX joint palette too large".into()));
            }
            let palette = deformer
                .clusters
                .iter()
                .map(|c| {
                    c.bone_node
                        .as_ref()
                        .map_or(0, |n| n.element.typed_id as usize)
                })
                .collect();
            let inverse_bind = deformer
                .clusters
                .iter()
                .map(|c| {
                    let m = c.geometry_to_bone;
                    [
                        m.m00 as f32,
                        m.m10 as f32,
                        m.m20 as f32,
                        0.,
                        m.m01 as f32,
                        m.m11 as f32,
                        m.m21 as f32,
                        0.,
                        m.m02 as f32,
                        m.m12 as f32,
                        m.m22 as f32,
                        0.,
                        m.m03 as f32,
                        m.m13 as f32,
                        m.m23 as f32,
                        1.,
                    ]
                })
                .collect();
            for i in 0..mesh.num_indices {
                let vertex = &deformer.vertices[mesh.vertex_indices[i] as usize];
                let mut j = [0; 4];
                let mut w = [0.; 4];
                for k in 0..(vertex.num_weights as usize).min(4) {
                    let weight = &deformer.weights[vertex.weight_begin as usize + k];
                    j[k] = u16::try_from(weight.cluster_index).map_err(|_| {
                        AssetError::Limit("FBX joint index exceeds palette limit".into())
                    })?;
                    w[k] = weight.weight as f32;
                }
                let total: f32 = w.iter().sum();
                if total > 0.0 {
                    for v in &mut w {
                        *v /= total;
                    }
                }
                joints.push(j);
                weights.push(w);
            }
            Some(crate::ModelSkin {
                joints: palette,
                inverse_bind,
            })
        } else {
            None
        };
        meshes.push(StaticMeshArtifact {
            material_index: None,
            source_node,
            skin,
            joints,
            weights,
            positions,
            normals,
            texcoords,
            indices,
        });
    }
    let mut animations = Vec::new();
    for stack in &scene.anim_stacks {
        let baked = ufbx::bake_anim(
            &scene,
            &stack.anim,
            ufbx::BakeOpts {
                trim_start_time: true,
                resample_rate: 60.0,
                maximum_sample_rate: 120.0,
                max_keyframe_segments: 100_000,
                ..Default::default()
            },
        )
        .map_err(|e| AssetError::Decode(format!("{e:?}")))?;
        let mut tracks = Vec::new();
        for n in &baked.nodes {
            let convert = |property: &str, keys: Vec<Keyframe>| Track {
                interpolation: TrackInterpolation::Linear,
                target: format!("node_{}/{property}", n.typed_id),
                keys,
            };
            if !n.translation_keys.is_empty() {
                tracks.push(convert(
                    "Position",
                    n.translation_keys
                        .iter()
                        .map(|k| Keyframe {
                            time: k.time,
                            value: Value::Vector(v3(k.value)),
                            easing: Ease::Linear,
                        })
                        .collect(),
                ));
            }
            if !n.rotation_keys.is_empty() {
                tracks.push(convert(
                    "Rotation",
                    n.rotation_keys
                        .iter()
                        .map(|k| Keyframe {
                            time: k.time,
                            value: Value::Rotation(q4(k.value)),
                            easing: Ease::Linear,
                        })
                        .collect(),
                ));
            }
            if !n.scale_keys.is_empty() {
                tracks.push(convert(
                    "Scale",
                    n.scale_keys
                        .iter()
                        .map(|k| Keyframe {
                            time: k.time,
                            value: Value::Vector(v3(k.value)),
                            easing: Ease::Linear,
                        })
                        .collect(),
                ));
            }
        }
        let duration = baked.playback_duration.max(baked.key_time_max);
        if duration > 0.0 {
            let clip = Clip {
                name: stack.element.name.to_string(),
                duration,
                tracks,
                markers: Vec::new(),
            };
            clip.validate().map_err(AssetError::Decode)?;
            animations.push(clip);
        }
    }
    Ok(ModelArtifact {
        meshes,
        materials: vec![default_material()],
        external_dependencies: Vec::new(),
        animations,
        nodes,
    })
}

/// Loads model sources and validated dependencies from a play snapshot's assets directory.
/// # Errors
/// Returns an error for unreadable or invalid models, metadata, or dependencies.
pub fn load_model_library(
    root: &Path,
) -> Result<
    std::collections::BTreeMap<engine_core::AssetId, (std::path::PathBuf, ModelArtifact)>,
    AssetError,
> {
    fn visit(
        root: &Path,
        directory: &Path,
        out: &mut std::collections::BTreeMap<
            engine_core::AssetId,
            (std::path::PathBuf, ModelArtifact),
        >,
    ) -> Result<(), AssetError> {
        if !directory.exists() {
            return Ok(());
        }
        for entry in std::fs::read_dir(directory).map_err(|e| crate::io_error(directory, e))? {
            let entry = entry.map_err(|e| crate::io_error(directory, e))?;
            let kind = entry
                .file_type()
                .map_err(|e| crate::io_error(entry.path(), e))?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                visit(root, &entry.path(), out)?;
                continue;
            }
            let path = entry.path();
            let extension = path
                .extension()
                .and_then(|v| v.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !matches!(extension.as_str(), "obj" | "gltf" | "glb" | "fbx") {
                continue;
            }
            let metadata = crate::load_meta(&crate::sidecar_path(&path))?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| AssetError::UnsafePath(path.clone()))?
                .to_path_buf();
            let bytes = std::fs::read(&path).map_err(|e| crate::io_error(&path, e))?;
            let mut request = ImportRequest::new(&relative, bytes);
            if matches!(extension.as_str(), "gltf" | "glb") {
                for uri in model_dependencies(&request.source_bytes)? {
                    let dependency = path
                        .parent()
                        .ok_or_else(|| AssetError::UnsafePath(path.clone()))?
                        .join(&uri);
                    request.external_bytes.insert(
                        uri,
                        std::fs::read(&dependency).map_err(|e| crate::io_error(&dependency, e))?,
                    );
                }
            }
            if let DerivedArtifact::Model(model) = ImporterRegistry::import(&request)? {
                out.insert(metadata.asset_id, (relative, model));
            }
        }
        Ok(())
    }
    let mut result = std::collections::BTreeMap::new();
    visit(root, &root.join("assets"), &mut result)?;
    Ok(result)
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests compare exact round trips and deterministic values"
)]
mod tests {
    use super::*;

    #[test]
    fn fbx_import_triangulates_geometry_and_bakes_named_animation_take() {
        let bytes = include_bytes!("../tests/fixtures/animated_triangle.fbx").to_vec();
        let DerivedArtifact::Model(model) =
            ImporterRegistry::import(&ImportRequest::new("triangle.fbx", bytes)).unwrap()
        else {
            panic!("model expected");
        };
        assert_eq!(model.meshes[0].indices.len(), 3);
        let clip = model
            .animations
            .iter()
            .find(|clip| clip.name == "Slide")
            .unwrap();
        assert!((clip.duration - 1.0).abs() < 1e-6);
        let pose = clip.sample(0.5).unwrap();
        assert!(pose.iter().any(|(key, value)| key.ends_with("/Position")
            && matches!(value, engine_core::gameplay::Value::Vector(v) if (v[0]-1.0).abs()<1e-4)));
    }

    #[test]
    fn gltf_import_retains_embedded_skin_bind_matrices_and_animation_modes() {
        let bytes = include_bytes!("../tests/fixtures/skinned_animation.gltf").to_vec();
        let DerivedArtifact::Model(model) =
            ImporterRegistry::import(&ImportRequest::new("rig.gltf", bytes)).unwrap()
        else {
            panic!("model expected");
        };
        assert_eq!(model.nodes[1].parent, Some(0));
        let skin = model.meshes[0].skin.as_ref().unwrap();
        assert_eq!(skin.joints, vec![1]);
        assert_eq!(skin.inverse_bind[0][12], -1.0);
        assert_eq!(model.meshes[0].weights[0], [1., 0., 0., 0.]);
        let pose = model.animations[0].sample(0.5).unwrap();
        assert_eq!(
            pose["node_1/Position"],
            engine_core::gameplay::Value::Vector([2., 0., 0.])
        );
        assert_eq!(
            pose["node_0/Scale"],
            engine_core::gameplay::Value::Vector([1., 1., 1.])
        );
        let mut broken = model.clone();
        broken.nodes[0].parent = Some(1);
        assert!(broken.validate().is_err());
        let mut broken = model.clone();
        broken.meshes[0].joints[0][0] = 10;
        assert!(broken.validate().is_err());
    }

    #[test]
    fn obj_import_produces_a_static_mesh() {
        let source = b"v -1 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0.5 1\nf 1/1 2/2 3/3\n";
        let result =
            ImporterRegistry::import(&ImportRequest::new("triangle.obj", source.to_vec())).unwrap();
        let DerivedArtifact::Model(model) = result else {
            panic!("expected model")
        };
        assert_eq!(model.meshes[0].positions.len(), 3);
        assert_eq!(model.meshes[0].indices.len(), 3);
    }

    #[test]
    fn wav_import_decodes_pcm() {
        let mut bytes = Cursor::new(Vec::new());
        {
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: 8_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut writer = hound::WavWriter::new(&mut bytes, spec).unwrap();
            writer.write_sample::<i16>(-10).unwrap();
            writer.write_sample::<i16>(10).unwrap();
            writer.finalize().unwrap();
        }
        let result =
            ImporterRegistry::import(&ImportRequest::new("tone.wav", bytes.into_inner())).unwrap();
        let DerivedArtifact::Audio(audio) = result else {
            panic!("expected audio")
        };
        assert_eq!(audio.channels, 1);
        assert_eq!(audio.samples.len(), 2);
    }

    #[test]
    fn malformed_corpus_never_panics() {
        let extensions = [
            "png", "jpg", "tga", "hdr", "gltf", "glb", "fbx", "obj", "wav", "ogg", "ttf", "otf",
        ];
        for extension in extensions {
            for bytes in [vec![0], vec![0xff; 31], b"../ malformed".to_vec()] {
                let result = std::panic::catch_unwind(|| {
                    ImporterRegistry::import(&ImportRequest::new(
                        format!("corrupt.{extension}"),
                        bytes,
                    ))
                });
                assert!(result.is_ok(), "decoder panicked for {extension}");
                assert!(result.unwrap().is_err(), "corrupt {extension} was accepted");
            }
        }
    }

    #[test]
    fn gltf_external_traversal_is_rejected() {
        let source = br#"{
            "asset":{"version":"2.0"},
            "buffers":[{"uri":"../outside.bin","byteLength":12}],
            "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":12}],
            "accessors":[{"bufferView":0,"componentType":5126,"count":1,"type":"VEC3"}],
            "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}]
        }"#;
        assert!(matches!(
            ImporterRegistry::import(&ImportRequest::new("unsafe.gltf", source.to_vec())),
            Err(AssetError::UnsafePath(_))
        ));
    }
}
