# Scripting Rustic games with Lua 5.4

Lua 5.4 is the most direct Rustic scripting option: the runtime is bundled, each
script is an isolated module, and Script API mutations are applied immediately.
Create a `.lua` file through the Explorer's scripting commands; do not create an
Asset ID or edit a scene file manually.

## Attach and choose a scope

- **Global Startup Script**: one game-lifetime instance, started when Play starts.
- **Scene Startup Script**: one scene-lifetime instance, started after globals when
  its `scene/{name}.scene` loads.
- **Object Component Script**: one instance per attachment, created and removed with
  its object. Select the object and use **+ Add Component**, or drag the script from
  Explorer onto the viewport.

Moving or renaming the `.lua` file in the editor is safe because the attachment holds
its persistent Asset ID. Do not edit `config/scripts.ron`.

## Module and callbacks

Return one behavior table. Every member is optional; Rustic looks up a callback before
calling it. Returning no value is also accepted and creates a no-op behavior.

```lua
local speed = 4.0 -- private state for this behavior instance

return {
  Start = function()
    rustic.log("info", "started " .. rustic.entity_id())
  end,

  OnEnable = function() end,

  FixedUpdate = function(dt)
    local key = rustic.key("KeyW")
    if key.held then
      local x, y, z = rustic.get_translation()
      rustic.set_translation(x, y, z + speed * dt)
    end
  end,

  Update = function(dt) end,
  OnDisable = function() end,
  OnDestroy = function() end,
}
```

Canonical public names are `Start`, `FixedUpdate`, `Update`, `OnEnable`,
`OnDisable`, and `OnDestroy`. The legacy spellings `on_start`, `fixed_update`,
`update`, `on_enable`, `on_disable`, and `on_destroy` still work. The legacy internal
hooks `on_create` and `on_stop` are also recognized. If both canonical and legacy
names are present, the canonical callback wins. Use `FixedUpdate(dt)` for physics and
deterministic movement; use `Update(dt)` for frame-rate work.

The engine-wide callback model also defines `OnCollisionEnter`,
`OnCollisionStay`, and `OnCollisionExit`. The current embedded Lua adapter does not
yet bind those three names, so do not rely on them in Lua code in this release.

## Script API

The read API returns engine-owned snapshots; writes validate against the host:

```lua
local id = rustic.entity_id()                 -- stable entity ID string
local frame_dt = rustic.delta_time()
local fixed_dt = rustic.fixed_delta_time()
local x, y, z = rustic.get_translation()
rustic.set_translation(x, y + 1, z)

local health = rustic.get_property("health") -- nil if absent
rustic.set_property("health", 90)             -- declared property only

local name = rustic.GetAttribute("Name")
rustic.EditAttribute("Anchored", true)
rustic.log("info", "entity " .. id)
rustic.set_enabled(false)
```

`get_attribute`/`GetAttribute` and `edit_attribute`/`EditAttribute` are aliases.
Built-in attributes are `Name`, `Position`, `Size`, `Color`, `CanTouch`,
`CanCollide`, `Anchored`, and `Parent`. Attribute and property writes must match the
existing value's type. Lua-to-engine writes currently accept only boolean, integer,
number, or string values. Consequently Lua can edit `Name` and the boolean attributes,
but cannot pass the Vec3 values required by `Position`, `Size`, or `Color`, or the
optional entity value required by `Parent`; use `set_translation` for position.
Invalid values fail and disable the offending script.

## Finding and creating objects

```lua
local table_id = Game.scene.Find("Room.Table")
local all_in_room = Game.scene.List("Room")
local direct_root_child = Game.scene.Camera

local new_id = instance.add("Cube")
local model_id = instance.add("assets/models/chair.glb", table_id)
local copy_id = instance.clone("Room.Table")
Game.setCurrentCamera("Room.Camera")
```

Paths may be dotted or slash-separated. `Find` returns an ID or `nil`; `List`
returns stable IDs. `instance.add` accepts an entity ID/path, an Explorer model path,
or `Part`, `Cube`, `Sphere`, `Cylinder`, `Plane`, `Rectangle2D`, or `Circle2D`.
`clone` copies an existing scene object. The optional parent must be a stable entity
ID string. Both calls return the new ID immediately. Camera selection accepts a path,
ID, or the compatibility form `{ "Game.scene.Room.Camera" }`.

## Input

`rustic.input("action")` reads a named action. `rustic.key("KeyW")` reads a raw
physical key. Both return `{pressed, released, held, axis}`. `pressed` and `released`
are one-frame edges; `held` persists. `rustic.key_events()` returns ordered tables
with `key`, `state` (`"pressed"`/`"released"`), and `repeat`.
`rustic.any_key_pressed()` is true for any new press. Use W3C/winit names such as
`KeyW`, `Digit1`, `ArrowLeft`, `Escape`, and `F12`.

## Isolation and failures

Lua cannot directly call a JavaScript or external-process behavior. API 1.0 does not
yet expose the scheduler's Engine Event `emit`/`subscribe` surface to scripts, so use
shared engine state for cross-language coordination.

Lua cannot access `io`, `os`, `package`, `debug`, `dofile`, `loadfile`, `require`,
or `collectgarbage`. It has no filesystem, network, process, registry, or editor
access. An exception or exhausted instruction budget disables only that behavior.
Use the Console diagnostic to open the source location, fix it, then reload scripts.
