//! Concrete wgpu backend. No type from the implementation dependency appears in the public API.

mod sky;

use engine_rhi::{
    BindingKind, RendererBackend, ShaderBinding, ShaderReflection, ShaderStage, SurfaceLifecycle,
    SurfaceState,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use thiserror::Error;
use wgpu::util::DeviceExt as _;

/// Hard cap for offscreen fixture dimensions and frame-transport allocations.
pub const MAX_RENDER_DIMENSION: u32 = 8_192;

// Shared by viewport pipelines and their color/depth attachments.
const VIEWPORT_SAMPLE_COUNT: u32 = 4;
const SELECTION_OUTLINE_WIDTH_PIXELS: f32 = 3.0;

const MESH_SHADER: &str = r"
struct Uniforms {
    model_view_projection: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var mesh_texture: texture_2d<f32>;
@group(0) @binding(2) var mesh_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = uniforms.model_view_projection * vec4<f32>(input.position, 1.0);
    output.uv = input.uv;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(mesh_texture, mesh_sampler, input.uv);
}
";

const VIEWPORT_SHADER: &str = r"
struct Uniforms {
    model_view_projection: mat4x4<f32>,
    model: mat4x4<f32>,
    color: vec4<f32>,
    normal_matrix: mat4x4<f32>,
    lighting: vec4<f32>,
    lights: array<Light, 32>,
    ambient: vec4<f32>,
    sun: vec4<f32>,
    sun_direction: vec4<f32>,
    haze: vec4<f32>,
    camera: vec4<f32>,
};
struct Light {
    position_kind: vec4<f32>,
    direction_range: vec4<f32>,
    color_intensity: vec4<f32>,
    cone: vec4<f32>,
};
@group(0) @binding(0) var<uniform> uniforms: Uniforms;
struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) world_position: vec3<f32>,
};
@vertex fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = uniforms.model_view_projection * vec4<f32>(input.position, 1.0);
    output.normal = normalize((uniforms.normal_matrix * vec4<f32>(input.normal, 0.0)).xyz);
    output.world_position = (uniforms.model * vec4<f32>(input.position, 1.0)).xyz;
    return output;
}
fn linear_to_srgb(color: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(color, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055), color * 12.92, color <= vec3<f32>(0.0031308));
}
@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if uniforms.lighting.y > 0.5 { return uniforms.color; }
    var illumination = uniforms.ambient.rgb * uniforms.ambient.w;
    illumination += uniforms.sun.rgb * uniforms.sun.w * max(dot(normalize(input.normal), uniforms.sun_direction.xyz), 0.0);
    for (var i = 0u; i < u32(uniforms.lighting.x); i += 1u) {
        let light = uniforms.lights[i];
        var direction = -light.direction_range.xyz;
        var attenuation = 1.0;
        if light.position_kind.w > 0.5 {
            let offset = light.position_kind.xyz - input.world_position;
            let distance = length(offset);
            direction = offset / max(distance, 0.0001);
            // Inverse-square irradiance, with a smooth finite-range cutoff.
            // Clamp only the near-field singularity (0.1 world units).
            let relative_distance = distance / max(light.direction_range.w, 0.0001);
            let relative_distance_squared = relative_distance * relative_distance;
            let cutoff = max(1.0 - relative_distance_squared * relative_distance_squared, 0.0);
            attenuation = cutoff * cutoff / max(dot(offset, offset), 0.01);
            if light.position_kind.w > 1.5 {
                let cosine = dot(-direction, light.direction_range.xyz);
                attenuation *= smoothstep(light.cone.x, light.cone.y, cosine);
            }
        }
        illumination += light.color_intensity.rgb * light.color_intensity.w * attenuation * max(dot(normalize(input.normal), direction), 0.0);
    }
    let distance = max(length(input.world_position - uniforms.camera.xyz) - uniforms.camera.w, 0.0);
    let haze = 1.0 - exp(-uniforms.haze.w * distance);
    // Inspector and imported material colors are already linear. Compress lit
    // highlights, mix the linear haze color, then encode once for the UI/runtime.
    let radiance = uniforms.color.rgb * illumination;
    let mapped = radiance / (vec3<f32>(1.0) + radiance);
    return vec4<f32>(linear_to_srgb(mix(mapped, uniforms.haze.rgb, haze)), uniforms.color.a);
}
";

/// A backend request. `Auto` tries platform-preferred candidates in explicit order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendRequest {
    /// Windows: discrete Vulkan first, then DX12, Vulkan, GL; Linux: Vulkan, GL; macOS: Metal.
    Auto,
    /// Direct3D 12 only.
    Direct3D12,
    /// Vulkan only.
    Vulkan,
    /// Metal only.
    Metal,
    /// GL/GLES compatibility only.
    OpenGl,
}

impl BackendRequest {
    fn candidates(self) -> Vec<(RendererBackend, wgpu::Backends, bool)> {
        match self {
            Self::Direct3D12 => vec![(RendererBackend::Direct3D12, wgpu::Backends::DX12, false)],
            Self::Vulkan => vec![(RendererBackend::Vulkan, wgpu::Backends::VULKAN, false)],
            Self::Metal => vec![(RendererBackend::Metal, wgpu::Backends::METAL, false)],
            Self::OpenGl => vec![(RendererBackend::OpenGl, wgpu::Backends::GL, false)],
            Self::Auto if cfg!(target_os = "windows") => vec![
                (RendererBackend::Vulkan, wgpu::Backends::VULKAN, true),
                (RendererBackend::Direct3D12, wgpu::Backends::DX12, false),
                (RendererBackend::Vulkan, wgpu::Backends::VULKAN, false),
                (RendererBackend::OpenGl, wgpu::Backends::GL, false),
            ],
            Self::Auto if cfg!(target_os = "macos") => {
                vec![(RendererBackend::Metal, wgpu::Backends::METAL, false)]
            }
            Self::Auto => vec![
                (RendererBackend::Vulkan, wgpu::Backends::VULKAN, false),
                (RendererBackend::OpenGl, wgpu::Backends::GL, false),
            ],
        }
    }
}

/// Try every dedicated Vulkan adapter before returning to the normal backend order.
/// Surface compatibility and device creation must succeed before an adapter is selected.
async fn initialize_adapter(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface<'_>>,
    discrete_only: bool,
    label: &str,
) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), String> {
    let adapters = if discrete_only {
        instance
            .enumerate_adapters(wgpu::Backends::VULKAN)
            .await
            .into_iter()
            .filter(|adapter| {
                adapter.get_info().device_type == wgpu::DeviceType::DiscreteGpu
                    && surface.is_none_or(|surface| adapter.is_surface_supported(surface))
            })
            .collect::<Vec<_>>()
    } else {
        vec![
            instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter: false,
                    compatible_surface: surface,
                    apply_limit_buckets: true,
                })
                .await
                .map_err(|error| error.to_string())?,
        ]
    };
    let mut failures = Vec::new();
    for adapter in adapters {
        match adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some(label),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
        {
            Ok((device, queue)) => return Ok((adapter, device, queue)),
            Err(error) => failures.push(format!("{}: {error}", adapter.get_info().name)),
        }
    }
    if failures.is_empty() {
        Err("no compatible dedicated Vulkan GPU".to_owned())
    } else {
        Err(failures.join("; "))
    }
}

/// Backend-independent adapter details for diagnostics and qualification records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterDiagnostics {
    /// Selected Rustic backend family.
    pub backend: RendererBackend,
    /// Driver-supplied adapter name.
    pub adapter_name: String,
    /// Driver name.
    pub driver: String,
    /// Driver information/version string.
    pub driver_info: String,
    /// Device/vendor identifiers where available.
    pub vendor: u32,
    /// Device identifier where available.
    pub device: u32,
}

/// Vertex data accepted by the initial mesh renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshVertex {
    /// Object-space position.
    pub position: [f32; 3],
    /// Normalized texture coordinates.
    pub uv: [f32; 2],
}

/// CPU-side immutable mesh fixture consumed by the wgpu adapter.
#[derive(Debug, Clone, PartialEq)]
pub struct TexturedMesh {
    /// Vertex stream.
    pub vertices: Vec<MeshVertex>,
    /// Triangle-list indices.
    pub indices: Vec<u32>,
    /// RGBA8 texture bytes.
    pub texture_rgba8: Vec<u8>,
    /// Texture width.
    pub texture_width: u32,
    /// Texture height.
    pub texture_height: u32,
    /// Column-major camera * model matrix.
    pub model_view_projection: [f32; 16],
}

/// Backend-independent vertex accepted by the persistent editor viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

/// One immutable scene instance. Stable keys allow GPU resource reuse across frames.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportMesh {
    pub instance_key: u64,
    pub mesh_key: u64,
    pub vertices: Vec<ViewportVertex>,
    pub indices: Vec<u32>,
    pub model: [f32; 16],
    pub color: [f32; 4],
    pub selected: bool,
}

/// An editor-only colored line list, used for selection trajectories and volumes.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportGuide {
    pub key: u64,
    pub vertices: Vec<[f32; 3]>,
    pub color: [f32; 4],
}

/// Complete editor viewport frame input without backend implementation types.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportScene {
    pub environment: engine_world::SceneEnvironment,
    pub camera_position: [f32; 3],
    pub view_projection: [f32; 16],
    pub meshes: Vec<ViewportMesh>,
    pub lights: Vec<engine_world::RenderLight>,
    /// Editor-only guides. Each consecutive pair of vertices forms one line.
    pub guides: Vec<ViewportGuide>,
    /// Line-list positions, normally the XZ authoring grid.
    pub grid_vertices: Vec<[f32; 3]>,
    pub clear_color: [f32; 4],
}

/// Observable resource lifecycle counters used by diagnostics and tests.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct ViewportResourceStats {
    pub device_generations: u64,
    pub pipeline_generations: u64,
    pub target_generations: u64,
    pub mesh_uploads: u64,
    pub frame_count: u64,
}

impl TexturedMesh {
    /// A single colored triangle, used as the first-frame fixture.
    pub fn triangle() -> Self {
        Self {
            vertices: vec![
                MeshVertex {
                    position: [-0.75, -0.65, 0.5],
                    uv: [0.0, 1.0],
                },
                MeshVertex {
                    position: [0.75, -0.65, 0.5],
                    uv: [1.0, 1.0],
                },
                MeshVertex {
                    position: [0.0, 0.75, 0.5],
                    uv: [0.5, 0.0],
                },
            ],
            indices: vec![0, 1, 2],
            texture_rgba8: vec![236, 108, 70, 255],
            texture_width: 1,
            texture_height: 1,
            model_view_projection: identity_matrix(),
        }
    }

    /// An indexed textured quad with depth and transform, used as the M2 mesh fixture.
    pub fn checkerboard_quad() -> Self {
        Self {
            vertices: vec![
                MeshVertex {
                    position: [-0.8, -0.8, 0.5],
                    uv: [0.0, 1.0],
                },
                MeshVertex {
                    position: [0.8, -0.8, 0.5],
                    uv: [1.0, 1.0],
                },
                MeshVertex {
                    position: [0.8, 0.8, 0.5],
                    uv: [1.0, 0.0],
                },
                MeshVertex {
                    position: [-0.8, 0.8, 0.5],
                    uv: [0.0, 0.0],
                },
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            texture_rgba8: vec![
                245, 245, 245, 255, 35, 70, 150, 255, 35, 70, 150, 255, 245, 245, 245, 255,
            ],
            texture_width: 2,
            texture_height: 2,
            model_view_projection: [
                0.92, 0.12, 0.0, 0.0, -0.12, 0.92, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    fn validate(&self) -> Result<(), RendererError> {
        if self.vertices.is_empty()
            || self.indices.is_empty()
            || !self.indices.len().is_multiple_of(3)
        {
            return Err(RendererError::InvalidMesh(
                "mesh needs vertices and triangle-list indices".to_owned(),
            ));
        }
        if self
            .indices
            .iter()
            .any(|index| *index as usize >= self.vertices.len())
        {
            return Err(RendererError::InvalidMesh(
                "mesh index is outside the vertex stream".to_owned(),
            ));
        }
        if self.texture_width == 0 || self.texture_height == 0 {
            return Err(RendererError::InvalidMesh(
                "texture dimensions must be non-zero".to_owned(),
            ));
        }
        let expected = u64::from(self.texture_width)
            .saturating_mul(u64::from(self.texture_height))
            .saturating_mul(4);
        if self.texture_rgba8.len() as u64 != expected {
            return Err(RendererError::InvalidMesh(
                "texture byte count does not match RGBA8 dimensions".to_owned(),
            ));
        }
        if self
            .vertices
            .iter()
            .flat_map(|vertex| vertex.position.into_iter().chain(vertex.uv))
            .chain(self.model_view_projection)
            .any(|value| !value.is_finite())
        {
            return Err(RendererError::InvalidMesh(
                "mesh contains non-finite values".to_owned(),
            ));
        }
        Ok(())
    }
}

/// CPU-readable rendered output with qualification metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFrame {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Tightly packed RGBA8 rows.
    pub rgba8: Vec<u8>,
    /// Selected adapter/backend details.
    pub adapter: AdapterDiagnostics,
    /// CPU/GPU capture labels emitted by this render.
    pub labels: Vec<String>,
}

impl RenderedFrame {
    /// Stable SHA-256 hexadecimal digest for golden comparisons.
    pub fn digest_hex(&self) -> String {
        format!("{:x}", Sha256::digest(&self.rgba8))
    }
}

/// Recoverable backend failures.
#[derive(Debug, Error)]
pub enum RendererError {
    /// Every requested backend failed initialization.
    #[error("no requested graphics backend initialized: {attempts:?}")]
    InitializationFailed {
        /// Backend-specific attempt messages in order.
        attempts: Vec<String>,
    },
    /// WGSL failed parsing or validation.
    #[error("shader validation failed: {0}")]
    Shader(String),
    /// Mesh/source data is invalid.
    #[error("invalid mesh: {0}")]
    InvalidMesh(String),
    /// Render target dimensions exceed policy.
    #[error("invalid render extent {width}x{height}")]
    InvalidExtent {
        /// Requested width.
        width: u32,
        /// Requested height.
        height: u32,
    },
    /// A GPU mapping/submission operation failed.
    #[error("GPU operation failed: {0}")]
    Gpu(String),
    /// Native surface creation, configuration, acquisition, or presentation failed.
    #[error("surface operation failed: {0}")]
    Surface(String),
}

struct BackendContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    diagnostics: AdapterDiagnostics,
    device_loss: Arc<Mutex<Option<String>>>,
}

/// Outcome of one native surface-frame request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceFrameStatus {
    /// A frame was submitted and presented.
    Presented,
    /// A lost/outdated swapchain was reconfigured, then the frame was presented.
    RecoveredAndPresented,
    /// Presentation is intentionally suspended for a zero-sized/minimized surface.
    SkippedMinimized,
    /// The compositor did not provide a frame before its deadline; callers may retry.
    TimedOut,
}

/// Real native `wgpu` surface hidden behind an engine-owned API.
///
/// The concrete surface, adapter, device, queue, and swapchain configuration never
/// cross this crate boundary. The owned native window keeps raw handles valid for the
/// complete surface lifetime.
pub struct SurfaceRenderer {
    window: Arc<winit::window::Window>,
    request: BackendRequest,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    context: BackendContext,
    configuration: wgpu::SurfaceConfiguration,
    lifecycle: SurfaceLifecycle,
}

impl SurfaceRenderer {
    /// Creates a real presentation surface and selects the first compatible requested
    /// backend. A zero initial extent starts suspended and configures on the first resize.
    ///
    /// # Errors
    ///
    /// Returns ordered backend-attempt diagnostics or invalid extent/surface failures.
    pub fn new(
        window: Arc<winit::window::Window>,
        request: BackendRequest,
        width: u32,
        height: u32,
        scale_factor: f64,
    ) -> Result<Self, RendererError> {
        if width > MAX_RENDER_DIMENSION || height > MAX_RENDER_DIMENSION {
            return Err(RendererError::InvalidExtent { width, height });
        }
        pollster::block_on(Self::initialize(
            window,
            request,
            width,
            height,
            scale_factor,
        ))
    }

    async fn initialize(
        window: Arc<winit::window::Window>,
        request: BackendRequest,
        width: u32,
        height: u32,
        scale_factor: f64,
    ) -> Result<Self, RendererError> {
        let mut attempts = Vec::new();
        for (backend, bit, discrete_only) in request.candidates() {
            let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
            descriptor.backends = bit;
            let instance = wgpu::Instance::new(descriptor);
            let surface = match instance.create_surface(Arc::clone(&window)) {
                Ok(surface) => surface,
                Err(error) => {
                    attempts.push(format!("{backend:?}: surface creation failed: {error}"));
                    continue;
                }
            };
            let (adapter, device, queue) = match initialize_adapter(
                &instance,
                Some(&surface),
                discrete_only,
                "rustic-m2-surface-device",
            )
            .await
            {
                Ok(result) => result,
                Err(error) => {
                    attempts.push(format!(
                        "{backend:?} (discrete_only={discrete_only}): {error}"
                    ));
                    continue;
                }
            };
            let config_width = width.max(1);
            let config_height = height.max(1);
            let Some(configuration) =
                surface.get_default_config(&adapter, config_width, config_height)
            else {
                attempts.push(format!("{backend:?}: surface has no compatible format"));
                continue;
            };
            let info = adapter.get_info();
            let context = BackendContext::new(
                device,
                queue,
                AdapterDiagnostics {
                    backend,
                    adapter_name: info.name,
                    driver: info.driver,
                    driver_info: info.driver_info,
                    vendor: info.vendor,
                    device: info.device,
                },
            );
            let mut renderer = Self {
                window,
                request,
                instance,
                surface,
                context,
                configuration,
                lifecycle: SurfaceLifecycle::new(),
            };
            renderer.resize(width, height, scale_factor)?;
            return Ok(renderer);
        }
        Err(RendererError::InitializationFailed { attempts })
    }

    /// Selected adapter/backend details without exposing backend-native types.
    pub const fn diagnostics(&self) -> &AdapterDiagnostics {
        &self.context.diagnostics
    }

    /// Engine-owned surface lifecycle state.
    pub const fn lifecycle(&self) -> SurfaceLifecycle {
        self.lifecycle
    }

    /// Applies a physical resize/DPI event and reconfigures only for non-zero extents.
    ///
    /// # Errors
    ///
    /// Returns an invalid-extent error when policy limits are exceeded.
    pub fn resize(
        &mut self,
        width: u32,
        height: u32,
        scale_factor: f64,
    ) -> Result<(), RendererError> {
        if width > MAX_RENDER_DIMENSION || height > MAX_RENDER_DIMENSION {
            return Err(RendererError::InvalidExtent { width, height });
        }
        self.lifecycle.configure(width, height, scale_factor);
        if width == 0 || height == 0 {
            return Ok(());
        }
        self.configuration.width = width;
        self.configuration.height = height;
        self.surface
            .configure(&self.context.device, &self.configuration);
        Ok(())
    }

    /// Renders and presents one indexed textured/depth-tested frame.
    ///
    /// # Errors
    ///
    /// Returns mesh, GPU, or unrecoverable surface acquisition failures. Lost and
    /// outdated swapchains are reconfigured once before retrying.
    pub fn render(&mut self, mesh: &TexturedMesh) -> Result<SurfaceFrameStatus, RendererError> {
        mesh.validate()?;
        if !matches!(self.lifecycle.state(), SurfaceState::Active { .. }) {
            return Ok(SurfaceFrameStatus::SkippedMinimized);
        }
        let _ = self.context.device.poll(wgpu::PollType::Poll);
        let mut recovered = self.recover_lost_device()?;
        let mut acquisition_attempts = 0_u8;
        let (frame, suboptimal) = loop {
            acquisition_attempts = acquisition_attempts.saturating_add(1);
            match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame) => break (frame, false),
                wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                    recovered = true;
                    break (frame, true);
                }
                wgpu::CurrentSurfaceTexture::Timeout => {
                    return Ok(SurfaceFrameStatus::TimedOut);
                }
                wgpu::CurrentSurfaceTexture::Occluded => {
                    return Ok(SurfaceFrameStatus::SkippedMinimized);
                }
                wgpu::CurrentSurfaceTexture::Outdated if acquisition_attempts == 1 => {
                    self.lifecycle.mark_lost();
                    self.surface
                        .configure(&self.context.device, &self.configuration);
                    self.lifecycle.configure(
                        self.configuration.width,
                        self.configuration.height,
                        self.window.scale_factor(),
                    );
                    recovered = true;
                }
                wgpu::CurrentSurfaceTexture::Lost if acquisition_attempts == 1 => {
                    self.lifecycle.mark_lost();
                    self.surface = self
                        .instance
                        .create_surface(Arc::clone(&self.window))
                        .map_err(|error| RendererError::Surface(error.to_string()))?;
                    self.surface
                        .configure(&self.context.device, &self.configuration);
                    self.lifecycle.configure(
                        self.configuration.width,
                        self.configuration.height,
                        self.window.scale_factor(),
                    );
                    recovered = true;
                }
                wgpu::CurrentSurfaceTexture::Outdated => {
                    return Err(RendererError::Surface(
                        "surface remained outdated after reconfiguration".to_owned(),
                    ));
                }
                wgpu::CurrentSurfaceTexture::Lost => {
                    return Err(RendererError::Surface(
                        "surface remained lost after recreation".to_owned(),
                    ));
                }
                wgpu::CurrentSurfaceTexture::Validation => {
                    return Err(RendererError::Surface(
                        "surface acquisition raised a validation error".to_owned(),
                    ));
                }
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let command = encode_surface_mesh(
            &self.context,
            self.configuration.format,
            self.configuration.width,
            self.configuration.height,
            &view,
            mesh,
        )?;
        self.context.queue.submit([command]);
        self.context.queue.present(frame);
        // Device-loss callbacks are dispatched asynchronously. Poll without blocking so a
        // loss reported by this submission is rebuilt before the next event-loop frame.
        let _ = self.context.device.poll(wgpu::PollType::Poll);
        if suboptimal {
            self.surface
                .configure(&self.context.device, &self.configuration);
        }
        Ok(if recovered {
            SurfaceFrameStatus::RecoveredAndPresented
        } else {
            SurfaceFrameStatus::Presented
        })
    }

    fn recover_lost_device(&mut self) -> Result<bool, RendererError> {
        let Some(reason) = self.context.take_device_loss() else {
            return Ok(false);
        };
        self.lifecycle.mark_lost();
        let replacement = pollster::block_on(Self::initialize(
            Arc::clone(&self.window),
            self.request,
            self.configuration.width,
            self.configuration.height,
            self.window.scale_factor(),
        ))
        .map_err(|error| {
            RendererError::Gpu(format!(
                "device loss ({reason}) could not be recovered: {error}"
            ))
        })?;
        *self = replacement;
        Ok(true)
    }
}

impl BackendContext {
    fn new(device: wgpu::Device, queue: wgpu::Queue, diagnostics: AdapterDiagnostics) -> Self {
        let device_loss = Arc::new(Mutex::new(None));
        let callback_state = Arc::clone(&device_loss);
        device.set_device_lost_callback(move |reason, message| {
            if let Ok(mut loss) = callback_state.lock() {
                *loss = Some(format!("{reason:?}: {message}"));
            }
        });
        Self {
            device,
            queue,
            diagnostics,
            device_loss,
        }
    }

    fn take_device_loss(&self) -> Option<String> {
        self.device_loss
            .lock()
            .ok()
            .and_then(|mut loss| loss.take())
    }

    async fn initialize(request: BackendRequest) -> Result<Self, RendererError> {
        let mut attempts = Vec::new();
        for (backend, bit, discrete_only) in request.candidates() {
            let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
            descriptor.backends = bit;
            let instance = wgpu::Instance::new(descriptor);
            let (adapter, device, queue) = match initialize_adapter(
                &instance,
                None,
                discrete_only,
                "rustic-m2-device",
            )
            .await
            {
                Ok(result) => result,
                Err(error) => {
                    attempts.push(format!(
                        "{backend:?} (discrete_only={discrete_only}): {error}"
                    ));
                    continue;
                }
            };
            let info = adapter.get_info();
            return Ok(Self::new(
                device,
                queue,
                AdapterDiagnostics {
                    backend,
                    adapter_name: info.name,
                    driver: info.driver,
                    driver_info: info.driver_info,
                    vendor: info.vendor,
                    device: info.device,
                },
            ));
        }
        Err(RendererError::InitializationFailed { attempts })
    }
}

/// Validates WGSL and converts reflection into engine-owned records.
///
/// # Errors
///
/// Returns [`RendererError::Shader`] with parser or validator context for malformed WGSL.
pub fn validate_and_reflect_wgsl(source: &str) -> Result<ShaderReflection, RendererError> {
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|error| RendererError::Shader(error.emit_to_string(source)))?;
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .map_err(|error| RendererError::Shader(error.to_string()))?;

    let mut reflection = ShaderReflection::default();
    for entry in &module.entry_points {
        let stage = match entry.stage {
            naga::ShaderStage::Vertex => ShaderStage::Vertex,
            naga::ShaderStage::Fragment => ShaderStage::Fragment,
            naga::ShaderStage::Compute => ShaderStage::Compute,
            _ => continue,
        };
        reflection.entry_points.push((entry.name.clone(), stage));
        if entry.stage == naga::ShaderStage::Vertex {
            for argument in &entry.function.arguments {
                match argument.binding {
                    Some(naga::Binding::Location { location, .. }) => {
                        reflection.vertex_locations.push(location);
                    }
                    None => {
                        if let naga::TypeInner::Struct { ref members, .. } =
                            module.types[argument.ty].inner
                        {
                            for member in members {
                                if let Some(naga::Binding::Location { location, .. }) =
                                    member.binding
                                {
                                    reflection.vertex_locations.push(location);
                                }
                            }
                        }
                    }
                    Some(_) => {}
                }
            }
        }
    }
    for (_, variable) in module.global_variables.iter() {
        let Some(binding) = variable.binding.as_ref() else {
            continue;
        };
        let kind = match variable.space {
            naga::AddressSpace::Uniform => BindingKind::UniformBuffer,
            naga::AddressSpace::Storage { access }
                if access.contains(naga::StorageAccess::STORE) =>
            {
                BindingKind::StorageBuffer
            }
            naga::AddressSpace::Storage { .. } => BindingKind::ReadOnlyStorageBuffer,
            naga::AddressSpace::Handle => match module.types[variable.ty].inner {
                naga::TypeInner::Sampler { .. } => BindingKind::Sampler,
                naga::TypeInner::Image {
                    class: naga::ImageClass::Storage { .. },
                    ..
                } => BindingKind::StorageTexture,
                _ => BindingKind::SampledTexture,
            },
            _ => continue,
        };
        reflection.bindings.push(ShaderBinding {
            group: binding.group,
            binding: binding.binding,
            kind,
            name: variable.name.clone(),
        });
    }
    reflection
        .entry_points
        .sort_by(|left, right| left.0.cmp(&right.0));
    reflection
        .bindings
        .sort_by_key(|binding| (binding.group, binding.binding));
    reflection.vertex_locations.sort_unstable();
    reflection.vertex_locations.dedup();
    Ok(reflection)
}

/// Renders a triangle or indexed textured mesh into a CPU-readable RGBA frame.
///
/// # Errors
///
/// Returns a structured initialization, validation, or GPU error. Callers can retry with
/// another explicit backend without changing project settings.
pub fn render_offscreen(
    request: BackendRequest,
    width: u32,
    height: u32,
    mesh: &TexturedMesh,
) -> Result<RenderedFrame, RendererError> {
    if width == 0 || height == 0 || width > MAX_RENDER_DIMENSION || height > MAX_RENDER_DIMENSION {
        return Err(RendererError::InvalidExtent { width, height });
    }
    mesh.validate()?;
    let _reflection = validate_and_reflect_wgsl(MESH_SHADER)?;
    let context = pollster::block_on(BackendContext::initialize(request))?;
    render_with_context(&context, width, height, mesh)
}

struct CachedViewportMesh {
    digest: [u8; 32],
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

struct CachedViewportInstance {
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

struct ViewportTargets {
    width: u32,
    height: u32,
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    multisample_color_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    readback: wgpu::Buffer,
    padded_bytes_per_row: u32,
}

/// Persistent engine-owned editor viewport renderer.
///
/// Device, queue, pipelines, bind groups, mesh buffers, and size-dependent targets are retained;
/// no backend type crosses this crate's public API.
pub struct SceneViewportRenderer {
    sky: sky::SkyRenderer,
    environment_root: std::path::PathBuf,
    request: BackendRequest,
    context: BackendContext,
    bind_layout: wgpu::BindGroupLayout,
    solid_pipeline: wgpu::RenderPipeline,
    outline_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    meshes: HashMap<u64, CachedViewportMesh>,
    instances: HashMap<u64, CachedViewportInstance>,
    grid: Option<CachedViewportMesh>,
    targets: Option<ViewportTargets>,
    stats: ViewportResourceStats,
}

impl SceneViewportRenderer {
    /// Initializes the selected graphics backend and all size-independent resources once.
    ///
    /// # Errors
    ///
    /// Returns shader-validation or ordered backend/device initialization diagnostics.
    pub fn new(request: BackendRequest) -> Result<Self, RendererError> {
        validate_and_reflect_wgsl(VIEWPORT_SHADER)?;
        let context = pollster::block_on(BackendContext::initialize(request))?;
        let bind_layout = viewport_bind_layout(&context.device);
        let solid_pipeline = viewport_pipeline(
            &context.device,
            &bind_layout,
            wgpu::PrimitiveTopology::TriangleList,
            Some(wgpu::Face::Back),
            "rustic-viewport-solid-pipeline",
        );
        let outline_pipeline = viewport_pipeline(
            &context.device,
            &bind_layout,
            wgpu::PrimitiveTopology::TriangleList,
            Some(wgpu::Face::Front),
            "rustic-viewport-outline-pipeline",
        );
        let line_pipeline = viewport_pipeline(
            &context.device,
            &bind_layout,
            wgpu::PrimitiveTopology::LineList,
            None,
            "rustic-viewport-grid-pipeline",
        );
        let sky = sky::SkyRenderer::new(&context.device)?;
        Ok(Self {
            sky,
            environment_root: std::path::PathBuf::new(),
            request,
            context,
            bind_layout,
            solid_pipeline,
            outline_pipeline,
            line_pipeline,
            meshes: HashMap::new(),
            instances: HashMap::new(),
            grid: None,
            targets: None,
            stats: ViewportResourceStats {
                device_generations: 1,
                pipeline_generations: 1,
                ..ViewportResourceStats::default()
            },
        })
    }

    /// Resolves scene sky image paths against a project or immutable play snapshot.
    pub fn set_environment_root(&mut self, root: impl Into<std::path::PathBuf>) {
        self.environment_root = root.into();
    }

    pub const fn stats(&self) -> ViewportResourceStats {
        self.stats
    }

    pub fn diagnostics(&self) -> &AdapterDiagnostics {
        &self.context.diagnostics
    }

    /// Renders the scene, returning `None` for a minimized/zero-sized viewport.
    ///
    /// # Errors
    ///
    /// Returns invalid scene/extent, GPU submission/readback, or recovery initialization errors.
    #[allow(
        clippy::too_many_lines,
        reason = "one render transaction keeps resource preparation, encoding, and readback auditable"
    )]
    pub fn render(
        &mut self,
        width: u32,
        height: u32,
        scene: &ViewportScene,
    ) -> Result<Option<RenderedFrame>, RendererError> {
        if width == 0 || height == 0 {
            return Ok(None);
        }
        if width > MAX_RENDER_DIMENSION || height > MAX_RENDER_DIMENSION {
            return Err(RendererError::InvalidExtent { width, height });
        }
        if scene
            .view_projection
            .into_iter()
            .chain(scene.clear_color)
            .any(|value| !value.is_finite())
        {
            return Err(RendererError::InvalidMesh(
                "viewport scene contains non-finite values".to_owned(),
            ));
        }
        if self.context.take_device_loss().is_some() {
            let previous = self.stats;
            let root = self.environment_root.clone();
            *self = Self::new(self.request)?;
            self.environment_root = root;
            self.stats.device_generations = previous.device_generations.saturating_add(1);
            self.stats.pipeline_generations = previous.pipeline_generations.saturating_add(1);
        }
        if !scene.environment.is_valid() || scene.camera_position.iter().any(|v| !v.is_finite()) {
            return Err(RendererError::InvalidMesh(
                "invalid scene environment or camera position".to_owned(),
            ));
        }
        self.sky.prepare(
            &self.context.device,
            &self.context.queue,
            &self.environment_root,
            scene,
        )?;
        self.ensure_targets(width, height);
        self.ensure_grid(&scene.grid_vertices)?;
        for guide in &scene.guides {
            self.ensure_guide(guide)?;
            self.ensure_instance(guide.key);
            self.write_instance(guide.key, identity_matrix(), guide.color, true, scene);
        }
        for mesh in &scene.meshes {
            validate_viewport_mesh(mesh)?;
            self.ensure_mesh(mesh)?;
            self.ensure_instance(mesh.instance_key);
            self.write_instance(mesh.instance_key, mesh.model, mesh.color, false, scene);
            if mesh.selected {
                let outline_key = mesh.instance_key ^ (1_u64 << 63);
                self.ensure_instance(outline_key);
                self.write_instance(
                    outline_key,
                    selection_outline_model(mesh, scene.view_projection, width, height),
                    [1.0, 0.48, 0.04, 1.0],
                    true,
                    scene,
                );
            }
        }
        self.write_instance(
            u64::MAX,
            identity_matrix(),
            [0.28, 0.31, 0.35, 1.0],
            true,
            scene,
        );

        let targets = self.targets.as_ref().ok_or_else(|| {
            RendererError::Gpu("non-zero viewport target was not created".to_owned())
        })?;
        let mut encoder =
            self.context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("rustic-viewport-command-encoder"),
                });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rustic-viewport-main-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.multisample_color_view,
                    resolve_target: Some(&targets.color_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(scene.clear_color[0]),
                            g: f64::from(scene.clear_color[1]),
                            b: f64::from(scene.clear_color[2]),
                            a: f64::from(scene.clear_color[3]),
                        }),
                        store: wgpu::StoreOp::Discard,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });

            if scene.environment.enabled {
                self.sky.draw(&mut pass);
            }
            if let Some(grid) = &self.grid {
                let key = u64::MAX;
                if let Some(instance) = self.instances.get(&key) {
                    pass.set_pipeline(&self.line_pipeline);
                    pass.set_bind_group(0, &instance.bind_group, &[]);
                    pass.set_vertex_buffer(0, grid.vertices.slice(..));
                    pass.set_index_buffer(grid.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..grid.index_count, 0, 0..1);
                }
            }

            for selected_pass in [true, false] {
                pass.set_pipeline(if selected_pass {
                    &self.outline_pipeline
                } else {
                    &self.solid_pipeline
                });
                for mesh in &scene.meshes {
                    if selected_pass && !mesh.selected {
                        continue;
                    }
                    let instance_key = if selected_pass {
                        mesh.instance_key ^ (1_u64 << 63)
                    } else {
                        mesh.instance_key
                    };
                    let (Some(gpu_mesh), Some(instance)) = (
                        self.meshes.get(&mesh.mesh_key),
                        self.instances.get(&instance_key),
                    ) else {
                        continue;
                    };
                    pass.set_bind_group(0, &instance.bind_group, &[]);
                    pass.set_vertex_buffer(0, gpu_mesh.vertices.slice(..));
                    pass.set_index_buffer(gpu_mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..gpu_mesh.index_count, 0, 0..1);
                }
            }

            pass.set_pipeline(&self.line_pipeline);
            for guide in &scene.guides {
                let (Some(gpu_mesh), Some(instance)) =
                    (self.meshes.get(&guide.key), self.instances.get(&guide.key))
                else {
                    continue;
                };
                pass.set_bind_group(0, &instance.bind_group, &[]);
                pass.set_vertex_buffer(0, gpu_mesh.vertices.slice(..));
                pass.set_index_buffer(gpu_mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..gpu_mesh.index_count, 0, 0..1);
            }
        }
        encoder.copy_texture_to_buffer(
            targets.color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &targets.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(targets.padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            targets.color.size(),
        );
        self.context.queue.submit([encoder.finish()]);
        let rgba8 = read_viewport_target(&self.context.device, targets)?;
        self.stats.frame_count = self.stats.frame_count.saturating_add(1);
        Ok(Some(RenderedFrame {
            width,
            height,
            rgba8,
            adapter: self.context.diagnostics.clone(),
            labels: vec![
                "rustic-viewport-main-pass".to_owned(),
                "rustic-viewport-solid-pipeline".to_owned(),
                "rustic-viewport-grid-pipeline".to_owned(),
            ],
        }))
    }

    fn ensure_targets(&mut self, width: u32, height: u32) {
        if self
            .targets
            .as_ref()
            .is_some_and(|target| target.width == width && target.height == height)
        {
            return;
        }
        let device = &self.context.device;
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rustic-viewport-color"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        // Resolve into the single-sample color texture used by CPU readback.
        let multisample_color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rustic-viewport-msaa-color"),
            size,
            mip_level_count: 1,
            sample_count: VIEWPORT_SAMPLE_COUNT,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rustic-viewport-depth"),
            size,
            mip_level_count: 1,
            sample_count: VIEWPORT_SAMPLE_COUNT,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let padded_bytes_per_row = align_to(width * 4, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rustic-viewport-readback"),
            size: u64::from(padded_bytes_per_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        self.targets = Some(ViewportTargets {
            width,
            height,
            color_view: color.create_view(&wgpu::TextureViewDescriptor::default()),
            multisample_color_view: multisample_color
                .create_view(&wgpu::TextureViewDescriptor::default()),
            depth_view: depth.create_view(&wgpu::TextureViewDescriptor::default()),
            color,
            readback,
            padded_bytes_per_row,
        });
        self.stats.target_generations = self.stats.target_generations.saturating_add(1);
    }

    fn ensure_mesh(&mut self, mesh: &ViewportMesh) -> Result<(), RendererError> {
        let digest = viewport_mesh_digest(&mesh.vertices, &mesh.indices);
        if self
            .meshes
            .get(&mesh.mesh_key)
            .is_some_and(|cached| cached.digest == digest)
        {
            return Ok(());
        }
        let index_count = u32::try_from(mesh.indices.len())
            .map_err(|_| RendererError::InvalidMesh("index count exceeds u32".to_owned()))?;
        let vertices = self
            .context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rustic-viewport-mesh-vertices"),
                contents: &viewport_vertex_bytes(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let indices = self
            .context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rustic-viewport-mesh-indices"),
                contents: &u32_bytes(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        self.meshes.insert(
            mesh.mesh_key,
            CachedViewportMesh {
                digest,
                vertices,
                indices,
                index_count,
            },
        );
        self.stats.mesh_uploads = self.stats.mesh_uploads.saturating_add(1);
        Ok(())
    }

    fn ensure_grid(&mut self, positions: &[[f32; 3]]) -> Result<(), RendererError> {
        if positions.is_empty() {
            self.grid = None;
            self.ensure_instance(u64::MAX);
            return Ok(());
        }
        let vertices: Vec<_> = positions
            .iter()
            .map(|position| ViewportVertex {
                position: *position,
                normal: [0.0, 1.0, 0.0],
            })
            .collect();
        let indices: Vec<_> = (0..u32::try_from(vertices.len())
            .map_err(|_| RendererError::InvalidMesh("grid vertex count exceeds u32".to_owned()))?)
            .collect();
        let mesh = ViewportMesh {
            instance_key: u64::MAX,
            mesh_key: u64::MAX,
            vertices,
            indices,
            model: identity_matrix(),
            color: [0.28, 0.31, 0.35, 1.0],
            selected: false,
        };
        let digest = viewport_mesh_digest(&mesh.vertices, &mesh.indices);
        if self.grid.as_ref().is_none_or(|grid| grid.digest != digest) {
            let vertices =
                self.context
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("rustic-viewport-grid-vertices"),
                        contents: &viewport_vertex_bytes(&mesh.vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
            let indices =
                self.context
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("rustic-viewport-grid-indices"),
                        contents: &u32_bytes(&mesh.indices),
                        usage: wgpu::BufferUsages::INDEX,
                    });
            self.grid = Some(CachedViewportMesh {
                digest,
                vertices,
                indices,
                index_count: u32::try_from(mesh.indices.len()).unwrap_or(u32::MAX),
            });
            self.stats.mesh_uploads = self.stats.mesh_uploads.saturating_add(1);
        }
        self.ensure_instance(u64::MAX);
        Ok(())
    }

    fn ensure_guide(&mut self, guide: &ViewportGuide) -> Result<(), RendererError> {
        if !guide.vertices.len().is_multiple_of(2) {
            return Err(RendererError::InvalidMesh(
                "viewport guide must contain pairs of line vertices".to_owned(),
            ));
        }
        if guide
            .vertices
            .iter()
            .flatten()
            .chain(guide.color.iter())
            .any(|value| !value.is_finite())
        {
            return Err(RendererError::InvalidMesh(
                "viewport guide contains non-finite values".to_owned(),
            ));
        }
        let vertices = guide
            .vertices
            .iter()
            .map(|position| ViewportVertex {
                position: *position,
                normal: [0.0, 1.0, 0.0],
            })
            .collect::<Vec<_>>();
        let indices = (0..u32::try_from(vertices.len()).map_err(|_| {
            RendererError::InvalidMesh("guide vertex count exceeds u32".to_owned())
        })?)
            .collect::<Vec<_>>();
        let mesh = ViewportMesh {
            instance_key: guide.key,
            mesh_key: guide.key,
            vertices,
            indices,
            model: identity_matrix(),
            color: guide.color,
            selected: false,
        };
        self.ensure_mesh(&mesh)
    }

    fn ensure_instance(&mut self, key: u64) {
        self.instances.entry(key).or_insert_with(|| {
            let uniform = self.context.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rustic-viewport-instance-uniform"),
                size: 2352,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = self
                .context
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("rustic-viewport-instance-bind-group"),
                    layout: &self.bind_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    }],
                });
            CachedViewportInstance {
                uniform,
                bind_group,
            }
        });
    }

    fn write_instance(
        &self,
        key: u64,
        model: [f32; 16],
        color: [f32; 4],
        unlit: bool,
        scene: &ViewportScene,
    ) {
        let lights = if unlit {
            &[][..]
        } else {
            scene.lights.as_slice()
        };
        let mut values = Vec::with_capacity(588);
        values.extend(multiply_matrix(scene.view_projection, model));
        values.extend(model);
        values.extend(color);
        values.extend(
            glam::Mat4::from_cols_array(&model)
                .inverse()
                .transpose()
                .to_cols_array(),
        );
        values.extend([
            f32::from(u8::try_from(lights.len().min(32)).unwrap_or(32)),
            if unlit { 1.0 } else { 0.0 },
            0.0,
            0.0,
        ]);
        for light in lights.iter().take(32) {
            let transform = glam::Mat4::from_cols_array(&light.transform);
            let (_, rotation, position) = transform.to_scale_rotation_translation();
            let direction = rotation * glam::Vec3::Z;
            let kind = match light.kind {
                engine_world::LightKind::Directional => 0.0,
                engine_world::LightKind::Point => 1.0,
                engine_world::LightKind::Spot => 2.0,
            };
            values.extend([position.x, position.y, position.z, kind]);
            values.extend([direction.x, direction.y, direction.z, light.range]);
            values.extend([
                light.color[0],
                light.color[1],
                light.color[2],
                light.intensity,
            ]);
            values.extend([
                light.spot_outer_angle_radians.cos(),
                (light.spot_outer_angle_radians * 0.8).cos(),
                0.0,
                0.0,
            ]);
        }
        values.resize(568, 0.0);
        let fallback = engine_world::SceneEnvironment::default();
        let env = if scene.environment.enabled {
            &scene.environment
        } else {
            &fallback
        };
        values.extend(env.ambient_color);
        values.push(env.ambient_intensity);
        values.extend(env.sun_color);
        values.push(env.sun_intensity);
        values.extend(
            glam::Vec3::from_array(env.sun_direction)
                .normalize()
                .to_array(),
        );
        values.push(0.0);
        values.extend(env.haze_color);
        values.push(env.haze_density);
        values.extend(scene.camera_position);
        values.push(env.haze_start);
        if let Some(instance) = self.instances.get(&key) {
            self.context
                .queue
                .write_buffer(&instance.uniform, 0, &f32_bytes(&values));
        }
    }
}

fn viewport_bind_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rustic-viewport-bind-layout"),
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
    })
}

fn viewport_pipeline(
    device: &wgpu::Device,
    bind_layout: &wgpu::BindGroupLayout,
    topology: wgpu::PrimitiveTopology,
    cull_mode: Option<wgpu::Face>,
    label: &'static str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rustic-viewport-wgsl"),
        source: wgpu::ShaderSource::Wgsl(VIEWPORT_SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rustic-viewport-pipeline-layout"),
        bind_group_layouts: &[Some(bind_layout)],
        immediate_size: 0,
    });
    let attributes = [
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
    ];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 24,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            })],
        },
        primitive: wgpu::PrimitiveState {
            topology,
            // Engine primitive meshes use clockwise winding when viewed from the
            // side their normals point toward. Match that convention so the
            // exterior is front-facing and back-face culling hides interiors.
            front_face: wgpu::FrontFace::Cw,
            cull_mode,
            ..wgpu::PrimitiveState::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
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

fn validate_viewport_mesh(mesh: &ViewportMesh) -> Result<(), RendererError> {
    if mesh.vertices.is_empty() || mesh.indices.is_empty() || !mesh.indices.len().is_multiple_of(3)
    {
        return Err(RendererError::InvalidMesh(
            "viewport mesh must contain triangles".to_owned(),
        ));
    }
    if mesh
        .indices
        .iter()
        .any(|index| *index as usize >= mesh.vertices.len())
    {
        return Err(RendererError::InvalidMesh(
            "viewport index is out of bounds".to_owned(),
        ));
    }
    if mesh
        .vertices
        .iter()
        .flat_map(|vertex| vertex.position.into_iter().chain(vertex.normal))
        .chain(mesh.model)
        .chain(mesh.color)
        .any(|value| !value.is_finite())
    {
        return Err(RendererError::InvalidMesh(
            "viewport mesh contains non-finite values".to_owned(),
        ));
    }
    Ok(())
}

fn viewport_vertex_bytes(vertices: &[ViewportVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * 24);
    for vertex in vertices {
        for value in vertex.position.into_iter().chain(vertex.normal) {
            bytes.extend_from_slice(&value.to_ne_bytes());
        }
    }
    bytes
}

fn viewport_mesh_digest(vertices: &[ViewportVertex], indices: &[u32]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(viewport_vertex_bytes(vertices));
    digest.update(u32_bytes(indices));
    digest.finalize().into()
}

fn read_viewport_target(
    device: &wgpu::Device,
    targets: &ViewportTargets,
) -> Result<Vec<u8>, RendererError> {
    let slice = targets.readback.slice(..);
    let (sender, receiver) = mpsc::sync_channel(1);
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result.map_err(|error| error.to_string()));
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(10)),
        })
        .map_err(|error| RendererError::Gpu(error.to_string()))?;
    receiver
        .recv_timeout(Duration::from_secs(10))
        .map_err(|error| RendererError::Gpu(error.to_string()))?
        .map_err(RendererError::Gpu)?;
    let mapped = slice
        .get_mapped_range()
        .map_err(|error| RendererError::Gpu(error.to_string()))?;
    let row_bytes = targets.width as usize * 4;
    let mut rgba = Vec::with_capacity(row_bytes * targets.height as usize);
    for row in mapped.chunks_exact(targets.padded_bytes_per_row as usize) {
        rgba.extend_from_slice(&row[..row_bytes]);
    }
    drop(mapped);
    targets.readback.unmap();
    Ok(rgba)
}

fn multiply_matrix(left: [f32; 16], right: [f32; 16]) -> [f32; 16] {
    let mut output = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            output[column * 4 + row] = (0..4)
                .map(|index| left[index * 4 + row] * right[column * 4 + index])
                .sum();
        }
    }
    output
}

#[allow(
    clippy::cast_precision_loss,
    reason = "viewport extents are bounded by GPU texture limits and mapped to float screen coordinates"
)]
fn selection_outline_model(
    mesh: &ViewportMesh,
    view_projection: [f32; 16],
    width: u32,
    height: u32,
) -> [f32; 16] {
    let mut half_extents = glam::Vec3::ZERO;
    for vertex in &mesh.vertices {
        half_extents = half_extents.max(glam::Vec3::from(vertex.position).abs());
    }
    let model = glam::Mat4::from_cols_array(&mesh.model);
    let view_projection = glam::Mat4::from_cols_array(&view_projection);
    let center = view_projection * model.w_axis;
    if !center.is_finite() || center.w.abs() <= f32::EPSILON {
        return mesh.model;
    }

    // Measure camera-plane pixels per world unit, not the projection of each
    // object axis. An axis facing the camera projects to zero even when the
    // object is large; dividing by that projection can inflate the shell until
    // it crosses the camera and fills the viewport.
    let pixels_per_world_unit = (view_projection.row(0).truncate().length() * width as f32)
        .max(view_projection.row(1).truncate().length() * height as f32)
        * 0.5
        / center.w.abs();
    if !pixels_per_world_unit.is_finite() || pixels_per_world_unit <= f32::EPSILON {
        return mesh.model;
    }
    let world_padding = SELECTION_OUTLINE_WIDTH_PIXELS / pixels_per_world_unit;
    let mut outline_scale = glam::Vec3::ONE;
    for axis in 0..3 {
        let world_radius = half_extents[axis] * model.col(axis).truncate().length();
        if world_radius > f32::EPSILON {
            outline_scale[axis] += world_padding / world_radius;
        }
    }
    (model * glam::Mat4::from_scale(outline_scale)).to_cols_array()
}

#[allow(
    clippy::too_many_lines,
    reason = "the first-frame backend keeps one labeled render transaction together for auditability"
)]
fn render_with_context(
    context: &BackendContext,
    width: u32,
    height: u32,
    mesh: &TexturedMesh,
) -> Result<RenderedFrame, RendererError> {
    let index_count = u32::try_from(mesh.indices.len())
        .map_err(|_| RendererError::InvalidMesh("index count exceeds u32".to_owned()))?;
    let device = &context.device;
    let queue = &context.queue;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rustic-m2-textured-mesh-wgsl"),
        source: wgpu::ShaderSource::Wgsl(MESH_SHADER.into()),
    });

    let uniform_bytes = f32_bytes(&mesh.model_view_projection);
    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("rustic-m2-camera-transform"),
        contents: &uniform_bytes,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rustic-m2-source-texture"),
        size: wgpu::Extent3d {
            width: mesh.texture_width,
            height: mesh.texture_height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        &mesh.texture_rgba8,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(mesh.texture_width * 4),
            rows_per_image: Some(mesh.texture_height),
        },
        texture.size(),
    );
    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("rustic-m2-linear-sampler"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });

    let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rustic-m2-bind-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
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
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rustic-m2-bind-group"),
        layout: &bind_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rustic-m2-pipeline-layout"),
        bind_group_layouts: &[Some(&bind_layout)],
        immediate_size: 0,
    });
    let vertex_attributes = [
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
    ];
    let vertex_layout = wgpu::VertexBufferLayout {
        array_stride: 20,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &vertex_attributes,
    };
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rustic-m2-textured-depth-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(vertex_layout)],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
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
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });

    let vertex_bytes = vertex_bytes(&mesh.vertices);
    let index_bytes = u32_bytes(&mesh.indices);
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("rustic-m2-vertices"),
        contents: &vertex_bytes,
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("rustic-m2-indices"),
        contents: &index_bytes,
        usage: wgpu::BufferUsages::INDEX,
    });

    let color = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rustic-m2-offscreen-color"),
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
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rustic-m2-offscreen-depth"),
        size: color.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());

    let unpadded_bytes_per_row = width.saturating_mul(4);
    let padded_bytes_per_row = align_to(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let readback_size = u64::from(padded_bytes_per_row).saturating_mul(u64::from(height));
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rustic-m2-readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("rustic-m2-command-encoder"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rustic-m2-main-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.035,
                        g: 0.045,
                        b: 0.065,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..index_count, 0, 0..1);
    }
    encoder.copy_texture_to_buffer(
        color.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        color.size(),
    );
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    let (sender, receiver) = mpsc::sync_channel(1);
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result.map_err(|error| error.to_string()));
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(10)),
        })
        .map_err(|error| RendererError::Gpu(error.to_string()))?;
    receiver
        .recv_timeout(Duration::from_secs(10))
        .map_err(|error| RendererError::Gpu(error.to_string()))?
        .map_err(RendererError::Gpu)?;
    let mapped = slice
        .get_mapped_range()
        .map_err(|error| RendererError::Gpu(error.to_string()))?;
    let mut rgba8 = Vec::with_capacity(width as usize * height as usize * 4);
    for row in mapped.chunks_exact(padded_bytes_per_row as usize) {
        rgba8.extend_from_slice(&row[..unpadded_bytes_per_row as usize]);
    }
    drop(mapped);
    readback.unmap();

    Ok(RenderedFrame {
        width,
        height,
        rgba8,
        adapter: context.diagnostics.clone(),
        labels: vec![
            "rustic-m2-command-encoder".to_owned(),
            "rustic-m2-main-pass".to_owned(),
            "rustic-m2-textured-depth-pipeline".to_owned(),
        ],
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "surface rendering mirrors the auditable offscreen M2 fixture without leaking backend resources"
)]
fn encode_surface_mesh(
    context: &BackendContext,
    color_format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    color_view: &wgpu::TextureView,
    mesh: &TexturedMesh,
) -> Result<wgpu::CommandBuffer, RendererError> {
    let index_count = u32::try_from(mesh.indices.len())
        .map_err(|_| RendererError::InvalidMesh("index count exceeds u32".to_owned()))?;
    let device = &context.device;
    let queue = &context.queue;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rustic-m2-surface-textured-mesh-wgsl"),
        source: wgpu::ShaderSource::Wgsl(MESH_SHADER.into()),
    });
    let uniform_bytes = f32_bytes(&mesh.model_view_projection);
    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("rustic-m2-surface-camera-transform"),
        contents: &uniform_bytes,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rustic-m2-surface-source-texture"),
        size: wgpu::Extent3d {
            width: mesh.texture_width,
            height: mesh.texture_height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        &mesh.texture_rgba8,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(mesh.texture_width * 4),
            rows_per_image: Some(mesh.texture_height),
        },
        texture.size(),
    );
    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("rustic-m2-surface-linear-sampler"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });
    let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rustic-m2-surface-bind-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
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
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rustic-m2-surface-bind-group"),
        layout: &bind_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rustic-m2-surface-pipeline-layout"),
        bind_group_layouts: &[Some(&bind_layout)],
        immediate_size: 0,
    });
    let vertex_attributes = [
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
    ];
    let vertex_layout = wgpu::VertexBufferLayout {
        array_stride: 20,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &vertex_attributes,
    };
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rustic-m2-surface-textured-depth-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(vertex_layout)],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
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
                format: color_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let vertex_bytes = vertex_bytes(&mesh.vertices);
    let index_bytes = u32_bytes(&mesh.indices);
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("rustic-m2-surface-vertices"),
        contents: &vertex_bytes,
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("rustic-m2-surface-indices"),
        contents: &index_bytes,
        usage: wgpu::BufferUsages::INDEX,
    });
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rustic-m2-surface-depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("rustic-m2-surface-command-encoder"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rustic-m2-surface-main-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.035,
                        g: 0.045,
                        b: 0.065,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..index_count, 0, 0..1);
    }
    Ok(encoder.finish())
}

const fn identity_matrix() -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn align_to(value: u32, alignment: u32) -> u32 {
    value.div_ceil(alignment).saturating_mul(alignment)
}

fn vertex_bytes(vertices: &[MeshVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * 20);
    for vertex in vertices {
        for value in vertex.position.into_iter().chain(vertex.uv) {
            bytes.extend_from_slice(&value.to_ne_bytes());
        }
    }
    bytes
}

fn f32_bytes(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(values));
    for value in values {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
    bytes
}

fn u32_bytes(values: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(values));
    for value in values {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
    bytes
}

/// Surface-state helper used by the native editor seam without exposing a wgpu surface.
pub fn update_surface_lifecycle(
    lifecycle: &mut SurfaceLifecycle,
    width: u32,
    height: u32,
    scale_factor: f64,
) {
    lifecycle.configure(width, height, scale_factor);
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests compare exact round trips and deterministic values"
)]
mod tests {
    use super::*;

    #[test]
    fn viewport_lighting_shader_is_valid() {
        validate_and_reflect_wgsl(VIEWPORT_SHADER).unwrap();
    }

    #[test]
    #[ignore = "requires a physical or software graphics adapter"]
    fn lighting_preserves_shadow_detail_and_highlight_gradation() {
        use engine_world::{EntityId, LightKind, RenderLight};
        let mut renderer = SceneViewportRenderer::new(BackendRequest::Auto).unwrap();
        let mut scene = ViewportScene {
            environment: engine_world::SceneEnvironment::default(),
            camera_position: [0.0; 3],
            view_projection: identity_matrix(),
            meshes: vec![ViewportMesh {
                instance_key: 1,
                mesh_key: 1,
                vertices: [[-1.0, -1.0, 0.5], [1.0, -1.0, 0.5], [0.0, 1.0, 0.5]]
                    .map(|position| ViewportVertex {
                        position,
                        normal: [0.0, 0.0, -1.0],
                    })
                    .to_vec(),
                indices: vec![0, 2, 1],
                model: identity_matrix(),
                color: [1.0; 4],
                selected: false,
            }],
            lights: vec![RenderLight {
                entity: EntityId::new(),
                transform: identity_matrix(),
                kind: LightKind::Directional,
                color: [1.0; 3],
                intensity: 0.0,
                range: 10.0,
                spot_outer_angle_radians: 45.0_f32.to_radians(),
                casts_shadows: false,
            }],
            guides: Vec::new(),
            grid_vertices: Vec::new(),
            clear_color: [0.0, 0.0, 0.0, 1.0],
        };
        let sample = |frame: &RenderedFrame, x: usize, y: usize| frame.rgba8[(y * 65 + x) * 4];
        let ambient = renderer.render(65, 65, &scene).unwrap().unwrap();
        assert!((85..=95).contains(&sample(&ambient, 32, 32)));
        scene.lights[0].intensity = 2.0;
        let bright = renderer.render(65, 65, &scene).unwrap().unwrap();
        scene.lights[0].intensity = 4.0;
        let brighter = renderer.render(65, 65, &scene).unwrap().unwrap();
        assert!(sample(&brighter, 32, 32) > sample(&bright, 32, 32));
        assert!(
            sample(&brighter, 32, 32) < 250,
            "highlights must not clip to white"
        );
        // Uniform lighting on one flat face must not produce dark pixel blocks.
        for y in 24..40 {
            for x in 24..40 {
                assert_eq!(sample(&bright, x, y), sample(&bright, 32, 32));
            }
        }
        // Inspector and imported colors are linear; do not decode them a second time.
        scene.meshes[0].color = [0.5, 0.5, 0.5, 1.0];
        scene.lights[0].intensity = 1.0;
        let gray = renderer.render(65, 65, &scene).unwrap().unwrap();
        assert!((159..=165).contains(&sample(&gray, 32, 32)));
        // The environment sun uses the same display conversion as scene lights.
        scene.lights.clear();
        scene.meshes[0].color = [1.0; 4];
        scene.environment.enabled = true;
        scene.environment.sun_direction = [0.0, 0.0, -1.0];
        scene.environment.sun_color = [1.0; 3];
        scene.environment.sun_intensity = 2.0;
        let sun = renderer.render(65, 65, &scene).unwrap().unwrap();
        assert_eq!(sample(&sun, 32, 32), sample(&bright, 32, 32));
        // A fully hazed surface displays the linear inspector haze color as sRGB.
        scene.environment.haze_color = [0.25, 0.5, 0.75];
        scene.environment.haze_density = 100.0;
        let haze = renderer.render(65, 65, &scene).unwrap().unwrap();
        let center = (32 * 65 + 32) * 4;
        for (actual, expected) in haze.rgba8[center..center + 3].iter().zip([137, 188, 225]) {
            assert!(actual.abs_diff(expected) <= 1);
        }
    }

    #[test]
    #[ignore = "requires a physical or software graphics adapter"]
    fn local_lights_obey_inverse_square_falloff_and_range() {
        use engine_world::{EntityId, LightKind, RenderLight};
        let mut renderer = SceneViewportRenderer::new(BackendRequest::Auto).unwrap();
        let mut scene = ViewportScene {
            environment: engine_world::SceneEnvironment::default(),
            camera_position: [0.0; 3],
            view_projection: identity_matrix(),
            meshes: vec![ViewportMesh {
                instance_key: 1,
                mesh_key: 1,
                vertices: [[-1.0, -1.0, 0.5], [1.0, -1.0, 0.5], [0.0, 1.0, 0.5]]
                    .map(|position| ViewportVertex {
                        position,
                        normal: [0.0, 0.0, -1.0],
                    })
                    .to_vec(),
                indices: vec![0, 2, 1],
                model: identity_matrix(),
                color: [1.0; 4],
                selected: false,
            }],
            lights: vec![RenderLight {
                entity: EntityId::new(),
                transform: identity_matrix(),
                kind: LightKind::Point,
                color: [1.0; 3],
                intensity: 0.5,
                range: 100.0,
                spot_outer_angle_radians: 45.0_f32.to_radians(),
                casts_shadows: false,
            }],
            guides: Vec::new(),
            grid_vertices: Vec::new(),
            clear_color: [0.0, 0.0, 0.0, 1.0],
        };
        let mut sample = |kind, distance: f32, range, intensity, rotation| {
            scene.lights[0].kind = kind;
            scene.lights[0].range = range;
            scene.lights[0].intensity = intensity;
            scene.lights[0].transform = glam::Mat4::from_rotation_translation(
                rotation,
                glam::Vec3::new(0.0, 0.0, 0.5 - distance),
            )
            .to_cols_array();
            let frame = renderer.render(65, 65, &scene).unwrap().unwrap();
            // Recover linear irradiance before comparing physical falloff.
            let encoded = f32::from(frame.rgba8[(32 * 65 + 32) * 4]) / 255.0;
            let mapped = if encoded <= 0.04045 {
                encoded / 12.92
            } else {
                ((encoded + 0.055) / 1.055).powf(2.4)
            };
            255.0 * mapped / (1.0 - mapped)
        };
        let identity = glam::Quat::IDENTITY;
        let ambient = sample(LightKind::Point, 1.0, 100.0, 0.0, identity);
        for kind in [LightKind::Point, LightKind::Spot] {
            let near = sample(kind, 1.0, 100.0, 0.5, identity) - ambient;
            let far = sample(kind, 2.0, 100.0, 0.5, identity) - ambient;
            assert!((near / far - 4.0).abs() < 0.25, "{kind:?}: {near}/{far}");
            assert_eq!(sample(kind, 4.0, 4.0, 0.5, identity), ambient);
            assert_eq!(sample(kind, 5.0, 4.0, 0.5, identity), ambient);
            let inside = sample(kind, 3.9, 4.0, 0.5, identity);
            assert!(inside <= ambient + 1.0, "cutoff should fade smoothly");
            let capped = sample(kind, 0.1, 100.0, 0.001, identity);
            assert!((sample(kind, 0.05, 100.0, 0.001, identity) - capped).abs() <= 1.0);
        }
        assert_eq!(
            sample(LightKind::Directional, 1.0, 100.0, 0.5, identity),
            sample(LightKind::Directional, 2.0, 100.0, 0.5, identity)
        );
        assert_eq!(
            sample(
                LightKind::Spot,
                1.0,
                100.0,
                0.5,
                glam::Quat::from_rotation_y(std::f32::consts::PI)
            ),
            ambient
        );
    }

    #[test]
    fn imported_skin_uses_live_joint_pose_and_preserves_bind_transform() {
        let bytes =
            include_bytes!("../../engine-assets/tests/fixtures/skinned_animation.gltf").to_vec();
        let engine_assets::DerivedArtifact::Model(model) = engine_assets::ImporterRegistry::import(
            &engine_assets::ImportRequest::new("rig.gltf", bytes),
        )
        .unwrap() else {
            panic!("expected model");
        };
        let asset = engine_world::AssetId::new();
        let root = engine_world::EntitySnapshot {
            mesh: Some(engine_world::Mesh { asset }),
            ..Default::default()
        };
        let mut joint = engine_world::EntitySnapshot {
            name: Some("node_1".into()),
            parent: Some(root.id),
            ..Default::default()
        };
        joint.local_transform.translation.x = 1.;
        let mut world = engine_world::SceneWorld::new();
        world
            .apply_commands(&[
                engine_world::WorldCommand::Spawn(Box::new(root.clone())),
                engine_world::WorldCommand::Spawn(Box::new(joint.clone())),
            ])
            .unwrap();
        world.propagate_transforms();
        let models = std::collections::BTreeMap::from([(asset, model)]);
        let bind = game_scene_with_models(&world, 1., &models);
        assert_eq!(bind.meshes[0].vertices[0].position, [0., 0., 0.]);
        joint.local_transform.translation.x = 2.;
        world
            .apply_commands(&[engine_world::WorldCommand::SetLocalTransform {
                entity: joint.id,
                value: joint.local_transform,
            }])
            .unwrap();
        world.propagate_transforms();
        let posed = game_scene_with_models(&world, 1., &models);
        assert_eq!(posed.meshes[0].vertices[0].position, [1., 0., 0.]);
        assert_eq!(posed.meshes[0].mesh_key, bind.meshes[0].mesh_key);
        assert!(
            posed.meshes[0]
                .vertices
                .iter()
                .flat_map(|v| v.normal)
                .all(f32::is_finite)
        );
    }

    #[test]
    fn shader_reflection_is_deterministic_and_engine_owned() {
        let first = validate_and_reflect_wgsl(MESH_SHADER).unwrap();
        let second = validate_and_reflect_wgsl(MESH_SHADER).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first.entry_points,
            vec![
                ("fs_main".to_owned(), ShaderStage::Fragment),
                ("vs_main".to_owned(), ShaderStage::Vertex),
            ]
        );
        assert_eq!(first.vertex_locations, vec![0, 1]);
        assert_eq!(first.bindings.len(), 3);
    }

    #[test]
    fn invalid_wgsl_has_a_localized_diagnostic() {
        let error = validate_and_reflect_wgsl("@vertex fn broken(").unwrap_err();
        assert!(matches!(error, RendererError::Shader(_)));
        assert!(error.to_string().contains("shader validation failed"));
    }

    #[test]
    fn malformed_meshes_are_rejected_before_gpu_initialization() {
        let mut mesh = TexturedMesh::triangle();
        mesh.indices[2] = 99;
        assert!(matches!(
            render_offscreen(BackendRequest::Auto, 64, 64, &mesh),
            Err(RendererError::InvalidMesh(_))
        ));
    }

    #[test]
    fn backend_order_is_explicit_and_platform_stable() {
        let candidates = BackendRequest::Auto.candidates();
        assert!(!candidates.is_empty());
        if cfg!(target_os = "windows") {
            assert_eq!(
                candidates[0],
                (RendererBackend::Vulkan, wgpu::Backends::VULKAN, true)
            );
            assert_eq!(candidates[1].0, RendererBackend::Direct3D12);
            assert_eq!(candidates[2].0, RendererBackend::Vulkan);
            assert_eq!(candidates[3].0, RendererBackend::OpenGl);
            assert!(candidates[1..].iter().all(|candidate| !candidate.2));
        }
    }

    #[test]
    #[ignore = "requires a physical or software graphics adapter; run explicitly for M2 qualification"]
    fn renders_triangle_and_indexed_textured_mesh() {
        let triangle = render_offscreen(BackendRequest::Auto, 128, 128, &TexturedMesh::triangle())
            .expect("render triangle");
        let mesh = render_offscreen(
            BackendRequest::Auto,
            128,
            128,
            &TexturedMesh::checkerboard_quad(),
        )
        .expect("render textured mesh");
        eprintln!(
            "qualified adapter={} backend={:?}",
            mesh.adapter.adapter_name, mesh.adapter.backend
        );
        assert_eq!(triangle.rgba8.len(), 128 * 128 * 4);
        assert_eq!(mesh.rgba8.len(), 128 * 128 * 4);
        assert_ne!(triangle.digest_hex(), mesh.digest_hex());
        assert!(mesh.labels.iter().any(|label| label.contains("main-pass")));
    }

    #[test]
    #[ignore = "requires a graphics adapter; run explicitly for viewport qualification"]
    fn persistent_viewport_reuses_resources_resizes_and_depth_tests() {
        let vertices = vec![
            ViewportVertex {
                position: [-0.8, -0.8, 0.0],
                normal: [0.0, 0.0, -1.0],
            },
            ViewportVertex {
                position: [0.8, -0.8, 0.0],
                normal: [0.0, 0.0, -1.0],
            },
            ViewportVertex {
                position: [0.0, 0.8, 0.0],
                normal: [0.0, 0.0, -1.0],
            },
        ];
        let mesh = |instance_key, mesh_key, z, color| ViewportMesh {
            instance_key,
            mesh_key,
            vertices: vertices.clone(),
            indices: vec![0, 2, 1],
            model: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, z, 1.0,
            ],
            color,
            selected: false,
        };
        let scene = ViewportScene {
            environment: engine_world::SceneEnvironment::default(),
            camera_position: [0.0; 3],
            view_projection: identity_matrix(),
            meshes: vec![
                mesh(1, 1, 0.8, [0.0, 0.0, 1.0, 1.0]),
                mesh(2, 1, 0.2, [1.0, 0.0, 0.0, 1.0]),
            ],
            lights: Vec::new(),
            guides: Vec::new(),
            grid_vertices: Vec::new(),
            clear_color: [0.0, 0.0, 0.0, 1.0],
        };
        let mut renderer = SceneViewportRenderer::new(BackendRequest::Auto).unwrap();
        let first = renderer.render(64, 64, &scene).unwrap().unwrap();
        let first_stats = renderer.stats();
        let second = renderer.render(64, 64, &scene).unwrap().unwrap();
        let second_stats = renderer.stats();
        assert_eq!(first_stats.mesh_uploads, second_stats.mesh_uploads);
        assert_eq!(
            first_stats.target_generations,
            second_stats.target_generations
        );
        assert_eq!(second_stats.frame_count, first_stats.frame_count + 1);
        let center = (32 * 64 + 32) * 4;
        assert!(first.rgba8[center] > first.rgba8[center + 2]);
        // A flat triangle against black must have partially covered edge pixels.
        // Single-sample rendering produces only black or the full interior color.
        assert!(
            first
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| { pixel[0] > 0 && pixel[0] < first.rgba8[center] })
        );
        assert_eq!(first.rgba8, second.rgba8);
        renderer.render(80, 64, &scene).unwrap();
        assert_eq!(
            renderer.stats().target_generations,
            second_stats.target_generations + 1
        );
        assert!(renderer.render(0, 0, &scene).unwrap().is_none());
    }

    #[test]
    #[ignore = "requires a graphics adapter; run explicitly for viewport qualification"]
    fn viewport_renders_colored_selection_guides() {
        let mut renderer = SceneViewportRenderer::new(BackendRequest::Auto).unwrap();
        let scene = ViewportScene {
            environment: engine_world::SceneEnvironment::default(),
            camera_position: [0.0; 3],
            view_projection: identity_matrix(),
            meshes: Vec::new(),
            lights: Vec::new(),
            guides: vec![ViewportGuide {
                key: 42,
                vertices: vec![[-0.8, 0.0, 0.2], [0.8, 0.0, 0.2]],
                color: [0.1, 0.8, 1.0, 1.0],
            }],
            grid_vertices: Vec::new(),
            clear_color: [0.0, 0.0, 0.0, 1.0],
        };
        let frame = renderer.render(64, 64, &scene).unwrap().unwrap();
        assert!(
            frame
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| { pixel[2] > 100 && pixel[1] > pixel[0].saturating_mul(2) })
        );
    }

    fn selected_cube(model: glam::Mat4) -> ViewportMesh {
        let primitive = engine_world::Primitive::Cube { size: 1.0 }.mesh().unwrap();
        ViewportMesh {
            instance_key: 10,
            mesh_key: 20,
            vertices: primitive
                .positions
                .iter()
                .zip(&primitive.normals)
                .map(|(position, normal)| ViewportVertex {
                    position: *position,
                    normal: *normal,
                })
                .collect(),
            indices: primitive.indices,
            model: model.to_cols_array(),
            color: [0.2, 0.35, 0.65, 1.0],
            selected: true,
        }
    }

    #[test]
    #[ignore = "requires a physical or software graphics adapter"]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "projected fixture samples lie inside the 256-pixel render target"
    )]
    fn environment_sun_shades_cube_faces_consistently_when_orbiting() {
        use glam::{Mat4, Vec3};
        let mut renderer = SceneViewportRenderer::new(BackendRequest::Auto).unwrap();
        let root = std::env::temp_dir().join(format!("rustic-lighting-hdr-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        image::codecs::hdr::HdrEncoder::new(std::fs::File::create(root.join("sky.hdr")).unwrap())
            .encode(&[image::Rgb([4.0, 0.2, 0.1]); 8], 4, 2)
            .unwrap();
        renderer.set_environment_root(&root);
        let mut mesh = selected_cube(Mat4::IDENTITY);
        mesh.color = [1.0; 4];
        for eye in [Vec3::new(3.0, 2.0, 3.0), Vec3::new(-3.0, 2.0, -3.0)] {
            let view = glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y);
            let projection = glam::camera::rh::proj::directx::perspective(
                55.0_f32.to_radians(),
                1.0,
                0.05,
                10_000.0,
            );
            let view_projection = projection * view;
            for environment in [
                engine_world::SceneEnvironment {
                    enabled: true,
                    sun_intensity: 8.0,
                    sun_color: [1.0; 3],
                    sun_direction: eye.to_array(),
                    ..Default::default()
                },
                engine_world::SceneEnvironment {
                    enabled: true,
                    sky_image: "sky.hdr".into(),
                    rotation_degrees: 180.0,
                    exposure: 1.5,
                    ambient_color: [0.214, 0.477, 0.85],
                    ambient_intensity: 5.0,
                    sun_color: [1.0, 0.7, 0.0],
                    sun_intensity: 10.0,
                    sun_direction: [0.3, 0.8, 0.4],
                    haze_density: 0.098,
                    haze_start: 100.0,
                    ..Default::default()
                },
            ] {
                let scene = ViewportScene {
                    environment,
                    camera_position: eye.to_array(),
                    view_projection: view_projection.to_cols_array(),
                    meshes: vec![mesh.clone()],
                    lights: Vec::new(),
                    guides: Vec::new(),
                    grid_vertices: Vec::new(),
                    clear_color: [0.0, 0.0, 0.0, 1.0],
                };
                let frame = renderer.render(256, 256, &scene).unwrap().unwrap();
                for axis in 0..3 {
                    let normal = Vec3::AXES[axis] * eye[axis].signum();
                    let mut face_value: Option<u8> = None;
                    for u in [-0.2, 0.0, 0.2] {
                        for v in [-0.2, 0.0, 0.2] {
                            let position = normal * 0.5
                                + Vec3::AXES[(axis + 1) % 3] * u
                                + Vec3::AXES[(axis + 2) % 3] * v;
                            let clip = view_projection * position.extend(1.0);
                            let ndc = clip.truncate() / clip.w;
                            let x = ((ndc.x * 0.5 + 0.5) * 256.0) as usize;
                            let y = ((0.5 - ndc.y * 0.5) * 256.0) as usize;
                            let pixel = frame.rgba8[(y * 256 + x) * 4];
                            assert!(pixel < 250, "sun-lit cube face clipped at {position}");
                            if let Some(value) = face_value {
                                assert!(
                                    pixel.abs_diff(value) <= 1,
                                    "flat face has a dark patch at {position}: {pixel} vs {value}"
                                );
                            }
                            face_value = Some(pixel);
                        }
                    }
                }
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn selection_outline_stays_bounded_near_camera_aligned_axes() {
        // Sweep through the axis-aligned views that used to divide by a nearly
        // zero projected radius. Include rotation and nonuniform object scale.
        for model in [
            glam::Mat4::IDENTITY,
            glam::Mat4::from_rotation_y(0.4)
                * glam::Mat4::from_scale(glam::Vec3::new(0.35, 0.5, 1.25)),
        ] {
            let mesh = selected_cube(model);
            for axis in [glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z] {
                let tangent = if axis == glam::Vec3::Y {
                    glam::Vec3::X
                } else {
                    glam::Vec3::Y
                };
                for offset in [-0.1, -0.01, -0.001, -0.0001, 0.0, 0.0001, 0.001, 0.01, 0.1] {
                    let eye = (axis + tangent * offset).normalize() * 3.0;
                    let view = glam::camera::rh::view::look_at_mat4(eye, glam::Vec3::ZERO, tangent);
                    let projection = glam::camera::rh::proj::directx::perspective(
                        60.0_f32.to_radians(),
                        1.0,
                        0.1,
                        100.0,
                    );
                    let outline = glam::Mat4::from_cols_array(&selection_outline_model(
                        &mesh,
                        (projection * view).to_cols_array(),
                        256,
                        256,
                    ));
                    assert!(outline.is_finite());
                    for column in 0..3 {
                        let ratio = outline.col(column).length() / model.col(column).length();
                        assert!(
                            (1.0..1.25).contains(&ratio),
                            "outline inflated by {ratio} at {eye}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "requires a graphics adapter; run explicitly for viewport qualification"]
    fn selection_outline_remains_a_border_when_orbiting_through_aligned_views() {
        let mut renderer = SceneViewportRenderer::new(BackendRequest::Auto).unwrap();
        let mesh = selected_cube(glam::Mat4::IDENTITY);
        for axis in [glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z] {
            let tangent = if axis == glam::Vec3::Y {
                glam::Vec3::X
            } else {
                glam::Vec3::Y
            };
            for offset in [-0.01, -0.001, 0.0, 0.001, 0.01] {
                let eye = (axis + tangent * offset).normalize() * 3.0;
                let view = glam::camera::rh::view::look_at_mat4(eye, glam::Vec3::ZERO, tangent);
                let projection = glam::camera::rh::proj::directx::perspective(
                    60.0_f32.to_radians(),
                    1.0,
                    0.1,
                    100.0,
                );
                let scene = ViewportScene {
                    environment: engine_world::SceneEnvironment::default(),
                    camera_position: [0.0; 3],
                    view_projection: (projection * view).to_cols_array(),
                    meshes: vec![mesh.clone()],
                    lights: Vec::new(),
                    guides: Vec::new(),
                    grid_vertices: Vec::new(),
                    clear_color: [0.0, 0.0, 0.0, 1.0],
                };
                let frame = renderer.render(256, 256, &scene).unwrap().unwrap();
                let mut orange_pixels = 0;
                for (index, pixel) in frame.rgba8.as_chunks::<4>().0.iter().enumerate() {
                    if pixel[0] > 64 && pixel[0] > pixel[1] && pixel[1] > pixel[2] {
                        orange_pixels += 1;
                        let x = index % 256;
                        let y = index / 256;
                        assert!(
                            (75..181).contains(&x) && (75..181).contains(&y),
                            "outline escaped cube border at ({x}, {y}) for camera {eye}"
                        );
                    }
                }
                assert!(orange_pixels > 0, "missing outline for camera {eye}");
                assert!(
                    orange_pixels < 2000,
                    "outline covered {orange_pixels} pixels for camera {eye}"
                );
            }
        }
    }

    #[test]
    #[ignore = "requires a graphics adapter; run explicitly for viewport qualification"]
    fn selection_outline_width_is_constant_across_object_scales() {
        let primitive = engine_world::Primitive::Cube { size: 1.0 }
            .mesh()
            .expect("cube mesh");
        let vertices = primitive
            .positions
            .iter()
            .zip(&primitive.normals)
            .map(|(position, normal)| ViewportVertex {
                position: *position,
                normal: *normal,
            })
            .collect::<Vec<_>>();
        let mesh = |scale: f32| ViewportMesh {
            instance_key: 10,
            mesh_key: 20,
            vertices: vertices.clone(),
            indices: primitive.indices.clone(),
            model: glam::Mat4::from_scale(glam::Vec3::new(scale, 0.5, 0.5)).to_cols_array(),
            color: [0.2, 0.35, 0.65, 1.0],
            selected: true,
        };
        let view = glam::camera::rh::view::look_at_mat4(
            glam::Vec3::new(0.0, 0.0, -3.0),
            glam::Vec3::ZERO,
            glam::Vec3::Y,
        );
        let projection =
            glam::camera::rh::proj::directx::perspective(60.0_f32.to_radians(), 1.0, 0.1, 100.0);
        let scene = |scale| ViewportScene {
            environment: engine_world::SceneEnvironment::default(),
            camera_position: [0.0; 3],
            view_projection: (projection * view).to_cols_array(),
            meshes: vec![mesh(scale)],
            lights: Vec::new(),
            guides: Vec::new(),
            grid_vertices: Vec::new(),
            clear_color: [0.0, 0.0, 0.0, 1.0],
        };
        let outline_run = |frame: &RenderedFrame| {
            let row = frame.height as usize / 2;
            let pixels =
                &frame.rgba8[row * frame.width as usize * 4..(row + 1) * frame.width as usize * 4];
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .skip_while(|pixel| pixel[0] < 64)
                .take_while(|pixel| pixel[0] > pixel[1] && pixel[1] > pixel[2])
                .count()
        };

        let mut renderer = SceneViewportRenderer::new(BackendRequest::Auto).unwrap();
        let small = renderer.render(256, 256, &scene(0.35)).unwrap().unwrap();
        let large = renderer.render(256, 256, &scene(1.25)).unwrap().unwrap();
        let small_width = outline_run(&small);
        let large_width = outline_run(&large);
        assert!(small_width > 0, "small object must have a visible outline");
        assert!(large_width > 0, "large object must have a visible outline");
        assert!(
            small_width.abs_diff(large_width) <= 1,
            "outline changed from {small_width}px to {large_width}px"
        );
    }
}

/// Builds a game frame from the live simulation world. Highest camera order wins.
/// No active camera produces an empty frame instead of a fabricated viewpoint.
pub fn game_scene(world: &engine_world::SceneWorld, aspect: f32) -> ViewportScene {
    let mut buffer = engine_world::RenderWorldBuffer::new();
    let extracted = buffer.extract(world);
    let mut scene = ViewportScene {
        environment: engine_world::SceneEnvironment::default(),
        camera_position: [0.0; 3],
        view_projection: identity_matrix(),
        meshes: Vec::new(),
        lights: extracted.lights.to_vec(),
        guides: Vec::new(),
        grid_vertices: Vec::new(),
        clear_color: [0.045, 0.06, 0.085, 1.0],
    };
    let Some(camera) = extracted.cameras.last() else {
        return scene;
    };
    scene.environment = world.environment().clone();
    scene.camera_position = glam::Mat4::from_cols_array(&camera.transform)
        .w_axis
        .truncate()
        .to_array();
    scene.view_projection = camera.view_projection(aspect).to_cols_array();
    for primitive in extracted.primitives {
        let Ok(mesh) = primitive.primitive.mesh() else {
            continue;
        };
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        primitive.entity.hash(&mut hasher);
        let key = hasher.finish();
        scene.meshes.push(ViewportMesh {
            instance_key: key,
            mesh_key: key,
            vertices: mesh
                .positions
                .iter()
                .zip(&mesh.normals)
                .map(|(position, normal)| ViewportVertex {
                    position: *position,
                    normal: *normal,
                })
                .collect(),
            indices: mesh.indices,
            model: primitive.transform,
            color: world
                .part_attributes(primitive.entity)
                .unwrap_or_default()
                .color,
            selected: false,
        });
    }
    scene
}

/// Render imported rigid and skinned models using source node poses from the runtime scene.
#[allow(
    clippy::too_many_lines,
    reason = "build model materials and skinned geometry against the same extracted frame"
)]
pub fn game_scene_with_models(
    world: &engine_world::SceneWorld,
    aspect: f32,
    models: &std::collections::BTreeMap<engine_world::AssetId, engine_assets::ModelArtifact>,
) -> ViewportScene {
    use glam::{Mat4, Vec3};
    let mut scene = game_scene(world, aspect);
    for entity in world.entity_ids() {
        let Ok(Some(mesh)) = world.mesh(entity) else {
            continue;
        };
        let Some(model) = models.get(&mesh.asset) else {
            continue;
        };
        let Ok(owner) = world.world_transform(entity) else {
            continue;
        };
        let mut matrices = Vec::new();
        for (index, node) in model.nodes.iter().enumerate() {
            let name = format!("node_{index}");
            let posed = world.entity_ids().find(|id| {
                if world.snapshot(*id).ok().and_then(|s| s.name).as_deref() != Some(&name) {
                    return false;
                }
                let mut parent = world.parent(*id).ok().flatten();
                while let Some(p) = parent {
                    if p == entity {
                        return true;
                    }
                    parent = world.parent(p).ok().flatten();
                }
                false
            });
            if let Some(id) = posed {
                matrices.push(
                    world
                        .world_transform(id)
                        .map_or(Mat4::IDENTITY, |t| owner.0.inverse() * t.0),
                );
            } else {
                let local = |node: &engine_assets::ModelNode| {
                    Mat4::from_scale_rotation_translation(
                        glam::DVec3::from_array(node.scale).as_vec3(),
                        glam::DQuat::from_array(node.rotation).as_quat(),
                        glam::DVec3::from_array(node.translation).as_vec3(),
                    )
                };
                let mut matrix = local(node);
                let mut parent = node.parent;
                let mut depth = 0;
                while let Some(index) = parent {
                    if depth >= model.nodes.len() {
                        break;
                    }
                    let Some(node) = model.nodes.get(index) else {
                        break;
                    };
                    matrix = local(node) * matrix;
                    parent = node.parent;
                    depth += 1;
                }
                matrices.push(matrix);
            }
        }
        for (index, source) in model.meshes.iter().enumerate() {
            let vertices = source
                .positions
                .iter()
                .enumerate()
                .map(|(vertex, p)| {
                    let mut position = Vec3::from_array(*p);
                    let mut normal = Vec3::from_array(
                        source.normals.get(vertex).copied().unwrap_or([0., 1., 0.]),
                    );
                    if let (Some(skin), Some(joints), Some(weights)) = (
                        &source.skin,
                        source.joints.get(vertex),
                        source.weights.get(vertex),
                    ) {
                        let mut skinned = Vec3::ZERO;
                        let mut n = Vec3::ZERO;
                        let mut total = 0.;
                        for k in 0..4 {
                            let palette = usize::from(joints[k]);
                            if weights[k] <= 0.0 {
                                continue;
                            }
                            if let (Some(joint), Some(bind)) =
                                (skin.joints.get(palette), skin.inverse_bind.get(palette))
                                && let Some(matrix) = matrices.get(*joint)
                            {
                                let matrix = *matrix * Mat4::from_cols_array(bind);
                                skinned += matrix.transform_point3(position) * weights[k];
                                n += matrix.inverse().transpose().transform_vector3(normal)
                                    * weights[k];
                                total += weights[k];
                            }
                        }
                        if total > 0.0 {
                            position = skinned / total;
                            normal = n.normalize_or_zero();
                        }
                    } else if let Some(matrix) = source.source_node.and_then(|i| matrices.get(i)) {
                        position = matrix.transform_point3(position);
                        normal = matrix
                            .inverse()
                            .transpose()
                            .transform_vector3(normal)
                            .normalize_or_zero();
                    }
                    ViewportVertex {
                        position: position.to_array(),
                        normal: normal.to_array(),
                    }
                })
                .collect();
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (entity, index).hash(&mut hasher);
            let key = hasher.finish();
            let mut color = model
                .materials
                .get(source.material_index.unwrap_or(0))
                .map_or([0.6, 0.6, 0.6, 1.], |m| m.base_color);
            color[3] *= world.part_attributes(entity).unwrap_or_default().color[3];
            scene.meshes.push(ViewportMesh {
                instance_key: key,
                mesh_key: key,
                vertices,
                indices: source.indices.clone(),
                model: owner.0.to_cols_array(),
                color,
                selected: false,
            });
        }
    }
    scene
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests compare exact deterministic render data"
)]
mod game_view_tests {
    use super::*;
    use engine_world::*;
    use glam::{Quat, Vec3};

    fn fixture() -> (SceneWorld, EntityId, EntityId) {
        let camera = EntitySnapshot {
            camera: Some(Camera::default()),
            local_transform: LocalTransform {
                translation: Vec3::new(0.0, 0.0, -5.0),
                ..LocalTransform::IDENTITY
            },
            ..EntitySnapshot::default()
        };
        let light = EntitySnapshot {
            light: Some(Light::default()),
            ..EntitySnapshot::default()
        };
        let ids = (camera.id, light.id);
        let mut world = SceneWorld::new();
        world
            .apply_commands(&[
                WorldCommand::Spawn(Box::new(camera)),
                WorldCommand::Spawn(Box::new(light)),
                WorldCommand::Spawn(Box::new(EntitySnapshot {
                    primitive: Some(Primitive::Cube { size: 2.0 }),
                    ..EntitySnapshot::default()
                })),
            ])
            .unwrap();
        world.propagate_transforms();
        (world, ids.0, ids.1)
    }

    #[test]
    fn game_camera_selection_and_parent_transform() {
        let (mut world, camera, _) = fixture();
        let first = game_scene(&world, 1.0);
        assert_eq!(first.meshes.len(), 1);
        let parent = EntitySnapshot {
            local_transform: LocalTransform {
                translation: Vec3::X,
                ..LocalTransform::IDENTITY
            },
            ..EntitySnapshot::default()
        };
        world
            .apply_commands(&[
                WorldCommand::Spawn(Box::new(parent.clone())),
                WorldCommand::SetParent {
                    child: camera,
                    parent: Some(parent.id),
                },
            ])
            .unwrap();
        world.propagate_transforms();
        assert_ne!(
            first.view_projection,
            game_scene(&world, 1.0).view_projection
        );
        let other = EntitySnapshot {
            camera: Some(Camera {
                order: 10,
                ..Camera::default()
            }),
            ..EntitySnapshot::default()
        };
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(other.clone()))])
            .unwrap();
        world.propagate_transforms();
        let mut buffer = RenderWorldBuffer::new();
        assert_eq!(
            buffer.extract(&world).cameras.last().unwrap().entity,
            other.id
        );
        world
            .apply_commands(&[
                WorldCommand::SetCamera {
                    entity: camera,
                    value: None,
                },
                WorldCommand::SetCamera {
                    entity: other.id,
                    value: Some(Camera {
                        active: false,
                        ..Camera::default()
                    }),
                },
            ])
            .unwrap();
        assert!(game_scene(&world, 1.0).meshes.is_empty());
    }

    #[test]
    #[ignore = "requires a graphics adapter"]
    #[allow(
        clippy::too_many_lines,
        reason = "keep the complete integration fixture and assertions together"
    )]
    fn game_camera_and_light_attributes_change_rendered_pixels() {
        let (mut world, camera, light) = fixture();
        let mut renderer = SceneViewportRenderer::new(BackendRequest::Auto).unwrap();
        let mut render = |world: &SceneWorld| {
            renderer
                .render(256, 256, &game_scene(world, 1.0))
                .unwrap()
                .unwrap()
        };
        let base = render(&world);
        let center = (128 * 256 + 128) * 4;
        assert!(
            base.rgba8[center] > 100,
            "camera must see the illuminated cube"
        );
        world
            .apply_commands(&[WorldCommand::SetLight {
                entity: light,
                value: Some(Light {
                    intensity: 0.0,
                    ..Light::default()
                }),
            }])
            .unwrap();
        let dark = render(&world);
        assert!(dark.rgba8[center] < base.rgba8[center] / 2);
        world
            .apply_commands(&[WorldCommand::SetLight {
                entity: light,
                value: Some(Light {
                    color: Vec3::X,
                    ..Light::default()
                }),
            }])
            .unwrap();
        let red = render(&world);
        assert!(red.rgba8[center] > red.rgba8[center + 1] * 2);
        world
            .apply_commands(&[
                WorldCommand::SetLocalTransform {
                    entity: light,
                    value: LocalTransform {
                        translation: Vec3::new(0.0, 0.0, -3.0),
                        ..LocalTransform::IDENTITY
                    },
                },
                WorldCommand::SetLight {
                    entity: light,
                    value: Some(Light {
                        kind: LightKind::Point,
                        range: 10.0,
                        ..Light::default()
                    }),
                },
            ])
            .unwrap();
        world.propagate_transforms();
        let point = render(&world);
        world
            .apply_commands(&[WorldCommand::SetLight {
                entity: light,
                value: Some(Light {
                    kind: LightKind::Point,
                    range: 0.5,
                    ..Light::default()
                }),
            }])
            .unwrap();
        assert!(render(&world).rgba8[center] < point.rgba8[center]);
        world
            .apply_commands(&[WorldCommand::SetLight {
                entity: light,
                value: Some(Light {
                    kind: LightKind::Spot,
                    ..Light::default()
                }),
            }])
            .unwrap();
        let spot = render(&world);
        world
            .apply_commands(&[WorldCommand::SetLocalTransform {
                entity: light,
                value: LocalTransform {
                    translation: Vec3::new(0.0, 0.0, -3.0),
                    rotation: Quat::from_rotation_y(std::f32::consts::PI),
                    ..LocalTransform::IDENTITY
                },
            }])
            .unwrap();
        world.propagate_transforms();
        assert!(render(&world).rgba8[center] < spot.rgba8[center]);
        world
            .apply_commands(&[WorldCommand::SetCamera {
                entity: camera,
                value: Some(Camera {
                    projection: CameraProjection::Perspective {
                        vertical_fov_radians: 100.0_f32.to_radians(),
                        near: 0.1,
                        far: 100.0,
                    },
                    ..Camera::default()
                }),
            }])
            .unwrap();
        let wide = render(&world);
        assert_ne!(wide.rgba8, dark.rgba8);
        world
            .apply_commands(&[WorldCommand::SetCamera {
                entity: camera,
                value: Some(Camera {
                    projection: CameraProjection::Orthographic {
                        vertical_size: 6.0,
                        near: 0.1,
                        far: 100.0,
                    },
                    ..Camera::default()
                }),
            }])
            .unwrap();
        let ortho = render(&world);
        assert_ne!(wide.rgba8, ortho.rgba8);
        world
            .apply_commands(&[WorldCommand::SetCamera {
                entity: camera,
                value: Some(Camera {
                    projection: CameraProjection::Orthographic {
                        vertical_size: 6.0,
                        near: 0.1,
                        far: 1.0,
                    },
                    ..Camera::default()
                }),
            }])
            .unwrap();
        assert_ne!(ortho.rgba8, render(&world).rgba8);
        if let Ok(directory) = std::env::var("RUSTIC_RENDER_EVIDENCE") {
            std::fs::create_dir_all(&directory).unwrap();
            for (name, frame) in [
                ("lit", base),
                ("dark", dark),
                ("red", red),
                ("point", point),
                ("spot", spot),
                ("orthographic", ortho),
            ] {
                let mut bytes = format!("P6\n{} {}\n255\n", frame.width, frame.height).into_bytes();
                for pixel in frame.rgba8.as_chunks::<4>().0 {
                    bytes.extend_from_slice(&pixel[..3]);
                }
                std::fs::write(
                    std::path::Path::new(&directory).join(format!("{name}.ppm")),
                    bytes,
                )
                .unwrap();
            }
        }
    }
}
