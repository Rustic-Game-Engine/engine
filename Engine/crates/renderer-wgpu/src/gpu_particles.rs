//! Persistent compute particles. Unsupported devices never create compute WGSL.
use super::{
    RendererError, VIEWPORT_COLOR_FORMAT, VIEWPORT_DEPTH_FORMAT, VIEWPORT_SAMPLE_COUNT,
    validate_and_reflect_wgsl,
};
use engine_world::{
    EntityId,
    effects::{
        BlendMode, CollisionResponse, GpuParticleEmitterFrame, GpuParticleState, SimulationSpace,
        VolumeShape,
    },
};
use glam::{Mat4, Vec3};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU64,
    sync::atomic::{AtomicU64, Ordering},
};

const COMPUTE: &str = include_str!("gpu_particles_compute.wgsl");
const RENDER: &str = include_str!("gpu_particles_render.wgsl");
const STATE_BYTES: u64 = 64;
const BIRTH_BYTES: u64 = 80;
const PARAMETER_BYTES: u64 = 3872;
static NEXT_CONTEXT: AtomicU64 = AtomicU64::new(1);

struct Emitter {
    states: wgpu::Buffer,
    uniforms: wgpu::Buffer,
    births: wgpu::Buffer,
    compute_group: wgpu::BindGroup,
    render_group: wgpu::BindGroup,
    capacity: u32,
    uniform_capacity: usize,
    birth_capacity: usize,
    epoch: u64,
    last_sequence: Option<u64>,
    prepared_sequence: Option<u64>,
    render_offset: u32,
    blend: BlendMode,
    visible: bool,
    visibility_layers: u32,
    render_bounds: (Vec3, f32, f32),
}
struct RenderView {
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
    last_used: u64,
    camera: Vec3,
}
pub(super) struct GpuParticles {
    compute_layout: wgpu::BindGroupLayout,
    render_layout: wgpu::BindGroupLayout,
    view_layout: wgpu::BindGroupLayout,
    views: BTreeMap<u64, RenderView>,
    view_frame: u64,
    spawn: wgpu::ComputePipeline,
    simulate: wgpu::ComputePipeline,
    render_pipelines: [wgpu::RenderPipeline; 3],
    emitters: BTreeMap<EntityId, Emitter>,
    uniform_stride: u64,
    context_id: u64,
    acknowledgements: Vec<(EntityId, u64)>,
}
impl GpuParticles {
    /// Checks actual limits before compiling or creating any compute shader.
    #[allow(
        clippy::too_many_lines,
        reason = "GPU pipeline creation keeps complete validated descriptors together"
    )]
    pub fn new(device: &wgpu::Device) -> Result<Option<Self>, RendererError> {
        if !supported_limits(&device.limits()) || cfg!(target_os = "emscripten") {
            return Ok(None);
        }
        validate_and_reflect_wgsl(COMPUTE)?;
        validate_and_reflect_wgsl(RENDER)?;
        let uniform_entry = || wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: NonZeroU64::new(PARAMETER_BYTES),
            },
            count: None,
        };
        let storage_entry = |binding, visibility, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let compute_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu-particles-compute-layout"),
            entries: &[
                uniform_entry(),
                storage_entry(1, wgpu::ShaderStages::COMPUTE, false),
                storage_entry(2, wgpu::ShaderStages::COMPUTE, true),
            ],
        });
        let mut render_uniform = uniform_entry();
        render_uniform.visibility = wgpu::ShaderStages::VERTEX;
        let render_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu-particles-render-layout"),
            entries: &[
                render_uniform,
                storage_entry(1, wgpu::ShaderStages::VERTEX, true),
            ],
        });
        let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu-particles-view-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(112),
                },
                count: None,
            }],
        });
        let compute_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpu-particle-simulation"),
            source: wgpu::ShaderSource::Wgsl(COMPUTE.into()),
        });
        let compute_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("gpu-particle-compute-pipeline-layout"),
                bind_group_layouts: &[Some(&compute_layout)],
                immediate_size: 0,
            });
        let make_compute = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&compute_pipeline_layout),
                module: &compute_module,
                entry_point: Some(entry),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            })
        };
        let spawn = make_compute("spawn");
        let simulate = make_compute("simulate");
        let render_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpu-particle-render"),
            source: wgpu::ShaderSource::Wgsl(RENDER.into()),
        });
        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("gpu-particle-render-pipeline-layout"),
                bind_group_layouts: &[Some(&render_layout), Some(&view_layout)],
                immediate_size: 0,
            });
        let render_pipelines = std::array::from_fn(|index| {
            let blend = match index {
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
                label: Some("gpu-particle-billboards"),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &render_module,
                    entry_point: Some("vertex"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..wgpu::PrimitiveState::default()
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
                    ..wgpu::MultisampleState::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &render_module,
                    entry_point: Some(if index == 2 { "multiply" } else { "fragment" }),
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
        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment.max(1));
        let uniform_stride = PARAMETER_BYTES.div_ceil(alignment) * alignment;
        Ok(Some(Self {
            compute_layout,
            render_layout,
            view_layout,
            views: BTreeMap::new(),
            view_frame: 0,
            spawn,
            simulate,
            render_pipelines,
            emitters: BTreeMap::new(),
            uniform_stride,
            context_id: NEXT_CONTEXT.fetch_add(1, Ordering::Relaxed),
            acknowledgements: Vec::new(),
        }))
    }
    pub const fn context_id(&self) -> u64 {
        self.context_id
    }
    pub fn clear(&mut self) {
        self.emitters.clear();
        self.acknowledgements.clear();
        self.views.clear();
    }
    pub fn take_acknowledgements(&mut self) -> Vec<(EntityId, u64)> {
        std::mem::take(&mut self.acknowledgements)
    }
    /// Retains a distinct camera uniform for every main/probe render pass. Queue
    /// writes to one face can never overwrite a different face's camera.
    pub fn prepare_view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: u64,
        view_projection: Mat4,
        camera: Vec3,
    ) -> Result<(), RendererError> {
        if !view_projection.is_finite()
            || view_projection.determinant().abs() < 1.0e-12
            || !camera.is_finite()
        {
            return Err(RendererError::InvalidMesh(
                "GPU particle view must be finite and invertible".into(),
            ));
        }
        if !self.views.contains_key(&key) {
            if self.views.len() >= 1024 {
                return Err(RendererError::InvalidMesh(
                    "GPU particle view budget exceeded".into(),
                ));
            }
            let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gpu-particle-view"),
                size: 112,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gpu-particle-view-binding"),
                layout: &self.view_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                }],
            });
            self.views.insert(
                key,
                RenderView {
                    uniform,
                    group,
                    last_used: self.view_frame,
                    camera,
                },
            );
        }
        let inverse = view_projection.inverse();
        let center = inverse.project_point3(Vec3::new(0.0, 0.0, 0.5));
        let right = (inverse.project_point3(Vec3::new(1.0, 0.0, 0.5)) - center).normalize_or_zero();
        let up = (inverse.project_point3(Vec3::new(0.0, 1.0, 0.5)) - center).normalize_or_zero();
        let mut bytes = Vec::with_capacity(112);
        push_matrix(&mut bytes, view_projection);
        push_vec4(&mut bytes, camera.extend(0.0).to_array());
        push_vec4(&mut bytes, right.extend(0.0).to_array());
        push_vec4(&mut bytes, up.extend(0.0).to_array());
        if let Some(view) = self.views.get_mut(&key) {
            view.last_used = self.view_frame;
            view.camera = camera;
            queue.write_buffer(&view.uniform, 0, &bytes);
        }
        Ok(())
    }
    /// Commits exactly the steps encoded by `prepare` after queue submission.
    /// Replayed acknowledged sequences produce acknowledgements without advancing.
    pub fn commit_submission(&mut self) {
        self.acknowledgements.clear();
        for (entity, emitter) in &mut self.emitters {
            if let Some(sequence) = emitter.prepared_sequence.take() {
                emitter.last_sequence = Some(sequence);
                self.acknowledgements.push((*entity, sequence));
            }
        }
    }
    /// Uploads bounded birth batches and simulates every unacknowledged world step once.
    #[allow(
        clippy::too_many_lines,
        reason = "one ordered upload/dispatch transaction preserves sequence acknowledgement semantics"
    )]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frames: &[GpuParticleEmitterFrame],
        view_projection: Mat4,
        camera: Vec3,
    ) -> Result<(), RendererError> {
        for frame in frames {
            frame.validate().map_err(RendererError::InvalidMesh)?;
        }
        self.view_frame = self.view_frame.wrapping_add(1);
        self.views
            .retain(|key, view| *key == 0 || self.view_frame.saturating_sub(view.last_used) <= 2);
        self.prepare_view(device, queue, 0, view_projection, camera)?;
        let alive: BTreeSet<_> = frames.iter().map(|f| f.entity).collect();
        self.emitters.retain(|entity, _| alive.contains(entity));
        self.acknowledgements.clear();
        let mut grouped: BTreeMap<EntityId, Vec<&GpuParticleEmitterFrame>> = BTreeMap::new();
        for frame in frames {
            frame.validate().map_err(RendererError::InvalidMesh)?;
            grouped.entry(frame.entity).or_default().push(frame);
        }
        let mut total = 0_u32;
        for (entity, mut commands) in grouped {
            commands.sort_by_key(|f| f.sequence);
            // A delayed old-epoch request must never revive particles after reset.
            if let Some(epoch) = commands.last().map(|frame| frame.epoch) {
                commands.retain(|frame| frame.epoch == epoch);
            }
            if commands.len() > 120 {
                return Err(RendererError::InvalidMesh(
                    "GPU particle queue exceeds 120 unacknowledged steps".into(),
                ));
            }
            let last = commands.last().copied().ok_or_else(|| {
                RendererError::InvalidMesh("GPU emitter command list is empty".into())
            })?;
            let capacity = commands.iter().map(|f| f.capacity).max().unwrap_or(1);
            total = total.saturating_add(capacity);
            if total > GpuParticleEmitterFrame::MAX_TOTAL_PARTICLES {
                return Err(RendererError::InvalidMesh(
                    "GPU particle storage budget exceeds 1000000 slots".into(),
                ));
            }
            let required_births = commands
                .iter()
                .map(|f| f.spawns.len())
                .sum::<usize>()
                .max(1);
            if required_births > 100_000
                || u64::try_from(required_births).unwrap_or(u64::MAX) * BIRTH_BYTES
                    > device.limits().max_storage_buffer_binding_size
            {
                return Err(RendererError::InvalidMesh(
                    "GPU particle birth queue exceeds bounded storage capacity".into(),
                ));
            }
            if u64::from(capacity) * STATE_BYTES > device.limits().max_storage_buffer_binding_size {
                return Err(RendererError::InvalidMesh(
                    "GPU particle emitter exceeds this device's storage binding size".into(),
                ));
            }
            let required_uniforms = commands.len().max(1);
            let recreate = self.emitters.get(&entity).is_none_or(|e| {
                e.capacity != capacity
                    || e.birth_capacity < required_births
                    || e.uniform_capacity < required_uniforms
            });
            if recreate {
                let old = self.emitters.remove(&entity);
                let mut next = self.create_emitter(
                    device,
                    capacity,
                    required_uniforms.next_power_of_two(),
                    required_births.next_power_of_two(),
                    last.epoch,
                );
                encoder.clear_buffer(&next.states, 0, None);
                if let Some(old) = old
                    && old.epoch == last.epoch
                {
                    encoder.copy_buffer_to_buffer(
                        &old.states,
                        0,
                        &next.states,
                        0,
                        u64::from(old.capacity.min(capacity)) * STATE_BYTES,
                    );
                    next.last_sequence = old.last_sequence;
                }
                self.emitters.insert(entity, next);
            }
            let emitter = self
                .emitters
                .get_mut(&entity)
                .ok_or_else(|| RendererError::Gpu("GPU emitter allocation failed".into()))?;
            if emitter.epoch != last.epoch {
                encoder.clear_buffer(&emitter.states, 0, None);
                emitter.epoch = last.epoch;
                emitter.last_sequence = None;
            }
            let mut births =
                Vec::with_capacity(required_births * usize::try_from(BIRTH_BYTES).unwrap_or(80));
            let mut uniforms = vec![
                0_u8;
                required_uniforms
                    * usize::try_from(self.uniform_stride).map_err(|error| {
                        RendererError::Gpu(error.to_string())
                    })?
            ];
            let mut offsets = Vec::new();
            let mut birth_offset = 0_u32;
            let mut latest_sequence = emitter.last_sequence;
            for (index, frame) in commands.iter().enumerate() {
                let fresh = latest_sequence.is_none_or(|sequence| frame.sequence > sequence);
                let count = if fresh {
                    u32::try_from(frame.spawns.len())
                        .map_err(|error| RendererError::InvalidMesh(error.to_string()))?
                } else {
                    0
                };
                if fresh {
                    for spawn in &frame.spawns {
                        push_u32x4(&mut births, [spawn.slot, 0, 0, 0]);
                        push_state(&mut births, spawn.state);
                    }
                }
                let bytes = parameter_bytes(frame, view_projection, camera, birth_offset, count);
                let offset = u64::try_from(index)
                    .map_err(|error| RendererError::Gpu(error.to_string()))?
                    * self.uniform_stride;
                let start = usize::try_from(offset)
                    .map_err(|error| RendererError::Gpu(error.to_string()))?;
                uniforms[start..start + bytes.len()].copy_from_slice(&bytes);
                let offset =
                    u32::try_from(offset).map_err(|error| RendererError::Gpu(error.to_string()))?;
                if fresh {
                    offsets.push((offset, count, frame.capacity));
                    birth_offset += count;
                    latest_sequence = Some(frame.sequence);
                }
                emitter.render_offset = offset;
                emitter.blend = frame.blend();
                emitter.visible = frame.visible;
                emitter.visibility_layers = frame.visibility_layers;
                emitter.render_bounds = frame.render_bounds;
            }
            queue.write_buffer(&emitter.uniforms, 0, &uniforms);
            if !births.is_empty() {
                queue.write_buffer(&emitter.births, 0, &births);
            }
            for (offset, count, capacity) in offsets {
                if count > 0 {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("gpu-particle-births"),
                        timestamp_writes: None,
                    });
                    pass.set_pipeline(&self.spawn);
                    pass.set_bind_group(0, &emitter.compute_group, &[offset]);
                    pass.dispatch_workgroups(count.div_ceil(64), 1, 1);
                }
                {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("gpu-particle-simulation"),
                        timestamp_writes: None,
                    });
                    pass.set_pipeline(&self.simulate);
                    pass.set_bind_group(0, &emitter.compute_group, &[offset]);
                    pass.dispatch_workgroups(capacity.div_ceil(64), 1, 1);
                }
            }
            emitter.prepared_sequence = latest_sequence;
        }
        Ok(())
    }
    pub fn render_layers<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, layers: u32) {
        self.render_view_layers(pass, 0, layers);
    }
    pub fn render_view_layers<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        view_key: u64,
        layers: u32,
    ) {
        let Some(view) = self.views.get(&view_key) else {
            return;
        };
        for emitter in self.emitters.values() {
            if !emitter.visible
                || emitter.visibility_layers & layers == 0
                || emitter.render_bounds.0.distance(view.camera) - emitter.render_bounds.1
                    > emitter.render_bounds.2
            {
                continue;
            }
            let index = match emitter.blend {
                BlendMode::Alpha => 0,
                BlendMode::Additive => 1,
                BlendMode::Multiply => 2,
            };
            pass.set_pipeline(&self.render_pipelines[index]);
            pass.set_bind_group(0, &emitter.render_group, &[emitter.render_offset]);
            pass.set_bind_group(1, &view.group, &[]);
            pass.draw(0..6, 0..emitter.capacity);
        }
    }
    fn create_emitter(
        &self,
        device: &wgpu::Device,
        capacity: u32,
        uniform_capacity: usize,
        birth_capacity: usize,
        epoch: u64,
    ) -> Emitter {
        let states = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu-particle-states"),
            size: u64::from(capacity) * STATE_BYTES,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu-particle-parameters"),
            size: self.uniform_stride * u64::try_from(uniform_capacity).unwrap_or(128),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let births = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu-particle-birth-storage"),
            size: BIRTH_BYTES * u64::try_from(birth_capacity).unwrap_or(1),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_binding = || {
            wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &uniforms,
                offset: 0,
                size: NonZeroU64::new(PARAMETER_BYTES),
            })
        };
        let compute_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gpu-particle-compute-bindings"),
            layout: &self.compute_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: states.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: births.as_entire_binding(),
                },
            ],
        });
        let render_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gpu-particle-render-bindings"),
            layout: &self.render_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: states.as_entire_binding(),
                },
            ],
        });
        Emitter {
            states,
            uniforms,
            births,
            compute_group,
            render_group,
            capacity,
            uniform_capacity,
            birth_capacity,
            epoch,
            last_sequence: None,
            prepared_sequence: None,
            render_offset: 0,
            blend: BlendMode::Alpha,
            visible: true,
            visibility_layers: u32::MAX,
            render_bounds: (Vec3::ZERO, 0.0, f32::MAX),
        }
    }
}
fn supported_limits(limits: &wgpu::Limits) -> bool {
    limits.max_compute_workgroups_per_dimension > 0
        && limits.max_compute_workgroup_size_x >= 64
        && limits.max_compute_invocations_per_workgroup >= 64
        && limits.max_storage_buffers_per_shader_stage >= 2
        && limits.max_storage_buffer_binding_size >= STATE_BYTES
        && limits.max_uniform_buffer_binding_size >= PARAMETER_BYTES
}
fn push_vec4(out: &mut Vec<u8>, v: [f32; 4]) {
    for value in v {
        out.extend_from_slice(&value.to_le_bytes());
    }
}
fn push_u32x4(out: &mut Vec<u8>, v: [u32; 4]) {
    for value in v {
        out.extend_from_slice(&value.to_le_bytes());
    }
}
fn push_matrix(out: &mut Vec<u8>, matrix: Mat4) {
    for value in matrix.to_cols_array() {
        out.extend_from_slice(&value.to_le_bytes());
    }
}
fn push_state(out: &mut Vec<u8>, state: GpuParticleState) {
    push_vec4(out, state.position_age);
    push_vec4(out, state.velocity_lifetime);
    push_vec4(out, state.size_rotation);
    push_vec4(out, state.color);
}
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "validated command capacities and finite delta bound all dispatch counts"
)]
fn parameter_bytes(
    frame: &GpuParticleEmitterFrame,
    view_projection: Mat4,
    camera: Vec3,
    birth_offset: u32,
    birth_count: u32,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(usize::try_from(PARAMETER_BYTES).unwrap_or(3872));
    push_vec4(
        &mut out,
        [
            frame.delta,
            frame.drag,
            frame.velocity_limit,
            frame.angular_velocity,
        ],
    );
    push_vec4(
        &mut out,
        [
            (frame.gravity[0] + frame.force[0]),
            (frame.gravity[1] + frame.force[1]),
            (frame.gravity[2] + frame.force[2]),
            frame.appearance.emission,
        ],
    );
    push_vec4(&mut out, camera.extend(frame.stretch).to_array());
    push_u32x4(
        &mut out,
        [
            frame.capacity,
            (frame.delta / (1.0 / 60.0)).ceil().clamp(1.0, 120.0) as u32,
            u32::from(frame.simulation_space == SimulationSpace::Local),
            frame.colliders.len() as u32,
        ],
    );
    push_u32x4(
        &mut out,
        [
            birth_offset,
            birth_count,
            frame.appearance.color.keys.len() as u32,
            frame.appearance.size.keys.len() as u32,
        ],
    );
    push_u32x4(
        &mut out,
        [frame.appearance.opacity.keys.len() as u32, 0, 0, 0],
    );
    push_matrix(&mut out, frame.transform);
    push_matrix(&mut out, frame.transform.inverse());
    push_matrix(&mut out, view_projection);
    push_appearance(&mut out, frame);
    for index in 0..16 {
        if let Some(c) = frame.colliders.get(index) {
            push_matrix(&mut out, c.inverse_transform);
            push_matrix(&mut out, c.transform);
            push_vec4(
                &mut out,
                [
                    c.dimensions[0],
                    c.dimensions[1],
                    c.dimensions[2],
                    match c.shape {
                        VolumeShape::Box => 0.,
                        VolumeShape::Sphere => 1.,
                        VolumeShape::Plane => 2.,
                        VolumeShape::Cylinder => 3.,
                    },
                ],
            );
            push_vec4(
                &mut out,
                [
                    c.bounce,
                    c.friction,
                    f32::from(u8::from(c.response == CollisionResponse::Stick)),
                    0.,
                ],
            );
        } else {
            out.resize(out.len() + 160, 0);
        }
    }
    debug_assert_eq!(out.len(), usize::try_from(PARAMETER_BYTES).unwrap_or(3872));
    out
}

fn push_appearance(out: &mut Vec<u8>, frame: &GpuParticleEmitterFrame) {
    for index in 0..16 {
        push_vec4(
            out,
            frame
                .appearance
                .color
                .keys
                .get(index)
                .map_or([1.; 4], |key| key.color),
        );
    }
    for index in 0..16 {
        push_vec4(
            out,
            [
                frame
                    .appearance
                    .color
                    .keys
                    .get(index)
                    .map_or(0., |key| key.time),
                0.,
                0.,
                0.,
            ],
        );
    }
    for curve in [&frame.appearance.size, &frame.appearance.opacity] {
        for index in 0..16 {
            push_vec4(
                out,
                curve
                    .keys
                    .get(index)
                    .map_or([0., 1., 0., 0.], |key| [key.time, key.value, 0., 0.]),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_world::effects::{
        Burst, CollisionVolume, EffectCommon, EffectComponent, EffectInput, EffectKind,
        EffectRuntime, GpuParticleSpawn, ParticleEmitter, ParticleSimulationMode, ParticleSortMode,
        PlaybackCommand, SpawnShape,
    };
    use std::{sync::mpsc, time::Duration};

    fn command() -> GpuParticleEmitterFrame {
        GpuParticleEmitterFrame {
            entity: EntityId::new(),
            epoch: 1,
            sequence: 1,
            capacity: 8,
            live_count: 1,
            visible: true,
            visibility_layers: u32::MAX,
            render_bounds: (Vec3::ZERO, 0.0, f32::MAX),
            delta: 0.1,
            transform: Mat4::IDENTITY,
            simulation_space: SimulationSpace::World,
            gravity: [0.0, -9.81, 0.0],
            force: [0.0; 3],
            drag: 0.15,
            velocity_limit: 1000.0,
            angular_velocity: 0.2,
            stretch: 0.0,
            appearance: engine_world::effects::EffectAppearance::default(),
            flipbook: [1, 1],
            flipbook_fps: 0.0,
            spawns: vec![GpuParticleSpawn {
                slot: 0,
                state: GpuParticleState::new(Vec3::Y, Vec3::new(1.0, 2.0, 0.0), 0.0, 2.0, 0.2, 0.0),
            }],
            colliders: Vec::new(),
        }
    }

    #[test]
    fn compute_and_storage_render_shaders_validate_and_uniform_abi_is_exact() {
        validate_and_reflect_wgsl(COMPUTE).unwrap();
        validate_and_reflect_wgsl(RENDER).unwrap();
        let frame = command();
        assert_eq!(
            parameter_bytes(&frame, Mat4::IDENTITY, Vec3::Z, 0, 1).len(),
            3872
        );
        assert!(!supported_limits(&wgpu::Limits::downlevel_webgl2_defaults()));
        assert!(supported_limits(&wgpu::Limits::downlevel_defaults()));
        let mut invalid = frame.clone();
        invalid.spawns[0].slot = invalid.capacity;
        assert!(invalid.validate().is_err());
        invalid = frame.clone();
        invalid.appearance.opacity.keys[0].value = f32::NAN;
        assert!(invalid.validate().is_err());
        invalid = frame.clone();
        invalid.appearance.color_parameter = Some(
            engine_world::effects::GradientParameter::Gradient(invalid.appearance.color.clone()),
        );
        assert!(invalid.validate().unwrap_err().contains("CPU"));
        invalid = frame;
        invalid.appearance.texture = Some(engine_world::AssetId::new());
        assert!(invalid.validate().unwrap_err().contains("CPU"));
    }

    fn device() -> (wgpu::Device, wgpu::Queue) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let (_, device, queue) = pollster::block_on(super::super::initialize_adapter(
            &instance,
            None,
            false,
            "particle-parity-test",
        ))
        .unwrap();
        (device, queue)
    }
    fn map(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
        let (sender, receiver) = mpsc::sync_channel(1);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sender.send(result).unwrap();
            });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(10)),
            })
            .unwrap();
        receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap();
        let bytes = buffer.slice(..).get_mapped_range().unwrap().to_vec();
        buffer.unmap();
        bytes
    }
    fn states(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        gpu: &GpuParticles,
        entity: EntityId,
    ) -> Vec<GpuParticleState> {
        let emitter = &gpu.emitters[&entity];
        let bytes = u64::from(emitter.capacity) * STATE_BYTES;
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particle-state-readback"),
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_buffer_to_buffer(&emitter.states, 0, &staging, 0, bytes);
        queue.submit([encoder.finish()]);
        map(device, &staging)
            .as_chunks::<64>()
            .0
            .iter()
            .map(|particle| {
                let values: [f32; 16] = std::array::from_fn(|i| {
                    f32::from_le_bytes(particle[i * 4..i * 4 + 4].try_into().unwrap())
                });
                GpuParticleState {
                    position_age: values[0..4].try_into().unwrap(),
                    velocity_lifetime: values[4..8].try_into().unwrap(),
                    size_rotation: values[8..12].try_into().unwrap(),
                    color: values[12..16].try_into().unwrap(),
                }
            })
            .collect()
    }
    fn submit(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        gpu: &mut GpuParticles,
        frames: &[GpuParticleEmitterFrame],
    ) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        gpu.prepare(
            device,
            queue,
            &mut encoder,
            frames,
            Mat4::IDENTITY,
            Vec3::Z * 2.0,
        )
        .unwrap();
        assert!(
            gpu.acknowledgements.is_empty(),
            "encoding is not submission acknowledgement"
        );
        queue.submit([encoder.finish()]);
        gpu.commit_submission();
    }
    fn emitter(space: SimulationSpace) -> EffectInput {
        EffectInput {
            entity: EntityId::new(),
            transform: Mat4::from_scale_rotation_translation(
                Vec3::new(2.0, 1.0, 0.5),
                glam::Quat::from_rotation_y(0.3),
                Vec3::Y,
            ),
            effect: EffectComponent {
                common: EffectCommon {
                    simulation_space: space,
                    random_seed: 7,
                    ..EffectCommon::default()
                },
                kind: EffectKind::Particle(ParticleEmitter {
                    rate: 0.0,
                    bursts: vec![Burst {
                        count: 1,
                        ..Burst::default()
                    }],
                    shape: SpawnShape::Point,
                    initial_velocity: [1.0, 2.0, 0.0],
                    speed: [0.0; 2],
                    lifetime: [2.0; 2],
                    start_size: [0.2; 2],
                    angular_velocity: 0.2,
                    gravity: [0.0, -9.81, 0.0],
                    drag: 0.15,
                    generate_events: false,
                    sorting: ParticleSortMode::None,
                    simulation_mode: ParticleSimulationMode::Gpu,
                    max_particles: 8,
                    ..ParticleEmitter::default()
                }),
                ..EffectComponent::default()
            },
        }
    }
    fn compare(
        gpu: GpuParticleState,
        cpu: &engine_world::effects::ParticleInstance,
        input: &EffectInput,
    ) {
        let mut position = Vec3::from_slice(&gpu.position_age[..3]);
        let mut velocity = Vec3::from_slice(&gpu.velocity_lifetime[..3]);
        if input.effect.common.simulation_space == SimulationSpace::Local {
            position = input.transform.transform_point3(position);
            velocity = input.transform.transform_vector3(velocity);
        }
        assert!(
            position.distance(cpu.position) < 0.0001,
            "GPU {position:?} CPU {:?}",
            cpu.position
        );
        assert!(
            velocity.distance(cpu.velocity) < 0.0001,
            "GPU {velocity:?} CPU {:?}",
            cpu.velocity
        );
        assert!((gpu.size_rotation[1] - cpu.rotation).abs() < 0.0001);
    }

    #[test]
    #[ignore = "requires a compute-capable Vulkan or GLES adapter"]
    fn gpu_persistent_storage_matches_cpu_motion_multitick_pause_restart_and_ack_retries() {
        let (device, queue) = device();
        for space in [SimulationSpace::World, SimulationSpace::Local] {
            let mut gpu = GpuParticles::new(&device)
                .unwrap()
                .expect("qualified compute adapter");
            let input = emitter(space);
            let mut cpu_input = input.clone();
            let EffectKind::Particle(p) = &mut cpu_input.effect.kind else {
                unreachable!()
            };
            p.simulation_mode = ParticleSimulationMode::Cpu;
            let mut runtime = EffectRuntime::new();
            runtime.set_gpu_particle_context(Some(gpu.context_id()));
            let mut cpu = EffectRuntime::new();
            for _ in 0..2 {
                runtime.update(0.1, std::slice::from_ref(&input), Vec3::Z);
                cpu.update(0.1, std::slice::from_ref(&cpu_input), Vec3::Z);
            }
            let mut commands = runtime.frame().gpu_emitters.clone();
            commands.push(commands.last().unwrap().clone()); // Duplicate view/request must not advance time.
            submit(&device, &queue, &mut gpu, &commands);
            compare(
                states(&device, &queue, &gpu, input.entity)[0],
                &cpu.frame().particles[0],
                &input,
            );
            let acknowledgements = gpu.take_acknowledgements();
            assert_eq!(acknowledgements.len(), 1);
            runtime.acknowledge_gpu_particles(&acknowledgements);
            let before = states(&device, &queue, &gpu, input.entity)[0];
            submit(&device, &queue, &mut gpu, &commands); // Lost main-frame response: retry original commands.
            assert_eq!(gpu.take_acknowledgements(), acknowledgements);
            assert_eq!(states(&device, &queue, &gpu, input.entity)[0], before);
            runtime.control(input.entity, PlaybackCommand::Pause);
            cpu.control(input.entity, PlaybackCommand::Pause);
            let frame = runtime
                .update(0.5, std::slice::from_ref(&input), Vec3::Z)
                .clone();
            cpu.update(0.5, std::slice::from_ref(&cpu_input), Vec3::Z);
            submit(&device, &queue, &mut gpu, &frame.gpu_emitters);
            assert_eq!(states(&device, &queue, &gpu, input.entity)[0], before);
            runtime.control(input.entity, PlaybackCommand::Restart);
            cpu.control(input.entity, PlaybackCommand::Restart);
            let frame = runtime
                .update(0.1, std::slice::from_ref(&input), Vec3::Z)
                .clone();
            cpu.update(0.1, std::slice::from_ref(&cpu_input), Vec3::Z);
            submit(&device, &queue, &mut gpu, &frame.gpu_emitters);
            compare(
                states(&device, &queue, &gpu, input.entity)[0],
                &cpu.frame().particles[0],
                &input,
            );
            submit(&device, &queue, &mut gpu, &[]);
            assert!(gpu.emitters.is_empty());
            gpu.clear();
            assert!(gpu.views.is_empty());
        }
    }

    #[test]
    #[ignore = "requires a compute-capable Vulkan or GLES adapter"]
    #[allow(
        clippy::too_many_lines,
        reason = "complete native pixel readback scenario verifies independent render views"
    )]
    fn gpu_storage_draws_hdr_pixels_with_independent_capture_views_and_layers() {
        let (device, queue) = device();
        let mut gpu = GpuParticles::new(&device).unwrap().unwrap();
        let mut frame = command();
        frame.delta = 0.0;
        frame.visibility_layers = 2;
        frame.render_bounds = (Vec3::ZERO, 0.0, 10.0);
        frame.appearance.emission = 4.0;
        frame.spawns[0].state =
            GpuParticleState::new(Vec3::new(0.0, 0.0, 0.5), Vec3::ZERO, 0.0, 2.0, 1.0, 0.0);
        submit(&device, &queue, &mut gpu, &[frame]);
        gpu.prepare_view(&device, &queue, 1, Mat4::IDENTITY, Vec3::Z * 2.0)
            .unwrap();
        gpu.prepare_view(
            &device,
            &queue,
            2,
            Mat4::from_translation(Vec3::X * 5.0),
            Vec3::Z * 2.0,
        )
        .unwrap();
        gpu.prepare_view(&device, &queue, 3, Mat4::IDENTITY, Vec3::Z * 50.0)
            .unwrap();
        let draw = |key, layers| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("gpu-storage-draw-test"),
                size: wgpu::Extent3d {
                    width: 32,
                    height: 32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: VIEWPORT_COLOR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let multisampled = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: texture.size(),
                mip_level_count: 1,
                sample_count: VIEWPORT_SAMPLE_COUNT,
                dimension: wgpu::TextureDimension::D2,
                format: VIEWPORT_COLOR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let depth = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: texture.size(),
                mip_level_count: 1,
                sample_count: VIEWPORT_SAMPLE_COUNT,
                dimension: wgpu::TextureDimension::D2,
                format: VIEWPORT_DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let color_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let msaa_view = multisampled.create_view(&wgpu::TextureViewDescriptor::default());
            let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
            let staging = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 8192,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &msaa_view,
                        resolve_target: Some(&color_view),
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                gpu.render_view_layers(&mut pass, key, layers);
            }
            encoder.copy_texture_to_buffer(
                texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &staging,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(32),
                    },
                },
                texture.size(),
            );
            queue.submit([encoder.finish()]);
            map(&device, &staging)
        };
        let visible = draw(1, 2);
        assert!(
            visible.iter().any(|byte| *byte != 0),
            "storage render must produce HDR pixels"
        );
        let center_red = u16::from_le_bytes(
            visible[16 * 256 + 16 * 8..16 * 256 + 16 * 8 + 2]
                .try_into()
                .unwrap(),
        );
        assert!(center_red > 0x3c00, "emission remains HDR above 1.0");
        assert!(
            draw(1, 1).iter().all(|byte| *byte == 0),
            "camera layer excludes GPU emitter"
        );
        assert!(
            draw(2, 2).iter().all(|byte| *byte == 0),
            "capture views keep separate uniform buffers"
        );
        assert!(
            draw(1, 2).iter().any(|byte| *byte != 0),
            "second view must not overwrite first view"
        );
        assert!(
            draw(3, 2).iter().all(|byte| *byte == 0),
            "each camera applies its own render distance"
        );
    }

    #[test]
    #[ignore = "requires a compute-capable Vulkan or GLES adapter"]
    fn gpu_collision_matches_cpu_and_uses_first_contact_volume() {
        let (device, queue) = device();
        let mut gpu = GpuParticles::new(&device)
            .unwrap()
            .expect("qualified compute adapter");
        let mut input = emitter(SimulationSpace::World);
        input.transform = Mat4::from_translation(Vec3::Y * 0.1);
        let EffectKind::Particle(p) = &mut input.effect.kind else {
            unreachable!()
        };
        p.initial_velocity = [0.0, -2.0, 0.0];
        p.drag = 0.0;
        p.collision = true;
        p.bounce = 0.6;
        let plane = EffectInput {
            entity: EntityId::new(),
            transform: Mat4::IDENTITY,
            effect: EffectComponent::new(EffectKind::CollisionVolume(CollisionVolume {
                shape: VolumeShape::Plane,
                dimensions: [10.0, 1.0, 10.0],
                bounce: 0.8,
                ..CollisionVolume::default()
            })),
        };
        let mut sticky = plane.clone();
        sticky.entity = EntityId::new();
        let EffectKind::CollisionVolume(volume) = &mut sticky.effect.kind else {
            unreachable!()
        };
        volume.response = CollisionResponse::Stick;
        let inputs = vec![input.clone(), plane, sticky];
        let mut cpu_inputs = inputs.clone();
        let EffectKind::Particle(p) = &mut cpu_inputs[0].effect.kind else {
            unreachable!()
        };
        p.simulation_mode = ParticleSimulationMode::Cpu;
        let mut runtime = EffectRuntime::new();
        runtime.set_gpu_particle_context(Some(gpu.context_id()));
        let mut cpu = EffectRuntime::new();
        let frame = runtime.update(0.1, &inputs, Vec3::Z).clone();
        cpu.update(0.1, &cpu_inputs, Vec3::Z);
        assert_eq!(frame.gpu_emitters.len(), 1);
        submit(&device, &queue, &mut gpu, &frame.gpu_emitters);
        let state = states(&device, &queue, &gpu, input.entity)[0];
        compare(state, &cpu.frame().particles[0], &input);
        assert!(
            state.size_rotation[2].abs() < f32::EPSILON,
            "second overlapping sticky volume must not override first bounce"
        );
    }
}
