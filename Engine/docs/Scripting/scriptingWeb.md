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

JavaScript mutations are applied in command order after the callback. Canonical
`game.scene.Scene.ObjectPath` target resolution flushes earlier queued commands
before resolving the live object, including renames and reparenting. Properties must
be declared and type-compatible. Scene lookup returns stable IDs; JavaScript
`Game.scene.List()` currently returns path strings. Instance operations are queued
and do not return the created ID.
The [Play input limits](scriptingLua.md#input) also apply to inline JavaScript:
only held WASD, arrow, and Shift state is forwarded from the embedded Play viewport.

## Security and limitations

Inline code cannot call another script object. API 1.1 adds shared `Events`
and gameplay actions to inline JavaScript; see [Shared gameplay actions](gameplayActions.md).

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

## Shared gameplay actions

API 1.1 exposes shared-core easing, tweens, movement, skeletal/keyframe/procedural
animation, timelines, timers, paths, cameras, physics, effects, audio and signals.
See [Shared gameplay actions](gameplayActions.md) for attachment, native call
conventions, duration/speed options, callbacks, scene-clock controls and backend limits.

A missing linked script is skipped with a runtime warning for that play session.
Remaining linked and inline scripts continue running. Restore the file, stop Play,
and start Play again to load it. Missing files are not retried during the session.

## Optional visual UI authoring

In the native editor's **Explorer**, right-click an `.html`, `.htm`, or `.css` file
inside the project's `ui/` directory and choose **Edit UI Visually**. Normal **Open**
continues to use your code editor. The action opens a dockable **Visual UI** panel
and switches **Properties** to **UI Properties**. For CSS, choose one of the HTML
documents that links that stylesheet; a stylesheet without a referencing HTML
document cannot be opened visually.

Start with a file such as `ui/hud.html`:

```html
<!doctype html>
<html><body>
  <div id="hud" style="position:absolute;left:20px;top:20px;width:320px;height:100px;background-color:#202020">
    <button style="position:absolute;left:12px;top:12px;width:160px;height:48px;color:#ffffff">Start</button>
  </div>
</body></html>
```

### Build and organize elements

1. Choose **Add element → Panel** to create a container (`div`). Give it a **Name**
   to find it easily; this is saved as its HTML ID.
2. Keep the panel selected and use **Add element → Text**, **Button**, or **Image**
   to add children inside it. Panels can contain other panels at any depth. Use
   **Add at page level** to create an element outside the selection.
3. Select elements in the **Elements and layers** tree. The arrow expands children;
   clicking the name selects the element without collapsing it.
4. Use the **Parent** menu to move the selected element and its children into
   another container, or choose **Page (root)** to move it out. The canvas position
   stays the same. The editor excludes the element itself, its descendants, and
   parents that cannot legally contain it, such as images or buttons containing panels.

Properties are grouped into **Layout**, **Text**, **Appearance**, **Layer order**,
and **HTML attributes**. Layout has **X position**, **Y position**, **Width**, and
**Height**, in pixels. The canvas **Move** and **Scale** handles still work.
**Layer order** includes numeric order and **Send to back / Bring to front**;
**HTML attributes** keeps ID, class, and image source/alt text available.
**Duplicate** copies a subtree; **Delete** removes the element and its children.
**Show scene properties** returns to scene editing.

### Colors, gradients, and rounded corners

In **Appearance → Background**, choose **Solid** or **Gradient**. A linear gradient
starts with two colors. Change each color and its percentage position, set the
angle, and use **Add color stop** or **Remove** to adjust the gradient (2–16 stops).
Angles follow CSS: 0° goes up, 90° right, 180° down, and 270° left.

**Text → Text color** offers the same solid and gradient controls. Select a text-only
element, such as Text or a Button containing plain text. If an element has other
editable children, select its text child to change text appearance. Background and
text gradients can coexist. Gradients preview on the canvas and save as ordinary
CSS `linear-gradient(...)` styles; text fills use a nested text span so the parent
can keep its own background.

In **Appearance → Corner radius**, leave **Link all corners** checked to change all
four corners with one value. Uncheck it to edit **Top left**, **Top right**,
**Bottom right**, and **Bottom left** separately. Linking different corners uses
the current top-left value for all four. Each radius supports 0–255 pixels and
previews with both solid and gradient backgrounds.

**Undo UI** and **Redo UI** affect the UI document independently of scene history.
A transform drag is one undo step. **Save UI** writes the HTML, retaining the previous
file as `.bak`; scene Save and scene autosave do not save UI changes. HTML formatting
is normalized by the HTML parser when edited; scripts, styles, and unrelated
attributes remain in the document. CSS preview does not copy stylesheet rules into
the saved HTML: visual edits write inline overrides. Save or discard changes before
opening another UI document. Closing the editor with unsaved UI changes offers save,
discard, and cancel. Editing is disabled during Play and in read-only projects.

The canvas is a native layout preview, not a browser. It supports inline pixel
positions and sizes, hex colors, linear gradients with hex color stops, pixel corner
radii, and flat selector rules from inline stylesheets
and local linked CSS files in `ui/`. Stylesheet rules are applied in document order,
then inline styles; full CSS specificity, `!important`, inheritance, flex/grid,
percentage sizing, media queries, image rendering, DOM events and script execution
are not supported by the preview. Image source and alt text can still be authored.
Use positioned containers for nested layouts. This feature does not add browser UI
rendering during Play.

If the action is absent, check the extension and that the file is inside `ui/`.
If CSS has no document choices, add a relative `<link rel="stylesheet" href="...">`
from an HTML file under `ui/`. If Save reports an external change, keep the document
open to retain your edits, or discard and reopen to load the disk version; Save
will not overwrite that external version. Files must be UTF-8 and at most 1 MiB;
paths and symlinks that leave `ui/` are rejected. Open failures appear in Terminal.
