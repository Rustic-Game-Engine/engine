//! Shared dynamic vertex buffers for transparent effects; one draw per blend mode.
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

use super::{
    RendererError, VIEWPORT_COLOR_FORMAT, VIEWPORT_DEPTH_FORMAT, VIEWPORT_SAMPLE_COUNT, f32_bytes,
    validate_and_reflect_wgsl,
};
use engine_world::effects::{BlendMode, EffectFrame};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

const SHADER: &str = r"
struct Camera { view_projection:mat4x4<f32>,inverse:mat4x4<f32>,position:vec4<f32> };
@group(0) @binding(0) var<uniform> camera:Camera;
@group(0) @binding(1) var scene_depth:texture_2d<f32>;
@group(1) @binding(0) var sprites:texture_2d_array<f32>;
@group(1) @binding(1) var sprite_sampler:sampler;
struct Input { @location(0) position:vec3<f32>,@location(1) uv:vec2<f32>,@location(2) color:vec4<f32>,@location(3) sprite:f32,@location(4) soft:f32 };
struct Output { @builtin(position) position:vec4<f32>,@location(0) uv:vec2<f32>,@location(1) color:vec4<f32>,@location(2) sprite:f32,@location(3) soft:f32,@location(4) world:vec3<f32> };
@vertex fn vs_main(i:Input)->Output {var o:Output;o.position=camera.view_projection*vec4<f32>(i.position,1.);o.uv=i.uv;o.color=i.color;o.sprite=i.sprite;o.soft=i.soft;o.world=i.position;return o;}
fn linear(c:vec3<f32>)->vec3<f32>{return select(pow((c+vec3<f32>(0.055))/1.055,vec3<f32>(2.4)),c/12.92,c<=vec3<f32>(0.04045));}
fn shade(i:Output)->vec4<f32> {
    let radius=length(i.uv*2.-vec2<f32>(1.));
    let softness=select(1.,pow(max(1.-radius*radius,0.),2.),i.sprite>0.5 && i.sprite<1.5);
    var color=vec4<f32>(i.color.rgb,i.color.a*softness);
    if i.sprite>1.5 {let texture=textureSampleLevel(sprites,sprite_sampler,i.uv,i32(i.sprite-2.),0.);color*=vec4<f32>(linear(texture.rgb),texture.a);}
    if i.soft>0.&&camera.position.w>0.5{let dimensions=textureDimensions(scene_depth);let uv=i.position.xy/vec2<f32>(dimensions);let texel=clamp(vec2<i32>(i.position.xy),vec2<i32>(0),vec2<i32>(dimensions)-vec2<i32>(1));let encoded=textureLoad(scene_depth,texel,0).rgb;let d=dot(encoded,vec3<f32>(255.,65280.,16711680.))/16777215.;if d<0.99999{let q=camera.inverse*vec4<f32>(uv*vec2<f32>(2.,-2.)+vec2<f32>(-1.,1.),d,1.);color.a*=clamp((distance(q.xyz/q.w,camera.position.xyz)-distance(i.world,camera.position.xyz))/i.soft,0.,1.);}}
    return color;
}
@fragment fn fs_main(i:Output)->@location(0) vec4<f32> {return shade(i);}
@fragment fn fs_multiply(i:Output)->@location(0) vec4<f32> {let color=shade(i);return vec4<f32>(color.rgb*color.a,color.a);}
";
#[derive(Clone, Copy, Debug, PartialEq)]
struct Vertex {
    position: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
    sprite: f32,
    soft: f32,
}
fn vertex_bytes(vertices: &[Vertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * 44);
    for v in vertices {
        for f in v
            .position
            .into_iter()
            .chain(v.uv)
            .chain(v.color)
            .chain([v.sprite, v.soft])
        {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
    }
    bytes
}
struct Batch {
    buffer: wgpu::Buffer,
    capacity: usize,
    count: u32,
}
pub(super) struct EffectsRenderer {
    views: std::collections::HashMap<u64, Box<Self>>,
    sprites: super::sprites::SpriteAtlas,
    pipelines: [wgpu::RenderPipeline; 3],
    layout: wgpu::BindGroupLayout,
    depth_enabled: bool,
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
    batches: [Batch; 3],
}
impl EffectsRenderer {
    pub fn new(device: &wgpu::Device) -> Result<Self, RendererError> {
        validate_and_reflect_wgsl(SHADER)?;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects-camera-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effects-camera"),
            size: 144,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let fallback = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("particle-fallback-depth"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let fallback_view = fallback.create_view(&wgpu::TextureViewDescriptor::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("effects-camera-group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&fallback_view),
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects-geometry-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let sprites = super::sprites::SpriteAtlas::new(device);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effects-geometry-layout"),
            bind_group_layouts: &[Some(&layout), Some(&sprites.layout)],
            immediate_size: 0,
        });
        let pipelines = std::array::from_fn(|blend| {
            let multiply = blend == 2;
            let blend = match blend {
                1 => wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent::OVER,
                },
                2 => wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::Dst,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent::OVER,
                },
                _ => wgpu::BlendState::ALPHA_BLENDING,
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("effects-geometry-pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: 44,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x3,
                                offset: 0,
                                shader_location: 0,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x2,
                                offset: 12,
                                shader_location: 1,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x4,
                                offset: 20,
                                shader_location: 2,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32,
                                offset: 36,
                                shader_location: 3,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32,
                                offset: 40,
                                shader_location: 4,
                            },
                        ],
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: VIEWPORT_DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: VIEWPORT_SAMPLE_COUNT,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(if multiply { "fs_multiply" } else { "fs_main" }),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: VIEWPORT_COLOR_FORMAT,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        });
        let batches = std::array::from_fn(|_| Batch {
            buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("effects-empty-vertices"),
                contents: &[0; 44],
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            }),
            capacity: 1,
            count: 0,
        });
        Ok(Self {
            views: std::collections::HashMap::default(),
            sprites,
            pipelines,
            layout,
            depth_enabled: false,
            uniform,
            group,
            batches,
        })
    }
    pub fn set_depth(&mut self, device: &wgpu::Device, depth: &wgpu::TextureView) {
        self.group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("soft-particle-depth-group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
            ],
        });
        self.depth_enabled = true;
    }
    pub fn clear_textures(&mut self) {
        self.sprites.clear();
        self.views.clear();
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "prepare combines frame camera and project asset context without backend types leaking"
    )]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &EffectFrame,
        view_projection: [f32; 16],
        camera: [f32; 3],
        root: &std::path::Path,
        paths: &std::collections::HashMap<engine_world::AssetId, String>,
    ) -> Result<(), RendererError> {
        self.sprites.prepare(device, queue, root, frame, paths)?;
        let vertices = build_vertices(
            frame,
            Mat4::from_cols_array(&view_projection),
            Vec3::from_array(camera),
            &self.sprites.layers,
        )?;
        let mut camera_values = view_projection.to_vec();
        camera_values.extend(
            Mat4::from_cols_array(&view_projection)
                .inverse()
                .to_cols_array(),
        );
        camera_values.extend(camera);
        camera_values.push(if self.depth_enabled { 1. } else { 0. });
        queue.write_buffer(&self.uniform, 0, &f32_bytes(&camera_values));
        for (batch, vertices) in self.batches.iter_mut().zip(vertices) {
            batch.count = u32::try_from(vertices.len()).map_err(|_| {
                RendererError::InvalidMesh("effect vertex count exceeds u32".into())
            })?;
            if vertices.is_empty() {
                continue;
            }
            let bytes = vertex_bytes(&vertices);
            if vertices.len() > batch.capacity {
                batch.capacity = vertices.len().next_power_of_two();
                batch.buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("effects-dynamic-vertices"),
                    size: (batch.capacity as u64) * 44,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            queue.write_buffer(&batch.buffer, 0, &bytes);
        }
        Ok(())
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "per-view resources require camera, frame and project context"
    )]
    pub fn prepare_view(
        &mut self,
        key: u64,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &EffectFrame,
        view_projection: [f32; 16],
        camera: [f32; 3],
        root: &std::path::Path,
        paths: &std::collections::HashMap<engine_world::AssetId, String>,
    ) -> Result<(), RendererError> {
        if !self.views.contains_key(&key) {
            if self.views.len() >= 32 {
                self.views.clear();
            }
            self.views.insert(key, Box::new(Self::new(device)?));
        }
        self.views
            .get_mut(&key)
            .expect("inserted effects view")
            .prepare(device, queue, frame, view_projection, camera, root, paths)
    }
    pub fn draw_view<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, key: u64) {
        if let Some(view) = self.views.get(&key) {
            view.draw(pass);
        }
    }
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        for (pipeline, batch) in self.pipelines.iter().zip(&self.batches) {
            if batch.count == 0 {
                continue;
            }
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.group, &[]);
            if let Some(group) = self.sprites.group() {
                pass.set_bind_group(1, group, &[]);
            }
            pass.set_vertex_buffer(0, batch.buffer.slice(..));
            pass.draw(0..batch.count, 0..1);
        }
    }
}
fn blend_index(blend: BlendMode) -> usize {
    match blend {
        BlendMode::Alpha => 0,
        BlendMode::Additive => 1,
        BlendMode::Multiply => 2,
    }
}
fn build_vertices(
    frame: &EffectFrame,
    vp: Mat4,
    camera: Vec3,
    layers: &std::collections::HashMap<engine_world::AssetId, u32>,
) -> Result<[Vec<Vertex>; 3], RendererError> {
    let mut batches: [Vec<Vertex>; 3] = std::array::from_fn(|_| Vec::new());
    let inverse = vp.inverse();
    let center = inverse.project_point3(Vec3::new(0., 0., 0.5));
    let right = (inverse.project_point3(Vec3::new(1., 0., 0.5)) - center).normalize_or_zero();
    let up = (inverse.project_point3(Vec3::new(0., 1., 0.5)) - center).normalize_or_zero();
    if !right.is_finite() || !up.is_finite() {
        return Err(RendererError::InvalidMesh(
            "invalid effects camera projection".into(),
        ));
    }
    let mut vertex_budget = 1_000_000usize;
    for geometry in &frame.geometry {
        if geometry.mesh.indices.len() > vertex_budget {
            return Err(RendererError::InvalidMesh(
                "effect frame exceeds one million geometry vertices".into(),
            ));
        }
        vertex_budget -= geometry.mesh.indices.len();
        // PBR geometry is appended to the existing material pipeline by append_effect_frame.
        if geometry.pbr || geometry.material.is_some() {
            continue;
        }
        let out = &mut batches[blend_index(geometry.blend)];
        for &index in &geometry.mesh.indices {
            let index = index as usize;
            let Some(&position) = geometry.mesh.positions.get(index) else {
                return Err(RendererError::InvalidMesh(
                    "effect index out of bounds".into(),
                ));
            };
            let mut color = geometry
                .colors
                .get(index)
                .copied()
                .unwrap_or(geometry.color);
            for channel in &mut color[..3] {
                *channel = super::srgb_channel_linear(*channel) * geometry.emission.max(0.0);
            }
            let uv = geometry
                .mesh
                .tex_coords
                .get(index)
                .copied()
                .unwrap_or([0., 0.]);
            out.push(Vertex {
                soft: 0.,
                position,
                uv,
                color,
                sprite: geometry
                    .texture
                    .map_or(0., |id| layers.get(&id).copied().unwrap_or(0) as f32 + 2.),
            });
        }
    }
    // Back-to-front particles inside each blend batch, without per-particle draw calls.
    let mut particles: Vec<_> = frame
        .particles
        .iter()
        .filter(|p| p.mesh.is_none())
        .collect();
    particles.sort_by(|a, b| {
        if a.entity == b.entity {
            match a.sort_mode {
                engine_world::effects::ParticleSortMode::OldestFirst => {
                    b.sort_key.total_cmp(&a.sort_key)
                }
                engine_world::effects::ParticleSortMode::YoungestFirst => {
                    a.sort_key.total_cmp(&b.sort_key)
                }
                engine_world::effects::ParticleSortMode::None => std::cmp::Ordering::Equal,
                engine_world::effects::ParticleSortMode::Distance => b
                    .position
                    .distance_squared(camera)
                    .total_cmp(&a.position.distance_squared(camera)),
            }
        } else {
            a.entity.cmp(&b.entity)
        }
    });
    for particle in particles {
        let (sin, cos) = particle.rotation.sin_cos();
        let base_right = right;
        let base_up = up;
        let mut right = (base_right * cos + base_up * sin) * particle.size * 0.5;
        let mut up = (base_up * cos - base_right * sin) * particle.size * 0.5;
        if particle.stretch > 0. && particle.velocity.length_squared() > 0.0001 {
            let screen_velocity = Vec3::new(
                particle.velocity.dot(base_right),
                particle.velocity.dot(base_up),
                0.,
            )
            .normalize_or_zero();
            up = (base_right * screen_velocity.x + base_up * screen_velocity.y)
                * (particle.size + particle.velocity.length() * particle.stretch)
                * 0.5;
            right = (base_right * screen_velocity.y - base_up * screen_velocity.x)
                * particle.size
                * 0.5;
        }
        let mut color = particle.color;
        for channel in &mut color[..3] {
            *channel = super::srgb_channel_linear(*channel) * particle.emission.max(0.0);
        }
        let positions = [
            particle.position - right - up,
            particle.position + right - up,
            particle.position + right + up,
            particle.position - right + up,
        ];
        let uv = [[0., 1.], [1., 1.], [1., 0.], [0., 0.]];
        for i in [0, 1, 2, 0, 2, 3] {
            batches[blend_index(particle.blend)].push(Vertex {
                soft: particle.soft_particle_distance,
                position: positions[i].to_array(),
                uv: if particle.texture.is_some() {
                    [
                        particle.uv_rect[0]
                            + uv[i][0] * (particle.uv_rect[2] - particle.uv_rect[0]),
                        particle.uv_rect[1]
                            + uv[i][1] * (particle.uv_rect[3] - particle.uv_rect[1]),
                    ]
                } else {
                    uv[i]
                },
                color,
                sprite: particle
                    .texture
                    .map_or(1., |id| layers.get(&id).copied().unwrap_or(0) as f32 + 2.),
            });
        }
    }
    if batches.iter().flatten().any(|v| {
        v.position
            .into_iter()
            .chain(v.uv)
            .chain(v.color)
            .any(|f| !f.is_finite())
    }) {
        return Err(RendererError::InvalidMesh(
            "non-finite effect vertices".into(),
        ));
    }
    Ok(batches)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shader_validates() {
        validate_and_reflect_wgsl(SHADER).unwrap();
    }
    #[test]
    fn particles_batch_into_camera_facing_quads() {
        let mut frame = EffectFrame::default();
        frame
            .particles
            .push(engine_world::effects::ParticleInstance {
                entity: engine_world::EntityId::new(),
                position: Vec3::ZERO,
                velocity: Vec3::ZERO,
                stretch: 0.,
                uv_rect: [0., 0., 1., 1.],
                rotation: 0.,
                rotation3: Vec3::ZERO,
                scale: Vec3::ONE,
                size: 2.,
                color: [1.; 4],
                mesh: None,
                material: None,
                texture: None,
                emission: 1.,
                blend: BlendMode::Alpha,
                cast_shadows: false,
                receive_shadows: false,
                wind: Vec3::ZERO,
                sort_mode: engine_world::effects::ParticleSortMode::Distance,
                sort_key: 0.,
                soft_particle_distance: 0.,
            });
        let vertices = build_vertices(
            &frame,
            Mat4::IDENTITY,
            Vec3::Z,
            &std::collections::HashMap::default(),
        )
        .unwrap();
        assert_eq!(vertices[0].len(), 6);
        assert!(vertices[1].is_empty());
        assert!(
            Vec3::from_array(vertices[0][0].position).distance(Vec3::new(-1., -1., 0.))
                < f32::EPSILON
        );
        assert!(
            Vec3::from_array(vertices[0][2].position).distance(Vec3::new(1., 1., 0.))
                < f32::EPSILON
        );
        let mut farther = frame.particles[0].clone();
        farther.position.z = 2.;
        farther.sort_key = 1.;
        frame.particles[0].sort_key = 10.;
        frame.particles.push(farther);
        for (mode, first_z) in [
            (engine_world::effects::ParticleSortMode::Distance, 2.),
            (engine_world::effects::ParticleSortMode::OldestFirst, 0.),
            (engine_world::effects::ParticleSortMode::YoungestFirst, 2.),
            (engine_world::effects::ParticleSortMode::None, 0.),
        ] {
            for particle in &mut frame.particles {
                particle.sort_mode = mode;
            }
            let vertices = build_vertices(
                &frame,
                Mat4::IDENTITY,
                Vec3::ZERO,
                &std::collections::HashMap::default(),
            )
            .unwrap();
            assert!(
                (vertices[0][0].position[2] - first_z).abs() < f32::EPSILON,
                "configured {mode:?} order ignored"
            );
        }
    }
}

#[cfg(test)]
mod gpu_tests {
    use super::*;
    use crate::{BackendRequest, SceneViewportRenderer, ViewportScene};
    #[test]
    #[ignore = "requires a physical or software graphics adapter"]
    fn effects_frame_renders_particles_post_distortion_fog_and_cleanup() {
        let mut renderer = SceneViewportRenderer::new(BackendRequest::Auto).unwrap();
        let scene = ViewportScene {
            environment: engine_world::SceneEnvironment::default(),
            camera_position: [0., 0., 0.],
            view_projection: Mat4::IDENTITY.to_cols_array(),
            meshes: vec![],
            lights: vec![],
            guides: vec![],
            grid_vertices: vec![],
            clear_color: [0., 0., 0.2, 1.],
        };
        let base = renderer.render(64, 64, &scene).unwrap().unwrap();
        let mut frame = EffectFrame::default();
        frame
            .particles
            .push(engine_world::effects::ParticleInstance {
                entity: engine_world::EntityId::new(),
                position: Vec3::new(0., 0., 0.5),
                velocity: Vec3::ZERO,
                stretch: 0.,
                uv_rect: [0., 0., 1., 1.],
                rotation: 0.,
                rotation3: Vec3::ZERO,
                scale: Vec3::ONE,
                size: 1.,
                color: [1., 0., 0., 1.],
                mesh: None,
                material: None,
                texture: None,
                emission: 4.,
                blend: BlendMode::Alpha,
                cast_shadows: false,
                receive_shadows: false,
                wind: Vec3::ZERO,
                sort_mode: engine_world::effects::ParticleSortMode::Distance,
                sort_key: 0.,
                soft_particle_distance: 0.,
            });
        renderer.set_effect_frame(&frame).unwrap();
        let particles = renderer.render(64, 64, &scene).unwrap().unwrap();
        let center = (32 * 64 + 32) * 4;
        assert!(particles.rgba8[center] > 175);
        assert_eq!(base.rgba8[center], 0);
        // Read the linear floating-point scene attachment independently of its
        // display output: bright particles must survive above1 for HDR bloom.
        let device = &renderer.context.device;
        let queue = &renderer.context.queue;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hdr-radiance-test"),
            size: 64 * 64 * 8,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            renderer.targets.as_ref().unwrap().color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(512),
                    rows_per_image: Some(64),
                },
            },
            wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sender.send(result).unwrap();
            });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(10)),
            })
            .unwrap();
        receiver.recv().unwrap().unwrap();
        let mapped = buffer.slice(..).get_mapped_range().unwrap();
        let offset = (32 * 64 + 32) * 8;
        let red = half::f16::from_le_bytes([mapped[offset], mapped[offset + 1]]).to_f32();
        assert!(red > 3.7, "HDR source clipped: {red}");
        drop(mapped);
        buffer.unmap();
        frame.post.bloom = 1.;
        frame.post.bloom_threshold = 1.5;
        frame.post.bloom_radius = 4.;
        renderer.set_effect_frame(&frame).unwrap();
        let bloomed = renderer.render(64, 64, &scene).unwrap().unwrap();
        let halo = (32 * 64 + 49) * 4;
        assert!(
            bloomed.rgba8[halo] > particles.rgba8[halo],
            "radiance above1 must create a bloom halo beyond the source"
        );
        frame.post.bloom = 0.;
        let mut intersection_scene = scene.clone();
        intersection_scene.meshes.push(crate::ViewportMesh {
            instance_key: 941,
            mesh_key: 941,
            vertices: [[-1., -1.], [1., -1.], [1., 1.], [-1., 1.]]
                .into_iter()
                .map(|p| crate::ViewportVertex {
                    position: [p[0], p[1], 0.55],
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
        renderer.set_effect_frame(&frame).unwrap();
        let hard = renderer
            .render(64, 64, &intersection_scene)
            .unwrap()
            .unwrap();
        frame.particles[0].soft_particle_distance = 0.2;
        renderer.set_effect_frame(&frame).unwrap();
        let soft = renderer
            .render(64, 64, &intersection_scene)
            .unwrap()
            .unwrap();
        assert!(
            soft.rgba8[center] > 20 && soft.rgba8[center].saturating_add(20) < hard.rgba8[center],
            "encoded scene depth must fade a particle approaching a surface: hard={} soft={}",
            hard.rgba8[center],
            soft.rgba8[center]
        );
        frame.particles[0].soft_particle_distance = 0.;

        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join("assets")).unwrap();
        let image_path = project.path().join("assets/green.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 255, 0, 255]))
            .save(&image_path)
            .unwrap();
        let meta = engine_assets::AssetMeta::new("rustic.image", 1);
        engine_assets::save_meta(&engine_assets::sidecar_path(&image_path), &meta).unwrap();
        let mut textured = frame.clone();
        textured.particles[0].texture = Some(meta.asset_id);
        textured.particles[0].color = [1.; 4];
        renderer.set_environment_root(project.path());
        renderer.set_effect_frame(&textured).unwrap();
        let sprite = renderer.render(64, 64, &scene).unwrap().unwrap();
        assert!(sprite.rgba8[center + 1] > 175 && sprite.rgba8[center] < 10);
        let mut multiply = frame.clone();
        multiply.particles[0].blend = BlendMode::Multiply;
        multiply.particles[0].color = [0.5; 4];
        multiply.particles[0].emission=1.;
        renderer.set_effect_frame(&multiply).unwrap();
        let darkened = renderer.render(64, 64, &scene).unwrap().unwrap();
        assert!(darkened.rgba8[center + 2] < base.rgba8[center + 2] - 5);
        frame.post.distortion = 0.15;
        frame.post.time = 1.2;
        renderer.set_effect_frame(&frame).unwrap();
        let warped = renderer.render(64, 64, &scene).unwrap().unwrap();
        assert_ne!(particles.rgba8, warped.rgba8);
        frame = EffectFrame::default();
        frame.fog.push(engine_world::effects::RenderFogVolume {
            entity: engine_world::EntityId::new(),
            inverse_transform: Mat4::IDENTITY,
            dimensions: [2.; 3],
            shape: engine_world::effects::VolumeShape::Box,
            density: 3.,
            color: [1., 0., 0.],
            height_falloff: 0.,
            edge_falloff: 0.1,
            noise_scale: 1.,
            noise_speed: 1.,
            steps: 32,
            noise_texture: None,
            anisotropy: 0.,
            light_scattering: 0.,
            max_distance: 100.,
            temporal_filtering: 0.,
            time: 0.,
        });
        renderer.set_effect_frame(&frame).unwrap();
        let fog = renderer.render(64, 64, &scene).unwrap().unwrap();
        assert!(fog.rgba8[center] > 150);
        renderer.set_effect_frame(&EffectFrame::default()).unwrap();
        let cleared = renderer.render(64, 64, &scene).unwrap().unwrap();
        assert_eq!(base.rgba8, cleared.rgba8);
        assert_eq!(renderer.stats().device_generations, 1);
        assert_eq!(renderer.stats().target_generations, 1);
    }
}
