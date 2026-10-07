#include "rustic.h"

void start(void) { rustic.log("info", "Behavior started"); }
void fixed(double dt) {
    RusticVector3 p = rustic.get_translation();
    if (rustic.key("KeyW").held) rustic.set_translation(p.x, p.y, p.z + dt);
}
int main(void) {
    return rustic_run((RusticBehavior){.on_start=start, .fixed_update=fixed});
}
