# Clock API

Scale or pause the shared scene clock for actions, animation, physics and audio.

## Setup and language support

These engine-owned services work in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript. Follow [Use gameplay actions](/docs/guides/gameplay-actions) to create and attach a script, install external toolchains, and choose the correct language binding. PHP and HTML scripts belong under `ui/`; CSS alone runs no gameplay code.

The signatures and complete example below use JavaScript. Attach a JavaScript Object Component script to the specified target and press **Play**. Globals are supplied by the engine. Schedule actions from `Start` or a callback, rather than at script top level. Native languages use typed options and their own builder conventions; see the guide's language bindings.

## Calls

- `Clock.timeScale(scale)`
- `Clock.pause()`
- `Clock.resume()`
- `Clock.state()`

## Parameters

Scale is finite non-negative; 1 is normal rate and 0.5 is half speed. JavaScript state returns the current scale and paused flag. Native getter/state availability varies; C/C++/Java timeScale requires a value.

## Return and timing

TimeScale returns the resulting scale in JavaScript; state returns a synchronous clock snapshot. Pause/resume change global clock state rather than creating an operation handle.

## Example

```javascript
globalThis.behavior = {
  Start() {
    Clock.timeScale(0.5);
    Tween.move(rustic.entity_id(), [10, 0, 0], 2)
      .onFinished(() => { Clock.timeScale(1); print("normal speed"); });
  }
};
```

## Behavior and limitations

- An anchored Part takes about four real seconds to finish the two-scene-second tween, then restores normal speed.
- Scaling applies to action/timer time, clip source clocks, physics and audio playback. Clip blend durations are scene-clock seconds.
- Clock.pause leaves script lifecycle callbacks available so a controller can call resume. A Timer alone cannot resume a paused clock because its own time is frozen.
- Play pause freezes the runtime; Stop discards it. Per-operation pause and voice.pause affect only their respective operation/voice.
- Check script attachment, enabled state and the Console when an operation fails. Asynchronous errors also appear in operation state. Callback exceptions disable the owner and clean up its actions.
- Owner destruction, disable, successful reload or Stop cleans up owned actions and subscriptions. Failed reload retains the previous behavior's operations. See [Operation handles](/docs/api/operation-handles) for controls and lifetime details.
