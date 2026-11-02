# Effect configuration reference

The table names match the typed scene configuration and scripting patches exactly. See [Scene Effects](/docs/guides/scene-effects) for attachment, tutorials, units, renderer support and limitations. Optional references default to `None` (null in JavaScript); vectors and colors use arrays. `Default::default()` refers to the shared default configuration described in that section. Values below are Rust default expressions; they are not JSON request syntax.

Schema presence does not promise an advanced backend capability. Use the compatibility guide before relying on GPU simulation, water solvers, probe capture or temporal features.

## Shared lifecycle

Configuration type: `EffectCommon`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `parameters` | `std::collections::BTreeMap<String, EffectParameter>` | `Default::default()` | parameters |
| `bounds` | `Option<EffectBounds>` | `None` | bounds |
| `enabled` | `bool` | `true` | enabled |
| `visible` | `bool` | `true` | visible |
| `auto_play` | `bool` | `true` | auto play |
| `looping` | `bool` | `true` | looping |
| `duration` | `f32` | `5.0` | seconds |
| `delay` | `f32` | `0.0` | seconds |
| `time_scale` | `f32` | `1.0` | clock multiplier |
| `simulation_space` | `SimulationSpace` | `SimulationSpace::World` | simulation space |
| `attachment_target` | `Option<EntityId>` | `None` | attachment target |
| `visibility_layers` | `u32` | `u32::MAX` | unsigned bit mask |
| `render_distance` | `f32` | `1000.0` | scene units |
| `quality` | `QualityLevel` | `QualityLevel::Medium` | quality |
| `random_seed` | `u64` | `1` | deterministic unsigned seed |
| `priority` | `i32` | `0` | priority |

## Shared appearance

Configuration type: `EffectAppearance`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `color` | `Gradient` | `Gradient::default()` | RGBA linear color |
| `opacity` | `Curve` | `Curve::linear(1.0, 0.0)` | opacity |
| `size` | `Curve` | `Curve::constant(1.0)` | size |
| `material` | `Option<AssetId>` | `None` | material |
| `texture` | `Option<AssetId>` | `None` | texture |
| `emission` | `f32` | `1.0` | emission |
| `blend` | `BlendMode` | `BlendMode::Alpha` | blend |
| `uv_scale` | `[f32; 2]` | `[1.0; 2]` | uv scale |
| `uv_scroll` | `[f32; 2]` | `[0.0; 2]` | UV units/second |

## Burst

Configuration type: `Burst`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `time` | `f32` | `0.0` | time |
| `count` | `u32` | `20` | count |
| `probability` | `f32` | `1.0` | probability |
| `cycles` | `u32` | `1` | cycles |
| `interval` | `f32` | `1.0` | interval |

## 1. Trail

Configuration type: `TrailRenderer`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `emitting` | `bool` | `true` | emitting |
| `lifetime` | `f32` | `1.0` | seconds or [minimum, maximum] seconds |
| `width` | `f32` | `0.2` | scene units |
| `min_vertex_distance` | `f32` | `0.05` | min vertex distance |
| `max_points` | `u32` | `256` | max points |
| `teleport_distance` | `f32` | `20.0` | teleport distance |
| `camera_facing` | `bool` | `true` | camera facing |
| `appearance` | `EffectAppearance` | `EffectAppearance::default()` | appearance |

## 2. Particle

Configuration type: `ParticleEmitter`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `prewarm` | `bool` | `false` | prewarm |
| `prewarm_time` | `f32` | `5.0` | prewarm time |
| `max_particles` | `u32` | `1000` | max particles |
| `rate` | `f32` | `30.0` | particles/second |
| `rate_over_distance` | `f32` | `0.0` | particles/scene unit |
| `bursts` | `Vec<Burst>` | `vec![]` | bursts |
| `shape` | `SpawnShape` | `SpawnShape::Cone` | shape |
| `dimensions` | `[f32; 3]` | `[1.0; 3]` | scene units, XYZ |
| `surface_only` | `bool` | `false` | surface only |
| `lifetime` | `[f32; 2]` | `[1.0, 2.0]` | seconds or [minimum, maximum] seconds |
| `speed` | `[f32; 2]` | `[1.0, 3.0]` | scene units/second |
| `direction` | `[f32; 3]` | `[0.0, 1.0, 0.0]` | direction |
| `start_size` | `[f32; 2]` | `[0.1, 0.25]` | start size |
| `gravity` | `[f32; 3]` | `[0.0, -1.0, 0.0]` | gravity |
| `force` | `[f32; 3]` | `[0.0; 3]` | force |
| `drag` | `f32` | `0.1` | drag |
| `wind_influence` | `f32` | `1.0` | wind influence |
| `noise` | `f32` | `0.0` | noise |
| `angular_velocity` | `f32` | `0.0` | angular velocity |
| `inherit_velocity` | `f32` | `0.0` | inherit velocity |
| `velocity_limit` | `f32` | `1000.0` | velocity limit |
| `appearance` | `EffectAppearance` | `EffectAppearance::default()` | appearance |
| `mesh` | `Option<AssetId>` | `None` | mesh |
| `mesh_spawn_triangles` | `Vec<[[f32; 3]; 3]>` | `Vec::new()` | mesh spawn triangles |
| `collision` | `bool` | `false` | collision |
| `collision_layers` | `u32` | `u32::MAX` | unsigned bit mask |
| `bounce` | `f32` | `0.5` | bounce |
| `friction` | `f32` | `0.1` | friction |
| `collision_response` | `CollisionResponse` | `CollisionResponse::Bounce` | collision response |
| `sub_emitter` | `Option<AssetId>` | `None` | sub emitter |
| `sub_emitter_probability` | `f32` | `1.0` | sub emitter probability |
| `max_generations` | `u8` | `3` | max generations |
| `flipbook` | `[u32; 2]` | `[1, 1]` | flipbook |
| `flipbook_fps` | `f32` | `0.0` | flipbook fps |
| `stretch` | `f32` | `0.0` | stretch |

## 3. Beam

Configuration type: `BeamRenderer`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `start_target` | `Option<EntityId>` | `None` | start target |
| `end_target` | `Option<EntityId>` | `None` | end target |
| `start` | `[f32; 3]` | `[0.0; 3]` | start |
| `end` | `[f32; 3]` | `[0.0, 3.0, 0.0]` | end |
| `width` | `f32` | `0.1` | scene units |
| `segments` | `u32` | `24` | segments |
| `noise_amplitude` | `f32` | `0.2` | noise amplitude |
| `noise_frequency` | `f32` | `3.0` | noise frequency |
| `noise_speed` | `f32` | `5.0` | noise speed |
| `branch_count` | `u32` | `0` | branch count |
| `appearance` | `EffectAppearance` | `EffectAppearance::default()` | appearance |

## 4. Shockwave

Configuration type: `RingEffect`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `target` | `Option<EntityId>` | `None` | target |
| `source_velocity` | `[f32; 3]` | `[0.0; 3]` | source velocity |
| `wake_angle` | `f32` | `0.6` | wake angle |
| `foam_intensity` | `f32` | `0.5` | foam intensity |
| `foam_lifetime` | `f32` | `1.0` | foam lifetime |
| `normal_strength` | `f32` | `1.0` | normal strength |
| `max_disturbances` | `u32` | `32` | max disturbances |
| `initial_radius` | `f32` | `0.1` | initial radius |
| `final_radius` | `f32` | `4.0` | final radius |
| `thickness` | `f32` | `0.2` | thickness |
| `vertical_offset` | `f32` | `0.01` | vertical offset |
| `amplitude` | `f32` | `0.1` | amplitude |
| `wavelength` | `f32` | `1.0` | wavelength |
| `damping` | `f32` | `1.0` | damping |
| `appearance` | `EffectAppearance` | `EffectAppearance::default()` | appearance |

## 5. MeshParticle

Configuration type: `ParticleEmitter`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `prewarm` | `bool` | `false` | prewarm |
| `prewarm_time` | `f32` | `5.0` | prewarm time |
| `max_particles` | `u32` | `1000` | max particles |
| `rate` | `f32` | `30.0` | particles/second |
| `rate_over_distance` | `f32` | `0.0` | particles/scene unit |
| `bursts` | `Vec<Burst>` | `vec![]` | bursts |
| `shape` | `SpawnShape` | `SpawnShape::Cone` | shape |
| `dimensions` | `[f32; 3]` | `[1.0; 3]` | scene units, XYZ |
| `surface_only` | `bool` | `false` | surface only |
| `lifetime` | `[f32; 2]` | `[1.0, 2.0]` | seconds or [minimum, maximum] seconds |
| `speed` | `[f32; 2]` | `[1.0, 3.0]` | scene units/second |
| `direction` | `[f32; 3]` | `[0.0, 1.0, 0.0]` | direction |
| `start_size` | `[f32; 2]` | `[0.1, 0.25]` | start size |
| `gravity` | `[f32; 3]` | `[0.0, -1.0, 0.0]` | gravity |
| `force` | `[f32; 3]` | `[0.0; 3]` | force |
| `drag` | `f32` | `0.1` | drag |
| `wind_influence` | `f32` | `1.0` | wind influence |
| `noise` | `f32` | `0.0` | noise |
| `angular_velocity` | `f32` | `0.0` | angular velocity |
| `inherit_velocity` | `f32` | `0.0` | inherit velocity |
| `velocity_limit` | `f32` | `1000.0` | velocity limit |
| `appearance` | `EffectAppearance` | `EffectAppearance::default()` | appearance |
| `mesh` | `Option<AssetId>` | `None` | mesh |
| `mesh_spawn_triangles` | `Vec<[[f32; 3]; 3]>` | `Vec::new()` | mesh spawn triangles |
| `collision` | `bool` | `false` | collision |
| `collision_layers` | `u32` | `u32::MAX` | unsigned bit mask |
| `bounce` | `f32` | `0.5` | bounce |
| `friction` | `f32` | `0.1` | friction |
| `collision_response` | `CollisionResponse` | `CollisionResponse::Bounce` | collision response |
| `sub_emitter` | `Option<AssetId>` | `None` | sub emitter |
| `sub_emitter_probability` | `f32` | `1.0` | sub emitter probability |
| `max_generations` | `u8` | `3` | max generations |
| `flipbook` | `[u32; 2]` | `[1, 1]` | flipbook |
| `flipbook_fps` | `f32` | `0.0` | flipbook fps |
| `stretch` | `f32` | `0.0` | stretch |

## 6. Ribbon

Configuration type: `RibbonRenderer`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `points` | `Vec<[f32; 3]>` | `vec![[0.0; 3], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0]]` | points |
| `width` | `f32` | `0.2` | scene units |
| `tube` | `bool` | `false` | tube |
| `radial_segments` | `u32` | `8` | radial segments |
| `subdivisions` | `u32` | `8` | subdivisions |
| `twist` | `f32` | `0.0` | twist |
| `appearance` | `EffectAppearance` | `EffectAppearance::default()` | appearance |

## 7. ForceField

Configuration type: `ForceField`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `shape` | `VolumeShape` | `VolumeShape::Sphere` | shape |
| `dimensions` | `[f32; 3]` | `[5.0; 3]` | scene units, XYZ |
| `force_type` | `ForceType` | `ForceType::Directional` | force type |
| `direction` | `[f32; 3]` | `[1.0, 0.0, 0.0]` | direction |
| `strength` | `f32` | `1.0` | strength |
| `falloff` | `f32` | `1.0` | falloff |
| `turbulence` | `f32` | `0.0` | turbulence |
| `frequency` | `f32` | `1.0` | cycles/second |
| `layers` | `u32` | `u32::MAX` | unsigned bit mask |
| `global` | `bool` | `false` | global |

## 8. CollisionVolume

Configuration type: `CollisionVolume`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `shape` | `VolumeShape` | `VolumeShape::Plane` | shape |
| `dimensions` | `[f32; 3]` | `[10.0; 3]` | scene units, XYZ |
| `layers` | `u32` | `u32::MAX` | unsigned bit mask |
| `bounce` | `f32` | `0.5` | bounce |
| `friction` | `f32` | `0.1` | friction |
| `response` | `CollisionResponse` | `CollisionResponse::Bounce` | response |
| `impact_effect` | `Option<AssetId>` | `None` | impact effect |

## 9. ProjectileTracer

Configuration type: `ProjectileTracer`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `velocity` | `[f32; 3]` | `[0.0, 0.0, 10.0]` | velocity |
| `length` | `f32` | `2.0` | length |
| `width` | `f32` | `0.05` | scene units |
| `appearance` | `EffectAppearance` | `EffectAppearance::default()` | appearance |

## 10. Fog

Configuration type: `FogVolume`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `shape` | `VolumeShape` | `VolumeShape::Box` | shape |
| `dimensions` | `[f32; 3]` | `[10.0, 3.0, 10.0]` | scene units, XYZ |
| `density` | `f32` | `0.1` | density |
| `color` | `[f32; 3]` | `[0.6, 0.7, 0.8]` | RGBA linear color |
| `height_falloff` | `f32` | `0.5` | height falloff |
| `edge_falloff` | `f32` | `1.0` | edge falloff |
| `noise_scale` | `f32` | `1.0` | noise scale |
| `noise_speed` | `f32` | `0.1` | noise speed |
| `anisotropy` | `f32` | `0.0` | anisotropy |
| `steps` | `u32` | `32` | steps |

## 11. Decal

Configuration type: `DecalProjector`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `dimensions` | `[f32; 3]` | `[1.0, 1.0, 1.0]` | scene units, XYZ |
| `appearance` | `EffectAppearance` | `EffectAppearance::default()` | appearance |
| `normal_map` | `Option<AssetId>` | `None` | normal map |
| `orm_map` | `Option<AssetId>` | `None` | orm map |
| `roughness` | `f32` | `0.8` | roughness |
| `metallic` | `f32` | `0.0` | metallic |
| `edge_fade` | `f32` | `0.1` | edge fade |
| `angle_fade` | `f32` | `0.5` | angle fade |
| `layers` | `u32` | `u32::MAX` | unsigned bit mask |

## 12. Weather

Configuration type: `WeatherVolume`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `weather` | `WeatherType` | `WeatherType::Rain` | weather |
| `emitter` | `ParticleEmitter` | `Box, [20,10,20], downward [8,10], rate 200, lifetime [2,3]` | emitter |
| `intensity` | `f32` | `1.0` | intensity |
| `camera_follow` | `bool` | `false` | camera follow |

## 13. WaterRipple

Configuration type: `RingEffect`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `target` | `Option<EntityId>` | `None` | target |
| `source_velocity` | `[f32; 3]` | `[0.0; 3]` | source velocity |
| `wake_angle` | `f32` | `0.6` | wake angle |
| `foam_intensity` | `f32` | `0.5` | foam intensity |
| `foam_lifetime` | `f32` | `1.0` | foam lifetime |
| `normal_strength` | `f32` | `1.0` | normal strength |
| `max_disturbances` | `u32` | `32` | max disturbances |
| `initial_radius` | `f32` | `0.1` | initial radius |
| `final_radius` | `f32` | `4.0` | final radius |
| `thickness` | `f32` | `0.2` | thickness |
| `vertical_offset` | `f32` | `0.01` | vertical offset |
| `amplitude` | `f32` | `0.1` | amplitude |
| `wavelength` | `f32` | `1.0` | wavelength |
| `damping` | `f32` | `1.0` | damping |
| `appearance` | `EffectAppearance` | `EffectAppearance::default()` | appearance |

## 14. Wind

Configuration type: `ForceField`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `shape` | `VolumeShape` | `VolumeShape::Sphere` | shape |
| `dimensions` | `[f32; 3]` | `[5.0; 3]` | scene units, XYZ |
| `force_type` | `ForceType` | `ForceType::Directional` | force type |
| `direction` | `[f32; 3]` | `[1.0, 0.0, 0.0]` | direction |
| `strength` | `f32` | `1.0` | strength |
| `falloff` | `f32` | `1.0` | falloff |
| `turbulence` | `f32` | `0.0` | turbulence |
| `frequency` | `f32` | `1.0` | cycles/second |
| `layers` | `u32` | `u32::MAX` | unsigned bit mask |
| `global` | `bool` | `false` | global |

## 15. HeatHaze

Configuration type: `DistortionEffect`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `strength` | `f32` | `0.01` | strength |
| `noise_scale` | `f32` | `10.0` | noise scale |
| `speed` | `f32` | `1.0` | scene units/second |
| `radius` | `f32` | `3.0` | scene units |
| `edge_falloff` | `f32` | `1.0` | edge falloff |

## 16. LightShaft

Configuration type: `LightShaft`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `light` | `Option<EntityId>` | `None` | light |
| `intensity` | `f32` | `0.5` | intensity |
| `color` | `[f32; 3]` | `[1.0, 0.9, 0.6]` | RGBA linear color |
| `density` | `f32` | `0.1` | density |
| `anisotropy` | `f32` | `0.5` | anisotropy |
| `max_distance` | `f32` | `100.0` | max distance |
| `samples` | `u32` | `32` | samples |

## 17. FoliageScatter

Configuration type: `FoliageScatter`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `meshes` | `Vec<AssetId>` | `vec![]` | meshes |
| `density` | `f32` | `1.0` | density |
| `dimensions` | `[f32; 3]` | `[10.0, 1.0, 10.0]` | scene units, XYZ |
| `scale` | `[f32; 2]` | `[0.5, 1.5]` | scale |
| `max_instances` | `u32` | `1000` | max instances |
| `wind_response` | `f32` | `1.0` | wind response |
| `appearance` | `EffectAppearance` | `EffectAppearance::default()` | appearance |

## 18. Dissolve

Configuration type: `DissolveEffect`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `target` | `Option<EntityId>` | `None` | target |
| `progress` | `f32` | `0.0` | progress |
| `edge_width` | `f32` | `0.05` | edge width |
| `edge_color` | `[f32; 3]` | `[1.0, 0.4, 0.0]` | edge color |
| `edge_emission` | `f32` | `3.0` | edge emission |
| `noise_scale` | `f32` | `10.0` | noise scale |
| `curve` | `Curve` | `Curve::linear(0.0, 1.0)` | curve |

## 19. AnimatedSurface

Configuration type: `AnimatedSurface`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `target` | `Option<EntityId>` | `None` | target |
| `uv_scroll` | `[f32; 2]` | `[0.2, 0.0]` | UV units/second |
| `uv_scale` | `[f32; 2]` | `[1.0; 2]` | uv scale |
| `uv_rotation` | `f32` | `0.0` | uv rotation |
| `color` | `Gradient` | `Gradient::default()` | RGBA linear color |
| `emission` | `f32` | `1.0` | emission |
| `speed` | `f32` | `1.0` | scene units/second |

## 20. LensFlare

Configuration type: `LensFlare`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `light` | `Option<EntityId>` | `None` | light |
| `intensity` | `f32` | `1.0` | intensity |
| `scale` | `f32` | `0.1` | scale |
| `color` | `[f32; 3]` | `[1.0, 0.8, 0.5]` | RGBA linear color |
| `ghosts` | `u32` | `4` | ghosts |
| `ghost_spacing` | `f32` | `0.2` | ghost spacing |
| `streak_length` | `f32` | `0.3` | streak length |
| `fade_speed` | `f32` | `5.0` | fade speed |
| `texture` | `Option<AssetId>` | `None` | texture |

## 21. LightPulse

Configuration type: `LightPulse`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `target` | `Option<EntityId>` | `None` | target |
| `intensity` | `Curve` | `Curve::linear(0.0, 1.0)` | intensity |
| `color` | `Gradient` | `Gradient::default()` | RGBA linear color |
| `frequency` | `f32` | `1.0` | cycles/second |
| `randomness` | `f32` | `0.0` | randomness |
| `phase` | `f32` | `0.0` | phase |

## 22. PostProcess

Configuration type: `PostProcessVolume`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `global` | `bool` | `true` | global |
| `dimensions` | `[f32; 3]` | `[10.0; 3]` | scene units, XYZ |
| `blend_distance` | `f32` | `1.0` | blend distance |
| `weight` | `f32` | `1.0` | weight |
| `exposure` | `f32` | `0.0` | exposure |
| `saturation` | `f32` | `1.0` | saturation |
| `contrast` | `f32` | `1.0` | contrast |
| `brightness` | `f32` | `0.0` | brightness |
| `tint` | `[f32; 3]` | `[1.0; 3]` | tint |
| `bloom` | `f32` | `0.0` | bloom |
| `vignette` | `f32` | `0.0` | vignette |
| `chromatic_aberration` | `f32` | `0.0` | chromatic aberration |
| `grain` | `f32` | `0.0` | grain |
| `depth_of_field` | `f32` | `0.0` | depth of field |

## 23. Outline

Configuration type: `OutlineEffect`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `targets` | `Vec<EntityId>` | `vec![]` | targets |
| `color` | `[f32; 4]` | `[1.0, 0.6, 0.0, 1.0]` | RGBA linear color |
| `thickness` | `f32` | `0.03` | thickness |
| `pulse_speed` | `f32` | `0.0` | pulse speed |
| `through_walls` | `bool` | `false` | through walls |

## 24. CameraShake

Configuration type: `CameraShake`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `target` | `Option<EntityId>` | `None` | target |
| `position_amplitude` | `[f32; 3]` | `[0.1; 3]` | scene units, XYZ |
| `rotation_amplitude` | `[f32; 3]` | `[0.02; 3]` | radians, XYZ |
| `frequency` | `f32` | `20.0` | cycles/second |
| `falloff` | `Curve` | `Curve::linear(1.0, 0.0)` | falloff |
| `radius` | `f32` | `20.0` | scene units |

## 25. ScreenFlash

Configuration type: `ScreenFlash`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `target` | `Option<EntityId>` | `None` | target |
| `color` | `[f32; 4]` | `[1.0, 0.0, 0.0, 1.0]` | RGBA linear color |
| `opacity` | `f32` | `0.5` | opacity |
| `fade_in` | `f32` | `0.05` | fade in |
| `fade_out` | `f32` | `0.4` | fade out |
| `vignette` | `f32` | `0.5` | vignette |
| `blend` | `BlendMode` | `BlendMode::Alpha` | blend |

## 26. ReflectionProbe

Configuration type: `ReflectionProbe`.

| Field | Type | Default | Units / interpretation |
| --- | --- | --- | --- |
| `dimensions` | `[f32; 3]` | `[10.0; 3]` | scene units, XYZ |
| `cubemap` | `Option<AssetId>` | `None` | cubemap |
| `resolution` | `u32` | `128` | resolution |
| `refresh` | `ProbeRefresh` | `ProbeRefresh::Baked` | refresh |
| `blend_distance` | `f32` | `1.0` | blend distance |
| `update_interval` | `f32` | `1.0` | update interval |
| `parallax_correction` | `bool` | `false` | parallax correction |

## Source asset identity

`EffectComponent.source` optionally stores `{asset, child}`: the existing stable asset UUID and zero-based child index. Embedded configuration is the instance override. Restore resolves the source through its adjacent `.rmeta` identity and reapplies the selected child defaults. Keep the sidecar when moving or renaming an asset; replacing the UUID breaks existing references. Runtime imports use importer `rustic.effect`, version 1.

## Validated limits

Effect assets support version 1, one to 128 children, and at most 4 MiB. Duration must be positive; delay, speed and distance must be finite and nonnegative. Particle capacity is bounded at 100,000 and burst arrays at 1024. Particle lifetimes must be positive and ordered. Trail capacity is 2 to 65,536 points; beam segments are 1 to 4096. Keep curve and gradient values finite. The runtime applies additional safety bounds to generated geometry, simulation and event recursion.

## Composite child metadata

EffectAsset version 1 optionally stores `entity_ids` and `transforms`, each empty or the same length as `effects`. IDs must be unique; transforms are column-major finite nonsingular TRS matrices relative to the group. `instances_with_ids(asset_id)` creates independent child snapshots, remaps internal targets to fresh IDs and records the remapped source baseline. Missing metadata preserves legacy identity transforms.
