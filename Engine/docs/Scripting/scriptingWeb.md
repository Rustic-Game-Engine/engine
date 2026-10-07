# Scripting Rustic UI assets with HTML and CSS

Web assets are `.html`, `.htm`, or `.css` files under `ui/`. The directory rule is
enforced: they are not accepted as gameplay assets elsewhere. Rustic structurally
validates the document and executes inline `<script>` content in the same bundled,
sandboxed JavaScript behavior runtime used by `.js` scripts. It does **not** embed a
browser renderer and does not replace the editor shell.

Create the file with the Explorer so the editor registers its Asset ID and scope.
Attach it as a global, scene, or object component through the editor; never insert a
path or source string into scene metadata.

## HTML behavior

```html
<!doctype html>
<html>
  <head>
    <style>
      /* Valid content, but not a rendered browser stylesheet in this release. */
      body { margin: 0; }
    </style>
  </head>
  <body>
    <script>
      globalThis.behavior = {
        Start() {
          rustic.log("info", "web behavior started");
        },
        Update(dt) {},
        OnDestroy() {},
      };
    </script>
  </body>
</html>
```

Inline script blocks and project-relative `<script src="...">` files are combined and
evaluated as one sandboxed JavaScript behavior. Linked files must remain inside the
play snapshot. A CSS-only asset has no callbacks and is therefore a valid no-op
behavior. Module imports, DOM queries, events, fetches, storage, and layout APIs are
not available.

Canonical callbacks are `Start`, `FixedUpdate`, `Update`, `OnEnable`, `OnDisable`,
and `OnDestroy`; snake-case spellings remain compatible. Collision callbacks are not
currently bound by the embedded Web/JavaScript adapter. Only methods actually present
on `globalThis.behavior` run.

## Available inline JavaScript API

Inline code receives `rustic`, `Game`, and `instance` exactly as described in
[the JavaScript guide](scriptingJavaScript.md):

```javascript
const [x, y, z] = rustic.get_translation();
rustic.set_translation(x, y + 1, z);
rustic.set_property("score", 10);
rustic.EditAttribute("Color", [1.0, 1.0, 1.0]);

const camera = Game.scene.Find("Room.Camera");
Game.setCurrentCamera(camera);
instance.add("Cube");

if (rustic.key("KeyW").held) {
  rustic.log("info", "forward key held in Play");
}
```

JavaScript mutations are applied in command order after the callback. Properties must
be declared and type-compatible. Scene lookup returns stable IDs; JavaScript
`Game.scene.List()` currently returns path strings. Instance operations are queued
and do not return the created ID.
The [Play input limits](scriptingLua.md#input) also apply to inline JavaScript:
only held WASD, arrow, and Shift state is forwarded from the embedded Play viewport.

## Security and limitations

Inline code cannot call another script object, and API 1.0 does not yet expose
script-level Engine Event `emit`/`subscribe`; use shared engine state for coordination.

Inline code has no DOM, filesystem, network, Node APIs, processes, environment
variables, package loading, or editor/backend access. Source must be UTF-8 and no
larger than 1 MiB. Syntax/structure is validated before Play or reload. A validation
failure leaves the previous good instance running; a callback failure disables only
the failing behavior. Use Web assets for lifecycle-driven UI logic or future-facing
content organization, not for browser rendering in the current release.

## Target another scene object

See [Edit scene objects](sceneObjects.md) for named-scene hierarchy calls, supported
attributes, copyable examples, and native SDK calls to edit another object. Use your language's native call syntax and its current runtime
limitations.
