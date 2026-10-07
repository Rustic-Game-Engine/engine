using static Rustic;

Run((callback, dt) => {
    if (callback == "on_start") rustic.log("info", "Behavior started");
    if (callback == "fixed_update" && rustic.key("KeyW").held) {
        var p = rustic.get_translation();
        rustic.set_translation(p[0], p[1], p[2] + dt);
    }
});
