# Gameplay programming

Rustic gameplay code can be written in Lua 5.4, JavaScript, Python, C#, C, C++, Java,
PHP, or HTML/CSS using VS Code, VS Code Insiders, a configured custom editor, or the
system-associated application. Rustic
deliberately has no internal code editor.

## Project layout and ownership

- `scripts/*.{lua,js,py,cs,c,cpp,java}` is user-owned gameplay source.
- `ui/*.{html,css,js,php}` is user-owned game UI source. HTML, CSS, and PHP
  entries outside this directory are rejected by the manifest loader.
- `config/scripts.ron` is the versioned source-controlled manifest. A stable
  `ScriptId` survives file rename or move; update the manifest path with the move.
- `<Project>.code-workspace` and `.rustic/generated/programming/*` are reproducible
  engine output and carry a generated marker.
- `.vscode/settings.json`, `tasks.json`, and `launch.json` are user-owned and never
  overwritten. Generated extension recommendations, API declarations, and protocol
  schema live below `.rustic/generated/programming`.

Use **Programming → Create Behavior** and select a language to create
and, when an entity is selected, attach a behavior. Use **Open Project in Code Editor**
or click a console source/frame to open the exact file, line, and column. Custom
editor argument templates are arrays of native arguments and support `{file}`,
`{line}`, `{column}`, `{project}`, and
`{workspace}`; shell commands are never constructed.

## Lua lifecycle and API 1.0

A module returns a table with any of these callbacks, called in order:

```lua
return {
  on_create = function() end,
  on_start = function() end,
  fixed_update = function(fixed_dt) end,
  update = function(frame_dt) end,
  on_destroy = function() end,
  on_stop = function() end,
}
```

The global `rustic` table exposes `entity_id`, `get_translation`, `set_translation`,
`find_entity`, `get_attribute`/`GetAttribute`, `edit_attribute`/`EditAttribute`,
`input`, `log`, `delta_time`, `fixed_delta_time`, `get_property`, `set_property`, and
`set_enabled`. The built-in attributes are `Name`, `Position`, `Size`, `Color`,
`CanTouch`, `CanCollide`, `Anchored`, and `Parent`.

Scene objects can be resolved by a dotted or slash-separated path. Lua provides
`Game.scene.Find("Room.Table")` and `Game.scene.List("Room")`; JavaScript additionally
supports direct root access such as `Game.scene.Table`. Entity lookup is fallible. Missing/stale entities,
unknown properties, type changes, non-finite transforms, and unauthorized operations
return errors and contain the failing instance. Public values are Boolean, Integer,
Number, String, Vec2, Vec3, and optional stable Entity ID.

Runtime objects can be created with `instance.add(source, parent?)` or copied with
`instance.clone(source, parent?)`. `source` may be a stable entity ID, a dotted or
slash-separated scene/explorer path, or a built-in object name (`Part`, `Cube`,
`Sphere`, `Cylinder`, `Plane`, `Rectangle2D`, or `Circle2D`). Lua returns the new
stable entity ID immediately. JavaScript queues the structural change for the end of
the callback, consistent with its other mutation APIs.

Model files from the Game Project Explorer can be instantiated directly, for example
`instance.add("assets/models/chair.obj")`. OBJ, glTF, and GLB sources are copied into
the immutable play snapshot with their adjacent `.rmeta` files; the runtime never
reads from or mutates the live project directory.

Scripts can use named actions or raw physical keys. `rustic.key("KeyW")` returns
`pressed`, `released`, `held`, and `axis`; `rustic.any_key_pressed()` detects any new
press; and `rustic.key_events()` returns every ordered press/release event, including
the key name and auto-repeat flag. Physical names follow W3C/winit conventions such
as `KeyW`, `Digit1`, `ArrowLeft`, `Escape`, and `F12`. Unknown platform keys remain
available by name. Focus loss releases all held keys and transient events are cleared
once per frame.

## JavaScript lifecycle and API 1.0

JavaScript assigns the same snake-case lifecycle to `globalThis.behavior`:

```javascript
globalThis.behavior = {
  on_start() { rustic.log("info", "started"); },
  fixed_update(dt) {
    const [x, y, z] = rustic.get_translation();
    rustic.set_translation(x + dt, y, z);
  },
  update(dt) {},
  on_stop() {},
};
```

Lua and JavaScript behaviors can be attached to the same entity. They share engine
state rather than VM values. The JavaScript bridge exposes time, entity identity,
translation, typed public properties, action-state snapshots, logging, and enabled
state. Generated declarations live at `.rustic/generated/programming/rustic_api.d.ts`.

## External process languages

Python, C#, C, C++, Java, and PHP use host protocol version 1. Rustic keeps one process
alive per behavior instance so language-global state survives between callbacks. Each
invocation reads one newline-delimited JSON request from standard input and writes one
newline-delimited response:

```json
{"format_version":1,"commands":[{"op":"set_translation","value":[1,2,3]}]}
```

The request contains `callback`, `delta`, `entity_id`, `delta_time`,
`fixed_delta_time`, `translation`, `properties`, `keys`, `key_events`, and
`any_key_pressed`. Supported commands are `set_translation`, `set_property`, `log`,
`set_enabled`, `add_instance`, and `clone_instance`. Instance commands use a `source`
string and optional stable `parent` entity ID, so every external language can add the
same Explorer asset:

```json
{"op":"add_instance","source":"assets/models/chair.obj","parent":null}
```

The generated starter for each language is already a valid protocol program. The generated JSON schema documents
the request. External callbacks have a three-second deadline and 1 MiB response limit.

Generated external-language starters hide these protocol commands behind native APIs:
Python uses `instance.clone(path)`, PHP uses `$instance->clone(path)`, C uses
`instance_clone(path, parent)`, C++ uses `instance.clone(path)`, C# uses
`Instance.Clone(path)`, and Java uses `instance.clone(path)`. The corresponding `add`
operation follows the same naming convention. PHP uses `->` because `instance.clone`
is not valid PHP syntax.

### C++ API

C++ behaviors include the generated `rustic.hpp`; Rustic supplies that header while
validating and building the behavior, and also writes an IntelliSense copy to
`.rustic/generated/programming/rustic.hpp`. Game code does not parse or emit host
protocol JSON. The API deliberately follows the Lua and JavaScript names:

```cpp
#include "rustic.hpp"

void fixed_update(double dt) {
    const auto forward = rustic.key("KeyW");
    if (forward.held) {
        const auto position = rustic.get_translation();
        rustic.set_translation(position.x, position.y, position.z + dt);
    }

    const auto table = Game.scene.Find("Room.Table");
    rustic.EditAttribute("Position", RusticValue::Array{1.0, 2.0, 3.0});
    instance.clone("assets/models/chair.obj");
}

int main() {
    return rustic_run(RusticBehavior{.fixed_update = fixed_update});
}
```

`RusticValue` represents the shared Boolean, Number, String, Vec2, Vec3, entity-ID,
array, object, and null values. `Game.scene.Find` returns
`std::optional<std::string>`, and `rustic.key`/`rustic.input` return a typed
`RusticActionState`.

Python requires Python 3; C# requires the .NET SDK; C/C++ requires Clang, GCC, or
MSVC; Java requires `java` and `javac`; PHP requires PHP CLI. Run `cargo xtask doctor`
to see the exact executable and version Rustic discovered.

On Windows, Setup offers these external toolchains as optional Gameplay Languages and
installs every selected item through Windows Package Manager. Lua, JavaScript, and
HTML/CSS are bundled. Additional toolchains can be installed later using **Rustic Game
Engine > Language Toolchain Manager** in the Start menu or by rerunning Setup.

## HTML/CSS content

HTML/CSS/PHP UI content is confined to the project's `ui/` directory. `.html` and
`.css` entries are structurally validated. Inline `<script>` lifecycle
objects use the sandboxed JavaScript API. CSS-only entries are valid content behaviors
with no callbacks. HTML and inline scripts have no DOM, network, filesystem, or Node
APIs. Web entries participate in gameplay lifecycle and state updates; they never
render or replace the editor shell.

## Validation, diagnostics, and reload

Source is UTF-8 and limited to 1 MiB. Validation compiles without execution before a
snapshot or reload is sent. Snapshot and reload hashes are verified. During Play,
**Reload Scripts** validates all manifest entries and sends only successful source over
authenticated, bounded IPC. Replacement occurs at a simulation boundary. Existing
typed properties and runtime transforms remain. Property-schema additions/removals
currently require Restart Play; incompatible live replacement is rejected. A failed
reload leaves the prior instance running. **Restart Play** rebuilds the
immutable snapshot.

Console records identify process and subsystem, collapse duplicates, filter/search,
copy, clear, and open source/stack locations externally. Interactive debugging is not
currently claimed.

## Sandbox and play safety

Lua, JavaScript, and Web inline scripts cannot access filesystem, network, environment
variables, processes, registry, packages, native memory, or editor/backend objects. Callback budgets
contain infinite loops; logs and IPC use bounded queues/payloads. Runtime worlds are
copies. Stop never writes transforms or properties into the authoring scene. Runtime
transform changes are offered for explicit conflict-checked review and apply as one
undoable transaction; source edits are never applied automatically.

## Troubleshooting

- **Editor executable missing:** install VS Code, select VS Code Insiders, configure a
  validated custom executable and native argument template, or use system association.
- **Invalid script:** click its diagnostic to open the exact location. The last-known-
  good runtime instance continues during Play.
- **Manifest mismatch/missing source:** restore the file or update the manifest;
  duplicate IDs are quarantined instead of guessed.
- **Reload rejected:** check API major, property types, source size, and syntax; use
  Restart Play after an intentionally incompatible change.
