# Scripting Rustic games with C++

C++ uses the built-in Rustic API, like every gameplay language. Rustic supplies `rustic.hpp`,
compiles each `.cc`, `.cpp`, or `.cxx` behavior as C++20, and runs one isolated
process per instance. The header parses protocol requests and exposes typed native
helpers, so ordinary game code should not emit JSON.

Install `clang++`, `g++`, or MSVC `cl` on `PATH`. Clang/GCC builds use `-Wall
-Wextra -Werror -std=c++20`; MSVC uses `/nologo /W4`. The editor also writes an
IntelliSense copy to `.rustic/generated/programming/rustic.hpp`, but Rustic supplies
the authoritative temporary copy when building. Never edit or vendor the generated
header.

## Complete behavior

```cpp
#include "rustic.hpp"

double speed = 4.0; // process/instance-local state

void on_start() {
    rustic.log("info", "behavior started");
}

void fixed_update(double dt) {
    const auto forward = rustic.key("KeyW");
    if (forward.held) {
        const auto p = rustic.get_translation();
        rustic.set_translation(p.x, p.y, p.z + speed * dt);
    }
}

void on_destroy() {
    rustic.log("info", "behavior destroyed");
}

int main() {
    return rustic_run(RusticBehavior{
        .on_start = on_start,
        .on_destroy = on_destroy,
        .fixed_update = fixed_update,
    });
}
```

`RusticBehavior` has `on_create`, `on_start`, `on_enable`, `on_disable`,
`on_destroy`, `on_stop`, `fixed_update`, and `update` slots. Omitted callbacks
are handled automatically. Frame callbacks receive seconds. Collision callbacks
are not exposed by this external SDK.

## Typed API

```cpp
const std::string id = rustic.entity_id();
const double frame_dt = rustic.delta_time();
const double physics_dt = rustic.fixed_delta_time();
const RusticVector3 p = rustic.get_translation();
rustic.set_translation(p.x, p.y + 1.0, p.z);

RusticValue speedValue = rustic.get_property("speed");
rustic.set_property("speed", RusticValue{5.0});
RusticValue color = rustic.GetAttribute("Color");
rustic.EditAttribute("Position",
    RusticValue::Array{1.0, 2.0, 3.0});

RusticActionState jump = rustic.input("Jump");
RusticActionState forward = rustic.key("KeyW");
std::vector<RusticKeyEvent> events = rustic.key_events();
bool pressed = rustic.any_key_pressed();
rustic.log("warn", "message");
rustic.set_enabled(false);
```

`RusticActionState` contains `pressed`, `released`, `held`, and `axis`.
`RusticKeyEvent` contains `key`, `state`, and `repeat`. Named actions currently arrive
as an empty map for external adapters. Only held WASD, arrow, and Shift keys from
the embedded Play viewport are populated; press/release fields and key events remain
empty. See the [Lua input guide](scriptingLua.md#input) for exact names and setup.
`RusticValue` supports null, boolean, double, string, array, and object;
use its type accessors only when the stored alternative matches.

## Scene and object operations

```cpp
std::optional<std::string> table = Game.scene.Find("Room.Table");
std::vector<std::string> room = Game.scene.List("Room");
Game.setCurrentCamera("Room.Camera");

instance.add("Cube");
instance.add("assets/models/tree.glb", table);
instance.clone("Room.Table");
```

`Find` returns a stable ID or `std::nullopt`; `List` returns IDs beneath a path
prefix. Instance source strings accept IDs, scene paths, model paths, or built-in
object names. The optional parent is an ID. These mutations are queued and applied
after the callback, in call order, so creation does not return a new ID.

Built-in attributes are `Name`, `Position`, `Size`, `Color`, `CanTouch`,
`CanCollide`, `Anchored`, and `Parent`. Property/attribute writes must retain the
existing engine type. A rejected command fails and disables only this behavior.

## Lifetime, safety, and compatibility

C++ cannot call another behavior process or VM object. API 1.0 does not yet expose
the scheduler's Engine Event `emit`/`subscribe` surface to scripts; cross-language
coordination must use shared engine state.

Create and attach the file in the Explorer as global, scene, or component scope. The
Asset ID—not the path—is stored in the attachment. Global instances run first, then
scene instances, then object components; editor-created references use order `0` and
stable IDs break ties. Process globals persist for that instance until it is destroyed.

The process reads/writes newline-delimited protocol v1 internally. Do not print to
standard output, because it is reserved for the SDK response. Callbacks have a
three-second deadline and 1 MiB response limit. The process environment and working
directory are isolated. Syntax/build failures prevent replacement; runtime failures
disable the instance. Legacy C++ behavior members are the current native contract and
remain backward compatible.

## Target another scene object

See [Edit scene objects](sceneObjects.md) for named-scene hierarchy calls, supported
attributes, copyable examples, and native SDK calls to edit another object. Use your language's native call syntax and its current runtime
limitations.

## Shared gameplay actions

API 1.1 exposes shared-core easing, tweens, movement, skeletal/keyframe/procedural
animation, timelines, timers, paths, cameras, physics, effects, audio and signals.
See [Shared gameplay actions](gameplayActions.md) for attachment, native call
conventions, duration/speed options, callbacks, scene-clock controls and backend limits.
