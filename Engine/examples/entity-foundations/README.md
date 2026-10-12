# Entity foundations

Two equivalent component scripts exercise live shared runtime services. Use one at
a time; this is a runnable script demonstration, not a serialized starter project.

1. Build/install the engine branch linked in the companion pull request (an older
   release will not contain Tags/Groups/Entity/Components).
2. Create a project and a scene object in the Rustic editor.
3. Use **Programming > New Script > Object Component Script**, choose Lua or Python,
   and replace the generated source with `behavior.lua` or `behavior.py` here.
4. Attach that script to the object with **+ Add Component** or by dragging it from
   Explorer onto the object. Do not edit the scene/manifest metadata manually.
5. Press **Play**. Expect `entity foundations ready` in the Console; the object
   disappears after one scaled fixed second. Stop Play to restore it.

Lua is bundled. Python needs the existing Python 3 toolchain. The engine supplies
`rustic.py`; do not install an unrelated Python package. Keep only one tagged
`enemy` object while running this example, because it asserts exactly one match.
Tags and groups exist only during Play and are not stored in authored scenes.
Paused Play/Clock pause stops the countdown. UI/global scripts should pass a real
scene ID instead of using their synthetic owner.

The companion engine tests execute both scripts against a live SceneWorld, check
that registration reaches Rust, and check destruction and group cleanup. See the
[API reference](https://rustic-game-engine.pages.dev/docs/api/entity-foundations)
for every signature, errors, language differences and limitations.
