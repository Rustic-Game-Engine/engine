# Scene Effects showcase

Generate a normal Rustic project through the public scene/project APIs:

```sh
cargo run -p engine-project --example scene_effects_showcase -- /tmp/rustic-effects-showcase
```

Run this command from `Engine/`. The destination must be empty; the generator never replaces an existing project. Open the generated `project.engine` through Project Manager. The scene contains numbered entities for all 26 component types, targets and lights, a moving Lua trail/ripple demonstration, procedural debris/foliage OBJ meshes, a baked HDR probe panorama and 35 editable `.effect` assets with authoritative `.rmeta` sidecars. The generator reloads its checksummed scene and verifies 26 effect components before succeeding.

Select an effect in the hierarchy and use Properties to change settings. **Effect preview** contains Play/Pause/Resume/Stop/Restart/Reset, frame stepping, speed, deterministic time replay, bounds and metrics. Press Play to run the attached orbit behavior and scene effects. Restart one-shot effects from preview. Stop Play before saving authored changes.

The Inspector's **Effects presets & assets** loads/saves assets and instantiates child groups. Select a composite group to edit its child timeline; source children expose restoring asset defaults and updating the shared source file. Built-in presets are editable authored data with no external downloads or hotlinked assets.

Read `docs/SCENE_EFFECTS.md` and `docs/SCENE_EFFECTS_RESEARCH.md` for API setup, compatibility and the acceptance audit. A scene containing all 26 registered components does not prove every requested advanced renderer capability is implemented. GPU particle simulation, dynamic probe capture, temporal history, motion-vector blur remain outside the implemented paths. Native/device/browser visual validation is separate from scene load and CPU tests.
