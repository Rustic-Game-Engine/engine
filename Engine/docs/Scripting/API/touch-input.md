# Touchscreen input and object clicks

Use `Input` to read real touchscreen contacts and handle mouse clicks or touch taps
on scene primitives in the editor's embedded **Play** game view. Lua 5.4, Luau,
JavaScript, Python, C, C++, C#, Java, PHP and HTML inline JavaScript use the same
engine-owned input state. New Window and Standalone do not forward these inputs.

## Set up a clickable object

1. Create a built-in primitive such as a cube. Select it and enable **CanTouch** in
   the Inspector. **CanCollide** can be disabled; clicking does not require physics.
   Enable **Anchored** if the object should stay in place.
2. Add an active Camera that faces the object. Cameras look along local **+Z**;
   a camera at `[0, 0, 0]` can see a cube at `[0, 0, 5]`. With several active
   cameras, the highest priority/order wins, matching the rendered game image.
3. Create a Lua Object Component script in Explorer and attach it to the cube
   using **+ Add Component**, or drag it onto the selected object in the viewport.
4. Paste this complete script, press **Play**, then click the cube or tap it on a
   touchscreen. The Console should print `clicked` once per click or tap.

```lua
return {
  Start = function()
    Input.onClick(rustic.entity_id(), function(id, position, device, pressure)
      print('clicked', id, device, position[1], position[2])
    end)
    Input.onTouchStart(function(id, position, device, pressure)
      print('finger down', id, pressure)
    end)
    Input.onTouchCancel(function(id)
      print('finger cancelled', id)
    end)
  end,
  Update = function(dt)
    for _, touch in ipairs(Input.touches()) do
      -- Normalized coordinates in the displayed game image, top-left is (0, 0).
      -- Each contact has its own stable string ID.
      local x, y = touch.position[1], touch.position[2]
    end
  end
}
```

The equivalent JavaScript click subscription belongs inside `Start`:

```javascript
globalThis.behavior = {
  Start() {
    Input.onClick(rustic.entity_id(), (id, position, device, pressure) => {
      print('clicked', id, device);
    });
  },
  Update(dt) {
    for (const touch of Input.touches()) {
      // touch.position is [x, y], touch.id is a string.
    }
  }
};
```

## Calls and returned data

- `Input.touches()` returns currently active touchscreen contacts, including
  separate contacts for multiple fingers. Mouse input is excluded.
- `Input.touchEvents()` returns ordered touchscreen events for the current frame.
  Read this in `Update`; reads do not consume events, so every script sees them.
- `Input.clicks(object?)` returns clicks for the current frame, optionally filtered
  by entity ID or object path. Each click has `{entity, pointer, point}`. `point`
  is the world-space hit point and `pointer` is the release event.
- `Input.onClick(object, callback)` subscribes to a specific object's `Click` event.
- `Input.onTouchStart(callback)`, `onTouchMove`, `onTouchEnd`, and `onTouchCancel`
  subscribe to touchscreen events, including fingers that hit no object.

Each touch record contains `{id, device, phase, position, pressure, aspect}`.
`id` is a stable string containing the platform device and finger IDs; never parse
it as a JavaScript number. `device` is `touch` (or `mouse` in click records).
`phase` is `start`, `move`, `end`, or `cancel`. `position` is `[x, y]` relative to
the displayed image, not the entire editor or the letterbox bars. Coordinates can
leave `[0, 1]` while a captured finger moves outside the image. `pressure` is
normalized to `[0, 1]`, with `0` when touch pressure is unavailable. `aspect` is the
displayed image's width divided by height.

Callbacks receive `(id, position, device, pressure)`. The callback's `position` is
`[x, y, 0]`, using the existing cross-language event vector representation. All
subscriptions return the normal Events connection; call `connection.disconnect()`
(or the corresponding native method) to remove one. Owner disable, destruction,
Stop and successful script reload clean up subscriptions. Failed reload preserves
the previous script's subscriptions. The event names `Click`, `TouchStart`,
`TouchMove`, `TouchEnd`, and `TouchCancel` are engine-reserved and cannot be emitted
from scripts using `Events.emit`.

## Click rules and limits

- Only primary mouse presses and actual touchscreen contacts generate clicks.
  A touchscreen's synthesized mouse event is suppressed to avoid duplicate clicks.
- Both press and release must pick the same primitive with **CanTouch enabled**.
  The closest primitive blocks those behind it, including when its CanTouch is
  disabled. CanTouch is checked again before delivering queued click callbacks.
- Picking uses primitive world AABBs, including parent transforms, and the active
  camera's perspective/orthographic projection and zoom. It does not perform
  triangle-level mesh picking. Imported model meshes need an authored primitive
  target; meshes alone are not clickable. The near and far camera planes bound picks.
- Input begins only inside the game image. Release outside it, cancellation,
  focus loss, hiding the game view, pause, Stop and changing scenes clear captures
  without clicking. Scene changes discard pending input from the previous scene.
- Touch input requires a touchscreen and OS/backend support. A mouse does not
  emulate a finger. No pinch, swipe or long-press gesture recognizer is included.
- Input is bounded to 64 active pointers and 1024 queued events. Queue overflow
  cancels active contacts rather than leaving them held.
- Collision contact callbacks remain unavailable. `CanTouch` controls picking;
  it does not enable solid collision response or `OnCollisionEnter` callbacks.

If nothing happens, verify **Play** is running, the game view is visible, the
camera can see the primitive, CanTouch is enabled, and the script is attached and
enabled. Check the Console for script/toolchain errors. Clicking a letterbox bar
or another editor panel does not target a game object.

## Other language bindings

Python imports `Input` from `rustic`. C# uses `Rustic.Input` (or `using static
Rustic;`), Java behaviors inherit `Rustic.Input`, C++ uses `Input::touches()` and
`Input::onClick(...)`, and PHP uses `Input::touches()` and `Input::onClick(...)`.
Lua/Luau and JavaScript/HTML receive `Input` as a global.

C uses `Input.touches()` / `Input.touchEvents()` returning `RusticPointerEvents`
(`count`, `items`) and `Input.clicks(entity_or_NULL)` returning
`RusticObjectClicks`. Each click contains `entity`, `pointer`, and `point`;
positions in a C pointer record are `position[0]` and `position[1]`. C subscription
callbacks use `void callback(const RusticValue *arguments, size_t count)` with
four arguments in the order above. Returned C reads live until the next callback.
C++ returns ordinary `RusticValue` arrays, C# returns `JsonElement` arrays, and
Java returns lists of maps. Other languages receive their ordinary arrays/lists
and objects/maps. Use the same signatures and the regular Events connection
conventions for your language. Regenerate the script workspace after upgrading
the engine so external scripts compile against the updated SDK.
