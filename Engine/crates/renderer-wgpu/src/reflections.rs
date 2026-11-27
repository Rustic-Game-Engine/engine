//! Bounded retained local environment maps, capture scheduling and overlap blending.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "probe dimensions and shader sample budgets are validated before allocation"
)]
use super::{HashMap, RendererError, ViewportScene, surface};
use crate::probe_capture::ProbeFilter;
use engine_world::{
    AssetId, EntityId,
    effects::{EffectFrame, ProbeRefresh, RenderReflectionProbe, VolumeShape},
};
use glam::{Vec3, Vec3Swizzles};
use std::{
    collections::{BTreeMap, HashSet},
    hash::{Hash, Hasher},
    path::Path,
    time::SystemTime,
};

const MAX_RETAINED_PROBES: usize = 16;
const MAX_RETAINED_PROBE_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceStamp {
    path: String,
    fingerprint: u64,
}

pub(super) struct ProbeTexture {
    pub _texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub mip_count: u32,
    /// Real six-face linear-HDR source, retained for readback and rebaking.
    pub cube: wgpu::Texture,
    pub resolution: u32,
    pub quality: engine_world::effects::QualityLevel,
}
#[derive(Clone, Debug)]
pub(super) struct ProbeBlend {
    pub probe: RenderReflectionProbe,
    pub weight: f32,
    pub mip_count: f32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct CaptureStamp {
    time: f32,
    revision: u64,
    resolution: u32,
    position: [f32; 3],
    layers: u32,
    quality: engine_world::effects::QualityLevel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TextureKey {
    id: AssetId,
    generation: u64,
}

pub(super) struct ReflectionCache {
    pub layout: wgpu::BindGroupLayout,
    maps: HashMap<AssetId, (ProbeTexture, u64)>,
    captured: HashMap<EntityId, CaptureStamp>,
    sources: HashMap<AssetId, SourceStamp>,
    capture_counts: HashMap<EntityId, u64>,
    generation: u64,
    sampler: wgpu::Sampler,
    neutral: wgpu::TextureView,
    fallback: wgpu::BindGroup,
    groups: HashMap<[Option<TextureKey>; 4], wgpu::BindGroup>,
    mesh_keys: HashMap<u64, [Option<TextureKey>; 4]>,
    blends: HashMap<u64, Vec<ProbeBlend>>,
    pub filter: ProbeFilter,
    pub readbacks: HashMap<EntityId, super::probe_capture::PendingProbeReadback>,
}
impl ReflectionCache {
    pub fn new(device: &wgpu::Device) -> Self {
        let mut entries = Vec::new();
        for binding in 0..4 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 4,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("local-probe-blend-layout"),
            entries: &entries,
        });
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("neutral-probe"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let neutral = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("probe-roughness-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let fallback = blend_group(
            device,
            &layout,
            &sampler,
            [&neutral, &neutral, &neutral, &neutral],
        );
        Self {
            layout,
            maps: HashMap::new(),
            captured: HashMap::new(),
            sources: HashMap::new(),
            capture_counts: HashMap::new(),
            generation: 0,
            sampler,
            neutral,
            fallback,
            groups: HashMap::new(),
            mesh_keys: HashMap::new(),
            blends: HashMap::new(),
            filter: ProbeFilter::new(device),
            readbacks: HashMap::new(),
        }
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        root: &Path,
        frame: &EffectFrame,
        paths: &HashMap<AssetId, String>,
    ) -> Result<(), RendererError> {
        let active: HashSet<_> = frame.probes.iter().map(|p| p.cubemap).collect();
        if active.len() > MAX_RETAINED_PROBES {
            return Err(RendererError::InvalidMesh(
                "at most 16 local reflection maps may be active at once".into(),
            ));
        }
        self.maps.retain(|id, _| active.contains(id));
        self.sources.retain(|id, _| active.contains(id));
        let entities: HashSet<_> = frame.probes.iter().map(|p| p.entity).collect();
        self.captured.retain(|id, _| entities.contains(id));
        self.capture_counts.retain(|id, _| entities.contains(id));
        self.readbacks.retain(|id, _| entities.contains(id));
        // Shared baked sources use one upload at the strongest requested settings.
        // Recompute the requested maximum so lowering the last high-quality user
        // releases an oversized map instead of retaining its old allocation.
        let mut baked = BTreeMap::new();
        for probe in &frame.probes {
            if probe.refresh != ProbeRefresh::Baked || probe.refresh_revision > 0 {
                continue;
            }
            let entry = baked
                .entry(probe.cubemap)
                .or_insert((probe.resolution, probe.quality));
            entry.0 = entry.0.max(probe.resolution);
            if quality_rank(probe.quality) > quality_rank(entry.1) {
                entry.1 = probe.quality;
            }
        }
        for (id, (requested_resolution, quality)) in baked {
            let resolution = requested_resolution.clamp(16, 2048);
            let Some(path) = paths.get(&id) else {
                return Err(RendererError::InvalidMesh(format!(
                    "reflection cubemap asset {id} is unavailable; assign a valid image or .cubemap source with a stable .rmeta sidecar"
                )));
            };
            let source_stamp = source_stamp(root, path)?;
            if self
                .maps
                .get(&id)
                .is_some_and(|(map, _)| map.resolution == resolution && map.quality == quality)
                && self.sources.get(&id) == Some(&source_stamp)
            {
                continue;
            }
            self.check_budget(id, resolution)?;
            let texture = if path.to_ascii_lowercase().ends_with(".cubemap") {
                let canonical = root
                    .canonicalize()
                    .map_err(|e| RendererError::InvalidMesh(e.to_string()))?;
                let source = canonical
                    .join(path)
                    .canonicalize()
                    .map_err(|e| RendererError::InvalidMesh(e.to_string()))?;
                if !source.starts_with(&canonical) {
                    return Err(RendererError::InvalidMesh(
                        "cubemap path leaves project".into(),
                    ));
                }
                let bytes =
                    std::fs::read(source).map_err(|e| RendererError::InvalidMesh(e.to_string()))?;
                let descriptor = engine_assets::CubemapAsset::from_bytes(&bytes)
                    .map_err(|e| RendererError::InvalidMesh(e.to_string()))?;
                let faces = descriptor
                    .decode_faces(&canonical)
                    .map_err(|e| RendererError::InvalidMesh(e.to_string()))?;
                self.filter
                    .upload_faces(device, queue, &faces, resolution, quality)?
            } else {
                let mut image = surface::load(root, path)?;
                if image.width() != image.height() * 2 {
                    return Err(RendererError::InvalidMesh(format!(
                        "probe {path} requires a 2:1 panorama or .cubemap descriptor"
                    )));
                }
                if !path.to_ascii_lowercase().ends_with(".hdr") {
                    for pixel in image.pixels_mut() {
                        for channel in &mut pixel.0[..3] {
                            *channel = srgb_linear(*channel);
                        }
                    }
                }
                self.filter
                    .upload_panorama(device, queue, &image, resolution, quality)?
            };
            self.install_map(id, texture);
            self.sources.insert(id, source_stamp);
        }
        Ok(())
    }
    fn install_map(&mut self, id: AssetId, texture: ProbeTexture) {
        self.generation = self.generation.wrapping_add(1);
        self.maps.insert(id, (texture, self.generation));
    }
    pub fn capture_requests(&self, frame: &EffectFrame) -> Vec<RenderReflectionProbe> {
        let mut requests: Vec<_> = frame
            .probes
            .iter()
            .filter(|probe| {
                capture_due(
                    probe,
                    self.captured.get(&probe.entity),
                    self.maps.contains_key(&probe.cubemap),
                )
            })
            .cloned()
            .collect();
        requests.sort_by(|a, b| {
            self.capture_count(a.entity)
                .cmp(&self.capture_count(b.entity))
                .then_with(|| b.priority.cmp(&a.priority))
                .then_with(|| a.entity.cmp(&b.entity))
        });
        requests
    }
    pub fn install_capture(&mut self, probe: &RenderReflectionProbe, texture: ProbeTexture) {
        self.sources.remove(&probe.cubemap);
        self.install_map(probe.cubemap, texture);
        self.captured.insert(probe.entity, stamp(probe));
        *self.capture_counts.entry(probe.entity).or_default() += 1;
    }
    pub fn capture_count(&self, id: EntityId) -> u64 {
        self.capture_counts.get(&id).copied().unwrap_or(0)
    }
    pub fn captured_texture(&self, id: AssetId) -> Option<&ProbeTexture> {
        self.maps.get(&id).map(|(texture, _)| texture)
    }
    pub fn prepare_blends(
        &mut self,
        device: &wgpu::Device,
        scene: &ViewportScene,
        frame: &EffectFrame,
    ) {
        self.mesh_keys.clear();
        self.blends.clear();
        let mut used = HashSet::new();
        for mesh in &scene.meshes {
            let center = glam::Mat4::from_cols_array(&mesh.model).w_axis.truncate();
            let probes = weighted_probes(
                &frame.probes,
                center,
                mesh.textures.visibility_layers,
                |id| self.has(id),
            );
            let values: Vec<_> = probes
                .into_iter()
                .map(|(probe, weight)| ProbeBlend {
                    probe: probe.clone(),
                    weight,
                    mip_count: self.mip_count(probe.cubemap) as f32,
                })
                .collect();
            let key = std::array::from_fn(|index| {
                values.get(index).and_then(|p| {
                    self.maps
                        .get(&p.probe.cubemap)
                        .map(|(_, generation)| TextureKey {
                            id: p.probe.cubemap,
                            generation: *generation,
                        })
                })
            });
            if !self.groups.contains_key(&key) {
                let views = std::array::from_fn(|index| {
                    key[index]
                        .and_then(|k| self.maps.get(&k.id))
                        .map_or(&self.neutral, |(texture, _)| &texture.view)
                });
                self.groups
                    .insert(key, blend_group(device, &self.layout, &self.sampler, views));
            }
            used.insert(key);
            self.mesh_keys.insert(mesh.instance_key, key);
            self.blends.insert(mesh.instance_key, values);
        }
        self.groups.retain(|key, _| used.contains(key));
    }
    pub fn blends_for_mesh(&self, key: u64) -> &[ProbeBlend] {
        self.blends.get(&key).map_or(&[], Vec::as_slice)
    }
    pub fn get_for_mesh(&self, key: u64) -> &wgpu::BindGroup {
        self.mesh_keys
            .get(&key)
            .and_then(|key| self.groups.get(key))
            .unwrap_or(&self.fallback)
    }
    pub fn resource_bytes(&self) -> u64 {
        self.maps
            .values()
            .map(|(map, _)| probe_bytes(map.resolution))
            .sum()
    }
    pub fn check_budget(&self, id: AssetId, resolution: u32) -> Result<(), RendererError> {
        let previous = self
            .maps
            .get(&id)
            .map_or(0, |(map, _)| probe_bytes(map.resolution));
        let count = self.maps.len() + usize::from(!self.maps.contains_key(&id));
        if count > MAX_RETAINED_PROBES
            || self.resource_bytes() - previous + probe_bytes(resolution) > MAX_RETAINED_PROBE_BYTES
        {
            return Err(RendererError::InvalidMesh("reflection probes exceed the retained budget (16 maps or 1 GiB); reduce capture resolution or active probe count".into()));
        }
        Ok(())
    }
    pub fn clear(&mut self) {
        self.maps.clear();
        self.readbacks.clear();
        self.captured.clear();
        self.sources.clear();
        self.capture_counts.clear();
        self.groups.clear();
        self.mesh_keys.clear();
        self.blends.clear();
    }
    pub fn has(&self, id: AssetId) -> bool {
        self.maps.contains_key(&id)
    }
    pub fn mip_count(&self, id: AssetId) -> u32 {
        self.maps
            .get(&id)
            .map_or(1, |(texture, _)| texture.mip_count)
    }
    /// Capture passes disable recursive local reflections; this group always binds neutral sources.
    pub fn get(&self, _id: Option<AssetId>) -> &wgpu::BindGroup {
        &self.fallback
    }
}
fn stamp(probe: &RenderReflectionProbe) -> CaptureStamp {
    CaptureStamp {
        time: probe.time,
        revision: probe.refresh_revision,
        resolution: probe.resolution,
        position: probe.capture_position.to_array(),
        layers: probe.visibility_layers,
        quality: probe.quality,
    }
}
fn quality_rank(quality: engine_world::effects::QualityLevel) -> u8 {
    match quality {
        engine_world::effects::QualityLevel::Low => 0,
        engine_world::effects::QualityLevel::Medium => 1,
        engine_world::effects::QualityLevel::High => 2,
    }
}
fn probe_bytes(resolution: u32) -> u64 {
    let mut bytes = u64::from(resolution).pow(2) * 6 * 8;
    let (mut width, mut height) = (resolution * 4, resolution * 2);
    loop {
        bytes += u64::from(width) * u64::from(height) * 8;
        if width == 1 && height == 1 {
            break;
        }
        width = (width / 2).max(1);
        height = (height / 2).max(1);
    }
    bytes
}

fn source_stamp(root: &Path, path: &str) -> Result<SourceStamp, RendererError> {
    engine_assets::validate_asset_relative_path(Path::new(path))
        .map_err(|error| RendererError::InvalidMesh(error.to_string()))?;
    let root = root
        .canonicalize()
        .map_err(|error| RendererError::InvalidMesh(error.to_string()))?;
    let checked = |relative: &Path, hash: &mut std::collections::hash_map::DefaultHasher| {
        let file = root
            .join(relative)
            .canonicalize()
            .map_err(|error| RendererError::InvalidMesh(error.to_string()))?;
        if !file.starts_with(&root) {
            return Err(RendererError::InvalidMesh(
                "probe source leaves project".into(),
            ));
        }
        let metadata = std::fs::metadata(&file)
            .map_err(|error| RendererError::InvalidMesh(error.to_string()))?;
        if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
            return Err(RendererError::InvalidMesh(
                "probe image must be a regular file of at most 64 MiB".into(),
            ));
        }
        relative.hash(hash);
        metadata.len().hash(hash);
        metadata
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH)
            .hash(hash);
        Ok(file)
    };
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let source = checked(Path::new(path), &mut hash)?;
    if path.to_ascii_lowercase().ends_with(".cubemap") {
        let size = std::fs::metadata(&source)
            .map_err(|error| RendererError::InvalidMesh(error.to_string()))?
            .len();
        if size > 64 * 1024 {
            return Err(RendererError::InvalidMesh(
                "cubemap descriptor exceeds 64 KiB".into(),
            ));
        }
        let bytes =
            std::fs::read(source).map_err(|error| RendererError::InvalidMesh(error.to_string()))?;
        let descriptor = engine_assets::CubemapAsset::from_bytes(&bytes)
            .map_err(|error| RendererError::InvalidMesh(error.to_string()))?;
        for face in descriptor.faces {
            checked(&face, &mut hash)?;
        }
    }
    Ok(SourceStamp {
        path: path.into(),
        fingerprint: hash.finish(),
    })
}
fn capture_due(
    probe: &RenderReflectionProbe,
    last: Option<&CaptureStamp>,
    has_texture: bool,
) -> bool {
    if probe.refresh == ProbeRefresh::Baked && probe.refresh_revision == 0 {
        return false;
    }
    let Some(last) = last else {
        return true;
    };
    let now = stamp(probe);
    !has_texture
        || last.revision != now.revision
        || last.resolution != now.resolution
        || last.position.map(f32::to_bits) != now.position.map(f32::to_bits)
        || last.layers != now.layers
        || last.quality != now.quality
        || (probe.refresh == ProbeRefresh::Interval
            && (probe.time - last.time >= probe.update_interval || probe.time < last.time))
}
fn srgb_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
fn blend_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    views: [&wgpu::TextureView; 4],
) -> wgpu::BindGroup {
    let mut entries: Vec<_> = views
        .iter()
        .enumerate()
        .map(|(index, view)| wgpu::BindGroupEntry {
            binding: u32::try_from(index).unwrap_or(0),
            resource: wgpu::BindingResource::TextureView(view),
        })
        .collect();
    entries.push(wgpu::BindGroupEntry {
        binding: 4,
        resource: wgpu::BindingResource::Sampler(sampler),
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("weighted-local-probes"),
        layout,
        entries: &entries,
    })
}
/// Same-priority probes normalize together. Each priority group covers its maximum
/// influence weight; lower priorities fill the remaining weight. The strongest four
/// sources are retained, then normalized without increasing the total influence.
pub(super) fn weighted_probes(
    probes: &[RenderReflectionProbe],
    position: Vec3,
    layers: u32,
    available: impl Fn(AssetId) -> bool,
) -> Vec<(&RenderReflectionProbe, f32)> {
    let mut candidates: Vec<_> = probes
        .iter()
        .filter(|p| p.receiver_layers & layers != 0 && available(p.cubemap))
        .filter_map(|p| {
            let local = p.inverse_transform.transform_point3(position);
            let half = (Vec3::from_array(p.dimensions).abs() * 0.5).max(Vec3::splat(0.0001));
            let closest = match p.shape {
                VolumeShape::Sphere => {
                    let normalized = local / half;
                    normalized / normalized.length().max(1.0) * half
                }
                VolumeShape::Cylinder => {
                    let normalized = local.xz() / half.xz();
                    let radial = normalized / normalized.length().max(1.0) * half.xz();
                    Vec3::new(radial.x, local.y.clamp(-half.y, half.y), radial.y)
                }
                _ => local.clamp(-half, half),
            };
            let outside = p
                .inverse_transform
                .inverse()
                .transform_vector3(local - closest)
                .length();
            let weight = (1.0 - outside / p.blend_distance.max(0.0001)).clamp(0.0, 1.0);
            (weight > 0.0).then_some((p, weight))
        })
        .collect();
    candidates.sort_by(|(a, aw), (b, bw)| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| bw.total_cmp(aw))
            .then_with(|| a.entity.cmp(&b.entity))
    });
    let mut output = Vec::new();
    let mut remaining = 1.0;
    let mut index = 0;
    while index < candidates.len() && remaining > 0.0001 {
        let priority = candidates[index].0.priority;
        let end = index
            + candidates[index..]
                .iter()
                .take_while(|(p, _)| p.priority == priority)
                .count();
        let group = &candidates[index..end];
        let sum: f32 = group.iter().map(|(_, weight)| *weight).sum();
        let coverage = group
            .iter()
            .map(|(_, weight)| *weight)
            .fold(0.0_f32, f32::max);
        for (probe, weight) in group {
            output.push((*probe, remaining * coverage * weight / sum));
        }
        remaining *= 1.0 - coverage;
        index = end;
    }
    let total: f32 = output.iter().map(|(_, weight)| *weight).sum();
    output.sort_by(|(_, a), (_, b)| b.total_cmp(a));
    output.truncate(4);
    let retained: f32 = output.iter().map(|(_, weight)| *weight).sum();
    if retained > 0.0 {
        for (_, weight) in &mut output {
            *weight *= total / retained;
        }
    }
    output
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use engine_world::effects::QualityLevel;
    pub fn probe() -> RenderReflectionProbe {
        RenderReflectionProbe {
            entity: EntityId::new(),
            position: Vec3::ZERO,
            capture_position: Vec3::ZERO,
            inverse_transform: glam::Mat4::IDENTITY,
            shape: VolumeShape::Box,
            dimensions: [10.0; 3],
            cubemap: AssetId::new(),
            blend_distance: 2.0,
            priority: 0,
            resolution: 16,
            refresh: ProbeRefresh::OnEnable,
            update_interval: 0.5,
            parallax_correction: true,
            visibility_layers: u32::MAX,
            receiver_layers: u32::MAX,
            quality: QualityLevel::Low,
            refresh_revision: 0,
            time: 0.0,
        }
    }
    #[test]
    fn overlap_normalizes_equal_priorities_and_lower_priorities_fill_boundary_weight() {
        let first = probe();
        let mut second = probe();
        second.dimensions = [100.0; 3];
        let mut probes = [first, second];
        let weights = weighted_probes(&probes, Vec3::ZERO, u32::MAX, |_| true);
        assert_eq!(weights.len(), 2);
        assert!(
            weights
                .iter()
                .all(|(_, weight)| (*weight - 0.5).abs() < 0.0001)
        );
        probes[0].priority = 2;
        let weights = weighted_probes(&probes, Vec3::new(6.0, 0.0, 0.0), u32::MAX, |_| true);
        assert_eq!(weights.len(), 2);
        assert!(
            weights
                .iter()
                .all(|(_, weight)| (*weight - 0.5).abs() < 0.0001)
        );
        assert!(weighted_probes(&probes, Vec3::ZERO, 0, |_| true).is_empty());
        assert!(weighted_probes(&probes, Vec3::ZERO, u32::MAX, |_| false).is_empty());
    }
    #[test]
    fn influence_shapes_honor_all_dimensions_and_parent_scale_in_world_space() {
        let mut p = probe();
        p.inverse_transform = glam::Mat4::from_scale(Vec3::splat(2.0)).inverse();
        assert!(
            weighted_probes(&[p.clone()], Vec3::new(12.1, 0.0, 0.0), u32::MAX, |_| true).is_empty()
        );
        p.inverse_transform = glam::Mat4::IDENTITY;
        p.shape = VolumeShape::Cylinder;
        p.dimensions = [2.0, 2.0, 10.0];
        let probes = [p.clone()];
        let values = weighted_probes(&probes, Vec3::new(0.0, 0.0, 4.0), u32::MAX, |_| true);
        assert_eq!(values.len(), 1);
        assert!((values[0].1 - 1.0).abs() < f32::EPSILON);
        p.shape = VolumeShape::Sphere;
        assert!(weighted_probes(&[p], Vec3::new(4.0, 4.0, 4.0), u32::MAX, |_| true).is_empty());
    }
    #[test]
    fn captures_follow_revision_interval_changes_and_restart_without_per_frame_on_enable() {
        let mut p = probe();
        assert!(capture_due(&p, None, false));
        let first = stamp(&p);
        p.time = 2.0;
        assert!(!capture_due(&p, Some(&first), true));
        p.refresh_revision = 1;
        assert!(capture_due(&p, Some(&first), true));
        let refreshed = stamp(&p);
        p.refresh = ProbeRefresh::Interval;
        p.time = 2.4;
        assert!(!capture_due(&p, Some(&refreshed), true));
        p.time = 2.5;
        assert!(capture_due(&p, Some(&refreshed), true));
        p.time = 0.0;
        assert!(capture_due(&p, Some(&refreshed), true));
        p.refresh = ProbeRefresh::Baked;
        p.refresh_revision = 0;
        assert!(!capture_due(&p, None, false));
    }
    #[test]
    fn hot_reload_fingerprints_follow_all_cube_faces_and_validate_project_boundary() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("assets")).unwrap();
        let descriptor = engine_assets::CubemapAsset {
            version: 1,
            faces: std::array::from_fn(|face| format!("assets/{face}.png").into()),
        };
        for face in &descriptor.faces {
            std::fs::write(temp.path().join(face), b"initial").unwrap();
        }
        let path = "assets/probe.cubemap";
        std::fs::write(temp.path().join(path), descriptor.to_bytes().unwrap()).unwrap();
        let first = source_stamp(temp.path(), path).unwrap();
        std::fs::write(
            temp.path().join(&descriptor.faces[3]),
            b"edited-face-content",
        )
        .unwrap();
        assert_ne!(first, source_stamp(temp.path(), path).unwrap());
        assert!(source_stamp(temp.path(), "../escaped.hdr").is_err());
        assert!(source_stamp(temp.path(), "assets/missing.hdr").is_err());
    }
    #[test]
    fn supported_maximum_resolution_fits_one_retained_budget_but_two_do_not() {
        assert!(probe_bytes(2048) < MAX_RETAINED_PROBE_BYTES);
        assert!(probe_bytes(2048) * 2 > MAX_RETAINED_PROBE_BYTES);
        assert!(probe_bytes(32) > probe_bytes(16));
    }
}
