# Physics queries and forces API

Query live primitive colliders and change simulated body velocity.

## Setup and language support

These engine-owned services work in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript. Follow [Use gameplay actions](/docs/guides/gameplay-actions) to create and attach a script, install external toolchains, and choose the correct language binding. PHP and HTML scripts belong under `ui/`; CSS alone runs no gameplay code.

The signatures and complete example below use JavaScript. Attach a JavaScript Object Component script to the specified target and press **Play**. Globals are supplied by the engine. Schedule actions from `Start` or a callback, rather than at script top level. Native languages use typed options and their own builder conventions; see the guide's language bindings.

## Calls

- `Physics.raycast(origin, direction, distance, ignore?)`
- `Physics.sphereCast(origin, direction, distance, radius, ignore?)`
- `Physics.overlap(center, radius, ignore?)`
- `Physics.force(entity, vector, delta?)`
- `Physics.impulse(entity, vector) / Physics.knockback(...)`
- `Physics.launch(entity, velocity)`
- `Physics.explosion(center, radius, strength, ignore?)`

## Parameters

Origins, centers and vectors are finite three-component world vectors. Direction must be nonzero and is normalized by the engine. Ignore is an optional array of entity IDs in JavaScript; native query signatures differ. Force delta defaults to fixed_delta_time in JavaScript.

## Return and timing

Raycast/sphereCast immediately return the nearest {entity, point, normal, distance} or null; overlap returns entity IDs. Forces mutate physics state synchronously, rather than returning action handles.

## Example

```javascript
globalThis.behavior = {
  Start() {
    const hit = Physics.raycast([0, 4, 0], [0, -1, 0], 10);
    if (hit) print("ground", hit.entity, hit.distance);
    Physics.launch(rustic.entity_id(), [0, 6, 0]);
  }
};
```

## Behavior and limitations

- Attach to an unanchored collidable Part above an anchored collidable floor. The query reports the nearest hit; launch replaces the Part velocity with an upward velocity.
- Queries use primitive world AABBs including parent transforms. SphereCast uses rounded sphere/box contacts; model meshes do not automatically acquire primitive colliders.
- The current simulator uses unit mass. Impulse/knockback adds velocity; launch sets it; force integrates over its interval. Clock scaling affects physics.
- Direct force, impulse or launch on an anchored or nonphysical object fails. Explosion uses linear radial falloff and skips anchored bodies.
- See [Basic physics](/docs/guides/physics) for collider setup, gravity and collision limits.
- Check script attachment, enabled state and the Console when an operation fails. Asynchronous errors also appear in operation state. Callback exceptions disable the owner and clean up its actions.
- Owner destruction, disable, successful reload or Stop cleans up owned actions and subscriptions. Failed reload retains the previous behavior's operations. See [Operation handles](/docs/api/operation-handles) for controls and lifetime details.
