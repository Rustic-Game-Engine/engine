-- Luau is an external protocol host, unlike embedded Lua 5.4.
while true do
    local request = io.read("*l")
    if request == nil then break end
    local onStart = string.find(request, '"callback":"on_start"', 1, true) ~= nil
    if onStart then
        print('{"format_version":1,"commands":[{"op":"set_translation","value":[0,0,0]},{"op":"set_property","name":"smoke_value","value":1.0},{"op":"edit_attribute","name":"Position","value":[0,0,0]},{"op":"log","level":"info","message":"Luau API smoke test passed"},{"op":"set_enabled","enabled":true},{"op":"add_instance","source":"Part","parent":null},{"op":"clone_instance","source":"Part","parent":null}]}')
    else
        print('{"format_version":1,"commands":[]}')
    end
end
