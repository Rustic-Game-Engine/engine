//! Linear HDR atmosphere, motion-aware temporal history, then bloom/grade/tone map.
//! Every stage reads a texture distinct from its attachment. History is reset on
//! resize, project changes and camera cuts; depth rejection prevents disocclusion trails.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "bounded GPU extents and effect budgets constrain numeric conversions"
)]
#![allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "GPU resource transactions keep binding and lifetime relationships explicit"
)]
use super::{
    RendererError, VIEWPORT_COLOR_FORMAT, ViewportScene, f32_bytes, validate_and_reflect_wgsl,
};
use engine_world::effects::{EffectFrame, VolumeShape};
use glam::{Mat4, Vec3};
use std::{collections::HashMap, path::Path};
const COMMON: &str = include_str!("post_common.wgsl");
pub(super) const ATMOSPHERE_SHADER: &str = include_str!("atmosphere.wgsl");
pub(super) const TEMPORAL_SHADER: &str = include_str!("temporal.wgsl");
pub(super) const FINAL_SHADER: &str = include_str!("final_post.wgsl");
pub(super) const FLARE_SHADER: &str = include_str!("flare_visibility.wgsl");
const SCENE_FLOATS: usize = 1280;
struct NoiseAtlas {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    key: Vec<(
        u32,
        engine_world::AssetId,
        Option<std::time::SystemTime>,
        u64,
    )>,
}
struct Image {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}
struct Targets {
    size: (u32, u32),
    atmosphere: Image,
    guide: Image,
    history: [Image; 2],
    history_guide: [Image; 2],
    output: Image,
    flare_visibility: [Image; 2],
    groups: [[wgpu::BindGroup; 4]; 2],
}
pub(super) struct PostRenderer {
    layout: wgpu::BindGroupLayout,
    pipelines: [wgpu::RenderPipeline; 4],
    uniform: wgpu::Buffer,
    scene_uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    noise_sampler: wgpu::Sampler,
    noise: Option<NoiseAtlas>,
    targets: Option<Targets>,
    write: usize,
    valid: bool,
    previous_view: Mat4,
    previous_camera: Vec3,
    previous_time: f32,
    frame_number: u64,
    history_sources: Vec<engine_world::EntityId>,
}
impl PostRenderer {
    pub fn new(device: &wgpu::Device) -> Result<Self, RendererError> {
        let mut entries = Vec::new();
        for binding in 0..11 {
            let ty = match binding {
                1 | 9 => wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                2 | 4 => wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                _ => wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float {
                        filterable: !matches!(binding, 3 | 5 | 7),
                    },
                    view_dimension: if binding == 8 {
                        wgpu::TextureViewDimension::D2Array
                    } else {
                        wgpu::TextureViewDimension::D2
                    },
                    multisampled: false,
                },
            };
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty,
                count: None,
            });
        }
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hdr-post-layout"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hdr-post-pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let mut pipelines = Vec::new();
        for (index, body) in [
            FLARE_SHADER,
            ATMOSPHERE_SHADER,
            TEMPORAL_SHADER,
            FINAL_SHADER,
        ]
        .into_iter()
        .enumerate()
        {
            let source = format!("{COMMON}\n{body}");
            validate_and_reflect_wgsl(&source)?;
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("hdr-post-shader"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            let formats = if index == 1 {
                vec![VIEWPORT_COLOR_FORMAT, VIEWPORT_COLOR_FORMAT]
            } else {
                vec![if index == 3 {
                    wgpu::TextureFormat::Rgba8Unorm
                } else {
                    VIEWPORT_COLOR_FORMAT
                }]
            };
            let targets: Vec<_> = formats
                .into_iter()
                .map(|format| {
                    Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })
                })
                .collect();
            pipelines.push(
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(
                        [
                            "flare-visibility-pipeline",
                            "hdr-atmosphere-pipeline",
                            "hdr-temporal-pipeline",
                            "hdr-final-pipeline",
                        ][index],
                    ),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &targets,
                    }),
                    multiview_mask: None,
                    cache: None,
                }),
            );
        }
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hdr-post-settings"),
            size: 176,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let scene_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hdr-atmosphere-settings"),
            size: (SCENE_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("hdr-post-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let noise_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fog-noise-repeat-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let pipelines = pipelines
            .try_into()
            .map_err(|_| RendererError::Gpu("HDR pipeline construction failed".into()))?;
        Ok(Self {
            layout,
            pipelines,
            uniform,
            scene_uniform,
            sampler,
            noise_sampler,
            noise: None,
            targets: None,
            write: 0,
            valid: false,
            previous_view: Mat4::IDENTITY,
            previous_camera: Vec3::ZERO,
            previous_time: 0.,
            frame_number: 0,
            history_sources: Vec::new(),
        })
    }
    pub fn reset_history(&mut self) {
        self.valid = false;
        self.frame_number = 0;
    }
    pub fn clear(&mut self) {
        self.reset_history();
        self.noise = None;
        self.targets = None;
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        motion: &wgpu::TextureView,
        width: u32,
        height: u32,
        frame: &EffectFrame,
        root: &Path,
        paths: &HashMap<engine_world::AssetId, String>,
    ) -> Result<(), RendererError> {
        let sources: Vec<_> = frame
            .fog
            .iter()
            .map(|f| f.entity)
            .chain(frame.shafts.iter().map(|f| f.entity))
            .chain(frame.flares.iter().map(|f| f.entity))
            .collect();
        if self.history_sources != sources {
            self.reset_history();
            self.history_sources = sources;
        }
        let assets: Vec<_> = frame
            .fog
            .iter()
            .take(8)
            .enumerate()
            .map(|(i, f)| (i as u32 + 1, f.noise_texture))
            .chain(
                frame
                    .distortions
                    .iter()
                    .take(4)
                    .enumerate()
                    .map(|(i, f)| (i as u32 + 9, f.noise_texture)),
            )
            .chain(
                frame
                    .flares
                    .iter()
                    .take(4)
                    .enumerate()
                    .map(|(i, f)| (i as u32 + 13, f.texture)),
            )
            .chain(
                frame
                    .overlays
                    .iter()
                    .take(8)
                    .enumerate()
                    .map(|(i, f)| (i as u32 + 17, f.screen_mask)),
            )
            .filter_map(|(slot, id)| id.map(|id| (slot, id)))
            .collect();
        let mut key = Vec::new();
        for &(slot, id) in &assets {
            let path = paths.get(&id).ok_or_else(|| {
                RendererError::InvalidMesh(format!("post effect texture {id:?} is missing"))
            })?;
            let metadata = std::fs::metadata(root.join(path)).map_err(|e| {
                RendererError::InvalidMesh(format!("post effect texture {path}: {e}"))
            })?;
            key.push((slot, id, metadata.modified().ok(), metadata.len()));
        }
        let noise_changed = self.noise.as_ref().is_none_or(|n| n.key != key);
        if noise_changed {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("post-effect-texture-array"),
                size: wgpu::Extent3d {
                    width: 64,
                    height: 64,
                    depth_or_array_layers: 25,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            for &(slot, id) in &assets {
                let image = super::surface::load(root, &paths[&id])?;
                let image =
                    image::imageops::resize(&image, 64, 64, image::imageops::FilterType::Triangle);
                let bytes: Vec<_> = image
                    .as_raw()
                    .iter()
                    .map(|f| (f.clamp(0., 1.) * 255.).round() as u8)
                    .collect();
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: 0,
                            z: slot,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &bytes,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(64),
                    },
                    wgpu::Extent3d {
                        width: 64,
                        height: 64,
                        depth_or_array_layers: 1,
                    },
                );
            }
            let view = texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            });
            self.noise = Some(NoiseAtlas {
                _texture: texture,
                view,
                key,
            });
        }
        let resize = self
            .targets
            .as_ref()
            .is_none_or(|t| t.size != (width, height));
        if resize || noise_changed {
            let image = |label, format, usage| {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                });
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                Image { texture, view }
            };
            let flags =
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
            let atmosphere = image("hdr-atmosphere", VIEWPORT_COLOR_FORMAT, flags);
            let guide = image(
                "hdr-temporal-guide",
                VIEWPORT_COLOR_FORMAT,
                flags | wgpu::TextureUsages::COPY_SRC,
            );
            let history =
                std::array::from_fn(|_| image("hdr-history", VIEWPORT_COLOR_FORMAT, flags));
            let history_guide = std::array::from_fn(|_| {
                image(
                    "hdr-history-depth",
                    VIEWPORT_COLOR_FORMAT,
                    wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                )
            });
            let output = image(
                "hdr-display-output",
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            );
            let flare_visibility = std::array::from_fn(|_| {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("flare-visibility-history"),
                    size: wgpu::Extent3d {
                        width: 4,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: VIEWPORT_COLOR_FORMAT,
                    usage: flags,
                    view_formats: &[],
                });
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                Image { texture, view }
            });
            let noise = &self
                .noise
                .as_ref()
                .ok_or_else(|| RendererError::Gpu("fog noise array missing".into()))?
                .view;
            let groups = std::array::from_fn(|write| {
                std::array::from_fn(|stage| {
                    let input = [source, source, &atmosphere.view, &history[write].view][stage];
                    let velocity = if stage == 2 { &guide.view } else { motion };
                    let old = 1 - write;
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("hdr-post-input"),
                        layout: &self.layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(input),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&self.sampler),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: self.uniform.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 3,
                                resource: wgpu::BindingResource::TextureView(depth),
                            },
                            wgpu::BindGroupEntry {
                                binding: 4,
                                resource: self.scene_uniform.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 5,
                                resource: wgpu::BindingResource::TextureView(velocity),
                            },
                            wgpu::BindGroupEntry {
                                binding: 6,
                                resource: wgpu::BindingResource::TextureView(&history[old].view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 7,
                                resource: wgpu::BindingResource::TextureView(
                                    &history_guide[old].view,
                                ),
                            },
                            wgpu::BindGroupEntry {
                                binding: 8,
                                resource: wgpu::BindingResource::TextureView(noise),
                            },
                            wgpu::BindGroupEntry {
                                binding: 9,
                                resource: wgpu::BindingResource::Sampler(&self.noise_sampler),
                            },
                            wgpu::BindGroupEntry {
                                binding: 10,
                                resource: wgpu::BindingResource::TextureView(
                                    &flare_visibility[if stage == 0 { old } else { write }].view,
                                ),
                            },
                        ],
                    })
                })
            });
            self.targets = Some(Targets {
                size: (width, height),
                atmosphere,
                guide,
                history,
                history_guide,
                output,
                flare_visibility,
                groups,
            });
            self.write = 0;
            self.reset_history();
        }
        Ok(())
    }
    pub fn render<'a>(
        &'a mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        scene: &ViewportScene,
        frame: &EffectFrame,
    ) -> Option<&'a wgpu::Texture> {
        let t = self.targets.as_ref()?;
        let camera = Vec3::from_array(scene.camera_position);
        let vp = Mat4::from_cols_array(&scene.view_projection);
        if (vp.inverse().project_point3(Vec3::new(0., 0., 0.5)) - camera)
            .normalize_or_zero()
            .dot(
                (self
                    .previous_view
                    .inverse()
                    .project_point3(Vec3::new(0., 0., 0.5))
                    - self.previous_camera)
                    .normalize_or_zero(),
            )
            < 0.5
            || camera.distance(self.previous_camera) > 20.
            || frame.post.time + 0.001 < self.previous_time
        {
            self.valid = false;
        }
        let temporal = frame
            .fog
            .iter()
            .map(|f| f.temporal_filtering)
            .chain(frame.shafts.iter().map(|s| s.temporal_filtering))
            .fold(frame.post.temporal_weight, f32::max);
        queue.write_buffer(
            &self.uniform,
            0,
            &f32_bytes(&settings_values(frame, temporal, self.valid)),
        );
        queue.write_buffer(
            &self.scene_uniform,
            0,
            &f32_bytes(&scene_values(
                scene,
                frame,
                self.previous_view,
                self.previous_camera,
                t.size,
                self.frame_number,
                (frame.post.time - self.previous_time).clamp(1. / 120., 0.25),
            )),
        );
        let clear = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("flare-visibility-pass"),
                color_attachments: &[clear(&t.flare_visibility[self.write].view)],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipelines[0]);
            pass.set_bind_group(0, &t.groups[self.write][0], &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hdr-atmosphere-pass"),
                color_attachments: &[clear(&t.atmosphere.view), clear(&t.guide.view)],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipelines[1]);
            pass.set_bind_group(0, &t.groups[self.write][1], &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hdr-temporal-pass"),
                color_attachments: &[clear(&t.history[self.write].view)],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipelines[2]);
            pass.set_bind_group(0, &t.groups[self.write][2], &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_texture(
            t.guide.texture.as_image_copy(),
            t.history_guide[self.write].texture.as_image_copy(),
            t.guide.texture.size(),
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hdr-display-pass"),
                color_attachments: &[clear(&t.output.view)],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipelines[3]);
            pass.set_bind_group(0, &t.groups[self.write][3], &[]);
            pass.draw(0..3, 0..1);
        }
        self.previous_view = vp;
        self.previous_camera = camera;
        self.previous_time = frame.post.time;
        self.frame_number = self.frame_number.wrapping_add(1);
        self.valid = true;
        self.write = 1 - self.write;
        Some(&t.output.texture)
    }
}
fn settings_values(frame: &EffectFrame, temporal: f32, valid: bool) -> [f32; 44] {
    let mut p = frame.post;
    if !frame.overlays.is_empty() {
        p.overlay = [0.; 4];
        p.overlay_add = [0.; 3];
        p.overlay_multiply = [1.; 3];
    }
    [
        p.saturation,
        p.contrast,
        p.brightness,
        p.exposure,
        p.vignette,
        p.chromatic_aberration,
        p.film_grain,
        p.time,
        p.distortion,
        p.distortion_scale,
        p.distortion_speed,
        0.,
        p.bloom,
        p.bloom_threshold,
        p.bloom_radius,
        0.,
        p.tint[0],
        p.tint[1],
        p.tint[2],
        1.,
        p.overlay[0],
        p.overlay[1],
        p.overlay[2],
        p.overlay[3],
        p.overlay_multiply[0],
        p.overlay_multiply[1],
        p.overlay_multiply[2],
        1.,
        p.overlay_add[0],
        p.overlay_add[1],
        p.overlay_add[2],
        p.depth_of_field,
        p.motion_blur,
        p.motion_blur_samples as f32,
        0.,
        0.,
        p.focus_distance,
        p.focus_range,
        0.,
        0.,
        temporal,
        if valid { 1. } else { 0. },
        0.,
        0.,
    ]
}
fn scene_values(
    scene: &ViewportScene,
    frame: &EffectFrame,
    previous: Mat4,
    previous_camera: Vec3,
    size: (u32, u32),
    number: u64,
    delta: f32,
) -> Vec<f32> {
    let vp = Mat4::from_cols_array(&scene.view_projection);
    let mut values = vp.inverse().to_cols_array().to_vec();
    values.extend(previous.to_cols_array());
    values.extend(previous.inverse().to_cols_array());
    values.extend(vp.to_cols_array());
    values.extend(scene.camera_position);
    values.push(0.);
    values.extend(previous_camera.to_array());
    values.push(0.);
    let env = if scene.environment.enabled {
        scene.environment.clone()
    } else {
        engine_world::SceneEnvironment::default()
    };
    values.extend(env.sun_direction);
    values.push(0.);
    values.extend(env.sun_color);
    values.push(env.sun_intensity);
    values.extend(env.ambient_color);
    values.push(env.ambient_intensity);
    values.extend([
        frame.fog.len().min(8) as f32,
        frame.shafts.len().min(4) as f32,
        frame.flares.len().min(4) as f32,
        scene.lights.len().min(32) as f32,
    ]);
    values.extend([
        size.0 as f32,
        size.1 as f32,
        frame.post.time,
        (number % 4096) as f32,
    ]);
    values.extend([
        frame.distortions.len().min(4) as f32,
        frame.overlays.len().min(8) as f32,
        delta,
        0.,
    ]);
    for (index, fog) in frame.fog.iter().take(8).enumerate() {
        values.extend(fog.inverse_transform.to_cols_array());
        values.extend(fog.dimensions);
        values.push(fog.density);
        values.extend(fog.color);
        values.push(fog.height_falloff);
        values.extend([
            match fog.shape {
                VolumeShape::Sphere => 1.,
                VolumeShape::Cylinder => 2.,
                _ => 0.,
            },
            fog.edge_falloff,
            fog.noise_scale,
            fog.steps.clamp(4, 128) as f32,
        ]);
        values.extend([
            fog.anisotropy,
            fog.light_scattering,
            fog.max_distance,
            fog.temporal_filtering,
        ]);
        values.extend([
            fog.noise_speed,
            if fog.noise_texture.is_some() {
                (index + 1) as f32
            } else {
                0.
            },
            fog.time,
            0.,
        ]);
    }
    values.resize(64 + 32 + 8 * 36, 0.);
    for shaft in frame.shafts.iter().take(4) {
        let position = shaft
            .light
            .and_then(|id| scene.lights.iter().find(|l| l.entity == id))
            .map_or(shaft.position, |l| {
                Mat4::from_cols_array(&l.transform).w_axis.truncate()
            });
        values.extend(shaft.inverse_transform.to_cols_array());
        values.extend(position.to_array());
        values.push(shaft.intensity);
        values.extend(shaft.color);
        values.push(shaft.density);
        values.extend([
            shaft.samples.clamp(4, 128) as f32,
            shaft.max_distance,
            shaft.anisotropy,
            if shaft.occlusion { 1. } else { 0. },
        ]);
        values.extend([
            shaft.temporal_filtering,
            if shaft.localized { 1. } else { 0. },
            0.,
            0.,
        ]);
        values.extend(shaft.dimensions);
        values.push(match shaft.shape {
            VolumeShape::Sphere => 1.,
            VolumeShape::Cylinder => 2.,
            _ => 0.,
        });
    }
    values.resize(64 + 32 + 8 * 36 + 4 * 36, 0.);
    for (index, flare) in frame.flares.iter().take(4).enumerate() {
        let position = flare
            .light
            .and_then(|id| scene.lights.iter().find(|l| l.entity == id))
            .map_or(flare.position, |l| {
                Mat4::from_cols_array(&l.transform).w_axis.truncate()
            });
        let p = vp * position.extend(1.);
        let ndc = p.truncate() / p.w;
        values.extend([
            ndc.x.midpoint(1.),
            (1. - ndc.y) * 0.5,
            if p.w > 0.
                && position.distance(Vec3::from_array(scene.camera_position)) <= flare.max_distance
            {
                ndc.z
            } else {
                -1.
            },
            flare.intensity,
        ]);
        values.extend(flare.color);
        values.push(flare.scale * 0.025);
        values.extend([
            flare.ghosts.min(16) as f32,
            flare.ghost_spacing,
            flare.streak_length * 0.01,
            flare.streak_orientation,
        ]);
        values.extend([
            if flare.texture.is_some() {
                (index + 13) as f32
            } else {
                0.
            },
            flare.fade_speed,
            if flare.occlusion { 1. } else { 0. },
            flare.max_distance,
        ]);
    }
    values.resize(64 + 32 + 8 * 36 + 4 * 36 + 4 * 16, 0.);
    for light in scene.lights.iter().take(32) {
        let (_, rotation, position) =
            Mat4::from_cols_array(&light.transform).to_scale_rotation_translation();
        values.extend(position.to_array());
        values.push(match light.kind {
            engine_world::LightKind::Directional => 0.,
            engine_world::LightKind::Point => 1.,
            engine_world::LightKind::Spot => 2.,
        });
        values.extend((rotation * Vec3::Z).to_array());
        values.push(light.range);
        values.extend(light.color);
        values.push(light.intensity);
        values.extend([
            light.spot_outer_angle_radians.cos(),
            (light.spot_outer_angle_radians * 0.8).cos(),
            0.,
            0.,
        ]);
    }
    values.resize(64 + 32 + 8 * 36 + 4 * 36 + 4 * 16 + 32 * 16, 0.);
    for (index, effect) in frame.distortions.iter().take(4).enumerate() {
        values.extend(effect.inverse_transform.to_cols_array());
        values.extend([
            effect.radius,
            effect.strength,
            effect.edge_falloff,
            if effect.screen_space { 1. } else { 0. },
        ]);
        values.extend([
            effect.noise_scale,
            effect.speed,
            if effect.noise_texture.is_some() {
                (index + 9) as f32
            } else {
                0.
            },
            if effect.depth_masking { 1. } else { 0. },
        ]);
        values.extend([
            effect.refraction_offset[0],
            effect.refraction_offset[1],
            effect.time,
            0.,
        ]);
    }
    values.resize(64 + 32 + 8 * 36 + 4 * 36 + 4 * 16 + 32 * 16 + 4 * 28, 0.);
    for (index, overlay) in frame.overlays.iter().take(8).enumerate() {
        values.extend([
            overlay.color[0],
            overlay.color[1],
            overlay.color[2],
            overlay.opacity,
        ]);
        values.extend([
            match overlay.blend {
                engine_world::effects::BlendMode::Additive => 1.,
                engine_world::effects::BlendMode::Multiply => 2.,
                engine_world::effects::BlendMode::Alpha => 0.,
            },
            if overlay.screen_mask.is_some() {
                (index + 17) as f32
            } else {
                0.
            },
            overlay.vignette,
            match overlay.stacking {
                engine_world::effects::OverlayStacking::Replace => 1.,
                engine_world::effects::OverlayStacking::Maximum => 2.,
                engine_world::effects::OverlayStacking::Composite => 0.,
            },
        ]);
    }
    values.resize(SCENE_FLOATS, 0.);
    debug_assert_eq!(values.len(), SCENE_FLOATS);
    values
}
#[cfg(test)]
mod tests {
    #[test]
    fn cpu_scene_layout_matches_gpu_uniform_size() {
        let module =
            naga::front::wgsl::parse_str(&format!("{}\n{}", super::COMMON, super::FINAL_SHADER))
                .unwrap();
        let span = module
            .types
            .iter()
            .find_map(|(_, ty)| {
                if ty.name.as_deref() == Some("Scene")
                    && let naga::TypeInner::Struct { span, .. } = ty.inner
                {
                    return Some(span);
                }
                None
            })
            .unwrap();
        assert_eq!(span as usize, super::SCENE_FLOATS * 4);
    }
    #[test]
    #[ignore = "requires a physical or software graphics adapter"]
    fn overlay_mask_flare_occlusion_and_motion_blur_change_real_pixels() {
        use super::*;
        use crate::{BackendRequest, SceneViewportRenderer, ViewportMesh, ViewportVertex};
        use engine_world::effects::{
            BlendMode, OverlayStacking, RenderLensFlare, RenderScreenOverlay,
        };
        let root = tempfile::tempdir().unwrap();
        image::RgbaImage::from_raw(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 255])
            .unwrap()
            .save(root.path().join("mask.png"))
            .unwrap();
        let mut renderer = SceneViewportRenderer::new(BackendRequest::Auto).unwrap();
        renderer.set_environment_root(root.path());
        let mask = engine_world::AssetId::new();
        renderer.effect_asset_paths.insert(mask, "mask.png".into());
        let mut scene = ViewportScene {
            environment: engine_world::SceneEnvironment::default(),
            camera_position: [0.; 3],
            view_projection: Mat4::IDENTITY.to_cols_array(),
            meshes: vec![],
            lights: vec![],
            guides: vec![],
            grid_vertices: vec![],
            clear_color: [0., 0., 0., 1.],
        };
        let mut frame = EffectFrame::default();
        frame.overlays.push(RenderScreenOverlay {
            entity: engine_world::EntityId::new(),
            color: [1., 0., 0., 1.],
            opacity: 1.,
            vignette: 0.,
            blend: BlendMode::Alpha,
            screen_mask: Some(mask),
            stacking: OverlayStacking::Composite,
        });
        renderer.set_effect_frame(&frame).unwrap();
        let overlay = renderer.render(32, 32, &scene).unwrap().unwrap();
        assert_eq!(overlay.rgba8[(16 * 32 + 3) * 4], 0);
        assert_eq!(
            overlay.rgba8[(16 * 32 + 28) * 4],
            255,
            "white screen mask must expose the overlay"
        );
        frame.overlays.clear();
        frame.flares.push(RenderLensFlare {
            entity: engine_world::EntityId::new(),
            light: None,
            position: Vec3::new(0., 0., 0.5),
            intensity: 4.,
            scale: 8.,
            color: [1., 0., 0.],
            ghosts: 0,
            ghost_spacing: 0.3,
            streak_length: 0.,
            streak_orientation: 0.,
            texture: None,
            occlusion: true,
            max_distance: 100.,
            fade_speed: 4.,
        });
        frame.post.time = 0.1;
        renderer.set_effect_frame(&frame).unwrap();
        let first = renderer.render(32, 32, &scene).unwrap().unwrap();
        let center = (16 * 32 + 16) * 4;
        let mut visible = first.clone();
        for _ in 0..12 {
            frame.post.time += 0.1;
            renderer.set_effect_frame(&frame).unwrap();
            visible = renderer.render(32, 32, &scene).unwrap().unwrap();
        }
        assert!(
            visible.rgba8[center] > first.rgba8[center].saturating_add(15),
            "flare visibility must fade in over time"
        );
        scene.meshes.push(ViewportMesh {
            instance_key: 709,
            mesh_key: 709,
            vertices: [[-0.2, -0.2], [0.2, -0.2], [0.2, 0.2], [-0.2, 0.2]]
                .into_iter()
                .map(|p| ViewportVertex {
                    position: [p[0], p[1], 0.25],
                    normal: [0., 0., -1.],
                    uv: [0.; 2],
                })
                .collect(),
            indices: vec![0, 2, 1, 0, 3, 2],
            model: Mat4::IDENTITY.to_cols_array(),
            color: [0., 0., 0., 1.],
            selected: false,
            textures: engine_world::SurfaceTextures {
                cast_shadows: false,
                ..engine_world::SurfaceTextures::default()
            },
        });
        frame.post.time += 0.1;
        renderer.set_effect_frame(&frame).unwrap();
        let hide_start = renderer.render(32, 32, &scene).unwrap().unwrap();
        let mut hidden = hide_start.clone();
        for _ in 0..12 {
            frame.post.time += 0.1;
            renderer.set_effect_frame(&frame).unwrap();
            hidden = renderer.render(32, 32, &scene).unwrap().unwrap();
        }
        assert!(
            hide_start.rgba8[center] > hidden.rgba8[center].saturating_add(30),
            "depth occlusion must fade the flare out"
        );
        frame.flares.clear();
        scene.environment = engine_world::SceneEnvironment {
            enabled: true,
            sky_color: [0.; 3],
            sun_intensity: 0.,
            ambient_color: [1.; 3],
            ambient_intensity: 1.,
            haze_density: 0.,
            ..engine_world::SceneEnvironment::default()
        };
        scene.meshes[0].color = [1., 0., 0., 1.];
        scene.meshes[0].model = Mat4::from_translation(Vec3::new(-0.3, 0., 0.)).to_cols_array();
        renderer.set_effect_frame(&frame).unwrap();
        renderer.render(32, 32, &scene).unwrap();
        scene.meshes[0].model = Mat4::from_translation(Vec3::new(0.3, 0., 0.)).to_cols_array();
        frame.post.motion_blur = 1.;
        frame.post.motion_blur_samples = 16;
        renderer.set_effect_frame(&frame).unwrap();
        let blurred = renderer.render(32, 32, &scene).unwrap().unwrap();
        frame.post.motion_blur = 0.;
        renderer.set_effect_frame(&frame).unwrap();
        let sharp = renderer.render(32, 32, &scene).unwrap().unwrap();
        assert!(
            sharp
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .zip(blurred.rgba8.as_chunks::<4>().0)
                .filter(|(a, b)| a[0] > b[0].saturating_add(10))
                .count()
                > 4,
            "object motion vectors must soften the moving silhouette"
        );
    }
    #[test]
    fn shaders_validate() {
        for body in [
            super::FLARE_SHADER,
            super::ATMOSPHERE_SHADER,
            super::TEMPORAL_SHADER,
            super::FINAL_SHADER,
        ] {
            super::validate_and_reflect_wgsl(&format!("{}\n{body}", super::COMMON)).unwrap();
        }
    }
}
