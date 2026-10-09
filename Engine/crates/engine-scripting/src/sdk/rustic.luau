local state = {}
local commands = {}
local output = print
local function push(op, fields)
    fields.op = op
    table.insert(commands, fields)
end
local function inactive() return {pressed=false,released=false,held=false,axis=0} end
local function objectPath(source)
    return setmetatable({__source=source}, {__index=function(_, name)
        if name=='EditAttribute' or name=='edit_attribute' then
            return function(_, attribute, value) push('edit_attribute',{source=source,name=attribute,value=value}) end
        end
        return objectPath(source .. '.' .. name)
    end})
end
rustic = {game=objectPath('rustic.game')}
function rustic.entity_id() return state.entity_id end
function rustic.delta_time() return state.delta_time end
function rustic.fixed_delta_time() return state.fixed_delta_time end
function rustic.get_translation() return table.unpack(state.translation) end
function rustic.set_translation(x,y,z) push('set_translation',{value={x,y,z}}) end
function rustic.get_property(name) return state.properties[name] end
function rustic.set_property(name,value) push('set_property',{name=name,value=value}) end
function rustic.get_attribute(name) return state.attributes[name] end
rustic.GetAttribute = rustic.get_attribute
function rustic.edit_attribute(name,value) push('edit_attribute',{name=name,value=value}) end
rustic.EditAttribute = rustic.edit_attribute
function rustic.input(name) return state.actions[name] or inactive() end
function rustic.key(name) return state.keys[name] or inactive() end
function rustic.key_events() return table.clone(state.key_events) end
function rustic.any_key_pressed() return state.any_key_pressed end
function rustic.log(level,message) push('log',{level=tostring(level),message=tostring(message)}) end
function rustic.set_enabled(enabled) push('set_enabled',{enabled=enabled}) end
function rustic.query(request)
    local pending=commands; commands={}
    return __rustic_query({query=request,commands=pending})
end
function rustic.gameplay(request) push('gameplay',{request=request}) end
function rustic.gameplay_connections() return state.gameplay_connections or {} end
function rustic.gameplay_states() return state.gameplay_states or {} end
function rustic.gameplay_callbacks() return state.gameplay_callbacks or {} end
instance = {}
function instance.add(source,parent) push('add_instance',{source=source,parent=parent}) end
function instance.clone(source,parent) push('clone_instance',{source=source,parent=parent}) end
Game = {scene={}}
function Game.scene.Find(path) return state.scene_paths[path] end
function Game.scene.List(path)
    local result = {}
    local prefix = (not path or path=='' or path=='Game.scene') and '' or path .. '.'
    for name,id in pairs(state.scene_paths) do
        if string.sub(name,1,#prefix)==prefix then table.insert(result,id) end
    end
    return result
end
function Game.setCurrentCamera(source)
    if type(source)=='table' then source=source[1] end
    assert(type(source)=='string','expected a camera path or ID')
    push('set_current_camera',{source=source})
end
local function quote(s)
    return '"' .. string.gsub(s, '[%c\\"]', function(c) return string.format('\\u%04x',string.byte(c)) end) .. '"'
end
local function encode(v)
    local t = type(v)
    if t=='nil' then return 'null' end
    if t=='string' then return quote(v) end
    if t=='boolean' then return tostring(v) end
    if t=='number' then assert(v==v and math.abs(v)~=math.huge,'non-finite value'); return string.format('%.17g',v) end
    assert(t=='table','unsupported engine value')
    local result = {}
    if #v>0 then
        for _,x in ipairs(v) do table.insert(result,encode(x)) end
        return '[' .. table.concat(result,',') .. ']'
    end
    local arrays={commands=true,actions=true,mask=true,tracks=true,keys=true,markers=true,points=true,ignore=true,arguments=true}
    for k,x in pairs(v) do local encoded=(arrays[k] and type(x)=='table' and next(x)==nil) and '[]' or encode(x);table.insert(result,quote(k)..':'..encoded) end
    return '{' .. table.concat(result,',') .. '}'
end
print = function(...) local a={};for _,v in ipairs({...}) do table.insert(a,tostring(v)) end;rustic.log('info',table.concat(a,'\t')) end
warn = function(...) local a={};for _,v in ipairs({...}) do table.insert(a,tostring(v)) end;rustic.log('warn',table.concat(a,'\t')) end
local aliases={on_create='OnCreate',on_start='Start',on_enable='OnEnable',on_disable='OnDisable',on_destroy='OnDestroy',on_stop='OnStop',fixed_update='FixedUpdate',update='Update'}
function __rustic_invoke(nextState)
    state=nextState; commands={}
    local response
    local ok,err=pcall(function()
        if not __rustic_behavior then __rustic_behavior=assert(__rustic_load(),'return a behavior table') end
        if state.callback=='update' and __rustic_gameplay_dispatch then __rustic_gameplay_dispatch() end
        local callback=__rustic_behavior[state.callback] or __rustic_behavior[aliases[state.callback]]
        if callback then
            if state.callback=='update' or state.callback=='fixed_update' then callback(state.delta or 0) else callback() end
        end
        local items={};for _,c in ipairs(commands) do table.insert(items,encode(c)) end
        response='{"format_version":1,"commands":['..table.concat(items,',')..']}'
    end)
    if not ok then output('{"format_version":1,"error":'..quote(tostring(err))..',"commands":[]}');return end
    output(response)
end
