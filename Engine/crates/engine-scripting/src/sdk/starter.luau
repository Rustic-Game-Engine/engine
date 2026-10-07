return {
    on_start = function() rustic.log("info", "Behavior started") end,
    fixed_update = function(dt)
        local x, y, z = rustic.get_translation()
        if rustic.key("KeyW").held then rustic.set_translation(x, y, z + dt) end
    end,
}
