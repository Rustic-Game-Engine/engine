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
`OnStart` is **not** a Lua callback name. A script with `OnStart` can load successfully
while its startup function never runs; rename it to `Start`.

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
print("entity", id)
warn("entity needs attention", id)
rustic.set_enabled(false)
```

`print(...)` writes an `info` entry and `warn(...)` writes a `warn` entry to the
Rustic console. Multiple arguments are converted with Lua's `tostring` and separated
by tabs. `rustic.log(level, message)` remains available when an explicit level is
needed.

`get_attribute`/`GetAttribute` and `edit_attribute`/`EditAttribute` are aliases.
Built-in attributes are `Name`, `Position`, `Size`, `Color`, `CanTouch`,
`CanCollide`, `Anchored`, and `Parent`. Attribute and property writes must match the
existing value's type. Attribute setters accept scalar values, exactly three finite
numbers in a table for `Position`, `Size`, and RGB `Color`, and a stable entity ID
string or `nil` for `Parent`. Public-property setters retain their existing scalar
write rules. To edit another object, use
`rustic.game.Demo.Room.Player:EditAttribute("Position", {1, 2, 3})`; see
[Edit scene objects](sceneObjects.md) for setup and supported types.
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

### What works in the editor's Play viewport

The editor forwards **held keys** to scripts while its Play viewport is hovered or
focused. Click the game view once if another panel has focus. The forwarded names
are `KeyW`, `KeyA`, `KeyS`, `KeyD`, `ArrowUp`, `ArrowDown`, `ArrowLeft`,
`ArrowRight`, `ShiftLeft`, and `ShiftRight`. Either physical Shift key sets both
Shift names because the editor currently receives a combined Shift modifier.
For these names, `rustic.key(name).held` is true while the key is held and false
after release or when the Play viewport loses keyboard focus. `axis` is `1` when
held and `0` otherwise.

The `rustic.key(name)` result also has `pressed` and `released` fields, but the
current Play bridge does **not** forward press/release edges. Those fields remain
false. `rustic.key_events()` returns an empty list, and
`rustic.any_key_pressed()` remains false. Named `rustic.input("action")` actions
remain inactive. Other names, including `Space`, `Escape`, `Digit1`, and function
keys, are not forwarded yet. Keyboard forwarding currently applies to the editor's
embedded **Play** viewport; a separate **New Window** or **Standalone** runtime
window does not send its keyboard events to scripts. Use `.held` with the names
listed above for gameplay movement in this build.

### Copyable top-down controller

1. In the editor, create a **Lua 5.4** `.lua` **Object Component Script**. `.luau` uses a different adapter. If the file already exists, select the player object and attach it with **+ Add Component** in the Inspector.
2. Replace the script contents with the example below. Keep `return { ... }` and use `Start`, not `OnStart`.
3. Start **Play**, hover or click the game viewport, then hold WASD or an arrow key. Hold Shift to sprint. The Console should show both startup messages.

```lua
local WALK_SPEED = 4.0
local SPRINT_SPEED = 7.0
local PLAYER_HEIGHT = 1.0

local function held(primary, alternate)
  return rustic.key(primary).held or rustic.key(alternate).held
end

return {
  Start = function()
    print("Character ready - use WASD or arrow keys to move")
    warn("Movement controller is running")
  end,

  FixedUpdate = function(dt)
    local horizontal = 0
    local vertical = 0
    if held("KeyA", "ArrowLeft") then horizontal = horizontal - 1 end
    if held("KeyD", "ArrowRight") then horizontal = horizontal + 1 end
    if held("KeyW", "ArrowUp") then vertical = vertical - 1 end
    if held("KeyS", "ArrowDown") then vertical = vertical + 1 end

    if horizontal == 0 and vertical == 0 then return end

    local length = math.sqrt(horizontal * horizontal + vertical * vertical)
    local speed = WALK_SPEED
    if held("ShiftLeft", "ShiftRight") then speed = SPRINT_SPEED end
    local x, _, z = rustic.get_translation()
    rustic.set_translation(
      x + horizontal / length * speed * dt,
      PLAYER_HEIGHT,
      z + vertical / length * speed * dt
    )
  end,
}
```

This moves the **object carrying the component**, not a player object found by
name. Diagonal motion is normalized so it has the same speed as straight motion.
`PLAYER_HEIGHT` is applied when movement starts; change it to match your scene.
Make sure the active game camera can see the object and has enough room to show
the translation.

If the startup messages are missing, check that the file is `.lua`, the script is
attached and enabled, the callback is named `Start`, and Play actually started.
If messages appear but movement does not, use the editor's embedded Play viewport,
hover or click it, and try `KeyW` first. Check the Console for a script error: a
failing callback is disabled until the script is fixed and reloaded or Play restarts.

## Isolation and failures

Lua cannot directly call a JavaScript or external-process behavior. API 1.0 does not
yet expose the scheduler's Engine Event `emit`/`subscribe` surface to scripts, so use
shared engine state for cross-language coordination.

Lua cannot access `io`, `os`, `package`, `debug`, `dofile`, `loadfile`, `require`,
or `collectgarbage`. It has no filesystem, network, process, registry, or editor
access. An exception or exhausted instruction budget disables only that behavior.
Use the Console diagnostic to open the source location, fix it, then reload scripts.

## Target another scene object

See [Edit scene objects](sceneObjects.md) for named-scene hierarchy calls, supported
attributes, copyable examples, and native SDK calls to edit another object. Use your language's native call syntax and its current runtime
limitations.

Lua owner and path-based `EditAttribute` calls accept three-number tables for
Position, Size, and RGB Color; Parent accepts a stable entity ID string or `nil`.
Vectors require exactly three finite components. These conversions apply to built-in
attributes; script public-property setters retain their existing value rules.
