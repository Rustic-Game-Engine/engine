# Scripting Rustic games with Python

Python behaviors run as isolated, long-lived Python 3 processes. Rustic validates the
file with `python -I -m py_compile`, then launches it with isolated mode (`-I`). One
process belongs to one behavior instance, so module variables persist across
callbacks but are never shared with another script.

Install Python 3 on `PATH`, or use the Windows installer/Language Toolchain Manager.
Check discovery with `cargo xtask doctor`. Create `.py` files and global, scene, or
component attachments in the editor; never edit the script registry or scene JSON.

## The generated host loop

The generated starter defines `rustic`, `instance`, and `Game`, then reads one JSON
request per line and writes one JSON response per line. Put game behavior in the
callback dispatch section without printing anything else to standard output:

```python
speed = 4.0

for line in sys.stdin:
    request = json.loads(line)
    instance.commands = []
    rustic.commands = instance.commands
    rustic.state = request
    Game.scene.state = request.get("scene_paths", {})

    callback = request["callback"]
    if callback == "on_start":
        rustic.log("info", f"started {rustic.entity_id()}")
    elif callback == "fixed_update":
        dt = request["delta"] or 0.0
        if rustic.key("KeyW")["held"]:
            x, y, z = rustic.get_translation()
            rustic.set_translation(x, y, z + speed * dt)
    elif callback == "on_destroy":
        rustic.log("info", "destroyed")

    print(json.dumps({"format_version": 1,
                      "commands": instance.commands}), flush=True)
```

External adapters receive legacy wire callback names: `on_create`, `on_start`,
`on_enable`, `fixed_update`, `update`, `on_disable`, `on_destroy`, and `on_stop`.
Only add branches you need, but always send exactly one response for every request,
including ignored callbacks. Collision callbacks are part of the common scheduler
model but are not present in the current external-process protocol.

## Generated Python API

The starter's helpers expose:

```python
rustic.entity_id()                 # stable ID string
rustic.delta_time()
rustic.fixed_delta_time()
rustic.get_translation()           # a new [x, y, z] list
rustic.set_translation(x, y, z)
rustic.get_property("health")      # None when absent
rustic.set_property("health", 90)
rustic.GetAttribute("Color")       # get_attribute is an alias
rustic.EditAttribute("Anchored", True)
rustic.key("Space")                # pressed/released/held/axis dict
rustic.key_events()                 # copied event list
rustic.any_key_pressed()
rustic.log("info", "message")
rustic.set_enabled(False)

Game.scene.Find("Room.Table")      # entity ID or None
Game.scene.List("Room")            # entity IDs below the prefix
Game.setCurrentCamera("Room.Camera")
Game.set_current_camera("Room.Camera")  # compatibility alias

instance.add("Cube")
instance.clone("assets/models/chair.glb", parent_id)
```

Named `rustic.input(name)` exists, but the current external invocation sends an empty
`actions` map; it therefore returns the default inactive state. Raw keys and key
events are populated. Built-in attributes are `Name`, `Position`, `Size`, `Color`,
`CanTouch`, `CanCollide`, `Anchored`, and `Parent`.

Commands are applied after the callback, in list order. `set_property` and
`edit_attribute` must preserve the current engine value type. The optional instance
parent is a stable entity ID, not a scene path. Instance sources may be an ID, scene
path, model asset path, or built-in object name.

## Protocol and failure rules

Python cannot call another behavior process or VM object. API 1.0 has no public
script-level Engine Event `emit`/`subscribe` operation yet; coordinate through shared
engine state and the commands below.

The complete request fields are `format_version`, `callback`, nullable `delta`,
`entity_id`, `delta_time`, `fixed_delta_time`, `translation`, `properties`, `actions`,
`attributes`, `scene_paths`, `keys`, `key_events`, and `any_key_pressed`. Responses
must contain `format_version: 1` and a `commands` array. Supported operations are
`set_current_camera`, `set_translation`, `set_property`, `edit_attribute`, `log`,
`set_enabled`, `add_instance`, and `clone_instance`.

Standard output is reserved for protocol responses; use `rustic.log` for game logs
and standard error for process diagnostics. The environment is cleared except for a
small safe allowlist, the working directory is an engine temporary directory, each
callback must answer within three seconds, and a response may not exceed 1 MiB. A
malformed, late, or failed response disables that behavior instance.
