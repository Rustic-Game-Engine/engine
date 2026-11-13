# Scene effects acceptance audit

This is a source-backed acceptance checklist against the uploaded “Complete Scene Effects & VFX System Implementation” request. It is an engineering audit, not a claim that the full request is finished. Updated 2026-11-09 during the completion pass. Individual rows record implementation evidence; pending rendering and platform checks remain explicit.

**Working** means a real authored/runtime/render path was found. It does not mean every property has a dedicated pixel test. **Partial** means an implicit equivalent, fixed behavior, restricted backend, or disconnected property. **Missing** means no implementation path was located. Serialized controls alone are not counted as working.

Evidence paths below are relative to `Engine/crates/`. Configuration lives in `engine-world/src/effects/model.rs`; simulation in `engine-world/src/effects/simulation.rs`; modifier extraction in `engine-world/src/effects/modifiers.rs`; renderer files in `renderer-wgpu/src/`. The editor uses native recursive typed widgets in `apps/editor/src/effects.rs::properties`, module grouping in `edit_particle_modules`, curve/gradient key editing in `edit_key_graph`, preview controls in `EffectPreview`, and timeline/asset operations in `composite_timeline`. A property missing from the model is also missing from that inspector.

The full 26-type persistence test is `engine-world/tests/effects_persistence.rs::every_effect_survives_scene_reload_and_world_capture`. That test proves round-tripping, not visual correctness or that every serialized field is consumed.


## 1. Trail Renderer

Configuration: `TrailRenderer`. Behavior: `simulation.rs::EffectRuntime::update_with_targets`. Existing evidence: `trailing_points_follow_attachment_and_expire`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Enabled and emitting | Working | Authored `TrailRenderer`/common field is consumed by the behavior path above. |
| Lifetime | Working | Authored `TrailRenderer`/common field is consumed by the behavior path above. |
| Start width and end width | Working | Explicit optional endpoint widths interpolate with point age in cpu_geometry::trail_geometry. |
| Width-over-life curve | Working | Authored `TrailRenderer`/common field is consumed by the behavior path above. |
| Colour-over-life gradient | Working | Authored `TrailRenderer`/common field is consumed by the behavior path above. |
| Opacity curve | Working | Authored `TrailRenderer`/common field is consumed by the behavior path above. |
| Trail material | Working | Authored `TrailRenderer`/common field is consumed by the behavior path above. |
| Trail texture | Working | Authored `TrailRenderer`/common field is consumed by the behavior path above. |
| UV tiling and scrolling | Working | Authored `TrailRenderer`/common field is consumed by the behavior path above. |
| Minimum vertex distance | Working | Authored `TrailRenderer`/common field is consumed by the behavior path above. |
| Maximum points/segments | Working | Authored `TrailRenderer`/common field is consumed by the behavior path above. |
| Corner and cap quality | Working | Bounded corner subdivision and indexed semicircular end caps; trails_transform_history_and_tessellate_caps. |
| Smoothing | Working | Bounded Catmull-Rom smoothing; trails_transform_history_and_tessellate_caps. |
| Camera-facing or world-oriented mode | Working | Authored `TrailRenderer`/common field is consumed by the behavior path above. |
| Local/world simulation space | Working | Parent-local history transforms coherently using set_parent_transforms; world history remains fixed. |
| Blend mode | Working | Authored `TrailRenderer`/common field is consumed by the behavior path above. |
| Emissive intensity | Working | Authored `TrailRenderer`/common field is consumed by the behavior path above. |
| Attachment target | Working | Authored `TrailRenderer`/common field is consumed by the behavior path above. |
| Discontinuity/teleport handling | Working | Authored `TrailRenderer`/common field is consumed by the behavior path above. |

Validation: CPU geometry coverage passed, including trails_transform_history_and_tessellate_caps. Backend pixel coverage is tracked separately.

## 2. Particle Emitter

Configuration: `ParticleEmitter`. Behavior: `simulation.rs::simulate_particles; effects.rs::build_vertices`. Existing evidence: `lifecycle_pause_resume_restart_and_dispose_are_observable; shape_samples_stay_in_bounds_and_triangle_samples_stay_on_surface`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Enabled | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Auto-play | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Loop | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Duration | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Start delay | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Prewarm | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Simulation space | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Simulation speed | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Random seed | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Maximum particles | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Rate over time | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Rate over distance | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Timed bursts | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Burst count | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Burst probability | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Burst cycles and intervals | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Emission shape | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Emission surface/volume mode | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Point | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Sphere | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Hemisphere | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Box | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Cone | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Circle | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Cylinder | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Mesh surface, when mesh data is available | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Lifetime minimum/maximum | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Initial speed minimum/maximum | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Initial direction | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Start size | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Start rotation | Working | Seeded authored XYZ start_rotation ranges feed CPU and instance rotation. |
| Angular velocity | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Start colour | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Inherit emitter velocity | Working | World and local conversion paths both inherit emitter velocity; local_space_inherits_world_emitter_velocity_and_reports_world_events. |
| Gravity | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Drag | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Wind influence | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Directional force | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Turbulence | Working | Independent turbulence acceleration module in simulate_particles. |
| Noise | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Velocity over lifetime | Working | Three authored velocity curves change integrated movement_velocity; weighted_meshes_3d_variation_and_lifetime_motion_are_consumed. |
| Limit velocity | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Rotation over lifetime | Working | Authored curve is composed with integrated angular rotation. |
| Billboard sprites | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Stretched billboards | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Mesh particles | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Material selection | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Texture selection | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| PBR-compatible materials for mesh particles | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Blend modes | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Colour over lifetime | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Size over lifetime | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Opacity over lifetime | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Emission | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Flipbook/sprite-sheet animation | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Soft particle intersection fading, where depth data is available | Working | Authored `soft_particle_distance` reaches CPU billboard vertices; scene depth gap fades alpha in world units. GPU configurations requesting it explicitly use CPU; capture passes without scene depth keep ordinary blending. Actual Vulkan regression verifies a particle intersecting scene depth fades by over 20 display levels. |
| Collision enabled | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Collision layers | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Collision response | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Bounce | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Friction | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Stick | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Kill on collision | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Collision event generation | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Spawn on particle birth | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Spawn on collision | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Spawn on death | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Child effect reference | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Trigger probability | Working | All typed/legacy birth/collision/death triggers check probability; zero_subemitter_probability_blocks_birth_and_death_children. |
| Inherited velocity | Working | Typed trigger velocity enters child emitter initial_velocity; explicit_subemitter_triggers_inherit_world_velocity. |
| Maximum active particles | Working | Per-emitter capacity plus a global 100,000-live admission budget preserve existing particles and reject further births. `global_particle_admission_budget_preserves_existing_particles` passes; editor previews show budget exhaustion and newest GPU lifetime occupancy. |
| Render distance | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Culling | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Particle sorting | Working | Authored Distance/OldestFirst/YoungestFirst/None modes reach per-emitter CPU batch ordering. Alpha emitters requesting sorting explicitly use CPU; unsorted/additive emitters can use GPU. Renderer regression checks generated vertex order for each authored sort mode. |
| Simulation mode | Working | `gpu.rs`/`gpu_particles.rs`: persistent storage compute plus direct storage billboards; Vulkan native tests verify CPU motion parity in local/world space, ordered fixed ticks, collisions, pause/restart, and ACK retries. Unsupported modules select the complete CPU simulator with a reason. |
| Quality scaling | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Bounds | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Offscreen simulation behavior | Working | Selectable Simulate/Pause/Clear honors main and probe visibility; offscreen_pause_and_clear_are_distinct_from_simulate. |

Validation: Required properties have consumed paths recorded above. Platform and final workspace verification remain tracked in the cross-cutting checklist.

## 3. Beam / Lightning Renderer

Configuration: `BeamRenderer`. Behavior: `simulation.rs::EffectRuntime::update_with_targets`. Existing evidence: `ordinary_scene_targets_drive_beam_endpoints`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Start target | Working | Authored `BeamRenderer`/common field is consumed by the behavior path above. |
| End target | Working | Authored `BeamRenderer`/common field is consumed by the behavior path above. |
| Start/end offsets | Working | Authored `BeamRenderer`/common field is consumed by the behavior path above. |
| Width | Working | Authored `BeamRenderer`/common field is consumed by the behavior path above. |
| Segment count | Working | Authored `BeamRenderer`/common field is consumed by the behavior path above. |
| Colour gradient | Working | Authored `BeamRenderer`/common field is consumed by the behavior path above. |
| Opacity | Working | Authored `BeamRenderer`/common field is consumed by the behavior path above. |
| Material | Working | Authored `BeamRenderer`/common field is consumed by the behavior path above. |
| Texture | Working | Authored `BeamRenderer`/common field is consumed by the behavior path above. |
| Texture scrolling | Working | Authored `BeamRenderer`/common field is consumed by the behavior path above. |
| Noise amplitude | Working | Authored `BeamRenderer`/common field is consumed by the behavior path above. |
| Noise frequency | Working | Authored `BeamRenderer`/common field is consumed by the behavior path above. |
| Noise animation speed | Working | Authored `BeamRenderer`/common field is consumed by the behavior path above. |
| Branch count | Working | Authored `BeamRenderer`/common field is consumed by the behavior path above. |
| Branch probability | Working | Seeded branch probability in cpu_geometry::beam_geometry; beam_probability_and_pulse_change_geometry. |
| Branch width scaling | Working | Authored scale multiplies per-branch widths. |
| Emissive intensity | Working | Authored `BeamRenderer`/common field is consumed by the behavior path above. |
| Pulse speed | Working | Animated longitudinal per-vertex alpha envelope; beam_probability_and_pulse_change_geometry. |
| Lifetime | Working | Common duration gates playback and completed instance stops extracting geometry. |
| Endpoint collision detection | Working | Layer-filtered nearest explicit volume raycast clips beam endpoint. |

Validation: CPU branch probability, pulse and endpoint clipping coverage passed.

## 4. Shockwave / Ring Effect

Configuration: `RingEffect`. Behavior: `simulation.rs::ring_mesh`. Existing evidence: `ribbon_is_indexed_and_camera_facing (not a ring-specific visual test)`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Initial radius | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Final radius | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Expansion speed | Working | Optional world-units/sec expansion independent of common duration. |
| Lifetime | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Ring thickness | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Colour gradient | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Opacity curve | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Edge softness | Working | Radial inner/outer feather bands fade per-vertex alpha; ring_feather_and_independent_expansion_are_applied. |
| Texture | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Surface alignment | Working | Nearest explicit collision volume aligns center and surface normal. |
| Vertical offset | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Noise/displacement | Working | Authored angular frequency and displacement amplitude deform ring vertices. |
| Emission intensity | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Optional screen-space distortion | Working | Distance-weighted screen_distortion contributes to post-process distortion during active lifetime. |

Validation: CPU ring expansion/alignment/feather/displacement output coverage passed; renderer pixel checks are tracked separately.

## 5. Mesh Particle Emitter

Configuration: `ParticleEmitter`. Behavior: `simulation.rs::append_cube; renderer lib.rs::append_mesh_particle_models`. Existing evidence: `mesh_particles_and_foliage_generate_batched_triangles_without_asset`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Mesh asset | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Multiple weighted mesh selections | Working | Seeded weighted mesh/material variants at birth; weighted_meshes_3d_variation_and_lifetime_motion_are_consumed. |
| Material override | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| PBR textures/material | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Scale variation | Working | Per-axis authored XYZ start_scale ranges. |
| Rotation variation | Working | Per-axis authored XYZ start_rotation and angular_velocity_range. |
| Starting velocity | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Angular velocity | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Gravity | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Drag | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Bounce | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Friction | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Collision layers | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Lifetime | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Shadow settings | Working | Authored cast/receive flags reach retained PBR mesh batches and real RGB24 world shadow maps (directional, spot and six-face point). Vulkan caster/receiver pixel regression passed; bounded atlas diagnostics report omitted lights. |
| Maximum instances | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |
| Render distance | Working | Authored `ParticleEmitter`/common field is consumed by the behavior path above. |

Validation: weighted mesh/per-axis geometry CPU tests and real Vulkan shadow caster/receiver regression passed.

## 6. Ribbon / Tube Renderer

Configuration: `RibbonRenderer`. Behavior: `simulation.rs::smooth_path / twisted_ribbon_mesh`. Existing evidence: `tube_closes_its_uv_seam_and_handles_parallel_tangents`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Control points | Working | Authored `RibbonRenderer`/common field is consumed by the behavior path above. |
| Spline interpolation | Working | Authored `RibbonRenderer`/common field is consumed by the behavior path above. |
| Width | Working | Authored `RibbonRenderer`/common field is consumed by the behavior path above. |
| Width curve | Working | Authored `RibbonRenderer`/common field is consumed by the behavior path above. |
| Flat ribbon/tube cross-section | Working | Authored `RibbonRenderer`/common field is consumed by the behavior path above. |
| Radial segment count | Working | Authored `RibbonRenderer`/common field is consumed by the behavior path above. |
| Length subdivisions | Working | Authored `RibbonRenderer`/common field is consumed by the behavior path above. |
| Twist | Working | Authored `RibbonRenderer`/common field is consumed by the behavior path above. |
| Smoothing | Working | Authored `RibbonRenderer`/common field is consumed by the behavior path above. |
| Material | Working | Authored `RibbonRenderer`/common field is consumed by the behavior path above. |
| Texture tiling | Working | Authored `RibbonRenderer`/common field is consumed by the behavior path above. |
| UV scrolling | Working | Authored `RibbonRenderer`/common field is consumed by the behavior path above. |
| Colour gradient | Working | Authored `RibbonRenderer`/common field is consumed by the behavior path above. |
| Emission | Working | Authored `RibbonRenderer`/common field is consumed by the behavior path above. |
| Animation speed | Working | Independent animation_speed deforms local path while preserving endpoints. |

Validation: Path animation and transform-aware CPU geometry coverage passed; gradient/twist pixel checks remain renderer validation.

## 7. Particle Force Field

Configuration: `ForceField`. Behavior: `simulation.rs::field_force`. Existing evidence: `gravity_drag_and_field_stop_affect_real_particle_motion`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Force type: directional, radial attraction, radial repulsion, vortex, turbulence | Working | Authored `ForceField`/common field is consumed by the behavior path above. |
| Shape | Working | Authored `ForceField`/common field is consumed by the behavior path above. |
| Position | Working | Authored `ForceField`/common field is consumed by the behavior path above. |
| Dimensions/radius | Working | Authored `ForceField`/common field is consumed by the behavior path above. |
| Strength | Working | Authored `ForceField`/common field is consumed by the behavior path above. |
| Falloff | Working | Authored `ForceField`/common field is consumed by the behavior path above. |
| Direction | Working | Authored `ForceField`/common field is consumed by the behavior path above. |
| Vortex speed | Working | Authored vortex_speed scales tangential field acceleration. |
| Noise frequency | Working | Authored `ForceField`/common field is consumed by the behavior path above. |
| Drag influence | Working | Spatially weighted field drag contributes exponential velocity damping. |
| Affected effect layers | Working | Authored `ForceField`/common field is consumed by the behavior path above. |
| Priority | Working | Only highest spatially applicable force priority contributes acceleration; equal-priority fields add. |

Validation: CPU drag/vortex/priority tests passed. Highest spatially applicable priority wins; ties add.

## 8. Particle Collision Volume

Configuration: `CollisionVolume`. Behavior: `simulation.rs::sweep_box / sweep_sphere / sweep_cylinder`. Existing evidence: `swept_contacts_handle_thin_volumes_and_cylinder_corners`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Shape: sphere, box, plane, cylinder where supported | Working | Authored `CollisionVolume`/common field is consumed by the behavior path above. |
| Dimensions | Working | Authored `CollisionVolume`/common field is consumed by the behavior path above. |
| Collision layers | Working | Authored `CollisionVolume`/common field is consumed by the behavior path above. |
| Bounce | Working | Authored `CollisionVolume`/common field is consumed by the behavior path above. |
| Friction | Working | Authored `CollisionVolume`/common field is consumed by the behavior path above. |
| Collision response | Working | Authored `CollisionVolume`/common field is consumed by the behavior path above. |
| Kill on contact | Working | Authored `CollisionVolume`/common field is consumed by the behavior path above. |
| Stick on contact | Working | Authored `CollisionVolume`/common field is consumed by the behavior path above. |
| Impact child effect | Working | Authored `CollisionVolume`/common field is consumed by the behavior path above. |
| Collision quality | Working | Volume and emitter select Discrete or Swept contact; existing swept_contacts_handle_thin_volumes_and_cylinder_corners covers swept path. |
| Enabled | Working | Authored `CollisionVolume`/common field is consumed by the behavior path above. |

Validation: Required properties have consumed paths recorded above. Platform and final workspace verification remain tracked in the cross-cutting checklist.

## 9. Projectile Tracer

Configuration: `ProjectileTracer`. Behavior: `simulation.rs::EffectRuntime::update_with_targets`. Existing evidence: `no dedicated trajectory/render test located`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Start/end position | Working | Authored local start_position, optional end_position or end_target; endpoint interpolates over lifetime. |
| Velocity | Working | Authored `ProjectileTracer`/common field is consumed by the behavior path above. |
| Length | Working | Authored `ProjectileTracer`/common field is consumed by the behavior path above. |
| Width | Working | Authored `ProjectileTracer`/common field is consumed by the behavior path above. |
| Colour | Working | Authored `ProjectileTracer`/common field is consumed by the behavior path above. |
| Opacity | Working | Authored `ProjectileTracer`/common field is consumed by the behavior path above. |
| Emission | Working | Authored `ProjectileTracer`/common field is consumed by the behavior path above. |
| Tail fade | Working | Subdivided per-vertex alpha ramp uses authored tail_fade exponent. |
| Velocity stretch | Working | Authored multiplier adds world speed to tracer length. |
| Lifetime | Working | Authored `ProjectileTracer`/common field is consumed by the behavior path above. |
| Attachment options | Working | Authored `ProjectileTracer`/common field is consumed by the behavior path above. |
| Material | Working | Authored `ProjectileTracer`/common field is consumed by the behavior path above. |

Validation: Explicit endpoint/target, tail fade and velocity-stretch CPU tests passed.

## 10. Fog Volume

Configuration: `FogVolume`. Behavior: `modifiers.rs::extract_modifier; post.rs / post_common.wgsl / atmosphere.wgsl / temporal.wgsl / final_post.wgsl`. Existing evidence: `post.rs::tests::shader_validates; renderer output test exercises representative fog`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Shape | Working | Authored `FogVolume`/common field is consumed by the behavior path above. |
| Dimensions | Working | Authored `FogVolume`/common field is consumed by the behavior path above. |
| Fog density | Working | Authored `FogVolume`/common field is consumed by the behavior path above. |
| Colour | Working | Authored `FogVolume`/common field is consumed by the behavior path above. |
| Height falloff | Working | Authored `FogVolume`/common field is consumed by the behavior path above. |
| Edge falloff | Working | Authored `FogVolume`/common field is consumed by the behavior path above. |
| Noise texture | Working | Optional asset resolves into the retained texture array and is sampled triplanarly in post_common.wgsl::fog_density. |
| Noise scale | Working | Authored `FogVolume`/common field is consumed by the behavior path above. |
| Noise animation speed | Working | Per-volume elapsed time multiplied by noise_speed controls three-axis drift. |
| Light scattering | Working | Beer-Lambert integration evaluates ambient/sun/point/spot radiance with scene-depth occlusion and self-extinction. |
| Anisotropy | Working | Henyey-Greenstein phase uses the authored per-volume anisotropy. |
| Maximum draw distance | Working | Each volume clips integration end by max_distance and the visible surface depth. |
| Volumetric quality | Working | Common Low/Medium/High scales bounded per-volume ray steps. |
| Raymarch step count or equivalent quality control | Working | Authored `FogVolume`/common field is consumed by the behavior path above. |

Validation: four post stages and CPU/GPU uniform-size regression pass; native Vulkan and GLES verify unclipped HDR, above-one bloom, soft-depth fading, representative fog, distortion and cleanup. Actual Vulkan and GLES overlay-mask, temporal depth-occlusion flare and moving-object motion-blur assertions also pass; dedicated localized haze/shaft pixel assertions remain a coverage limitation; GLES/browser verification is tracked in the final validation record. Camera-depth occlusion cannot query surfaces outside the rendered depth image.

## 11. Decal Projector

Configuration: `DecalProjector`. Behavior: `decals.rs::project; material.rs::MaterialCache::resolve; decals.rs::apply_view_fade`. Existing evidence: `projector_clips_receiver_triangles_and_offsets_along_surface`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Projection shape | Working | Typed Box/Sphere/Cylinder/Plane selection builds convex clipping planes over actual receiver triangles; curved shapes use bounded tessellation. |
| Width, height, and depth | Working | Authored `DecalProjector`/common field is consumed by the behavior path above. |
| Rotation | Working | Authored `DecalProjector`/common field is consumed by the behavior path above. |
| Albedo texture | Working | Authored `DecalProjector`/common field is consumed by the behavior path above. |
| Normal map | Working | Authored `DecalProjector`/common field is consumed by the behavior path above. |
| Roughness | Working | Authored `DecalProjector`/common field is consumed by the behavior path above. |
| Metallic | Working | Authored `DecalProjector`/common field is consumed by the behavior path above. |
| Ambient occlusion | Working | Packed ORM resolves through stable asset IDs; linear red channel populates AO atlas slot while G/B provide roughness/metallic. |
| Emissive map | Working | `emissive_map` resolves to the PBR emissive atlas slot with linear HDR sampling and authored emission strength. |
| Opacity | Working | Authored `DecalProjector`/common field is consumed by the behavior path above. |
| Edge fading | Working | Authored `DecalProjector`/common field is consumed by the behavior path above. |
| Angle fading | Working | Authored `DecalProjector`/common field is consumed by the behavior path above. |
| Surface layer mask | Working | Projector intersects receiver visibility layers before clipping; CPU test explicitly rejects zero overlap. |
| Render priority | Working | Per-view helper sorts projected decal meshes stably after receiver meshes by common priority. CPU regression asserts ordering. |
| Lifetime | Working | Authored `DecalProjector`/common field is consumed by the behavior path above. |
| Distance fading | Working | `distance_fade` is a world-unit alpha band before common render distance; main/probe clones evaluate their own camera distance. |

Validation: The 2026-11-09 Vulkan suite passed 53/53 tests, including expanded receiver-clipping coverage for spherical/cylindrical/plane bounds, layers, priority and distance fade; packed ORM channel mapping; and HDR/source texture pixel rendering. A subsequent shader-only angle/emission refinement still requires the coordinator final backend rerun.

## 12. Weather Volume

Configuration: `WeatherVolume`. Behavior: `simulation.rs weather emitter adapter`. Existing evidence: `generic particle simulation tests; no six-mode visual comparison`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Rain | Working | Authored `WeatherVolume`/common field is consumed by the behavior path above. |
| Snow | Working | Authored `WeatherVolume`/common field is consumed by the behavior path above. |
| Hail | Working | Authored `WeatherVolume`/common field is consumed by the behavior path above. |
| Dust | Working | Authored `WeatherVolume`/common field is consumed by the behavior path above. |
| Sand | Working | Authored `WeatherVolume`/common field is consumed by the behavior path above. |
| Falling leaves | Working | Authored `WeatherVolume`/common field is consumed by the behavior path above. |
| Weather type | Working | Authored `WeatherVolume`/common field is consumed by the behavior path above. |
| Coverage bounds | Working | Authored `WeatherVolume`/common field is consumed by the behavior path above. |
| Intensity | Working | Authored `WeatherVolume`/common field is consumed by the behavior path above. |
| Particle size | Working | Authored `WeatherVolume`/common field is consumed by the behavior path above. |
| Falling speed | Working | Authored `WeatherVolume`/common field is consumed by the behavior path above. |
| Wind influence | Working | Authored `WeatherVolume`/common field is consumed by the behavior path above. |
| Turbulence | Working | Independent underlying emitter turbulence module. |
| Spawn distribution | Working | Authored `WeatherVolume`/common field is consumed by the behavior path above. |
| Collision enabled | Working | Authored `WeatherVolume`/common field is consumed by the behavior path above. |
| Splash effect | Working | Dedicated splash_effect adds typed Collision child trigger without mutating authored emitter. |
| Accumulation appearance | Working | Bounded normal-aligned settled patches fade over accumulation_lifetime; weather_collision_spawns_splash_and_deposits_expire_without_events. |
| Camera-follow mode | Working | Authored `WeatherVolume`/common field is consumed by the behavior path above. |
| Draw distance | Working | Authored `WeatherVolume`/common field is consumed by the behavior path above. |
| Density falloff | Working | Seeded radial spawn probability varies inside authored volume. |
| Quality level | Working | Authored `WeatherVolume`/common field is consumed by the behavior path above. |

Validation: Weather accumulation/splash/density CPU coverage passed; visual mode comparisons remain renderer validation.

## 13. Water Ripple / Wake Effect

Configuration: `RingEffect`. Behavior: `water.rs::WaterDisturbance::sample / water_surface_mesh; renderer lib.rs::append_effect_frame`. Existing evidence: `disturbance_displaces_and_expires; water_grid_has_displaced_normals_and_bounded_storage`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Water surface target | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Source position | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Radius | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Wavelength | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Amplitude | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Propagation speed | Working | Optional expansion_speed also controls WaterDisturbance propagation. |
| Damping | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Disturbance duration | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Source velocity | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Wake angle | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Foam intensity | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Foam lifetime | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Normal strength | Working | Authored `RingEffect`/common field is consumed by the behavior path above. |
| Maximum simultaneous disturbances | Working | Runtime groups receiver disturbances under strictest target budget; persistent RippleWaterSurface also enforces bounded storage. |

Validation: Water per-target disturbance budgeting and independent expansion-speed coverage passed; imported-target foam pixels remain renderer validation.

## 14. Wind Zone

Configuration: `ForceField`. Behavior: `simulation.rs::sample_wind / field_force`. Existing evidence: `gravity_drag_and_field_stop_affect_real_particle_motion`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Global/local mode | Working | Authored `ForceField`/common field is consumed by the behavior path above. |
| Shape | Working | Authored `ForceField`/common field is consumed by the behavior path above. |
| Direction | Working | Authored `ForceField`/common field is consumed by the behavior path above. |
| Strength | Working | Authored `ForceField`/common field is consumed by the behavior path above. |
| Gust strength | Working | Independent gust envelope adds to base strength. |
| Gust frequency | Working | Authored Hz controls gust envelope; material_wind_gusts_respect_recipient_mask_and_layers. |
| Turbulence | Working | Authored `ForceField`/common field is consumed by the behavior path above. |
| Spatial noise scale | Working | Authored `ForceField`/common field is consumed by the behavior path above. |
| Falloff | Working | Authored `ForceField`/common field is consumed by the behavior path above. |
| Bounds | Working | Authored `ForceField`/common field is consumed by the behavior path above. |
| Affected component types | Working | Recipient flags filter particles, scatter and sample_render_wind material consumers. |
| Layer masks | Working | Authored `ForceField`/common field is consumed by the behavior path above. |

Validation: Wind recipient flags, gust envelope, noise-scale and priority tests passed. Material opt-in is tracked with renderer evidence.

## 15. Heat Haze / Distortion

Configuration: `DistortionEffect`. Behavior: `modifiers.rs::extract_modifier; post.rs / post_common.wgsl / atmosphere.wgsl / temporal.wgsl / final_post.wgsl`. Existing evidence: `shader validation and renderer output checks; no depth-mask test`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Radius/area | Working | Authored `DistortionEffect`/common field is consumed by the behavior path above. |
| Distortion strength | Working | Authored `DistortionEffect`/common field is consumed by the behavior path above. |
| Noise texture | Working | Optional sampled RG noise asset is retained in the post texture array. |
| Noise scale | Working | Authored `DistortionEffect`/common field is consumed by the behavior path above. |
| Noise speed | Working | Authored `DistortionEffect`/common field is consumed by the behavior path above. |
| Refraction offset | Working | Independent authored XY vector is added to the noise displacement. |
| Edge falloff | Working | Authored exponent shapes the projected volume edge. |
| Depth masking | Working | Foreground volume rejection and displaced-depth discontinuity rejection prevent refracting unrelated surfaces. |
| Screen-space/world-space mode | Working | Screen mode covers the image; world mode intersects each camera ray with the transformed radius. |
| Lifetime | Working | Authored `DistortionEffect`/common field is consumed by the behavior path above. |

Validation: four post stages and CPU/GPU uniform-size regression pass; native Vulkan and GLES verify unclipped HDR, above-one bloom, soft-depth fading, representative fog, distortion and cleanup. Actual Vulkan and GLES overlay-mask, temporal depth-occlusion flare and moving-object motion-blur assertions also pass; dedicated localized haze/shaft pixel assertions remain a coverage limitation; GLES/browser verification is tracked in the final validation record. Camera-depth occlusion cannot query surfaces outside the rendered depth image.

## 16. Light Shaft / God Ray

Configuration: `LightShaft`. Behavior: `modifiers.rs::extract_modifier; post.rs::scene_values / atmosphere.wgsl / flare_visibility.wgsl`. Existing evidence: `shader validation only for shaft-specific path`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Light source reference | Working | Authored `LightShaft`/common field is consumed by the behavior path above. |
| Intensity | Working | Authored `LightShaft`/common field is consumed by the behavior path above. |
| Shaft colour | Working | Authored `LightShaft`/common field is consumed by the behavior path above. |
| Scattering density | Working | Authored `LightShaft`/common field is consumed by the behavior path above. |
| Anisotropy | Working | Authored phase parameter controls directional scattering. |
| Maximum distance | Working | Distance to the resolved source is tested before shaft sampling. |
| Sample count | Working | Authored `LightShaft`/common field is consumed by the behavior path above. |
| Occlusion | Working | Authored `LightShaft`/common field is consumed by the behavior path above. |
| Temporal filtering | Working | Motion/depth reprojected HDR history filters shafts and rejects disocclusions. |
| Quality | Working | Common quality scales bounded radial integration samples. |
| Bounds if localized | Working | Inverse transform, dimensions and shape intersect camera rays to restrict localized shafts. |

Validation: four post stages and CPU/GPU uniform-size regression pass; native Vulkan and GLES verify unclipped HDR, above-one bloom, soft-depth fading, representative fog, distortion and cleanup. Actual Vulkan and GLES overlay-mask, temporal depth-occlusion flare and moving-object motion-blur assertions also pass; dedicated localized haze/shaft pixel assertions remain a coverage limitation; GLES/browser verification is tracked in the final validation record. Camera-depth occlusion cannot query surfaces outside the rendered depth image.

## 17. Foliage Scatter

Configuration: `FoliageScatter`. Behavior: `simulation.rs scatter branch / append_foliage`. Existing evidence: `mesh_particles_and_foliage_generate_batched_triangles_without_asset`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Source mesh selection | Working | Authored `FoliageScatter`/common field is consumed by the behavior path above. |
| Weighted mesh variants | Working | Seeded weighted mesh/material choice in cpu_geometry::scatter_samples. |
| Distribution density | Working | Authored `FoliageScatter`/common field is consumed by the behavior path above. |
| Distribution seed | Working | Authored `FoliageScatter`/common field is consumed by the behavior path above. |
| Surface bounds | Working | Authored `FoliageScatter`/common field is consumed by the behavior path above. |
| Minimum/maximum slope | Working | Surface normal slope restricts eligible receiver triangles. |
| Height restrictions | Working | Receiver polygons clip against world height range before area sampling. |
| Scale variation | Working | Authored `FoliageScatter`/common field is consumed by the behavior path above. |
| Rotation variation | Working | Authored `FoliageScatter`/common field is consumed by the behavior path above. |
| Surface alignment | Working | Quaternion rotates source up to sampled receiver normal. |
| Wind response | Working | Authored `FoliageScatter`/common field is consumed by the behavior path above. |
| Render distance | Working | Authored `FoliageScatter`/common field is consumed by the behavior path above. |
| LOD | Working | Stable density thinning and mesh substitution by minimum main/probe view distance. |
| Shadow settings | Working | Authored cast/receive flags reach retained PBR mesh batches and real RGB24 world shadow maps (directional, spot and six-face point). Vulkan caster/receiver pixel regression passed; bounded atlas diagnostics report omitted lights. |
| Maximum instances | Working | Authored `FoliageScatter`/common field is consumed by the behavior path above. |

Validation: Weighted target-local receiver sampling, slope/height, alignment and LOD CPU tests passed. Shadow rendering is tracked separately.

## 18. Dissolve Effect

Configuration: `DissolveEffect`. Behavior: `modifiers.rs::extract_modifier; renderer lib.rs::MESH_SHADER`. Existing evidence: `representative renderer output check`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Target material/object | Working | `target` selects an object; `target_material` selects all instances of a stable `.material` source through `material::surface_matches`. |
| Noise texture | Working | Stable texture identity resolves to effect atlas layer0; `MESH_SHADER::dissolve_order` samples linear red coverage. |
| Dissolve progress | Working | Authored `DissolveEffect`/common field is consumed by the behavior path above. |
| Threshold | Working | Authored threshold biases progress plus the animation curve before clamping in `extract_modifier`. |
| Edge width | Working | Authored `DissolveEffect`/common field is consumed by the behavior path above. |
| Edge colour | Working | Authored `DissolveEffect`/common field is consumed by the behavior path above. |
| Edge emission | Working | Authored `DissolveEffect`/common field is consumed by the behavior path above. |
| Dissolve direction | Working | `material::effect_direction` normalizes world-space projection over receiver vertices; shader and boundary sampler consume it. |
| Direction mask | Working | Stable mask identity resolves to effect atlas layer1; red coverage controls dissolve order in color/depth/shadow passes. |
| Particle spawn density | Working | `MaterialCache::boundary_particles` samples real receiver triangles crossed by the animated threshold; density scales surface-area sampling with bounded births and retained fading particles. Boundary slots respect the shared 100,000 CPU and GPU live budget and 4,096 boundary-particle cap. |
| Duration | Working | Authored `DissolveEffect`/common field is consumed by the behavior path above. |
| Animation curve | Working | Authored `DissolveEffect`/common field is consumed by the behavior path above. |

Validation: Vulkan 53/53 passed, including CPU boundary-on-receiver/rewind/global-budget regression and stable-ID GPU mask reload/restoration pixel assertions. Authored materials remain unchanged because modifiers apply to extracted render recipes. Final shader refinements are subject to the coordinator backend rerun.

## 19. Animated Surface Effect

Configuration: `AnimatedSurface`. Behavior: `modifiers.rs::extract_modifier; renderer lib.rs material uniform assembly`. Existing evidence: `representative renderer output check`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Target material | Working | Stable `target_material` matches shared PBR material instances; object targets remain available. |
| UV scroll speed | Working | Authored `AnimatedSurface`/common field is consumed by the behavior path above. |
| UV scale | Working | Authored `AnimatedSurface`/common field is consumed by the behavior path above. |
| UV rotation | Working | Authored `AnimatedSurface`/common field is consumed by the behavior path above. |
| Distortion map | Working | Static stable-ID RG texture resolves to effect atlas layer2 and offsets animated UV coordinates. |
| Distortion strength | Working | Instance uniform scales sampled RG displacement without animated texture reuploads. |
| Colour gradient | Working | Authored `AnimatedSurface`/common field is consumed by the behavior path above. |
| Emission intensity | Working | Authored `AnimatedSurface`/common field is consumed by the behavior path above. |
| Animation phase | Working | Authored phase combines with elapsed time and loop speed for scrolling/distortion sampling. |
| Mask texture | Working | Effect atlas mask scales tint/emission influence while preserving underlying source material outside coverage. |
| Loop speed | Working | Authored `AnimatedSurface`/common field is consumed by the behavior path above. |

Validation: Native typed controls expose phase, map pickers, strengths and gradient graphs. Static atlases cache by recipe and source modification stamp; per-frame values remain instance uniforms. Final backend tests are tracked by the coordinator.

## 20. Lens Flare

Configuration: `LensFlare`. Behavior: `post.rs::scene_values / atmosphere.wgsl / flare_visibility.wgsl`. Existing evidence: `post::tests::overlay_mask_flare_occlusion_and_motion_blur_change_real_pixels` passes on Vulkan and GLES; verifies temporal fade after a real depth occluder enters the flare ray.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Light source target | Working | Authored `LensFlare`/common field is consumed by the behavior path above. |
| Flare texture | Working | Optional authored texture resolves into the retained post texture array and modulates ghosts. |
| Intensity | Working | Authored `LensFlare`/common field is consumed by the behavior path above. |
| Scale | Working | Authored `LensFlare`/common field is consumed by the behavior path above. |
| Colour | Working | Authored `LensFlare`/common field is consumed by the behavior path above. |
| Ghost count | Working | Authored `LensFlare`/common field is consumed by the behavior path above. |
| Ghost spacing | Working | Authored `LensFlare`/common field is consumed by the behavior path above. |
| Streak length | Working | Authored `LensFlare`/common field is consumed by the behavior path above. |
| Streak orientation | Working | Authored radians rotate the streak basis. |
| Occlusion | Working | Authored `LensFlare`/common field is consumed by the behavior path above. |
| Visibility fade speed | Working | A retained4x1 visibility history exponentially smooths depth occlusion transitions independently of HDR TAA. |
| Maximum distance | Working | Source-camera distance gates the source before visibility evaluation. |

Validation: four post stages and CPU/GPU uniform-size regression pass; native Vulkan and GLES verify unclipped HDR, above-one bloom, soft-depth fading, representative fog, distortion and cleanup. Actual Vulkan and GLES overlay-mask, temporal depth-occlusion flare and moving-object motion-blur assertions also pass; dedicated localized haze/shaft pixel assertions remain a coverage limitation; GLES/browser verification is tracked in the final validation record. Camera-depth occlusion cannot query surfaces outside the rendered depth image.

## 21. Light Pulse / Flicker

Configuration: `LightPulse`. Behavior: `modifiers.rs::extract_modifier; renderer lib.rs::append_effect_frame`. Existing evidence: `modifier tests check target light values`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Target light | Working | Authored `LightPulse`/common field is consumed by the behavior path above. |
| Intensity curve | Working | Authored `LightPulse`/common field is consumed by the behavior path above. |
| Colour gradient | Working | Authored `LightPulse`/common field is consumed by the behavior path above. |
| Frequency | Working | Authored `LightPulse`/common field is consumed by the behavior path above. |
| Randomness | Working | Authored `LightPulse`/common field is consumed by the behavior path above. |
| Duration | Working | Authored `LightPulse`/common field is consumed by the behavior path above. |
| Phase offset | Working | Authored `LightPulse`/common field is consumed by the behavior path above. |
| Loop | Working | Authored `LightPulse`/common field is consumed by the behavior path above. |
| Fade in/out | Partial | Can be baked into intensity curve; no explicit fade envelopes. |
| Random seed | Working | Authored `LightPulse`/common field is consumed by the behavior path above. |

Remaining acceptance: Fade in/out. See each row for the exact closure condition.

## 22. Post-Processing Volume

Configuration: `PostProcessVolume`. Behavior: `modifiers.rs::extract_modifier; post.rs / post_common.wgsl / atmosphere.wgsl / temporal.wgsl / final_post.wgsl`. Existing evidence: `overlapping volume tests and renderer output check`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Global/local | Working | Authored `PostProcessVolume`/common field is consumed by the behavior path above. |
| Shape | Working | Box/sphere/cylinder local volume distances respect world transform and blend distance. |
| Size | Working | Authored `PostProcessVolume`/common field is consumed by the behavior path above. |
| Priority | Working | Authored `PostProcessVolume`/common field is consumed by the behavior path above. |
| Blend distance | Working | Authored `PostProcessVolume`/common field is consumed by the behavior path above. |
| Blend weight | Working | Authored `PostProcessVolume`/common field is consumed by the behavior path above. |
| Camera/layer applicability | Working | Explicit camera target and layers filter volume extraction. |
| Bloom | Working | Authored `PostProcessVolume`/common field is consumed by the behavior path above. |
| Exposure | Working | Authored `PostProcessVolume`/common field is consumed by the behavior path above. |
| Tonemapping-compatible adjustments | Working | Rgba16Float scene/atmosphere/history retain linear HDR; bloom/grade/exposure precede final Reinhard+sRGB. |
| Colour grading | Working | Authored `PostProcessVolume`/common field is consumed by the behavior path above. |
| Saturation | Working | Authored `PostProcessVolume`/common field is consumed by the behavior path above. |
| Contrast | Working | Authored `PostProcessVolume`/common field is consumed by the behavior path above. |
| Brightness | Working | Authored `PostProcessVolume`/common field is consumed by the behavior path above. |
| Depth of field | Working | Authored `PostProcessVolume`/common field is consumed by the behavior path above. |
| Vignette | Working | Authored `PostProcessVolume`/common field is consumed by the behavior path above. |
| Chromatic aberration | Working | Authored `PostProcessVolume`/common field is consumed by the behavior path above. |
| Motion blur where supported | Working | Portable RGB24 depth plus Rgba16Float rigid/deformed-object motion vectors drive depth-aware shutter sampling. |
| Film grain | Working | Authored `PostProcessVolume`/common field is consumed by the behavior path above. |
| Colour tint | Working | Authored `PostProcessVolume`/common field is consumed by the behavior path above. |

Validation: four post stages and CPU/GPU uniform-size regression pass; native Vulkan and GLES verify unclipped HDR, above-one bloom, soft-depth fading, representative fog, distortion and cleanup. Actual Vulkan and GLES overlay-mask, temporal depth-occlusion flare and moving-object motion-blur assertions also pass; dedicated localized haze/shaft pixel assertions remain a coverage limitation; GLES/browser verification is tracked in the final validation record. Camera-depth occlusion cannot query surfaces outside the rendered depth image.

## 23. Outline / Highlight

Configuration: `OutlineEffect`. Behavior: `modifiers.rs::extract_modifier; renderer selection pipeline`. Existing evidence: `outline_width_is_stable_across_object_scale`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Target entities | Working | Authored `OutlineEffect`/common field is consumed by the behavior path above. |
| Target layers | Working | `target_layers` intersects receiver `visibility_layers` during regular and through-wall outline selection. |
| Outline colour | Working | Authored `OutlineEffect`/common field is consumed by the behavior path above. |
| Outline thickness | Working | Authored `OutlineEffect`/common field is consumed by the behavior path above. |
| Opacity | Working | Authored `OutlineEffect`/common field is consumed by the behavior path above. |
| Pulse speed | Working | Authored `OutlineEffect`/common field is consumed by the behavior path above. |
| Visible-through-walls option | Working | Authored `OutlineEffect`/common field is consumed by the behavior path above. |
| Depth sensitivity | Working | Depth bias/sensitivity participates in the scene depth-tested outline pipeline. |
| Emissive intensity | Working | Outline emission contributes HDR radiance before tone mapping, including through-wall dilation compositing. |
| Render priority | Working | Priority-ordered extraction and stable per-target selection define overlapping outline composition; through-wall groups composite in priority order. |

Validation: `outline_width_is_stable_across_object_scale` covers width behavior; `ThroughWallOutlines` builds a depth-independent silhouette mask and excludes silhouette interiors during HDR dilation. Coordinator owns final backend pixel test results.

## 24. Camera Shake / Impulse

Configuration: `CameraShake`. Behavior: `modifiers.rs::extract_modifier; renderer lib.rs::append_effect_frame`. Existing evidence: `atlas_frames_and_camera_targets_follow_configuration; modifier camera tests`.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Target camera | Working | Authored `CameraShake`/common field is consumed by the behavior path above. |
| Position amplitude | Working | Authored `CameraShake`/common field is consumed by the behavior path above. |
| Rotation amplitude | Working | Authored `CameraShake`/common field is consumed by the behavior path above. |
| Frequency | Working | Authored `CameraShake`/common field is consumed by the behavior path above. |
| Duration | Working | Authored `CameraShake`/common field is consumed by the behavior path above. |
| Noise seed | Working | Authored `CameraShake`/common field is consumed by the behavior path above. |
| Falloff curve | Working | Authored `CameraShake`/common field is consumed by the behavior path above. |
| Effect radius | Working | Authored `CameraShake`/common field is consumed by the behavior path above. |
| Distance attenuation | Working | Authored `CameraShake`/common field is consumed by the behavior path above. |
| Priority | Working | Authored `CameraShake`/common field is consumed by the behavior path above. |
| Blending | Partial | Impulses sum; there is no authored blend mode. |
| Reset behavior | Partial | Offsets are extraction-only and clear next frame, but no explicit reset-mode selector. |

Remaining acceptance: Blending, Reset behavior. See each row for the exact closure condition.

## 25. Screen Flash / Damage Overlay

Configuration: `ScreenFlash`. Behavior: `modifiers.rs::extract_modifier; post.rs / post_common.wgsl / atmosphere.wgsl / temporal.wgsl / final_post.wgsl`. Existing evidence: CPU alpha/additive/multiply stacking tests plus Vulkan/GLES `post::tests::overlay_mask_flare_occlusion_and_motion_blur_change_real_pixels` verifies black/white screen-mask coverage.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Target camera | Working | Authored `ScreenFlash`/common field is consumed by the behavior path above. |
| Colour | Working | Authored `ScreenFlash`/common field is consumed by the behavior path above. |
| Opacity | Working | Authored `ScreenFlash`/common field is consumed by the behavior path above. |
| Fade in | Working | Authored `ScreenFlash`/common field is consumed by the behavior path above. |
| Fade out | Working | Authored `ScreenFlash`/common field is consumed by the behavior path above. |
| Duration | Working | Authored `ScreenFlash`/common field is consumed by the behavior path above. |
| Screen mask | Working | Optional screen mask asset controls per-pixel overlay opacity. |
| Vignette strength | Working | Authored `ScreenFlash`/common field is consumed by the behavior path above. |
| Blend mode | Working | Authored `ScreenFlash`/common field is consumed by the behavior path above. |
| Priority | Working | Authored `ScreenFlash`/common field is consumed by the behavior path above. |
| Stacking behavior | Working | Composite/Replace/Maximum policies are independent of alpha/additive/multiply blend. |

Validation: four post stages and CPU/GPU uniform-size regression pass; native Vulkan and GLES verify unclipped HDR, above-one bloom, soft-depth fading, representative fog, distortion and cleanup. Actual Vulkan and GLES overlay-mask, temporal depth-occlusion flare and moving-object motion-blur assertions also pass; dedicated localized haze/shaft pixel assertions remain a coverage limitation; GLES/browser verification is tracked in the final validation record. Camera-depth occlusion cannot query surfaces outside the rendered depth image.

## 26. Reflection Probe

Configuration: `ReflectionProbe`. Extraction: `engine-world/src/effects/modifiers.rs`.
Rendering: `renderer-wgpu/src/reflections.rs` and `probe_capture.rs`; assets:
`engine-assets/src/cubemap.rs`. Five reflection math/cache tests and five actual
GPU probe tests passed on lavapipe (`cargo test -p renderer-wgpu --lib probe --
--include-ignored`, plus `reflections::tests`). GPU readback verifies radiance,
rendered overlap/priority, box parallax and resource lifecycle, rather than only
serialized settings. Camera final post and local fog post are outside cube capture;
sky, PBR meshes, decals, transparent CPU effects and GPU particles are included.

| Required property | Status | Evidence or remaining work |
| --- | --- | --- |
| Probe shape | Working | Box, ellipsoid and elliptical cylinder influence/projection; shape/dimension and parent-scale math tests. |
| Influence bounds | Working | Inverse world transform and authored dimensions; blend distance measured in world units. |
| Capture position | Working | Six cameras use transformed local capture position; actual displaced box-parallax pixel test. |
| Cubemap asset | Working | Versioned `.cubemap` references six equal square HDR/LDR faces; bounded decode and dependency tracking; panorama sources remain supported. |
| Capture resolution | Working | Power-of-two 16..2048 faces; real readback changes 16→32; allocations preflight the retained 16-map/1GiB budget. |
| Refresh mode | Working | Baked source/hot reload, one capture on enable, timed interval and explicit refresh revision; actual GPU capture-count tests. |
| Blend distance | Working | Four weighted local sources, same-priority normalization and lower-priority boundary fill; math and actual rendered overlap tests. |
| Priority | Working | Higher-priority sources consume influence first; actual red/green capture shader regression. |
| Object visibility layers | Working | Independent camera, capture and receiver masks; capture-only layer HDR readback remains visible with main mask zero. |
| Update frequency | Working | Interval scheduling, time reset and fair bounded capture queue; actual interval boundary test. |
| Parallax correction where supported | Working | Toggle reaches shader; box/sphere/cylinder projection. Actual box test changes reflected distant cyan wall to correct nearby red wall without recapture. |
| Render quality | Working | Low/Medium/High use 8/24/64 GGX convolution samples and 8/16/32 capture lights; quality invalidates the cached capture. |

Resource tests verify resolution changes, explicit refresh, inactive-map/readback disposal,
exact retained byte accounting and six-face source-file hot reload fingerprints. Native
`SceneWorld::request_reflection_probe_refresh` uses ordinary validated component commands;
`Effects.refreshProbe` exposes the same operation through the existing script query host.

## Cross-cutting acceptance

| Requirement | Status | Evidence and required closure |
| --- | --- | --- |
| 26 registered component types | Working | `EffectKind::all_defaults`; scene schema `rustic.effect`; ordinary scene entity workflow. |
| Common lifecycle, delay, duration, looping, seed and playback events | Working | `EffectRuntime::control/update_with_targets`; completion/lifecycle/seed tests; world scripting wrappers. |
| Common transforms, parenting and attachment IDs | Working | Existing scene transforms and full target-transform map; additive target remap persistence test. |
| Visibility layers | Working | Main PBR/CPU/GPU effects filter the active camera mask after independent probe captures; receivers and capture passes retain their own masks. Actual Vulkan capture-only-layer and GPU independent-view readbacks pass. |
| Common parameters and bounds | Working | `EffectComponent::resolve_parameters`; parameter/prewarm test; conservative bounds-based distance culling for simulation geometry. |
| Asset create/save/load/instantiate and per-instance overrides | Working | `EffectAsset`, `EffectSource`, engine asset classification, editor load/save/reset/shared-update; `loading_changed_assets_refreshes_defaults_without_losing_saved_overrides`. |
| Composite child timing/reorder/duplicate/delete/playback | Working | Asset child list plus editor timeline and world grouped lifecycle; composite persistence/completion tests. |
| Composite property bindings and trigger events | Working | Typed parent parameters bind validated child property paths; event links drive child lifecycle actions. Native selectors expose compatible paths, scales/offsets, source/event/target/action tracks; document transactions persist and undo metadata. |
| Audio cue in composites | Working | Typed delayed cues resolve actual audio assets into the existing Mixer; native preview opt-in uses separate voices, playback speed, pause/stop/reset and silent scrub. Presets install generated WAV resources through `install_builtin_effect_resources`. |
| Curves/gradients and key editing | Working | Scalar `Curve`, `Gradient`, `ScalarParameter`; egui graph/key editor and key-drag test. |
| Random between two gradients; vector curves | Missing | No authored dual-gradient/vector-curve type with editor and evaluation path located. |
| Existing undo/redo/save/reload/duplicate/delete | Working | Existing `SceneEdit::SetEffect`; persistence/undo tests. |
| Play/stop authoring restoration | Working | Preview uses separate scene runtime; preview scrub and stop/sub-emitter tests preserve authored document. |
| Gizmos | Working | Native upstream transform-gizmo has inspector-selectable entity, emission/volume size, beam start/end and force/wind direction targets. Handles respect parenting and endpoint target coordinates; validated previews commit one undo transaction or restore on Escape. Matching sphere/cylinder/cone/box guides and wind arrows visualize geometry. |
| Inspector resource/object pickers and validation | Working | Recursive typed widgets, asset/entity choices, valid composite binding selectors, explicit optional numeric/vector controls and model validation; every-type native widget roundtrip test. |
| Reusable PBR material assets | Working | `.material` typed importer, stable map/ORM IDs, source and instance controls, native source editor/Explorer Open, undoable assignment, source-preserving overrides, texture modification cache invalidation and wind opt-in. |
| Public scripting integration | Working | Uses existing `world.effect`, `world.setEffect`, `world.controlEffect`, `world.effectState` bridge and SDK wrappers; per-language full live-runtime coverage still needs expansion. |
| CPU simulation and bounded event/sub-emitter recursion | Working | Retained state, 8192 event budget, world asset resolver/generation and spawn budgets; bounded sub-emitter integration test. |
| CPU pooling across entity destruction | Working | `EffectRuntime.particle_pool` retains at most 64 cleared buffers and 100,000 total slots, reuses storage after removal/disappearance, and releases it on unload. Pooling pointer/capacity reuse, RNG determinism and unload-release regression passes in the 39-test CPU suite. |
| GPU particle simulation | Working | Persistent compute/storage and direct HDR billboard draws are capability-gated by compute/vertex-storage flags and limits. Four native Vulkan tests pass: shader/ABI, CPU motion parity and lifecycle/ACKs, collision parity, actual HDR pixel readback with independent capture views/layers/render distances. Browser/WebGL uses CPU fallback; no compute support is advertised there. |
| Mesh particle rendering | Working | Existing PBR mesh/material extraction and batched procedural cube fallback; needs weighted meshes/3D variation/shadow control above. |
| HDR/PBR pipeline | Working | Scene, atmosphere, temporal and bloom passes retain linear RGBA16F until final tone mapping. Vulkan pixel/readback regression preserves emission above 3.7 and verifies a bloom halo; material, light, depth and reflection tests cover retained PBR paths. |
| Capability fallback | Partial | CPU particle/render path works without compute; fog uses bounded fragment raymarch. Actual browser/WebGL fallback rendering has not been visually verified here. |
| Preset library | Partial | Named editable preset library exists; no captured visual approval/evidence for every required preset, material, texture and category. |
| Actual runtime showcase | Working | `examples/scene-effects/project`; generated through engine-project example. Every-type visual quality and interaction acceptance remains unverified. |
| Documentation | Partial | Scene-effects overview/reference/research and website guides exist; update claims to this matrix and expose all completed typed fields/limitations. |
| Agent plugin guidance/tool updates | Working | `engine-scripting/src/agent_effects.md` and agent workspace/tool context integration; retain verification against final API. |
| Performance measurement | Partial | CPU debug smoke: 10,000 particles ×120 frames in 1.552 s (~12.93 ms/frame), excluding GPU rendering. Browser GPU memory/frame performance and repeated spawn/destruction profiling are still needed. |
| Tests/build/lint | Partial | Simulation tests passed 15 plus explicit performance smoke before follow-on changes. Global coordinator owns current full-workspace check/test/strict Clippy results. Never infer final green status from this snapshot. |
| Windows installer | Missing | Required Windows PowerShell installer has not been produced in this Linux environment; a fresh `dist/RusticGameEngine-Setup-*.exe` is required by repository instructions. |

## Final validation remaining

1. Complete the pending soft-depth/sorting and final authored-mode regressions listed in individual rows.
2. Capture real exported WebGL lifecycle and all-35-preset output. Retain the screenshot/measurement artifacts; source serialization alone is insufficient.
3. Run the final full-workspace checks, tests, strict Clippy and dependency checks against the same final sources.
4. Regenerate public references after the final typed fields and rerun the standalone documentation checks.
5. Produce the required fresh Windows installer before reporting full repository completion.
