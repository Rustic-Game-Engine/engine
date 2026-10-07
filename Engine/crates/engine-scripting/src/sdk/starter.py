from rustic import rustic, instance, Game, run

def on_start():
    rustic.log("info", "Behavior started")

def fixed_update(dt):
    x, y, z = rustic.get_translation()
    if rustic.key("KeyW")["held"]:
        rustic.set_translation(x, y, z + dt)

run(globals())
