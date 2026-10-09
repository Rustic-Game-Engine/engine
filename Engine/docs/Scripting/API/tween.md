# Tween API

Animate an entity property or script value without writing an Update loop.

## Setup and language support

These engine-owned services work in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript. Follow [Use gameplay actions](/docs/guides/gameplay-actions) to create and attach a script, install external toolchains, and choose the correct language binding. PHP and HTML scripts belong under `ui/`; CSS alone runs no gameplay code.

The signatures and complete example below use JavaScript. Attach a JavaScript Object Component script to the specified target and press **Play**. Globals are supplied by the engine. Schedule actions from `Start` or a callback, rather than at script top level. Native languages use typed options and their own builder conventions; see the guide's language bindings.

## Calls

- `Tween.to(entity, property, to, timing, easing?)`
- `Tween.move(entity, position, timing, easing?)`
- `Tween.rotate(entity, quaternion, timing, easing?)`
- `Tween.scale(entity, scale, timing, easing?)`
- `Tween.value(from, to, timing, onUpdate, easing?)`

## Parameters

| Parameter | Meaning |
| --- | --- |
| entity | Stable entity ID, resolvable scene path or object proxy; null means the script owner in JavaScript. |
| property | Position, Scale/Size, Rotation, Color, Opacity, perspective Fov, or audio voice Volume/Pitch. |
| to / from | Finite scalar, three-component vector or nonzero four-component quaternion matching the property. |
| timing | Finite non-negative seconds, or an object with exactly one of duration or positive speed. |
| easing | An Ease constant; Linear by default. Options can supply easing. |
| onUpdate | Receives each interpolated value for script/UI state. |

## Return and timing

Entity and value tweens return an [operation handle](/docs/api/operation-handles). The engine advances it each scene frame and queues completion during Update. No user Update callback is required.

## Example

```javascript
globalThis.behavior = {
  Start() {
    Tween.move(rustic.entity_id(), [10, 0, 0], 2, Ease.OutCubic)
      .onFinished(() => print("arrived"));
  }
};
```

## Behavior and limitations

- Attach this example to an anchored Part: it reaches local position (10, 0, 0) in two seconds and prints arrived.
- The start value is captured on first advancement. Zero duration applies the final value on the next advancing frame.
- Speed is units/second, or radians/second for rotation. Easing changes instantaneous velocity; speed-based duration uses the total distance.
- Transforms are local to the parent. Opacity and Color are bounded; Fov is in radians between zero and pi on perspective cameras. Arbitrary component reflection is not supported; use value callbacks for your state.
- Avoid physics or Update writes to the same property. If multiple actions write it, the last-created action writes last.
- Check script attachment, enabled state and the Console when an operation fails. Asynchronous errors also appear in operation state. Callback exceptions disable the owner and clean up its actions.
- Owner destruction, disable, successful reload or Stop cleans up owned actions and subscriptions. Failed reload retains the previous behavior's operations. See [Operation handles](/docs/api/operation-handles) for controls and lifetime details.
