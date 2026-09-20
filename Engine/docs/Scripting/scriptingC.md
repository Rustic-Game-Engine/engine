# Scripting Rustic games with C

C behaviors are low-level protocol programs. Rustic compiles `.c` files as C17 and
runs one isolated process per behavior instance. Unlike C++, C currently has no
generated parser or full Script API SDK: the generated starter only demonstrates the
response loop and helper functions for instance/camera commands. Production C scripts
must parse the request JSON and serialize a valid response themselves.

Install `clang`, `gcc`, or MSVC `cl` on `PATH`. Clang/GCC use `-Wall -Wextra -Werror
-std=c17`; MSVC uses `/nologo /W4`. Use `cargo xtask doctor` to see which compiler
Rustic found.

## Required process contract

Keep the process alive. For every newline-delimited JSON request on standard input,
write exactly one newline-delimited JSON response and flush:

```c
#include <stdio.h>

int main(void) {
    char request[1048577];
    while (fgets(request, sizeof request, stdin)) {
        /* Parse request, dispatch request.callback, build commands here. */
        puts("{\"format_version\":1,\"commands\":[]}");
        fflush(stdout);
    }
    return 0;
}
```

Do not use `printf` on standard output for debugging. Add a `log` command to the
response or write diagnostics to standard error. Use a real bounds-checked JSON
parser/serializer in nontrivial code; never interpolate untrusted scene names or
property strings into JSON.

Request fields are:

| Field | Type and meaning |
| --- | --- |
| `format_version` | integer; currently `1` |
| `callback` | `on_create`, `on_start`, `on_enable`, `fixed_update`, `update`, `on_disable`, `on_destroy`, or `on_stop` |
| `delta` | number for update callbacks, otherwise `null` |
| `entity_id` | stable entity ID string |
| `delta_time`, `fixed_delta_time` | current engine time steps |
| `translation` | three-number array |
| `properties` | declared public property object |
| `attributes` | available built-in attributes |
| `scene_paths` | path-to-entity-ID object |
| `keys`, `key_events`, `any_key_pressed` | raw input snapshot |
| `actions` | currently an empty object for external adapters |

Collision callbacks are not currently sent to external protocol programs. An ignored
lifecycle callback still requires an empty response.

## Commands

Return commands in the order they should be applied:

```json
{"format_version":1,"commands":[
  {"op":"set_translation","value":[1,2,3]},
  {"op":"set_property","name":"health","value":90},
  {"op":"edit_attribute","name":"Color","value":[1,0.5,0.25]},
  {"op":"log","level":"info","message":"started"},
  {"op":"set_enabled","enabled":false},
  {"op":"set_current_camera","source":"Room.Camera"},
  {"op":"add_instance","source":"Cube","parent":null},
  {"op":"clone_instance","source":"Room.Table","parent":null}
]}
```

`set_property` only accepts a declared property and must preserve its engine type.
`edit_attribute` supports `Name`, `Position`, `Size`, `Color`, `CanTouch`,
`CanCollide`, `Anchored`, and `Parent`, again with the existing type. Instance source
may be a stable ID, scene path, Explorer model path, or built-in name. `parent`, when
not null, must be a stable entity ID.

The generated starter includes `instance_add`, `instance_clone`, and
`Game_setCurrentCamera`, but its single-command buffer is illustrative: each helper
overwrites that buffer, and the starter resets it before every response. Extend it to
an array builder or JSON library before expecting multiple commands. There is no
current generated C equivalent of `rustic.get_translation()` or scene lookup; read
those values from the request structure you parse.

## Editor lifecycle and containment

C cannot call another behavior process directly. API 1.0 does not yet include an
external command for Engine Event `emit`/`subscribe`; cross-language coordination must
use shared engine state.

Create/attach C scripts through global, scene, or component commands in the editor so
their persistent Asset IDs are recorded. Execution is global, then scene, then
component, with configured order and IDs as tie-breakers. A process-global variable
belongs only to that behavior instance and persists until teardown.

Source and responses are limited to 1 MiB, and each callback has three seconds to
respond. The environment is cleared to a safe allowlist and the working directory is
temporary. A nonzero exit, timeout, malformed response, or rejected command disables
the behavior. Build/reload validation failures leave the last-known-good instance
running.
