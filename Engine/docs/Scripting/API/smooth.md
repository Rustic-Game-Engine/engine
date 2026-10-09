# Smooth and Interpolation API

Evaluate interpolation and damping immediately without scheduling an action.

## Setup and language support

These engine-owned services work in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript. Follow [Use gameplay actions](/docs/guides/gameplay-actions) to create and attach a script, install external toolchains, and choose the correct language binding. PHP and HTML scripts belong under `ui/`; CSS alone runs no gameplay code.

The signatures and complete example below use JavaScript. Attach a JavaScript Object Component script to the specified target and press **Play**. Globals are supplied by the engine. Schedule actions from `Start` or a callback, rather than at script top level. Native languages use typed options and their own builder conventions; see the guide's language bindings.

## Calls

- `Smooth.lerp(from, to, progress, easing?)`
- `Smooth.slerp(fromQuaternion, toQuaternion, progress, easing?)`
- `Smooth.inverseLerp(from, to, value)`
- `Smooth.remap(value, inMin, inMax, outMin, outMax)`
- `Smooth.smoothDamp(current, target, velocity, smoothTime, delta)`

## Parameters

Lerp accepts scalars or three-component vectors. Slerp accepts nonzero [x, y, z, w] quaternions. Progress and inputs must be finite. Easing defaults to Linear. SmoothDamp takes scalar state, a smoothing time and elapsed seconds; feed the returned velocity into the next call.

## Return and timing

Math results are synchronous. JavaScript smoothDamp returns {value, velocity}; native return shapes differ (for example C# tuple and Java double[2]). Interpolation aliases Smooth.

## Example

```javascript
globalThis.behavior = {
  Start() {
    print(Smooth.lerp(0, 10, 0.5)); // 5
    print(Smooth.remap(25, 0, 100, 0, 1)); // 0.25
    const next = Smooth.smoothDamp(0, 10, 0, 0.5, 1 / 60);
    print(next.value, next.velocity);
  }
};
```

## Behavior and limitations

- This example prints interpolated values immediately in Start; it creates no operation handle.
- InverseLerp and remap do not clamp out-of-range values. A degenerate input interval returns inverse progress zero (and remap returns outMin). Damping uses an analytic spring rather than a fixed per-frame fraction.
- Use Tween.value or Animation.value when you want the engine to advance interpolation and dispatch callbacks over time.
- Check script attachment, enabled state and the Console when an operation fails. Asynchronous errors also appear in operation state. Callback exceptions disable the owner and clean up its actions.
- Owner destruction, disable, successful reload or Stop cleans up owned actions and subscriptions. Failed reload retains the previous behavior's operations. See [Operation handles](/docs/api/operation-handles) for controls and lifetime details.
