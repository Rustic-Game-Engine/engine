# Sequence and Timeline API

Compose sequential and parallel actions without nesting timer callbacks.

## Setup and language support

These engine-owned services work in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript. Follow [Use gameplay actions](/docs/guides/gameplay-actions) to create and attach a script, install external toolchains, and choose the correct language binding. PHP and HTML scripts belong under `ui/`; CSS alone runs no gameplay code.

The signatures and complete example below use JavaScript. Attach a JavaScript Object Component script to the specified target and press **Play**. Globals are supplied by the engine. Schedule actions from `Start` or a callback, rather than at script top level. Native languages use typed options and their own builder conventions; see the guide's language bindings.

## Calls

- `Sequence.new() / Timeline.new()`
- `builder.move(entity, position, timing, easing?)`
- `builder.to(entity, property, value, timing, easing?)`
- `builder.animation(entity, clipName, options?)`
- `builder.wait(duration)`
- `builder.call(callback)`
- `builder.parallel(builders)`
- `builder.play()`

## Parameters

Wait takes finite non-negative seconds. Parallel takes an array of sequence builders. Motion and clip parameters match Tween and Animation. Call takes a short callback. Native construction is Sequence.create() in C/C++/Java/PHP and Sequence.New() in C#; Java waits with waitFor.

## Return and timing

Builder methods return the builder; play schedules its action tree and returns a single operation handle. Parallel steps wait for their longest branch. Leftover frame time continues into subsequent steps.

## Example

```javascript
globalThis.behavior = {
  Start() {
    const door = rustic.entity_id();
    Sequence.new()
      .move(door, [10, 0, 0], 1, Ease.OutCubic)
      .wait(2)
      .move(door, [0, 0, 0], 1, Ease.InCubic)
      .call(() => print("closed"))
      .play();
  }
};
```

## Behavior and limitations

- An anchored Part opens in one second, holds for two, closes in one second and prints closed.
- Child tweens capture starting values when reached, after previous actions have run. Timeline is an alias of Sequence.
- Build a fresh sequence for each playback when callbacks are present; replaying callback tokens is unsupported.
- Empty sequences are rejected. Trees are limited to 4096 nodes and depth 32. Reverse is unsupported on sequence/parallel handles.
- C builder storage requires Sequence.destroy after scheduling; playback owns a copy of the action data.
- Check script attachment, enabled state and the Console when an operation fails. Asynchronous errors also appear in operation state. Callback exceptions disable the owner and clean up its actions.
- Owner destruction, disable, successful reload or Stop cleans up owned actions and subscriptions. Failed reload retains the previous behavior's operations. See [Operation handles](/docs/api/operation-handles) for controls and lifetime details.
