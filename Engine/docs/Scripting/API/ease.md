# Ease API

Choose one of 31 shared easing curves for actions and synchronous interpolation.

## Setup and language support

These engine-owned services work in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript. Follow [Use gameplay actions](/docs/guides/gameplay-actions) to create and attach a script, install external toolchains, and choose the correct language binding. PHP and HTML scripts belong under `ui/`; CSS alone runs no gameplay code.

The signatures and complete example below use JavaScript. Attach a JavaScript Object Component script to the specified target and press **Play**. Globals are supplied by the engine. Schedule actions from `Start` or a callback, rather than at script top level. Native languages use typed options and their own builder conventions; see the guide's language bindings.

## Calls

- `Ease.Linear`
- `Ease.InSine / OutSine / InOutSine`
- `Ease.InQuad / OutQuad / InOutQuad`
- `Ease.InCubic / OutCubic / InOutCubic`
- `Ease.InQuart / OutQuart / InOutQuart`
- `Ease.InQuint / OutQuint / InOutQuint`
- `Ease.InExpo / OutExpo / InOutExpo`
- `Ease.InCirc / OutCirc / InOutCirc`
- `Ease.InBack / OutBack / InOutBack`
- `Ease.InElastic / OutElastic / InOutElastic`
- `Ease.InBounce / OutBounce / InOutBounce`

## Parameters

Pass a constant to an action or Smooth function. In accelerates, Out decelerates, and InOut combines both. Linear is the default.

## Return and timing

Ease members are identifiers interpreted by the shared Rust curve sampler, rather than language-specific easing functions.

## Example

```javascript
globalThis.behavior = {
  Start() {
    Tween.move(rustic.entity_id(), [8, 0, 0], 2, Ease.InOutSine);
    print(Smooth.lerp(0, 10, 0.5, Ease.Linear)); // 5
  }
};
```

## Behavior and limitations

- An anchored Part eases into and out of the movement. The Console also prints 5.
- Back and Elastic may overshoot scalar, vector and rotation progress. Path parameters, Color and Opacity remain bounded by their backends.
- Animation blend easing affects weights; source time remains linear unless progressionEase is explicitly supplied.
- Check script attachment, enabled state and the Console when an operation fails. Asynchronous errors also appear in operation state. Callback exceptions disable the owner and clean up its actions.
- Owner destruction, disable, successful reload or Stop cleans up owned actions and subscriptions. Failed reload retains the previous behavior's operations. See [Operation handles](/docs/api/operation-handles) for controls and lifetime details.
