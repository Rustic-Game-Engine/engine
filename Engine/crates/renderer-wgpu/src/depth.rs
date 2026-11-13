//! Portable RGB24 depth plus rigid/deformed-object motion vectors. The prepass
//! samples ordinary color textures on GLES/WebGL rather than multisampled depth.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "GPU vertex and viewport budgets bound conversions"
)]
#![allow(
    clippy::too_many_lines,
    reason = "GPU descriptor construction keeps resource lifetimes explicit"
)]
use super::{
    CachedViewportMesh, HashMap, RendererError, VIEWPORT_DEPTH_FORMAT, ViewportScene, f32_bytes,
    validate_and_reflect_wgsl,
};
use engine_world::effects::EffectFrame;
use glam::Mat4;

const SHADER: &str = r"
struct Camera {current:mat4x4<f32>,previous:mat4x4<f32>,model:mat4x4<f32>,options:vec4<f32>,uv:vec4<f32>,effect:vec4<f32>,direction:vec4<f32>,effect_options:vec4<f32>,misc:vec4<f32>,wind:vec4<f32>,tint:vec4<f32>,previous_wind:vec4<f32>};
@group(0) @binding(0) var<uniform> camera:Camera;
struct Surface {uv:vec4<f32>,strength:vec4<f32>,mask:vec4<u32>,base_color:vec4<f32>,emissive_color:vec4<f32>,factors:vec4<f32>};
@group(1) @binding(0) var maps:texture_2d_array<f32>;
@group(1) @binding(1) var map_sampler:sampler;
@group(1) @binding(2) var<uniform> surface:Surface;
@group(1) @binding(3) var effect_maps:texture_2d_array<f32>;
fn mapped(layer:u32,uv:vec2<f32>,fallback:vec4<f32>)->vec4<f32>{if (surface.mask.x&(1u<<layer))!=0u{return textureSampleLevel(maps,map_sampler,uv,i32(layer),0.);}return fallback;}
fn effect_texcoord(animated:vec2<f32>,base:vec2<f32>)->vec2<f32>{let mask=select(1.,textureSampleLevel(effect_maps,map_sampler,base,1,0.).r,camera.effect_options.y>0.5);var uv=animated;if camera.misc.x<0.5{uv=mix(base,animated,mask);}if camera.effect_options.z>0.5{uv+=(textureSampleLevel(effect_maps,map_sampler,uv+vec2<f32>(camera.misc.y),2,0.).rg*2.-vec2<f32>(1.))*camera.effect_options.w*mask;}return uv;}
struct Output {@builtin(position) position:vec4<f32>,@location(0) previous:vec4<f32>,@location(1) world:vec3<f32>,@location(2) uv:vec2<f32>,@location(3) base_uv:vec2<f32>};
@vertex fn vs_main(@location(0) p:vec3<f32>,@location(1) normal:vec3<f32>,@location(2) uv:vec2<f32>,@location(3) previous:vec3<f32>)->Output {
 let sine=sin(camera.effect.x);let cosine=cos(camera.effect.x);let q=uv-vec2<f32>(0.5);
 let rotated=vec2<f32>(q.x*cosine-q.y*sine,q.x*sine+q.y*cosine)+vec2<f32>(0.5);
 var texcoord=(rotated*surface.uv.xy+surface.uv.zw)*camera.uv.xy+camera.uv.zw;
 let base_uv=uv*surface.uv.xy+surface.uv.zw;let geometry_uv=effect_texcoord(texcoord,base_uv);
 let displacement=camera.wind.xyz*p.y*p.y+normal*(mapped(5u,geometry_uv,vec4<f32>(0.5)).r-0.5)*surface.strength.x;
 var o:Output;o.position=camera.current*vec4<f32>(p+displacement,1.);o.previous=camera.previous*vec4<f32>(previous+displacement-camera.wind.xyz*p.y*p.y+camera.previous_wind.xyz*previous.y*previous.y,1.);o.world=(camera.model*vec4<f32>(p+displacement,1.)).xyz;o.uv=texcoord;o.base_uv=base_uv;return o;
}
struct DepthMotion {@location(0) depth:vec4<f32>,@location(1) motion:vec4<f32>};
@fragment fn fs_main(i:Output)->DepthMotion {
 let uv=effect_texcoord(i.uv,i.base_uv);
 let mask=select(1.,textureSampleLevel(effect_maps,map_sampler,i.base_uv,1,0.).r,camera.effect_options.y>0.5);let alpha=camera.effect.z*surface.base_color.a*mapped(0u,uv,vec4<f32>(1.)).a*mix(1.,camera.tint.a,mask);
 if alpha<=0.001 || (surface.factors.w>=0. && alpha<surface.factors.w){discard;}
 var noise=fract(sin(dot(floor(i.world*max(camera.effect.y,1.)),vec3<f32>(12.9898,78.233,37.719)))*43758.5453);
 if camera.effect_options.x>0.5{noise=textureSampleLevel(effect_maps,map_sampler,uv*camera.effect.y,0,0.).r;}if dot(camera.direction.xyz,camera.direction.xyz)>0.{noise=clamp(dot(i.world,camera.direction.xyz)+camera.direction.w,0.,1.);}if camera.effect_options.y>0.5{noise=textureSampleLevel(effect_maps,map_sampler,uv,1,0.).r;}
 if camera.misc.x>0.5&&camera.options.w>0. && noise<camera.options.w {discard;}
 let depth=u32(clamp(i.position.z,0.,1.)*16777215.);
 var o:DepthMotion;o.depth=vec4<f32>(f32(depth&255u),f32((depth>>8u)&255u),f32((depth>>16u)&255u),255.)/255.;
 let current=i.position.xy/camera.options.xy;
 let previous=i.previous.xy/max(abs(i.previous.w),0.000001)*vec2<f32>(0.5,-0.5)+vec2<f32>(0.5);
 o.motion=vec4<f32>(clamp(current-previous,vec2<f32>(-1.),vec2<f32>(1.)),i.previous.z/max(abs(i.previous.w),0.000001),camera.options.z*select(0.,1.,i.previous.w>0.));return o;
}
";
struct Targets {
    size: (u32, u32),
    _color: wgpu::Texture,
    view: wgpu::TextureView,
    _motion: wgpu::Texture,
    motion_view: wgpu::TextureView,
    _depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
}
struct Instance {
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
    previous_vertices: wgpu::Buffer,
    capacity: usize,
    positions: Vec<[f32; 3]>,
    model: Mat4,
    wind: glam::Vec3,
}
pub(super) struct DepthCapture {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    targets: Option<Targets>,
    instances: HashMap<u64, Instance>,
    previous_view: Option<Mat4>,
}
impl DepthCapture {
    pub fn new(
        device: &wgpu::Device,
        surface_layout: &wgpu::BindGroupLayout,
    ) -> Result<Self, RendererError> {
        validate_and_reflect_wgsl(SHADER)?;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects-depth-motion-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects-depth-motion-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effects-depth-motion-pipeline-layout"),
            bind_group_layouts: &[Some(&layout), Some(surface_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("effects-depth-motion-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: 32,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x3,
                                offset: 0,
                                shader_location: 0,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x3,
                                offset: 12,
                                shader_location: 1,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x2,
                                offset: 24,
                                shader_location: 2,
                            },
                        ],
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: 12,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 3,
                        }],
                    }),
                ],
            },
            primitive: wgpu::PrimitiveState {
                front_face: wgpu::FrontFace::Cw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: VIEWPORT_DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });
        Ok(Self {
            pipeline,
            layout,
            targets: None,
            instances: HashMap::new(),
            previous_view: None,
        })
    }
    pub fn reset(&mut self) {
        self.previous_view = None;
        self.instances.clear();
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        scene: &ViewportScene,
        frame: &EffectFrame,
    ) {
        if self
            .targets
            .as_ref()
            .is_none_or(|t| t.size != (width, height))
        {
            let make = |label, format, usage| {
                device.create_texture(&wgpu::TextureDescriptor {
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
                })
            };
            let color = make(
                "effects-depth-color",
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
            );
            let motion = make(
                "effects-motion-vectors",
                wgpu::TextureFormat::Rgba16Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            );
            let depth = make(
                "effects-depth-test",
                VIEWPORT_DEPTH_FORMAT,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            );
            let view = color.create_view(&wgpu::TextureViewDescriptor::default());
            let motion_view = motion.create_view(&wgpu::TextureViewDescriptor::default());
            let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
            self.targets = Some(Targets {
                size: (width, height),
                _color: color,
                view,
                _motion: motion,
                motion_view,
                _depth: depth,
                depth_view,
            });
            self.reset();
        }
        let vp = Mat4::from_cols_array(&scene.view_projection);
        let previous = self.previous_view.unwrap_or(vp);
        self.instances
            .retain(|key, _| scene.meshes.iter().any(|m| m.instance_key == *key));
        for mesh in &scene.meshes {
            let model = Mat4::from_cols_array(&mesh.model);
            let positions: Vec<_> = mesh.vertices.iter().map(|v| v.position).collect();
            let instance = self.instances.entry(mesh.instance_key).or_insert_with(|| {
                let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("effects-motion-instance"),
                    size: 336,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("effects-motion-instance-group"),
                    layout: &self.layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    }],
                });
                let capacity = positions.len().max(1).next_power_of_two();
                let previous_vertices = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("effects-previous-vertices"),
                    size: capacity as u64 * 12,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                Instance {
                    uniform,
                    group,
                    previous_vertices,
                    capacity,
                    positions: positions.clone(),
                    model,
                    wind: glam::Vec3::ZERO,
                }
            });
            let valid = self.previous_view.is_some() && instance.positions.len() == positions.len();
            if positions.len() > instance.capacity {
                instance.capacity = positions.len().next_power_of_two();
                instance.previous_vertices = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("effects-previous-vertices"),
                    size: instance.capacity as u64 * 12,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            let old = if valid {
                &instance.positions
            } else {
                &positions
            };
            let data: Vec<f32> = old.iter().flatten().copied().collect();
            if !data.is_empty() {
                queue.write_buffer(&instance.previous_vertices, 0, &f32_bytes(&data));
            }
            let modifier = frame
                .surfaces
                .iter()
                .find(|m| super::material::surface_matches(m, mesh));
            let mut values = (vp * model).to_cols_array().to_vec();
            values.extend((previous * instance.model).to_cols_array());
            values.extend(model.to_cols_array());
            values.extend([
                width as f32,
                height as f32,
                if valid { 1. } else { 0. },
                modifier.map_or(0., |m| m.dissolve),
            ]);
            values.extend(modifier.map_or([1., 1., 0., 0.], |m| {
                [m.uv_scale[0], m.uv_scale[1], m.uv_offset[0], m.uv_offset[1]]
            }));
            values.extend([
                modifier.map_or(0., |m| m.uv_rotation),
                modifier.map_or(24., |m| m.dissolve_noise_scale),
                mesh.color[3],
                0.,
            ]);
            values.extend(modifier.map_or([0.; 4], |m| {
                super::material::effect_direction(mesh, m.direction)
            }));
            values.extend(modifier.map_or([0.; 4], |m| {
                [
                    if m.noise_texture.is_some() { 1. } else { 0. },
                    if m.mask_texture.is_some() { 1. } else { 0. },
                    if m.distortion_texture.is_some() {
                        1.
                    } else {
                        0.
                    },
                    m.distortion_strength,
                ]
            }));
            values.extend(modifier.map_or([0.; 4], |m| {
                [if m.is_dissolve { 1. } else { 0. }, m.phase, 0., 0.]
            }));
            let wind = engine_world::effects::sample_render_wind(
                &frame.wind_zones,
                model.w_axis.truncate(),
                frame.post.time,
                mesh.textures.visibility_layers,
            ) * mesh.textures.wind_response.unwrap_or(0.);
            values.extend(wind.to_array());
            values.push(0.);
            values.extend(modifier.map_or([1.; 4], |m| m.color));
            values.extend(if valid { instance.wind } else { wind }.to_array());
            values.push(0.);
            instance.wind = wind;
            queue.write_buffer(&instance.uniform, 0, &f32_bytes(&values));
            instance.positions = positions;
            instance.model = model;
        }
        self.previous_view = Some(vp);
    }
    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.view)
    }
    pub fn motion_view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.motion_view)
    }
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &ViewportScene,
        meshes: &HashMap<u64, CachedViewportMesh>,
        surfaces: &super::surface::SurfaceCache,
    ) {
        let Some(t) = &self.targets else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("effects-depth-motion-pass"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: &t.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &t.motion_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.,
                            g: 0.,
                            b: 1.,
                            a: 0.,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                }),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &t.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        for mesh in &scene.meshes {
            if let (Some(gpu), Some(instance)) = (
                meshes.get(&mesh.mesh_key),
                self.instances.get(&mesh.instance_key),
            ) {
                pass.set_bind_group(0, &instance.group, &[]);
                pass.set_bind_group(1, surfaces.group(&mesh.textures), &[]);
                pass.set_vertex_buffer(0, gpu.vertices.slice(..));
                pass.set_vertex_buffer(1, instance.previous_vertices.slice(..));
                pass.set_index_buffer(gpu.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..gpu.index_count, 0, 0..1);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn shader_validates() {
        super::validate_and_reflect_wgsl(super::SHADER).unwrap();
    }
}
