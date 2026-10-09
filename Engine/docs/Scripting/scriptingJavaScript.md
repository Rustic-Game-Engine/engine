# Scripting Rustic games with JavaScript

JavaScript is bundled and sandboxed. A script assigns one behavior object to
`globalThis.behavior`; it does not export a Node module and cannot use Node or browser
APIs. Create `.js` or `.mjs` gameplay files with the editor so an Asset ID and the
chosen global, scene, or component attachment are created automatically.

## Behavior lifecycle

```javascript
let speed = 4; // private to this behavior instance

globalThis.behavior = {
  Start() {
    rustic.log("info", `started ${rustic.entity_id()}`);
  },
  OnEnable() {},
  FixedUpdate(dt) {
    const move = rustic.key("KeyW");
    if (move.held) {
      const [x, y, z] = rustic.get_translation();
      rustic.set_translation(x, y, z + speed * dt);
    }
  },
  Update(dt) {},
  OnDisable() {},
  OnDestroy() {},
};
```

Every method is optional. Canonical names are `Start`, `FixedUpdate`, `Update`,
`OnEnable`, `OnDisable`, and `OnDestroy`; legacy `on_start`, `fixed_update`, `update`,
`on_enable`, `on_disable`, and `on_destroy` remain compatible. Canonical wins when
both exist. `on_create` and `on_stop` remain recognized legacy internal hooks.
Collision names exist in the common scheduler contract, but the current JavaScript
adapter does not bind them yet.

Global scripts start before scene scripts, which start before object components.
Within a scope, execution order, entity ID, script Asset ID, and attachment order are
the stable tie-breakers. Disabling an object stops its component updates; destruction
invokes teardown and unregisters the instance.

## API reference

```javascript
const id = rustic.entity_id();
const dt = rustic.delta_time();
const fixedDt = rustic.fixed_delta_time();
const [x, y, z] = rustic.get_translation();
rustic.set_translation(x + 1, y, z);

const health = rustic.get_property("health");
rustic.set_property("health", 90);
const color = rustic.GetAttribute("Color"); // [red, green, blue]
rustic.EditAttribute("Anchored", true);
print("loaded", id);
warn("message");
console.debug("details");
rustic.set_enabled(false);
```

`print(...)`, `warn(...)`, and `console.log/info/warn/error/debug(...)` write directly
to the Rustic console. `rustic.log(level, message)` remains available when code needs
to select a level dynamically.

Lower-case `get_attribute`/`edit_attribute` are equivalent. Set `Color`, `Position`,
and `Size` with three-number arrays, and `Parent` with an entity ID string or `null`.
Supported built-ins are
`Name`, `Position`, `Size`, `Color`, `CanTouch`, `CanCollide`, `Anchored`, and
`Parent`. Properties must already be declared and writes must preserve their type.
Mutations are queued and applied in issue order after the callback.

## Scene, instances, cameras, and input

```javascript
const camera = Game.scene.Find("Room.Camera");
const rootTable = Game.scene.Table;
const paths = Game.scene.List(); // scene path strings in JavaScript
instance.add("Cube");
instance.clone("assets/models/chair.obj", camera); // optional parent ID
Game.setCurrentCamera(camera);

if (rustic.key("KeyW").held) {
  rustic.log("debug", "forward key is held");
}
```

`Find` and direct scene properties return a stable entity ID or `undefined`.
The current JavaScript `List` implementation returns scene path strings, unlike Lua
and the external SDKs, whose `List` returns IDs. An instance source can be a stable
ID, scene path, model path, or built-in object name. Instance calls queue creation and
do not return the new ID. A parent, when supplied, must be a stable ID.

The editor's embedded Play viewport currently forwards held WASD, arrow, and Shift
keys. Use `rustic.key(name).held` with `KeyW`, `KeyA`, `KeyS`, `KeyD`,
`ArrowUp`, `ArrowDown`, `ArrowLeft`, `ArrowRight`, `ShiftLeft`, or `ShiftRight`.
The API exposes `pressed`, `released`, `key_events()`, `any_key_pressed()`, and
named `input()` actions, but those values are not populated by the current Play
bridge. Other key names and separate runtime windows do not forward input yet.
For focus and setup steps, see the [Lua input guide](scriptingLua.md#input); the
input transport and limitations are the same for JavaScript.

## Sandbox, values, and diagnostics

JavaScript cannot obtain another behavior's VM object. API 1.1 adds shared
`Events.on`, `once`, `emit`, and object signals; see [Shared gameplay actions](gameplayActions.md).

There is no DOM, `window`, Node `require`, module loader, filesystem, network,
environment, native browser timers, process, or editor/backend access.
Use the engine-owned `Timer` facade for delayed/repeating gameplay callbacks. Script-visible state and API
objects are frozen; do not attempt to modify them. Values crossing the bridge are
JSON-compatible representations of the shared engine types. A callback exception or
instruction-budget failure disables that behavior, while other scripts continue.
Generated type declarations are at
`.rustic/generated/programming/rustic_api.d.ts`; generated files may be regenerated,
so never put game logic there.

## Target another scene object

See [Edit scene objects](sceneObjects.md) for named-scene hierarchy calls, supported
attributes, copyable examples, and native SDK calls to edit another object. Use your language's native call syntax and its current runtime
limitations.

## Shared gameplay actions

API 1.1 exposes shared-core easing, tweens, movement, skeletal/keyframe/procedural
animation, timelines, timers, paths, cameras, physics, effects, audio and signals.
See [Shared gameplay actions](gameplayActions.md) for attachment, native call
conventions, duration/speed options, callbacks, scene-clock controls and backend limits.
