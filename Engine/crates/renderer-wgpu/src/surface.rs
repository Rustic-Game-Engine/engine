//! Persistent half-float texture arrays. Color maps decode to linear light;
//! scalar and normal maps retain their raw channels. HDR keeps values above one.
use super::*;
use engine_world::{SurfaceTextures, TextureMap};
use std::path::Path;

type SourceStamp = (String, Option<std::time::SystemTime>, u64);

pub(super) struct SurfaceCache {
    pub layout: wgpu::BindGroupLayout,
    groups: HashMap<String, wgpu::BindGroup>,
    stamps: HashMap<String, Vec<SourceStamp>>,
}
fn key(material: &SurfaceTextures) -> String {
    let mut recipe = material.clone();
    recipe.effect_source = None;
    format!("{recipe:?}")
}
impl SurfaceCache {
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("surface maps"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        Self {
            layout,
            groups: HashMap::new(),
            stamps: HashMap::new(),
        }
    }
    pub fn clear(&mut self) {
        self.groups.clear();
        self.stamps.clear();
    }
    pub fn default_group(&self) -> &wgpu::BindGroup {
        self.group(&SurfaceTextures::default())
    }
    pub fn group(&self, material: &SurfaceTextures) -> &wgpu::BindGroup {
        &self.groups[&key(material)]
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        root: &Path,
        scene: &ViewportScene,
    ) -> Result<(), RendererError> {
        let default = SurfaceTextures::default();
        let mut active = std::collections::HashSet::new();
        for material in std::iter::once(&default).chain(scene.meshes.iter().map(|m| &m.textures)) {
            let key = key(material);
            active.insert(key.clone());
            let stamp = source_stamps(root, material);
            if self.groups.contains_key(&key) && self.stamps.get(&key) == Some(&stamp) {
                continue;
            }
            let group = upload(device, queue, &self.layout, root, material)?;
            self.stamps.insert(key.clone(), stamp);
            self.groups.insert(key, group);
        }
        self.groups.retain(|key, _| active.contains(key));
        self.stamps.retain(|key, _| active.contains(key));
        Ok(())
    }
}
pub(super) fn load(root: &Path, relative: &str) -> Result<image::Rgba32FImage, RendererError> {
    let fail = |e| RendererError::InvalidMesh(format!("Texture {relative}: {e}"));
    let root = root.canonicalize().map_err(|e| fail(e.to_string()))?;
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|e| fail(e.to_string()))?;
    if !path.starts_with(root) {
        return Err(fail("path leaves project".into()));
    }
    let mut reader = image::ImageReader::open(path)
        .map_err(|e| fail(e.to_string()))?
        .with_guessed_format()
        .map_err(|e| fail(e.to_string()))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader
        .decode()
        .map_err(|e| fail(e.to_string()))?
        .to_rgba32f())
}
#[allow(
    clippy::too_many_lines,
    reason = "texture, sampler and uniform descriptors form one GPU upload"
)]
fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    root: &Path,
    material: &SurfaceTextures,
) -> Result<wgpu::BindGroup, RendererError> {
    material.validate().map_err(RendererError::InvalidMesh)?;
    let mut images = Vec::new();
    let mut size = [1, 1];
    let mut mask = 0u32;
    for (i, kind) in TextureMap::ALL.iter().enumerate() {
        let Some(path) = material.maps.get(kind) else {
            continue;
        };
        let mut image = load(root, path)?;
        if kind.is_color() && !path.to_ascii_lowercase().ends_with(".hdr") {
            for pixel in image.pixels_mut() {
                for v in &mut pixel.0[..3] {
                    *v = if *v <= 0.04045 {
                        *v / 12.92
                    } else {
                        ((*v + 0.055) / 1.055).powf(2.4)
                    };
                }
            }
        }
        // Bound GPU residency per material; authoring retains the original files.
        size[0] = size[0].max(image.width().min(1024));
        size[1] = size[1].max(image.height().min(1024));
        mask |= 1 << i;
        images.push((i, image));
    }
    if let Some(path) = &material.orm_map {
        let packed = load(root, path)?;
        for (kind, channel) in [
            (TextureMap::AmbientOcclusion, 0),
            (TextureMap::Roughness, 1),
            (TextureMap::Metallic, 2),
        ] {
            if material.maps.contains_key(&kind) {
                continue;
            }
            let layer = TextureMap::ALL
                .iter()
                .position(|value| *value == kind)
                .unwrap_or(0);
            let image = split_orm_channel(&packed, channel);
            size[0] = size[0].max(image.width().min(1024));
            size[1] = size[1].max(image.height().min(1024));
            mask |= 1 << layer;
            images.push((layer, image));
        }
    }
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("surface HDR maps"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 17,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (i, image) in images {
        let image = image::imageops::resize(
            &image,
            size[0],
            size[1],
            image::imageops::FilterType::Triangle,
        );
        let bytes: Vec<_> = image
            .as_raw()
            .iter()
            .flat_map(|v| {
                half::f16::from_f32(if v.is_finite() {
                    v.clamp(-65504.0, 65504.0)
                } else {
                    0.0
                })
                .to_le_bytes()
            })
            .collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: u32::try_from(i).unwrap_or(0),
                },
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size[0] * 8),
                rows_per_image: Some(size[1]),
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
    }
    let effect_view = upload_effect_maps(device, queue, root, material)?;
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("surface repeat"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let mut bytes = f32_bytes(&[
        material.uv_scale[0],
        material.uv_scale[1],
        material.uv_offset[0],
        material.uv_offset[1],
        material.height_scale,
        material.emissive_strength,
        material
            .roughness
            .unwrap_or(if mask & (1 << 2) != 0 { 1.0 } else { 0.5 }),
        material
            .metallic
            .unwrap_or(if mask & (1 << 3) != 0 { 1.0 } else { 0.0 }),
    ]);
    bytes.extend_from_slice(&mask.to_le_bytes());
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend(f32_bytes(&material.base_color));
    bytes.extend(f32_bytes(&[
        material.emissive_color[0],
        material.emissive_color[1],
        material.emissive_color[2],
        1.0,
    ]));
    bytes.extend(f32_bytes(&[
        material.occlusion_strength,
        material.normal_scale,
        if material.pbr { 1.0 } else { 0.0 },
        material.alpha_cutoff.unwrap_or(-1.0),
    ]));
    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("surface settings"),
        contents: &bytes,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    Ok(device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("surface material"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&effect_view),
            },
        ],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::{Hash, Hasher};
    #[test]
    #[ignore = "requires a physical or software graphics adapter"]
    #[allow(
        clippy::too_many_lines,
        reason = "one GPU context exercises HDR, UV, stable mask reload and restoration end to end"
    )]
    fn surface_maps_render_hdr_emission_and_clear_without_reuploading_geometry() {
        let directory = tempfile::tempdir().unwrap();
        let hdr = std::fs::File::create(directory.path().join("emissive.hdr")).unwrap();
        image::codecs::hdr::HdrEncoder::new(hdr)
            .encode(&[image::Rgb([4.0, 0.0, 0.0])], 1, 1)
            .unwrap();
        image::RgbaImage::from_pixel(1, 1, image::Rgba([128u8, 128, 255, 255]))
            .save(directory.path().join("map.png"))
            .unwrap();
        let mut renderer = SceneViewportRenderer::new(BackendRequest::Auto).unwrap();
        renderer.set_environment_root(directory.path());
        let mut scene = ViewportScene {
            environment: engine_world::SceneEnvironment {
                enabled: true,
                sun_intensity: 0.0,
                ambient_intensity: 0.0,
                haze_density: 0.0,
                ..Default::default()
            },
            camera_position: [0.0, 0.0, -1.0],
            view_projection: identity_matrix(),
            meshes: vec![ViewportMesh {
                textures: SurfaceTextures::default(),
                instance_key: 1,
                mesh_key: 1,
                vertices: [[-1.0, -1.0, 0.5], [1.0, -1.0, 0.5], [0.0, 1.0, 0.5]]
                    .map(|position| ViewportVertex {
                        position,
                        normal: [0.0, 0.0, -1.0],
                        uv: [0.5, 0.5],
                    })
                    .to_vec(),
                indices: vec![0, 2, 1],
                model: identity_matrix(),
                color: [1.0; 4],
                selected: false,
            }],
            lights: vec![],
            guides: vec![],
            grid_vertices: vec![],
            clear_color: [0.0, 0.0, 0.0, 1.0],
        };
        let center = |frame: &RenderedFrame| {
            frame.rgba8[(16 * 33 + 16) * 4..(16 * 33 + 16) * 4 + 3].to_vec()
        };
        let black = renderer.render(33, 33, &scene).unwrap().unwrap();
        assert_eq!(center(&black), vec![0, 0, 0]);
        for kind in TextureMap::ALL {
            scene.meshes[0].textures.maps.insert(kind, "map.png".into());
        }
        scene.meshes[0]
            .textures
            .maps
            .insert(TextureMap::Emissive, "emissive.hdr".into());
        let lit = renderer.render(33, 33, &scene).unwrap().unwrap();
        let color = center(&lit);
        assert!(
            color[0] > 220 && color[1] < 8 && color[2] < 8,
            "HDR was clipped or rendered incorrectly: {color:?}"
        );
        scene.meshes[0].textures = SurfaceTextures::default();
        let clear = renderer.render(33, 33, &scene).unwrap().unwrap();
        assert_eq!(center(&clear), vec![0, 0, 0]);
        assert_eq!(renderer.stats().mesh_uploads, 1);
        // Real mesh UVs and material offset must select different image texels.
        let image = image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
        image.save(directory.path().join("albedo.png")).unwrap();
        scene.environment.ambient_intensity = 1.0;
        scene.environment.ambient_color = [1.0; 3];
        for vertex in &mut scene.meshes[0].vertices {
            vertex.uv = [0.25, 0.5];
        }
        scene.meshes[0]
            .textures
            .maps
            .insert(TextureMap::BaseColor, "albedo.png".into());
        let red = renderer.render(33, 33, &scene).unwrap().unwrap();
        let color = center(&red);
        assert!(
            color[0] > 150 && color[2] < 8,
            "albedo UV sample: {color:?}"
        );
        scene.meshes[0].textures.uv_offset[0] = 0.5;
        let blue = renderer.render(33, 33, &scene).unwrap().unwrap();
        let color = center(&blue);
        assert!(
            color[2] > 150 && color[0] < 8,
            "UV offset sample: {color:?}"
        );
        // A real stable-ID mask controls coverage and source edits invalidate its atlas.
        let target = engine_world::EntityId::new();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        target.hash(&mut hasher);
        scene.meshes[0].instance_key = hasher.finish();
        let mask_path = directory.path().join("coverage.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([0_u8, 0, 0, 255]))
            .save(&mask_path)
            .unwrap();
        let metadata = engine_assets::AssetMeta::new("rustic.image", 1);
        engine_assets::save_meta(&engine_assets::sidecar_path(&mask_path), &metadata).unwrap();
        let mut frame = engine_world::effects::EffectFrame::default();
        frame.surfaces.push(engine_world::effects::SurfaceModifier {
            target_material: None,
            is_dissolve: true,
            source: engine_world::EntityId::new(),
            target,
            noise_texture: None,
            mask_texture: Some(metadata.asset_id),
            distortion_texture: None,
            distortion_strength: 0.0,
            direction: [0.0; 3],
            phase: 0.0,
            particle_density: 0.0,
            uv_offset: [0.0; 2],
            uv_scale: [1.0; 2],
            uv_rotation: 0.0,
            dissolve_noise_scale: 1.0,
            color: [1.0; 4],
            emission: 0.0,
            dissolve: 0.5,
            edge_width: 0.0,
            edge_color: [0.0; 3],
        });
        renderer.set_effect_frame(&frame).unwrap();
        assert_eq!(
            center(&renderer.render(33, 33, &scene).unwrap().unwrap()),
            vec![0, 0, 0]
        );
        image::RgbaImage::from_pixel(1, 1, image::Rgba([255_u8; 4]))
            .save(&mask_path)
            .unwrap();
        let restored = center(&renderer.render(33, 33, &scene).unwrap().unwrap());
        assert!(
            restored[2] > 150,
            "modified dissolve mask did not reload: {restored:?}"
        );
        renderer
            .set_effect_frame(&engine_world::effects::EffectFrame::default())
            .unwrap();
        assert_eq!(
            center(&renderer.render(33, 33, &scene).unwrap().unwrap()),
            restored
        );
    }
}

fn split_orm_channel(image: &image::Rgba32FImage, channel: usize) -> image::Rgba32FImage {
    image::ImageBuffer::from_fn(image.width(), image.height(), |x, y| {
        let value = image.get_pixel(x, y).0[channel];
        image::Rgba([value, value, value, 1.0])
    })
}

#[cfg(test)]
mod packed_orm_tests {
    use super::*;
    #[test]
    fn orm_channels_remain_linear_and_keep_gltf_channel_convention() {
        let image = image::Rgba32FImage::from_pixel(1, 1, image::Rgba([0.25, 0.5, 0.75, 1.0]));
        for (channel, expected) in [0.25, 0.5, 0.75].into_iter().enumerate() {
            let split = split_orm_channel(&image, channel);
            assert!((split.get_pixel(0, 0).0[0] - expected).abs() < f32::EPSILON);
        }
    }
}

fn source_stamps(root: &Path, material: &SurfaceTextures) -> Vec<SourceStamp> {
    material
        .maps
        .values()
        .chain(material.orm_map.iter())
        .chain(material.effect_noise_map.iter())
        .chain(material.effect_mask_map.iter())
        .chain(material.effect_distortion_map.iter())
        .map(|path| {
            let metadata = std::fs::metadata(root.join(path)).ok();
            (
                path.clone(),
                metadata.as_ref().and_then(|value| value.modified().ok()),
                metadata.map_or(0, |value| value.len()),
            )
        })
        .collect()
}
fn upload_effect_maps(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    root: &Path,
    material: &SurfaceTextures,
) -> Result<wgpu::TextureView, RendererError> {
    const SIZE: u32 = 256;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("surface effect masks"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 3,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (layer, path) in [
        material.effect_noise_map.as_ref(),
        material.effect_mask_map.as_ref(),
        material.effect_distortion_map.as_ref(),
    ]
    .into_iter()
    .enumerate()
    {
        let source = if let Some(path) = path {
            load(root, path)?
        } else {
            image::Rgba32FImage::from_pixel(
                1,
                1,
                image::Rgba(if layer == 2 {
                    [0.5, 0.5, 0.0, 1.0]
                } else {
                    [1.0; 4]
                }),
            )
        };
        let resized =
            image::imageops::resize(&source, SIZE, SIZE, image::imageops::FilterType::Triangle);
        let bytes: Vec<_> = resized
            .as_raw()
            .iter()
            .flat_map(|v| {
                half::f16::from_f32(if v.is_finite() {
                    v.clamp(-65504.0, 65504.0)
                } else {
                    0.0
                })
                .to_le_bytes()
            })
            .collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: u32::try_from(layer).unwrap_or_default(),
                },
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 8),
                rows_per_image: Some(SIZE),
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
    }
    Ok(texture.create_view(&wgpu::TextureViewDescriptor::default()))
}
