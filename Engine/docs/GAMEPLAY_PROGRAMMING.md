# Gameplay programming

Rustic gameplay code can be written in Lua 5.4, JavaScript, Python, C#, C, C++, Java,
PHP, or HTML/CSS using VS Code, VS Code Insiders, a configured custom editor, or the
system-associated application. Rustic
deliberately has no internal code editor.

## Project layout and ownership

Rustic's agent instructions and Codex/Claude integration files live in editor app
data. Use the `rustic-workspace` MCP tools to inspect or edit them; see
[AI agents and editor app data](AGENT_INTEGRATION.md) for setup and examples.

- `scripts/*.{lua,js,py,cs,c,cpp,java}` is user-owned gameplay source.
- `scene/{scene-name}.scene` contains authored scenes. Existing
  `scenes/*.rscene` projects remain supported and are saved in place.
- `ui/*.{html,css,js,php}` is user-owned game UI source. HTML, CSS, and PHP
  entries outside this directory are rejected by the manifest loader.
- `config/scripts.ron` is the editor-owned, versioned asset registry. Developers do
  not edit it. Every source receives a stable Asset ID; editor moves/renames update
  the path transactionally while scene and object references remain unchanged.
- `<Project>.code-workspace` and `.rustic/generated/programming/*` are reproducible
  engine output and carry a generated marker.
- `.vscode/settings.json`, `tasks.json`, and `launch.json` are user-owned and never
  overwritten. Generated extension recommendations, API declarations, and protocol
  schema live below `.rustic/generated/programming`.

Use **Programming** to create a Global Startup, Scene Startup, or Object Component
script in any supported language. Use **+ Add Component** in the Inspector or drag a
script from Explorer onto the viewport to attach an existing script to the selected
object. Use **Open Project in Code Editor**
or click a console source/frame to open the exact file, line, and column. Custom
editor argument templates are arrays of native arguments and support `{file}`,
`{line}`, `{column}`, `{project}`, and
`{workspace}`; shell commands are never constructed.

## Lifecycle, execution order, and API 1.0

Global scripts are instantiated first when the game starts. Scene scripts are next
when their scene loads. Component scripts are instantiated with their object and are
disabled/unregistered with it. Only callbacks actually supplied by a script are
invoked. The canonical callbacks are `Start`, `Update`, `FixedUpdate`,
`OnCollisionEnter`, `OnCollisionStay`, `OnCollisionExit`, `OnEnable`, `OnDisable`, and
`OnDestroy`. Original snake-case callback names continue to work.

The basic Play simulator runs gravity and primitive box collision response after
`FixedUpdate`, including in scenes with no scripts. `Anchored` prevents physics
movement; `CanCollide` must be enabled on both objects for solid response. Collision
callback names are reserved: the simulator does not dispatch collision or touch
events. See `PHYSICS.md` for floor setup, falling objects, and current limitations.

Execution is deterministic: scope (global, scene, component), explicit execution
order, entity Asset ID, script Asset ID, then attachment order. Scripts communicate
through typed Engine Events and the common Script API, never by sharing language VM
objects. This keeps cross-language calls and future language adapters independent of
the core scheduler.

### Lua

A module returns a table with any of these callbacks, called in order:

```lua
return {
  Start = function() end,
  FixedUpdate = function(fixed_dt) end,
  Update = function(frame_dt) end,
  OnEnable = function() end,
  OnDisable = function() end,
  OnDestroy = function() end,
}
```

The global `rustic` table exposes `entity_id`, `get_translation`, `set_translation`,
`find_entity`, `get_attribute`/`GetAttribute`, `edit_attribute`/`EditAttribute`,
`input`, `log`, `delta_time`, `fixed_delta_time`, `get_property`, `set_property`, and
`set_enabled`. The built-in attributes are `Name`, `Position`, `Size`, `Color`,
`CanTouch`, `CanCollide`, `Anchored`, and `Parent`.
Lua also provides `print(...)` for an info Console entry and `warn(...)` for a
warning Console entry. Use `rustic.log(level, message)` to choose a different level.

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

**Current Play input:** The editor's embedded Play viewport forwards the held
state of WASD, arrow keys, and Shift to scripts while that viewport is hovered or
focused. The supported names are `KeyW`, `KeyA`, `KeyS`, `KeyD`, `ArrowUp`,
`ArrowDown`, `ArrowLeft`, `ArrowRight`, `ShiftLeft`, and `ShiftRight`.
`rustic.key(name).held` is available for movement; `axis` is `1` when held and
`0` otherwise. The API also exposes `pressed`, `released`, `key_events()`,
`any_key_pressed()`, and named `input()` actions, but the editor does not yet
forward their events or action state. Press/release fields remain false, event
lists remain empty, and named actions remain inactive. Other key names and
keyboard input in New Window or Standalone mode are not forwarded yet. The
[Lua guide](Scripting/scriptingLua.md#input) gives exact setup steps and a
copyable controller.

## JavaScript lifecycle and API 1.0

JavaScript assigns the same lifecycle to `globalThis.behavior`:

```javascript
globalThis.behavior = {
  Start() { rustic.log("info", "started"); },
  FixedUpdate(dt) {
    const [x, y, z] = rustic.get_translation();
    rustic.set_translation(x + dt, y, z);
  },
  Update(dt) {},
  OnDestroy() {},
};
```

Lua and JavaScript behaviors can be attached to the same entity. They share engine
state rather than VM values. The JavaScript bridge exposes time, entity identity,
translation, typed public properties, action-state snapshots, logging, and enabled
state. Generated declarations live at `.rustic/generated/programming/rustic_api.d.ts`.

## External process languages

### Selecting the game camera

Pass a scene path or a camera entity ID to `Game.setCurrentCamera` during a gameplay
callback. Lua also accepts the requested single-item table syntax:

```lua
Game.setCurrentCamera({"Game.scene.Room.Camera"})
-- These work too:
Game.setCurrentCamera("Room.Camera")
Game.setCurrentCamera(Game.scene.Find("Room.Camera"))
```

The selected camera becomes active and all other cameras become inactive, regardless
of priority. Projection, transforms, and priorities are preserved. The next game
frame uses this camera in Play, New Window, and Standalone. Missing/stale IDs and
non-camera entities produce a script error without changing camera activation.

| Language | Call |
| --- | --- |
| Lua / Luau | `Game.setCurrentCamera({"Game.scene.Room.Camera"})` |
| JavaScript / HTML inline JavaScript | `Game.setCurrentCamera("Game.scene.Room.Camera")` |
| Python | `Game.setCurrentCamera("Game.scene.Room.Camera")` (also `set_current_camera`) |
| C++ / Java | `Game.setCurrentCamera("Game.scene.Room.Camera");` |
| C# | `Game.SetCurrentCamera("Game.scene.Room.Camera");` (also `setCurrentCamera`) |
| C | `Game_setCurrentCamera("Game.scene.Room.Camera");` |
| PHP | `$Game->setCurrentCamera("Game.scene.Room.Camera");` |

Every supported gameplay language calls the engine-owned API. SDKs are supplied
by Rustic during validation/build and at runtime; generated editor copies live in
`.rustic/generated/programming`. Gameplay scripts do not implement a JSON request
loop, build response commands, or serialize output. Existing custom protocol
programs remain compatible, but new scripts should use the native API.

Lua and JavaScript use embedded bindings. Python, C#, C, C++, Java, PHP, and Luau
use persistent isolated sessions. The engine supplies each callback's owner state,
input, properties, attributes and scene references. External mutations apply after
the callback in issue order. Luau returns a behavior table and keeps its typed
locals alive across callbacks; its engine-owned Luau host handles invocation.
CSS alone is styling; HTML inline JavaScript has the JavaScript API.

### Native callback setup

| Language | Setup |
| --- | --- |
| Python | `from rustic import rustic, instance, Game, run`; define callback functions; `run(globals())` |
| C# | `using static Rustic;`; `Run((callback, dt) => { ... });` |
| C | `#include "rustic.h"`; `rustic_run((RusticBehavior){.on_start=start})` |
| C++ | `#include "rustic.hpp"`; `rustic_run(RusticBehavior{.on_start=start})` |
| Java | `class RusticBehavior extends Rustic`; call `run((callback, dt) -> { ... })` in main |
| PHP | `require __DIR__."/rustic.php"`; `rustic_run(["on_start"=>"on_start"])` |
| Luau | `return {on_start=function() ... end, fixed_update=function(dt) ... end}` |

Each SDK handles omitted callbacks automatically. Supported names are `on_create`,
`on_start`, `on_enable`, `fixed_update`, `update`, `on_disable`, `on_destroy`, and
`on_stop`. Only frame callbacks take dt. Luau also accepts the capitalized Lua
aliases. External collision callbacks remain unavailable. API 1.1 adds shared
`Events` to every supported script type, backed by the same scene services.
See [Shared gameplay actions](Scripting/gameplayActions.md) for coverage and setup. Use `rustic.log` for game diagnostics and run scripts through Play.

See the [language guides](Scripting/README.md) for complete copyable examples,
attachment steps, native return types, current limits, and troubleshooting.

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
- **Manifest mismatch/missing source:** restore or re-import the source, or move it
  through the editor so the asset registry updates automatically;
  duplicate IDs are quarantined instead of guessed.
- **Reload rejected:** check API major, property types, source size, and syntax; use
  Restart Play after an intentionally incompatible change.

## Targeting scene objects by name

Scripts can edit built-in attributes on another object in the currently loaded
scene using `rustic.game.<scene-name>.<hierarchy>`. Lua uses
`rustic.game.Demo.Room.Player:EditAttribute("Position", {1,2,3})`; other languages
use native member calls, subscripts, or path-selection methods. See
[Edit scene objects](Scripting/sceneObjects.md) for exact attachment steps, all
language examples, supported values, and current limitations. This does not
invoke another script's custom functions or load another scene.
