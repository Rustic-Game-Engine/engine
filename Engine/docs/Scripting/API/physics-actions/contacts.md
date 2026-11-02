# Collision callbacks and slope queries

[Back to Physics APIs](/docs/api/physics-actions).

## Setup

Create an anchored, collidable Rectangular Prism floor and an unanchored Cube at
`[0, 5, 0]`. Enable **Can touch** on both. In Programming create an Object Component,
then select the cube and attach it using **+ Add Component**. Physics runs in Play,
New Window and Standalone; edit mode does not simulate.

## Calls

- `Physics.onCollisionEnter(entity, callback)`
- `Physics.onCollisionStay(entity, callback)`
- `Physics.onCollisionExit(entity, callback)`
- `Physics.ground(entity, maxAngle = 45)`
- `rustic.EditAttribute("Friction", coefficient)` and `rustic.GetAttribute("Friction")`

Subscriptions return an Events connection; call `disconnect()` or
`Events.disconnect(connection)` to remove it. Owner disable, destruction, scene
unload and successful reload clean up subscriptions using the existing event rules.
Callbacks run during the next `Update` event dispatch, after fixed-step collision
resolution. Multiple fixed ticks can queue multiple stay notifications before a
rendered frame. Exceptions disable the behavior using the normal script error path.

Each callback receives `(otherEntityId, normal)`. The ID is a stable string. The
normal is a three-component world vector pointing from the other body toward the
subscribed body. Exit reports the last contact normal; the other ID may refer to
an object that was removed. There is one enter/stay notification per touching pair
per fixed tick, mirrored to both bodies; substeps and solver iterations do not
duplicate notifications. Leaving contact, removing a collider, anchoring both bodies,
or disabling collision/touch produces exit on the next fixed tick. At least one
body must be dynamic, both must enable CanCollide, and both must enable CanTouch.
These are solid-contact callbacks; noncollidable triggers are not implemented.
Collision event names are reserved and cannot be emitted by scripts.

`ground` returns null without an upward support contact, otherwise
`{entity, normal, angle, walkable}`. `angle` is degrees from world up; `walkable`
compares it with `maxAngle`, a finite number from 0 through 90. It chooses the
most upward-facing contact from the latest completed fixed step, independently
of CanTouch. It does not cast a ray, predict the next step, stop a body on a steep
slope, or implement stairs. Query after a fixed step has completed; startup has no
contacts yet. Friction and the solver produce sliding along rotated surfaces.

## JavaScript example

```javascript
globalThis.behavior = {
  Start() {
    rustic.EditAttribute("Friction", 0.5);
    Physics.onCollisionEnter(rustic.entity_id(), (other, normal) => {
      print("Landed on", other, normal);
    });
  },
  Update() {
    const support = Physics.ground(rustic.entity_id(), 45);
    if (support && support.walkable) {
      // Surface projection: tangent = movement - normal * dot(movement, normal).
    }
  }
};
```

Press Play: one landing message appears, followed by stay events if subscribed.
Rotate the floor about Z to test ramps. Set friction to zero to observe sliding.
If no callback arrives, check attachment, Console errors, both flags on both
bodies, and that Update is running. If ground is null in Start, wait for a physics
tick. Merely defining `OnCollisionEnter` lifecycle functions does not subscribe.

## Language bindings

Lua 5.4 and Luau use `Physics.onCollisionEnter(id, function(other, normal) ... end)`
and `Physics.ground(id, 45)`; vectors are one-indexed arrays. JavaScript and HTML
inline scripts use the example above. Python uses the same method names and
`lambda other, normal: ...`. PHP uses `Physics::onCollisionEnter($id, $fn)` and
`Physics::ground($id, 45)`; a callback receives the two positional arguments.

C++ uses `Physics::onCollisionEnter(id, [](const RusticValue::Array& args){ ... })`
and `Physics::ground(id, 45)`, which returns a RusticValue object or null. C# uses
`Physics.onCollisionEnter(id, args => { ... })` and `Physics.ground(id, 45)`, which
returns a JsonElement (test ValueKind for Null). Java uses the same calls with a
Consumer<Object[]> callback and returns an Object (null or Map). Native callback
arrays contain the other ID at index 0 and the normal at index 1.

C uses `Physics.onCollisionEnter(id, fn)` where
`void fn(const RusticValue *args, size_t count)` receives those same two values.
`Physics.ground(id, 45)` returns `RusticGround` with `found`, `entity`, `normal`,
`angle` and `walkable`; check `found` before reading the other fields. C and Java
require an explicit maximum angle. The matching Stay/Exit methods use the same
callback signature in every language.

The shared query protocol is
`{"op":"physics_ground","entity":"<stable-id>","max_angle":45}`.
The event subscription protocol is the existing `connect` gameplay request with
signal `{name:"CollisionEnter",source:"<stable-id>"}` (or CollisionStay/Exit).
Friction accepts numeric values from 0 to 10 and rejects NaN/infinity. Old scenes
without a coefficient load with 0.5. These features use oriented box proxies;
curved primitives and meshes do not gain exact shape collision. Sheared transforms
fall back to enclosing AABBs. See [Basic physics](/docs/guides/physics).
