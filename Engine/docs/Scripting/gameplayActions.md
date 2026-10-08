# Shared gameplay actions (API 1.1)

The engine now owns an action scheduler shared by scripting languages. Easing,
interpolation, path traversal, composition, and lifetime cleanup run in Rust. The
script only starts an action and handles its callbacks. You do not edit JSON or
advance an action manually from `Update`.

The same modules are available in Lua 5.4, Luau, JavaScript, Python, C, C++, C#,
Java, PHP, and HTML inline JavaScript. Named skeletal clips, procedural actions,
physics queries, audio voices, and synchronous interpolation use engine services.

## API reference pages

- [Tween](/docs/api/tween) — Animate an entity property or script value without writing an Update loop.
- [Ease](/docs/api/ease) — Choose one of 31 shared easing curves for actions and synchronous interpolation.
- [Movement](/docs/api/movement) — Move, turn, follow and orbit objects with finite engine-scheduled actions.
- [Animation](/docs/api/animation) — Play imported skeletal clips, register property tracks, animate script values and run procedural poses.
- [Sequence and Timeline](/docs/api/sequence) — Compose sequential and parallel actions without nesting timer callbacks.
- [Timer](/docs/api/timer) — Schedule one-shot or repeating scene-clock callbacks.
- [Smooth and Interpolation](/docs/api/smooth) — Evaluate interpolation and damping immediately without scheduling an action.
- [Path](/docs/api/path) — Traverse linear, Bezier and spline paths with global and per-segment easing.
- [Camera actions](/docs/api/camera-actions) — Move, aim, zoom, follow and shake a game camera using shared actions.
- [Physics queries and forces](/docs/api/physics-actions) — Query live primitive colliders and change simulated body velocity.
- [Effects](/docs/api/effects) — Fade, flash, shake and pulse entity properties with shared easing.
- [Audio](/docs/api/audio) — Play engine-decoded WAV/OGG voices, adjust parameters and schedule fades.
- [Events and signals](/docs/api/events) — Communicate through queued global events and object-scoped signals across scripting languages.
- [Clock](/docs/api/clock) — Scale or pause the shared scene clock for actions, animation, physics and audio.
- [Operation handles](/docs/api/operation-handles) — Inspect, pause, resume, cancel and observe engine-scheduled actions.

## Setup and attachment

1. Open a game project and scene. Create/select a **Part**.
2. Set its **Anchored** attribute to true so gravity does not compete with scripted
   movement. Action transforms are local to the object's parent.
3. Use **Programming > New Script > Object Component Script**, select Lua 5.4, and
   save it under `scripts/`. Attach it through **Programming > Attach Existing
   Script**, **+ Add Component** in the Inspector, or drag it onto the object.
4. Replace its source with the complete example below and press **Play**. The object
   moves to `(10, 5, 0)` in one second, holds for two seconds, and returns in one
   second. The Console prints `closed` when the sequence finishes. Stop Play restores
   the authored scene. No `Update` function is required.

```lua
return {
  Start = function()
    local door = rustic.entity_id()
    Sequence.new()
      .move(door, Vector3(10, 5, 0), 1, Ease.OutCubic)
      .wait(2)
      .move(door, Vector3(0, 0, 0), 1, Ease.InCubic)
      .play()
      .onFinished(function() print("closed") end)
  end,
}
```

Global and scene scripts use the same Programming menus for their scope. Timers
and subscriptions in those scopes last until Stop/reload; they are not removed
when an unrelated object disappears. Actions targeting an object are removed when
that object disappears. Use explicit stable entity IDs resolved through
`Game.scene.Find(path)` for controllers that target other objects.

For external languages, install the toolchain described in the corresponding
language guide. **Programming > Open Programming Workspace** regenerates SDK/editor
support after an engine upgrade; generated SDK files are not gameplay source.

## Language bindings

Every supported script type exposes `Tween`, `Ease`, `Movement`, `Animation`,
`Sequence` / `Timeline`, `Timer`, `Smooth` / `Interpolation`, `Path`, `Camera`,
`Physics`, `Effects`, `Audio`, and `Events`. `Clock` controls the shared scene clock.
Callbacks stay in their language runtime; action state and math stay in Rust.

| Language | Access and construction |
| --- | --- |
| Lua 5.4 / Luau | Globals; `Sequence.new()`; dot or colon handle calls |
| JavaScript / HTML inline JS | Globals; `Sequence.new()`; member calls |
| Python | Import modules from `rustic`; `Sequence.new()` |
| C++ | Namespaces; `Sequence::create()`; returned shared handle uses `->` (chained methods return a reference) |
| C# | `using static Rustic`; `Sequence.New()`; typed option/clip records |
| Java | Extend `Rustic`; `Sequence.create()`; `.waitFor(seconds)`; typed records |
| PHP | Static classes; `Sequence::create()`; returned handle uses `->` |
| C | Function tables such as `Tween.move`; typed structs; callbacks are function pointers |

C uses `Sequence.create()` and explicit `Sequence.destroy(&builder)` / `Path.destroy(&path)`
for builder storage. Playback owns its action data after scheduling. C query list strings
last until the next lifecycle callback; copy any string you retain. An omitted C callback
or easing is `NULL`; easing then defaults to Linear. Native bindings provide typed motion
options (`MotionOptions`, `RusticMotionOptions`) for duration or speed. C uses
`Tween.toOptions` / `Movement.moveToOptions` for those options.

For external languages, install the toolchain in its language guide and regenerate the
Programming Workspace after upgrading. Python imports the modules it uses, for example:
`from rustic import rustic, run, Tween, Animation, Physics, Audio, Clock`.

PHP and HTML entries remain restricted to `ui/`; CSS alone executes no actions.

## Easing, timing, and properties

`Ease.Linear` is the default. The other families are Sine, Quad, Cubic, Quart,
Quint, Expo, Circ, Back, Elastic, and Bounce, each with `In`, `Out`, and `InOut`
prefixes (`Ease.InOutSine`, `Ease.OutBounce`, etc.). Every time-based action uses
the same core curves. Back/Elastic can overshoot numeric, vector, and quaternion progress. Path
parameters, opacity, and color are bounded by their backend contracts.

```lua
return { Start = function()
  local owner = rustic.entity_id()
  Movement.moveTo(owner, Vector3(10, 0, 0), {
    speed = 5, easing = Ease.OutCubic
  })
  -- Alternatively: { duration = 2, easing = Ease.InOutSine }
end }
```

Dynamic-language movement/tween helpers accept either a duration
number or an options object/table with exactly one of `duration` or `speed`.
Speed is units/second (radians/second for quaternion rotations). For finite tweens,
speed determines the total duration; easing intentionally changes instantaneous
speed. Invalid/non-finite values and non-positive speeds are rejected. A zero
finite duration snaps on the next advancing frame.

Properties supported by the scene adapter are `Position`, `Scale` (`Size`),
`Rotation`, `Color`, `Opacity`, and `Fov`. Position/scale/color use three numbers;
rotation uses a nonzero quaternion `[x, y, z, w]`. FOV is in radians and requires a
perspective camera. Opacity and RGB are clamped to `[0, 1]`. Opacity updates material alpha and the renderer uses alpha blending.
Audio voices additionally expose `Volume` and `Pitch`. For script/component/UI
values, use `Tween.value` or `Animation.value` with an update callback. The property
adapter is extensible; arbitrary component reflection is not part of this API.

The core interpolates scalars, vectors, and rotations. Callback tweening applies
those values to your own gameplay or UI state:

```lua
return { Start = function()
  Tween.value(0, 100, 2, function(value)
    -- Apply value to your own gameplay state here.
    rustic.log("info", tostring(value))
  end, Ease.OutQuad)
end }
```

A tween captures its starting value when it first advances. Sequence children
capture their starts when reached, so earlier movement is respected. If several
actions write the same property, the action created last writes last each frame.
Avoid competing physics or ordinary `Update` setters on that property.

## Paths

`Path.create(points, kind)` supports `Linear`, `Bezier`, `CubicBezier` (exactly four
control points), `CatmullRom`, and `Spline` (uniform Catmull-Rom). Bezier supports up
to 32 control points. Other paths allow 2–4096 points. Linear/spline paths use each
starting point's easing for the following segment. Bezier uses the first point's
segment easing for its single curve span.

```lua
return { Start = function()
  local patrol = Path.create({
    { point = Vector3(0, 0, 0), easing = Ease.OutQuad },
    { point = Vector3(10, 0, 0), easing = Ease.InOutCubic },
    { point = Vector3(20, 5, 0), easing = Ease.InSine },
  })
  Path.follow(rustic.entity_id(), patrol, {
    duration = 8, easing = Ease.InOutSine,
    loop = true, pingPong = true, orientToPath = true,
  })
end }
```

Whole-path easing and per-segment easing alter traversal, not geometry/control
points. Speed-based paths use an approximate arc-length table; segment easing can
still accelerate/decelerate locally. Looping repeats the traversal; a closed
geometric loop requires matching endpoints. Ping-pong reverses at endpoints.
`orientToPath` faces local -Z along the tangent. Coordinates are local to the
object's parent. Choose Catmull-Rom or Spline for smooth corners; segment curves provide local
acceleration/deceleration without moving control points.

## Timers, sequences, and handles

```lua
return { Start = function()
  Timer.after(2, function() print("delayed") end)
  local repeating = Timer.every(0.5, function() print("tick") end, 4)
  repeating.pause()
  repeating.resume()
  -- repeating.cancel() prevents subsequent delivery.
end }
```

`Timer.every(interval, callback, count?)` requires a positive interval/count. Omit
count to repeat until cancelled. Large frame deltas preserve leftover time and may
queue several ticks. Infinite zero-duration action loops fail within a bounded
step budget. Callback execution itself is still subject to the language runtime's
budget/deadline; keep callbacks short.

Lua handles use dot or colon calls; JavaScript/Python use member calls.
`onFinished` registers a completion function, `state()` reports status/error (`Gameplay.state(handle)` in C), and `pause`, `resume`, `cancel`, `reverse` control
an operation. Reverse is supported on tween/value/move/lookAt/path/orbit/wait, not
sequence/parallel/follow/shake. Cancel does not call completion. Completed state
history is bounded to 1024 operations. The scene supports at most 8192 retained
operations (up to 65536 aggregate action nodes), 4096 subscriptions, and 4096 queued callbacks per queue.

`Sequence.new().call(fn)` queues a callback between steps; `.parallel({builder1,
builder2})` starts their sequences together and waits for the longest. `Timeline`
provides the same builder in every language. Do not replay a builder containing callbacks;
construct a new builder so it has new callback tokens. `.animation(object, clipName,
options?)` inserts clip playback as a sequence step.

## Camera, movement, and effects

Camera helpers accept an explicit camera entity. Lua/Luau/JavaScript also accept
`Camera.moveTo(position, duration, easing?)` and `Camera.zoom(fov, duration, easing?)`
for the current camera. `Camera.current()` resolves the active camera in those and
the C++/C#/Java/Python/PHP facades:

```lua
return { Start = function()
  local camera = Game.scene.Find("Game.scene.Camera")
  Camera.moveTo(camera, Vector3(0, 4, 10), 2, Ease.OutCubic)
  Camera.zoom(camera, math.rad(45), 2, Ease.InOutSine)
  Camera.lookAt(camera, Vector3(0, 0, 0), 0.2, Ease.OutQuad)
end }
```

`Movement.lookAt`/`Camera.lookAt` sample the target orientation when starting.
`follow(object, target, duration, easing?, offset?)` re-reads the target position
each frame during its finite catch-up duration.
`orbit(object, center, radius, turns, duration, easing?)` circles in the local XZ
plane. `Effects.fade` animates Opacity, `flash` animates Color and returns it, and
`shake` applies a deterministic decaying position offset and restores the start.
Camera shake uses the same action. `Effects.pulse(object, scale, duration, easing?)`
scales out and returns. `Camera.transition(camera, position, rotation, fov, duration,
easing?)` changes position, rotation and FOV in parallel.

## Events and object signals

```lua
return { Start = function()
  Events.once("DoorOpened", function(playerId)
    print("opened by", playerId)
  end)
  Events.emit("DoorOpened", { rustic.entity_id() })
end }
```

`Events.on(name, callback)` persists; `once` runs once. Both return a connection
with `disconnect()`. `Events.connect(object, name, callback)` listens to that
object's signal; emit it with `Events.emit(name, arguments, object)`. Global events
are distinct from object signals. Payloads are booleans, strings (including stable
entity IDs), numbers, vectors, and quaternions; arbitrary tables are not supported.
At most 64 arguments are allowed; each string is limited to 4096 bytes and the
combined payload is limited to 16 KiB. All supported language instances can communicate across language boundaries
inside the same play scene. Events are dispatched during `Update` before the
user's frame callback; emitting does not synchronously invoke another VM.

## Lifetime and diagnosis

The play scene advances actions once per frame using actual elapsed seconds,
independent of script count and omitted `Update` callbacks. Play pause freezes the
runtime; Stop discards its state. `Clock.timeScale(0.5)` slows actions, clip source clocks, physics and audio playback.
`Clock.pause()` / `resume()` pause the scene clock while leaving script callbacks
available to resume it. Per-operation pause freezes just that action. Audio voice
pause freezes its source cursor independently.

Owner destruction/disable/failure, scene disposal, and successful reload remove
owned operations, voices, callback references and subscriptions. Removed targets cancel referring actions and
queued completion callbacks. Failed reload retains the previous owner's actions.
Callbacks can enqueue new actions without blocking the action scheduler.

If nothing moves, check attachment/enabled state, press Play, anchor the Part, and
check the Console. For external languages, check toolchain discovery; Java needs
both `java` **and** `javac`, not just a JRE. An unsupported property, wrong value
type, or unavailable perspective camera is an error, not a successful action.
Asynchronous target errors appear in operation state and the Console. Schedule
through `Start` or an event/callback; API state is not initialized at script top
level. Callback exceptions disable the owning behavior and clean its actions.

## Imported and engine-created animation

Import a glTF, GLB or FBX model through the Explorer so it has an `.rmeta` sidecar.
Attach a script to its mesh entity. Before `Start`, the engine binds source nodes
as descendants named `node_N`, retains the skin palette/inverse bind matrices and
registers named clips. Script-created model instances receive the same binding.
`Animation.clips(object)` lists names; `Animation.load(object, "assets/character.glb")`
explicitly binds a model to a fresh target. Rebinding an animated target is rejected
so duplicate skeletons cannot accumulate.

```lua
return { Start = function()
  local actor = rustic.entity_id()
  for _, name in ipairs(Animation.clips(actor)) do print(name) end
  Animation.addMarker(actor, "Attack", 0.4, "Hit") -- add before playing
  Animation.play(actor, "Attack", {
    speed = 1.2,
    blendIn = 0.25, blendInEase = Ease.OutCubic,
    blendOut = 0.2, blendOutEase = Ease.InCubic,
  }).onMarker("Hit", function() print("apply damage") end)
    .onFinished(function() print("attack finished") end)
end }
```

`Animation.addMarker(object, clipName, time, name)` adds a source-clock event to
a registered/imported clip. Running actions retain their existing clip snapshot, so
add markers before playing.

Play returns the usual pausable/resumable/cancellable handle plus `stop`, `speed`,
`loop` and `onMarker`. Source time stays linear at the selected speed. Changing playback speed leaves
blend durations measured in scene-clock seconds; looping keeps the initial blend
clock and turning looping off finishes the current cycle. `easing` is
an alternative default for blend curves; only explicit `progressionEase` warps
clip sampling. Marker times use the source clock. `Animation.blend(actor, "Walk",
"Run", {duration=0.3, easing=Ease.InOutCubic})` samples both clips during the
transition; `transition` is an alias. Duration measures blend time, not clip length.

`weight`, `mask` (track-name prefixes), and `additive` control layering. Start a base
clip first, then a masked layer so the layer sees the base pose each frame. Additive
tracks apply their delta from the first clip pose to the current pose. Blend weights
use the shared easing curves. Completed clips leave the final pose unless blend-out
returns their weight to zero. Stop cancels playback without resetting the pose.

`Animation.register(object, clip)` adds engine-created tracks without editing an
asset or JSON. Track targets are `Position`, `Rotation`, `Scale`, `Opacity`, `Color`,
`Fov`, or `child/path/Property`; imported tracks use `node_N/Property`.

```lua
return { Start = function()
  local object = rustic.entity_id()
  Animation.register(object, {
    name="Reveal", duration=1,
    tracks={{target="Opacity", keys={
      {time=0, value=0, easing=Ease.OutSine},
      {time=1, value=1},
    }}}, markers={{time=0.5, name="Halfway"}},
  })
  Sequence.new().animation(object, "Reveal").wait(1).call(function()
    print("cutscene step finished")
  end).play()
end }
```

`Animation.value(keys, onUpdate)` runs numeric/vector/quaternion keyframes for
script or UI state; its callback receives an ordinary value. Keys start at zero
and have strictly increasing times. The starting key's easing controls its span.
The following example changes a script value from zero to one and back:

```lua
return { Start = function()
  Animation.value({
    {time=0, value=0, easing=Ease.OutQuad},
    {time=1, value=1, easing=Ease.InQuad},
    {time=2, value=0},
  }, function(value) print(value) end)
end }
```

The glTF importer preserves linear, step and cubic-spline interpolation. FBX takes
are baked at 60 Hz; skeletal vertices use four linear skin influences and retained
inverse binds. Rendering consumes live node transforms. Morph-target animation,
dual-quaternion skinning and animation graph/editor UI are outside this surface.

## Procedural animation and interpolation

`Animation.ik(root, middle, tip, target, options?)` solves a two-bone chain in world
space. `options` supplies `pole`, `weight`, `duration` and `easing`. The solver clamps
unreachable targets and preserves segment lengths; joints must form a direct
root/middle/tip parent chain. `footPlacement` uses the same solver.
`headTracking` and `lookAt` turn toward a target; update the target through your
controller when needed. `recoil` eases a rotation out and back. Native bindings
provide typed parameters rather than dynamic option tables.

`Smooth.lerp` and `slerp` accept shared easing. `inverseLerp` and `remap` do not clamp
values. `smoothDamp(current, target, velocity, smoothTime, delta)` returns updated
value and velocity; pass the velocity into the next call. It uses an analytic damped
spring rather than a frame-rate-dependent fixed fraction. `Interpolation` exposes
the same math. Smoothing is synchronous; actions and callbacks are asynchronous.

## Physics

```lua
return { Start = function()
  local hit = Physics.sphereCast(Vector3(0, 4, 0), Vector3(0, -1, 0), 10, 0.25)
  if hit then print(hit.entity, hit.distance) end
  Physics.explosion(Vector3(0, 0, 0), 5, 10)
end }
```

`raycast(origin, direction, distance, ignore?)` returns the nearest hit or nil/null.
`sphereCast` adds a radius and uses rounded sphere/box contacts; `overlap(center,
radius, ignore?)` returns entity IDs. Hits contain entity, point, normal and distance.
Direction is normalized by the engine. Queries use the simulation's authored
primitive colliders represented by world AABBs, including parent transforms.

`force(object, vector, delta?)` applies force for the interval, `impulse` / `knockback`
add velocity, and `launch` sets velocity. The current simulation uses unit mass.
`explosion(center, radius, strength, ignore?)` applies radial distance-falloff impulses
and skips anchored bodies. Direct force/impulse/launch on an anchored or nonphysical
object is an error. Models do not automatically acquire primitive colliders.

## Audio

```lua
return { Start = function()
  local music = Audio.play("assets/music.ogg", {loop=true, volume=0})
  Audio.fadeIn(music, 2, Ease.InSine)
  Timer.after(5, function() Audio.fadeOut(music, 1, Ease.OutSine) end)
end }
```

`play` decodes WAV/OGG into a retained PCM voice; `playAt(source, position, options?)`
adds distance attenuation relative to the active camera. A voice has an ID and
`pause`, `resume`, `stop` controls. `volume(voice, value)` and `pitch(voice, value)`
change playback parameters; fades and crossfades are scheduler actions with easing.
`crossfade(oldVoice, newVoice, duration, easing?)` fades old volume to zero and new
volume from zero to one in parallel. Stopping a voice cancels actions targeting it
on the next scheduler tick. Finished non-looping voices release their PCM storage.

The PCM mixer runs on every platform. Windows player output feeds the default
audio device through a bounded queue; other platforms currently provide headless
mixing. Library tests verify PCM consumption and voice controls without a device.

## Validation and extension

Library tests cover easing endpoints/overshoot, timing/sequence leftovers, path
geometry and traversal, pause/reverse/repeat, ownership/reload/signal cleanup,
physics contacts, PCM mixing, clip sampling/blends/markers, IK, glTF/FBX import,
and live skeletal vertex deformation. The integration fixture executes shared
math, tween/timer callbacks and animation playback/markers in all ten script types.
`RUSTIC_REQUIRE_ALL_SDKS=1` makes a missing toolchain fail that fixture.

The serializable action, clip and path types can be consumed by future animation,
timeline, path and easing editors. Add backend properties through `GameplayWorld`
without introducing new interpolation logic into language bindings.
