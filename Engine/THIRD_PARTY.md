# Third-party dependency record

This record highlights direct dependency boundaries used by the M0-M6 native slice. Exact
resolved versions are authoritative in `Cargo.lock`; `cargo-deny` enforces source and
license policy on the complete transitive graph.

| Dependency | Boundary and purpose | License family | Failure boundary |
|---|---|---|---|
| `eframe` / `egui` | Native launcher and editor shells | MIT OR Apache-2.0 | Window creation errors return a failing process status; the data and process models remain testable without a physical window |
| `winit` | Native New Window and Standalone runtime shells and `wgpu` surface ownership | MIT OR Apache-2.0 | Window/surface failures return a failing runtime status; process gates can suppress only the physical window while retaining IPC and simulation |
| `directories` | `engine-platform::PlatformPaths` user-directory discovery | MIT OR Apache-2.0 | Unavailable required paths are typed startup errors |
| `crossbeam-channel` | Bounded logger and job queues behind engine-owned APIs | MIT OR Apache-2.0 | Full queues use documented non-blocking overflow/rejection behavior |
| `parking_lot` | Private synchronization inside foundation services | MIT OR Apache-2.0 | No lock representation crosses a public/persistent boundary |
| `serde` / `ron` | Versioned project, catalog, settings, and diagnostics formats | MIT OR Apache-2.0 | Future versions are rejected; corruption is diagnosed/quarantined/recovered |
| `uuid` | Backing storage for engine-owned typed IDs | MIT OR Apache-2.0 | Domain-specific newtypes prevent cross-domain ID interchange |
| `thiserror` | Typed library error implementations | MIT OR Apache-2.0 | Application boundaries convert errors to diagnostics and nonzero exits |
| `time` | Human-readable structured-log timestamps | MIT OR Apache-2.0 | Invalid timestamps fall back to Unix epoch formatting |
| `rfd` | Native project/folder picker in the launcher | MIT OR Apache-2.0 | Cancellation is a no-op; manually entered paths remain validated |
| `wgpu` / `naga` | Private renderer adapter, native surface rendering, and WGSL validation/reflection | MIT OR Apache-2.0 | Backend types remain inside `renderer-wgpu`; surface/device/validation errors cross an engine-owned boundary |
| `bevy_ecs` / `glam` | World storage/schedules and engine math values | MIT OR Apache-2.0 | Stable IDs and serialized engine components remain independent of ECS runtime handles |
| `rusqlite` | Disposable asset index | MIT | `.rmeta` files remain authoritative and index corruption triggers rebuild/quarantine |
| `image`, `gltf`, `tobj`, `hound`, `lewton`, `skrifa` | Bounded baseline asset decoders behind engine-owned artifacts | MIT OR Apache-2.0 compatible | Validation, limits, placeholders, corpus tests, and the isolated worker contain decoder failures |
| `interprocess` | Local authenticated editor/runtime IPC transport | MIT OR Apache-2.0 | Versioned framing, one-use secrets, payload limits, deadlines, and process supervision wrap it |
| `egui_tiles` | Dockable native editor layout | MIT OR Apache-2.0 | Only editor UI state depends on it; authoring state remains in testable engine models |
| Material Design Icons (Pictogrammers) | Embedded upstream SVG file-type, language, and transform-tool icons | Apache-2.0 | Icons are compiled into the editor, so rendering never depends on network availability |

Development-only `tempfile` is used for isolated persistence and corruption fixtures
and is not an engine runtime API.
