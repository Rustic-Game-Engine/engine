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
    RendererError, VIEWPORT_DEPTH_FORMAT, VIEWPORT_SAMPLE_COUNT, f32_bytes,
    validate_and_reflect_wgsl,
};
use engine_world::effects::{BlendMode, EffectFrame};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

const SHADER: &str = r"
struct Camera { view_projection:mat4x4<f32> };
@group(0) @binding(0) var<uniform> camera:Camera;
@group(1) @binding(0) var sprites:texture_2d_array<f32>;
@group(1) @binding(1) var sprite_sampler:sampler;
struct Input { @location(0) position:vec3<f32>,@location(1) uv:vec2<f32>,@location(2) color:vec4<f32>,@location(3) sprite:f32 };
struct Output { @builtin(position) position:vec4<f32>,@location(0) uv:vec2<f32>,@location(1) color:vec4<f32>,@location(2) sprite:f32 };
@vertex fn vs_main(i:Input)->Output {var o:Output;o.position=camera.view_projection*vec4<f32>(i.position,1.);o.uv=i.uv;o.color=i.color;o.sprite=i.sprite;return o;}
fn shade(i:Output)->vec4<f32> {
    let radius=length(i.uv*2.-vec2<f32>(1.));
    let softness=select(1.,pow(max(1.-radius*radius,0.),2.),i.sprite>0.5 && i.sprite<1.5);
    var color=vec4<f32>(i.color.rgb,i.color.a*softness);
    if i.sprite>1.5 {color*=textureSampleLevel(sprites,sprite_sampler,i.uv,i32(i.sprite-2.),0.);}
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
}
fn vertex_bytes(vertices: &[Vertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * 40);
    for v in vertices {
        for f in v
            .position
            .into_iter()
            .chain(v.uv)
            .chain(v.color)
            .chain([v.sprite])
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
    sprites: super::sprites::SpriteAtlas,
    pipelines: [wgpu::RenderPipeline; 3],
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
    batches: [Batch; 3],
}
impl EffectsRenderer {
    pub fn new(device: &wgpu::Device) -> Result<Self, RendererError> {
        validate_and_reflect_wgsl(SHADER)?;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects-camera-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effects-camera"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("effects-camera-group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
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
                        array_stride: 40,
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
                        format: wgpu::TextureFormat::Rgba8Unorm,
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
                contents: &[0; 40],
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            }),
            capacity: 1,
            count: 0,
        });
        Ok(Self {
            sprites,
            pipelines,
            uniform,
            group,
            batches,
        })
    }
    pub fn clear_textures(&mut self) {
        self.sprites.clear();
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
        queue.write_buffer(&self.uniform, 0, &f32_bytes(&view_projection));
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
                    size: (batch.capacity as u64) * 40,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            queue.write_buffer(&batch.buffer, 0, &bytes);
        }
        Ok(())
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
        if geometry.material.is_some() {
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
                *channel *= geometry.emission.max(0.0);
            }
            let uv = geometry
                .mesh
                .tex_coords
                .get(index)
                .copied()
                .unwrap_or([0., 0.]);
            out.push(Vertex {
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
        b.position
            .distance_squared(camera)
            .total_cmp(&a.position.distance_squared(camera))
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
            *channel *= particle.emission.max(0.0);
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
                size: 2.,
                color: [1.; 4],
                mesh: None,
                material: None,
                texture: None,
                emission: 1.,
                blend: BlendMode::Alpha,
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
                size: 1.,
                color: [1., 0., 0., 1.],
                mesh: None,
                material: None,
                texture: None,
                emission: 1.,
                blend: BlendMode::Alpha,
            });
        renderer.set_effect_frame(&frame).unwrap();
        let particles = renderer.render(64, 64, &scene).unwrap().unwrap();
        let center = (32 * 64 + 32) * 4;
        assert!(particles.rgba8[center] > 200);
        assert_eq!(base.rgba8[center], 0);
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
        assert!(sprite.rgba8[center + 1] > 200 && sprite.rgba8[center] < 10);
        let mut multiply = frame.clone();
        multiply.particles[0].blend = BlendMode::Multiply;
        multiply.particles[0].color = [0.5; 4];
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
