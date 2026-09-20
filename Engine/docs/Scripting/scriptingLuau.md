# Scripting Rustic games with Luau

Luau `.luau` files use the external Luau CLI, not the bundled Lua 5.4 VM. Install the
`luau` executable on `PATH`; Rustic validates source with `luau --compile=- -` and
launches the file in an isolated external-process session.

Create and attach Luau through the editor so a stable Asset ID is registered for the
global, scene, or object-component scope. Do not copy Lua metadata or edit scene data
by hand: `.lua` and `.luau` are different adapters.

## Current adapter capability

The generated Luau starter is a protocol-response scaffold focused on camera
selection:

```luau
local commands = {}
local Game = {}

function Game.setCurrentCamera(source)
    if type(source) == "table" then source = source[1] end
    assert(type(source) == "string",
        "expected a camera path or {cameraPath}")
    local escaped = string.gsub(source, '[%c\\"]', function(c)
        return string.format("\\u%04x", string.byte(c))
    end)
    table.insert(commands,
        '{"op":"set_current_camera","source":"' .. escaped .. '"}')
end

Game.setCurrentCamera({"Game.scene.Room.Camera"})
print('{"format_version":1,"commands":[' ..
      table.concat(commands, ",") .. ']}')
```

This scaffold emits one response and exits. The engine's external behavior host is
designed for a persistent request/response process, so repeated lifecycle callbacks
are **not currently usable with the stock Luau starter**. Luau also does not yet
receive the generated `rustic`, `instance`, or `Game.scene` convenience APIs. Treat
Luau support in this release as compile validation plus the limited one-response CLI
bridge; choose Lua 5.4 for a fully embedded gameplay behavior.

That limitation is documented intentionally: do not write `return { Start = ... }`
as if a `.luau` asset were Lua 5.4, and do not expect `Update` to repeat. Existing
Luau files retain their adapter classification and Asset IDs for backward
compatibility.

## Protocol reference for adapter authors

The external host sends newline-delimited version-1 requests containing `callback`,
nullable `delta`, `entity_id`, timing, translation, properties, attributes,
`scene_paths`, raw keys/events, and `any_key_pressed`. External lifecycle names are
`on_create`, `on_start`, `on_enable`, `fixed_update`, `update`, `on_disable`,
`on_destroy`, and `on_stop`; collision events are not included. A conforming adapter
must answer every request with exactly one line:

```json
{"format_version":1,"commands":[]}
```

Valid commands are `set_current_camera`, `set_translation`, `set_property`,
`edit_attribute`, `log`, `set_enabled`, `add_instance`, and `clone_instance`.
Responses are applied in order after a callback. Properties and attributes must retain
their declared types; instance parents must be stable entity IDs.

The current Luau CLI environment does not provide an engine-owned persistent stdin
loop, so this protocol description is for understanding compatibility and future
adapter work, not a promise that ordinary Luau source can implement it today.

## Safety and troubleshooting

Luau cannot call another behavior or publish Engine Events through API 1.0; no public
`emit`/`subscribe` command exists yet. Use Lua 5.4 or shared engine state where the
current adapter permits it.

Validation source is limited to 1 MiB. Runtime responses are limited to 1 MiB and
must arrive within three seconds. Standard output is reserved for response JSON.
If the executable is missing, the script cannot be prepared; check
`cargo xtask doctor`. If the first invocation succeeds and a later callback reports a
closed process, that is the known single-response limitation—use Lua 5.4 rather than
trying to repair Asset IDs or scene files.
