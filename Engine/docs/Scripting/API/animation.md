# Animation API

Play imported skeletal clips, register property tracks, animate script values and run procedural poses.

## Setup and language support

These engine-owned services work in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript. Follow [Use gameplay actions](/docs/guides/gameplay-actions) to create and attach a script, install external toolchains, and choose the correct language binding. PHP and HTML scripts belong under `ui/`; CSS alone runs no gameplay code.

The signatures and complete example below use JavaScript. Attach a JavaScript Object Component script to the specified target and press **Play**. Globals are supplied by the engine. Schedule actions from `Start` or a callback, rather than at script top level. Native languages use typed options and their own builder conventions; see the guide's language bindings.

## Calls

- `Animation.clips(entity)`
- `Animation.load(entity, source)`
- `Animation.register(entity, clip)`
- `Animation.addMarker(entity, clipName, time, name)`
- `Animation.play(entity, clipName, options?)`
- `Animation.blend(entity, fromClip, toClip, options?)`
- `Animation.transition(entity, fromClip, toClip, options?)`
- `Animation.stop(handle) / Animation.pause(handle)`
- `Animation.speed(handle, value) / Animation.loop(handle, enabled)`
- `handle.stop() / speed(value) / loop(enabled) / onMarker(name, callback)`
- `Animation.value(keys, onUpdate)`
- `Animation.ik(root, middle, tip, target, options?)`
- `Animation.footPlacement(root, middle, tip, target, options?)`
- `Animation.lookAt / headTracking(entity, position, duration, easing?)`
- `Animation.recoil(entity, quaternion, duration, easing?)`

## Parameters

| Parameter | Meaning |
| --- | --- |
| clip | Object with name, positive duration, tracks and optional markers. A track contains target and keys. |
| keys | At least two time/value entries beginning at zero with strictly increasing times; a starting key's easing controls the following interval. |
| play options | speed (default 1), loop, blendIn/Out (seconds), blendInEase/OutEase, weight (default 1), additive, mask, layered, progressionEase. |
| blend options | duration (default 0.3 seconds) and easing for the transition, plus playback options. |
| marker | Source-clock time and event name; add before starting playback. |
| IK options | pole, weight, duration and easing; joints must form a direct root/middle/tip parent chain. |

## Return and timing

Clips lists registered names; load/register/addMarker update the engine registry. Play returns a handle with stop, speed, loop and onMarker in addition to standard controls. Value and procedural actions return operation handles; value callbacks receive the interpolated scalar/vector/quaternion.

## Example

```javascript
globalThis.behavior = {
  Start() {
    const actor = rustic.entity_id();
    Animation.register(actor, {
      name: "Reveal", duration: 1,
      tracks: [{target: "Opacity", keys: [
        {time: 0, value: 0, easing: Ease.OutSine},
        {time: 1, value: 1}
      ]}], markers: [{time: 0.5, name: "Halfway"}]
    });
    Animation.play(actor, "Reveal")
      .onMarker("Halfway", () => print("halfway"))
      .onFinished(() => print("revealed"));
  }
};
```

## Behavior and limitations

- Attach the example to a Part: it becomes opaque over one second and prints halfway, then revealed.
- Import glTF/GLB/FBX through Explorer and attach to the mesh entity to play skeletal clips. Source node_N descendants, inverse binds, skin weights and named clips are bound before Start. Use clips to discover actual names. Explicit load requires a fresh target.
- Tracks target Position, Rotation, Scale, Color, Opacity, Fov or child/path/Property. C/C++ use registerClip; native facades use typed clip/key/marker records.
- Source time is linear at selected speed unless progressionEase warps sampling. Markers use raw source time; running actions retain their clip snapshot. Blend duration stays in scene seconds when speed changes.
- Blend/transition samples both source and destination clips. Start a base clip before a masked layer; masks match track names or hierarchy prefixes. Additive tracks apply a delta from their initial pose.
- Turning loop off finishes the current cycle. Stop cancels without resetting the pose; completion retains the final pose unless blend-out restores the base.
- IK solves in world space, preserves segment lengths and clamps unreachable targets. FootPlacement aliases IK; headTracking is finite lookAt, requiring controller updates for changing targets.
- Clip limits are 4096 tracks, one million keys and 4096 markers. Add markers before playback because running actions retain their clip snapshots.
- glTF retains linear/step/cubic-spline interpolation; FBX is baked at 60 Hz. Live renderer skinning uses four linear influences. Morph targets, dual-quaternion skinning and animation graph/editor UI are not implemented.
- Check script attachment, enabled state and the Console when an operation fails. Asynchronous errors also appear in operation state. Callback exceptions disable the owner and clean up its actions.
- Owner destruction, disable, successful reload or Stop cleans up owned actions and subscriptions. Failed reload retains the previous behavior's operations. See [Operation handles](/docs/api/operation-handles) for controls and lifetime details.
