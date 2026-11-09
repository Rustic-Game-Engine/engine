//! Six real HDR scene renders and roughness-prefiltered local environment maps.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "validated 2048-face sizes and 64-sample convolution budgets bound conversions"
)]
#![allow(
    clippy::too_many_lines,
    reason = "GPU render-pass descriptors are kept together for resource/lifetime auditing"
)]
use super::reflections::ProbeTexture;
use super::{
    RendererError, SceneViewportRenderer, VIEWPORT_DEPTH_FORMAT, VIEWPORT_SAMPLE_COUNT,
    ViewportScene, f32_bytes,
};
use engine_assets::{TextureArtifact, TexturePixels};
use engine_world::effects::{QualityLevel, RenderReflectionProbe};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt as _;

const FILTER_SHADER: &str = r"
struct Params {roughness:f32,samples:f32,pad:vec2<f32>};
@group(0) @binding(0) var source:texture_cube<f32>;
@group(0) @binding(1) var linear_sampler:sampler;
@group(0) @binding(2) var<uniform> params:Params;
struct Varying {@builtin(position) position:vec4<f32>,@location(0) uv:vec2<f32>};
@vertex fn vs_main(@builtin(vertex_index) index:u32)->Varying {
 let points=array<vec2<f32>,3>(vec2<f32>(-1.,-1.),vec2<f32>(3.,-1.),vec2<f32>(-1.,3.));
 var out:Varying;out.position=vec4<f32>(points[index],0.,1.);out.uv=points[index]*vec2<f32>(0.5,-0.5)+0.5;return out;
}
@fragment fn fs_main(input:Varying)->@location(0) vec4<f32> {
 let theta=(input.uv.x-0.5)*6.283185307;let phi=input.uv.y*3.141592654;
 let n=vec3<f32>(cos(theta)*sin(phi),cos(phi),sin(theta)*sin(phi));
 if params.roughness<0.001 {return vec4<f32>(textureSampleLevel(source,linear_sampler,n,0.).rgb,1.);}
 let reference=select(vec3<f32>(0.,1.,0.),vec3<f32>(1.,0.,0.),abs(n.y)>0.98);
 let tangent=normalize(cross(reference,n));let bitangent=cross(n,tangent);
 let a=max(params.roughness*params.roughness,0.002);var color=vec3<f32>(0.);var weight=0.;
 for(var i=0u;i<64u;i=i+1u){if f32(i)>=params.samples {break;}
   let x=(f32(i)+0.5)/params.samples;let y=fract(f32(i)*0.61803398875);
   let angle=6.283185307*y;let cos_theta=sqrt((1.-x)/(1.+(a*a-1.)*x));let sin_theta=sqrt(max(0.,1.-cos_theta*cos_theta));
   let h=normalize(tangent*(cos(angle)*sin_theta)+bitangent*(sin(angle)*sin_theta)+n*cos_theta);
   let light=normalize(2.*dot(n,h)*h-n);let ndl=max(dot(n,light),0.);
   color+=textureSampleLevel(source,linear_sampler,light,0.).rgb*ndl;weight+=ndl;
 }
 return vec4<f32>(color/max(weight,0.0001),1.);
}";

pub(super) struct ProbeFilter {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}
impl ProbeFilter {
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("probe-convolution-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::Cube,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
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
            label: Some("GGX-probe-convolution"),
            source: wgpu::ShaderSource::Wgsl(FILTER_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("probe-convolution"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("probe-linear-HDR-convolution"),
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
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("probe-cubemap-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            layout,
            pipeline,
            sampler,
        }
    }
    pub fn filter(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cube: wgpu::Texture,
        resolution: u32,
        quality: QualityLevel,
    ) -> ProbeTexture {
        let width = resolution * 4;
        let height = resolution * 2;
        let mip_count = width.ilog2() + 1;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("roughness-prefiltered-probe"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: mip_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let cube_view = cube.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        });
        let samples = match quality {
            QualityLevel::Low => 8.,
            QualityLevel::Medium => 24.,
            QualityLevel::High => 64.,
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("probe-prefilter-mip-chain"),
        });
        for level in 0..mip_count {
            let roughness = level as f32 / (mip_count - 1) as f32;
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("probe-roughness"),
                contents: &f32_bytes(&[roughness, samples, 0., 0.]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("probe-prefilter-source"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&cube_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform.as_entire_binding(),
                    },
                ],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("GGX-probe-roughness-level"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        ProbeTexture {
            _texture: texture,
            view,
            mip_count,
            cube,
            resolution,
            quality,
        }
    }
    pub fn upload_faces(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        faces: &[TextureArtifact; 6],
        resolution: u32,
        quality: QualityLevel,
    ) -> Result<ProbeTexture, RendererError> {
        let cube = cube_texture(device, resolution);
        for (face, image) in faces.iter().enumerate() {
            let pixels: Vec<f32> = match &image.pixels {
                TexturePixels::Rgba8(bytes) => bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|p| {
                        [
                            linear(f32::from(p[0]) / 255.),
                            linear(f32::from(p[1]) / 255.),
                            linear(f32::from(p[2]) / 255.),
                            f32::from(p[3]) / 255.,
                        ]
                    })
                    .collect(),
                TexturePixels::Rgba32Float(values) => values.clone(),
            };
            let image = image::Rgba32FImage::from_raw(image.width, image.height, pixels)
                .ok_or_else(|| {
                    RendererError::InvalidMesh("cubemap face pixel count is invalid".into())
                })?;
            let resized = image::imageops::resize(
                &image,
                resolution,
                resolution,
                image::imageops::FilterType::Triangle,
            );
            let bytes: Vec<_> = resized
                .as_raw()
                .iter()
                .flat_map(|v| half::f16::from_f32(*v).to_le_bytes())
                .collect();
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &cube,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: face as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(resolution * 8),
                    rows_per_image: Some(resolution),
                },
                wgpu::Extent3d {
                    width: resolution,
                    height: resolution,
                    depth_or_array_layers: 1,
                },
            );
        }
        Ok(self.filter(device, queue, cube, resolution, quality))
    }
    pub fn upload_panorama(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &image::Rgba32FImage,
        resolution: u32,
        quality: QualityLevel,
    ) -> Result<ProbeTexture, RendererError> {
        let faces = std::array::from_fn(|face| {
            let mut pixels = Vec::with_capacity((resolution * resolution * 4) as usize);
            for y in 0..resolution {
                for x in 0..resolution {
                    let uv = glam::Vec2::new(
                        (x as f32 + 0.5) / resolution as f32 * 2. - 1.,
                        (y as f32 + 0.5) / resolution as f32 * 2. - 1.,
                    );
                    let direction = face_direction(face, uv.x, uv.y);
                    let u = direction.z.atan2(direction.x) / std::f32::consts::TAU + 0.5;
                    let v = direction.y.clamp(-1., 1.).acos() / std::f32::consts::PI;
                    let pixel = image.get_pixel(
                        ((u * image.width() as f32) as u32).min(image.width() - 1),
                        ((v * image.height() as f32) as u32).min(image.height() - 1),
                    );
                    pixels.extend(pixel.0);
                }
            }
            TextureArtifact {
                width: resolution,
                height: resolution,
                srgb: false,
                pixels: TexturePixels::Rgba32Float(pixels),
            }
        });
        self.upload_faces(device, queue, &faces, resolution, quality)
    }
}
fn linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
fn face_direction(face: usize, u: f32, v: f32) -> Vec3 {
    match face {
        0 => Vec3::new(1., -v, -u),
        1 => Vec3::new(-1., -v, u),
        2 => Vec3::new(u, 1., v),
        3 => Vec3::new(u, -1., -v),
        4 => Vec3::new(u, -v, 1.),
        _ => Vec3::new(-u, -v, -1.),
    }
    .normalize()
}
fn cube_texture(device: &wgpu::Device, resolution: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("six-face-linear-HDR-probe"),
        size: wgpu::Extent3d {
            width: resolution,
            height: resolution,
            depth_or_array_layers: 6,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

impl SceneViewportRenderer {
    /// Refreshes due probes on the same GPU and PBR pipeline as the visible scene.
    /// Separate ordered submissions preserve each face's camera uniforms without
    /// duplicating scene geometry or blocking for a CPU readback.
    pub(super) fn update_reflection_captures(
        &mut self,
        scene: &ViewportScene,
    ) -> Result<(), RendererError> {
        let requests = self.reflections.capture_requests(&self.effect_frame);
        for probe in requests.into_iter().take(4) {
            let texture = self.capture_probe(&probe, scene)?;
            self.reflections.install_capture(&probe, texture);
        }
        self.reflections
            .prepare_blends(&self.context.device, scene, &self.effect_frame);
        Ok(())
    }
    fn capture_probe(
        &mut self,
        probe: &RenderReflectionProbe,
        source: &ViewportScene,
    ) -> Result<ProbeTexture, RendererError> {
        let resolution = probe.resolution.clamp(16, 2048);
        self.reflections.check_budget(probe.cubemap, resolution)?;
        let cube = cube_texture(&self.context.device, resolution);
        let multisample = self
            .context
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("probe-MSAA"),
                size: wgpu::Extent3d {
                    width: resolution,
                    height: resolution,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: VIEWPORT_SAMPLE_COUNT,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
        let multisample_view = multisample.create_view(&wgpu::TextureViewDescriptor::default());
        let depth = self
            .context
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("probe-depth-stencil"),
                size: wgpu::Extent3d {
                    width: resolution,
                    height: resolution,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: VIEWPORT_SAMPLE_COUNT,
                dimension: wgpu::TextureDimension::D2,
                format: VIEWPORT_DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
        let mut scene = source.clone();
        scene.guides.clear();
        scene.grid_vertices.clear();
        scene.camera_position = probe.capture_position.to_array();
        scene.meshes.retain(|mesh| {
            mesh.textures.visibility_layers & probe.visibility_layers != 0
                && !super::effect_entity_matches(probe.entity, mesh.instance_key)
                && mesh.textures.effect_source.is_none_or(|entity| {
                    self.effect_frame
                        .effect_render_distances
                        .get(&entity)
                        .is_none_or(|(center, radius, max_distance)| {
                            center.distance(probe.capture_position) - radius <= *max_distance
                        })
                })
        });
        scene.lights.truncate(match probe.quality {
            QualityLevel::Low => 8,
            QualityLevel::Medium => 16,
            QualityLevel::High => 32,
        });
        for mesh in &scene.meshes {
            super::validate_viewport_mesh(mesh)?;
            self.ensure_mesh(mesh)?;
            self.ensure_instance(mesh.instance_key);
        }
        let directions = [
            (Vec3::X, -Vec3::Y),
            (-Vec3::X, -Vec3::Y),
            (Vec3::Y, Vec3::Z),
            (-Vec3::Y, -Vec3::Z),
            (Vec3::Z, -Vec3::Y),
            (-Vec3::Z, -Vec3::Y),
        ];
        let mut capture_frame = self.effect_frame.clone();
        let layers = &capture_frame.effect_layers;
        let distances = &capture_frame.effect_render_distances;
        let visible = |entity: engine_world::EntityId| {
            entity != probe.entity
                && layers.get(&entity).copied().unwrap_or(u32::MAX) & probe.visibility_layers != 0
                && distances
                    .get(&entity)
                    .is_none_or(|(center, radius, max_distance)| {
                        center.distance(probe.capture_position) - radius <= *max_distance
                    })
        };
        capture_frame
            .particles
            .retain(|particle| visible(particle.entity));
        capture_frame
            .geometry
            .retain(|geometry| visible(geometry.entity));
        for (face, (direction, up)) in directions.into_iter().enumerate() {
            let distance = Vec3::from_array(probe.dimensions).length().max(10.) * 100.;
            scene.view_projection = (glam::camera::rh::proj::directx::perspective(
                std::f32::consts::FRAC_PI_2,
                1.,
                0.05,
                distance,
            ) * glam::camera::rh::view::look_at_mat4(
                probe.capture_position,
                probe.capture_position + direction,
                up,
            ))
            .to_cols_array();
            self.sky.prepare(
                &self.context.device,
                &self.context.queue,
                &self.environment_root,
                &scene,
            )?;
            // Face submissions are ordered, so one bounded view slot can be reused.
            let view_key = 1;
            self.effects.prepare_view(
                view_key,
                &self.context.device,
                &self.context.queue,
                &capture_frame,
                scene.view_projection,
                scene.camera_position,
                &self.environment_root,
                &self.effect_asset_paths,
            )?;
            if let Some(gpu) = &mut self.gpu_particles {
                gpu.prepare_view(
                    &self.context.device,
                    &self.context.queue,
                    view_key,
                    Mat4::from_cols_array(&scene.view_projection),
                    probe.capture_position,
                )?;
            }
            for mesh in &scene.meshes {
                self.write_instance(mesh.instance_key, mesh.model, mesh.color, false, &scene);
            }
            let view = cube.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: face as u32,
                array_layer_count: Some(1),
                ..Default::default()
            });
            let mut encoder =
                self.context
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("reflection-probe-scene-face"),
                    });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("reflection-probe-PBR-face"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &multisample_view,
                        resolve_target: Some(&view),
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: f64::from(scene.clear_color[0]),
                                g: f64::from(scene.clear_color[1]),
                                b: f64::from(scene.clear_color[2]),
                                a: 1.,
                            }),
                            store: wgpu::StoreOp::Discard,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(0),
                            store: wgpu::StoreOp::Discard,
                        }),
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                if scene.environment.enabled {
                    self.sky.draw(&mut pass);
                }
                pass.set_pipeline(&self.solid_pipeline);
                pass.set_bind_group(2, self.reflections.get(None), &[]);
                pass.set_bind_group(3, &self.shadows.bind, &[]);
                for mesh in &scene.meshes {
                    let (Some(geometry), Some(instance)) = (
                        self.meshes.get(&mesh.mesh_key),
                        self.instances.get(&mesh.instance_key),
                    ) else {
                        continue;
                    };
                    pass.set_bind_group(0, &instance.bind_group, &[]);
                    pass.set_bind_group(1, self.surfaces.group(&mesh.textures), &[]);
                    pass.set_vertex_buffer(0, geometry.vertices.slice(..));
                    pass.set_index_buffer(geometry.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..geometry.index_count, 0, 0..1);
                }
                self.effects.draw_view(&mut pass, view_key);
                if let Some(gpu) = &self.gpu_particles {
                    gpu.render_view_layers(&mut pass, view_key, probe.visibility_layers);
                }
            }
            self.context.queue.submit([encoder.finish()]);
        }
        Ok(self.reflections.filter.filter(
            &self.context.device,
            &self.context.queue,
            cube,
            resolution,
            probe.quality,
        ))
    }
    /// Number of completed GPU capture submissions for a currently active probe.
    pub fn reflection_probe_capture_count(&self, entity: engine_world::EntityId) -> u64 {
        self.reflections.capture_count(entity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn probe_convolution_shader_is_valid_without_webgl_incompatible_mip_queries() {
        crate::validate_and_reflect_wgsl(FILTER_SHADER).unwrap();
        assert!(!FILTER_SHADER.contains("textureNumLevels"));
    }

    fn six_emissive_walls() -> ViewportScene {
        let primitive = engine_world::Primitive::Plane { size: [8.0; 2] }
            .mesh()
            .unwrap();
        let directions = [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z];
        let colors = [
            [4., 0., 0.],
            [0., 4., 0.],
            [0., 0., 4.],
            [4., 4., 0.],
            [4., 0., 4.],
            [0., 4., 4.],
        ];
        let meshes = directions
            .into_iter()
            .zip(colors)
            .enumerate()
            .map(|(index, (direction, emissive_color))| {
                let key = u64::try_from(index).unwrap() + 1;
                super::super::ViewportMesh {
                    textures: engine_world::SurfaceTextures {
                        emissive_color,
                        pbr: true,
                        visibility_layers: 1,
                        ..Default::default()
                    },
                    instance_key: key,
                    mesh_key: key,
                    vertices: primitive
                        .positions
                        .iter()
                        .enumerate()
                        .map(|(i, p)| super::super::ViewportVertex {
                            uv: primitive.tex_coords[i],
                            position: *p,
                            normal: primitive.normals[i],
                        })
                        .collect(),
                    indices: primitive.indices.clone(),
                    model: Mat4::from_rotation_translation(
                        glam::Quat::from_rotation_arc(Vec3::Y, -direction),
                        direction * 3.0,
                    )
                    .to_cols_array(),
                    color: [0., 0., 0., 1.],
                    selected: false,
                }
            })
            .collect();
        ViewportScene {
            environment: engine_world::SceneEnvironment::default(),
            camera_position: [0., 0., -8.],
            view_projection: (glam::camera::rh::proj::directx::perspective(1.0, 1.0, 0.1, 100.0)
                * glam::camera::rh::view::look_at_mat4(
                    Vec3::new(0., 0., -8.),
                    Vec3::ZERO,
                    Vec3::Y,
                ))
            .to_cols_array(),
            meshes,
            lights: vec![],
            guides: vec![],
            grid_vertices: vec![],
            clear_color: [0., 0., 0., 1.],
        }
    }
    fn read_capture(
        renderer: &mut SceneViewportRenderer,
        entity: engine_world::EntityId,
    ) -> ReflectionProbeReadback {
        assert!(renderer.request_reflection_probe_readback(entity).unwrap());
        let started = std::time::Instant::now();
        loop {
            if let Some(value) = renderer.poll_reflection_probe_readback(entity).unwrap() {
                return value;
            }
            assert!(
                started.elapsed() < std::time::Duration::from_secs(5),
                "reflection readback timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    #[test]
    #[ignore = "requires a physical or software graphics adapter"]
    fn real_probe_capture_preserves_six_hdr_orientations_and_independent_capture_layers() {
        let mut renderer = SceneViewportRenderer::new(super::super::BackendRequest::Auto).unwrap();
        let mut frame = engine_world::effects::EffectFrame::default();
        let mut probe = super::super::reflections::tests::probe();
        probe.visibility_layers = 1;
        let entity = probe.entity;
        frame.probes.push(probe);
        frame.camera_visibility_layers = Some(0);
        renderer.set_effect_frame(&frame).unwrap();
        let scene = six_emissive_walls();
        let visible = renderer.render(32, 32, &scene).unwrap().unwrap();
        assert!(
            visible
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[..3].iter().all(|channel| *channel == 0)),
            "main camera must hide the capture-only layer"
        );
        assert_eq!(renderer.reflection_probe_capture_count(entity), 1);
        let captured = read_capture(&mut renderer, entity);
        assert_eq!(captured.resolution, 16);
        let expected = [
            [4., 0., 0.],
            [0., 4., 0.],
            [0., 0., 4.],
            [4., 4., 0.],
            [4., 0., 4.],
            [0., 4., 4.],
        ];
        for (face, expected) in captured.faces.iter().zip(expected) {
            let center = face[8 * 16 + 8];
            for channel in 0..3 {
                assert!(
                    (center[channel] - expected[channel]).abs() < 0.05,
                    "captured HDR face center {center:?}, expected {expected:?}"
                );
            }
        }
        frame.probes[0].visibility_layers = 2;
        renderer.set_effect_frame(&frame).unwrap();
        renderer.render(32, 32, &scene).unwrap();
        let masked = read_capture(&mut renderer, entity);
        assert!(
            masked.faces.iter().all(|face| face
                .iter()
                .all(|pixel| pixel[..3].iter().all(|v| v.abs() < 0.01))),
            "capture mask must exclude every wall"
        );
    }
    #[test]
    #[ignore = "requires a physical or software graphics adapter"]
    fn overlapping_probe_shader_blends_real_captures_and_respects_priority() {
        let mut renderer = SceneViewportRenderer::new(super::super::BackendRequest::Auto).unwrap();
        let mut frame = engine_world::effects::EffectFrame {
            camera_visibility_layers: Some(0),
            ..Default::default()
        };
        let first = super::super::reflections::tests::probe();
        let second = super::super::reflections::tests::probe();
        frame.probes.push(first);
        let mut scene = six_emissive_walls();
        for wall in &mut scene.meshes {
            wall.textures.emissive_color = [4., 0., 0.];
        }
        renderer.set_effect_frame(&frame).unwrap();
        renderer.render(32, 32, &scene).unwrap();
        frame.probes.push(second);
        for wall in &mut scene.meshes {
            wall.textures.emissive_color = [0., 4., 0.];
        }
        renderer.set_effect_frame(&frame).unwrap();
        renderer.render(32, 32, &scene).unwrap();
        let mut receiver = scene.meshes[0].clone();
        receiver.instance_key = 100;
        receiver.model = Mat4::from_rotation_translation(
            glam::Quat::from_rotation_arc(Vec3::Y, -Vec3::Z),
            Vec3::ZERO,
        )
        .to_cols_array();
        receiver.color = [1.; 4];
        receiver.textures.emissive_color = [0.; 3];
        receiver.textures.metallic = Some(1.);
        receiver.textures.roughness = Some(0.02);
        scene.meshes = vec![receiver];
        frame.camera_visibility_layers = Some(1);
        let pixel = |renderer: &mut SceneViewportRenderer,
                     frame: &engine_world::effects::EffectFrame| {
            renderer.set_effect_frame(frame).unwrap();
            let result = renderer.render(32, 32, &scene).unwrap().unwrap();
            let offset = (16 * 32 + 16) * 4;
            [
                result.rgba8[offset],
                result.rgba8[offset + 1],
                result.rgba8[offset + 2],
            ]
        };
        let blended = pixel(&mut renderer, &frame);
        frame.probes[0].priority = 2;
        let red = pixel(&mut renderer, &frame);
        frame.probes[1].priority = 3;
        let green = pixel(&mut renderer, &frame);
        assert!(red[0] > red[1].saturating_add(40), "red priority: {red:?}");
        assert!(
            green[1] > green[0].saturating_add(40),
            "green priority: {green:?}"
        );
        assert!(
            blended[0] > green[0].saturating_add(40) && blended[1] > red[1].saturating_add(40),
            "overlap: {blended:?}, red {red:?}, green {green:?}"
        );
    }
    #[test]
    #[ignore = "requires a physical or software graphics adapter"]
    fn box_parallax_moves_reflection_to_the_correct_near_wall_in_real_pixels() {
        let mut renderer = SceneViewportRenderer::new(super::super::BackendRequest::Auto).unwrap();
        let mut frame = engine_world::effects::EffectFrame {
            camera_visibility_layers: Some(0),
            ..Default::default()
        };
        let mut probe = super::super::reflections::tests::probe();
        probe.dimensions = [6.; 3];
        probe.capture_position = Vec3::new(-2., 0., 0.);
        probe.parallax_correction = false;
        frame.probes.push(probe);
        let mut scene = six_emissive_walls();
        renderer.set_effect_frame(&frame).unwrap();
        renderer.render(64, 64, &scene).unwrap();
        let mut receiver = scene.meshes[0].clone();
        receiver.instance_key = 100;
        let position = Vec3::new(2.8, 0., 0.);
        let reflected = Vec3::new(0.3, 0., -1.).normalize();
        let normal = (reflected - Vec3::Z).normalize();
        receiver.model = Mat4::from_rotation_translation(
            glam::Quat::from_rotation_arc(Vec3::Y, normal),
            position,
        )
        .to_cols_array();
        receiver.color = [1.; 4];
        receiver.textures.emissive_color = [0.; 3];
        receiver.textures.metallic = Some(1.);
        receiver.textures.roughness = Some(0.02);
        scene.meshes = vec![receiver];
        let eye = position - Vec3::Z * 8.;
        scene.camera_position = eye.to_array();
        scene.view_projection = (glam::camera::rh::proj::directx::perspective(1., 1., 0.1, 100.)
            * glam::camera::rh::view::look_at_mat4(eye, position, Vec3::Y))
        .to_cols_array();
        frame.camera_visibility_layers = Some(1);
        let pixel = |renderer: &mut SceneViewportRenderer,
                     frame: &engine_world::effects::EffectFrame| {
            renderer.set_effect_frame(frame).unwrap();
            let result = renderer.render(64, 64, &scene).unwrap().unwrap();
            let offset = (32 * 64 + 32) * 4;
            [
                result.rgba8[offset],
                result.rgba8[offset + 1],
                result.rgba8[offset + 2],
            ]
        };
        let uncorrected = pixel(&mut renderer, &frame);
        frame.probes[0].parallax_correction = true;
        let corrected = pixel(&mut renderer, &frame);
        assert!(
            uncorrected[1] > uncorrected[0].saturating_add(40),
            "uncorrected ray samples distant cyan wall: {uncorrected:?}"
        );
        assert!(
            corrected[0] > corrected[1].saturating_add(40),
            "box-corrected ray samples nearby red wall: {corrected:?}"
        );
        assert_eq!(
            renderer.reflection_probe_capture_count(frame.probes[0].entity),
            1
        );
    }
    #[test]
    #[ignore = "requires a physical or software graphics adapter"]
    fn probe_refresh_interval_revision_resolution_quality_and_disposal_update_real_resources() {
        let mut renderer = SceneViewportRenderer::new(super::super::BackendRequest::Auto).unwrap();
        let mut frame = engine_world::effects::EffectFrame::default();
        frame.probes.push(super::super::reflections::tests::probe());
        let entity = frame.probes[0].entity;
        let mut scene = six_emissive_walls();
        renderer.set_effect_frame(&frame).unwrap();
        renderer.render(32, 32, &scene).unwrap();
        let original = read_capture(&mut renderer, entity);
        let initial_bytes = renderer.reflection_probe_gpu_bytes();
        scene.meshes[0].textures.emissive_color = [8., 0., 0.];
        frame.probes[0].time = 0.2;
        renderer.set_effect_frame(&frame).unwrap();
        renderer.render(32, 32, &scene).unwrap();
        assert_eq!(renderer.reflection_probe_capture_count(entity), 1);
        assert_eq!(read_capture(&mut renderer, entity).faces, original.faces);
        frame.probes[0].refresh_revision = 1;
        frame.probes[0].resolution = 32;
        frame.probes[0].quality = QualityLevel::High;
        renderer.set_effect_frame(&frame).unwrap();
        renderer.render(32, 32, &scene).unwrap();
        assert_eq!(renderer.reflection_probe_capture_count(entity), 2);
        let edited = read_capture(&mut renderer, entity);
        assert_eq!(edited.resolution, 32);
        assert!((edited.faces[0][16 * 32 + 16][0] - 8.0).abs() < 0.05);
        assert!(renderer.reflection_probe_gpu_bytes() > initial_bytes);
        frame.probes[0].refresh = engine_world::effects::ProbeRefresh::Interval;
        frame.probes[0].time = 0.6;
        renderer.set_effect_frame(&frame).unwrap();
        renderer.render(32, 32, &scene).unwrap();
        assert_eq!(renderer.reflection_probe_capture_count(entity), 2);
        frame.probes[0].time = 0.8;
        renderer.set_effect_frame(&frame).unwrap();
        renderer.render(32, 32, &scene).unwrap();
        assert_eq!(renderer.reflection_probe_capture_count(entity), 3);
        frame.probes.clear();
        renderer.set_effect_frame(&frame).unwrap();
        renderer.render(32, 32, &scene).unwrap();
        assert_eq!(renderer.reflection_probe_gpu_bytes(), 0);
        assert!(!renderer.request_reflection_probe_readback(entity).unwrap());
    }
}

/// Linear HDR pixels for each of the six faces, in +X,-X,+Y,-Y,+Z,-Z order.
/// Readback is explicit and asynchronous, including in browser render loops.
#[derive(Clone, Debug)]
pub struct ReflectionProbeReadback {
    pub resolution: u32,
    pub faces: [Vec<[f32; 4]>; 6],
}
pub(super) struct PendingProbeReadback {
    buffer: wgpu::Buffer,
    receiver: std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    resolution: u32,
    padded_row: u32,
}
impl SceneViewportRenderer {
    /// Starts one bounded GPU copy; returns false while the probe has no capture
    /// or another readback for it is already pending. Rendering never waits for it.
    /// # Errors
    /// Returns missing device resources or an invalid allocation size.
    pub fn request_reflection_probe_readback(
        &mut self,
        entity: engine_world::EntityId,
    ) -> Result<bool, RendererError> {
        if self.reflections.readbacks.contains_key(&entity) {
            return Ok(false);
        }
        let Some(probe) = self.effect_frame.probes.iter().find(|p| p.entity == entity) else {
            return Ok(false);
        };
        let Some(texture) = self.reflections.captured_texture(probe.cubemap) else {
            return Ok(false);
        };
        let resolution = texture.resolution;
        let padded_row = (resolution * 8).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("explicit-six-face-probe-readback"),
            size: u64::from(padded_row) * u64::from(resolution) * 6,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder =
            self.context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("probe-readback-copy"),
                });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture.cube,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(resolution),
                },
            },
            wgpu::Extent3d {
                width: resolution,
                height: resolution,
                depth_or_array_layers: 6,
            },
        );
        self.context.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        self.reflections.readbacks.insert(
            entity,
            PendingProbeReadback {
                buffer,
                receiver,
                resolution,
                padded_row,
            },
        );
        Ok(true)
    }
    /// Polls without blocking. Returned values are scene-linear radiance, so
    /// emissive values above 1 remain intact rather than passing through tone mapping.
    /// # Errors
    /// Returns device loss, failed mapping or a disconnected readback callback.
    pub fn poll_reflection_probe_readback(
        &mut self,
        entity: engine_world::EntityId,
    ) -> Result<Option<ReflectionProbeReadback>, RendererError> {
        self.context
            .device
            .poll(wgpu::PollType::Poll)
            .map_err(|e| RendererError::Gpu(e.to_string()))?;
        let Some(pending) = self.reflections.readbacks.get(&entity) else {
            return Ok(None);
        };
        match pending.receiver.try_recv() {
            Ok(result) => result.map_err(|e| RendererError::Gpu(e.to_string()))?,
            Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(None),
            Err(error) => return Err(RendererError::Gpu(error.to_string())),
        }
        let pending = self
            .reflections
            .readbacks
            .remove(&entity)
            .ok_or_else(|| RendererError::Gpu("probe readback disappeared".into()))?;
        let mapped = pending
            .buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|error| RendererError::Gpu(error.to_string()))?;
        let row = pending.padded_row as usize;
        let side = pending.resolution as usize;
        let face_bytes = row * side;
        let faces = std::array::from_fn(|face| {
            let mut pixels = Vec::with_capacity(side * side);
            for y in 0..side {
                let start = face * face_bytes + y * row;
                for pixel in mapped[start..start + side * 8].as_chunks::<8>().0.iter() {
                    pixels.push(std::array::from_fn(|channel| {
                        half::f16::from_le_bytes([pixel[channel * 2], pixel[channel * 2 + 1]])
                            .to_f32()
                    }));
                }
            }
            pixels
        });
        drop(mapped);
        pending.buffer.unmap();
        Ok(Some(ReflectionProbeReadback {
            resolution: pending.resolution,
            faces,
        }))
    }
    /// Current retained cubemap+filtered-map allocation estimate, excluding
    /// temporary attachments used only while a capture is submitted.
    pub fn reflection_probe_gpu_bytes(&self) -> u64 {
        self.reflections.resource_bytes()
    }
}
