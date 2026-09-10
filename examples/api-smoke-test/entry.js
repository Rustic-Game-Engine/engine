function inspectApi() {
  const [x, y, z] = rustic.get_translation();
  const id = rustic.entity_id();
  rustic.delta_time(); rustic.fixed_delta_time();
  const value = rustic.get_property("smoke_value");
  const position = rustic.get_attribute("Position");
  rustic.GetAttribute("Name"); rustic.input("Move"); rustic.key("KeyW");
  rustic.key_events(); rustic.any_key_pressed();
  Game.scene.Find("Game.scene"); Game.scene.List("Game.scene");
  void Game.scene.Camera;
  rustic.set_translation(x, y, z);
  rustic.set_property("smoke_value", value ?? 1.0);
  if (position !== undefined) rustic.EditAttribute("Position", position);
  rustic.set_enabled(true);
  return id;
}

globalThis.behavior = {
  on_create() { inspectApi(); },
  on_start() {
    const id = inspectApi();
    instance.add("Part"); instance.clone(id);
    rustic.log("info", "JavaScript API smoke test passed");
  },
  fixed_update(_) { inspectApi(); },
  update(_) { inspectApi(); },
  on_destroy() { rustic.log("info", "JavaScript behavior destroyed"); },
  on_stop() { rustic.log("info", "JavaScript behavior stopped"); },
};

