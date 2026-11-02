//! Persistent half-float texture arrays. Color maps decode to linear light;
//! scalar and normal maps retain their raw channels. HDR keeps values above one.
use super::*;
use engine_world::{SurfaceTextures, TextureMap};
use std::path::Path;

pub(super) struct SurfaceCache {
    pub layout: wgpu::BindGroupLayout,
    groups: HashMap<String, wgpu::BindGroup>,
}
fn key(material: &SurfaceTextures) -> String {
    format!("{material:?}")
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
            ],
        });
        Self {
            layout,
            groups: HashMap::new(),
        }
    }
    pub fn clear(&mut self) {
        self.groups.clear();
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
            if self.groups.contains_key(&key) {
                continue;
            }
            let group = upload(device, queue, &self.layout, root, material)?;
            self.groups.insert(key, group);
        }
        self.groups.retain(|key, _| active.contains(key));
        Ok(())
    }
}
fn load(root: &Path, relative: &str) -> Result<image::Rgba32FImage, RendererError> {
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
        0.0,
        0.0,
    ]);
    bytes.extend_from_slice(&mask.to_le_bytes());
    bytes.extend_from_slice(&[0; 12]);
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
        ],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a physical or software graphics adapter"]
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
    }
}
