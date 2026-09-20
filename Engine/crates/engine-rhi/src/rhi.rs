//! Backend-neutral resource, submission, reflection, and surface contracts.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use thiserror::Error;

/// Buffer handle marker.
pub enum BufferResource {}
/// Texture handle marker.
pub enum TextureResource {}
/// Sampler handle marker.
pub enum SamplerResource {}
/// Shader handle marker.
pub enum ShaderResource {}
/// Render-pipeline handle marker.
pub enum PipelineResource {}

/// A stale-safe opaque resource reference. Concrete backend objects never cross this boundary.
#[repr(C)]
pub struct Handle<T> {
    index: u32,
    generation: u32,
    marker: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    const fn new(index: u32, generation: u32) -> Self {
        Self {
            index,
            generation,
            marker: PhantomData,
        }
    }

    /// Dense slot index used in diagnostics and protocol payloads.
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Slot generation used to reject stale handles.
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T> Eq for Handle<T> {}

impl<T> Hash for Handle<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self.generation.hash(state);
    }
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Handle")
            .field("index", &self.index)
            .field("generation", &self.generation)
            .finish()
    }
}

impl<T> Serialize for Handle<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (self.index, self.generation).serialize(serializer)
    }
}

impl<'de, T> Deserialize<'de> for Handle<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (index, generation) = <(u32, u32)>::deserialize(deserializer)?;
        Ok(Self::new(index, generation))
    }
}

/// Typed buffer handle.
pub type BufferHandle = Handle<BufferResource>;
/// Typed texture handle.
pub type TextureHandle = Handle<TextureResource>;
/// Typed sampler handle.
pub type SamplerHandle = Handle<SamplerResource>;
/// Typed shader handle.
pub type ShaderHandle = Handle<ShaderResource>;
/// Typed render-pipeline handle.
pub type PipelineHandle = Handle<PipelineResource>;

/// Stable engine buffer-usage flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BufferUsage(u32);

impl BufferUsage {
    /// Vertex-input data.
    pub const VERTEX: Self = Self(1 << 0);
    /// Index-input data.
    pub const INDEX: Self = Self(1 << 1);
    /// Uniform/constant data.
    pub const UNIFORM: Self = Self(1 << 2);
    /// Storage data.
    pub const STORAGE: Self = Self(1 << 3);
    /// Transfer source.
    pub const COPY_SOURCE: Self = Self(1 << 4);
    /// Transfer destination.
    pub const COPY_DESTINATION: Self = Self(1 << 5);

    /// Empty usage set.
    pub const NONE: Self = Self(0);

    /// Combines usage flags.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Tests whether every requested flag is present.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// Stable engine texture-usage flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TextureUsage(u32);

impl TextureUsage {
    /// Sampled in a shader.
    pub const SAMPLED: Self = Self(1 << 0);
    /// Color attachment.
    pub const COLOR_ATTACHMENT: Self = Self(1 << 1);
    /// Depth/stencil attachment.
    pub const DEPTH_ATTACHMENT: Self = Self(1 << 2);
    /// Transfer source.
    pub const COPY_SOURCE: Self = Self(1 << 3);
    /// Transfer destination.
    pub const COPY_DESTINATION: Self = Self(1 << 4);

    /// Combines usage flags.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Tests whether every requested flag is present.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// Backend-independent pixel/attachment formats used by the initial renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureFormat {
    /// Linear eight-bit red/green/blue/alpha.
    Rgba8Unorm,
    /// sRGB eight-bit red/green/blue/alpha.
    Rgba8Srgb,
    /// Linear eight-bit blue/green/red/alpha frame transport.
    Bgra8Unorm,
    /// 32-bit floating-point depth.
    Depth32Float,
}

/// Index-buffer element width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexFormat {
    /// 16-bit unsigned indices.
    Uint16,
    /// 32-bit unsigned indices.
    Uint32,
}

/// Primitive topology understood by the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimitiveTopology {
    /// Independent triangle list.
    TriangleList,
    /// Independent line list.
    LineList,
}

/// Immutable buffer creation contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BufferDescriptor {
    /// Human-readable capture/debug label.
    pub label: String,
    /// Buffer size in bytes.
    pub size: u64,
    /// Intended uses.
    pub usage: BufferUsage,
}

impl BufferDescriptor {
    fn validate(&self) -> Result<(), RhiError> {
        if self.size == 0 {
            return Err(RhiError::InvalidDescriptor(
                "buffer size must be non-zero".to_owned(),
            ));
        }
        if self.usage == BufferUsage::NONE {
            return Err(RhiError::InvalidDescriptor(
                "buffer usage must be non-empty".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Immutable texture creation contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextureDescriptor {
    /// Human-readable capture/debug label.
    pub label: String,
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// Array/depth layers.
    pub depth_or_layers: u32,
    /// Mip levels.
    pub mip_levels: u32,
    /// Multisample count.
    pub sample_count: u32,
    /// Pixel format.
    pub format: TextureFormat,
    /// Intended uses.
    pub usage: TextureUsage,
}

impl TextureDescriptor {
    fn validate(&self) -> Result<(), RhiError> {
        if self.width == 0
            || self.height == 0
            || self.depth_or_layers == 0
            || self.mip_levels == 0
            || self.sample_count == 0
        {
            return Err(RhiError::InvalidDescriptor(
                "texture dimensions, levels, and sample count must be non-zero".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Texture filtering mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterMode {
    /// Nearest-neighbor sampling.
    Nearest,
    /// Linear sampling.
    Linear,
}

/// Texture address mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AddressMode {
    /// Clamp at texture edges.
    ClampToEdge,
    /// Repeat normalized coordinates.
    Repeat,
    /// Mirror each repeated tile.
    MirrorRepeat,
}

/// Immutable sampler creation contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SamplerDescriptor {
    /// Human-readable capture/debug label.
    pub label: String,
    /// Minification filter.
    pub min_filter: FilterMode,
    /// Magnification filter.
    pub mag_filter: FilterMode,
    /// Mipmap filter.
    pub mip_filter: FilterMode,
    /// Horizontal addressing.
    pub address_u: AddressMode,
    /// Vertical addressing.
    pub address_v: AddressMode,
}

/// Shader stages used in reflection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShaderStage {
    /// Vertex shader.
    Vertex,
    /// Fragment/pixel shader.
    Fragment,
    /// Compute shader.
    Compute,
}

/// Reflected resource kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingKind {
    /// Uniform/constant buffer.
    UniformBuffer,
    /// Read-only storage buffer.
    ReadOnlyStorageBuffer,
    /// Writable storage buffer.
    StorageBuffer,
    /// Sampled texture.
    SampledTexture,
    /// Storage image.
    StorageTexture,
    /// Sampler.
    Sampler,
}

/// One engine-owned shader binding record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShaderBinding {
    /// Bind group/set index.
    pub group: u32,
    /// Binding index within the group.
    pub binding: u32,
    /// Resource family.
    pub kind: BindingKind,
    /// Optional WGSL variable name.
    pub name: Option<String>,
}

/// Backend-independent shader reflection output.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ShaderReflection {
    /// Declared entry points and stages.
    pub entry_points: Vec<(String, ShaderStage)>,
    /// Declared resource bindings in deterministic group/binding order.
    pub bindings: Vec<ShaderBinding>,
    /// Vertex attribute locations used by the vertex entry point.
    pub vertex_locations: Vec<u32>,
}

/// WGSL shader source and validated reflection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShaderDescriptor {
    /// Human-readable capture/debug label.
    pub label: String,
    /// Canonical WGSL source.
    pub wgsl: String,
    /// Reflection produced by a validated compiler front end.
    pub reflection: ShaderReflection,
}

/// Initial render-pipeline description.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineDescriptor {
    /// Human-readable capture/debug label.
    pub label: String,
    /// Validated shader module.
    pub shader: ShaderHandle,
    /// Vertex entry-point name.
    pub vertex_entry: String,
    /// Fragment entry-point name.
    pub fragment_entry: String,
    /// Primitive topology.
    pub topology: PrimitiveTopology,
    /// Color attachment format.
    pub color_format: TextureFormat,
    /// Optional depth attachment format.
    pub depth_format: Option<TextureFormat>,
}

/// Clear color in linear floating-point space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ClearColor {
    /// Red channel.
    pub red: f64,
    /// Green channel.
    pub green: f64,
    /// Blue channel.
    pub blue: f64,
    /// Alpha channel.
    pub alpha: f64,
}

/// Coarse command stream recorded by renderer code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RenderCommand {
    /// Begin a labeled render pass targeting color and optional depth textures.
    BeginPass {
        /// Capture/debug label.
        label: String,
        /// Color attachment.
        color: TextureHandle,
        /// Optional depth attachment.
        depth: Option<TextureHandle>,
        /// Color clear.
        clear: ClearColor,
        /// Depth clear.
        clear_depth: f32,
    },
    /// Select a render pipeline.
    SetPipeline(PipelineHandle),
    /// Select a vertex buffer.
    SetVertexBuffer(BufferHandle),
    /// Select an index buffer and format.
    SetIndexBuffer(BufferHandle, IndexFormat),
    /// Draw non-indexed geometry.
    Draw {
        /// Vertex count.
        vertices: u32,
        /// Instance count.
        instances: u32,
    },
    /// Draw indexed geometry.
    DrawIndexed {
        /// Index count.
        indices: u32,
        /// Instance count.
        instances: u32,
    },
    /// End the active render pass.
    EndPass,
    /// Copy a texture into a CPU-readable buffer.
    CopyTextureToBuffer {
        /// Source texture.
        source: TextureHandle,
        /// Destination buffer.
        destination: BufferHandle,
    },
}

/// One immutable, labeled submission unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandList {
    /// Capture/debug label.
    pub label: String,
    /// Commands in execution order.
    pub commands: Vec<RenderCommand>,
}

/// Monotonic submission serial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SubmissionId(pub u64);

/// Surface lifecycle independent of a windowing or GPU implementation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurfaceState {
    /// Surface has a non-zero drawable area.
    Active {
        /// Physical width.
        width: u32,
        /// Physical height.
        height: u32,
        /// Logical-to-physical scale.
        scale_factor: f64,
    },
    /// Zero-sized/minimized; presentation is intentionally suspended.
    Suspended,
    /// Surface became invalid and must be recreated.
    Lost,
}

/// Deterministic surface state machine used by native and test backends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceLifecycle {
    state: SurfaceState,
}

impl SurfaceLifecycle {
    /// Starts suspended until the first non-zero configure event.
    pub const fn new() -> Self {
        Self {
            state: SurfaceState::Suspended,
        }
    }

    /// Applies a resize/DPI event. A zero extent suspends presentation.
    pub fn configure(&mut self, width: u32, height: u32, scale_factor: f64) {
        self.state = if width == 0 || height == 0 {
            SurfaceState::Suspended
        } else {
            SurfaceState::Active {
                width,
                height,
                scale_factor: scale_factor.max(f64::EPSILON),
            }
        };
    }

    /// Marks the surface as lost without affecting project data.
    pub fn mark_lost(&mut self) {
        self.state = SurfaceState::Lost;
    }

    /// Current surface state.
    pub const fn state(self) -> SurfaceState {
        self.state
    }
}

impl Default for SurfaceLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

/// Health and work counters exported by every backend.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RhiDiagnostics {
    /// Live resources across all registries.
    pub live_resources: u64,
    /// Resources waiting for a completed submission before destruction.
    pub pending_retirements: u64,
    /// Total command lists accepted.
    pub submissions: u64,
    /// Total draw calls accepted.
    pub draw_calls: u64,
    /// Rejected stale-handle uses.
    pub stale_handle_errors: u64,
    /// Recoverable device-loss events.
    pub device_losses: u64,
    /// Nested CPU diagnostic scope labels.
    pub cpu_scopes: Vec<String>,
    /// Backend GPU scope labels.
    pub gpu_scopes: Vec<String>,
}

/// Stable failures returned across the backend boundary.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum RhiError {
    /// Invalid immutable descriptor.
    #[error("invalid RHI descriptor: {0}")]
    InvalidDescriptor(String),
    /// Resource was destroyed or belongs to an older generation.
    #[error("stale {kind} handle at slot {index}, generation {generation}")]
    StaleHandle {
        /// Resource family.
        kind: &'static str,
        /// Slot index.
        index: u32,
        /// Supplied generation.
        generation: u32,
    },
    /// Command list violates pass or resource rules.
    #[error("invalid command list: {0}")]
    InvalidCommand(String),
    /// GPU device was lost; callers may recreate on another validated backend.
    #[error("graphics device was lost: {0}")]
    DeviceLost(String),
    /// Surface is suspended/lost/unavailable.
    #[error("render surface is unavailable: {0}")]
    SurfaceUnavailable(String),
}

/// Backend-neutral device operations used by the renderer.
pub trait RhiDevice {
    /// Creates a buffer, optionally initialized with bytes.
    ///
    /// # Errors
    ///
    /// Returns a descriptor, allocation, stale-device, or device-loss error.
    fn create_buffer(
        &mut self,
        descriptor: BufferDescriptor,
        initial_data: &[u8],
    ) -> Result<BufferHandle, RhiError>;
    /// Creates a texture, optionally initialized with tightly packed bytes.
    ///
    /// # Errors
    ///
    /// Returns a descriptor, allocation, stale-device, or device-loss error.
    fn create_texture(
        &mut self,
        descriptor: TextureDescriptor,
        initial_data: &[u8],
    ) -> Result<TextureHandle, RhiError>;
    /// Creates a sampler.
    ///
    /// # Errors
    ///
    /// Returns a descriptor, allocation, stale-device, or device-loss error.
    fn create_sampler(&mut self, descriptor: SamplerDescriptor) -> Result<SamplerHandle, RhiError>;
    /// Creates a validated shader module.
    ///
    /// # Errors
    ///
    /// Returns a shader-validation, allocation, or device-loss error.
    fn create_shader(&mut self, descriptor: ShaderDescriptor) -> Result<ShaderHandle, RhiError>;
    /// Creates a render pipeline.
    ///
    /// # Errors
    ///
    /// Returns a stale shader, descriptor, allocation, or device-loss error.
    fn create_pipeline(
        &mut self,
        descriptor: PipelineDescriptor,
    ) -> Result<PipelineHandle, RhiError>;
    /// Defers buffer destruction until prior submissions complete.
    ///
    /// # Errors
    ///
    /// Returns an error when the handle is stale or belongs to no live buffer.
    fn retire_buffer(&mut self, handle: BufferHandle) -> Result<(), RhiError>;
    /// Defers texture destruction until prior submissions complete.
    ///
    /// # Errors
    ///
    /// Returns an error when the handle is stale or belongs to no live texture.
    fn retire_texture(&mut self, handle: TextureHandle) -> Result<(), RhiError>;
    /// Defers sampler destruction until prior submissions complete.
    ///
    /// # Errors
    ///
    /// Returns an error when the handle is stale or belongs to no live sampler.
    fn retire_sampler(&mut self, handle: SamplerHandle) -> Result<(), RhiError>;
    /// Defers shader destruction until prior submissions complete.
    ///
    /// # Errors
    ///
    /// Returns an error when the handle is stale or belongs to no live shader.
    fn retire_shader(&mut self, handle: ShaderHandle) -> Result<(), RhiError>;
    /// Defers pipeline destruction until prior submissions complete.
    ///
    /// # Errors
    ///
    /// Returns an error when the handle is stale or belongs to no live pipeline.
    fn retire_pipeline(&mut self, handle: PipelineHandle) -> Result<(), RhiError>;
    /// Validates and submits one coarse command list.
    ///
    /// # Errors
    ///
    /// Returns a command-validation, stale-handle, surface, or device-loss error.
    fn submit(&mut self, commands: CommandList) -> Result<SubmissionId, RhiError>;
    /// Releases resources whose last referenced submission has completed.
    fn complete_through(&mut self, submission: SubmissionId);
    /// Current diagnostics snapshot.
    fn diagnostics(&self) -> RhiDiagnostics;
}

#[derive(Debug, Clone)]
struct Slot<T> {
    generation: u32,
    value: Option<T>,
    last_used: SubmissionId,
    retire_after: Option<SubmissionId>,
}

#[derive(Debug, Clone)]
struct Registry<T> {
    kind: &'static str,
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
}

impl<T> Registry<T> {
    fn new(kind: &'static str) -> Self {
        Self {
            kind,
            slots: Vec::new(),
            free: Vec::new(),
        }
    }

    fn insert<R>(&mut self, value: T) -> Handle<R> {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.value = Some(value);
            slot.last_used = SubmissionId(0);
            slot.retire_after = None;
            return Handle::new(index, slot.generation);
        }
        let index = u32::try_from(self.slots.len()).expect("resource slot index overflow");
        self.slots.push(Slot {
            generation: 1,
            value: Some(value),
            last_used: SubmissionId(0),
            retire_after: None,
        });
        Handle::new(index, 1)
    }

    fn slot<R>(&self, handle: Handle<R>) -> Result<&Slot<T>, RhiError> {
        let Some(slot) = self.slots.get(handle.index as usize) else {
            return Err(self.stale(handle));
        };
        if slot.generation != handle.generation
            || slot.value.is_none()
            || slot.retire_after.is_some()
        {
            return Err(self.stale(handle));
        }
        Ok(slot)
    }

    fn mark_used<R>(
        &mut self,
        handle: Handle<R>,
        submission: SubmissionId,
    ) -> Result<(), RhiError> {
        let kind = self.kind;
        let Some(slot) = self.slots.get_mut(handle.index as usize) else {
            return Err(RhiError::StaleHandle {
                kind,
                index: handle.index,
                generation: handle.generation,
            });
        };
        if slot.generation != handle.generation
            || slot.value.is_none()
            || slot.retire_after.is_some()
        {
            return Err(RhiError::StaleHandle {
                kind,
                index: handle.index,
                generation: handle.generation,
            });
        }
        slot.last_used = submission;
        Ok(())
    }

    fn retire<R>(&mut self, handle: Handle<R>) -> Result<(), RhiError> {
        let _ = self.slot(handle)?;
        let slot = &mut self.slots[handle.index as usize];
        slot.retire_after = Some(slot.last_used);
        Ok(())
    }

    fn complete_through(&mut self, submission: SubmissionId) -> u64 {
        let mut freed = 0;
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot.retire_after.is_some_and(|retire| retire <= submission) {
                slot.value = None;
                slot.retire_after = None;
                slot.generation = slot.generation.wrapping_add(1).max(1);
                self.free
                    .push(u32::try_from(index).expect("resource slot index overflow"));
                freed += 1;
            }
        }
        freed
    }

    fn live_count(&self) -> u64 {
        self.slots
            .iter()
            .filter(|slot| slot.value.is_some())
            .count() as u64
    }

    fn pending_count(&self) -> u64 {
        self.slots
            .iter()
            .filter(|slot| slot.retire_after.is_some())
            .count() as u64
    }

    fn stale<R>(&self, handle: Handle<R>) -> RhiError {
        RhiError::StaleHandle {
            kind: self.kind,
            index: handle.index,
            generation: handle.generation,
        }
    }
}

/// Deterministic in-memory RHI used by tests, servers, and renderer recovery.
pub struct NullRhi {
    buffers: Registry<BufferDescriptor>,
    textures: Registry<TextureDescriptor>,
    samplers: Registry<SamplerDescriptor>,
    shaders: Registry<ShaderDescriptor>,
    pipelines: Registry<PipelineDescriptor>,
    next_submission: u64,
    completed: SubmissionId,
    submissions: VecDeque<CommandList>,
    diagnostics: RhiDiagnostics,
    lost_reason: Option<String>,
}

impl NullRhi {
    /// Creates an empty deterministic device.
    pub fn new() -> Self {
        Self {
            buffers: Registry::new("buffer"),
            textures: Registry::new("texture"),
            samplers: Registry::new("sampler"),
            shaders: Registry::new("shader"),
            pipelines: Registry::new("pipeline"),
            next_submission: 1,
            completed: SubmissionId(0),
            submissions: VecDeque::new(),
            diagnostics: RhiDiagnostics::default(),
            lost_reason: None,
        }
    }

    /// Injects a recoverable device-loss condition for failure-path tests.
    pub fn inject_device_loss(&mut self, reason: impl Into<String>) {
        self.lost_reason = Some(reason.into());
        self.diagnostics.device_losses = self.diagnostics.device_losses.saturating_add(1);
    }

    /// Recreates backend-owned state while retaining only diagnostics.
    pub fn recover(&mut self) {
        let losses = self.diagnostics.device_losses;
        *self = Self::new();
        self.diagnostics.device_losses = losses;
    }

    /// Accepted command lists, in deterministic submission order.
    pub fn submitted_lists(&self) -> &VecDeque<CommandList> {
        &self.submissions
    }

    /// Records a CPU scope label.
    pub fn record_cpu_scope(&mut self, label: impl Into<String>) {
        self.diagnostics.cpu_scopes.push(label.into());
    }

    /// Records a backend GPU scope label.
    pub fn record_gpu_scope(&mut self, label: impl Into<String>) {
        self.diagnostics.gpu_scopes.push(label.into());
    }

    fn ensure_available(&self) -> Result<(), RhiError> {
        if let Some(reason) = &self.lost_reason {
            return Err(RhiError::DeviceLost(reason.clone()));
        }
        Ok(())
    }

    fn refresh_counts(&mut self) {
        self.diagnostics.live_resources = self.buffers.live_count()
            + self.textures.live_count()
            + self.samplers.live_count()
            + self.shaders.live_count()
            + self.pipelines.live_count();
        self.diagnostics.pending_retirements = self.buffers.pending_count()
            + self.textures.pending_count()
            + self.samplers.pending_count()
            + self.shaders.pending_count()
            + self.pipelines.pending_count();
    }

    fn note_stale<T>(&mut self, result: Result<T, RhiError>) -> Result<T, RhiError> {
        if matches!(result, Err(RhiError::StaleHandle { .. })) {
            self.diagnostics.stale_handle_errors =
                self.diagnostics.stale_handle_errors.saturating_add(1);
        }
        result
    }
}

impl Default for NullRhi {
    fn default() -> Self {
        Self::new()
    }
}

impl RhiDevice for NullRhi {
    fn create_buffer(
        &mut self,
        descriptor: BufferDescriptor,
        initial_data: &[u8],
    ) -> Result<BufferHandle, RhiError> {
        self.ensure_available()?;
        descriptor.validate()?;
        if initial_data.len() as u64 > descriptor.size {
            return Err(RhiError::InvalidDescriptor(
                "initial buffer data exceeds declared size".to_owned(),
            ));
        }
        let handle = self.buffers.insert(descriptor);
        self.refresh_counts();
        Ok(handle)
    }

    fn create_texture(
        &mut self,
        descriptor: TextureDescriptor,
        _initial_data: &[u8],
    ) -> Result<TextureHandle, RhiError> {
        self.ensure_available()?;
        descriptor.validate()?;
        let handle = self.textures.insert(descriptor);
        self.refresh_counts();
        Ok(handle)
    }

    fn create_sampler(&mut self, descriptor: SamplerDescriptor) -> Result<SamplerHandle, RhiError> {
        self.ensure_available()?;
        let handle = self.samplers.insert(descriptor);
        self.refresh_counts();
        Ok(handle)
    }

    fn create_shader(&mut self, descriptor: ShaderDescriptor) -> Result<ShaderHandle, RhiError> {
        self.ensure_available()?;
        if descriptor.wgsl.trim().is_empty() || descriptor.reflection.entry_points.is_empty() {
            return Err(RhiError::InvalidDescriptor(
                "shader source and reflected entry points are required".to_owned(),
            ));
        }
        let handle = self.shaders.insert(descriptor);
        self.refresh_counts();
        Ok(handle)
    }

    fn create_pipeline(
        &mut self,
        descriptor: PipelineDescriptor,
    ) -> Result<PipelineHandle, RhiError> {
        self.ensure_available()?;
        let shader_check = self.shaders.slot(descriptor.shader).map(|_| ());
        self.note_stale(shader_check)?;
        if descriptor.vertex_entry.trim().is_empty() || descriptor.fragment_entry.trim().is_empty()
        {
            return Err(RhiError::InvalidDescriptor(
                "graphics pipelines require vertex and fragment entry points".to_owned(),
            ));
        }
        let handle = self.pipelines.insert(descriptor);
        self.refresh_counts();
        Ok(handle)
    }

    fn retire_buffer(&mut self, handle: BufferHandle) -> Result<(), RhiError> {
        let result = self.buffers.retire(handle);
        self.note_stale(result)?;
        self.refresh_counts();
        Ok(())
    }

    fn retire_texture(&mut self, handle: TextureHandle) -> Result<(), RhiError> {
        let result = self.textures.retire(handle);
        self.note_stale(result)?;
        self.refresh_counts();
        Ok(())
    }

    fn retire_sampler(&mut self, handle: SamplerHandle) -> Result<(), RhiError> {
        let result = self.samplers.retire(handle);
        self.note_stale(result)?;
        self.refresh_counts();
        Ok(())
    }

    fn retire_shader(&mut self, handle: ShaderHandle) -> Result<(), RhiError> {
        let result = self.shaders.retire(handle);
        self.note_stale(result)?;
        self.refresh_counts();
        Ok(())
    }

    fn retire_pipeline(&mut self, handle: PipelineHandle) -> Result<(), RhiError> {
        let result = self.pipelines.retire(handle);
        self.note_stale(result)?;
        self.refresh_counts();
        Ok(())
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the null RHI validates the complete command grammar in one explicit state machine"
    )]
    fn submit(&mut self, commands: CommandList) -> Result<SubmissionId, RhiError> {
        self.ensure_available()?;
        let submission = SubmissionId(self.next_submission);
        let mut in_pass = false;
        let mut pipeline_bound = false;
        let mut index_bound = false;
        let mut draw_calls = 0_u64;

        for command in &commands.commands {
            let result = match command {
                RenderCommand::BeginPass { color, depth, .. } => {
                    if in_pass {
                        Err(RhiError::InvalidCommand("nested render pass".to_owned()))
                    } else {
                        self.textures.mark_used(*color, submission)?;
                        if let Some(depth) = depth {
                            self.textures.mark_used(*depth, submission)?;
                        }
                        in_pass = true;
                        pipeline_bound = false;
                        index_bound = false;
                        Ok(())
                    }
                }
                RenderCommand::SetPipeline(handle) => {
                    if in_pass {
                        self.pipelines.mark_used(*handle, submission)?;
                        pipeline_bound = true;
                        Ok(())
                    } else {
                        Err(RhiError::InvalidCommand(
                            "pipeline selected outside render pass".to_owned(),
                        ))
                    }
                }
                RenderCommand::SetVertexBuffer(handle) => {
                    self.buffers.mark_used(*handle, submission)
                }
                RenderCommand::SetIndexBuffer(handle, _) => {
                    self.buffers.mark_used(*handle, submission)?;
                    index_bound = true;
                    Ok(())
                }
                RenderCommand::Draw {
                    vertices,
                    instances,
                } => {
                    if !in_pass || !pipeline_bound || *vertices == 0 || *instances == 0 {
                        Err(RhiError::InvalidCommand(
                            "draw requires an active pass, pipeline, vertices, and instances"
                                .to_owned(),
                        ))
                    } else {
                        draw_calls = draw_calls.saturating_add(1);
                        Ok(())
                    }
                }
                RenderCommand::DrawIndexed { indices, instances } => {
                    if !in_pass
                        || !pipeline_bound
                        || !index_bound
                        || *indices == 0
                        || *instances == 0
                    {
                        Err(RhiError::InvalidCommand(
                            "indexed draw requires an active pass, pipeline, index buffer, indices, and instances"
                                .to_owned(),
                        ))
                    } else {
                        draw_calls = draw_calls.saturating_add(1);
                        Ok(())
                    }
                }
                RenderCommand::EndPass => {
                    if in_pass {
                        in_pass = false;
                        Ok(())
                    } else {
                        Err(RhiError::InvalidCommand(
                            "render pass ended while no pass was active".to_owned(),
                        ))
                    }
                }
                RenderCommand::CopyTextureToBuffer {
                    source,
                    destination,
                } => {
                    if in_pass {
                        Err(RhiError::InvalidCommand(
                            "texture copy must be outside a render pass".to_owned(),
                        ))
                    } else {
                        self.textures.mark_used(*source, submission)?;
                        self.buffers.mark_used(*destination, submission)
                    }
                }
            };
            self.note_stale(result)?;
        }
        if in_pass {
            return Err(RhiError::InvalidCommand(
                "render pass was not ended".to_owned(),
            ));
        }

        self.next_submission = self.next_submission.saturating_add(1);
        self.diagnostics.submissions = self.diagnostics.submissions.saturating_add(1);
        self.diagnostics.draw_calls = self.diagnostics.draw_calls.saturating_add(draw_calls);
        self.submissions.push_back(commands);
        Ok(submission)
    }

    fn complete_through(&mut self, submission: SubmissionId) {
        if submission <= self.completed {
            return;
        }
        self.completed = submission;
        let _ = self.buffers.complete_through(submission);
        let _ = self.textures.complete_through(submission);
        let _ = self.samplers.complete_through(submission);
        let _ = self.shaders.complete_through(submission);
        let _ = self.pipelines.complete_through(submission);
        self.refresh_counts();
    }

    fn diagnostics(&self) -> RhiDiagnostics {
        self.diagnostics.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color_descriptor() -> TextureDescriptor {
        TextureDescriptor {
            label: "color".to_owned(),
            width: 64,
            height: 64,
            depth_or_layers: 1,
            mip_levels: 1,
            sample_count: 1,
            format: TextureFormat::Rgba8Srgb,
            usage: TextureUsage::COLOR_ATTACHMENT.union(TextureUsage::COPY_SOURCE),
        }
    }

    fn shader_descriptor() -> ShaderDescriptor {
        ShaderDescriptor {
            label: "fixture".to_owned(),
            wgsl: "@vertex fn vs() -> @builtin(position) vec4f { return vec4f(); }".to_owned(),
            reflection: ShaderReflection {
                entry_points: vec![("vs".to_owned(), ShaderStage::Vertex)],
                ..ShaderReflection::default()
            },
        }
    }

    #[test]
    fn stale_handles_fail_after_submission_retirement() {
        let mut rhi = NullRhi::new();
        let buffer = rhi
            .create_buffer(
                BufferDescriptor {
                    label: "vertices".to_owned(),
                    size: 12,
                    usage: BufferUsage::VERTEX,
                },
                &[0; 12],
            )
            .unwrap();
        rhi.retire_buffer(buffer).unwrap();
        rhi.complete_through(SubmissionId(0));
        assert!(matches!(
            rhi.retire_buffer(buffer),
            Err(RhiError::StaleHandle { kind: "buffer", .. })
        ));
        assert_eq!(rhi.diagnostics().stale_handle_errors, 1);
    }

    #[test]
    fn resources_are_retired_only_after_last_submission() {
        let mut rhi = NullRhi::new();
        let color = rhi.create_texture(color_descriptor(), &[]).unwrap();
        let shader = rhi.create_shader(shader_descriptor()).unwrap();
        let pipeline = rhi
            .create_pipeline(PipelineDescriptor {
                label: "pipeline".to_owned(),
                shader,
                vertex_entry: "vs".to_owned(),
                fragment_entry: "fs".to_owned(),
                topology: PrimitiveTopology::TriangleList,
                color_format: TextureFormat::Rgba8Srgb,
                depth_format: None,
            })
            .unwrap();
        let submission = rhi
            .submit(CommandList {
                label: "triangle".to_owned(),
                commands: vec![
                    RenderCommand::BeginPass {
                        label: "main".to_owned(),
                        color,
                        depth: None,
                        clear: ClearColor {
                            red: 0.0,
                            green: 0.0,
                            blue: 0.0,
                            alpha: 1.0,
                        },
                        clear_depth: 1.0,
                    },
                    RenderCommand::SetPipeline(pipeline),
                    RenderCommand::Draw {
                        vertices: 3,
                        instances: 1,
                    },
                    RenderCommand::EndPass,
                ],
            })
            .unwrap();
        rhi.retire_texture(color).unwrap();
        assert_eq!(rhi.diagnostics().pending_retirements, 1);
        rhi.complete_through(SubmissionId(submission.0 - 1));
        assert_eq!(rhi.diagnostics().pending_retirements, 1);
        rhi.complete_through(submission);
        assert_eq!(rhi.diagnostics().pending_retirements, 0);
    }

    #[test]
    fn command_validation_rejects_draw_without_pass() {
        let mut rhi = NullRhi::new();
        assert!(matches!(
            rhi.submit(CommandList {
                label: "bad".to_owned(),
                commands: vec![RenderCommand::Draw {
                    vertices: 3,
                    instances: 1,
                }],
            }),
            Err(RhiError::InvalidCommand(_))
        ));
    }

    #[test]
    fn surface_lifecycle_handles_minimize_dpi_and_loss() {
        let mut surface = SurfaceLifecycle::new();
        assert_eq!(surface.state(), SurfaceState::Suspended);
        surface.configure(800, 600, 1.5);
        assert!(matches!(
            surface.state(),
            SurfaceState::Active {
                width: 800,
                height: 600,
                scale_factor: 1.5
            }
        ));
        surface.configure(0, 600, 1.5);
        assert_eq!(surface.state(), SurfaceState::Suspended);
        surface.mark_lost();
        assert_eq!(surface.state(), SurfaceState::Lost);
        surface.configure(1024, 768, 2.0);
        assert!(matches!(surface.state(), SurfaceState::Active { .. }));
    }

    #[test]
    fn device_loss_is_recoverable() {
        let mut rhi = NullRhi::new();
        rhi.inject_device_loss("injected");
        assert!(matches!(
            rhi.create_texture(color_descriptor(), &[]),
            Err(RhiError::DeviceLost(_))
        ));
        rhi.recover();
        assert!(rhi.create_texture(color_descriptor(), &[]).is_ok());
        assert_eq!(rhi.diagnostics().device_losses, 1);
    }
}
