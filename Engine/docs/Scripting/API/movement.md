# Movement API

Move, turn, follow and orbit objects with finite engine-scheduled actions.

## Setup and language support

These engine-owned services work in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript. Follow [Use gameplay actions](/docs/guides/gameplay-actions) to create and attach a script, install external toolchains, and choose the correct language binding. PHP and HTML scripts belong under `ui/`; CSS alone runs no gameplay code.

The signatures and complete example below use JavaScript. Attach a JavaScript Object Component script to the specified target and press **Play**. Globals are supplied by the engine. Schedule actions from `Start` or a callback, rather than at script top level. Native languages use typed options and their own builder conventions; see the guide's language bindings.

## Calls

- `Movement.move(entity, offset, timing, easing?)`
- `Movement.moveTo(entity, position, timing, easing?)`
- `Movement.rotateTo(entity, quaternion, timing, easing?)`
- `Movement.lookAt(entity, position, duration = 0, easing?)`
- `Movement.follow(entity, target, duration, easing?, offset?)`
- `Movement.orbit(entity, center, radius, turns, duration, easing?)`

## Parameters

Entities are stable IDs or scene paths. Position, offset and center are three-component local vectors. Rotation is a nonzero quaternion [x, y, z, w]. Move/moveTo/rotateTo accept seconds or duration/speed options; follow and orbit take finite durations. Follow offset defaults to [0, 0, 0].

## Return and timing

Each helper returns an operation handle. Move adds an offset to the captured start; moveTo selects an absolute local destination.

## Example

```javascript
globalThis.behavior = {
  Start() {
    Movement.move(rustic.entity_id(), [5, 0, 0], 1, Ease.OutQuad)
      .onFinished(() => Movement.lookAt(rustic.entity_id(), [0, 0, 0], 0.2));
  }
};
```

## Behavior and limitations

- An anchored Part moves five units along local X, then turns toward the origin.
- Follow re-reads the target each frame during its catch-up duration; it does not install a permanent follower.
- LookAt samples orientation when starting. Reissue from a controller when the target changes.
- Orbit circles in local XZ; turns controls revolution count. Physics and simultaneous property writes can compete with motion.
- Check script attachment, enabled state and the Console when an operation fails. Asynchronous errors also appear in operation state. Callback exceptions disable the owner and clean up its actions.
- Owner destruction, disable, successful reload or Stop cleans up owned actions and subscriptions. Failed reload retains the previous behavior's operations. See [Operation handles](/docs/api/operation-handles) for controls and lifetime details.
