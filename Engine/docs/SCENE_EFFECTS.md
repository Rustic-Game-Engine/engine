# Scene Effects and VFX

Scene effects are authored entity components. They use the existing hierarchy, transforms, scene persistence and undo history. An entity has at most one effect component; use child entities to combine effects. Configuration is saved; live particles and playback clocks are transient.

This guide describes the native editor and Play path, with capability-gated GPU particles and CPU fallback. The compatibility section describes renderer and browser limits.

## Add and preview an effect

1. Open a project and scene in the editor.
2. Use hierarchy **+ Add → Effects** to create an effect entity, or select an entity and open **Properties → Scene effects → + Add / replace effect**.
3. Select the new effect entity. Expand **Common** for playback, seed, duration and visibility, then the effect-specific section for simulation and appearance.
4. Open **Properties → Effect preview** to play, pause, resume, stop, restart, reset or step by 1/60 second. Preview speed ranges from 0 to 4; time scrubbing replays up to 30 seconds, and bounds/metrics are available. Use the Inspector gizmo selector for **Entity transform**, **Emission-volume size**, **Beam start**, **Beam end** or **Force-wind direction**. These use the existing transform-gizmo handles; releasing records one undo transaction and Escape restores the original. Enable preview audio explicitly to hear composite cues; scrubbing stays silent. An emitting trail needs movement before it creates a ribbon. Preview runs a scratch SceneWorld through the same simulation path, including registered child-asset sub-emitters. Stopping preview clears transient child groups.
5. Save the scene, close it and reopen it. The configuration returns; playback starts from the configured initial state.

Open **Effects presets & assets**, search the built-in library and click a preset to instantiate a group under the selection. Drag a preset onto the viewport or hierarchy to place it. Use **Load .effect** or **Save selection .effect** for custom assets. The preset is editable. For a selected single-effect component, **Assign single-effect asset…** changes its source. Composite assets are instantiated through the asset browser. Configure entity references such as a light pulse's target or a beam's endpoint; effects requiring targets cannot infer your intended object.

## Common properties

Distances use scene world units, times use seconds, rotations use radians, and rates use units per second unless stated otherwise. Entity transforms provide position, rotation, scale and parenting. Optional entity/asset references are stable UUIDs; do not substitute OS file paths for an asset identifier.

| Property | Type | Default | Meaning |
| --- | --- | --- | --- |
| enabled / visible | boolean | true / true | Enable simulation and render visibility |
| auto_play | boolean | true | Start a newly encountered component automatically |
| looping | boolean | true | Repeat the duration |
| duration / delay | number | 5 / 0 | Positive cycle duration and initial delay, seconds |
| time_scale | number | 1 | Nonnegative effect clock multiplier |
| simulation_space | enum | World | World or Local particle coordinates |
| attachment_target | entity or null | null | Follow a scene entity |
| visibility_layers | unsigned integer | 4294967295 | Effect layer mask |
| render_distance | number | 1000 | Maximum camera distance |
| quality | enum | Medium | Low, Medium or High |
| random_seed | unsigned integer | 1 | Deterministic simulation seed |
| priority | integer | 0 | Ascending modifier order; higher priority blends last |
| parameters | typed map | empty | Named per-instance parameter bindings |
| bounds | optional bounds | null | Explicit culling region |

Simulation remains cosmetic. Do not use particle collision events for authoritative damage or gameplay physics. Replaying the same seed requires the same transforms and time steps; a seed alone does not make arbitrary frame partitions identical.

Named `common.parameters` bindings accept `Scalar`, `Vector`, `Color` or `Boolean` variants. Supported names include enabled, visible, emission_rate, intensity, color, speed, size, gravity, force, density, progress, width and radius on compatible effects. Unknown or incompatible names do not invent behavior; use a documented binding appropriate to the effect kind. Explicit bounds use a local center and positive half extents for camera culling.

## Curves and gradients

`Curve` stores `keys` with normalized `time` and scalar `value`. `Gradient` stores `keys` with normalized `time` and RGBA `color`. Evaluation interpolates linearly between surrounding keys and holds the nearest endpoint outside their range. Keep times between zero and one and use distinct times for predictable interpolation. The Inspector edits key times and values with native numeric controls, plus native color pickers for gradients. Key lists support add, duplicate, reorder and remove. The interactive curve/gradient graph supports dragging keys, double-clicking to add an interpolated key and right-clicking a key to remove it. The scalar parameter type also supports constants, ranges and a random interpolation between two curves. Color parameters can interpolate randomly between two gradients; vector curves keep separate normalized X/Y/Z channels.

Appearance configuration is shared by particles, trails and other geometry effects:

| Property | Default | Meaning |
| --- | --- | --- |
| color | opaque white gradient | Color over normalized lifetime |
| opacity | linear 1 to 0 | Alpha multiplier over lifetime |
| size | constant 1 | Size/width multiplier over lifetime |
| material / texture | null | Optional stable asset references |
| emission | 1 | Emissive strength |
| blend | Alpha | Alpha, Additive or Multiply |
| uv_scale | [1, 1] | Texture coordinate scaling |
| uv_scroll | [0, 0] | Texture movement per second |

## Particle Emitter tutorial

Create a **Particle Emitter**, keep auto-play enabled and expand the emitter settings. The Inspector groups its settings under Emission, Initialization, Movement, Appearance, Collision, Sub-emitters, and Capacity and simulation. For MeshSurface emission, first assign a mesh under Appearance; the editor extracts source triangles before validating the shape. Start with a rate of 30, maximum 1000, lifetime [1, 2] and speed [1, 3]. Cone emission, upward direction [0, 1, 0] and gravity [0, -1, 0] create a small fountain. Change the appearance gradient to orange and use Additive blending for sparks. For smoke, use Alpha blending, larger sizes and a longer lifetime.

A burst contains `time`, `count`, `probability`, `cycles` and `interval`; a zero continuous rate plus a time-zero burst creates an impulse. Disable looping to play it once. Particle collision uses explicit collision volumes, with layer masks, bounce/friction and Bounce, Stick or Kill response. A force field and a wind zone can affect multiple compatible emitters without allocating rigid bodies per particle.

For a looping emitter, `prewarm` simulates `prewarm_time` (bounded to ten seconds) before its first visible update. It uses 1/60-second steps and suppresses warmup event delivery.

Select the Rain, Snow or Leaves preset for weather. Weather reuses the particle emitter; its intensity multiplies emission. Move a local weather volume to the desired region rather than creating an unbounded scene population.

## Trail Renderer tutorial

Add a **Trail Renderer** to a moving entity or create a child of a moving object. Set lifetime to 1 second, width to 0.2, minimum vertex distance to 0.05 and maximum points to 256. Move the object in Play or while previewing. The renderer records separated points, removes expired points and builds an indexed ribbon. Changing the appearance size curve changes width along the ribbon; the opacity curve fades the tail.

A teleport farther than `teleport_distance` clears the previous strip so it does not connect unrelated positions. Stop emitting to let existing points expire. Use separate trail entities for left and right wheels. The generated showcase includes a Lua orbit behavior to exercise a moving trail.

## Effect asset and composite tutorial

An `.effect` file contains `EffectAsset { version, name, effects }`, with optional `entity_ids` and column-major `transforms` arrays for authored child identities and transforms relative to the group. Aligned light, primitive, mesh, material, texture and part-attribute arrays preserve ordinary child components. Optional `group_settings`, `sound_cues`, `parameters`, `bindings` and `trigger_bindings` describe the composite timeline. Instantiation allocates fresh child IDs and remaps references between children, preserving their relative transforms. Legacy assets without those arrays instantiate at the group origin. References to scene objects outside the asset remain external references; assign them for the destination scene. Version 1 stores an ordered list of fully typed child configurations. `common.delay` and `common.duration` give each child its position in the composite timeline. The asset reader rejects unsupported versions, empty groups, groups above 128 children and files above 4 MiB.

`presets()` returns 35 independent built-in assets; `instantiate()` clones a group's configuration. `instantiate_from(asset_id)` additionally retains the source UUID and child index; `restore(instance)` resets that child to its shared asset defaults. Editing one instance does not mutate another instance or the source asset. Explosion combines a time-zero burst, delayed smoke, an expanding shockwave, mesh debris, a light flash, camera impulse and a generated sound cue. The preset installer provides the stable-ID WAV and debris OBJ with normal asset sidecars. Object dissolve and Character highlight include self-targeting demonstration geometry. The showcase generator writes all built-in assets under `assets/effects`.

A composite's children are ordinary scene effects. Parenting preserves transform inheritance and standard duplication/deletion behavior. Reset individual instance values by reapplying the asset. Select the group and open **Scene effects → Composite timeline (seconds)** to change child enabled state, start and duration, select a child to edit, duplicate, move earlier or delete it. Select a source child for **Restore asset defaults** and **Update shared asset**. **Update shared asset** refreshes matching instances in the currently open authoring scene while preserving fields that differ from the old defaults; arrays are treated as complete overrides. That scene refresh is one undo transaction. The shared asset file write is a separate asset edit. When an asset is registered during scene loading, persisted source baselines merge current asset defaults while preserving per-field overrides; arrays remain atomic overrides. Legacy instances without a baseline retain embedded configuration until restored.

## All effect types

| # | Kind key | Intended behavior and setup |
| --- | --- | --- |
| 1 | Trail | Moving-object ribbon with bounded lifetime and point count |
| 2 | Particle | CPU particle emitter with emission, initialization, movement, appearance and collisions |
| 3 | Beam | Segmented beam/lightning between positions or scene targets |
| 4 | Shockwave | Expanding annulus aligned by its entity transform |
| 5 | MeshParticle | Particle simulation carrying a mesh asset for geometry rendering |
| 6 | Ribbon | Control-point ribbon or tube with spline subdivision |
| 7 | ForceField | Directional, attraction, repulsion, vortex or turbulence influence volume |
| 8 | CollisionVolume | Particle sphere, box, plane or cylinder interaction volume |
| 9 | ProjectileTracer | Velocity-driven visible streak |
| 10 | Fog | Localized depth-aware fog volume |
| 11 | Decal | Surface projection region with appearance configuration |
| 12 | Weather | Rain, Snow, Hail, Dust, Sand or Leaves using the particle infrastructure |
| 13 | WaterRipple | Bounded water disturbances, displaced grid/target surface, normals and wake foam |
| 14 | Wind | Global or local sampled wind influence |
| 15 | HeatHaze | Actual resolved-framebuffer UV distortion |
| 16 | LightShaft | Light scattering input linked to a light source |
| 17 | FoliageScatter | Deterministic decorative geometry distribution |
| 18 | Dissolve | Temporary target surface threshold and emissive edge modifier |
| 19 | AnimatedSurface | Temporary UV movement, color and emission modifier |
| 20 | LensFlare | Camera-projected flare ghosts and streak |
| 21 | LightPulse | Temporary light color/intensity animation; assign a light target |
| 22 | PostProcess | Priority-weighted global or local camera grading settings |
| 23 | Outline | Target object highlight; assign target entities |
| 24 | CameraShake | Additive render-camera translation and rotation impulse |
| 25 | ScreenFlash | Fading camera color overlay with vignette |
| 26 | ReflectionProbe | Authored local reflection influence and cubemap reference; see compatibility limits |

The [configuration reference](/docs/guides/scene-effects/configuration) lists the exact typed fields. A registered component does not establish support for every advanced rendering option: consult the limitations below before choosing an effect for production.

## Playback and scripting

Attach a Lua Object Component Script to an entity through **Programming → New Script**, then attach it in the Inspector. The `Effects` global extends the existing fade/flash/shake/pulse tween helpers. It does not replace them.

```lua
return {
  Start = function()
    Effects.create(nil, "Particle")
    Effects.configure(nil, {
      common = {looping = false, duration = 2.0},
      kind = {Particle = {rate = 0, bursts = {
        {time = 0, count = 50, probability = 1, cycles = 1, interval = 1}
      }}}
    })
    Effects.restart(nil)
  end
}
```

Run this on an entity that does not already have an effect. For a previously authored effect, omit `create`. `nil` means the script owner. Passing a real entity reference targets another object. Public signatures:

| Call | Result |
| --- | --- |
| Effects.types() | Array of supported, case-sensitive kind keys |
| Effects.create(entity, kind) | Creates the one effect component and returns its configuration |
| Effects.get(entity) | Configuration or null/nil |
| Effects.configure(entity, settings) | Validated nested configuration patch; returns new configuration |
| Effects.play/pause/resume/stop/restart/reset(entity) | Playback operation |
| Effects.dispose(entity) | Removes runtime state and the component |
| Effects.state(entity) | Playback state string or null/nil |
| Effects.instantiate(asset, parent, position) | Instantiated composite root entity UUID |
| Effects.spawn(asset, position, lifetime, parent) | Temporary group; positive explicit lifetime schedules group destruction |

JavaScript uses the same names and objects, with `null` instead of `nil`. Python also exposes the same `Effects` wrappers. Other generic-query adapters can use the existing `rustic.query` boundary, for example `{op:'effect_types'}`. Effect requests include `effect_create`, `effect_get`, `effect_configure`, `effect_control` and `effect_state`. Entity arguments in raw protocol requests must be stable UUIDs. Configuration uses the typed variant shape `kind: {Particle: {...}}`; unknown fields, unknown kinds and invalid values return errors before committing a scene mutation.

Runtime edits affect the live Play world. Stop Play to return to the authored scene. Use the Inspector and Save for persistent authoring. `dispose` removes the effect component, not its entity. Completion and particle birth/collision/death records are available in Rust `EffectFrame.events` and the existing Events service. Subscribe with `Events.connect(entity, "EffectCompleted", callback)` or `EffectBirth`, `EffectCollision` and `EffectDeath`; event arguments contain the world position vector. A composite root emits EffectCompleted once when its own timeline completes; its default duration includes child tails. Restarting or resetting begins a new completion cycle. `Effects.sampleWind(position, time, layers)` samples compatible wind at a position, using the supplied time in seconds for gust phase.

## Renderer compatibility and color management

The native renderer supports up to eight fog volumes, four shafts and four flares per frame. Fog raymarching uses 8–64 steps. Shafts use radial framebuffer sampling with depth occlusion and remain available with fog because the current fog model does not already produce equivalent directional shafts. Fog has height/edge density, procedural or textured noise, anisotropy and per-light scattering. Occlusion samples the available camera depth, so offscreen occluders are outside that approximation. Temporal fog/shaft history reprojects with motion/depth rejection and resets on camera cuts, resize and project changes. Motion blur uses object and camera motion vectors. Depth of field uses a 25-tap spatial blur with configurable focal distance and range.

The native wgpu renderer batches depth-tested transparent geometry and particles. It supports Alpha, Additive and Multiply effects without a separate draw call for every billboard. Empty texture slots use procedural sprites. Geometry-based effects should use the existing mesh/material conventions when selecting assets. Mesh particles use batched source geometry through the PBR path with a 16-step color palette; a missing source mesh is omitted. Custom particle sprite atlases support up to 63 textures per frame, resampled to 256-pixel layers, with flipbook UVs and velocity stretching. Decal projection clips receiver triangles and supports albedo/normal asset textures plus scalar roughness/metallicity; packed ORM, decal surface-layer filtering and explicit material-asset overrides are unavailable. Outline color and thickness use depth-occluded stencil silhouettes; through-wall outlines are unavailable.

The native scene path renders linear HDR into `Rgba16Float`; bloom thresholds scene radiance before the final tone map. Heat haze changes sampled framebuffer coordinates. Particle `simulation_mode` supports `Cpu`, `Gpu` and `Auto`. The GPU path keeps particle state in compute buffers and draws it directly; unsupported authored modules fall back to CPU with an Inspector reason. Particle event generation, sorted transparency, collisions, textures/materials and force/noise modules require CPU simulation. The showcase includes a 10,000-particle Auto station. Browser WebGL uses the CPU path.

Reflection probes capture six HDR faces in the native renderer on enable, at an interval or on explicit refresh. Captures include sky, scene meshes/lights and transparent particles; camera post effects and local fog post are excluded. Baked `.cubemap` assets describe six square HDR or LDR faces; legacy 2:1 panoramas remain supported. Probe quality controls prefilter sample count, overlap blends weighted probe input, and box parallax correction follows world transforms. Allocation is bounded to 16 maps and 1 GiB of retained probe textures. Water uses the reusable Rust `WaterSurface::sample(world_position)` interface, returning displacement, normal and foam. `RippleWaterSurface` combines a bounded queue of disturbances; `water_surface_mesh` builds a displaced grid. Configure target/source velocity, wavelength, damping, wake angle, foam and normal strength. This is an analytical cosmetic surface model, not a fluid solver. Browser export must be checked independently; the native effect renderer does not establish browser support.

## Projection, light and camera envelopes

Decals support box, sphere, cylinder and thin-plane projectors, receiver layer masks, ORM/emissive maps and a continuous distance fade before the configured render distance. Sphere/cylinder clipping uses a bounded convex approximation. `LightPulse.fade_in` and `fade_out` multiply its intensity curve using seconds within the cycle. Camera impulses expose `Additive`, `Replace` and signed-component `Maximum` blending in priority order. Their reset mode either returns to rest or holds the last active offset after natural completion until an explicit lifecycle command; authored camera transforms stay unchanged.

## Performance and cleanup

Use bounded particle/point/instance counts, shorter lifetimes and local volumes. Prefer one emitter containing many particles over thousands of effect entities. Keep transparent overdraw low: overlapping large smoke sprites and weather volumes can dominate frame time even with batching. Low quality tiers reduce cosmetic density; never use these tiers to gate gameplay damage or authoritative collisions.

World effect state is keyed by stable entity IDs. Destroying entities or unloading scenes must release their simulation state. Playback does not permanently overwrite the authored camera or light configuration; frame modifiers are applied during rendering. Reuse configured entities for repeated bursts when practical. Asset instantiation clones authored data; it is not a GPU resource duplication guarantee.

Sub-emitter `max_generations` is compared with the current absolute generation at each emitter, so a child may use a different limit from its source; the entire chain is capped at eight generations, 128 child spawns per tick and 1,024 active temporary child effects.

## Showcase and troubleshooting

From the engine's `Engine/` directory:

```sh
cargo run -p engine-project --example scene_effects_showcase -- /tmp/rustic-effects-showcase
```

The generator creates a normal project with a checksummed scene, all 26 component types, moving trail behavior and all 35 preset assets. Open that project in Project Manager. Scene entities have numbered names corresponding to the table above. Select a section and change its configuration, save and reopen to check persistence. Replay a one-shot effect with Restart.

If no particles appear, check enabled, visible, auto-play, rate/bursts, lifetime, camera distance and the layer mask. If a trail is empty, move the emitting entity farther than its minimum vertex distance. If a target effect has no result, choose a valid target of the appropriate component type. If a preset cannot load, inspect its version and validation error; do not remove a scene checksum to force-load corrupted content. If a graphical backend is unavailable, CPU/configuration tests cannot replace visual verification.
