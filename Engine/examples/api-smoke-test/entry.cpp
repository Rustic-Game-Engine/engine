#include "rustic.hpp"

int main() {
    RusticBehavior behavior;
    behavior.on_start = [] {
        rustic.log("info", "Behavior started");
    };
    behavior.fixed_update = [](double dt) {
        const auto p = rustic.get_translation();
        if (rustic.key("KeyW").held)
            rustic.set_translation(p.x, p.y, p.z + dt);
    };
    return rustic_run(behavior);
}
