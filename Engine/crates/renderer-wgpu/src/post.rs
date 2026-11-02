//! Display-space postprocessing of the resolved scene. The source and destination are
//! distinct retained textures: distortion samples the image, never its own attachment.
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

use super::{RendererError, ViewportScene, f32_bytes, validate_and_reflect_wgsl};
use engine_world::effects::{EffectFrame, VolumeShape};
use glam::{Mat4, Vec3};

const SHADER: &str = r"
struct Settings { grade: vec4<f32>, lens: vec4<f32>, warp: vec4<f32>, bloom: vec4<f32>, tint: vec4<f32>, overlay: vec4<f32>, multiply:vec4<f32>, add_dof:vec4<f32> };
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;
@group(0) @binding(2) var<uniform> settings: Settings;
struct Fog { inverse:mat4x4<f32>, dimensions_density:vec4<f32>, color_height:vec4<f32>, options:vec4<f32> };
struct Shaft { screen:vec4<f32>,color_density:vec4<f32>,options:vec4<f32> };
struct Flare { screen:vec4<f32>,color_scale:vec4<f32>,options:vec4<f32> };
struct Scene { inverse:mat4x4<f32>,camera:vec4<f32>,counts:vec4<f32>,fog:array<Fog,8>,shafts:array<Shaft,4>,flares:array<Flare,4> };
@group(0) @binding(3) var depth:texture_2d<f32>;
@group(0) @binding(4) var<uniform> scene:Scene;
fn depth_at(uv:vec2<f32>)->f32 {let size=vec2<i32>(textureDimensions(depth));let encoded=textureLoad(depth,clamp(vec2<i32>(uv*vec2<f32>(size)),vec2<i32>(0),size-vec2<i32>(1)),0).rgb;return dot(encoded,vec3<f32>(255.,65280.,16711680.))/16777215.;}
fn fog_density(fog:Fog,p:vec3<f32>)->f32 {
    let local=(fog.inverse*vec4<f32>(p,1.)).xyz / max(fog.dimensions_density.xyz*0.5,vec3<f32>(0.001));
    var distance=max(max(abs(local.x),abs(local.y)),abs(local.z));
    if fog.options.x>0.5 && fog.options.x<1.5 {distance=length(local);}
    if fog.options.x>1.5 && fog.options.x<2.5 {distance=max(length(local.xz),abs(local.y));}
    if distance>1. {return 0.;}
    let edge=clamp((1.-distance)/max(fog.options.y,0.001),0.,1.);
    let noise=0.65+0.35*sin(dot(p,vec3<f32>(1.7,2.3,1.1))*fog.options.z+settings.lens.w);
    return fog.dimensions_density.w*edge*noise*exp(-max(local.y+1.,0.)*fog.color_height.w);
}
struct Output { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) index:u32) -> Output {
    let positions = array<vec2<f32>,3>(vec2<f32>(-1.,-1.),vec2<f32>(3.,-1.),vec2<f32>(-1.,3.));
    var o:Output; o.position=vec4<f32>(positions[index],0.,1.); o.uv=positions[index]*vec2<f32>(0.5,-0.5)+vec2<f32>(0.5); return o;
}
fn image(uv:vec2<f32>) -> vec4<f32> { return textureSampleLevel(source,image_sampler,clamp(uv,vec2<f32>(0.),vec2<f32>(1.)),0.); }
fn srgb_to_linear(c:vec3<f32>) -> vec3<f32> { return select(pow((c+vec3<f32>(0.055))/1.055,vec3<f32>(2.4)),c/12.92,c<=vec3<f32>(0.04045)); }
fn linear_to_srgb(c:vec3<f32>) -> vec3<f32> { return select(1.055*pow(max(c,vec3<f32>(0.)),vec3<f32>(1./2.4))-vec3<f32>(0.055),c*12.92,c<=vec3<f32>(0.0031308)); }
@fragment fn fs_main(input:Output) -> @location(0) vec4<f32> {
    let t=settings.lens.w;
    let edge=clamp(1.-length((input.uv-vec2<f32>(0.5))*2.),0.,1.);
    let wave=vec2<f32>(sin(input.uv.y*settings.warp.y+t*settings.warp.z),cos(input.uv.x*settings.warp.y+t*settings.warp.z*0.73));
    let uv=input.uv+wave*settings.warp.x*edge;
    let base=image(uv);
    let radial=(uv-vec2<f32>(0.5))*settings.lens.y;
    var color=srgb_to_linear(vec3<f32>(image(uv+radial).r,base.g,image(uv-radial).b));
    let device_depth=depth_at(input.uv);
    if settings.add_dof.w>0. {
        let focus=depth_at(vec2<f32>(0.5));
        let blur=clamp(abs(device_depth-focus)*settings.add_dof.w*1000.,0.,8.);
        let pixel=vec2<f32>(blur)/vec2<f32>(textureDimensions(source));
        var sum=vec3<f32>(0.);
        for(var y=-2;y<=2;y+=1) {for(var x=-2;x<=2;x+=1) {sum+=srgb_to_linear(image(uv+vec2<f32>(f32(x),f32(y))*pixel).rgb);}}
        color=sum/25.;
    }
    if settings.bloom.x>0. {
        let pixel=vec2<f32>(settings.bloom.z)/vec2<f32>(textureDimensions(source));
        var glow=vec3<f32>(0.);
        for(var y=-2;y<=2;y+=1) { for(var x=-2;x<=2;x+=1) {
            let sample=srgb_to_linear(image(uv+vec2<f32>(f32(x),f32(y))*pixel).rgb);
            glow+=max(sample-vec3<f32>(settings.bloom.y),vec3<f32>(0.));
        }}
        color+=glow*(settings.bloom.x/25.);
    }
    let endpoint=scene.inverse*vec4<f32>(input.uv*vec2<f32>(2.,-2.)+vec2<f32>(-1.,1.),min(device_depth,0.9999),1.);
    let world=endpoint.xyz/endpoint.w;
    let ray=world-scene.camera.xyz;
    let ray_length=min(length(ray),2000.);
    let ray_direction=normalize(ray+vec3<f32>(0.000001));
    // Fixed integration limits are a compatibility path with no compute/storage dependencies.
    for(var v=0u;v<u32(scene.counts.x);v+=1u) {
        let fog=scene.fog[v];
        let steps=u32(clamp(fog.options.w,8.,64.));
        let origin=(fog.inverse*vec4<f32>(scene.camera.xyz,1.)).xyz;
        let direction=(fog.inverse*vec4<f32>(ray_direction,0.)).xyz;
        let safe=select(vec3<f32>(0.000001),direction,abs(direction)>vec3<f32>(0.000001));
        let near=(-fog.dimensions_density.xyz*0.5-origin)/safe;
        let far=(fog.dimensions_density.xyz*0.5-origin)/safe;
        let entry=min(near,far);let exit=max(near,far);
        let begin=max(max(max(entry.x,entry.y),entry.z),0.);
        let end=min(min(min(exit.x,exit.y),exit.z),ray_length);
        let step=max(end-begin,0.)/f32(steps);
        var optical=0.;
        for(var j=0u;j<steps;j+=1u) {optical+=fog_density(fog,scene.camera.xyz+ray_direction*(begin+(f32(j)+0.5)*step))*step;}
        let transmittance=exp(-optical);
        color=color*transmittance+fog.color_height.rgb*(1.-transmittance);
    }
    for(var v=0u;v<u32(scene.counts.y);v+=1u) {
        let shaft=scene.shafts[v];if shaft.screen.z<0. || shaft.screen.z>1. {continue;}let steps=u32(clamp(shaft.options.x,4.,64.));var glow=vec3<f32>(0.);
        for(var j=0u;j<steps;j+=1u) {let p=mix(input.uv,shaft.screen.xy,f32(j)/f32(steps));let open=select(0.,1.,depth_at(p)>0.999);glow+=srgb_to_linear(image(p).rgb)*open;}
        color+=glow/f32(steps)*shaft.color_density.rgb*shaft.screen.w*shaft.color_density.w;
    }
    for(var v=0u;v<u32(scene.counts.z);v+=1u) {
        let flare=scene.flares[v];
        if flare.screen.z>0. && flare.screen.z<=1. && depth_at(flare.screen.xy)+0.001>=flare.screen.z {
            let aspect=f32(textureDimensions(source).x)/f32(textureDimensions(source).y);
            for(var j=0u;j<=u32(flare.options.x);j+=1u) {
                let center=mix(flare.screen.xy,vec2<f32>(0.5),f32(j)*flare.options.y);
                let d=(input.uv-center)*vec2<f32>(aspect,1.);
                let radius=max(flare.color_scale.w*(1.+f32(j)*0.25),0.001);
                let disc=pow(max(1.-dot(d,d)/(radius*radius),0.),3.);
                let streak=exp(-abs(d.y)*800.)*exp(-abs(d.x)/max(flare.options.z,0.0001));
                color+=(disc+streak)*flare.color_scale.rgb*flare.screen.w/(1.+f32(j));
            }
        }
    }
    color*=exp2(settings.grade.w)*settings.tint.rgb;
    let luma=dot(color,vec3<f32>(0.2126,0.7152,0.0722));
    color=mix(vec3<f32>(luma),color,settings.grade.x);
    color=(color-vec3<f32>(0.18))*settings.grade.y+vec3<f32>(0.18+settings.grade.z);
    let radius=length((input.uv-vec2<f32>(0.5))*vec2<f32>(1.41421356));
    color*=1.-settings.lens.x*smoothstep(0.25,0.95,radius);
    let grain=fract(sin(dot(input.position.xy+vec2<f32>(t*71.),vec2<f32>(12.9898,78.233)))*43758.5453)-0.5;
    color+=vec3<f32>(grain*settings.lens.z);
    return vec4<f32>(mix(linear_to_srgb(max(color,vec3<f32>(0.)))*settings.multiply.rgb+settings.add_dof.rgb,settings.overlay.rgb,clamp(settings.overlay.a,0.,1.)),base.a);
}
";

pub(super) struct PostRenderer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    scene_uniform: wgpu::Buffer,
    target: Option<Target>,
}
struct Target {
    size: (u32, u32),
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    group: wgpu::BindGroup,
}
impl PostRenderer {
    pub fn new(device: &wgpu::Device) -> Result<Self, RendererError> {
        validate_and_reflect_wgsl(SHADER)?;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects-post-layout"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
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
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects-post-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effects-post-pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("effects-post-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
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
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effects-post-settings"),
            size: 128,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("effects-post-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let scene_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effects-atmosphere-settings"),
            size: 1376,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            scene_uniform,
            pipeline,
            layout,
            uniform,
            sampler,
            target: None,
        })
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        source: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) {
        // Rebind after viewport resize/device recovery; dimensions uniquely identify its targets.
        if self
            .target
            .as_ref()
            .is_some_and(|t| t.size == (width, height))
        {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("effects-post-output"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("effects-post-input"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
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
            ],
        });
        self.target = Some(Target {
            size: (width, height),
            texture,
            view,
            group,
        });
    }
    pub fn render<'a>(
        &'a self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        values: &[f32; 32],
        scene: &ViewportScene,
        frame: &EffectFrame,
    ) -> Option<&'a wgpu::Texture> {
        let target = self.target.as_ref()?;
        queue.write_buffer(&self.uniform, 0, &f32_bytes(values));
        queue.write_buffer(
            &self.scene_uniform,
            0,
            &f32_bytes(&scene_values(scene, frame)),
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("effects-post-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &target.group, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        Some(&target.texture)
    }
}
fn scene_values(scene: &ViewportScene, frame: &EffectFrame) -> Vec<f32> {
    let mut values = Mat4::from_cols_array(&scene.view_projection)
        .inverse()
        .to_cols_array()
        .to_vec();
    values.extend(scene.camera_position);
    values.push(0.);
    values.extend([
        frame.fog.len().min(8) as f32,
        frame.shafts.len().min(4) as f32,
        frame.flares.len().min(4) as f32,
        0.,
    ]);
    for fog in frame.fog.iter().take(8) {
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
            fog.steps.clamp(8, 64) as f32,
        ]);
    }
    values.resize(24 + 8 * 28, 0.);
    let vp = Mat4::from_cols_array(&scene.view_projection);
    let project = |target: Option<engine_world::EntityId>, position: Vec3| {
        let position = target
            .and_then(|id| scene.lights.iter().find(|l| l.entity == id))
            .map_or(position, |l| {
                Mat4::from_cols_array(&l.transform).w_axis.truncate()
            });
        let p = vp * position.extend(1.);
        let ndc = p.truncate() / p.w;
        [
            ndc.x.midpoint(1.),
            (1. - ndc.y) * 0.5,
            if p.w > 0. { ndc.z } else { -1. },
        ]
    };
    for shaft in frame.shafts.iter().take(4) {
        let p = project(shaft.light, shaft.position);
        values.extend([p[0], p[1], p[2], shaft.intensity]);
        values.extend(shaft.color);
        values.push(shaft.density);
        values.extend([
            shaft.samples.clamp(4, 64) as f32,
            shaft.max_distance,
            0.,
            0.,
        ]);
    }
    values.resize(24 + 8 * 28 + 4 * 12, 0.);
    for flare in frame.flares.iter().take(4) {
        let p = project(flare.light, flare.position);
        values.extend([p[0], p[1], p[2], flare.intensity]);
        values.extend(flare.color);
        values.push(flare.scale * 0.025);
        values.extend([
            flare.ghosts.min(16) as f32,
            flare.ghost_spacing,
            flare.streak_length * 0.01,
            0.,
        ]);
    }
    values.resize(24 + 8 * 28 + 8 * 12, 0.);
    values
}
#[cfg(test)]
mod tests {
    #[test]
    fn shader_validates() {
        super::validate_and_reflect_wgsl(super::SHADER).unwrap();
    }
}
