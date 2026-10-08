# Timer API

Schedule one-shot or repeating scene-clock callbacks.

## Setup and language support

These engine-owned services work in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript. Follow [Use gameplay actions](/docs/guides/gameplay-actions) to create and attach a script, install external toolchains, and choose the correct language binding. PHP and HTML scripts belong under `ui/`; CSS alone runs no gameplay code.

The signatures and complete example below use JavaScript. Attach a JavaScript Object Component script to the specified target and press **Play**. Globals are supplied by the engine. Schedule actions from `Start` or a callback, rather than at script top level. Native languages use typed options and their own builder conventions; see the guide's language bindings.

## Calls

- `Timer.after(delay, callback)`
- `Timer.every(interval, callback, count?)`

## Parameters

Delay is finite non-negative seconds. Interval is positive finite seconds. Optional count is a positive integer; omit it for indefinite repetition. In C, count 0 means indefinite repetition. Callbacks take no timer argument.

## Return and timing

Both return pausable, resumable, cancellable operation handles. Deliveries are queued during Update; they do not block the scripting runtime.

## Example

```javascript
globalThis.behavior = {
  Start() {
    Timer.after(2, () => print("delayed"));
    Timer.every(0.5, () => print("tick"), 4)
      .onFinished(() => print("timer finished"));
  }
};
```

## Behavior and limitations

- The Console prints four ticks at half-second intervals, delayed at two seconds, and timer finished on repeat completion.
- Large frames preserve leftover time and can deliver multiple ticks. Keep callbacks short and within the runtime budget.
- Clock scaling and pause affect timers. Cancel prevents future delivery and does not invoke onFinished.
- Zero-duration infinite loops are rejected or fail within a bounded scheduler step budget.
- Check script attachment, enabled state and the Console when an operation fails. Asynchronous errors also appear in operation state. Callback exceptions disable the owner and clean up its actions.
- Owner destruction, disable, successful reload or Stop cleans up owned actions and subscriptions. Failed reload retains the previous behavior's operations. See [Operation handles](/docs/api/operation-handles) for controls and lifetime details.
