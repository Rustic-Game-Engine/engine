use super::{
    RendererError, VIEWPORT_SAMPLE_COUNT, ViewportScene, f32_bytes, validate_and_reflect_wgsl,
};
use std::path::{Path, PathBuf};

const SKY_SHADER: &str = r"
struct Uniforms {
    inverse_view_projection: mat4x4<f32>,
    sky: vec4<f32>,
    options: vec4<f32>,
    haze: vec4<f32>,
};
@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var panorama: texture_2d<f32>;
struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) clip: vec2<f32>,
};
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> Output {
    let positions = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var output: Output;
    output.clip = positions[index];
    output.position = vec4<f32>(output.clip, 1.0, 1.0);
    return output;
}
fn texel(p: vec2<i32>, size: vec2<i32>) -> vec3<f32> {
    return textureLoad(panorama, vec2<i32>(((p.x % size.x) + size.x) % size.x, clamp(p.y, 0, size.y - 1)), 0).rgb;
}
@fragment fn fs_main(input: Output) -> @location(0) vec4<f32> {
    let near = uniforms.inverse_view_projection * vec4<f32>(input.clip, 0.0, 1.0);
    let far = uniforms.inverse_view_projection * vec4<f32>(input.clip, 0.99, 1.0);
    let direction = normalize(far.xyz / far.w - near.xyz / near.w);
    var color = uniforms.sky.rgb;
    if uniforms.options.x > 0.5 {
        let uv = vec2<f32>(atan2(direction.z, direction.x) / 6.2831853 + 0.5 + uniforms.options.y, acos(clamp(direction.y, -1.0, 1.0)) / 3.1415927);
        let size = vec2<i32>(textureDimensions(panorama));
        let position = uv * vec2<f32>(size) - vec2<f32>(0.5);
        let p = vec2<i32>(floor(position));
        let f = fract(position);
        color = mix(mix(texel(p, size), texel(p + vec2<i32>(1, 0), size), f.x), mix(texel(p + vec2<i32>(0, 1), size), texel(p + vec2<i32>(1, 1), size), f.x), f.y);
        color *= exp2(uniforms.options.z);
        if uniforms.options.w > 0.5 { color = color / (vec3<f32>(1.0) + color); }
    }
    let horizon = exp(-abs(direction.y) * 8.0) * (1.0 - exp(-uniforms.haze.w * 100.0));
    color = mix(color, uniforms.haze.rgb, horizon);
    return vec4<f32>(color, 1.0);
}
";

pub(super) struct SkyRenderer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    bind_group: Option<wgpu::BindGroup>,
    image_key: Option<(PathBuf, Option<std::time::SystemTime>, u64)>,
    hdr: bool,
}

impl SkyRenderer {
    pub fn new(device: &wgpu::Device) -> Result<Self, RendererError> {
        validate_and_reflect_wgsl(SKY_SHADER)?;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rustic-sky-layout"),
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
        let pipeline = sky_pipeline(device, &layout);
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rustic-sky-uniform"),
            size: 112,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            pipeline,
            layout,
            uniform,
            bind_group: None,
            image_key: None,
            hdr: false,
        })
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        root: &Path,
        scene: &ViewportScene,
    ) -> Result<(), RendererError> {
        let env = &scene.environment;
        if !env.enabled {
            return Ok(());
        }
        let image_path = if env.sky_image.is_empty() {
            PathBuf::new()
        } else {
            let project = root
                .canonicalize()
                .map_err(|e| RendererError::InvalidMesh(format!("sky project root: {e}")))?;
            let path = project.join(&env.sky_image).canonicalize().map_err(|e| {
                RendererError::InvalidMesh(format!("sky image {}: {e}", env.sky_image))
            })?;
            if !path.starts_with(&project) {
                return Err(RendererError::InvalidMesh(
                    "sky image must stay inside the project".into(),
                ));
            }
            path
        };
        let metadata = if image_path.as_os_str().is_empty() {
            None
        } else {
            Some(
                std::fs::metadata(&image_path)
                    .map_err(|e| RendererError::InvalidMesh(e.to_string()))?,
            )
        };
        let key = (
            image_path.clone(),
            metadata.as_ref().and_then(|m| m.modified().ok()),
            metadata.as_ref().map_or(0, std::fs::Metadata::len),
        );
        if self.image_key.as_ref() != Some(&key) {
            let (width, height, pixels) = if image_path.as_os_str().is_empty() {
                self.hdr = false;
                (1, 1, vec![0.0, 0.0, 0.0, 1.0])
            } else {
                let reader = image::ImageReader::open(&image_path)
                    .map_err(|e| RendererError::InvalidMesh(format!("sky image: {e}")))?
                    .with_guessed_format()
                    .map_err(|e| RendererError::InvalidMesh(e.to_string()))?;
                let hdr = reader.format() == Some(image::ImageFormat::Hdr);
                let mut reader = reader;
                let mut limits = image::Limits::default();
                let max_dimension =
                    super::MAX_RENDER_DIMENSION.min(device.limits().max_texture_dimension_2d);
                limits.max_image_width = Some(max_dimension);
                limits.max_image_height = Some(max_dimension);
                limits.max_alloc = Some(512 * 1024 * 1024);
                reader.limits(limits);
                let image = reader
                    .decode()
                    .map_err(|e| RendererError::InvalidMesh(format!("sky image: {e}")))?
                    .to_rgba32f();
                let (width, height) = image.dimensions();
                if width != height * 2 {
                    return Err(RendererError::InvalidMesh(
                        "sky image must be a 2:1 equirectangular panorama".into(),
                    ));
                }
                self.hdr = hdr;
                (width, height, image.into_raw())
            };
            self.upload(device, queue, width, height, &pixels);
            self.image_key = Some(key);
        }
        let inverse = glam::Mat4::from_cols_array(&scene.view_projection).inverse();
        if !inverse.is_finite() {
            return Err(RendererError::InvalidMesh(
                "sky camera matrix is singular".into(),
            ));
        }
        let mut values = inverse.to_cols_array().to_vec();
        values.extend(env.sky_color);
        values.push(1.0);
        values.extend([
            if env.sky_image.is_empty() { 0.0 } else { 1.0 },
            env.rotation_degrees / 360.0,
            env.exposure,
            if self.hdr { 1.0 } else { 0.0 },
        ]);
        values.extend(env.haze_color);
        values.push(env.haze_density);
        queue.write_buffer(&self.uniform, 0, &f32_bytes(&values));
        Ok(())
    }

    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        pixels: &[f32],
    ) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rustic-sky-panorama"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &f32_bytes(pixels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 16),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rustic-sky-bind-group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        }));
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(bind_group) = &self.bind_group {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

fn sky_pipeline(
    device: &wgpu::Device,
    bind_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rustic-viewport-wgsl"),
        source: wgpu::ShaderSource::Wgsl(SKY_SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rustic-viewport-pipeline-layout"),
        bind_group_layouts: &[Some(bind_layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rustic-sky-pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            // Engine primitive meshes use clockwise winding when viewed from the
            // side their normals point toward. Match that convention so the
            // exterior is front-facing and back-face culling hides interiors.
            front_face: wgpu::FrontFace::Cw,
            cull_mode: None,
            ..wgpu::PrimitiveState::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: VIEWPORT_SAMPLE_COUNT,
            ..wgpu::MultisampleState::default()
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BackendRequest, SceneViewportRenderer};

    fn scene(env: engine_world::SceneEnvironment) -> ViewportScene {
        ViewportScene {
            environment: env,
            camera_position: [0.0; 3],
            view_projection: glam::camera::rh::proj::directx::perspective(1.0, 1.0, 0.1, 100.0)
                .to_cols_array(),
            meshes: Vec::new(),
            lights: Vec::new(),
            guides: Vec::new(),
            grid_vertices: Vec::new(),
            clear_color: [0.0, 0.0, 0.0, 1.0],
        }
    }

    #[test]
    fn sky_shader_validates() {
        validate_and_reflect_wgsl(SKY_SHADER).unwrap();
    }

    #[test]
    fn environment_lighting_and_haze_change_geometry_pixels() {
        use engine_world::{
            Camera, EntitySnapshot, LocalTransform, Primitive, SceneEnvironment, SceneWorld,
            WorldCommand,
        };
        let mut world = SceneWorld::new();
        world
            .apply_commands(&[
                WorldCommand::Spawn(Box::new(EntitySnapshot {
                    camera: Some(Camera::default()),
                    local_transform: LocalTransform {
                        translation: glam::Vec3::new(0.0, 0.0, -5.0),
                        ..LocalTransform::IDENTITY
                    },
                    ..EntitySnapshot::default()
                })),
                WorldCommand::Spawn(Box::new(EntitySnapshot {
                    primitive: Some(Primitive::Cube { size: 2.0 }),
                    ..EntitySnapshot::default()
                })),
            ])
            .unwrap();
        world.propagate_transforms();
        let mut renderer = SceneViewportRenderer::new(BackendRequest::Auto).unwrap();
        let mut env = SceneEnvironment {
            enabled: true,
            ambient_intensity: 0.0,
            sun_color: [0.0, 1.0, 0.0],
            sun_direction: [0.0, 0.0, -1.0],
            ..SceneEnvironment::default()
        };
        let mut center = |env: &SceneEnvironment| {
            world
                .apply_commands(&[WorldCommand::SetEnvironment(env.clone())])
                .unwrap();
            let scene = crate::game_scene(&world, 1.0);
            assert_eq!(scene.environment, *env);
            let frame = renderer.render(32, 32, &scene).unwrap().unwrap();
            frame.rgba8[(16 * 32 + 16) * 4..(16 * 32 + 16) * 4 + 3].to_vec()
        };
        assert_eq!(center(&env), vec![0, 0, 0]);
        env.ambient_color = [0.0, 0.0, 1.0];
        env.ambient_intensity = 1.0;
        let ambient = center(&env);
        assert!(ambient[2] > 100 && ambient[0] == 0 && ambient[1] == 0);
        env.ambient_intensity = 0.0;
        env.sun_intensity = 1.0;
        let sun = center(&env);
        assert!(sun[1] > 100 && sun[0] == 0 && sun[2] == 0);
        env.haze_density = 10.0;
        env.haze_color = [1.0, 0.0, 0.0];
        let haze = center(&env);
        assert!(haze[0] > 250 && haze[1] < 5 && haze[2] < 5);
        env.haze_start = 100.0;
        assert_eq!(center(&env), sun);
    }

    #[test]
    fn sky_pixels_follow_scene_color_panorama_rotation_and_exposure() {
        let mut renderer = SceneViewportRenderer::new(BackendRequest::Auto).unwrap();
        let mut scene = scene(engine_world::SceneEnvironment {
            enabled: true,
            sky_color: [0.2, 0.4, 0.6],
            ..Default::default()
        });
        let frame = renderer.render(32, 32, &scene).unwrap().unwrap();
        assert!(
            frame
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[0].abs_diff(51) <= 1
                    && p[1].abs_diff(102) <= 1
                    && p[2].abs_diff(153) <= 1)
        );
        let root = std::env::temp_dir().join(format!("rustic-sky-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut panorama = image::RgbaImage::new(16, 8);
        for (x, _, pixel) in panorama.enumerate_pixels_mut() {
            *pixel = if x < 8 {
                image::Rgba([180, 0, 0, 255])
            } else {
                image::Rgba([0, 180, 0, 255])
            };
        }
        panorama.save(root.join("sky.png")).unwrap();
        renderer.set_environment_root(&root);
        scene.environment.sky_image = "sky.png".into();
        let base = renderer.render(32, 32, &scene).unwrap().unwrap().rgba8;
        scene.environment.rotation_degrees = 180.0;
        let rotated = renderer.render(32, 32, &scene).unwrap().unwrap().rgba8;
        assert_ne!(base, rotated);
        scene.view_projection = (glam::Mat4::from_cols_array(&scene.view_projection)
            * glam::Mat4::from_translation(glam::Vec3::new(-3.0, 2.0, -5.0)))
        .to_cols_array();
        let translated = renderer.render(32, 32, &scene).unwrap().unwrap().rgba8;
        assert!(
            rotated
                .iter()
                .zip(&translated)
                .all(|(a, b)| a.abs_diff(*b) <= 1),
            "sky must stay at infinity"
        );
        scene.environment.exposure = -2.0;
        let darker = renderer.render(32, 32, &scene).unwrap().unwrap().rgba8;
        assert!(
            darker.iter().map(|v| u64::from(*v)).sum::<u64>()
                < translated.iter().map(|v| u64::from(*v)).sum::<u64>()
        );
        scene.environment.sky_image = "missing.png".into();
        assert!(
            renderer
                .render(32, 32, &scene)
                .unwrap_err()
                .to_string()
                .contains("sky image")
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
