# Scripting Rustic games with Java

Java behaviors use the built-in Rustic API. Define callbacks and call
`rustic.get_translation()` (PHP: `$rustic->get_translation()`). Rustic supplies the
SDK, dispatches lifecycle calls, and handles communication internally. Scripts do
not parse requests, build commands, serialize JSON, or print responses.

## Setup and attachment

1. Install OpenJDK 11 or newer: java and javac on PATH. Restart the editor after changing PATH.
   Run `cargo xtask doctor` from the Engine folder to check discovery.
2. Open your game project and scene. Select a **Part** object for this movement example.
3. Use **Programming > New Script > Object Component Script** and select Java.
   Save the asset, then attach it to the selected object using
   **Programming > Attach Existing Script** or **+ Add Component** in the Inspector.
   Create global or scene scripts through their corresponding Programming commands.
   Keep the editor-assigned Asset ID; do not edit the registry or scene files by hand.
4. Paste the complete example below. Press **Play**, then hover or click the embedded
   Play viewport and hold **W**. The owner moves along positive Z at one unit per
   second. The Console shows **Behavior started** once. Stop Play to restore the
   authored scene. Attach to an object with a transform to see movement.

## Complete behavior

```java
class RusticBehavior extends Rustic {
    public static void main(String[] args) throws Exception {
        run((callback, dt) -> {
            if (callback.equals("on_start")) rustic.log("info", "Behavior started");
            if (callback.equals("fixed_update") && rustic.key("KeyW").held) {
                double[] p = rustic.get_translation();
                rustic.set_translation(p[0], p[1], p[2] + dt);
            }
        });
    }
}
```

The SDK is staged automatically beside the source in an engine temporary directory.
You do not install a package, copy the SDK into your game, or write a process loop.
**Programming > Open Programming Workspace** also writes editor support files under
`.rustic/generated/programming`. Regenerate those files after upgrading Rustic.
Edit your behavior source, not generated SDK files. Run through Rustic Play so the
engine can supply the API and callback state.

## Built-in functions

All gameplay languages expose owner ID and timing, translation, declared public
properties, built-in attributes, held-key input, logging, enabled state, scene lookup,
instance creation, and camera selection. Use native member syntax for your language.

| API | Result or effect |
| --- | --- |
| `rustic.entity_id()` | Stable owner ID string |
| `rustic.delta_time()`, `rustic.fixed_delta_time()` | Seconds |
| `rustic.get_translation()` | Three numbers; C uses `RusticVector3` with x/y/z |
| `rustic.set_translation(x,y,z)` | Queues owner movement |
| `rustic.get_property(name)`, `rustic.set_property(name,value)` | Reads or updates an already declared property |
| `rustic.get_attribute(name)`, `rustic.edit_attribute(name,value)` | Reads or edits an owner built-in attribute |
| `rustic.key(name)`, `rustic.input(name)` | Action state with pressed/released/held/axis |
| `rustic.key_events()`, `rustic.any_key_pressed()` | Input event snapshot |
| `rustic.log(level,message)`, `rustic.set_enabled(enabled)` | Logs or queues enabled state |
| `Game.scene.Find(path)`, `Game.scene.List()` | Finds an ID or lists matching IDs |
| `instance.add(source,parent)`, `instance.clone(source,parent)` | Queues creation; no immediate new ID |
| `Game.setCurrentCamera(source)` | Queues camera selection by path or ID |

Mutations apply after the callback, in call order. Getters read the callback's
snapshot, so a getter after a setter still reads the original state. Properties and
attributes must retain their engine types; numbers must be finite. Attribute names
are `Name`, `Position`, `Size`, `Color`, `CanTouch`, `CanCollide`, `Anchored`, and
`Parent`. Color is three RGB numbers. Instance parents must be stable entity IDs;
resolve a path with `Game.scene.Find` first. Scene List returns IDs, not path strings.

## Callbacks and values

Supported lifecycle names are `on_create`, `on_start`, `on_enable`, `fixed_update`,
`update`, `on_disable`, `on_destroy`, and `on_stop`. Only frame callbacks receive
`dt` (seconds). Omitted callbacks are handled automatically. C and C++ register
function pointers in `RusticBehavior`; Python passes a callback dictionary to `run`;
PHP passes one to `rustic_run`; C# and Java dispatch the supplied callback name.
Luau returns a table and also accepts `Start`, `FixedUpdate`, `Update`, and the other
capitalized lifecycle aliases used by Lua. State declared outside callbacks persists
for this behavior instance until teardown or reload.

Python and PHP use native dictionaries/arrays for key state. C#, Java, and C use
native action structs/objects with `.held`. Luau uses tables and returns translation
as three separate numbers. C property/attribute reads return `RusticValue`: inspect
its `type` and use `boolean`, `number`, `string`, or `vector`/`length`. C strings and
read values last until the next callback; copy them if you need to retain them.
C# property/attribute reads return ordinary `object?` values (bool, long/double,
string, double[] or null); Java returns Object values (Boolean, Long/Double,
String, double[] or null). Cast to the declared property type before arithmetic.
C scene listing takes a path argument, e.g. `Game.scene.List("Game.scene")`, and
returns a `RusticList` with count/items.

## Current limits and diagnosis

The embedded Play viewport forwards held WASD, arrows, and Shift. Other keys,
press/release events, named actions, and separate runtime-window input are not wired
in this build. Collision callbacks and cross-language event emit/subscribe are not
exposed by these external SDKs. Coordinate through shared engine state.

Each instance runs in its own process with a safe environment allowlist and a
temporary working directory. Sources and responses are limited to 1 MiB; callbacks
have three seconds to finish. An exception, invalid engine value, process exit,
timeout, or rejected call disables the behavior. Failed build/reload validation
keeps the last good instance running.

- **Toolchain unavailable:** check PATH in the editor's environment, then restart it.
- **Nothing moves:** check attachment, owner transform, Play focus, and held KeyW.
- **Property rejected:** declare it in the editor and preserve its type.
- **Wrong parent:** pass the ID returned by Find, not the path string.
- **SDK missing when running manually:** run the behavior through Rustic Play.
- **Callback failed:** read the Rustic Console/build diagnostic. Use `rustic.log`
  for gameplay logs; external stdout belongs to the engine's private transport.
- **Old script has a request loop:** replace it with the callback setup above.
  If the old starter defines its own SDK classes, remove those definitions and
  keep your gameplay logic in the callbacks. Rustic now supplies the SDK.

## Target another scene object

See [Edit scene objects](sceneObjects.md) for named-scene hierarchy calls, supported
attributes, copyable examples, and native calls to edit another object. Use your language's native call syntax and its current runtime
limitations.

## Shared gameplay actions

API 1.1 exposes shared-core easing, tweens, movement, skeletal/keyframe/procedural
animation, timelines, timers, paths, cameras, physics, effects, audio and signals.
See [Shared gameplay actions](gameplayActions.md) for attachment, native call
conventions, duration/speed options, callbacks, scene-clock controls and backend limits.
