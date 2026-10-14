# Call APIs through scene and file paths

All ten scripting types provide the same three roots. Choose the root from the
thing the call operates on:

| Target | Root | Lua / Luau example |
| --- | --- | --- |
| An object in a named scene | `game.scene.SceneName.ObjectPath` | `game.scene.Demo.Room.Player:tween("Position", {8,0,0}, 2)` |
| A service that needs no scene object | `game.rustic` | `game.rustic:after(1, function() print("ready") end)` |
| A file in the Explorer | `game.rustic.files.ExplorerPath` | `game.rustic.files.assets.sounds["hit.wav"]:load()` |

The scene path supplies the object argument. The file path supplies the source
file argument. Pass the remaining arguments in their usual order. Returned action
handles, voices, timers and builders keep their existing methods and lifecycle.
Old `Tween`, `Movement`, `Game.scene`, `rustic.game`, and other service calls remain
available for existing scripts.

## Set up and run

1. Open a scene whose document name is **Demo**. Create a root named **Room**, a
   child named **Player**, and a separate root named **Controller**. Give siblings
   unique names. Player's complete hierarchy path is `Room.Player`.
2. Select Controller. Use **Programming > Object Component Script**, choose Lua,
   paste the example below, save, and attach it to Controller with
   **Programming > Attach Existing Script** or **+ Add Component** in the Inspector.
3. Press **Play**. Player moves to local position `(8, 0, 0)` over two scene-clock
   seconds. Controller retains its position. After one scene-clock second the log
   shows `ready`. Stop Play to return to the authored scene.

```lua
return {Start=function()
    game.scene.Demo.Room.Player:tween("Position", {8,0,0}, 2, "OutCubic")
    game.rustic:after(1, function() print("ready") end)
end}
```

Global and Scene startup scripts can also use named paths; the target does not
have to own the script. Calls resolve the live loaded scene, including edits
queued earlier in the same callback. A path retained in a variable remains a
path: after renaming or reparenting its target, use the new hierarchy path.
An unloaded scene, missing object, or duplicate full hierarchy path is rejected
rather than selecting a different object. Only the currently loaded scene is
addressable.

## Native syntax in every language

These snippets go inside your existing `Start` / `on_start` callback. Each starts
the same Player tween. See [Scopes, lifecycle and execution](/docs/guides/scripting)
for creating and attaching scripts in each language.

| Language | Scene object call |
| --- | --- |
| Lua 5.4 / Luau | `game.scene.Demo.Room.Player:tween("Position", {8,0,0}, 2)` |
| JavaScript / HTML inline JS | `game.scene.Demo.Room.Player.tween("Position", [8,0,0], 2);` |
| Python | `game.scene.Demo.Room.Player.tween("Position", [8,0,0], 2)` (import `game` from `rustic`) |
| C# | `game.scene["Demo"]["Room"]["Player"].tween("Position", new double[]{8,0,0}, 2);` (use `using static Rustic;`) |
| C++ | `game.scene["Demo"]["Room"]["Player"].tween("Position", RusticValue::Array{8,0,0}, 2.0);` |
| Java | `game.scene.scene("Demo").object("Room.Player").tween("Position", new double[]{8,0,0}, 2);` (inside a `Rustic` subclass) |
| PHP | `$game->scene->Demo->Room->Player->tween("Position", [8,0,0], 2);` (declare `global $game;` in callbacks) |

C has no implicit receiver or arbitrary named members. Select a path value and
pass it explicitly as `self`; the selected value supplies the target:

```c
RusticScenePath player = game.scene("Demo", "Room.Player");
player.tween(player, "Position",
    (RusticValue){.type=RUSTIC_VECTOR,.vector={8,0,0},.length=3},
    2, "Linear", NULL);
```

Use subscripts for names with spaces and filenames with extensions in Lua,
Luau, JavaScript, Python, C# and C++. For example,
`game.scene["Demo Scene"]["Room"]["Player One"]`.
Java accepts the scene and complete object path as strings. PHP accepts a
property such as `$game->scene->{"Demo Scene"}->Room->{"Player One"}`.

## Object methods

Short names cover common calls. Family-prefixed names distinguish APIs whose
original names overlap. Both forms bind the receiver to the target:

| Family | Methods on `game.scene.Scene.ObjectPath` |
| --- | --- |
| Attributes | `EditAttribute`, `edit_attribute` (`GetAttribute` / `get_attribute` in embedded Lua) |
| Tween | `tween` / `tweenTo`, `tweenMove`, `tweenRotate`, `tweenScale`; action/options variants where already supported |
| Movement | `moveTo`, `move`, `rotateTo`, `lookAt`, `follow`, `orbit`; also `movementMoveTo`, `movementMove`, `movementRotateTo`, `movementLookAt`, `movementFollow`, `movementOrbit` |
| Paths | `followPath` / `pathFollow`, `pathFollowSpeed` where already supported |
| Effects | `fade`, `flash`, `shake`, `pulse`; also the `effects` prefix, such as `effectsFade` |
| Camera | `setCurrentCamera()`, `cameraMoveTo`, `cameraZoom`, `cameraFov`, `cameraFollow`, `cameraOrbit`, `cameraLookAt`, `cameraShake`, `cameraTransition` |
| Physics | `impulse`, `force`, `launch`, `knockback`; also the `physics` prefix |
| Animation | `playAnimation` / `animationPlay`, `animationRegister`, `animationClips`, `animationBlend`, `animationTransition`, `animationAddMarker`, `animationIk`, `animationFootPlacement`, `animationLookAt`, `animationHeadTracking`, `animationRecoil`; registration/action variants where already supported |
| Events | `eventsConnect(name, callback)`, `emit(name, arguments)` |
| Tags and groups | `addTag` / `tagsAdd`, `removeTag` / `tagsRemove`, `groupsAdd(groupName)` |
| Hierarchy | `setParent` / `hierarchySetParent`, `getChildren` / `hierarchyGetChildren`, `detach` / `hierarchyDetach` |
| Instances | `clone(parent)`, `add(parent)` |
| Lifetime | `destroy` / `entityDestroy`, `destroyAfter` / `entityDestroyAfter`, `isValid` / `entityIsValid` |
| Components | `hasComponent` / `componentsHas`, `getComponents` / `componentsGetAll`, `findInChildren` / `componentsFindInChildren` |

Arguments and backend limits are those of the existing API in your language,
minus the receiver argument. For example, `groupsAdd` binds the **second** original
argument; pass only the group name. In languages whose follow/hierarchy APIs take
string targets, pass another canonical scene path string, such as
`"game.scene.Demo.Room.Leader"`. Lua, JavaScript, Python and PHP also accept scene
path objects as these arguments.

## Global methods

Call timers with `game.rustic:after(...)` or `:every(...)` in Lua/Luau, and native
member calls in other languages. Interpolation uses `lerp`, `slerp`,
`inverseLerp`, `remap`, and `smoothDamp`. World physics queries use `raycast`,
`sphereCast`, `overlap`, and `explosion`. Clock operations use `timeScale`,
`pause`, and `resume`; camera selection can use the scene object’s `setCurrentCamera()` method
(or the global `setCurrentCamera(path)` convenience); `currentCamera()` reads
the current camera where supported. Scene lighting uses `getEnvironment()` and
`setEnvironment(settings)`.

Builders use `sequence()` and `timeline()`, and curves use `createPath(...)`.
Value animation uses `tweenValue(...)` / `animationValue(...)`. Global signals use
`on`, `once`, `emit`, and `disconnect`. Audio device enumeration/selection keeps
its snake case names; voice operations use `audioVolume`, `audioPitch`,
`audioFadeIn`, `audioFadeOut`, and `audioCrossfade`. Tag/group/world searches use
`tagsFindAll`, `groupsCreate`, `groupsGetMembers`, `worldFindByTag`, and
`worldFindInRadius`.

Family-prefixed equivalents are available, such as `timerAfter`, `smoothLerp`,
`clockPause`, `physicsRaycast`, and `sequenceNew`. The existing owner API is also
reachable under `game.rustic`, including `entity_id`, `delta_time`, `input`,
`key`, `get_property`, and `log`. These owner calls keep their existing scope.
No adapter gains an API its previous SDK did not implement.

## Explorer file methods

The Explorer path begins at the project root; include `assets` where applicable.
Each selected child adds one `/` to the source path. An extension stays in a
single filename segment: `["hit.wav"]`, not `.hit.wav`.

For Lua/Luau audio, import a WAV, MP3, OGG, FLAC or AAC file as
`assets/sounds/hit.wav` (use its actual extension) and ensure it appears in the Play
snapshot. Load the selected file before playback:

```lua
local sound = game.rustic.files.assets.sounds["hit.wav"]
sound:load()
local voice = sound:play({volume=0.5})
-- Later, release the decoded clip:
sound:Unload()
```

Separate expressions selecting the same Lua/Luau file retain its load state.
`:play()` before `:load()` reports an error. Other language SDKs keep their
existing direct playback model, for example:

```javascript
game.rustic.files.assets.sounds["hit.wav"].play({volume: 0.5});
```

```python
game.rustic.files.assets.sounds["hit.wav"].play(volume=0.5)
```

```cpp
game.rustic.files["assets"]["sounds"]["hit.wav"].play(0.5);
```

C# uses the same subscripts and `.play(...)`. Java uses
`game.rustic.files.object("assets/sounds/hit.wav").play(0.5, 1, false)`.
PHP uses `$game->rustic->files->assets->sounds->{"hit.wav"}->play(...)`.
C selects `RusticFilePath sound=game.rustic.files("assets/sounds/hit.wav")` and
calls `sound.play(sound, &options)`.

Other file operations are `playAt` / `audioPlayAt`, `loadAnimation(object)`,
`add(parent)`, `clone(parent)`, and `setSkyTexture()`, using each language's
existing optional arguments. Loading animation binds the file's source argument
and still requires the scene entity argument. In Lua/Luau, file recordings use
`game.rustic.files["capture.wav"]:startRecording(options)`; recording without a
file and microphone capture use `game.rustic:startRecording()` and
`game.rustic:startHotMic()`.

File paths retain the backend's validation. Missing assets, paths outside the
snapshot and unsupported file types fail with the existing diagnostics.

## Troubleshooting

- **Scene not loaded:** use the document name of the scene currently in Play.
- **Object missing:** include every parent, check exact spelling, and use the new
  path after renaming or reparenting. Creating a path value does not validate it;
  calling a method resolves it.
- **Ambiguous object:** rename siblings so the complete hierarchy path is unique.
- **Lua argument errors:** use `:` on the canonical roots and scene/file objects.
- **File missing:** select each directory separately and keep the extension in
  one segment. Verify that the asset is included in the Play snapshot.
- **API unavailable:** check the original API's language support and signature in
  [Use gameplay actions](/docs/guides/gameplay-actions). The path facade preserves
  those capabilities and arguments.
