# Rustic Game Engine Delivery Roadmap

Status: execution plan derived from `ARCHITECTURE.md`  
Planning unit: vertical slice with an objective acceptance gate  
Initial release target: Windows 11 x86-64; portability boundaries are enforced from
the first commit

## 1. Delivery rules

1. A milestone starts only when its dependency gate is green. Parallel research is
   allowed, but unvalidated future code is not merged into the critical path.
2. Each slice ends in a runnable executable, test, or inspected artifact. A directory
   full of interfaces with no caller is not progress.
3. Persistent formats, public plugin APIs, C ABI, and IPC start with a schema version
   and migration/compatibility test.
4. The editor must remain able to open in safe mode whenever project scripts, plugins,
   assets, or renderer initialization fail.
5. Performance budgets are measured on named reference machines. Results record scene,
   resolution, build hash, driver, median, p95, and allocations; words such as
   "optimized" are not acceptance evidence.
6. Dependencies are introduced only in the milestone that uses them and must include
   an engine-owned boundary, license record, and a tested failure path.
7. Every milestone keeps `cargo fmt --check`, Clippy with warnings denied, unit tests,
   and headless smoke tests green. Platform-specific tests are explicit, not silently
   skipped.
8. Native UI means `egui`/`winit`. Electron is not an accepted implementation or
   fallback for any milestone.

## 2. Phased roadmap

### M0 — Reproducible workspace and governance

**Outcome:** any contributor can build and test the same empty-but-runnable native
product skeleton.

- Pin the stable Rust toolchain and create the Cargo workspace, profiles, dependency
  policy, license policy, formatting, linting, and CI.
- Add `xtask doctor/build/test/run`, a foundation error type, a process entry contract,
  and a headless smoke mode.
- Add contributor-facing architecture links and decision-record conventions.

**Gate:** clean checkout passes format, Clippy, tests, license audit, and
`cargo xtask doctor`; the non-UI library/process smoke tests return success without a
display server. See section 4 for the exact first task.

### M1 — Platform foundation, observability, and launcher shell

**Outcome:** a native launcher process starts reliably and failures are diagnosable.

- Implement platform paths, VFS mount/path validation, clocks, cancellation, atomic
  file writes, configuration, async structured logging, rotation, crash markers, and
  basic CPU job scheduling.
- Open a native `winit`/`egui` Project Manager window with an empty-project state,
  settings, and clean shutdown. Keep UI model and native shell separate.
- Add the per-user project catalog schema, search/sort/recent model, import/remove
  operations, and thumbnail cache. Removing a catalog entry does not delete project
  files; deletion is a later explicit recoverable operation.

**Gate:** native launcher remains responsive during a synthetic background job;
read-only/unwritable log destinations do not crash it; path traversal tests pass;
project catalog survives atomic restart and corruption recovery.

### M2 — RHI and first rendered frame

**Outcome:** one renderer abstraction displays validated geometry without leaking
backend types into engine APIs.

- Define typed RHI descriptors/handles, surface lifecycle, command submission, shader
  reflection contract, resource retirement, and renderer diagnostics.
- Implement `renderer-wgpu`, initially DX12 on Windows with explicit Vulkan and GL
  fallback switches. Add a null/headless RHI for tests.
- Compile WGSL, render triangle then indexed mesh with depth, camera, transform, and
  resize/minimize/DPI handling. Add RenderDoc-compatible labels and CPU/GPU scopes.

**Gate:** the golden triangle and textured mesh render on at least two Windows backend
candidates in CI/manual release qualification; resize/minimize/device-init failure is
recoverable; no `wgpu` type appears outside the backend crate's public boundary test.

### M3 — World, scene graph, components, and serialization

**Outcome:** a scene can be created, transformed, saved, loaded, diffed, and migrated.

- Integrate ECS schedules and deferred structural commands.
- Implement stable entity IDs, generational runtime references, Transform hierarchy,
  Mesh/Material/Camera/Light components, cycle rejection, and dirty propagation.
- Implement deterministic versioned `.rscene` serialization, migration registry,
  corruption diagnostics, atomic saves, scene instances, and baseline undo commands.
- Extract an immutable render world and display the scene from M2.

**Gate:** round-trip/golden/migration tests are deterministic; a hierarchy cycle is
rejected without partial mutation; corrupt components yield localized diagnostics;
10,000 static transforms do not allocate after warm-up during unchanged frames.

### M4 — Asset database and import pipeline

**Outcome:** source assets receive stable IDs and are imported incrementally without
blocking the editor/render thread.

- Implement `.rmeta`, importer/version contracts, content hashes, dependency graph,
  SQLite cache index, derived-data cache, async handles, placeholder/error assets, and
  file-watch debounce.
- First importers: glTF/GLB, OBJ, PNG/JPEG/TGA/HDR, WAV/OGG, and TTF/OTF. Add EXR and
  FBX after their isolated decoder workers and corpus tests are ready.
- Implement textures, static meshes, basic PBR materials, asset moves, reimport,
  duplicate-ID quarantine, and missing-reference reports.

**Gate:** moving source plus meta preserves references; unchanged assets are not
reimported; modifying a dependency invalidates only dependents; malformed/fuzz-corpus
assets cannot crash the editor; import work does not run on the UI thread.

### M5 — Project creation and authoring editor

**Outcome:** a user can create/open an Empty 3D project in a separate, useful editor.

- Add template manifests and implement Blank, Empty 2D, and Empty 3D first; remaining
  named templates are content packages added incrementally.
- Launcher spawns a separate version-compatible editor process with project locking,
  stale-lock recovery, and read-only mode.
- Implement native docked 3D/2D viewport, hierarchy, inspector, content browser,
  console, settings, workspace persistence, detaching/multi-monitor support, selection,
  gizmos, drag/drop, and centralized undo/redo transactions.
- Add primitive Cube, Rectangular Prism, Plane, Sphere, Cylinder, Capsule, Cone, Torus,
  and 2D Rectangle/Circle/Polygon with editable dimensions.

**Gate:** create -> edit -> undo/redo -> save -> close -> reopen is lossless; an editor
process crash does not corrupt the last saved scene; layouts restore across monitors
with missing-monitor recovery; launcher remains a distinct interface/process.

### M6 — Isolated local playtesting and runtime console

**Outcome:** the authored scene runs without putting editor data or process stability
at risk.

- Implement versioned/authenticated IPC, immutable play snapshots, `rustic-runtime`,
  process-group cleanup, crash handling, and bounded frame transport.
- Add the single **Play dropdown** with Play, New Window, and Standalone. Add Stop,
  Pause, Resume, and exactly-one-fixed-tick Frame Advance.
- Automatically activate a live runtime console with severity/subsystem/language/
  process filters, search, collapse, copy, clear, timestamps, and clickable frames.
- Persist editor/runtime/crash logs, rotation/retention, overflow counters, and an
  explicit undoable runtime-change apply workflow.

**Gate:** all three modes run; Pause freezes simulation; Frame Advance increments a
test counter exactly once; Stop leaves no child; runtime crash leaves editor responsive;
source scene hash is unchanged after play unless the apply command is accepted; logs
appear both live and under the project `logs/` layout.

### M7 — Language-neutral API and first mixed-language slice

Lua and JavaScript external-editor vertical slices delivered 2026-09-09: versioned stable script
components/manifests, sandboxed runtime callbacks, engine API 1.0, workspace generation,
diagnostics/navigation, input-action foundation, and authenticated last-good reload are
implemented. The mixed-language runtime fixture and JavaScript last-good reload pass;
Luau, automatic discovery, and cross-language events remain in the original M7 gate.

**Outcome:** one scene uses Lua and JavaScript components together without selecting a
project language.

- Define the versioned scripting schema, `EngineValue`, generated binding contract,
  batched ECS queries/commands, lifecycle event batches, diagnostics, time budgets,
  state snapshots, and last-good reload transaction.
- Implement extension discovery and the Lua/Luau adapter, then the QuickJS adapter.
  Run adapters in a child host first; selectively allow hardened trusted Lua/Luau
  in-process later.
- Demonstrate a Lua component publishing a typed engine event consumed by JavaScript,
  with both manipulating components through the shared engine API rather than direct
  VM interoperability.

**Gate:** `.lua`, `.luau`, and `.js` are discovered automatically; the mixed-language
fixture passes; syntax error and failed hot reload retain last-good behavior; infinite
loop is timed out/restarted without editor failure; no project-language setting exists.

### M8 — Core gameplay services and first usable MVP

**Outcome:** Rustic 0.1 meets the usable-MVP definition in section 3.

- Implement action-mapped keyboard/mouse/gamepad input.
- Implement initial Rapier rigid/static/kinematic bodies, colliders, triggers, layers,
  raycasts, fixed-step synchronization, and debug draw behind `physics-api`.
  Basic Play gravity and enclosing-box primitive collision are already implemented
  in `engine-play`; see `PHYSICS.md`. Full rigid-body rotation, triggers, queries,
  collision callbacks, and imported-mesh physics remain future work.
- Implement WAV/OGG 2D/3D playback, source/listener, attenuation, loop, bus, volume,
  pitch, and streaming music behind `audio-api`.
- Implement native runtime labels, buttons, images, panels, text input, basic flex/grid
  containers, anchors, and responsive scale.
- Add Windows standalone cooking/export with manifest, required licenses, and a clean-
  machine smoke test.

**Gate:** every MVP acceptance item in section 3 passes on the named reference Windows
machines; a packaged sample runs without Rust, a compiler, editor files, or a project
source tree installed.

### M9 — Renderer auto-selection and adaptive quality

**Outcome:** normal users receive a safe backend and measured quality profile without
understanding graphics APIs.

- Implement hardware inventory, isolated probe executable, candidate scoring, crash
  marker/driver rules, cache invalidation, five profiles, diagnostics UI, safe mode,
  and validated manual overrides exactly as specified in `ARCHITECTURE.md` section 8.
- Establish per-profile render budgets and capability clamps. Add dynamic resolution/
  quality adaptation with percentile windows, hysteresis, cooldowns, and load-spike
  exclusion; never switch API during play.

**Gate:** deterministic inventory fixtures select expected candidates/profiles;
forced probe crash falls back; driver/GPU/display change invalidates the cache; a 20-
minute variable-load test has no rapid quality oscillation; every decision has a
machine-readable reason. The MVP contains an earlier conservative selector; this
milestone makes it production-grade.

### M10 — Modeling, materials, animation, and broader rendering

**Outcome:** common blockout and presentation workflows no longer require an external
modeler.

- Add editable half-edge (or equivalently validated) mesh topology, vertex/edge/face
  selection, extrude, inset, bevel, duplicate, merge, separate, join, slice/cut, flip/
  recalculate normals, and undoable operations.
- Add a non-destructive modifier stack for union/intersect/subtract/negate with cached
  evaluation, clear failure diagnostics, and explicit destructive bake.
- Add material graph/editor, animation timeline/state machine, particles, cascaded
  shadows, SSAO, reflections, bloom, tone mapping, TAA/FXAA/MSAA where valid,
  volumetrics, sky, instancing, culling, and LOD by measured slices.

**Gate:** topology invariant/property tests and a hostile boolean corpus pass; every
modeling edit is undoable; a failed modifier preserves source geometry; rendering
features degrade to Compatibility rather than becoming hard requirements.

### M11 — Remaining language adapters and native SDK

External-host vertical slice delivered 2026-09-09 for Python, C#, C/C++, Java, PHP,
and HTML/CSS: toolchain probing, compile-before-run, protocol starters, isolated
working directories, bounded lifecycle execution, last-good reload, Web inline-script
sandboxing, editor creation, doctor reporting, and CI conformance are implemented.
Interactive debug adapters and OS-level per-process memory quotas remain hardening work.

**Outcome:** every required source extension participates in a mixed-language project
through the same engine API.

Implement and harden adapters in this order, based on isolation/toolchain complexity:

1. Python host and generated Python bindings.
2. C# .NET host and bindings.
3. C/C++ SDK, Clang/MSVC build plans, and restart-on-reload native host.
4. Java JVM host.
5. PHP persistent host.
6. HTML/CSS/JS sandboxed native-webview content surface.

Each adapter has toolchain discovery, reproducible build report, source/stack mapping,
debug protocol, hot-reload policy, memory/time limits, fixture project, and packaging
rules before being labeled supported.

**Gate:** one conformance suite exercises values, entities, components, errors, events,
reload, timeouts, and shutdown in every adapter; one fixture contains all supported
languages simultaneously; direct pairwise VM/native ABI calls are absent.

### M12 — Plugins, build targets, and production hardening

**Outcome:** stable extension points and reliable Windows/Linux/macOS desktop releases.

- Implement plugin manifests, semantic API negotiation, capability permissions,
  Wasm host, isolated native host, registration rollback, safe-mode disable, SDK, and
  samples for panel/importer/language runtime/build target.
- Complete Linux and macOS Tier-1 ports; implement package/sign/notarization hooks,
  patchable asset packs, reproducible exports, and crash/update channels.
- The Windows installer detects and installs user-selected gameplay language
  toolchains and provides a Start-menu manager for adding them later. Continue to
  detect and automatically install missing device dependencies, add them to `PATH`
  when required, and add any required dependency
  paths to the installed project configuration.
- Add profiler, debugger adapters, accessibility pass, localization, large-project
  asset streaming, build farm hooks, Git LFS/Perforce validation, recovery exercises,
  and end-to-end performance gates.

**Gate:** API compatibility suite covers two supported plugin minor versions; failed
plugin cannot prevent safe-mode open; editor and exported sample pass clean-machine,
GPU-matrix, locale/path, suspend/resume, device-loss, and crash-recovery suites on all
Tier-1 OSes.

### M13+ — Advanced platform and rendering program

Progressively deliver GPU-driven rendering, occlusion, virtualized/streamed resources,
advanced GI, ray tracing, vendor-neutral upscaling interfaces, spatial audio, richer UI,
networking components, Android/iOS runtimes, and WebGPU/WASM research. Console work is
scheduled only after licensed platform access; no roadmap text is a support claim.

## 3. First usable MVP: Rustic 0.1

The first usable MVP is not a tech demo. It is a small end-to-end engine capable of
authoring, debugging, and exporting a simple interactive 3D scene. It completes at the
M8 gate, with the conservative portion of renderer selection brought forward from M9.

### Included

- Windows 11 x86-64 release with native Project Manager and separate native Editor.
- Create/import/open/search/remove-from-list projects; Blank, Empty 2D, and Empty 3D
  templates; stable manifest and project lock handling.
- Dockable 3D/2D viewport, hierarchy, transform/component inspector, asset browser,
  console, settings, saved workspaces, undo/redo, and primitive creation/dimensions.
- Versioned scenes with Transform hierarchy, Mesh, Material, Camera, Light, Script,
  Rigidbody, Collider, Audio Source/Listener, and basic runtime UI components.
- DX12 preferred with Vulkan and GL fallback; PBR mesh, texture, camera, directional/
  point/spot lights, depth, basic shadows, tone mapping, frustum culling, and five
  conservative quality profiles. Unsupported features degrade cleanly.
- glTF/GLB, OBJ, PNG/JPEG/TGA/HDR, WAV/OGG, and TTF/OTF import with stable asset IDs,
  asynchronous derived-data cache, reimport, and missing/error placeholders.
- Play dropdown with Play, New Window, Standalone plus Stop, Pause, Resume, and exact
  Frame Advance in an isolated child runtime. Authoring scenes cannot change silently.
- Live/filterable runtime console, clickable Lua/JavaScript errors, rotating persistent
  editor/runtime/crash logs, and recovery after a runtime or script-host crash.
- Automatic `.lua`, `.luau`, and `.js` discovery, shared engine API, mixed-language
  typed events, build diagnostics, and last-good hot reload. No project language choice.
- Action input, baseline 3D physics/raycast, 2D/3D audio, basic engine-native runtime
  UI, and a self-contained Windows standalone export.

### Objective acceptance

1. From a clean install, create Empty 3D, add/resize a primitive, assign an imported
   texture/material, add a collider and Lua behavior, and save/reopen without warnings.
2. Add a JavaScript behavior that consumes a typed event emitted by the Lua behavior;
   both run without a language setting or direct VM bridge.
3. Play in all three modes. Pause and Frame Advance are exact, Stop leaves no process,
   and discarding play keeps the source scene byte-for-byte unchanged.
4. Introduce a script syntax error, malformed image, shader error, and runtime crash in
   the test fixture. Each produces diagnostics and the editor remains usable.
5. Logs are live and persisted/rotated under the documented layout even when one log
   sink is unavailable.
6. The conservative selector chooses a validated backend/profile and falls back after
   an injected device/probe failure. Compatibility runs without optional high-end GPU
   features.
7. Export and run the sample on a clean Windows VM with no Rust toolchain or editor.
8. The release performance report meets budgets established before M8 on low,
   integrated, and mid-range discrete reference machines; no per-frame allocations
   remain in unchanged transform extraction, audio callback, or steady-state logging.

### Explicitly deferred from 0.1

Full FBX/EXR production qualification, advanced mesh booleans/topology editing, all
remaining language adapters, animation/material graphs, ray tracing/GI, production
dynamic quality, Wasm/native public plugins, Linux/macOS release support, mobile, Web,
networking, and console support are deferred, not removed from the product vision.

## 4. Exact first implementation task

### ENG-0001 — Native foundation vertical slice

**Goal:** establish the first buildable end-to-end product seam: a native Project
Manager can create/import a safe versioned project, record structured logs, choose a
renderer policy without initializing a GPU, and launch a distinct native Editor
process. Do not add ECS, asset importing, actual GPU calls, scripting, or a web UI in
this task.

**Create or complete:**

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml
.gitignore
README.md
apps/project-manager/Cargo.toml
apps/project-manager/src/main.rs
apps/project-manager/src/registry.rs
apps/editor/Cargo.toml
apps/editor/src/main.rs
crates/engine-core/Cargo.toml
crates/engine-core/src/application.rs
crates/engine-core/src/logging.rs
crates/engine-project/Cargo.toml
crates/engine-project/src/lib.rs
crates/engine-project/tests/project.rs
crates/engine-rhi/Cargo.toml
crates/engine-rhi/src/lib.rs
```

**Implementation contract:**

- The root Cargo workspace uses resolver 3, edition 2024, centralized dependency
  versions/lints/profiles, a pinned stable toolchain, and a committed lockfile. Unsafe
  Rust is forbidden in this slice.
- `engine-core` owns typed engine/application identity and a bounded asynchronous
  logger with severity, subsystem, optional language/source metadata, filtering,
  runtime/editor/crash streams, rotation/retention, latest-log handling, flush/shutdown,
  overflow counters, and tests for unavailable sinks. Logging failure cannot panic.
- `engine-project` owns `project.engine` format version 1 in RON, `ProjectId`, all eight
  requested template kinds, metadata, configurable virtual directory map, create/open/
  validate, traversal/absolute-path rejection, non-overwrite behavior, and atomic
  descriptor writes. Its integration test creates, reopens, and validates a project.
- `engine-rhi` contains only serializable backend/profile policy types and a pure,
  deterministic selector for Vulkan, Direct3D 12, Metal, OpenGL, and software recovery
  across Ultra/High/Balanced/Low/Compatibility. It performs no graphics API call and
  has fixtures for OS affinity, invalid candidate rejection, power policy, and stable
  tie-breaking.
- `rustic-project-manager` is a native `eframe`/`egui` application. It displays/searches/
  sorts the per-user project catalog, creates/imports/opens projects, removes only the
  catalog record by default, exposes all template kinds, and launches `rustic-editor`
  with an argument array and canonical project path.
- `rustic-editor` is a separate native process and interface. In this slice it validates
  the supplied project, displays project identity/path plus a foundation-status view,
  writes an editor log, and exits cleanly. It does not impersonate a complete editor.
- Catalog and project writes use temporary-file/flush/atomic-replace behavior where
  the target filesystem supports it. User-facing failures retain the prior valid data
  and remain visible in the native UI.
- Neither application starts a browser, local web server, Node process, or Electron.
  The README states the current slice honestly and documents exact quality commands.

**Commands that must pass:**

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --workspace --all-targets
cargo run -p rustic-project-manager
```

The last command is a manual native-window acceptance check; the preceding commands
must run headlessly in CI.

**Acceptance:** all non-UI commands pass from a clean checkout on Windows; creating a
project produces `project.engine` plus exactly the configured directory set without
overwriting an occupied destination; reopen preserves ID/template/metadata; malicious
virtual paths are rejected before mutation; logger rotation and unwritable-sink tests
pass; equal valid Windows candidates prefer DX12 while invalid candidates are excluded;
Open starts the separate `rustic-editor` process; closing either interface does not
corrupt the catalog/project; and the dependency graph contains no Electron/Node/npm.

**Next task after acceptance:** ENG-0002 adds `xtask doctor/test`, `cargo-deny`, Windows
and Linux CI, platform path/process abstractions, and failure-injected atomic catalog
tests. It completes the M0 gate before real renderer work begins.

## 5. Major technical risks and mitigations

| Risk | Failure mode | Mitigation and evidence required |
|---|---|---|
| Scope explosion | Many half-built systems, no usable vertical slice | Milestone gates, one critical path, WIP limit, MVP inclusions/deferred list, runnable fixture at each phase |
| Unsafe multi-language code | Script/native crash, hang, memory corruption, or malicious project execution destroys editor/data | External trust database, no execution while browsing, process isolation, capability limits, deadlines, restartable hosts, no direct pairwise ABI, hostile fixtures |
| IPC becomes a frame-time bottleneck | Per-property calls and copying stall simulation | Batched schemas/commands, immutable snapshots, shared bulk buffers, declared barriers, latency/copy counters and budgets from M6 |
| Native ABI churn | C/C++ plugins break or corrupt state after upgrade | Versioned C tables, opaque generational handles, size/version negotiation, conformance suite, native host restart, never expose Rust/C++ layout |
| Hot reload corrupts state | New code partially replaces old code or reinterprets memory | Side-by-side generations, serializable versioned state, atomic barrier swap, last-good rollback, reload fault-injection tests |
| GPU/driver diversity | Device creation, shader, device-loss, or backend crash prevents project recovery | Probe child process/watchdog, scored fallback, crash marker, compatibility tier, safe/headless recovery, GPU/driver matrix |
| Renderer abstraction is too leaky or too generic | Backend types infect assets/world, or abstraction blocks modern features | Boundary compile tests, engine descriptors/handles, capability queries/extension slots, one real backend before freezing RHI, profile on representative passes |
| Dynamic quality oscillates | Distracting visual changes and unstable frame pacing | CPU/GPU percentiles, two-threshold hysteresis, spike exclusion, cooldown, one-step changes, long variable-load test, never live-switch API |
| Cross-platform native UI gaps | Docking, DPI, IME, detached windows, or multi-monitor layouts fail | Platform UI harness, normalized logical/physical coordinates, missing-monitor recovery, IME/accessibility tests, `egui` behind editor shell model |
| Asset parser attack/corruption | Malformed FBX/image/archive crashes worker or exhausts resources | Isolated workers, size/time/memory caps, fuzz corpora, checksums, placeholder assets, ufbx/license review, never parse on UI thread |
| Stable-ID loss/duplication | Moves break references or copied sidecars alias assets | Move source+meta transaction, duplicate quarantine/remap tool, source-control hooks, broken-reference report, backups |
| Persistent format evolution | New builds destroy old scenes or old builds overwrite future data | Schema envelopes, deterministic golden files, ordered migrations on copies, read-only future versions, atomic save/backup/recovery drills |
| Logging harms gameplay or loses fatal evidence | Disk stall, queue flood, unwritable folder, recursive sink failure | Bounded async queues, severity reserve, rotation/retention, fallback OS sink, drop counters, no audio/render-thread formatting, failure injection |
| Job-system races | Nondeterminism, deadlocks, unsafe ECS mutation | Declared access, deferred structural commands, bounded queues, cancellation, Loom/model tests where useful, ThreadSanitizer-compatible native CI |
| Physics abstraction hides backend semantics | Incorrect collision behavior or expensive conversion | Define engine units/tick/order/query semantics, adapter conformance scenes, expose explicit capability differences rather than lowest-common-denominator guesses |
| Audio real-time violations | Dropouts from allocation, locks, logging, or streaming I/O | Preallocated command ring, RT-safe mixer rules, background decode, underrun counters and stress test |
| Dependency/license/toolchain drift | Shipping violation or irreproducible user scripts | Committed lockfile, `cargo-deny`, third-party notices, adapter build reports, pinned CI images, per-export toolchain/license manifest |
| Web content expands into editor architecture | Browser security/size complexity or accidental Electron adoption | Optional sandboxed OS webview only for game content, allowlisted bridge, editor stays egui/winit, architecture/CI dependency check forbids Electron |
| Plugin API freezes too early | Permanent poor API or ecosystem breakage | Delay public stabilization to M12, semantic negotiation, Wasm capabilities, experimental namespace, compatibility suite and deprecation window |
| Cache mistaken for source of truth | Lost cache loses project metadata | `.rmeta`/manifests are authoritative; SQLite/DDC is disposable and rebuild-tested in CI |
| Export differs from play | Game works in editor but lacks assets/runtimes when shipped | Standalone mode uses staged export-like mounts from M6, clean-machine package smoke, manifest completeness checks |

## 6. Release evidence and change control

Each milestone release record contains commands/results, supported target matrix,
dependency/license changes, persistent schema changes and migration tests, IPC/API
versions, performance report, known limitations, and recovery test results. An
architecture change must state the old decision, observed problem, replacement,
migration impact, and rollback plan in a short decision record linked from
`ARCHITECTURE.md`.

The next milestone is not accepted by code volume. It is accepted only when its gate
is reproducible and the prior runnable slice still works.
