# Scene Effects API

Use `Effects` to control one typed effect component on an existing scene entity. The same global retains the existing `fade`, `flash`, `shake` and `pulse` tween helpers. Read the [authoring guide](/docs/guides/scene-effects) for editor steps, all 26 kind names, presets, composites and backend limitations.

## Setup and attachment

Create a Lua or JavaScript **Object Component Script** through Programming, attach it to an entity in the Inspector, and press Play. Python has equivalent `Effects` wrappers in the engine-provided SDK; import `Effects` from `rustic`. Other SDKs use the existing generic `rustic.query` boundary with the request operations below. Do not install a third-party package named rustic.

## Signatures

| Call | Result |
| --- | --- |
| `Effects.types()` | Array of the 26 case-sensitive kind keys |
| `Effects.create(entity, kind)` | Creates an effect and returns its configuration; existing effect is an error |
| `Effects.get(entity)` | Configuration, or null/nil when no component exists |
| `Effects.configure(entity, settings)` | Validated recursive patch and resulting configuration |
| `Effects.play(entity)` | Begins playback |
| `Effects.pause(entity)` | Freezes playback |
| `Effects.resume(entity)` | Resumes playback |
| `Effects.stop(entity)` | Stops playback |
| `Effects.restart(entity)` | Clears previous simulation and starts again |
| `Effects.reset(entity)` | Clears simulation and restores the initial playback state |
| `Effects.dispose(entity)` | Removes effect component and runtime state, preserving the entity |
| `Effects.state(entity)` | Playback state string, or null/nil |
| `Effects.instantiate(asset, parent, position)` | Composite root UUID, using registered asset UUID, optional parent and default zero position |
| `Effects.spawn(asset, position, lifetime, parent)` | Temporary root UUID; explicit positive lifetime schedules whole group destruction |
| `Effects.sampleWind(position, time, layers)` | XYZ wind vector; time in seconds; layers defaults to all bits |

Entity arguments accept the existing resolved entity forms. Lua `nil` or JavaScript `null` chooses the script owner. UI/global script owners may be synthetic; pass a real scene entity for those scopes. Raw protocol requests require stable entity UUID strings. Playback states are `Stopped`, `Playing`, `Paused`, `Completed` and `Disposed`. Disposal removes the component, so subsequent component state reads return null/nil.

## Complete Lua example

This example requires an entity without an existing effect. For an authored emitter, remove `Effects.create`.

```lua
return {
  Start = function()
    Effects.create(nil, "Particle")
    Effects.configure(nil, {
      common = {duration = 2, looping = false},
      kind = {Particle = {
        rate = 40,
        lifetime = {0.5, 1.0},
        gravity = {0, -2, 0}
      }}
    })
    Effects.restart(nil)
  end
}
```

Expected result: an upward emitter runs for its configured duration. Restart repeats it from cleared simulation state. Changing settings is a live Play mutation; saving the authored scene requires Inspector changes outside Play.

## JavaScript and Python

```javascript
globalThis.behavior = {
  Start() {
    Effects.create(null, "Weather");
    Effects.configure(null, {kind: {Weather: {intensity: 0.3}}});
    Effects.restart(null);
  }
};
```

```python
from rustic import Effects

def on_start():
    Effects.create(None, "Wind")
    Effects.configure(None, {"kind": {"Wind": {"strength": 3.0}}})
```

Use the generated Python starter's callback registration pattern if it differs from this callback body. The wind example changes compatible particles; a wind volume has no standalone visible mesh.

## Events and generic requests

The existing Events service carries `EffectBirth`, `EffectCollision`, `EffectDeath` and `EffectCompleted`. Subscribe with `Events.connect(entity, name, callback)`; payload includes a world-position vector. Composite roots emit `EffectCompleted` once per completion cycle after child effects complete. Restart/reset starts a new cycle. Keep callbacks small and avoid recursively creating unbounded effects. Sub-emitter recursion is separately bounded by the runtime generation limit.

Generic operations are `effect_types`, `effect_create`, `effect_get`, `effect_configure`, `effect_control`, `effect_state` `effect_wind`, `effect_instantiate` and `effect_spawn`. For example, `rustic.query({op:'effect_create',entity:id,kind:'Particle'})` is equivalent to creation through the wrapper. Configuration requests use `settings`; controls use an `action` string such as `pause`. `effect_instantiate` accepts asset UUID, optional parent and position; `effect_spawn` requires position and a positive lifetime so a missing lifetime cannot silently create a persistent instance. Use the native query syntax for your language; do not construct private transport messages yourself.

## Validation and lifetime

Configuration has the shape `{source, common, kind}`. Kind settings are nested under the variant key, for example `{kind:{Particle:{rate:50}}}`. Unknown fields and invalid values return an error before the world transaction is committed. Use the [typed reference](/docs/guides/scene-effects/configuration) for exact fields, defaults and limits.

Effect configuration survives scene serialization when authored. Runtime clocks, live particles, script mutations and event queues do not become permanent scene edits. Entity destruction and scene unload release effect state. One component is allowed per entity; composite assets instantiate children as ordinary scene effects. Controls and state can target the composite group root; disposal removes descendant effect components and preserves the entity hierarchy. This API does not expose GPU particle simulation or automatically make cosmetic particle collisions authoritative gameplay physics.
