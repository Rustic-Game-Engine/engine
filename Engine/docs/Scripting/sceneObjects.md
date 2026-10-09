# Edit another scene object

Use a named scene and the object's full hierarchy path to change its built-in
properties from a script attached elsewhere. For example:

```lua
return {
    Start = function()
        rustic.game.Demo.Room.Player:EditAttribute("Position", {1, 2, 3})
        rustic.game.Demo.Room.Player:EditAttribute("Anchored", true)
    end,
}
```

## Set up and run

1. Open a scene named **Demo** in the editor. The name used here is the scene's
   document name, not the script filename or project name.
2. Create a root object named **Room**, then a child named **Player**. Give siblings
   unique names. The full object path is `Room.Player`.
3. Use **Programming > New Script > Object Component Script**, select Lua, and
   paste the example. Save it. Select another object such as **Controller** and
   attach the script using **Programming > Attach Existing Script** or
   **+ Add Component** in the Inspector. Scene/global startup scripts can also
   target objects by path.
4. Press **Play**. Player moves to local position `(1, 2, 3)` and becomes anchored;
   Controller keeps its own position. Changes affect the Play snapshot. Stop Play
   to return to the authored scene, or use the editor's runtime-change workflow
   when you want to retain supported changes.

Only the currently loaded scene is accessible. Referring to another scene does
not load it. Names are case-sensitive. Paths are resolved when a call applies;
if you rename or reparent an object, use its new path in subsequent calls.
These functions edit engine attributes, not another script's public properties
or arbitrary user-defined functions.

## Language syntax

The colon is Lua method-call syntax. Other languages use their own member and
method syntax for the same engine operation. Use these inside the language's
existing startup/update callback, following its scripting guide's setup:

```javascript
// JavaScript and HTML inline JavaScript
rustic.game.Demo.Room.Player.EditAttribute("Position", [1, 2, 3]);
```

```python
# Generated Python SDK
rustic.game.Demo.Room.Player.EditAttribute("Position", [1, 2, 3])
```

```csharp
// Generated C# SDK
rustic.game.Demo.Room.Player.EditAttribute("Position", new double[]{1, 2, 3});
```

```cpp
// C++: runtime names use subscripts rather than compile-time fields
rustic.game["Demo"]["Room"]["Player"].EditAttribute("Position", RusticValue::Array{1.0, 2.0, 3.0});
```

```php
// PHP SDK, inside a callback declaring global $rustic
$rustic->game->Demo->Room->Player->EditAttribute("Position", [1, 2, 3]);
```

Use brackets for a name containing spaces or reserved words, for example
`rustic.game["My Scene"]["Room"]["Player One"]` in Lua, JavaScript, Python,
and C#. C++ already uses brackets; PHP supports `->{"Player One"}`.
A hierarchy segment named `EditAttribute`, `edit_attribute`, `GetAttribute`, or
`get_attribute` can shadow proxy methods. For those names use the full-path helper
through the native SDK in external adapters.
Dots in object names are ambiguous with hierarchy separators; use names without
dots for this path API. Scenes may contain dots in their names because the runtime
matches the complete loaded scene name before resolving the object hierarchy.

```lua
-- Luau uses the same colon syntax as Lua
rustic.game.Demo.Room.Player:EditAttribute("Position", {1, 2, 3})
```

```java
// Java names are selected with methods because fields must exist at compile time
rustic.game.scene("Demo").object("Room.Player").EditAttribute("Position", new double[]{1, 2, 3});
```

```c
// C selects the scene and hierarchy through string arguments
rustic.game.EditAttribute("Demo", "Room.Player", "Position",
    (RusticValue){.type=RUSTIC_VECTOR,.vector={1,2,3},.length=3});
```

All gameplay languages use engine-owned APIs. The engine handles command transport;
use the native calls above without serializing or printing requests/responses.
CSS alone cannot call functions; use inline JavaScript in an HTML UI asset.

Embedded Lua applies edits immediately and supports
`:GetAttribute("Position")` on the same path. JavaScript and external languages
queue edits in issue order and apply them after the callback; they do not expose
path-based getters. Do not use a queued edit followed by an owner getter to read
another object's new value.

## Supported attributes

| Attribute | Value | Meaning |
| --- | --- | --- |
| `Name` | string | Changes the object's name and future path |
| `Position` | three finite numbers | Local translation relative to parent |
| `Size` | three finite numbers | Local transform scale |
| `Color` | three finite numbers | RGB; preserves existing alpha |
| `CanTouch` | boolean | Reserved touch flag; does not affect solid response |
| `CanCollide` | boolean | Enables solid box collisions with other collidable primitives in Play |
| `Anchored` | boolean | Prevents physics movement; unchecked primitives fall under gravity |
| `Parent` | stable entity ID or null | Reparents the target; changes future path |

`EditAttribute` and `edit_attribute` are equivalent. The direct Lua and JavaScript
helper `rustic.edit_object_attribute(source, name, value)` also accepts the full
path string. Material and Enabled are not supported attributes; `set_enabled`
still affects the script owner.

Physics runs after fixed-update callbacks. Imported meshes do not yet simulate;
curved and rotated primitives use enclosing boxes. Physics does not dispatch
collision callbacks. See the **Basic physics** guide for setup and limitations.

## Diagnose failures

- **Scene is not loaded:** check the scene document name and that it is the scene
  being played. The API resolves paths of the form
  `rustic.game.<scene-name>.<hierarchy>`.
- **Object was not found:** match every parent in the Hierarchy; check spelling,
  case, and any earlier rename/reparent. Check that the object exists in Play.
- **Ambiguous:** two objects have the same full path. Rename siblings uniquely.
- **Type mismatch / unknown attribute:** use the table above; vectors must have
  exactly three finite numbers, and booleans must be actual boolean values.
- **Missing SDK member:** regenerate the programming workspace with the updated
  engine. Preserve your script and use the native SDK call shown above. External languages still require their toolchains.

An invalid call reports a scripting error and can fail that behavior. Queued
commands applied before a failure remain applied; a callback is not a transaction.
