//! Portable RGB24 scene depth. Sampling multisampled depth is unsupported by
//! wgpu's GLES shader translator; this retained prepass works on both GPU paths.
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
    CachedViewportInstance, CachedViewportMesh, HashMap, RendererError, VIEWPORT_DEPTH_FORMAT,
    ViewportScene, validate_and_reflect_wgsl,
};
const SHADER: &str = r"
struct Camera {mvp:mat4x4<f32>};
@group(0) @binding(0) var<uniform> camera:Camera;
@vertex fn vs_main(@location(0) position:vec3<f32>)->@builtin(position) vec4<f32> {return camera.mvp*vec4<f32>(position,1.);}
@fragment fn fs_main(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32> {
 let depth=u32(clamp(position.z,0.,1.)*16777215.);
 return vec4<f32>(f32(depth&255u),f32((depth>>8u)&255u),f32((depth>>16u)&255u),255.)/255.;
}
";
struct Targets {
    size: (u32, u32),
    _color: wgpu::Texture,
    view: wgpu::TextureView,
    _depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
}
pub(super) struct DepthCapture {
    pipeline: wgpu::RenderPipeline,
    targets: Option<Targets>,
}
impl DepthCapture {
    pub fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
    ) -> Result<Self, RendererError> {
        validate_and_reflect_wgsl(SHADER)?;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects-depth-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effects-depth-layout"),
            bind_group_layouts: &[Some(layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("effects-depth-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
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
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Ok(Self {
            pipeline,
            targets: None,
        })
    }
    pub fn prepare(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self
            .targets
            .as_ref()
            .is_some_and(|t| t.size == (width, height))
        {
            return;
        }
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("effects-depth-color"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("effects-depth-test"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: VIEWPORT_DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = color.create_view(&wgpu::TextureViewDescriptor::default());
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
        self.targets = Some(Targets {
            size: (width, height),
            _color: color,
            view,
            _depth: depth,
            depth_view,
        });
    }
    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.view)
    }
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &ViewportScene,
        meshes: &HashMap<u64, CachedViewportMesh>,
        instances: &HashMap<u64, CachedViewportInstance>,
    ) {
        let Some(targets) = &self.targets else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("effects-depth-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &targets.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &targets.depth_view,
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
                instances.get(&mesh.instance_key),
            ) {
                pass.set_bind_group(0, &instance.bind_group, &[]);
                pass.set_vertex_buffer(0, gpu.vertices.slice(..));
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
