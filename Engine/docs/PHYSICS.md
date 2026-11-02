# Physics: forces, friction, contacts and slopes

Built-in primitives simulate automatically in Play, New Window, and Standalone.
No script or physics component is required. Edit mode never moves objects. Gravity
points down world Y at 9.81 units per second squared. Use one unit as one metre.

## Make a falling box

1. Open a scene in the editor. Add a **Rectangular Prism** from the object menu and
   name it `Floor`. Set its primitive dimensions to `[20, 1, 20]`, position to
   `[0, -0.5, 0]`, and transform scale to `[1, 1, 1]`.
2. Select `Floor`. In the Inspector, check **Anchored** and **Can collide**.
3. Add a **Cube**, name it `Box`, use primitive size `1`, position `[0, 5, 0]`, and
   scale `[1, 1, 1]`. Leave **Anchored** unchecked and **Can collide** checked.
4. Add a camera looking toward the box and floor, then press **Play**. The box falls
   and rests with its centre near Y = 0.5. Add another box higher up to make a stack.
5. Pause to freeze physics. **Frame Advance** performs one fixed step, including
   scripts and physics. Stop and start Play to reset positions and velocities.
   Applying runtime changes after Stop can deliberately save the simulated positions.

For a 2D scene, use a wide **Rectangle2D** as the anchored platform and a smaller
Rectangle2D or Circle2D above it. Gravity still acts along world Y.

## Attributes

| Attribute | Runtime effect |
| --- | --- |
| `Anchored = true` | Physics does not move the primitive and clears its velocity on the next fixed tick. Scripts and parent transforms can still move it. |
| `Anchored = false` | The primitive falls, including when collision is disabled. Unanchoring starts with zero velocity after an anchored tick. |
| `CanCollide = true` | Solid response occurs only when both primitives have this flag enabled. Anchored primitives can be floors or walls. |
| `CanCollide = false` | The object passes through other objects; it still falls if unanchored. |
| `CanTouch` | Enables collision subscriptions when both bodies allow touch, plus object clicks and taps. Does not change solid response. |
| `Friction` | Finite coefficient from 0 to 10; default 0.5. Zero is frictionless. Set it in the Inspector or through `EditAttribute`. The contact coefficient is the geometric mean of both values. |
| `Size` | Local transform scale, multiplied by primitive dimensions for collision bounds. |

These flags are read every fixed tick, after all `FixedUpdate` script callbacks.
Scripts that change `Position` teleport objects; position writes do not calculate
velocity. Avoid resetting a falling object's position from every `Update` callback.
Disabling a behavior stops its script callbacks, not its object's physics.

## Release a box from a script

The scene above works without scripts. To hold the box until startup, check its
**Anchored** checkbox, use **Programming** to create an **Object Component** Lua
script, then select `Box` and attach it through **+ Add Component** in the Inspector.
Replace the script source with:

```lua
return {
  Start = function()
    rustic.EditAttribute("CanCollide", true)
    rustic.EditAttribute("Anchored", false)
    print("Box released")
  end,
}
```

Press Play. The Console shows `Box released` and the box falls onto the floor.
JavaScript uses the same attributes:

```javascript
globalThis.behavior = {
  Start() {
    rustic.EditAttribute("CanCollide", true);
    rustic.EditAttribute("Anchored", false);
  },
};
```

Attach this version as a JavaScript Object Component instead of the Lua script.
Scene paths can also target other objects; see the **Edit scene objects** guide.

## Forces, friction and slopes

Use the falling-box scene above. Create a JavaScript **Object Component** in
Programming, attach it to `Box` with **+ Add Component**, and replace its source:

```javascript
globalThis.behavior = {
  Start() {
    rustic.EditAttribute("Friction", 0.6);
    Physics.onCollisionEnter(rustic.entity_id(), (other, normal) => {
      print("Contact", other, "normal", normal);
    });
    Physics.onCollisionExit(rustic.entity_id(), other => print("Left", other));
    Physics.impulse(rustic.entity_id(), [2, 0, 0]);
  },
  FixedUpdate(dt) {
    // Unit mass: an upward force of 9.81 cancels gravity over this interval.
    // Physics.force(rustic.entity_id(), [0, 9.81, 0], dt);
    const support = Physics.ground(rustic.entity_id(), 45);
    if (support && support.walkable) {
      // normal is world-space; angle is degrees from world up.
      // Use support.normal to project controller movement along the surface.
    }
  }
};
```

Press Play. The box falls, emits one enter callback, and slows on the floor.
Rotate the anchored floor about Z to make a ramp. With zero friction on either
body the box slides down the slope; sufficiently high friction holds it. There
is no automatic character stair stepping or slope-angle movement clamp.
`Physics.ground` reads contacts from the most recent completed fixed step, so it
is null before the first physics tick and may lag a scripted teleport by a tick.

`Physics.force` adds `force * scaledDelta` to velocity. Call it once per
`FixedUpdate`, passing `dt` in languages requiring an explicit interval. Impulse
adds velocity immediately; launch replaces it. Applying force from every rendered
frame with a fixed interval produces frame-rate-dependent acceleration.

## Current limits

- Only built-in primitives participate. Imported mesh assets, cameras, lights,
  folders, and empty nodes have no physics bodies. Runtime-created primitives join
  the next fixed tick; removed primitives lose their velocity state.
- Collision uses oriented boxes enclosing primitive dimensions, transformed by rotation, scale, and the hierarchy. Rotated boxes and planes provide slope normals. Spheres and circles still behave as boxes and torus holes are solid. Sheared parent transforms fall back to world-aligned boxes. Planes and 2D shapes have a minimum collision half-thickness of 0.025 units.
- Bodies have equal unit mass and translate without rotating, bouncing, joints or configurable gravity. Coulomb friction reduces relative tangential motion and can hold bodies on shallow slopes. Live primitive raycasts, sphere casts, overlaps, forces, impulses and velocity changes are available through the Physics scripting API.
- Swept box checks reduce tunnelling during simulated falls. Script teleports and
  script-moved anchored platforms are not swept. Fast multi-body impacts and large
  stacks are approximate; use separated starting positions and modest scene sizes.
- The runtime normally uses 60 fixed ticks per second with four physics substeps.
  Falling speed is capped at 100 units per second. A single supplied physics interval
  above 0.1 seconds is clamped. Collision pairs are checked directly, so cost grows
  quadratically with the number of primitives.
- Child bodies receive world-space gravity and are converted back to local
  positions. Anchored children still follow their parent's transform. For predictable
  independent rigid bodies, place them at scene root or under an organizational folder.
- Collision callbacks use explicit `Physics.onCollisionEnter`, `Physics.onCollisionStay`, and `Physics.onCollisionExit` subscriptions. Merely declaring lifecycle functions named `OnCollisionEnter` does not register a subscription. See [Contacts and slopes](/docs/api/physics-actions/contacts).

## Diagnose problems

- **Nothing falls:** enter Play, resume if paused, verify the object has a built-in
  primitive, and uncheck Anchored. Confirm a script is not restoring its position.
- **Everything falls away:** floors also default to unanchored. Anchor the floor;
  there is no invisible ground or world boundary.
- **The box falls through:** enable Can collide on both the box and floor. Imported
  models need a separate built-in primitive as a collision proxy for now.
- **Objects hover or collide too early:** check primitive dimensions and transform
  scale. Curved primitives use enclosing boxes; sheared transforms use world-aligned enclosing boxes.
- **The object vanishes:** check the camera view and floor placement. Without a
  collidable floor, a body continues falling beyond the camera.
- **A script release fails:** verify attachment and language, and read Console
  errors. Attribute values must be booleans, not strings such as `"false"`.
