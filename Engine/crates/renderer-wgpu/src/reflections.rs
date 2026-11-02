//! Local baked environment probes using the same project-relative HDR decoding as surfaces.
//! A 2:1 equirectangular image is sampled in reflected world direction with box projection.
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
use std::path::Path;
pub(super) struct ReflectionCache {
    pub layout: wgpu::BindGroupLayout,
    groups: HashMap<engine_world::AssetId, wgpu::BindGroup>,
    fallback: wgpu::BindGroup,
}
impl ReflectionCache {
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("probe-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
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
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("probe-neutral"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let fallback = group(device, &layout, &texture);
        Self {
            layout,
            groups: HashMap::new(),
            fallback,
        }
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        root: &Path,
        frame: &EffectFrame,
        paths: &HashMap<engine_world::AssetId, String>,
    ) -> Result<(), RendererError> {
        let active: std::collections::HashSet<_> = frame.probes.iter().map(|p| p.cubemap).collect();
        self.groups.retain(|id, _| active.contains(id));
        for probe in &frame.probes {
            if self.groups.contains_key(&probe.cubemap) {
                continue;
            }
            let Some(path) = paths.get(&probe.cubemap) else {
                continue;
            };
            let mut image = surface::load(root, path)?;
            if image.width() != image.height() * 2 {
                return Err(RendererError::InvalidMesh(format!(
                    "baked reflection probe {path} requires a 2:1 equirectangular image"
                )));
            }
            if !path.to_lowercase().ends_with(".hdr") {
                for p in image.pixels_mut() {
                    for c in &mut p.0[..3] {
                        *c = if *c <= 0.04045 {
                            *c / 12.92
                        } else {
                            ((*c + 0.055) / 1.055).powf(2.4)
                        };
                    }
                }
            }
            let width = image.width().min(1024);
            let height = (width / 2).max(1);
            image = image::imageops::resize(
                &image,
                width,
                height,
                image::imageops::FilterType::Triangle,
            );
            let levels = width.max(height).ilog2() + 1;
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("baked-probe-HDR"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: levels,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            for level in 0..levels {
                let w = (width >> level).max(1);
                let h = (height >> level).max(1);
                let mip =
                    image::imageops::resize(&image, w, h, image::imageops::FilterType::Triangle);
                let bytes: Vec<_> = mip
                    .as_raw()
                    .iter()
                    .flat_map(|f| half::f16::from_f32(*f).to_le_bytes())
                    .collect();
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: level,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &bytes,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(w * 8),
                        rows_per_image: Some(h),
                    },
                    wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                );
            }
            self.groups
                .insert(probe.cubemap, group(device, &self.layout, &texture));
        }
        Ok(())
    }
    pub fn clear(&mut self) {
        self.groups.clear();
    }
    pub fn has(&self, id: engine_world::AssetId) -> bool {
        self.groups.contains_key(&id)
    }
    pub fn get(&self, id: Option<engine_world::AssetId>) -> &wgpu::BindGroup {
        id.and_then(|id| self.groups.get(&id))
            .unwrap_or(&self.fallback)
    }
}
fn group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture: &wgpu::Texture,
) -> wgpu::BindGroup {
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("probe-sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("probe-group"),
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
        ],
    })
}
