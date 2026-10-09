# Operation handles API

Inspect, pause, resume, cancel and observe engine-scheduled actions.

## Setup and language support

These engine-owned services work in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript. Follow [Use gameplay actions](/docs/guides/gameplay-actions) to create and attach a script, install external toolchains, and choose the correct language binding. PHP and HTML scripts belong under `ui/`; CSS alone runs no gameplay code.

The signatures and complete example below use JavaScript. Attach a JavaScript Object Component script to the specified target and press **Play**. Globals are supplied by the engine. Schedule actions from `Start` or a callback, rather than at script top level. Native languages use typed options and their own builder conventions; see the guide's language bindings.

## Calls

- `handle.state()`
- `handle.pause()`
- `handle.resume()`
- `handle.cancel()`
- `handle.reverse()`
- `handle.onFinished(callback)`

## Parameters

Use the handle returned from Tween, Movement, Animation, Path, Timer, Sequence, Camera or Effects, or an audio fade. OnFinished takes a no-argument completion callback. C uses Gameplay.state/pause/resume/cancel/reverse with an explicit handle.

## Return and timing

JavaScript controls return the handle for chaining; state returns its retained status/error record, or undefined if absent from bounded history. Completion is queued during Update. Cancel and target destruction do not invoke onFinished.

## Example

```javascript
globalThis.behavior = {
  Start() {
    const motion = Tween.move(rustic.entity_id(), [10, 0, 0], 2);
    motion.pause();
    print(motion.state());
    Timer.after(1, () => motion.resume());
    motion.onFinished(() => print("finished"));
  }
};
```

## Behavior and limitations

- An anchored Part waits one scene second, resumes its two-second movement and prints finished when it reaches the destination.
- Reverse supports tween/value/move/lookAt/path/orbit/wait actions. It is unsupported for sequence/parallel/follow/shake/animation handles.
- At most 8192 operations and 65536 aggregate action nodes are retained; finished history is bounded to 1024 operations. Do not treat state as permanent storage.
- Removing a target cancels referencing actions and queued completion callbacks. Disabling/destroying an owner, successful reload and scene disposal release its resources; a failed reload preserves the previous operations.
- Animation playback adds stop (cancel), speed, loop and onMarker controls. Audio voices have their own pause/resume/stop controls and are distinct from fade handles.
- Check script attachment, enabled state and the Console when an operation fails. Asynchronous errors also appear in operation state. Callback exceptions disable the owner and clean up its actions.
- Owner destruction, disable, successful reload or Stop cleans up owned actions and subscriptions. Failed reload retains the previous behavior's operations. See [Operation handles](/docs/api/operation-handles) for controls and lifetime details.
