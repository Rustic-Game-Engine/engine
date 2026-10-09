# Effects API

Fade, flash, shake and pulse entity properties with shared easing.

## Setup and language support

These engine-owned services work in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript. Follow [Use gameplay actions](/docs/guides/gameplay-actions) to create and attach a script, install external toolchains, and choose the correct language binding. PHP and HTML scripts belong under `ui/`; CSS alone runs no gameplay code.

The signatures and complete example below use JavaScript. Attach a JavaScript Object Component script to the specified target and press **Play**. Globals are supplied by the engine. Schedule actions from `Start` or a callback, rather than at script top level. Native languages use typed options and their own builder conventions; see the guide's language bindings.

## Calls

- `Effects.fade(entity, opacity, duration, easing?)`
- `Effects.flash(entity, color, duration, easing?)`
- `Effects.shake(entity, strength, duration, easing?)`
- `Effects.pulse(entity, scale, duration, easing?)`

## Parameters

Entity is a stable ID or scene path. Opacity is scalar, Color is RGB, Scale is a three-component vector, and shake strength controls the position offset. Duration is finite non-negative seconds; easing defaults to Linear.

## Return and timing

Each returns an operation handle. Flash and pulse ping-pong through the outgoing and return legs; their duration applies to each leg. Fade changes Opacity once; shake restores its initial position.

## Example

```javascript
globalThis.behavior = {
  Start() {
    Effects.flash(rustic.entity_id(), [1, 0.2, 0.2], 0.25, Ease.OutQuad)
      .onFinished(() => print("flash finished"));
  }
};
```

## Behavior and limitations

- A Part flashes red and returns to its original color after two quarter-second legs.
- Opacity and Color obey bounded property contracts. These actions operate on existing properties rather than creating particles or arbitrary UI components.
- Shake uses deterministic decaying offsets. Avoid overlapping actions or physics that write the same property. Camera.shake uses the same implementation.
- Check script attachment, enabled state and the Console when an operation fails. Asynchronous errors also appear in operation state. Callback exceptions disable the owner and clean up its actions.
- Owner destruction, disable, successful reload or Stop cleans up owned actions and subscriptions. Failed reload retains the previous behavior's operations. See [Operation handles](/docs/api/operation-handles) for controls and lifetime details.
