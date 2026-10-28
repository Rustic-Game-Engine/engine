# Load, unload and switch scenes

Use `game.scene` to manage scenes from Lua 5.4, Luau, JavaScript, Python, C++,
C#, Java, PHP and HTML with inline JavaScript. C uses `Game.scene`.
The scene open when Play starts is already loaded and active.

## Set up and run

1. Save two scenes with unique document names, such as **Menu** and **Level**.
   Save Level as `scene/level.scene` (legacy `scenes/*.rscene` also works).
   Scene names and scene IDs must both be unique among loaded scenes.
2. Open Menu. Create a **Global Startup Script** through the Programming menu
   and select a language. Paste the matching example into its Start/on_start
   callback, keeping the generated imports and callback registration.
   Object component and scene startup scripts can call these APIs too; they
   belong to their scene and end when that scene is unloaded. Global scripts
   remain alive and can load a scene even when none is active.
3. Save all target scenes before pressing **Play**. Play snapshots include scene
   files from `scene/` and `scenes/`; keeping a file on disk does not load its
   world into runtime memory. Edits made on disk after Play starts require a
   new Play session. The initially open scene includes its current editor state.
4. Run the example. Level becomes the active scene on the next scene transition
   boundary. Menu is unloaded, its scene/object scripts are destroyed, and its
   world, physics, decoded audio, capture state and script runtimes are released.
   Level must contain a camera to produce a camera view.

```lua
return {
    Start = function()
        local level = game.scene.load('scene/level.scene')
        assert(game.scene.isLoaded(level))
        game.scene.switchTo(level)
        game.scene.unload('Menu')
        assert(not game.scene.isLoaded('Menu'))
    end,
}
```

```javascript
// JavaScript, or inside a <script> in a ui/ HTML asset.
globalThis.behavior = {
  Start() {
    const level = game.scene.load('scene/level.scene');
    game.scene.switchTo(level);
    game.scene.unload('Menu');
  }
};
```

Use the same Lua example for Luau. Other language callback bodies:

```python
# from rustic import game, run
# Register with run(globals()) after defining callbacks.
def on_start():
    level = game.scene.load('scene/level.scene')
    game.scene.switchTo(level)
    game.scene.unload('Menu')
```

```cpp
// #include "rustic.hpp"; register start with RusticBehavior.on_start.
void start() {
    auto level = game.scene.load("scene/level.scene");
    game.scene.switchTo(level);
    game.scene.unload("Menu");
}
```

```csharp
// using static Rustic; inside the generated on_start callback:
var level = game.scene.load("scene/level.scene");
game.scene.switchTo(level);
game.scene.unload("Menu");
```

```c
// #include "rustic.h"; register start with RusticBehavior.on_start.
void start(void) {
    const char *level = Game.scene.load("scene/level.scene");
    Game.scene.switchTo(level);
    Game.scene.unload("Menu");
}
```

```java
// Inside a Rustic subclass's on_start callback:
String level = game.scene.load("scene/level.scene");
game.scene.switchTo(level);
game.scene.unload("Menu");
```

```php
// require rustic.php and register on_start with rustic_run.
function on_start(): void {
    global $game;
    $level = $game->scene->load("scene/level.scene");
    $game->scene->switchTo($level);
    $game->scene->unload("Menu");
}
```

See the [language guides](/docs/guides/api-languages) for complete SDK setup.
`Game.scene` also exposes these methods in each language. CSS alone has no
callbacks; HTML runs the JavaScript API. PHP and HTML remain UI script types.

## Calls and results

| Call | Result |
| --- | --- |
| `game.scene.load(path)` | Read a saved relative scene file, build its world, and return its document name. Does not activate it or run its startup callbacks. |
| `game.scene.switchTo(scene)` | Select an already loaded document name. Scene/object startup callbacks run on first activation. |
| `game.scene.unload(scene)` | Remove that document name from the loaded set; discard its unsaved runtime state and release its world and scene-owned runtime resources. |
| `game.scene.isLoaded(scene)` | Return whether the document name is loaded. |
| `game.scene.getLoaded()` | Return loaded document names in deterministic name order. |
| `game.scene.getActive()` | Return the selected document name, or nil/null/None when no scene is selected. |

`load` returns a string in every language. Lua/Luau/JS/Python/C#/Java/PHP return
native lists from `getLoaded`; C returns `RusticList` (`count`, `items`), and
C++ returns a `RusticValue` array. C++ `getActive` returns a nullable `RusticValue`;
check `is_null()` before `string()`. C query strings and lists last until the end
of the callback; copy strings if keeping them across callbacks.

## Lifetime and transitions

Loading allocates only that scene's live world. Switching preserves other loaded
worlds, edits and script state. Inactive loaded scenes do not tick or render;
switching back resumes them without rerunning Start. Unloading releases a scene
instead of caching it. Reloading reads the saved file and starts fresh.

Calls update the loaded set and selected name synchronously, so loading and then
switching in one callback works. Scene runtime activation, teardown callbacks and
final memory release happen at the runtime boundary after the current callback
batch. Code already running can finish; return after requesting a transition if
it should do no more work on the old scene. A scene selected from a newly started
scene's Start callback is activated at the following boundary.

Unloading the active scene clears selection without choosing a fallback.
Global scripts continue, but the view is empty until a loaded scene is selected.
Scene/object OnDestroy and OnStop run on unload. Global scripts end only when
Play ends or they are disabled. The engine owns shared renderer/asset caches;
those are separate from the scene world and its scene-owned runtime state.

Object-path lookup and rendering use the active scene. Apply Runtime Changes
still compares the originally opened scene if it remains loaded; it does not save
other scenes. Explicitly save authored changes before Play; runtime unload
never writes to scene files.

## Errors and diagnosis

Switching to an unknown or unloaded name raises a script error and preserves the
selection. Loading a duplicate name or scene ID, or unloading an unknown name,
also raises an error. Catch expected errors with `pcall`/try/catch as appropriate.

`load` takes a relative project file path, while switch/unload/query calls take
the **document name returned by load**. Paths cannot escape the Play snapshot.
Missing files, invalid RON, invalid world data, incompatible schema versions and
checksum mismatches raise errors without adding a loaded scene. The loader does
not silently use a backup. Open and save externally edited scene files in the
editor to refresh their checksums, then start Play again.

If a target's script source fails to load at first activation, the Console reports
its startup failure and clears the selection. Fix the source, then reload the
scene. Missing script attachments retain the existing warning-and-skip behavior.
Scene lifecycle mutations are rejected during final Play teardown.

## Rust host API

Rust hosts can also use `engine_world::SceneManager` with `load`, `load_file`,
`switch`, `unload`, `is_loaded`, `loaded_ids`, and active/world borrowing methods.
That lower-level manager owns ECS worlds only; the scripting runtime above also
coordinates callbacks, physics and audio lifetime.
