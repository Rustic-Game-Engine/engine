# Scripting expansion audit and implementation status

This is an incremental implementation record, not a claim that the comprehensive
expansion is complete. Baseline: engine `017db99b`, docs `1a3fa6c`.

## Architecture and reuse audit

- `engine-world`: private Bevy ECS, stable UUIDs and generational runtime refs;
  validated batched commands, deterministic hierarchy and transform propagation;
  versioned scene documents and additive instantiation. Reuse these for hierarchy,
  destruction, component queries, prefab/scene foundations; preserve scene format.
- `engine-core/gameplay`: engine-owned easing, movement, tween, timers, sequences,
  paths, animation, IK, signal delivery and audio mixer. These are implemented;
  no replacement scheduler, timer, event bus, or easing implementation is needed.
- `engine-scripting`: bundled Lua 5.4/QuickJS; external isolated C/C++, Python,
  .NET, Java, PHP and Luau hosts. All use `GameplayHost.gameplay_query` and generated
  SDKs; extend this single boundary. PHP/Web remain UI-scoped. Installed toolchains
  determine external adapter availability. Snapshot state differs from live queries.
- `engine-play`: shared runtime world/context, fixed simulation, isolated scripts,
  snapshot asset reads and lifecycle cleanup. Physics is a custom primitive AABB
  solver (`physics.rs`), not Rapier/Jolt: advanced joints, ragdolls, vehicles, CCD,
  slopes and robust shape casts require a deliberate backend integration first.
- `engine-rhi`, `renderer-wgpu`: typed generational GPU resources, renderer selection,
  mesh/primitive extraction, WGSL pipelines and environment/sky. Resource presence
  is not evidence of a script API or an advanced rendering effect. Do not advertise
  volumetrics, cloth or post-processing before rendering paths exist.
- `apps/runtime/game_ui.rs`: static HTML text/CSS to SVG raster overlay. Embedded
  Web JavaScript runs separately in QuickJS. No browser DOM, forms, layout engine,
  input routing or reactive rendering bridge exists. Browser runtime integration
  is a prerequisite for Category F; state maps alone cannot implement that category.
- `engine-assets`, `engine-project`, `engine-platform`: existing import workers,
  VFS/path validation, project storage, task/logging facilities and window plumbing.
  Script saves must use a per-game writable directory, not immutable play snapshots.
- `apps/editor`, `engine-editor`: scene attachment/inspection and embedded isolated
  Play already share engine-world/runtime; no competing native UI framework needed.
- `docs`: Markdown guide/API sources, generated API reference, `docs-catalog.ts`
  navigation and API example tests. Engine-local documentation mirrors relevant pages.
- `examples`: currently contains only a license; add small runnable scripts with
  editor setup rather than fake project files or placeholders.

## Constraints and verification

No new runtime dependencies are needed for entity foundations. Existing project
serialization and public function names must stay compatible. Runtime-only tags,
groups and lifetime deadlines must be documented as such. Hierarchy preservation
must reject transforms that need shear rather than silently changing world poses.
Linux can run Rust and available language tests; Windows installer qualification
requires the repository Windows workflow. Hardware-dependent rendering remains
unverified unless executed.

## Feature-by-feature checklist

“Partial” records an existing engine substrate, not completion of the requested API.
“Not started” means no implementation was found in the inspected runtime/API paths.
The advanced physics and browser UI prerequisites above block those dependent
features; other unchecked work remains independent and not started.

| Category | Requested capability | Status |
| --- | --- | --- |
| Category A | `Tags.add()` | Implemented: Rust/runtime/bindings/docs; executed validation below |
| Category A | `Tags.remove()` | Implemented: Rust/runtime/bindings/docs; executed validation below |
| Category A | `Tags.find_all()` | Implemented: Rust/runtime/bindings/docs; executed validation below |
| Category A | `Groups.create()` | Implemented: Rust/runtime/bindings/docs; executed validation below |
| Category A | `Groups.add()` | Implemented: Rust/runtime/bindings/docs; executed validation below |
| Category A | `Groups.get_members()` | Implemented: Rust/runtime/bindings/docs; executed validation below |
| Category A | `Hierarchy.set_parent()` | Implemented: Rust/runtime/bindings/docs; executed validation below |
| Category A | `Hierarchy.get_children()` | Implemented: Rust/runtime/bindings/docs; executed validation below |
| Category A | `Hierarchy.detach()` | Implemented: Rust/runtime/bindings/docs; executed validation below |
| Category A | `Entity.destroy()` | Implemented: Rust/runtime/bindings/docs; executed validation below |
| Category A | `Entity.destroy_after()` | Implemented: Rust/runtime/bindings/docs; executed validation below |
| Category A | `Entity.is_valid()` | Implemented: Rust/runtime/bindings/docs; executed validation below |
| Category A | `Components.has()` | Implemented: Rust/runtime/bindings/docs; executed validation below |
| Category A | `Components.get_all()` | Implemented: Rust/runtime/bindings/docs; executed validation below |
| Category A | `Components.find_in_children()` | Implemented: Rust/runtime/bindings/docs; executed validation below |
| Category A | `Components.attach()` | Partial: existing substrate; requested binding/behavior missing |
| Category A | `Components.detach()` | Partial: existing substrate; requested binding/behavior missing |
| Category A | `Components.set_active()` | Partial: existing substrate; requested binding/behavior missing |
| Category A | `Prefab.instantiate()` | Partial: existing substrate; requested binding/behavior missing |
| Category A | `Prefab.override()` | Partial: existing substrate; requested binding/behavior missing |
| Category A | `Prefab.reset()` | Partial: existing substrate; requested binding/behavior missing |
| Category A | `Scene.load_additive()` | Partial: existing substrate; requested binding/behavior missing |
| Category A | `Scene.unload_additive()` | Partial: existing substrate; requested binding/behavior missing |
| Category A | `Scene.mark_persistent()` | Partial: existing substrate; requested binding/behavior missing |
| Category A | `Scene.transfer_entity()` | Partial: existing substrate; requested binding/behavior missing |
| Category A | `GameState.set()` | Not started |
| Category A | `GameState.get()` | Not started |
| Category A | `GameState.observe()` | Not started |
| Category A | `World.find_by_tag()` | Implemented: Rust/runtime/bindings/docs; executed validation below |
| Category A | `World.find_in_radius()` | Implemented: Rust/runtime/bindings/docs; executed validation below |
| Category A | `Pool.register()` | Not started |
| Category A | `Pool.acquire()` | Not started |
| Category A | `Pool.release()` | Not started |
| Category A | `Interaction.register()` | Not started |
| Category A | `Interaction.use()` | Not started |
| Category A | `Interaction.can_use()` | Not started |
| Category A | `Damage.apply()` | Not started |
| Category A | `Damage.set_resistance()` | Not started |
| Category A | `Damage.on_death()` | Not started |
| Category A | `Inventory.add_item()` | Not started |
| Category A | `Inventory.remove_item()` | Not started |
| Category A | `Inventory.transfer()` | Not started |
| Category A | `Quest.start()` | Not started |
| Category A | `Quest.complete_objective()` | Not started |
| Category A | `Quest.get_progress()` | Not started |
| Category A | `Checkpoint.activate()` | Not started |
| Category A | `Checkpoint.respawn()` | Not started |
| Category A | `Ability.activate()` | Not started |
| Category A | `Ability.set_cooldown()` | Not started |
| Category A | `Ability.can_activate()` | Not started |
| Category B | `RigidBody.set_mass()` | Partial: existing substrate; requested binding/behavior missing |
| Category B | `RigidBody.set_velocity()` | Partial: existing substrate; requested binding/behavior missing |
| Category B | `RigidBody.set_angular_velocity()` | Partial: existing substrate; requested binding/behavior missing |
| Category B | `Gravity.set_global()` | Partial: existing substrate; requested binding/behavior missing |
| Category B | `Gravity.set_for_body()` | Partial: existing substrate; requested binding/behavior missing |
| Category B | `Gravity.enable()` | Partial: existing substrate; requested binding/behavior missing |
| Category B | `PhysicsMaterial.set_friction()` | Partial: existing substrate; requested binding/behavior missing |
| Category B | `PhysicsMaterial.set_restitution()` | Partial: existing substrate; requested binding/behavior missing |
| Category B | `CollisionLayer.set()` | Partial: existing substrate; requested binding/behavior missing |
| Category B | `CollisionLayer.ignore_pair()` | Partial: existing substrate; requested binding/behavior missing |
| Category B | `on_collision_enter()` | Not started |
| Category B | `on_collision_stay()` | Not started |
| Category B | `on_collision_exit()` | Not started |
| Category B | `Trigger.on_enter()` | Not started |
| Category B | `Trigger.on_exit()` | Not started |
| Category B | `Trigger.get_overlaps()` | Not started |
| Category B | `Joint.create()` | Not started |
| Category B | `Joint.set_limits()` | Not started |
| Category B | `Joint.break()` | Not started |
| Category B | `Character.move()` | Not started |
| Category B | `Character.is_grounded()` | Not started |
| Category B | `Character.set_slope_limit()` | Not started |
| Category B | `Vehicle.set_throttle()` | Not started |
| Category B | `Vehicle.set_steering()` | Not started |
| Category B | `Vehicle.set_brake()` | Not started |
| Category B | `Buoyancy.add_body()` | Not started |
| Category B | `Buoyancy.set_density()` | Not started |
| Category B | `Ragdoll.enable()` | Not started |
| Category B | `Ragdoll.disable()` | Not started |
| Category B | `Ragdoll.blend_weight()` | Not started |
| Category B | `RigidBody.set_ccd_mode()` | Partial: existing substrate; requested binding/behavior missing |
| Category B | `Physics.box_cast()` | Partial: existing substrate; requested binding/behavior missing |
| Category B | `Physics.capsule_cast()` | Partial: existing substrate; requested binding/behavior missing |
| Category B | `Physics.set_substeps()` | Partial: existing substrate; requested binding/behavior missing |
| Category B | `Physics.get_step_stats()` | Partial: existing substrate; requested binding/behavior missing |
| Category B | `Physics2D.set_axis_lock()` | Not started |
| Category B | `Physics2D.set_gravity()` | Not started |
| Category C | `NavMesh.build()` | Not started |
| Category C | `NavMesh.find_path()` | Not started |
| Category C | `NavMesh.update()` | Not started |
| Category C | `NavAgent.set_destination()` | Not started |
| Category C | `NavAgent.stop()` | Not started |
| Category C | `NavAgent.has_arrived()` | Not started |
| Category C | `BehaviorTree.create()` | Not started |
| Category C | `BehaviorTree.run()` | Not started |
| Category C | `BehaviorTree.set_blackboard()` | Not started |
| Category C | `StateMachine.add_state()` | Not started |
| Category C | `StateMachine.transition()` | Not started |
| Category C | `StateMachine.get_state()` | Not started |
| Category C | `Perception.can_see()` | Not started |
| Category C | `Perception.can_hear()` | Not started |
| Category C | `Perception.get_targets()` | Not started |
| Category C | `AIMemory.remember()` | Not started |
| Category C | `AIMemory.recall()` | Not started |
| Category C | `AIMemory.forget()` | Not started |
| Category C | `Patrol.set_points()` | Not started |
| Category C | `Patrol.start()` | Not started |
| Category C | `Patrol.pause()` | Not started |
| Category C | `Steering.seek()` | Not started |
| Category C | `Steering.flee()` | Not started |
| Category C | `Steering.avoid()` | Not started |
| Category C | `Flock.create()` | Not started |
| Category C | `Flock.set_cohesion()` | Not started |
| Category C | `Flock.set_separation()` | Not started |
| Category C | `Cover.find_nearest()` | Not started |
| Category C | `Cover.reserve()` | Not started |
| Category C | `Cover.release()` | Not started |
| Category C | `Threat.add()` | Not started |
| Category C | `Threat.get_highest()` | Not started |
| Category C | `Threat.clear()` | Not started |
| Category C | `Spawner.configure()` | Not started |
| Category C | `Spawner.set_limit()` | Not started |
| Category C | `Spawner.spawn_wave()` | Not started |
| Category C | `WorldStream.load_region()` | Not started |
| Category C | `WorldStream.unload_region()` | Not started |
| Category C | `WorldGen.generate_chunk()` | Not started |
| Category C | `WorldGen.set_seed()` | Not started |
| Category C | `Biome.get_at()` | Not started |
| Category C | `Biome.set_region()` | Not started |
| Category C | `Biome.get_properties()` | Not started |
| Category C | `Weather.set_rain()` | Not started |
| Category C | `Weather.set_wind()` | Not started |
| Category C | `Weather.set_snow()` | Not started |
| Category C | `DayNight.set_time()` | Not started |
| Category C | `DayNight.set_cycle_speed()` | Not started |
| Category C | `Destruction.fracture()` | Not started |
| Category C | `Destruction.apply_damage()` | Not started |
| Category D | `Material.set_texture()` | Partial: existing substrate; requested binding/behavior missing |
| Category D | `Material.set_parameter()` | Partial: existing substrate; requested binding/behavior missing |
| Category D | `Shader.load()` | Not started |
| Category D | `Shader.set_uniform()` | Not started |
| Category D | `Shader.enable_keyword()` | Not started |
| Category D | `Particles.create()` | Not started |
| Category D | `Particles.emit()` | Not started |
| Category D | `Particles.set_rate()` | Not started |
| Category D | `PostFX.set_bloom()` | Not started |
| Category D | `PostFX.set_exposure()` | Not started |
| Category D | `PostFX.set_vignette()` | Not started |
| Category D | `Light.set_intensity()` | Partial: existing substrate; requested binding/behavior missing |
| Category D | `Light.set_range()` | Partial: existing substrate; requested binding/behavior missing |
| Category D | `Light.set_shadow_enabled()` | Partial: existing substrate; requested binding/behavior missing |
| Category D | `Volumetrics.set_density()` | Not started |
| Category D | `Volumetrics.set_scattering()` | Not started |
| Category D | `Mesh.create()` | Not started |
| Category D | `Mesh.set_vertices()` | Not started |
| Category D | `Mesh.set_indices()` | Not started |
| Category D | `MeshDeform.set_vertex()` | Not started |
| Category D | `MeshDeform.apply()` | Not started |
| Category D | `Terrain.set_height()` | Not started |
| Category D | `Terrain.paint_layer()` | Not started |
| Category D | `Texture.create()` | Not started |
| Category D | `Texture.set_pixels()` | Not started |
| Category D | `Texture.generate_mipmaps()` | Not started |
| Category D | `RenderTarget.create()` | Partial: existing substrate; requested binding/behavior missing |
| Category D | `RenderTarget.capture_camera()` | Partial: existing substrate; requested binding/behavior missing |
| Category D | `Decal.spawn()` | Not started |
| Category D | `Decal.set_material()` | Not started |
| Category D | `Cloth.set_wind()` | Not started |
| Category D | `Cloth.set_stiffness()` | Not started |
| Category D | `Instancing.create_batch()` | Partial: existing substrate; requested binding/behavior missing |
| Category D | `Instancing.update_instances()` | Partial: existing substrate; requested binding/behavior missing |
| Category D | `LOD.set_distances()` | Partial: existing substrate; requested binding/behavior missing |
| Category D | `LOD.set_bias()` | Partial: existing substrate; requested binding/behavior missing |
| Category D | `Occlusion.set_enabled()` | Not started |
| Category D | `Occlusion.set_occluder()` | Not started |
| Category D | `RenderSettings.set_shadow_quality()` | Partial: existing substrate; requested binding/behavior missing |
| Category D | `RenderSettings.set_antialiasing()` | Partial: existing substrate; requested binding/behavior missing |
| Category D | `Sprite.set_region()` | Partial: existing substrate; requested binding/behavior missing |
| Category D | `Sprite.set_flip()` | Partial: existing substrate; requested binding/behavior missing |
| Category D | `Sprite.set_sorting_order()` | Partial: existing substrate; requested binding/behavior missing |
| Category D | `Tilemap.set_tile()` | Not started |
| Category D | `Tilemap.fill_area()` | Not started |
| Category D | `Tilemap.get_tile()` | Not started |
| Category E | `SaveGame.write()` | Not started |
| Category E | `SaveGame.read()` | Not started |
| Category E | `SaveGame.delete()` | Not started |
| Category E | `Preferences.set()` | Not started |
| Category E | `Preferences.get()` | Not started |
| Category E | `Preferences.reset()` | Not started |
| Category E | `InputMap.register()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `InputMap.rebind()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `InputMap.get_strength()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `InputContext.push()` | Not started |
| Category E | `InputContext.pop()` | Not started |
| Category E | `Gamepad.get_axis()` | Not started |
| Category E | `Gamepad.get_button()` | Not started |
| Category E | `Gamepad.set_vibration()` | Not started |
| Category E | `Touch.get_points()` | Not started |
| Category E | `Touch.get_swipe()` | Not started |
| Category E | `Touch.get_pinch()` | Not started |
| Category E | `Mouse.capture()` | Not started |
| Category E | `Mouse.release()` | Not started |
| Category E | `Mouse.get_delta()` | Not started |
| Category E | `Assets.load_async()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `Assets.release()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `Assets.get_status()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `AssetBundle.load()` | Not started |
| Category E | `AssetBundle.unload()` | Not started |
| Category E | `Serializer.encode()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `Serializer.decode()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `Localization.translate()` | Not started |
| Category E | `Localization.set_locale()` | Not started |
| Category E | `Random.set_seed()` | Not started |
| Category E | `Random.range()` | Not started |
| Category E | `Random.weighted_choice()` | Not started |
| Category E | `Display.set_resolution()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `Display.set_fullscreen()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `Profiler.begin_sample()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `Profiler.end_sample()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `Profiler.get_results()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `Memory.get_usage()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `Memory.get_allocations()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `Debug.draw_line()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `Debug.draw_sphere()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `Debug.draw_box()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `Capture.screenshot()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `Capture.save_to_user_folder()` | Partial: existing substrate; requested binding/behavior missing |
| Category E | `Task.run_async()` | Not started |
| Category E | `Task.cancel()` | Not started |
| Category E | `Task.get_status()` | Not started |
| Category E | `Platform.get_os()` | Not started |
| Category E | `Platform.get_architecture()` | Not started |
| Category E | `Platform.is_mobile()` | Not started |
| Category E | `Accessibility.get_preferences()` | Not started |
| Category E | `Accessibility.set_subtitles()` | Not started |
| Category F | `UI.open()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.close()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.is_open()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.set_state()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.get_state()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.watch_state()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.send()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.on_message()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.focus()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.blur()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.get_focused()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.set_z_index()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.bring_to_front()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.open_modal()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.close_modal()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.capture_input()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.release_input()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UITheme.set()` | Not started |
| Category F | `UITheme.get()` | Not started |
| Category F | `UITheme.update_variable()` | Not started |
| Category F | `UI.translate()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.set_locale()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.set_viewport()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.set_scale()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `WorldUI.attach()` | Not started |
| Category F | `WorldUI.set_size()` | Not started |
| Category F | `WorldUI.face_camera()` | Not started |
| Category F | `UI.world_to_screen()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.screen_to_world()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.notify()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.dismiss_notification()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.set_loading_progress()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.set_loading_message()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.set_screen_reader_label()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.announce()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.add_class()` | Partial: existing substrate; requested binding/behavior missing |
| Category F | `UI.remove_class()` | Partial: existing substrate; requested binding/behavior missing |

## Entity-foundations slice

17 shared runtime APIs now use the existing SceneWorld command/lifetime system.
This is the first incremental slice of Phase 1; the rest of the comprehensive
request remains unfinished. Dynamic attachment, prefabs/pooling, streaming,
physics expansion, input devices, persistence, AI, rendering and browser UI are
not implemented by this slice. Do not infer their support from this checklist.

Implementation choices: runtime-only metadata avoids a project schema change;
subtree deletion uses validated batches; deadlines use generation-checked refs;
world-preserving reparent rejects shear; component queries use existing schemas.
C and C++ preserve their established error/ownership conventions. No dependencies
were added. The next independent slice should finish component lifecycle and
prefab/scene instance lifecycle before pooling; then wire fixed input and saves.
Advanced physics needs a backend decision/integration, and interactive HTML needs
an actual browser DOM runtime with a capability-limited gameplay bridge.

Validation is recorded here after execution. Missing toolchains and Windows
installer/hardware qualification remain separate from API test results.

## Executed validation (Linux)

- `cargo test -p engine-world -p engine-play -p engine-scripting --locked`:
  **137 passed**, 0 failures (30 world, 67 play, 40 scripting).
- `cargo check --workspace --all-targets --locked`: passed, including editor/runtime.
- Clippy for all targets of the three changed crates with `-D warnings`: passed.
- `cargo fmt --all --check` and Git whitespace checks: passed.
- Live foundation fixtures: Lua 5.4, Luau, JavaScript, HTML inline JavaScript,
  Python, C and C++ passed. C#, Java and PHP fixtures were **skipped** because
  those toolchains are unavailable. Their SDK code is included but unverified here.
- Built the separate bundled Luau host and put it on PATH for the Rust tests.
  This also resolved the original audio-recording test's missing-host failure.
- Both published Lua/Python example scripts executed in the real world adapter.
- Docs: lint with zero warnings, static build, TypeScript check, moderate npm audit
  (0 vulnerabilities), 17 site tests, API reference/example validation all passed.
  The example validator checks coverage/rendering (1600 existing/new snippets);
  it does not execute every documented snippet in every external toolchain.
- Local Windows installer command failed: `powershell` is absent on this Linux
  host. Windows workflow qualification is attempted separately; no fresh installer
  or Windows/macOS hardware verification is claimed by these Linux results.

## Changed files and deliverables

Engine implementation: `crates/engine-world/src/foundations.rs`, `world.rs`,
`lib.rs`; `crates/engine-play/src/entity_services.rs`, `script_runtime.rs`, `lib.rs`.
Bindings/editor hints: `crates/engine-scripting/src/lua.rs`, `cpp_sdk.rs`,
`workspace.rs`, and `sdk/{gameplay.lua,gameplay.js,rustic.h,rustic.py,rustic.php,
Rustic.cs,Rustic.java,entity-api.lua,entity-api.d.ts}`.
Engine docs: this checklist, `Scripting/API/entity-foundations.md`,
`Scripting/README.md`. Tested examples: `examples/entity-foundations/`.

Docs repository: matching checklist/API/README Markdown, `lib/api-reference.ts`,
`lib/api-reference-markdown.ts`, `lib/api-example-data.json`, `lib/api-examples.ts`,
`lib/docs-catalog.ts`. Navigation adds a setup guide, status guide and 17
individual API pages. Signatures and language differences are listed in the
[entity foundations reference](Scripting/API/entity-foundations.md).

Examples repository: `entity-foundations/{README.md,behavior.lua,behavior.py}`.
No cloud hosting, database, billing, infrastructure or other unrelated repo changed.
