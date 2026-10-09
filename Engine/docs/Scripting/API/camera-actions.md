# Camera actions API

Move, aim, zoom, follow and shake a game camera using shared actions.

## Setup and language support

These engine-owned services work in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript. Follow [Use gameplay actions](/docs/guides/gameplay-actions) to create and attach a script, install external toolchains, and choose the correct language binding. PHP and HTML scripts belong under `ui/`; CSS alone runs no gameplay code.

The signatures and complete example below use JavaScript. Attach a JavaScript Object Component script to the specified target and press **Play**. Globals are supplied by the engine. Schedule actions from `Start` or a callback, rather than at script top level. Native languages use typed options and their own builder conventions; see the guide's language bindings.

## Calls

- `Camera.current()`
- `Camera.moveTo(camera, position, duration, easing?)`
- `Camera.zoom(camera, fov, duration, easing?) / Camera.fov(...)`
- `Camera.lookAt(camera, position, duration, easing?)`
- `Camera.follow(camera, target, duration, easing?, offset?)`
- `Camera.orbit(camera, center, radius, turns, duration, easing?)`
- `Camera.shake(camera, strength, duration, easing?)`
- `Camera.transition(camera, position, quaternion, fov, duration, easing?)`

## Parameters

Camera is a stable camera ID or scene path. Position/center/offset are local vectors; rotation is [x, y, z, w]. Fov is finite radians between zero and pi on a perspective camera. Duration is finite non-negative seconds; easing defaults to Linear.

## Return and timing

Current returns the active camera ID where supported; actions return operation handles. Transition animates position, rotation and FOV in parallel. Select a viewpoint with [Game.setCurrentCamera](/docs/api/camera).

## Example

```javascript
globalThis.behavior = {
  Start() {
    const camera = Camera.current();
    Camera.transition(camera, [0, 4, 10], [0, 0, 0, 1],
      Math.PI / 4, 2, Ease.OutCubic)
      .onFinished(() => print("camera ready"));
  }
};
```

## Behavior and limitations

- Create/select an active perspective camera before Play. It reaches the given position, identity rotation and 45-degree FOV in two seconds.
- Lua/Luau/JavaScript additionally allow moveTo(position, duration, easing) and zoom(fov, duration, easing) for the current camera. Explicit targets work across all bindings.
- C has no Camera.current helper: resolve an explicit camera ID. Other native bindings expose current.
- LookAt samples when starting; follow re-reads its target only during the finite duration. Orbit uses local XZ. Shake restores its initial position.
- Check script attachment, enabled state and the Console when an operation fails. Asynchronous errors also appear in operation state. Callback exceptions disable the owner and clean up its actions.
- Owner destruction, disable, successful reload or Stop cleans up owned actions and subscriptions. Failed reload retains the previous behavior's operations. See [Operation handles](/docs/api/operation-handles) for controls and lifetime details.
