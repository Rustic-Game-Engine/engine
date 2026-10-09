# Path API

Traverse linear, Bezier and spline paths with global and per-segment easing.

## Setup and language support

These engine-owned services work in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript. Follow [Use gameplay actions](/docs/guides/gameplay-actions) to create and attach a script, install external toolchains, and choose the correct language binding. PHP and HTML scripts belong under `ui/`; CSS alone runs no gameplay code.

The signatures and complete example below use JavaScript. Attach a JavaScript Object Component script to the specified target and press **Play**. Globals are supplied by the engine. Schedule actions from `Start` or a callback, rather than at script top level. Native languages use typed options and their own builder conventions; see the guide's language bindings.

## Calls

- `Path.create(points, kind = "Linear")`
- `Path.follow(entity, path, options)`

## Parameters

Points are vectors or {point, easing} entries. Kind is Linear, Bezier, CubicBezier, CatmullRom or Spline. Follow options require exactly one of duration or speed, with optional easing, loop, pingPong and orientToPath.

## Return and timing

Create returns reusable path data. Follow returns an operation handle advanced by the shared scheduler.

## Example

```javascript
globalThis.behavior = {
  Start() {
    const path = Path.create([
      {point: [0, 0, 0], easing: Ease.OutQuad},
      {point: [10, 0, 0], easing: Ease.InOutCubic},
      {point: [20, 5, 0], easing: Ease.Linear}
    ], "CatmullRom");
    Path.follow(rustic.entity_id(), path, {
      duration: 8, easing: Ease.InOutSine,
      loop: true, pingPong: true, orientToPath: true
    });
  }
};
```

## Behavior and limitations

- An anchored Part traverses the local path in eight seconds, reverses at its end and repeats, facing its direction of travel.
- Bezier supports 2–32 control points; CubicBezier requires four. Other paths support 2–4096 points. Spline uses uniform Catmull-Rom.
- Global and segment easing change traversal, not curve geometry. Starting-point easing applies to the following segment; Bezier uses its first point for the single span.
- Speed uses approximate arc length. Looping repeats a span; a geometrically closed loop needs matching endpoints. OrientToPath faces local -Z along the tangent.
- C path storage requires Path.destroy after use; the scheduled action owns its path data.
- Check script attachment, enabled state and the Console when an operation fails. Asynchronous errors also appear in operation state. Callback exceptions disable the owner and clean up its actions.
- Owner destruction, disable, successful reload or Stop cleans up owned actions and subscriptions. Failed reload retains the previous behavior's operations. See [Operation handles](/docs/api/operation-handles) for controls and lifetime details.
