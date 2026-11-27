//! Shared bounded sprite texture array; particle flipbooks use authored UV rectangles.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "validated effect and GPU dimension budgets bound numeric conversions"
)]
#![allow(
    clippy::too_many_lines,
    reason = "GPU descriptor construction and effect extraction remain auditable transactions"
)]

use super::{HashMap, RendererError, surface};
use engine_world::effects::EffectFrame;
pub(super) struct SpriteAtlas {
    pub layout: wgpu::BindGroupLayout,
    group: Option<wgpu::BindGroup>,
    ids: Vec<engine_world::AssetId>,
    pub layers: HashMap<engine_world::AssetId, u32>,
}
impl SpriteAtlas {
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effect-sprites-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        Self {
            layout,
            group: None,
            ids: vec![],
            layers: HashMap::new(),
        }
    }
    pub fn clear(&mut self) {
        self.group = None;
        self.ids.clear();
        self.layers.clear();
    }
    pub fn group(&self) -> Option<&wgpu::BindGroup> {
        self.group.as_ref()
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        root: &std::path::Path,
        frame: &EffectFrame,
        paths: &HashMap<engine_world::AssetId, String>,
    ) -> Result<(), RendererError> {
        let ids: Vec<_> = frame
            .particles
            .iter()
            .filter_map(|p| p.texture)
            .chain(frame.geometry.iter().filter_map(|g| g.texture))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        if ids.len() > 63 {
            return Err(RendererError::InvalidMesh(
                "effect frame exceeds 63 sprite textures".into(),
            ));
        }
        if self.group.is_some() && ids == self.ids {
            return Ok(());
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("effect-sprite-array"),
            size: wgpu::Extent3d {
                width: 256,
                height: 256,
                depth_or_array_layers: ids.len() as u32 + 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.layers.clear();
        for (i, id) in ids.iter().enumerate() {
            let Some(path) = paths.get(id) else {
                continue;
            };
            let image = surface::load(root, path)?;
            let image =
                image::imageops::resize(&image, 256, 256, image::imageops::FilterType::Triangle);
            let bytes: Vec<_> = image
                .as_raw()
                .iter()
                .flat_map(|f| half::f16::from_f32(*f).to_le_bytes())
                .collect();
            let layer = i as u32 + 1;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256 * 8),
                    rows_per_image: Some(256),
                },
                wgpu::Extent3d {
                    width: 256,
                    height: 256,
                    depth_or_array_layers: 1,
                },
            );
            self.layers.insert(*id, layer);
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("effect-sprite-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        self.group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("effect-sprite-group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        }));
        self.ids = ids;
        Ok(())
    }
}
