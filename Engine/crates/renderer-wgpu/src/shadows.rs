//! Bounded world-space shadow maps. RGB24 depth keeps sampling portable to WebGL 2.
#![allow(
    clippy::cast_precision_loss,
    reason = "eight shadow faces and 1024-pixel maps bound GPU conversions"
)]
use super::{RendererError, ViewportScene, ViewportVertex, f32_bytes};
use glam::{Mat4, Vec3};
use std::collections::{HashMap, HashSet};
use wgpu::util::DeviceExt as _;

const MAX_FACES: usize = 8;
const MAX_CASTERS: usize = 4096;
pub(super) const SHADOW_WGSL: &str = r"
struct ShadowData {
    matrices: array<mat4x4<f32>,8>,
    parameters: array<vec4<f32>,8>,
    count_size: vec4<f32>,
};
@group(3) @binding(0) var shadow_depth: texture_2d_array<f32>;
@group(3) @binding(1) var<uniform> shadow_data: ShadowData;
fn sample_shadow(light_index:u32,position:vec3<f32>,normal_light:f32)->f32 {
    var visibility=1.;
    let size=i32(shadow_data.count_size.y);
    for(var face=0u;face<u32(shadow_data.count_size.x);face+=1u){
        let settings=shadow_data.parameters[face];
        if u32(settings.x)!=light_index {continue;}
        let clip=shadow_data.matrices[face]*vec4<f32>(position,1.);
        if clip.w<=0. {continue;}
        let projected=clip.xyz/clip.w;
        let uv=vec2<f32>(projected.x*0.5+0.5,0.5-projected.y*0.5);
        if projected.z<0.||projected.z>1.||any(uv<vec2<f32>(0.))||any(uv>vec2<f32>(1.)){continue;}
        let pixel=vec2<i32>(uv*f32(size));
        let bias=settings.z*(1.+2.*(1.-clamp(normal_light,0.,1.)));
        var lit=0.;
        for(var y=-1;y<=1;y+=1){for(var x=-1;x<=1;x+=1){
            let point=clamp(pixel+vec2<i32>(x,y),vec2<i32>(0),vec2<i32>(size-1));
            let encoded=textureLoad(shadow_depth,point,i32(face),0).rgb;
            let depth=dot(encoded,vec3<f32>(255.,65280.,16711680.))/16777215.;
            lit+=select(0.,1.,projected.z-bias<=depth);
        }}
        visibility=min(visibility,lit/9.);
    }
    return visibility;
}
";

pub(super) struct ShadowMesh<'a> {
    pub instance_key: u64,
    pub vertices: &'a wgpu::Buffer,
    pub indices: &'a wgpu::Buffer,
    pub index_count: u32,
    pub model: Mat4,
    pub uniform_values: &'a [f32],
    pub material: &'a wgpu::BindGroup,
    pub casts_shadows: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShadowStats {
    pub maps: usize,
    pub caster_draws: usize,
    pub omitted_lights: usize,
}
struct Face {
    light: u32,
    matrix: Mat4,
    cube_index: u32,
}
struct Instance {
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    size: usize,
}
pub(super) struct Shadows {
    pub layout: wgpu::BindGroupLayout,
    pub bind: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    color: wgpu::Texture,
    depth: wgpu::Texture,
    colors: Vec<wgpu::TextureView>,
    depths: Vec<wgpu::TextureView>,
    instances: HashMap<(usize, u64), Instance>,
    faces: Vec<Face>,
    resolution: u32,
    stats: ShadowStats,
}
impl Shadows {
    pub fn new(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        instance_layout: &wgpu::BindGroupLayout,
        surface_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rustic-shadow-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rustic-shadow-pipeline-layout"),
            bind_group_layouts: &[Some(instance_layout), Some(surface_layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rustic-shadow-depth-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<ViewportVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..wgpu::PrimitiveState::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.,
                    clamp: 0.,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_shadow_encodedDepth"),
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
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rustic-shadow-uniform"),
            size: 656,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (color, depth, colors, depths, bind) = targets(device, &layout, &uniform, 1);
        Self {
            layout,
            bind,
            pipeline,
            uniform,
            color,
            depth,
            colors,
            depths,
            instances: HashMap::new(),
            faces: Vec::new(),
            resolution: 1,
            stats: ShadowStats::default(),
        }
    }
    pub fn stats(&self) -> ShadowStats {
        self.stats
    }
    fn prepare_faces(&mut self, scene: &ViewportScene) -> Result<(), RendererError> {
        self.faces.clear();
        self.stats = ShadowStats::default();
        let (center, radius) = scene_bounds(scene);
        let default_environment = engine_world::SceneEnvironment::default();
        let environment = if scene.environment.enabled {
            &scene.environment
        } else {
            &default_environment
        };
        if environment.sun_intensity > 0. {
            let direction = -Vec3::from_array(environment.sun_direction).normalize_or_zero();
            self.faces.push(Face {
                light: 32,
                matrix: directional_matrix(center, radius, direction),
                cube_index: 0,
            });
        }
        for (index, light) in scene.lights.iter().take(32).enumerate() {
            if !light.casts_shadows || light.intensity <= 0. {
                continue;
            }
            let transform = Mat4::from_cols_array(&light.transform);
            let position = transform.w_axis.truncate();
            let direction = transform.transform_vector3(Vec3::Z).normalize_or_zero();
            let needed = if light.kind == engine_world::LightKind::Point {
                6
            } else {
                1
            };
            if self.faces.len() + needed > MAX_FACES {
                self.stats.omitted_lights += 1;
                continue;
            }
            let index = u32::try_from(index)
                .map_err(|_| RendererError::InvalidMesh("shadow light index overflow".into()))?;
            match light.kind {
                engine_world::LightKind::Directional => self.faces.push(Face {
                    light: index,
                    matrix: directional_matrix(center, radius, direction),
                    cube_index: 0,
                }),
                engine_world::LightKind::Spot => {
                    let projection = glam::camera::rh::proj::directx::perspective(
                        (2. * light.spot_outer_angle_radians).clamp(0.05, 3.),
                        1.,
                        0.05,
                        light.range.max(0.1),
                    );
                    self.faces.push(Face {
                        light: index,
                        matrix: projection * view(position, direction),
                        cube_index: 0,
                    });
                }
                engine_world::LightKind::Point => {
                    let projection = glam::camera::rh::proj::directx::perspective(
                        std::f32::consts::FRAC_PI_2,
                        1.,
                        0.05,
                        light.range.max(0.1),
                    );
                    for (face, (direction, up)) in cube_directions().into_iter().enumerate() {
                        self.faces.push(Face {
                            light: index,
                            matrix: projection
                                * glam::camera::rh::view::look_at_mat4(
                                    position,
                                    position + direction,
                                    up,
                                ),
                            cube_index: u32::try_from(face).unwrap_or(0),
                        });
                    }
                }
            }
        }
        Ok(())
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &ViewportScene,
        meshes: &[ShadowMesh<'_>],
        instance_layout: &wgpu::BindGroupLayout,
    ) -> Result<(), RendererError> {
        self.prepare_faces(scene)?;
        if !self.faces.is_empty() && self.resolution != 1024 {
            let resolution = 1024.min(device.limits().max_texture_dimension_2d);
            let (color, depth, colors, depths, bind) =
                targets(device, &self.layout, &self.uniform, resolution);
            self.color = color;
            self.depth = depth;
            self.colors = colors;
            self.depths = depths;
            self.bind = bind;
            self.resolution = resolution;
        }
        let mut values = Vec::with_capacity(164);
        for index in 0..MAX_FACES {
            values.extend(
                self.faces
                    .get(index)
                    .map_or(Mat4::IDENTITY, |face| face.matrix)
                    .to_cols_array(),
            );
        }
        for index in 0..MAX_FACES {
            values.extend(self.faces.get(index).map_or([0.; 4], |face| {
                [face.light as f32, face.cube_index as f32, 0.0004, 0.]
            }));
        }
        values.extend([self.faces.len() as f32, self.resolution as f32, 0., 0.]);
        queue.write_buffer(&self.uniform, 0, &f32_bytes(&values));
        self.prepare_instances(device, queue, meshes, instance_layout)
    }
    fn prepare_instances(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        meshes: &[ShadowMesh<'_>],
        instance_layout: &wgpu::BindGroupLayout,
    ) -> Result<(), RendererError> {
        let mut live = HashSet::new();
        for (face_index, face) in self.faces.iter().enumerate() {
            for mesh in meshes
                .iter()
                .filter(|mesh| mesh.casts_shadows)
                .take(MAX_CASTERS)
            {
                if mesh.uniform_values.len() < 32
                    || mesh.uniform_values.iter().any(|value| !value.is_finite())
                {
                    return Err(RendererError::InvalidMesh(
                        "invalid shadow caster uniforms".into(),
                    ));
                }
                let mut values = mesh.uniform_values.to_vec();
                values[..16].copy_from_slice(&(face.matrix * mesh.model).to_cols_array());
                let key = (face_index, mesh.instance_key);
                live.insert(key);
                if self
                    .instances
                    .get(&key)
                    .is_none_or(|instance| instance.size != values.len())
                {
                    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("rustic-shadow-instance"),
                        contents: &f32_bytes(&values),
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    });
                    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("rustic-shadow-instance-bind"),
                        layout: instance_layout,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: uniform.as_entire_binding(),
                        }],
                    });
                    self.instances.insert(
                        key,
                        Instance {
                            uniform,
                            bind,
                            size: values.len(),
                        },
                    );
                } else if let Some(instance) = self.instances.get(&key) {
                    queue.write_buffer(&instance.uniform, 0, &f32_bytes(&values));
                }
            }
        }
        self.instances.retain(|key, _| live.contains(key));
        self.stats.maps = self.faces.len();
        self.stats.caster_draws = live.len();
        Ok(())
    }
    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, meshes: &[ShadowMesh<'_>]) {
        for (face, _) in self.faces.iter().enumerate() {
            let colors = [Some(wgpu::RenderPassColorAttachment {
                view: &self.colors[face],
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                    store: wgpu::StoreOp::Store,
                },
            })];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rustic-shadow-map-pass"),
                color_attachments: &colors,
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depths[face],
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            for mesh in meshes
                .iter()
                .filter(|mesh| mesh.casts_shadows)
                .take(MAX_CASTERS)
            {
                if let Some(instance) = self.instances.get(&(face, mesh.instance_key)) {
                    pass.set_bind_group(0, &instance.bind, &[]);
                    pass.set_bind_group(1, mesh.material, &[]);
                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
            }
        }
    }
}
fn targets(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    resolution: u32,
) -> (
    wgpu::Texture,
    wgpu::Texture,
    Vec<wgpu::TextureView>,
    Vec<wgpu::TextureView>,
    wgpu::BindGroup,
) {
    let size = wgpu::Extent3d {
        width: resolution,
        height: resolution,
        depth_or_array_layers: 8,
    };
    let color = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rustic-shadow-rgb24-array"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rustic-shadow-depth-array"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth24PlusStencil8,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let layers = |texture: &wgpu::Texture| {
        (0..8)
            .map(|layer| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..wgpu::TextureViewDescriptor::default()
                })
            })
            .collect()
    };
    let colors = layers(&color);
    let depths = layers(&depth);
    let view = color.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..wgpu::TextureViewDescriptor::default()
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rustic-shadow-sampling-bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: uniform.as_entire_binding(),
            },
        ],
    });
    (color, depth, colors, depths, bind)
}
fn scene_bounds(scene: &ViewportScene) -> (Vec3, f32) {
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    for mesh in &scene.meshes {
        let model = Mat4::from_cols_array(&mesh.model);
        for vertex in &mesh.vertices {
            let point = model.transform_point3(Vec3::from(vertex.position));
            if point.is_finite() {
                minimum = minimum.min(point);
                maximum = maximum.max(point);
            }
        }
    }
    if !minimum.is_finite() || !maximum.is_finite() {
        return (Vec3::ZERO, 10.);
    }
    (
        (minimum + maximum) * 0.5,
        minimum.distance(maximum).mul_add(0.5, 1.).max(1.),
    )
}
fn view(position: Vec3, direction: Vec3) -> Mat4 {
    let direction = if direction.length_squared() > 0.5 {
        direction.normalize()
    } else {
        -Vec3::Y
    };
    glam::camera::rh::view::look_at_mat4(
        position,
        position + direction,
        if direction.y.abs() > 0.95 {
            Vec3::Z
        } else {
            Vec3::Y
        },
    )
}
fn directional_matrix(center: Vec3, radius: f32, direction: Vec3) -> Mat4 {
    let direction = if direction.length_squared() > 0.5 {
        direction.normalize()
    } else {
        -Vec3::Y
    };
    glam::camera::rh::proj::directx::orthographic(
        -radius,
        radius,
        -radius,
        radius,
        0.1,
        radius * 4. + 20.,
    ) * view(center - direction * (radius * 2. + 10.), direction)
}
fn cube_directions() -> [(Vec3, Vec3); 6] {
    [
        (Vec3::X, -Vec3::Y),
        (-Vec3::X, -Vec3::Y),
        (Vec3::Y, Vec3::Z),
        (-Vec3::Y, -Vec3::Z),
        (Vec3::Z, -Vec3::Y),
        (-Vec3::Z, -Vec3::Y),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directional_fit_contains_scene_center() {
        for direction in [Vec3::Y, Vec3::Z, Vec3::new(0.3, -1., 0.2)] {
            let matrix = directional_matrix(Vec3::new(2., 3., 4.), 10., direction);
            let projected = matrix.project_point3(Vec3::new(2., 3., 4.));
            assert!(
                projected.x.abs() < 0.01
                    && projected.y.abs() < 0.01
                    && (0.0..=1.0).contains(&projected.z)
            );
        }
    }
    #[test]
    fn all_point_faces_center_their_axis() {
        let projection = glam::camera::rh::proj::directx::perspective(
            std::f32::consts::FRAC_PI_2,
            1.,
            0.05,
            20.,
        );
        for (direction, up) in cube_directions() {
            let p = (projection * glam::camera::rh::view::look_at_mat4(Vec3::ZERO, direction, up))
                .project_point3(direction * 2.);
            assert!(p.x.abs() < 0.001 && p.y.abs() < 0.001 && (0.0..=1.0).contains(&p.z));
        }
    }
}
