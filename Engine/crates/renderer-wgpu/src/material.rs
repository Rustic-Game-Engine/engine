//! Resolves reusable PBR sources at the scene boundary. GPU resources remain in
//! `SurfaceCache` and are shared by the resolved material recipe.
use super::{RendererError, ViewportScene, decals, effect_geometry_key};
use engine_assets::MaterialAsset;
use engine_world::{AssetId, SurfaceTextures, TextureMap, effects::EffectFrame};
use std::{collections::HashMap, path::Path, time::SystemTime};

type CachedBoundaryImage = (Option<SystemTime>, u64, std::sync::Arc<image::Rgba32FImage>);

#[derive(Default)]
pub(super) struct MaterialCache {
    sources: HashMap<AssetId, (SystemTime, u64, MaterialAsset)>,
    paths: HashMap<AssetId, String>,
    boundary: BoundaryParticles,
    boundary_images: HashMap<String, CachedBoundaryImage>,
}
impl MaterialCache {
    pub fn clear(&mut self) {
        self.sources.clear();
        self.paths.clear();
        self.boundary = BoundaryParticles::default();
        self.boundary_images.clear();
    }
    #[allow(
        clippy::too_many_lines,
        reason = "resolve material sources, stable resource references and per-instance overrides before GPU upload"
    )]
    pub fn resolve(
        &mut self,
        scene: &mut ViewportScene,
        frame: &EffectFrame,
        root: &Path,
        paths: &HashMap<AssetId, String>,
    ) -> Result<(), RendererError> {
        self.paths
            .extend(paths.iter().map(|(id, path)| (*id, path.clone())));
        let mut requested: Vec<_> = scene
            .meshes
            .iter()
            .filter_map(|mesh| mesh.textures.material)
            .collect();
        requested.extend(
            frame
                .geometry
                .iter()
                .filter_map(|geometry| geometry.material),
        );
        requested.extend(
            frame
                .decals
                .iter()
                .filter_map(|decal| decal.appearance.material),
        );
        requested.sort_unstable();
        requested.dedup();
        if requested.iter().any(|id| !self.paths.contains_key(id)) {
            self.paths.extend(source_paths(root));
        }
        for id in requested {
            let path = self.path(id)?;
            let absolute = safe_source(root, &path)?;
            let meta = std::fs::metadata(&absolute)
                .map_err(|error| fail(format!("material {path}: {error}")))?;
            let stamp = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            if self
                .sources
                .get(&id)
                .is_some_and(|(old, size, _)| *old == stamp && *size == meta.len())
            {
                continue;
            }
            let bytes = std::fs::read(&absolute).map_err(|error| fail(error.to_string()))?;
            let source = MaterialAsset::from_bytes(&bytes)
                .map_err(|error| fail(format!("material {path}: {error}")))?;
            if source
                .dependencies()
                .iter()
                .any(|id| !self.paths.contains_key(id))
            {
                self.paths.extend(source_paths(root));
            }
            self.sources.insert(id, (stamp, meta.len(), source));
        }
        for (index, geometry) in frame.geometry.iter().enumerate() {
            if let Some(mesh) = scene
                .meshes
                .iter_mut()
                .find(|mesh| mesh.instance_key == effect_geometry_key(geometry.entity, index))
            {
                mesh.textures.material = geometry.material;
                if let Some(id) = geometry.texture {
                    mesh.textures
                        .maps
                        .insert(TextureMap::BaseColor, self.resolve_texture(root, id)?);
                }
            }
        }
        for decal in &frame.decals {
            if let Some(mesh) = scene
                .meshes
                .iter_mut()
                .find(|mesh| mesh.instance_key == decals::key(decal.entity))
            {
                mesh.textures.material = decal.appearance.material;
                mesh.textures.roughness = Some(decal.roughness);
                mesh.textures.metallic = Some(decal.metallic);
                mesh.textures.pbr = true;
                mesh.textures.emissive_strength = decal.appearance.emission;
                if decal.appearance.material.is_none() && decal.emissive_map.is_none() {
                    mesh.textures.emissive_color = [0.0; 3];
                }
                if let Some(id) = decal.appearance.texture {
                    mesh.textures
                        .maps
                        .insert(TextureMap::BaseColor, self.resolve_texture(root, id)?);
                }
                if let Some(id) = decal.normal_map {
                    mesh.textures
                        .maps
                        .insert(TextureMap::Normal, self.resolve_texture(root, id)?);
                }
                if let Some(id) = decal.orm_map {
                    mesh.textures.orm_map = Some(self.resolve_texture(root, id)?);
                }
                if let Some(id) = decal.emissive_map {
                    mesh.textures
                        .maps
                        .insert(TextureMap::Emissive, self.resolve_texture(root, id)?);
                }
            }
        }
        let mut used = std::collections::HashSet::new();
        for mesh in &mut scene.meshes {
            let Some(id) = mesh.textures.material else {
                continue;
            };
            used.insert(id);
            let source = self
                .sources
                .get(&id)
                .ok_or_else(|| fail(format!("material asset {id} is unavailable")))?
                .2
                .clone();
            let mut resolved = self.surface(&source)?;
            overlay(&mut resolved, &mesh.textures);
            resolved.material = Some(id);
            mesh.textures = resolved;
        }
        for modifier in &frame.surfaces {
            let noise = modifier
                .noise_texture
                .map(|id| self.resolve_texture(root, id))
                .transpose()?;
            let mask = modifier
                .mask_texture
                .map(|id| self.resolve_texture(root, id))
                .transpose()?;
            let distortion = modifier
                .distortion_texture
                .map(|id| self.resolve_texture(root, id))
                .transpose()?;
            for mesh in &mut scene.meshes {
                if surface_matches(modifier, mesh) {
                    mesh.textures.effect_noise_map.clone_from(&noise);
                    mesh.textures.effect_mask_map.clone_from(&mask);
                    mesh.textures.effect_distortion_map.clone_from(&distortion);
                }
            }
        }
        self.sources.retain(|id, _| used.contains(id));
        Ok(())
    }
    fn path(&self, id: AssetId) -> Result<String, RendererError> {
        self.paths
            .get(&id)
            .cloned()
            .ok_or_else(|| fail(format!("asset {id} has no source .rmeta in this project")))
    }
    fn resolve_texture(&mut self, root: &Path, id: AssetId) -> Result<String, RendererError> {
        if !self.paths.contains_key(&id) {
            self.paths.extend(source_paths(root));
        }
        self.path(id)
    }
    fn surface(&self, asset: &MaterialAsset) -> Result<SurfaceTextures, RendererError> {
        let mut maps = std::collections::BTreeMap::new();
        for (kind, id) in &asset.maps {
            maps.insert(*kind, self.path(*id)?);
        }
        Ok(SurfaceTextures {
            maps,
            orm_map: asset.orm_map.map(|id| self.path(id)).transpose()?,
            roughness: Some(asset.roughness),
            metallic: Some(asset.metallic),
            base_color: asset.base_color,
            emissive_color: asset.emissive_color,
            emissive_strength: asset.emissive_strength,
            normal_scale: asset.normal_scale,
            occlusion_strength: asset.occlusion_strength,
            uv_scale: asset.uv_scale,
            uv_offset: asset.uv_offset,
            height_scale: asset.height_scale,
            alpha_cutoff: asset.alpha_cutoff,
            pbr: true,
            cast_shadows: asset.cast_shadows,
            receive_shadows: asset.receive_shadows,
            wind_response: Some(asset.wind_response),
            ..SurfaceTextures::default()
        })
    }
}
fn fail(message: String) -> RendererError {
    RendererError::InvalidMesh(message)
}
fn safe_source(root: &Path, relative: &str) -> Result<std::path::PathBuf, RendererError> {
    let root = root
        .canonicalize()
        .map_err(|error| fail(error.to_string()))?;
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|error| fail(error.to_string()))?;
    if !path.starts_with(root) {
        return Err(fail("material source leaves the project".into()));
    }
    Ok(path)
}
/// Source material is shared; mesh-side controls are per-instance overrides.
#[allow(
    clippy::float_cmp,
    reason = "exact authored neutral defaults identify whether an instance override is present"
)]
fn overlay(base: &mut SurfaceTextures, instance: &SurfaceTextures) {
    base.maps.extend(instance.maps.clone());
    if instance.orm_map.is_some() {
        base.orm_map.clone_from(&instance.orm_map);
    }
    if instance.roughness.is_some() {
        base.roughness = instance.roughness;
    }
    if instance.metallic.is_some() {
        base.metallic = instance.metallic;
    }
    base.base_color = std::array::from_fn(|i| base.base_color[i] * instance.base_color[i]);
    // Neutral instance controls preserve the authored shared source.
    if instance.emissive_color != [1.0; 3] {
        base.emissive_color = instance.emissive_color;
    }
    base.emissive_strength *= instance.emissive_strength;
    base.normal_scale *= instance.normal_scale;
    base.occlusion_strength *= instance.occlusion_strength;
    base.uv_scale = std::array::from_fn(|i| base.uv_scale[i] * instance.uv_scale[i]);
    base.uv_offset = std::array::from_fn(|i| base.uv_offset[i] + instance.uv_offset[i]);
    if (instance.height_scale - 0.05).abs() > f32::EPSILON {
        base.height_scale = instance.height_scale;
    }
    if instance.alpha_cutoff.is_some() {
        base.alpha_cutoff = instance.alpha_cutoff;
    }
    base.visibility_layers = instance.visibility_layers;
    base.effect_source = instance.effect_source;
    base.cast_shadows &= instance.cast_shadows;
    base.receive_shadows &= instance.receive_shadows;
    if instance.wind_response.is_some() {
        base.wind_response = instance.wind_response;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_assets::{AssetMeta, save_meta, sidecar_path};
    #[test]
    fn shared_sources_resolve_stable_maps_and_instance_factors_without_mutating_asset() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("copper.material");
        let asset = MaterialAsset {
            name: "Copper".into(),
            metallic: 1.0,
            base_color: [0.8, 0.2, 0.1, 1.0],
            ..MaterialAsset::default()
        };
        std::fs::write(&path, asset.to_bytes().unwrap()).unwrap();
        let meta = AssetMeta::new("rustic.material", 1);
        save_meta(&sidecar_path(&path), &meta).unwrap();
        let mut scene = super::super::ViewportScene {
            environment: engine_world::SceneEnvironment::default(),
            camera_position: [0.0; 3],
            view_projection: glam::Mat4::IDENTITY.to_cols_array(),
            meshes: vec![super::super::ViewportMesh {
                textures: SurfaceTextures {
                    material: Some(meta.asset_id),
                    roughness: Some(0.9),
                    ..SurfaceTextures::default()
                },
                instance_key: 1,
                mesh_key: 1,
                vertices: vec![],
                indices: vec![],
                model: glam::Mat4::IDENTITY.to_cols_array(),
                color: [1.0; 4],
                selected: false,
            }],
            lights: vec![],
            guides: vec![],
            grid_vertices: vec![],
            clear_color: [0.0; 4],
        };
        let mut cache = MaterialCache::default();
        cache
            .resolve(
                &mut scene,
                &EffectFrame::default(),
                root.path(),
                &HashMap::new(),
            )
            .unwrap();
        assert_eq!(scene.meshes[0].textures.metallic, Some(1.0));
        assert_eq!(scene.meshes[0].textures.roughness, Some(0.9));
        assert_eq!(
            MaterialAsset::from_bytes(&std::fs::read(path).unwrap()).unwrap(),
            asset
        );
    }
}

fn source_paths(root: &Path) -> HashMap<AssetId, String> {
    let mut paths = HashMap::new();
    let assets = root.join("assets");
    let (folder, prefix) = if assets.is_dir() {
        (assets, "assets/")
    } else {
        (root.to_path_buf(), "")
    };
    if let Ok(report) = engine_assets::scan_sidecars(&folder) {
        for asset in report.entries {
            paths.insert(
                asset.meta.asset_id,
                format!(
                    "{prefix}{}",
                    asset.source_relative.to_string_lossy().replace('\\', "/")
                ),
            );
        }
    }
    paths
}

/// Normalize world-space directional dissolve across the receiver's actual bounds.
pub(super) fn effect_direction(mesh: &super::ViewportMesh, direction: [f32; 3]) -> [f32; 4] {
    let direction = glam::Vec3::from_array(direction).normalize_or_zero();
    if direction == glam::Vec3::ZERO {
        return [0.0; 4];
    }
    let model = glam::Mat4::from_cols_array(&mesh.model);
    let mut minimum = f32::INFINITY;
    let mut maximum = f32::NEG_INFINITY;
    for vertex in &mesh.vertices {
        let value = direction.dot(model.transform_point3(glam::Vec3::from_array(vertex.position)));
        minimum = minimum.min(value);
        maximum = maximum.max(value);
    }
    let extent = maximum - minimum;
    if !extent.is_finite() || extent <= f32::EPSILON {
        return [0.0; 4];
    }
    let scaled = direction / extent;
    [scaled.x, scaled.y, scaled.z, -minimum / extent]
}

#[derive(Default)]
struct BoundaryParticles {
    progress: HashMap<engine_world::EntityId, f32>,
    particles: Vec<(
        engine_world::EntityId,
        f32,
        engine_world::effects::ParticleInstance,
    )>,
    time: f32,
    completed: std::collections::HashSet<engine_world::EntityId>,
}
impl MaterialCache {
    fn boundary_image(
        &mut self,
        root: &Path,
        path: &str,
    ) -> Result<std::sync::Arc<image::Rgba32FImage>, RendererError> {
        let metadata =
            std::fs::metadata(root.join(path)).map_err(|error| fail(error.to_string()))?;
        let stamp = metadata.modified().ok();
        if let Some((old, size, image)) = self.boundary_images.get(path)
            && *old == stamp
            && *size == metadata.len()
        {
            return Ok(std::sync::Arc::clone(image));
        }
        if self.boundary_images.len() >= 128 {
            self.boundary_images.clear();
        }
        let image = super::surface::load(root, path)?;
        let image = std::sync::Arc::new(image::imageops::resize(
            &image,
            256,
            256,
            image::imageops::FilterType::Triangle,
        ));
        self.boundary_images.insert(
            path.into(),
            (stamp, metadata.len(), std::sync::Arc::clone(&image)),
        );
        Ok(image)
    }
    /// Emit only where a moving dissolve boundary crosses actual receiver triangles.
    #[allow(
        clippy::too_many_lines,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "bounded deterministic triangle sampling and GPU-sized particle budgets"
    )]
    pub fn boundary_particles(
        &mut self,
        scene: &ViewportScene,
        frame: &EffectFrame,
        root: &Path,
    ) -> Result<Vec<engine_world::effects::ParticleInstance>, RendererError> {
        use engine_world::effects::{BlendMode, ParticleInstance, ParticleSortMode};
        use glam::{Vec2, Vec3};
        let elapsed = (frame.post.time - self.boundary.time).clamp(0.0, 0.1);
        if frame.post.time < self.boundary.time {
            self.boundary = BoundaryParticles::default();
        }
        self.boundary.time = frame.post.time;
        let gpu_counts: std::collections::BTreeMap<_, _> = frame
            .gpu_emitters
            .iter()
            .map(|emitter| (emitter.entity, emitter.live_count as usize))
            .collect();
        let available = 100_000_usize
            .saturating_sub(
                frame
                    .particles
                    .len()
                    .saturating_add(gpu_counts.values().sum::<usize>()),
            )
            .min(4096);
        self.boundary.particles.truncate(available);

        let active: std::collections::HashSet<_> = frame
            .surfaces
            .iter()
            .filter(|modifier| modifier.is_dissolve)
            .map(|modifier| modifier.source)
            .collect();
        self.boundary.completed.extend(
            frame
                .events
                .iter()
                .filter(|event| event.kind == engine_world::effects::EffectEventKind::Completed)
                .map(|event| event.entity),
        );
        self.boundary
            .particles
            .retain_mut(|(source, age, particle)| {
                *age += elapsed;
                particle.position += particle.velocity * elapsed;
                particle.color[3] = (1.0 - *age / 0.4).max(0.0);
                *age < 0.4 && (active.contains(source) || self.boundary.completed.contains(source))
            });
        self.boundary
            .progress
            .retain(|source, _| active.contains(source));
        let mut births = 0;
        let mut budget = 16384_usize;
        for modifier in frame
            .surfaces
            .iter()
            .filter(|modifier| modifier.is_dissolve && modifier.particle_density > 0.0)
        {
            let previous = self
                .boundary
                .progress
                .insert(modifier.source, modifier.dissolve)
                .unwrap_or(0.0);
            if modifier.dissolve < previous {
                self.boundary
                    .particles
                    .retain(|(source, _, _)| *source != modifier.source);
                continue;
            }
            if modifier.dissolve <= previous {
                continue;
            }
            for mesh in scene
                .meshes
                .iter()
                .filter(|mesh| surface_matches(modifier, mesh))
            {
                let model = glam::Mat4::from_cols_array(&mesh.model);
                let projection = effect_direction(mesh, modifier.direction);
                let noise = mesh
                    .textures
                    .effect_noise_map
                    .as_ref()
                    .map(|path| self.boundary_image(root, path))
                    .transpose()?;
                let mask = mesh
                    .textures
                    .effect_mask_map
                    .as_ref()
                    .map(|path| self.boundary_image(root, path))
                    .transpose()?;
                for (face_index, face) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
                    let Some(vertices) = face
                        .iter()
                        .map(|index| mesh.vertices.get(*index as usize))
                        .collect::<Option<Vec<_>>>()
                    else {
                        continue;
                    };
                    let positions: Vec<_> = vertices
                        .iter()
                        .map(|vertex| model.transform_point3(Vec3::from_array(vertex.position)))
                        .collect();
                    let cross = (positions[1] - positions[0]).cross(positions[2] - positions[0]);
                    let area = cross.length() * 0.5;
                    let normal = cross.normalize_or_zero();
                    let count = (area * modifier.particle_density).ceil().min(4096.0) as usize;
                    for sample in 0..count {
                        if births >= 512
                            || budget == 0
                            || self.boundary.particles.len() >= available
                        {
                            break;
                        }
                        budget -= 1;
                        let seed = (face_index as u64)
                            .wrapping_mul(104_729)
                            .wrapping_add(sample as u64)
                            .wrapping_add(mesh.instance_key);
                        let a = boundary_random(seed).sqrt();
                        let b = boundary_random(seed.wrapping_add(1));
                        let weights = [1.0 - a, a * (1.0 - b), a * b];
                        let position = positions[0] * weights[0]
                            + positions[1] * weights[1]
                            + positions[2] * weights[2];
                        let uv = Vec2::from_array(vertices[0].uv) * weights[0]
                            + Vec2::from_array(vertices[1].uv) * weights[1]
                            + Vec2::from_array(vertices[2].uv) * weights[2];
                        let uv = uv * Vec2::from_array(mesh.textures.uv_scale)
                            + Vec2::from_array(mesh.textures.uv_offset);
                        let mut value = if let Some(image) = &noise {
                            boundary_sample(image, uv * modifier.dissolve_noise_scale)
                        } else {
                            let p = (position * modifier.dissolve_noise_scale.max(1.0)).floor();
                            let raw = p.dot(Vec3::new(12.9898, 78.233, 37.719)).sin() * 43_758.547;
                            raw - raw.floor()
                        };
                        if projection[..3].iter().any(|value| *value != 0.0) {
                            value = (position.dot(Vec3::new(
                                projection[0],
                                projection[1],
                                projection[2],
                            )) + projection[3])
                                .clamp(0.0, 1.0);
                        }
                        if let Some(mask) = &mask {
                            value = boundary_sample(mask, uv);
                        }
                        if value < previous || value >= modifier.dissolve {
                            continue;
                        }
                        let particle = ParticleInstance {
                            entity: modifier.source,
                            position,
                            velocity: normal * (0.3 + boundary_random(seed.wrapping_add(2)) * 0.5),
                            stretch: 0.0,
                            soft_particle_distance: 0.0,
                            uv_rect: [0.0, 0.0, 1.0, 1.0],
                            rotation: 0.0,
                            rotation3: Vec3::ZERO,
                            scale: Vec3::ONE,
                            size: 0.025,
                            color: [
                                modifier.edge_color[0],
                                modifier.edge_color[1],
                                modifier.edge_color[2],
                                1.0,
                            ],
                            mesh: None,
                            material: None,
                            texture: None,
                            emission: modifier.emission.max(1.0),
                            blend: BlendMode::Additive,
                            cast_shadows: false,
                            receive_shadows: false,
                            wind: Vec3::ZERO,
                            sort_mode: ParticleSortMode::None,
                            sort_key: 0.0,
                        };
                        self.boundary
                            .particles
                            .push((modifier.source, 0.0, particle));
                        births += 1;
                    }
                }
            }
        }
        let owners: std::collections::HashSet<_> = self
            .boundary
            .particles
            .iter()
            .map(|(source, _, _)| *source)
            .collect();
        self.boundary
            .completed
            .retain(|source| owners.contains(source));
        Ok(self
            .boundary
            .particles
            .iter()
            .map(|(_, _, particle)| particle.clone())
            .collect())
    }
}
fn boundary_random(mut seed: u64) -> f32 {
    seed ^= seed >> 30;
    seed = seed.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    seed ^= seed >> 27;
    seed = seed.wrapping_mul(0x94d0_49bb_1331_11eb);
    seed ^= seed >> 31;
    f32::from_bits(0x3f80_0000 | u32::try_from(seed & 0x007f_ffff).unwrap_or_default()) - 1.0
}
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "fixed256px repeated texture coordinates are reduced modulo image extent"
)]
fn boundary_sample(image: &image::Rgba32FImage, uv: glam::Vec2) -> f32 {
    let x = uv.x.rem_euclid(1.0) * image.width() as f32 - 0.5;
    let y = uv.y.rem_euclid(1.0) * image.height() as f32 - 0.5;
    let sample = |dx: i32, dy: i32| {
        image
            .get_pixel(
                (x.floor() as i32 + dx).rem_euclid(image.width() as i32) as u32,
                (y.floor() as i32 + dy).rem_euclid(image.height() as i32) as u32,
            )
            .0[0]
    };
    let tx = x - x.floor();
    let ty = y - y.floor();
    let a = sample(0, 0) * (1.0 - tx) + sample(1, 0) * tx;
    let b = sample(0, 1) * (1.0 - tx) + sample(1, 1) * tx;
    a * (1.0 - ty) + b * ty
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    use engine_world::effects::SurfaceModifier;
    use std::hash::{Hash, Hasher};
    #[test]
    fn directional_boundary_emits_on_receiver_and_rewind_clears_particles() {
        let target = engine_world::EntityId::new();
        let mut scene = super::super::ViewportScene {
            environment: engine_world::SceneEnvironment::default(),
            camera_position: [0.0; 3],
            view_projection: glam::Mat4::IDENTITY.to_cols_array(),
            meshes: vec![super::super::ViewportMesh {
                instance_key: super::super::effect_geometry_key(target, 0),
                mesh_key: 1,
                vertices: [[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]]
                    .map(|position| super::super::ViewportVertex {
                        position,
                        normal: [0.0, 0.0, 1.0],
                        uv: [0.5; 2],
                    })
                    .to_vec(),
                indices: vec![0, 1, 2],
                model: glam::Mat4::IDENTITY.to_cols_array(),
                color: [1.0; 4],
                textures: SurfaceTextures::default(),
                selected: false,
            }],
            lights: vec![],
            guides: vec![],
            grid_vertices: vec![],
            clear_color: [0.0; 4],
        };
        // Use the scene's ordinary entity instance key, including imported parts.
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        target.hash(&mut hasher);
        scene.meshes[0].instance_key = hasher.finish();
        let mut frame = EffectFrame::default();
        frame.surfaces.push(SurfaceModifier {
            target_material: None,
            is_dissolve: true,
            source: target,
            target,
            noise_texture: None,
            mask_texture: None,
            distortion_texture: None,
            distortion_strength: 0.0,
            direction: [1.0, 0.0, 0.0],
            phase: 0.0,
            particle_density: 100.0,
            uv_offset: [0.0; 2],
            uv_scale: [1.0; 2],
            uv_rotation: 0.0,
            dissolve_noise_scale: 1.0,
            color: [1.0; 4],
            emission: 2.0,
            dissolve: 0.5,
            edge_width: 0.05,
            edge_color: [1.0, 0.5, 0.0],
        });
        let mut cache = MaterialCache::default();
        let particles = cache
            .boundary_particles(&scene, &frame, Path::new("."))
            .unwrap();
        assert!(!particles.is_empty());
        assert!(
            particles
                .iter()
                .all(|particle| particle.position.x < 0.0
                    && particle.position.z.abs() < f32::EPSILON)
        );
        assert_eq!(
            cache
                .boundary_particles(&scene, &frame, Path::new("."))
                .unwrap()
                .len(),
            particles.len()
        );
        frame.surfaces[0].dissolve = 0.0;
        assert!(
            cache
                .boundary_particles(&scene, &frame, Path::new("."))
                .unwrap()
                .is_empty()
        );
        frame.particles = vec![particles[0].clone(); 100_000];
        frame.surfaces[0].dissolve = 0.5;
        assert!(
            cache
                .boundary_particles(&scene, &frame, Path::new("."))
                .unwrap()
                .is_empty(),
            "boundary particles exceeded the shared live budget"
        );
    }
}

pub(super) fn surface_matches(
    modifier: &engine_world::effects::SurfaceModifier,
    mesh: &super::ViewportMesh,
) -> bool {
    modifier.target_material.map_or_else(
        || super::effect_entity_matches(modifier.target, mesh.instance_key),
        |material| mesh.textures.material == Some(material),
    )
}
