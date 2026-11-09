//! A camera-space silhouette mask produces gameplay outlines through occluders.
//! The center mask is excluded, so highlighting never paints object interiors.
use super::*;
use engine_world::effects::EffectFrame;
const MASK_SHADER: &str = r"
struct Instance { mvp:mat4x4<f32> };
@group(0) @binding(0) var<uniform> instance:Instance;
@vertex fn vs_main(@location(0) position:vec3<f32>)->@builtin(position) vec4<f32> {
 return instance.mvp*vec4<f32>(position,1.0);
}
@fragment fn fs_main()->@location(0) vec4<f32> {return vec4<f32>(1.0);}
";
const COMPOSITE_SHADER: &str = r"
struct Settings {color:vec4<f32>,extent:vec4<f32>};
@group(0) @binding(0) var mask:texture_2d<f32>;
@group(0) @binding(1) var<uniform> settings:Settings;
@vertex fn vs_main(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32> {
 let p=vec2<f32>(f32((index<<1u)&2u),f32(index&2u));
 return vec4<f32>(p*2.0-1.0,0.0,1.0);
}
fn sample_mask(point:vec2<i32>)->f32 {
 let size=vec2<i32>(textureDimensions(mask));
 if any(point<vec2<i32>(0))||any(point>=size){return 0.0;}
 return textureLoad(mask,point,0).r;
}
@fragment fn fs_main(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32> {
 let p=vec2<i32>(position.xy);
 if sample_mask(p)>0.5 {return vec4<f32>(0.0);}
 var edge=0.0;
 let radius=clamp(settings.extent.z,0.0,64.0);
 for(var index=0u;index<32u;index+=1u) {
  let phase=f32(index)*6.2831853/32.0;
  let direction=vec2<f32>(cos(phase),sin(phase));
  for(var ring=1u;ring<=4u;ring+=1u) {
   let offset=vec2<i32>(round(direction*radius*f32(ring)/4.0));
   edge=max(edge,sample_mask(p+offset));
  }
 }
 return vec4<f32>(settings.color.rgb,settings.color.a*edge*min(radius,1.0));
}
";
struct OutlineGroup {
    uniform: wgpu::Buffer,
    bindings: wgpu::BindGroup,
}
pub(super) struct ThroughWallOutlines {
    mask_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    composite_layout: wgpu::BindGroupLayout,
    mask: Option<(u32, u32, wgpu::TextureView)>,
    groups: Vec<OutlineGroup>,
}
impl ThroughWallOutlines {
    #[allow(
        clippy::too_many_lines,
        reason = "silhouette mask and dilation pipelines share compatible GPU layout descriptors"
    )]
    pub fn new(device: &wgpu::Device, instance_layout: &wgpu::BindGroupLayout) -> Self {
        let mask_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("through-wall silhouette"),
            source: wgpu::ShaderSource::Wgsl(MASK_SHADER.into()),
        });
        let mask_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("outline mask layout"),
            bind_group_layouts: &[Some(instance_layout)],
            immediate_size: 0,
        });
        let mask_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("outline depth-independent mask"),
            layout: Some(&mask_layout),
            vertex: wgpu::VertexState {
                module: &mask_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 32,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    }],
                })],
            },
            primitive: wgpu::PrimitiveState {
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                ..wgpu::PrimitiveState::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &mask_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::R8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("outline composite bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("outline dilation shader"),
            source: wgpu::ShaderSource::Wgsl(COMPOSITE_SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("outline dilation layout"),
            bind_group_layouts: &[Some(&composite_layout)],
            immediate_size: 0,
        });
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("through-wall outline composite"),
            layout: Some(&layout),
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
                targets: &[Some(wgpu::ColorTargetState {
                    format: VIEWPORT_COLOR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            mask_pipeline,
            composite_pipeline,
            composite_layout,
            mask: None,
            groups: Vec::new(),
        }
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        frame: &EffectFrame,
    ) {
        let count = frame
            .outlines
            .iter()
            .filter(|outline| outline.through_walls)
            .count();
        if count == 0 {
            self.groups.clear();
            self.mask = None;
            return;
        }
        if !self
            .mask
            .as_ref()
            .is_some_and(|(w, h, _)| *w == width && *h == height)
        {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("through-wall silhouette mask"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.mask = Some((
                width,
                height,
                texture.create_view(&wgpu::TextureViewDescriptor::default()),
            ));
            self.groups.clear();
        }
        self.groups.truncate(count);
        let Some((_, _, view)) = &self.mask else {
            return;
        };
        for (index, outline) in frame
            .outlines
            .iter()
            .filter(|outline| outline.through_walls)
            .enumerate()
        {
            let mut color = outline.color;
            for channel in &mut color[..3] {
                let linear = if *channel <= 0.04045 {
                    *channel / 12.92
                } else {
                    ((*channel + 0.055) / 1.055).powf(2.4)
                };
                *channel = linear / ((1.0 - linear).max(0.0001));
            }
            let bytes = f32_bytes(&[
                color[0],
                color[1],
                color[2],
                color[3],
                0.0,
                0.0,
                outline.thickness,
                0.0,
            ]);
            if index == self.groups.len() {
                let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("outline color and thickness"),
                    contents: &bytes,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                });
                let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("outline composite"),
                    layout: &self.composite_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: uniform.as_entire_binding(),
                        },
                    ],
                });
                self.groups.push(OutlineGroup { uniform, bindings });
            } else {
                queue.write_buffer(&self.groups[index].uniform, 0, &bytes);
            }
        }
    }
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &ViewportScene,
        frame: &EffectFrame,
        meshes: &HashMap<u64, CachedViewportMesh>,
        instances: &HashMap<u64, CachedViewportInstance>,
        output: &wgpu::TextureView,
    ) {
        let Some((_, _, mask)) = &self.mask else {
            return;
        };
        for (index, outline) in frame
            .outlines
            .iter()
            .filter(|outline| outline.through_walls)
            .enumerate()
        {
            let Some(group) = self.groups.get(index) else {
                continue;
            };
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("hidden and visible target silhouette"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: mask,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    occlusion_query_set: None,
                    timestamp_writes: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.mask_pipeline);
                for mesh in &scene.meshes {
                    if !outline
                        .targets
                        .iter()
                        .any(|id| effect_entity_matches(*id, mesh.instance_key))
                    {
                        continue;
                    }
                    if let (Some(gpu), Some(instance)) = (
                        meshes.get(&mesh.mesh_key),
                        instances.get(&mesh.instance_key),
                    ) {
                        pass.set_bind_group(0, &instance.bind_group, &[]);
                        pass.set_vertex_buffer(0, gpu.vertices.slice(..));
                        pass.set_index_buffer(gpu.indices.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..gpu.index_count, 0, 0..1);
                    }
                }
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("through-wall border only"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.composite_pipeline);
            pass.set_bind_group(0, &group.bindings, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
