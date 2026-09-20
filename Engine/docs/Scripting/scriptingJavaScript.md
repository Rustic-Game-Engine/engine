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
rustic.log("warn", "message");
rustic.set_enabled(false);
```

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

const action = rustic.input("Jump");
const key = rustic.key("Space");
for (const event of rustic.key_events()) {
  rustic.log("debug", `${event.key}: ${event.state}`);
}
if (rustic.any_key_pressed()) { /* one or more press edges */ }
```

`Find` and direct scene properties return a stable entity ID or `undefined`.
The current JavaScript `List` implementation returns scene path strings, unlike Lua
and the external SDKs, whose `List` returns IDs. An instance source can be a stable
ID, scene path, model path, or built-in object name. Instance calls queue creation and
do not return the new ID. A parent, when supplied, must be a stable ID.

Input states expose `pressed`, `released`, `held`, and `axis`; key events also expose
`repeat`. Use physical names such as `KeyW`, `Space`, `ArrowLeft`, and `Escape`.

## Sandbox, values, and diagnostics

JavaScript cannot obtain or invoke another behavior's VM object. API 1.0 does not yet
expose script-level Engine Event `emit`/`subscribe`; cross-language coordination must
use shared engine state.

There is no DOM, `window`, Node `require`, module loader, filesystem, network,
environment, timer, process, or editor/backend access. Script-visible state and API
objects are frozen; do not attempt to modify them. Values crossing the bridge are
JSON-compatible representations of the shared engine types. A callback exception or
instruction-budget failure disables that behavior, while other scripts continue.
Generated type declarations are at
`.rustic/generated/programming/rustic_api.d.ts`; generated files may be regenerated,
so never put game logic there.
