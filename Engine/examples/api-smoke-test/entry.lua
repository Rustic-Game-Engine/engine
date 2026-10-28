local function inspect_api()
  local x, y, z = rustic.get_translation()
  local id = rustic.entity_id()
  local dt = rustic.delta_time()
  local fixed_dt = rustic.fixed_delta_time()
  local property = rustic.get_property("smoke_value")
  local position = rustic.get_attribute("Position")
  rustic.GetAttribute("Name")
  rustic.input("Move")
  rustic.key("KeyW")
  rustic.key_events()
  rustic.any_key_pressed()
  rustic.find_entity("Game.scene")
  Game.scene.Find("Game.scene")
  Game.scene.List("Game.scene")

  rustic.set_translation(x, y, z)
  if property ~= nil then rustic.set_property("smoke_value", property) end
  if position ~= nil then rustic.EditAttribute("Position", position) end
  rustic.set_enabled(true)
  return id, dt, fixed_dt
end

return {
  on_create = function() inspect_api() end,
  on_start = function()
    inspect_api()
    rustic.log("info", "Behavior started")
  end,
  fixed_update = function(dt)
    local x, y, z = rustic.get_translation()
    if rustic.key("KeyW").held then rustic.set_translation(x, y, z + dt) end
  end,
  update = function(_) inspect_api() end,
  on_destroy = function() rustic.log("info", "Lua behavior destroyed") end,
  on_stop = function() rustic.log("info", "Lua behavior stopped") end,
}

