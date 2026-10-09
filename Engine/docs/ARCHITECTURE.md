# Rustic Game Engine Architecture

Status: baseline architecture decision, version 1  
Audience: engine, editor, tooling, and runtime contributors  
Scope: long-term product architecture; delivery order is in `ROADMAP.md`

## 1. Product and technical decisions

Rustic is a native, multi-process 2D/3D game engine. The Project Manager, Editor,
play runtime, asset workers, and unsafe language hosts are separate processes with
versioned IPC boundaries. Projects do not choose one scripting language: source
files are discovered by extension and routed to independently versioned language
adapters.

The editor shell is native Rust using `winit` and `egui`. **Electron is explicitly
excluded** from the launcher, editor, game runtime, and build toolchain. HTML/CSS
support is an optional game-content surface hosted by a sandboxed platform webview;
it is not the editor UI and does not introduce Electron.

Decision priorities are correctness, project safety, editor stability, runtime
stability, performance, portability, maintainability, developer experience, polish,
then feature count.

### 1.1 Implementation languages

| Area | Language | Decision |
|---|---|---|
| Engine, editor, launcher, runtime, asset pipeline, CLI | Rust, stable toolchain, edition 2024 | Memory safety, deterministic cleanup, strong concurrency model, a single cross-platform workspace, and a practical native ecosystem. Unsafe code is allowed only in audited FFI/platform modules. |
| GPU shaders | WGSL as the canonical authored/intermediate language | It is portable across the initial `wgpu` backends and can be validated/reflected by Naga. Backend-specific shaders remain an escape hatch behind the RHI. |
| Native extension ABI | C ABI with versioned POD tables and opaque handles | C is the narrow compatibility ABI for C, C++, and other native languages. Rust layouts and C++ ABIs are never persisted or exposed as the stable plugin ABI. |
| Build orchestration | Rust (`xtask`) plus Cargo | Keeps platform logic testable and avoids shell-script divergence. CMake is permitted only inside a native SDK/sample or a third-party native adapter. |
| Game scripting | C, C++, C#, Python, JavaScript, Lua, Luau, Java, PHP, and HTML/CSS content | Implemented by adapters; none of these languages are dependencies of the engine core. |

Rust is the sole primary engine implementation language. C/C++ are supported as
game/native-extension languages, not as a second engine core. This prevents two
ownership models and two build graphs from spreading through every subsystem.

### 1.2 Build system

The repository is one Cargo workspace with resolver 3. All dependency versions and
features are declared in the root workspace manifest and committed in `Cargo.lock`.
The checked-in `rust-toolchain.toml` pins the supported stable toolchain; upgrades
are deliberate pull requests.

`cargo xtask` is the public build interface:

```text
cargo xtask doctor
cargo xtask build --profile dev
cargo xtask test
cargo xtask run project-manager
cargo xtask package-editor --target <triple>
cargo xtask export --project <path> --target <triple> --profile release
```

`xtask doctor` verifies the Rust target, platform SDK, shader tools, optional language
toolchains, and license policy. Cargo profiles are `dev`, `dev-fast`, `release`, and
`distribution`; engine and game export profiles are separate. CI runs formatting,
Clippy with warnings denied, unit/integration tests, license/source auditing, and a
headless smoke test. Native language adapters invoke Clang/MSVC or project-selected
toolchains through the build service, never from a render or UI thread.

### 1.3 Operating-system strategy

| Support tier | Initial targets | Promise |
|---|---|---|
| Tier 1 | Windows 11 x86-64 | Every merge is built and tested. Dedicated GPUs prefer Vulkan; integrated graphics retain DX12 preference. DX12, Vulkan, and GL remain recovery paths if the preferred candidate cannot initialize. The first usable MVP ships here. |
| Tier 1 after MVP | Linux x86-64 on current Ubuntu LTS and one rolling distribution | Every release is built/tested. Vulkan is preferred and GL compatibility is the recovery path. Wayland and X11 are both exercised through `winit`. |
| Tier 1 after MVP | macOS on Apple silicon, with x86-64 while upstream dependencies support it | Every release is built/tested. Metal is the only preferred native backend; GL is compatibility-only where the OS and `wgpu` support it. |
| Tier 2 future | Android and iOS | Runtime/export support after desktop API boundaries are stable; no editor commitment. |
| Research/partner | Web and consoles | Web may use WebGPU/WASM. Console support requires licensed SDKs and partner access and is not claimed until validated. |

Platform code is confined to `platform-*` crates. Paths are UTF-8 at public engine
boundaries but retain the native `OsString` internally. Case sensitivity, atomic
rename behavior, file watching, dynamic library naming, process control, input, and
surface creation have explicit platform tests. The engine never assumes a drive
letter, a case-insensitive filesystem, or `/` as the path separator.

## 2. Dependency policy and proposed dependencies

Major third-party libraries are wrapped by engine-owned interfaces. Versions below
are intentionally selected and pinned during the implementation milestone that
first uses them, after API and license verification; this document does not pretend
that an unintegrated version has been tested.

| Dependency | Purpose | License | Why selected | Reasonable alternatives |
|---|---|---|---|---|
| `winit` | Native windows, events, monitors, input surface integration | Apache-2.0 OR MIT | Mature cross-platform Rust window layer used by `wgpu`; supports native multi-window editors | SDL3, GLFW |
| `wgpu` + Naga | Portable GPU API, shader validation/reflection, Vulkan/DX12/Metal/GLES backends | Apache-2.0 OR MIT | Safe, actively maintained backend unification with native and future WebGPU paths | `ash` + `windows` + Metal bindings, `rend3`, `gfx-hal` |
| `egui`/`eframe` | Native immediate-mode launcher/editor UI | Apache-2.0 OR MIT | Rust-native, fast iteration, custom rendering, multi-viewport support; no browser shell | Slint, Qt, Dear ImGui bindings |
| `egui_tiles` | Dock tabs, splits, and saved panel layouts | MIT OR Apache-2.0 | Maintained tiling model hidden behind the editor shell | Custom egui docking, Dear ImGui docking |
| `bevy_ecs` | Data-oriented ECS and schedules | Apache-2.0 OR MIT | Proven parallel ECS usable without adopting the Bevy application/render stack | `hecs`, `shipyard`, custom archetypes |
| `glam` | SIMD-friendly vectors, matrices, and quaternions | Apache-2.0 OR MIT | Widely used with `wgpu`, serde support, predictable layouts behind engine math types | `nalgebra`, custom math |
| `serde`, `toml`, `ron` | Config, project manifests, versioned text assets/scenes | Apache-2.0 OR MIT | Mature serialization ecosystem and source-control-friendly formats | JSON5, YAML, Protobuf |
| `uuid` | Project, entity, asset, and plugin identifiers | Apache-2.0 OR MIT | Standard 128-bit stable IDs with no central allocator | ULID, custom 128-bit IDs |
| `tracing`, `tracing-subscriber`, `tracing-appender` | Structured spans, async log fan-out, rolling files | MIT | One observability path for logs, spans, console events, and profiling bridges | `log` + `env_logger`, `slog` |
| `thiserror`, `anyhow` | Typed library errors and contextual application failures | Apache-2.0 OR MIT | Separates recoverable subsystem errors from executable-level reports | `miette`, hand-written errors |
| `crossbeam`, `rayon` | Bounded queues and CPU work scheduling foundation | Apache-2.0 OR MIT | Proven work stealing and concurrency primitives; wrapped by the engine job API | `tokio`, custom scheduler |
| `notify` | Cross-platform source/asset watching | CC0-1.0 | Native notifications with polling fallback | `watchexec`, explicit rescan |
| `rusqlite` + SQLite | Rebuildable local asset/import index | MIT; SQLite is public domain | Transactional, inspectable cache with mature tooling; never the source of truth | `redb`, RocksDB |
| `gltf`, `image`, `exr` | glTF/GLB, common raster image, and EXR import | Apache-2.0 OR MIT; EXR implementation BSD-3-Clause | Rust-native baseline importers with controllable decoding | Assimp, OpenImageIO, stb libraries |
| `ufbx` through a thin audited FFI crate | FBX import | MIT | Small, permissive, focused FBX reader without requiring Autodesk SDK redistribution | Assimp (BSD-3-Clause), Autodesk FBX SDK (proprietary) |
| `zstd` | Derived-data and package compression | BSD-3-Clause library; Rust wrapper MIT | Fast decompression and tunable compression | LZ4, Deflate |
| Rapier 2D/3D | Planned full rigid-body physics backend | Apache-2.0 | Rust-native, cross-platform, 2D and 3D; hidden behind `PhysicsWorld` | Jolt, PhysX, Box2D |
| `kira` + `cpal` | Mixer/streaming and native audio devices | MIT; Apache-2.0 | Rust-native control layer and broad host audio support | `rodio`, SDL audio, FMOD/Wwise commercial SDKs |
| `gilrs` | Gamepad discovery and events | Apache-2.0 OR MIT | Cross-platform controller normalization | SDL3 gamepad API, platform APIs |
| `mlua` with Lua/Luau features | Initial Lua and Luau adapters | MIT | Maintained Rust bindings, selectable vendored runtime, broad Lua API | `rlua`, direct Lua/Luau C API |
| QuickJS via a maintained Rust binding | JavaScript adapter | MIT | Small embeddable ES runtime without a browser dependency | V8 (BSD), SpiderMonkey (MPL-2.0), Node child process |
| `libloading` | Loading trusted native adapter libraries inside dedicated hosts | ISC | Small cross-platform dynamic-loader wrapper | Platform loader APIs |
| `postcard` or equivalent framed codec | Ephemeral IPC messages | Apache-2.0 OR MIT | Compact serde messages; schema/version envelope remains engine-owned | Protobuf, MessagePack, FlatBuffers |
| `memmap2` | Shared-memory frame/bulk-data transport | Apache-2.0 OR MIT | Portable memory mapping behind the IPC transport | OS shared-memory APIs, local sockets only |
| `clap` | CLI and `xtask` argument parsing | Apache-2.0 OR MIT | Typed, testable command surfaces | `lexopt`, `argh` |
| Wasmtime (plugin milestone) | Sandboxed portable editor/runtime plugins | Apache-2.0 WITH LLVM exception | Capability-based WASM host with resource limits | Wasmer, Extism, out-of-process native plugins |

Current Play physics uses the engine-owned basic translational simulator in
`engine-play`, not Rapier. It provides gravity, swept enclosing-box collision, and
equal-mass stacking for built-in primitives. See `PHYSICS.md` for supported flags,
setup, and limits; the full backend and query API below remain planned.

External language toolchains are optional adapter dependencies: LLVM/Clang
(Apache-2.0 with LLVM exception), .NET (MIT), CPython (PSF-2.0), OpenJDK
(GPL-2.0 with Classpath Exception), PHP (PHP-3.01), Lua (MIT), and Luau (MIT).
The doctor command records discovered versions; export builds lock them into a build
report. MP3 decoding is accepted only after codec, patent, and redistribution review
for each shipping target. `cargo-deny` policy rejects incompatible/copyleft code in
distributed engine binaries unless an explicit reviewed exception exists.

## 3. Layered module architecture

Dependencies point downward. Public types cross layers through engine-owned crates;
implementation crates are not re-exported casually.

```text
Applications     launcher  editor  runtime  cli  asset-worker  language-host
                         |       commands/events/handles
Product services projects  play  build/export  plugins  modeling  undo/redo
                         |       stable service traits
Runtime domains  world/ECS  assets  scripts  input  UI  physics  audio  animation
                         |       extracted immutable frame data
Rendering        renderer  render-graph  RHI  backend-wgpu  shader pipeline
                         |       platform service traits
Foundation       core  IDs  math  VFS  serialization  jobs  logging  diagnostics
Platform         platform-api  platform-windows  platform-linux  platform-macos
```

### 3.1 Foundation and platform modules

- `core`: clocks, error taxonomy, lifecycle, cancellation, typed handles, and common
  traits. It contains no editor or GPU types.
- `ids`: strongly typed UUIDs (`ProjectId`, `AssetId`, `SceneId`, `EntityId`) and
  generational transient handles. An ID from one domain cannot be passed as another.
- `math`: engine-owned transform and geometry types backed initially by `glam`.
- `vfs`: normalized project paths, mount tables, atomic writes, watched directories,
  and read-only package mounts. It rejects traversal beyond a mount root.
- `serialization`: schema envelopes, deterministic writers, migration registry,
  unknown-field policy, checksums, and corruption diagnostics. Raw Rust memory is
  never a file format.
- `jobs`: named priorities, cancellation, dependency fences, thread-affinity queues,
  bounded work submission, and profiling spans. Game callbacks receive controlled
  command buffers rather than arbitrary shared mutable state.
- `logging`: non-blocking structured event ingestion and sinks for console, editor
  IPC, rotating files, crash reports, and tests. Sink failure is reported to a
  fallback OS diagnostic channel and never panics the game.
- `diagnostics`: counters, CPU/GPU scopes, health state, crash markers, and report
  bundles with secret/path redaction.
- `platform-api`: windows, displays, processes, pipes/sockets, dynamic libraries,
  power/battery state, filesystem notifications, and hardware inventory as traits.
- `platform-{windows,linux,macos}`: the only crates allowed to use raw OS APIs.

### 3.2 Runtime domain modules

- `world`: Bevy ECS storage plus an engine scene-graph component. `LocalTransform`,
  `Parent`, and `Children` are authoring components; `WorldTransform` is derived in
  topological order and cycles are rejected. Structural changes are deferred through
  commands at schedule barriers. Components have stable schema IDs and versions.
- `scene`: additive scene instances, prefab inheritance, versioned `.rscene` I/O,
  entity-reference remapping, and transactional load. A corrupt entity does not make
  all other valid entities inaccessible.
- `assets`: `AssetId` registry, importer contracts, dependency graph, content hashes,
  derived-data cache, async load states, placeholders, hot reimport, and streaming.
  SQLite accelerates queries but `.rmeta` files and source assets remain authoritative.
- `input`: raw device events become timestamped state, then configurable action maps
  such as `Jump` and `MoveForward`. UI consumption and game input are separate layers.
- `physics-api`: backend-neutral bodies, colliders, queries, layers/masks, joints,
  controllers, CCD, debug draw, and fixed-step synchronization; `physics-rapier` is
  the initial adapter.
- `audio-api`: handles for clips, streams, sources, listeners, buses, effects, and
  spatialization; `audio-kira` is the initial adapter.
- `ui-runtime`: engine-native retained UI tree, layout, focus, input, rendering data,
  accessibility metadata, anchors, responsive scaling, and animation. It does not
  depend on `egui`, which is editor-only.
- `animation`: clips, state machines, blending, skeletal pose evaluation, and events.
- `particles`: CPU authoring representation and GPU simulation/render hooks.
- `scripting-api` and `scripting-host`: language-neutral API/schema, adapter registry,
  lifecycle, build cache, IPC, diagnostics, and hot reload; detailed in section 6.

### 3.3 Product/editor modules

- `projects`: per-user project catalog, thumbnails, recent/search/sort/import/remove,
  templates, engine-version resolution, and project upgrade previews.
- `editor-shell`: dock host, commands, selection, workspaces, settings, shortcuts,
  multi-window state, and accessibility. Workspace layout is stored per user, not in
  a scene.
- `editor-panels`: 3D/2D viewport, hierarchy, inspector, asset/content browser,
  console/debugger, profiler, animation, materials, project settings, and engine
  settings. Panels communicate through typed commands/events, not direct mutation.
- `undo`: command transactions with apply/revert, merge keys for drags, saved-scene
  checkpoints, memory budgeting, and asset-edit transactions.
- `modeling`: parametric primitives, editable mesh topology, gizmos, selection, and
  non-destructive modifier stacks. Boolean, slice, extrude, inset, bevel, normals,
  merge/separate/join, and vertex/edge/face tools enter in roadmap stages.
- `play`: source-scene snapshot, runtime process control, pause/step/stop protocol,
  frame transport, runtime-change diff, and explicit "Apply Runtime Changes" command.
- `build`: target descriptors, deterministic staging, adapter compilation, asset
  cooking, package manifests, signing hooks, and export reports.
- `plugins-api`: capability declarations, API/schema negotiation, registrations for
  panels/importers/exporters/runtimes/components/build targets, and isolation policy.

### 3.4 Ownership, threading, and frame schedule

The main editor thread owns native UI and window events. Each `World` is owned by its
simulation schedule. Systems may read shared component columns in parallel; writes
are statically declared and structural changes apply at barriers. Asset I/O/import,
shader compilation, and script builds use bounded background queues. The render
thread owns submission and surface presentation; resource creation arrives through
commands. Audio owns a real-time thread that never allocates, locks an unbounded
mutex, logs synchronously, or performs file I/O.

```text
poll OS/input
  -> fixed simulation ticks (0..N, bounded catch-up)
  -> scripts/physics/animation/game systems
  -> apply ECS command buffers
  -> extract immutable RenderWorld
  -> build/compile render graph in parallel
  -> record/submit GPU commands
  -> present and publish diagnostics
```

The fixed simulation clock defaults to 60 Hz and is independent of rendering.
`Frame Advance` while paused executes exactly one fixed tick, applies commands, and
renders the result. No asynchronous completion may silently advance simulation time.

## 4. Process and editor/runtime architecture

```text
rustic-project-manager
  | spawn(project path, engine version, one-use auth token)
  v
rustic-editor ---- rustic-asset-worker pool
  |  |                    |
  |  +---- build/import IPC + diagnostics
  |
  +---- spawn play snapshot ----> rustic-runtime
  |                                  |-- language-host-native
  |<--- control/logs/profiling -------|-- language-host-managed
  |<--- frame/audio-debug transport --|-- language-host-web
  |
  +---- rustic-crash-handler <---- crash records from every process
```

The Project Manager (`rustic-project-manager`) and editor are different executables
and different interfaces. Closing
an editor returns naturally to the still-running launcher when it was launcher-
started. Opening a project obtains an advisory project lock; read-only open and
explicit stale-lock recovery are supported.

All play modes use a child `rustic-runtime` so game/runtime faults do not destroy the
editor:

- **Play** renders into a runtime-owned offscreen target. The initial transport is a
  bounded shared-memory BGRA frame ring; later, same-GPU shared textures replace the
  copy where platform support is reliable. The editor displays it in the viewport.
- **New Window** gives the runtime its own native window while the editor stays live.
- **Standalone** launches from a staged export-like directory with editor-only mounts
  removed, catching packaging and path errors early.

The toolbar exposes one primary `Play` button with a dropdown for these modes. While
active, that control becomes Stop/Pause/Resume/Frame Advance state; it does not add a
row of unrelated play buttons. Starting play activates the runtime console panel.

Play serializes an immutable authoring snapshot into the project `temp/play/` area.
Runtime worlds, assets with mutable state, script VMs, and physics worlds are never
the editor authoring instances. On stop the snapshot is discarded. Runtime changes
can only reach authoring data through a reviewed diff and one undoable "Apply Runtime
Changes" transaction.

IPC uses an authenticated local named pipe/Unix-domain socket, a fixed magic number,
protocol major/minor, message length, request ID, process role, and bounded payload.
Major mismatch refuses connection with an actionable error. Bulk frames, meshes, and
profiling batches use negotiated shared memory; control messages never contain raw
pointers or Rust layouts. Every request has cancellation/deadline semantics, and
editor shutdown terminates its process group/job object after a grace period.

## 5. Rendering abstraction

### 5.1 Boundaries

```text
World/ECS + editor viewport
        -> Render extraction (`RenderWorld`, no live ECS references)
        -> Renderer (visibility, materials, lighting, passes)
        -> Render graph (resources, dependencies, barriers, aliasing)
        -> engine RHI (devices, queues, resources, pipelines, command lists)
        -> `renderer-wgpu` adapter
        -> Vulkan | Direct3D 12 | Metal | OpenGL ES/GL compatibility
```

No public engine component stores a `wgpu` type. RHI resources are typed generational
handles; stale handles produce diagnostics rather than use-after-free. The RHI uses
coarse command encoders and immutable pipeline/resource descriptors instead of a
virtual call for every draw. Resource destruction is deferred until the last GPU
submission that references it completes.

The render graph declares reads/writes and lifetimes, detects cycles and uninitialized
reads, allocates transient resources, and emits barriers. Persistent resources live
in the renderer resource registry. Device loss tears down only the backend-owned
state, shows a recovery dialog, tries the next validated backend when restart is
safe, and never rewrites project settings silently.

WGSL shaders are compiled ahead of use, reflected into engine-owned binding layouts,
hashed with defines/backend/compiler version, and cached. Failed hot reload retains
the last valid pipeline. Materials reference shader and texture `AssetId`s, not GPU
objects. The renderer progressively adds PBR/HDR, lights/shadows, culling/LOD,
post-processing, particles, streaming, GPU-driven work, compute, ray tracing, GI,
and upscaling without making those features RHI requirements.

### 5.2 Backends and five rendering tiers

The initial RHI implementation uses four actual desktop graphics backends exposed by
`wgpu`: Vulkan, Direct3D 12, Metal, and GL/GLES compatibility. Rustic defines the five
required renderer tiers independently of API choice, because a backend name alone
does not describe device capability:

| Tier | Required behavior | Typical defaults |
|---|---|---|
| Ultra | Optional high-end feature set; gracefully disables each unsupported item | native resolution or quality upscaling, highest textures/shadows, RT/GI only when validated |
| High | Full PBR/HDR path without requiring ray tracing | high textures/shadows, TAA, SSAO/reflections, volumetrics |
| Balanced | Same material model with bounded bandwidth/GPU cost | medium shadows, moderate LOD, quality/balanced upscaling, reduced effects |
| Low | Forward/trimmed deferred path and aggressive budgets | low shadows, FXAA, reduced particles/post, lower render scale |
| Compatibility | Minimum supported feature set and safest shaders | unlit/basic PBR fallback, one simple shadow path or none, FXAA/off, no compute dependency |

A headless/null RHI exists for servers and tests but does not count as one of the five
visual tiers. DX11 is not promised: current `wgpu` desktop portability is stronger
through DX12/Vulkan/Metal/GL. A future `rhi-d3d11` can implement the engine RHI if real
device telemetry shows that GL compatibility is insufficient.

## 6. Multi-language scripting architecture

The implemented adapters are the external-editor-first Lua 5.4, QuickJS, external
process-language, and Web vertical slices described by
[`ADR-0002`](decisions/ADR-0002-external-editor-lua-gameplay.md),
[`ADR-0003`](decisions/ADR-0003-quickjs-gameplay.md),
[`ADR-0004`](decisions/ADR-0004-external-language-hosts.md), and the
[`gameplay programming guide`](GAMEPLAY_PROGRAMMING.md). They establish stable script
identity, the scene component, generated workspace, engine API 1.0, runtime-only VM,
authenticated reload, and diagnostics boundary.

### 6.1 Discovery and adapter contract

`LanguageManager` registers adapters by stable runtime ID and extensions:

```text
.c                 -> CRuntime
.cc/.cpp/.cxx      -> CppRuntime
.cs                -> CSharpRuntime
.py                -> PythonRuntime
.js/.mjs           -> JavaScriptRuntime
.lua               -> LuaRuntime
.luau              -> LuauRuntime
.java              -> JavaRuntime
.php               -> PHPRuntime
.html (+ .css deps) -> WebRuntime
```

Headers, packages, and support files are dependencies rather than executable entry
points. Discovery scans configured source roots, reads optional per-script metadata,
and builds a dependency graph. There is no `project_language` field. A project may
contain all adapters simultaneously. Local trust policy can disable an adapter or
require confirmation without changing what language the project "uses."

Every adapter implements the asynchronous lifecycle:

```text
probe_toolchain -> discover -> plan_build -> compile -> load -> instantiate
-> dispatch_events -> snapshot_state -> reload -> unload -> diagnostics -> shutdown
```

Calls return typed results and structured diagnostics with file, range, severity,
language, adapter version, and stack frames. Build artifacts are keyed by source and
dependency hashes, compiler/runtime version, target, flags, engine API version, and
adapter version. Failed compilation never replaces the last-good generation.

### 6.2 Shared API and interoperability

Languages communicate through generated idiomatic bindings over one versioned schema,
not through pairwise language ABIs. The schema supports null, bool, signed/unsigned
integers, float, string, byte buffer, Vector2/3/4, Quaternion, Matrix4, color,
`EntityRef`, `AssetId`, arrays, maps with string keys, and versioned serialized values.
`EntityRef` includes world identity and generation so stale or cross-world references
fail safely.

The shared API exposes batched services for entity/component commands, immutable
query snapshots, scene lookup, actions/input, physics queries, audio, assets, time,
events, and structured logging. Components cross the boundary as schema-generated
views and command buffers. Per-property IPC round trips in an inner loop are forbidden;
queries and updates are batched, with shared read-only buffers for large arrays.

Each script instance has explicit lifecycle events such as `on_create`, `fixed_update`,
`update`, event batches, and `on_destroy`. The runtime waits only at declared schedule
barriers with a time budget. A stalled host can skip a frame, be restarted, or stop
play according to policy; it cannot hold the ECS write lock indefinitely.

### 6.3 Runtime placement

- Lua and Luau may run in-process in `rustic-runtime` for trusted projects after the
  adapter is hardened; an out-of-process mode remains available.
- C and C++ compile against the versioned C SDK and load only in a dedicated native
  language host. This prevents a bad game DLL from crashing the editor or main play
  runtime. Calls use the same schema/command protocol as other languages.
- C# uses a .NET host process; Python uses a CPython host; JavaScript uses QuickJS;
  Java uses a JVM host; PHP uses a persistent CLI host. Each has memory/time limits,
  sanitized environment inheritance, and a per-project working directory.
- `WebRuntime` hosts HTML/CSS/JS game content in a sandboxed native OS webview/offscreen
  surface with an allowlisted bridge. It is optional by platform and never renders
  the editor shell. No Electron or bundled Chromium application shell is used.

Imported projects are untrusted until the user grants trust in an external per-user
trust database keyed by canonical path and project ID. Project files cannot mark
themselves trusted. Merely browsing assets or opening a scene does not run project
scripts, build scripts, native binaries, or plugins.

### 6.4 Hot reload

Reload builds a new generation beside the active generation, validates API/schema
compatibility, asks instances for serializable state, creates replacements, restores
compatible state, then atomically swaps at a simulation barrier. If any required step
fails, the new generation is unloaded, the last-good generation continues, and the
console receives actionable diagnostics. Native hosts restart rather than unloading
arbitrary libraries in place. State migration is opt-in and versioned; it never
reinterprets raw VM/native memory.

## 7. Project and asset format

### 7.1 Directory layout

```text
MyGame/
|-- project.engine              # source-controlled versioned RON descriptor
|-- engine.lock                 # resolved engine/API/plugin versions
|-- assets/                     # source art/audio/fonts and adjacent .rmeta files
|-- scenes/                     # .rscene text resources
|-- scripts/                    # any supported language, mixed freely
|-- shaders/                    # WGSL and explicit backend overrides
|-- materials/                  # .rmat text resources
|-- prefabs/                    # .rprefab text resources
|-- plugins/                    # plugin manifests/packages; never auto-run on browse
|-- config/                     # source-controlled game/editor team defaults
|-- builds/                     # exports; ignored by default
|-- cache/                      # rebuildable imported/compiled data; ignored
|-- temp/                       # atomic-write/play staging; ignored
|-- logs/                       # editor/runtime/crash logs; ignored
`-- .rustic/                    # local project state and lock; ignored
```

Folder names come from `ProjectPaths` loaded from the manifest; subsystems request
VFS mounts such as `project://assets` and never hard-code physical directories.

`project.engine` uses the same deterministic, versioned RON family as other native
engine resources. A descriptor matching the initial `engine-project` schema is:

```ron
(
    format_version: 1,
    project_id: "018f5d71-5c5c-7ac2-a83b-89ad4e4f4f26",
    template: empty_3d,
    metadata: (
        name: "MyGame",
        description: Some("A minimal 3D project ready for scenes."),
        author: None,
        engine_version: Some("0.1.0"),
        tags: ["3d"],
        created_unix_seconds: 0,
        updated_unix_seconds: 0,
    ),
    directories: (
        assets: "assets", scenes: "scenes", scripts: "scripts",
        shaders: "shaders", materials: "materials", plugins: "plugins",
        config: "config", cache: "cache", temp: "temp",
        builds: "builds", logs: "logs",
    ),
)
```

Startup scene, rendering defaults, input maps, logging defaults, and other evolving
settings are versioned resources below `config/`, referenced by asset/resource ID as
their schemas land; they are not added as an unversioned catch-all table. Machine
overrides, trust decisions, detected toolchains, and hardware scores do not belong in
the source-controlled descriptor. They live below `.rustic/local/` or the user's engine
config directory.

### 7.2 IDs, serialization, and source control

Every imported source has an adjacent `.rmeta` text file containing its `AssetId`,
importer ID/version, settings, and dependency IDs. The editor moves source plus meta
atomically. If a sidecar is lost, the importer creates a new ID and emits a broken-
reference report rather than guessing. Duplicate IDs are quarantined and repaired
through an explicit remap transaction.

Scenes, prefabs, materials, settings, and native resources use deterministic RON with
an envelope containing resource kind, schema version, resource ID, and optional
checksum. Migrations are ordered, tested, and run on a temporary copy; saves use
write-file, flush, atomic rename, and recoverable backup. Unknown future versions open
read-only. Entity/component ordering is deterministic for useful Git/Perforce diffs.
Large source binaries use Git LFS or Perforce; `cache`, `temp`, `logs`, `.rustic/local`,
and normal `builds` output are ignored and always rebuildable.

Logs follow:

```text
logs/latest.log
logs/runtime-YYYY-MM-DD-HH-MM-SS.log
logs/editor-YYYY-MM-DD-HH-MM-SS.log
logs/crash-YYYY-MM-DD-HH-MM-SS.log
```

`latest.log` is a best-effort link/copy to the active runtime log. A bounded queue
feeds rotating file and IPC sinks. Overflow drops oldest low-severity messages first,
emits one loss counter, and preserves Error/Fatal through a reserved lane. Console
filters cover severity, subsystem, language, process, time, search, duplicate collapse,
copy, and clickable source/stack frames.

## 8. Renderer and quality auto-selection algorithm

The initial `engine-rhi` crate deliberately implements only the pure, deterministic
policy seed over supplied candidates; it does not claim to inventory or benchmark
hardware. The production selector evolves that tested policy boundary into the
following process without moving OS/GPU probing into the policy crate.

Selection is deterministic, explainable, cached, and recoverable:

1. **Check recovery state.** If the previous startup left a renderer crash marker,
   penalize that exact GPU/backend/driver tuple and probe the next candidate. `--safe-
   mode` forces Compatibility. A user override is honored only after capability
   validation; failure falls back and reports why.
2. **Inventory hardware.** Record OS/build, GPU vendor/device/driver, UMA/discrete,
   VRAM or shared-memory budget, supported APIs/features/limits, CPU topology, RAM,
   display pixels/refresh/HDR, power source, thermal/power hints, and multi-GPU topology.
3. **Enumerate candidates.** Windows: dedicated Vulkan adapters first, then DX12, Vulkan, GL; Linux: Vulkan, GL; macOS:
   Metal, then available compatibility. Enumerate each physical adapter/backend pair.
4. **Hard reject.** Remove candidates that cannot create the target surface, lack the
   Compatibility baseline limits/formats, have a known fatal driver rule, or exceed
   policy (for example, a software adapter unless no hardware adapter works).
5. **Probe safely.** A short-lived probe process creates instance/device/surface,
   compiles representative shaders, uploads a texture/mesh, renders/readbacks a known
   frame, and submits repeated representative passes. A watchdog terminates hangs.
   Probe crashes cannot take down launcher/editor. Total cold-start probe budget is
   2 seconds by default; incomplete candidates retain conservative estimates.
6. **Score candidates.** Normalize each term to 0..1 and compute:

   ```text
   candidate = 30*stability + 20*measured_performance + 15*feature_coverage
             + 10*memory_headroom + 10*OS_backend_affinity
             + 10*display_fit + 5*power_fit - penalties
   ```

   Stability uses successful probes, prior clean sessions, crash telemetry stored
   locally, and reviewed driver rules. Display fit measures cost at actual resolution
   and refresh. Power fit favors an integrated GPU on battery only when it can meet
   the frame target; it favors a discrete GPU when plugged in or required. Penalties
   cover software adapters, recent device loss, critically low memory, thermal pressure,
   and an unrecognized driver. Tie order is stability, measured performance, OS-native
   backend, then deterministic PCI/device order.
7. **Classify quality.** Derive a separate 0..100 capability score from 35% measured
   GPU frame/bandwidth result, 15% feature/limit coverage, 15% usable graphics memory,
   10% CPU result, 5% system RAM, 10% actual display cost, and 10% power/thermal headroom.
   Start at Ultra >= 85, High >= 70, Balanced >= 45, Low >= 25, otherwise Compatibility.
   Clamp individual features by capabilities and budgets, so a score never enables an
   unsupported feature. The benchmark is intentionally lightweight, not a vendor-name
   lookup.
8. **Validate and persist.** Render a hidden validation frame, then commit backend,
   adapter ID, score components, chosen profile, and reasons to
   `.rustic/local/hardware-profile.toml`. The cache key includes engine renderer
   version, OS version, GPU ID, driver version, displays, and power class; any change
   invalidates relevant measurements. A clean startup clears the crash marker.
9. **Fallback.** On initialization failure, try candidates in scored order. If all
   hardware candidates fail, show a diagnostic with driver/update guidance and offer
   headless project recovery; never loop endlessly or silently edit `project.engine`.

Optional dynamic quality changes settings only, never the graphics API during active
play. It uses rolling 120-frame CPU/GPU percentiles, excludes loading/shader-compile
spikes, requires 5 seconds above the high frame-time threshold to lower one cost level,
15 seconds below the low threshold to raise one, and waits at least 10 seconds between
changes. A two-threshold dead band and per-feature cooldown prevent oscillation. Texture
budget emergencies can react faster, but backend changes require a runtime restart.
Every automatic decision is visible and manually overrideable in diagnostics/settings.

## 9. Plugin, resilience, and security boundaries

Plugins declare an ID, semantic version, engine API range, entry type, permissions,
and registrations. Pure tools prefer Wasm with capability-scoped filesystem/network/
UI services. Native plugins and language runtimes run out-of-process by default.
Trusted in-process plugins are an advanced opt-in and cannot claim ABI stability
across engine minor versions. Registrations are revoked atomically if startup fails;
one bad importer or panel does not prevent safe-mode project opening.

Imported content is data, never executable. Asset decoders run in memory/time-limited
workers for risky formats. Paths are canonicalized beneath VFS mounts, archive sizes
and nesting are capped, IPC is authenticated/local-only, secrets are redacted from
reports, and child processes receive a minimal environment. Export manifests list
licenses, runtime/toolchain versions, content hashes, and enabled capabilities.

Fatal failures write a minimal crash record through pre-opened resources, while normal
diagnostics are asynchronous. Shader/import/script failures preserve last-good output.
Scene and manifest edits are transactional. Safe mode disables project plugins/scripts,
uses the Compatibility renderer, and lets the user inspect, back up, or repair data.

## 10. Proposed repository structure

```text
RusticGameEngine/
|-- Cargo.toml
|-- Cargo.lock
|-- rust-toolchain.toml
|-- deny.toml
|-- README.md
|-- LICENSES/
|-- docs/
|   |-- ARCHITECTURE.md
|   `-- ROADMAP.md
|-- apps/
|   |-- project-manager/   # package/binary: rustic-project-manager
|   |-- editor/            # package/binary: rustic-editor
|   |-- runtime/           # package/binary: rustic-runtime
|   |-- cli/
|   |-- asset-worker/
|   |-- language-host/
|   `-- crash-handler/
|-- crates/
|   |-- engine-core/       # app identity, errors, logging; later IDs/jobs/diagnostics
|   |-- engine-project/    # descriptor, templates, safe virtual project directories
|   |-- engine-rhi/        # renderer policy and RHI contracts
|   |-- engine-platform/   # API and Windows/Linux/macOS implementations
|   |-- engine-world/      # ECS, scene graph, scene, prefab
|   |-- engine-assets/     # registry, import, DDC, format adapters
|   |-- engine-renderer/   # render graph, wgpu backend, shaders
|   |-- engine-gameplay/   # input, physics/audio APIs, animation, runtime UI
|   |-- engine-scripting/  # shared API, host, codegen, adapters
|   |-- engine-editor/     # shell, panels, undo, modeling, play
|   |-- engine-build/      # cooking, packaging, target definitions
|   `-- engine-plugins/    # API, manifest, Wasm/native hosts
|-- sdk/
|   |-- c/
|   |-- cpp/
|   |-- csharp/
|   |-- python/
|   |-- javascript/
|   |-- lua/
|   |-- luau/
|   |-- java/
|   |-- php/
|   `-- web/
|-- templates/
|-- assets/                # editor-owned icons, shaders, fonts, templates
|-- tools/
|   `-- xtask/
|-- tests/
|   |-- fixtures/
|   |-- integration/
|   `-- golden/
`-- .github/workflows/
```

Leaf crates should remain small enough to test independently, but directories are
not a demand for dozens of empty crates on day one. A crate is split when its public
boundary or build/platform dependencies justify it. Root code never imports from an
application crate.

## 11. Architecture compliance checklist

The first-response requirements are covered as follows:

| Required item | Location |
|---|---|
| 1. Primary implementation languages | 1.1 |
| 2. Build system | 1.2 |
| 3. OS strategy | 1.3 |
| 4. Dependencies, purposes, licenses, alternatives | 2 |
| 5. Complete module architecture | 3 |
| 6. Editor/runtime process architecture | 4 |
| 7. Rendering abstraction | 5 |
| 8. Multi-language scripting | 6 |
| 9. Project format | 7 |
| 10. Renderer auto-selection | 8 |
| 11. Phased roadmap | `ROADMAP.md` section 2 |
| 12. Risks and mitigations | `ROADMAP.md` section 5 |
| 13. First usable MVP | `ROADMAP.md` section 3 |
| 14. Repository structure | 10 |
| 15. Exact first implementation task | `ROADMAP.md` section 4 |
