-- Thin Lua binding: Rust owns all progress, curves, paths, and composition.
function Game.scene.getEnvironment() return rustic.query({op='scene_environment'}) end
function Game.scene.setEnvironment(settings)
    assert(type(settings)=='table','environment settings must be a table')
    if next(settings)==nil then return Game.scene.getEnvironment() end
    return rustic.query({op='scene_environment',settings=settings})
end
function Game.scene.setSkyTexture(path)
    assert(type(path)=='string','sky texture path must be a string')
    return Game.scene.setEnvironment({sky_image=path})
end
local callbacks = {}
local handles = {}
local connections={}
local serial = 0
local function token()
    if rustic.new_handle then return rustic.new_handle() end
    serial = serial + 1
    return 'gameplay-' .. tostring(serial)
end
local function callback(fn, persistent)
    assert(type(fn)=='function','expected a callback')
    local id=token(); callbacks[id]={fn=fn,persistent=persistent}; return id
end
local function send(request) rustic.gameplay(request) end
local function entity(object)
    if object==nil then return rustic.entity_id() end
    if type(object)=='table' then object=rawget(object,'__source') or rawget(object,'id') end
    assert(type(object)=='string','expected an entity ID or scene path')
    if string.sub(object,1,11)=='game.scene.' then return rustic.query({op='scene_object',source=object}) end
    local id=rustic.find_entity and rustic.find_entity(object) or Game.scene.Find(object)
    -- Stable UUIDs are already handles, not paths.
    if not id and string.match(object,'^%x%x%x%x%x%x%x%x%-') then id=object end
    assert(id,'object was not found: '..object); return id
end
local function options(timing,easing)
    if type(timing)=='table' then
        assert(not (timing.duration and timing.speed),'choose duration or speed')
        local r={duration=timing.duration,speed=timing.speed,easing=timing.easing or easing or 'Linear'}
        assert(r.duration~=nil or r.speed~=nil,'duration or speed is required');return r
    end
    return {duration=timing or 0,easing=easing or 'Linear'}
end
Ease={Linear='Linear'}
for _,family in ipairs({'Sine','Quad','Cubic','Quart','Quint','Expo','Circ','Back','Elastic','Bounce'}) do
    for _,direction in ipairs({'In','Out','InOut'}) do Ease[direction..family]=direction..family end
end
Vector3 = Vector3 or function(x,y,z) return {x,y,z} end
local function tween(object,property,to,timing,easing,from)
    local o=options(timing,easing)
    return {kind='tween',target={entity=entity(object),property=property},from=from,to=to,duration=o.duration,speed=o.speed,easing=o.easing}
end
local function start(action,opts)
    opts=opts or {};local id=token();local handle={id=id}
    local owned={}
    local function collect(a)
        if a.kind=='callback' or a.kind=='value' then table.insert(owned,a.token) end
        if a.marker_token then table.insert(owned,a.marker_token) end
        for _,child in ipairs(a.actions or {}) do collect(child) end
    end
    collect(action)
    local function cleanup() for _,t in ipairs(owned) do callbacks[t]=nil end;handles[id]=nil end
    local done=callback(function() cleanup();if handle.finished then handle.finished() end end)
    handles[id]={handle=handle,done=done,cleanup=cleanup}
    for _,command in ipairs({'pause','resume','cancel','reverse'}) do
        handle[command]=function()
            send({command=command,handle=id})
            if command=='cancel' then callbacks[done]=nil;cleanup() end
            return handle
        end
    end
    handle.onFinished=function(a,b) handle.finished=b or a;assert(type(handle.finished)=='function','expected callback');return handle end
    handle.state=function()
        for _,state in ipairs(rustic.gameplay_states()) do if state.handle==id then return state end end
        return nil
    end
    local repeats=opts.repeats or 0
    local playback={ping_pong=opts.pingPong or false,looping=opts.loop or false,repeats=repeats}
    if not opts.loop then playback.repeats=repeats end
    send({command='start',handle=id,action=action,playback=playback,on_finished=done})
    return handle
end
Tween={}
function Tween.value(from,to,duration,fn,easing)
    local o=options(duration,easing)
    return start({kind='value',from=from,to=to,duration=o.duration,speed=o.speed,easing=o.easing,token=callback(fn,true)})
end
function Tween.to(object,property,to,duration,easing) return start(tween(object,property,to,duration,easing)) end
function Tween.move(object,to,duration,easing) return Tween.to(object,'Position',to,duration,easing) end
function Tween.rotate(object,to,duration,easing) return Tween.to(object,'Rotation',to,duration,easing) end
function Tween.scale(object,to,duration,easing) return Tween.to(object,'Scale',to,duration,easing) end
Movement={moveTo=Tween.move,rotateTo=Tween.rotate}
function Movement.move(object,offset,timing,easing)
    local o=options(timing,easing)
    return start({kind='move',entity=entity(object),offset=offset,duration=o.duration,speed=o.speed,easing=o.easing})
end
function Movement.lookAt(object,position,duration,easing)
    return start({kind='look_at',entity=entity(object),position=position,duration=duration or 0,easing=easing or Ease.Linear})
end
function Movement.follow(object,target,duration,easing,offset)
    return start({kind='follow',entity=entity(object),target=entity(target),duration=duration,easing=easing or Ease.Linear,offset=offset or {0,0,0}})
end
function Movement.orbit(object,center,radius,turns,duration,easing)
    return start({kind='orbit',entity=entity(object),center=center,radius=radius,turns=turns,duration=duration,easing=easing or Ease.Linear})
end
Path={}
function Path.create(points,kind)
    local result={kind=kind or 'Linear',points={}}
    for _,point in ipairs(points) do table.insert(result.points,point.point and point or {point=point,easing=Ease.Linear}) end
    return result
end
function Path.follow(object,path,opts)
    opts=opts or {};local o=options(opts)
    return start({kind='path',entity=entity(object),path=path,duration=o.duration,speed=o.speed,easing=o.easing,orient_to_path=opts.orientToPath or false},opts)
end
Timer={}
function Timer.after(delay,fn) return start({kind='sequence',actions={{kind='wait',duration=delay},{kind='callback',token=callback(fn)}}}) end
function Timer.every(interval,fn,count)
    assert(interval>0,'repeat interval must be positive')
    assert(count==nil or (count>=1 and count%1==0),'count must be a positive integer')
    local cb=callback(fn,true)
    local h=start({kind='sequence',actions={{kind='wait',duration=interval},{kind='callback',token=cb}}},{loop=count==nil,repeats=count and count-1 or 0})
    local cancel=h.cancel
    h.cancel=function() callbacks[cb]=nil;return cancel() end
    return h.onFinished(function() callbacks[cb]=nil end)
end
Sequence={}
function Sequence.new()
    local builder={actions={}}
    local function args(...) local a={...};if a[1]==builder then table.remove(a,1) end;return a end
    builder.move=function(...) local a=args(...);table.insert(builder.actions,tween(a[1],'Position',a[2],a[3],a[4]));return builder end
    builder.to=function(...) local a=args(...);table.insert(builder.actions,tween(a[1],a[2],a[3],a[4],a[5]));return builder end
    builder.animation=function(...) local a=args(...);table.insert(builder.actions,Animation.action(a[1],a[2],a[3]));return builder end
    builder.wait=function(...) local a=args(...);table.insert(builder.actions,{kind='wait',duration=a[1]});return builder end
    builder.call=function(...) local a=args(...);table.insert(builder.actions,{kind='callback',token=callback(a[1])});return builder end
    builder.parallel=function(...) local a=args(...);local items={};for _,b in ipairs(a[1]) do table.insert(items,{kind='sequence',actions=b.actions}) end;table.insert(builder.actions,{kind='parallel',actions=items});return builder end
    builder.play=function() assert(#builder.actions>0,'sequence is empty');return start({kind='sequence',actions=builder.actions}) end
    return builder
end
Timeline=Sequence
Effects={}
function Effects.fade(object,opacity,duration,easing) return Tween.to(object,'Opacity',opacity,duration,easing) end
function Effects.flash(object,color,duration,easing)
    return start({kind='tween',target={entity=entity(object),property='Color'},to=color,duration=duration,easing=easing or Ease.Linear},{repeats=1,pingPong=true})
end
Camera={}
-- Explicit camera entity avoids hidden active-camera state.
function Camera.moveTo(camera,position,duration,easing) return Tween.move(camera,position,duration,easing) end
function Camera.zoom(camera,fov,duration,easing) return Tween.to(camera,'Fov',fov,duration,easing) end
Camera.fov=Camera.zoom;Camera.follow=Movement.follow;Camera.orbit=Movement.orbit;Camera.lookAt=Movement.lookAt
function Effects.shake(object,strength,duration,easing) return start({kind='shake',entity=entity(object),strength=strength,duration=duration,easing=easing or Ease.Linear}) end
Camera.shake=Effects.shake
Events={}
local function connect(name,fn,once,source)
    local id=callback(fn,not once)
    send({command='connect',token=id,signal={name=name,source=source and entity(source)},once=once or false})
    connections[id]=true
    local c={}
    c.disconnect=function() connections[id]=nil;callbacks[id]=nil;send({command='disconnect',token=id}) end
    return c
end
function Events.on(name,fn) return connect(name,fn,false) end
function Events.once(name,fn) return connect(name,fn,true) end
function Events.connect(object,name,fn) return connect(name,fn,false,object) end
function Events.emit(name,arguments,source) send({command='emit',signal={name=name,source=source and entity(source)},arguments=arguments}) end
function Events.disconnect(connection) connection.disconnect() end
function __rustic_gameplay_dispatch()
    local live={};for _,id in ipairs(rustic.gameplay_connections()) do live[id]=true end
    for id in pairs(connections) do if not live[id] then callbacks[id]=nil;connections[id]=nil end end
    local existing={};for id,h in pairs(handles) do existing[id]=h end
    for _,delivery in ipairs(rustic.gameplay_callbacks()) do
        local cb=callbacks[delivery.token]
        if cb then
            if not cb.persistent then callbacks[delivery.token]=nil end
            cb.fn(table.unpack(delivery.arguments or {}))
        end
    end
    local present={}
    for _,state in ipairs(rustic.gameplay_states()) do
        present[state.handle]=true
        if state.status=='failed' or state.status=='cancelled' then
            local h=handles[state.handle]
            if h then callbacks[h.done]=nil;h.cleanup() end
            if h and state.status=='failed' then rustic.log('error',state.error or 'gameplay action failed') end
        end
    end
    for id,h in pairs(existing) do if not present[id] then callbacks[h.done]=nil;h.cleanup() end end
end


Smooth={}
function Smooth.lerp(a,b,t,e) return rustic.query({op='lerp',from=a,to=b,progress=t,easing=e or Ease.Linear}) end
function Smooth.slerp(a,b,t,e) return rustic.query({op='slerp',from=a,to=b,progress=t,easing=e or Ease.Linear}) end
function Smooth.inverseLerp(a,b,v) return rustic.query({op='inverse_lerp',from=a,to=b,value=v}) end
function Smooth.remap(v,a,b,c,d) return rustic.query({op='remap',value=v,in_min=a,in_max=b,out_min=c,out_max=d}) end
function Smooth.smoothDamp(c,t,v,s,d) return rustic.query({op='smooth_damp',current=c,target=t,velocity=v,smooth_time=s,delta=d}) end
Interpolation=Smooth
Effects.pulse=function(o,scale,duration,e) return start(tween(o,'Scale',scale,duration,e),{repeats=1,pingPong=true}) end
Camera.transition=function(o,position,rotation,fov,duration,e)
    return start({kind='parallel',actions={tween(o,'Position',position,duration,e),tween(o,'Rotation',rotation,duration,e),tween(o,'Fov',fov,duration,e)}})
end

Physics={}
function Physics.raycast(origin,direction,distance,ignore) return rustic.query({op='physics_raycast',origin=origin,direction=direction,distance=distance,ignore=ignore or {}}) end
function Physics.sphereCast(origin,direction,distance,radius,ignore) return rustic.query({op='physics_sphere_cast',origin=origin,direction=direction,distance=distance,radius=radius,ignore=ignore or {}}) end
function Physics.overlap(center,radius,ignore) return rustic.query({op='physics_overlap',center=center,radius=radius,ignore=ignore or {}}) end
function Physics.impulse(o,v) return rustic.query({op='physics_impulse',entity=entity(o),vector=v}) end
function Physics.force(o,v,delta) return rustic.query({op='physics_force',entity=entity(o),vector=v,delta=delta or rustic.fixed_delta_time()}) end
function Physics.launch(o,v) return rustic.query({op='physics_launch',entity=entity(o),vector=v}) end
Physics.knockback=Physics.impulse
function Physics.explosion(center,radius,strength,ignore) return rustic.query({op='physics_explosion',center=center,radius=radius,strength=strength,ignore=ignore or {}}) end

Animation={}
function Animation.load(o,source) return rustic.query({op="animation_load",entity=entity(o),source=source}) end
function Animation.register(o,clip) rustic.query({op='animation_register',entity=entity(o),clip=clip}) end
function Animation.clips(o) return rustic.query({op='animation_list',entity=entity(o)}) end
local function animationAction(o,name,opts)
    opts=opts or {};local id=entity(o)
    local clip=type(name)=='table' and name or rustic.query({op='animation_ref',entity=id,name=name})
    return {kind='animation_ref',entity=id,clip=clip,options={speed=opts.speed or 1,blend_in=opts.blendIn or 0,blend_in_ease=opts.blendInEase or opts.easing or Ease.Linear,progression_ease=opts.progressionEase,blend_out=opts.blendOut or 0,blend_out_ease=opts.blendOutEase or opts.easing or Ease.Linear,weight=opts.weight or 1,additive=opts.additive or false,layered=opts.layered or (opts.mask and #opts.mask>0) or false},blend_source=opts.sourceClip,mask=opts.mask or {}}
end
Animation.action=animationAction
function Animation.play(o,name,opts)
    opts=opts or {};local action=animationAction(o,name,opts);local markers={}
    action.marker_token=callback(function(name) if markers[name] then markers[name]() end end,true)
    local h=start(action,opts)
    h.onMarker=function(a,b,c) local name,fn=a,b;if a==h then name,fn=b,c end;assert(type(fn)=='function','expected callback');markers[name]=fn;return h end
    h.stop=h.cancel
    h.speed=function(a,b) send({command='speed',handle=h.id,speed=b or a});return h end
    h.loop=function(a,b) local v=b;if a~=h then v=a end;send({command='loop',handle=h.id,looping=v~=false});return h end
    return h
end
Animation.stop=function(h) return h.stop() end
Animation.pause=function(h) return h.pause() end
Animation.speed=function(h,v) return h.speed(v) end
Animation.loop=function(h,v) return h.loop(v) end
function Animation.blend(o,from,to,opts) opts=opts or {};opts.blendIn=opts.duration or 0.3;opts.sourceClip=type(from)=='table' and from or rustic.query({op='animation_ref',entity=entity(o),name=from});return Animation.play(o,to,opts) end
Animation.transition=Animation.blend

Audio={}
-- Audio objects retain one decoded clip and one current playback per load.
function __rustic_audio_object(source)
    local object = {}
    local loaded
    function object:load()
        if not loaded then loaded=rustic.query({op='audio_load',source=source}) end
        return object
    end
    local function requireLoaded()
        assert(loaded, 'audio is not loaded; call :load() before :play()')
        return loaded
    end
    function object:play(opts)
        local query={op='audio_loaded_play',entity=requireLoaded()}
        for k,v in pairs(opts or {}) do
            if k=='volume' or k=='pitch' or k=='loop' or k=='position' then query[k]=v end
        end
        local id=rustic.query(query)
        return {id=id,stop=function() rustic.query({op='audio_stop',entity=id}) end,pause=function() rustic.query({op='audio_pause',entity=id}) end,resume=function() rustic.query({op='audio_resume',entity=id}) end}
    end
    function object:stop() rustic.query({op='audio_loaded_stop',entity=requireLoaded()}) end
    function object:pause() rustic.query({op='audio_loaded_pause',entity=requireLoaded()}) end
    function object:resume() rustic.query({op='audio_loaded_resume',entity=requireLoaded()}) end
    function object:Unload()
        if loaded then rustic.query({op='audio_unload',entity=loaded});loaded=nil end
    end
    object.unload=object.Unload
    return object
end
function Audio.get_output_devices() return rustic.query({op='audio_get_output_devices'}) end
function Audio.get_input_devices() return rustic.query({op='audio_get_input_devices'}) end
function Audio.set_output_device(device) return rustic.query({op='audio_set_output_device',device=device}) end
function Audio.set_input_device(device) return rustic.query({op='audio_set_input_device',device=device}) end
function Audio.load(source) return __rustic_audio_object(source):load() end
function Audio.play(source,opts)
    opts=opts or {};opts.op='audio_play';opts.source=source;local id=rustic.query(opts)
    return {id=id,stop=function() rustic.query({op='audio_stop',entity=id}) end,pause=function() rustic.query({op='audio_pause',entity=id}) end,resume=function() rustic.query({op='audio_resume',entity=id}) end}
end
function Audio.playAt(source,position,opts) opts=opts or {};opts.position=position;return Audio.play(source,opts) end
function Audio.volume(voice,value) return rustic.query({op='audio_volume',entity=entity(voice),value=value}) end
function Audio.pitch(voice,value) return rustic.query({op='audio_pitch',entity=entity(voice),value=value}) end
function Audio.fadeIn(voice,duration,easing,volume) return start(tween(voice,'Volume',volume or 1,duration,easing,0)) end
function Audio.fadeOut(voice,duration,easing) return Tween.to(voice,'Volume',0,duration,easing) end
function Audio.crossfade(from,to,duration,easing) return start({kind='parallel',actions={tween(from,'Volume',0,duration,easing),tween(to,'Volume',1,duration,easing,0)}}) end

function Animation.ik(root,middle,tip,target,opts)
    opts=opts or {};return start(rustic.query({op='animation_ik',root=entity(root),middle=entity(middle),tip=entity(tip),target=target,pole=opts.pole,duration=opts.duration or 0,weight=opts.weight or 1,easing=opts.easing or Ease.Linear}))
end
Animation.footPlacement=Animation.ik
Animation.lookAt=Movement.lookAt
Animation.headTracking=Movement.lookAt
function Animation.recoil(o,rotation,duration,easing) return start(tween(o,'Rotation',rotation,duration,easing),{repeats=1,pingPong=true}) end

Clock={}
function Clock.timeScale(scale) return rustic.query({op="clock",scale=scale}).scale end
function Clock.pause() rustic.query({op="clock",paused=true}) end
function Clock.resume() rustic.query({op="clock",paused=false}) end
function Clock.state() return rustic.query({op="clock"}) end
function Camera.current() return rustic.query({op="camera_current"}) end
local cameraMove,cameraZoom=Camera.moveTo,Camera.zoom
function Camera.moveTo(a,b,c,d) if type(a)=="table" and not a.id and not a.__source then return cameraMove(Camera.current(),a,b,c) end return cameraMove(a,b,c,d) end
function Camera.zoom(a,b,c,d) if type(a)=="number" then return cameraZoom(Camera.current(),a,b,c) end return cameraZoom(a,b,c,d) end
Camera.fov=Camera.zoom

function Animation.value(keys,fn) return start(rustic.query({op="keyframes",keys=keys,token=callback(fn,true)})) end

function Animation.addMarker(o,clip,time,name) return rustic.query({op="animation_marker",entity=entity(o),clip=clip,time=time,name=name}) end

-- Capture handles retain recorded PCM until unload or owner cleanup.
local function captureHandle(id, recording)
    local handle={id=id}
    function handle:read() return rustic.query({op='capture_read',id=id}) end
    function handle:unload() rustic.query({op='capture_unload',id=id}) end
    local function stop()
        rustic.query({op=recording and 'capture_recording_stop' or 'capture_mic_stop',id=id})
        return handle
    end
    handle.stop=stop
    if recording then
        handle.stopRecording=stop; handle.stop_recording=stop
    else
        handle.stopHotMic=stop; handle.stop_hot_mic=stop
    end
    return handle
end
function rustic.game:startRecording(path, options)
    assert(path==nil or type(path)=='string','recording path must be a string or nil')
    options=options or {}
    assert(type(options)=='table','recording options must be a table')
    return captureHandle(rustic.query({op='capture_recording_start',path=path,source=options.source or 'game'}),true)
end
function rustic.game:stopRecording(recording)
    rustic.query({op='capture_recording_stop',id=recording and recording.id})
    return recording
end
function rustic.game:startHotMic()
    return captureHandle(rustic.query({op='capture_mic_start'}),false)
end
function rustic.game:stopHotMic(mic)
    rustic.query({op='capture_mic_stop',id=mic and mic.id})
    return mic
end
rustic.game.start_recording=rustic.game.startRecording
rustic.game.stop_recording=rustic.game.stopRecording
rustic.game.start_hot_mic=rustic.game.startHotMic
rustic.game.stop_hot_mic=rustic.game.stopHotMic

-- Entity services use the live shared Rust world, never cached scene paths.
local resolve_entity=entity
Tags={}
function Tags.add(entity,name) return rustic.query({op="tags_add",entity=resolve_entity(entity),name=name}) end
function Tags.remove(entity,name) return rustic.query({op="tags_remove",entity=resolve_entity(entity),name=name}) end
function Tags.find_all(name) return rustic.query({op="tags_find_all",name=name}) end
Groups={}
function Groups.create(name) return rustic.query({op="groups_create",name=name}) end
function Groups.add(name,entity) return rustic.query({op="groups_add",name=name,entity=resolve_entity(entity)}) end
function Groups.get_members(name) return rustic.query({op="groups_get_members",name=name}) end
Hierarchy={}
function Hierarchy.set_parent(entity,parent,keep_world) return rustic.query({op="hierarchy_set_parent",entity=resolve_entity(entity),parent=(parent~=nil and resolve_entity(parent) or nil),keep_world=(keep_world==nil or keep_world)}) end
function Hierarchy.get_children(entity) return rustic.query({op="hierarchy_get_children",entity=resolve_entity(entity)}) end
function Hierarchy.detach(entity,keep_world) return rustic.query({op="hierarchy_detach",entity=resolve_entity(entity),keep_world=(keep_world==nil or keep_world)}) end
Entity={}
function Entity.destroy(entity) return rustic.query({op="entity_destroy",entity=resolve_entity(entity)}) end
function Entity.destroy_after(entity,delay) return rustic.query({op="entity_destroy_after",entity=resolve_entity(entity),delay=delay}) end
function Entity.is_valid(entity) return rustic.query({op="entity_is_valid",entity=resolve_entity(entity)}) end
Components={}
function Components.has(entity,name) return rustic.query({op="components_has",entity=resolve_entity(entity),name=name}) end
function Components.get_all(entity) return rustic.query({op="components_get_all",entity=resolve_entity(entity)}) end
function Components.find_in_children(entity,name) return rustic.query({op="components_find_in_children",entity=resolve_entity(entity),name=name}) end
World={}
function World.find_by_tag(name) return rustic.query({op="world_find_by_tag",name=name}) end
function World.find_in_radius(center,radius) return rustic.query({op="world_find_in_radius",center=center,radius=radius}) end

-- Canonical API paths.
local sceneMethods={
    ['tweenTo']={Tween.to,0},
    ['tweenMove']={Tween.move,0},
    ['tweenRotate']={Tween.rotate,0},
    ['tweenScale']={Tween.scale,0},
    ['tweenAction']={Tween.action,0},
    ['movementMoveTo']={Movement.moveTo,0},
    ['movementMove']={Movement.move,0},
    ['movementRotateTo']={Movement.rotateTo,0},
    ['movementLookAt']={Movement.lookAt,0},
    ['movementFollow']={Movement.follow,0},
    ['movementOrbit']={Movement.orbit,0},
    ['pathFollow']={Path.follow,0},
    ['effectsFade']={Effects.fade,0},
    ['effectsFlash']={Effects.flash,0},
    ['effectsShake']={Effects.shake,0},
    ['effectsPulse']={Effects.pulse,0},
    ['cameraMoveTo']={Camera.moveTo,0},
    ['cameraZoom']={Camera.zoom,0},
    ['cameraFov']={Camera.fov,0},
    ['cameraFollow']={Camera.follow,0},
    ['cameraOrbit']={Camera.orbit,0},
    ['cameraLookAt']={Camera.lookAt,0},
    ['cameraShake']={Camera.shake,0},
    ['cameraTransition']={Camera.transition,0},
    ['physicsImpulse']={Physics.impulse,0},
    ['physicsForce']={Physics.force,0},
    ['physicsLaunch']={Physics.launch,0},
    ['physicsKnockback']={Physics.knockback,0},
    ['animationRegister']={Animation.register,0},
    ['animationClips']={Animation.clips,0},
    ['animationAction']={Animation.action,0},
    ['animationPlay']={Animation.play,0},
    ['animationBlend']={Animation.blend,0},
    ['animationTransition']={Animation.transition,0},
    ['animationIk']={Animation.ik,0},
    ['animationFootPlacement']={Animation.footPlacement,0},
    ['animationLookAt']={Animation.lookAt,0},
    ['animationHeadTracking']={Animation.headTracking,0},
    ['animationRecoil']={Animation.recoil,0},
    ['animationAddMarker']={Animation.addMarker,0},
    ['eventsConnect']={Events.connect,0},
    ['tagsAdd']={Tags.add,0},
    ['tagsRemove']={Tags.remove,0},
    ['groupsAdd']={Groups.add,1},
    ['hierarchySetParent']={Hierarchy.set_parent,0},
    ['hierarchyGetChildren']={Hierarchy.get_children,0},
    ['hierarchyDetach']={Hierarchy.detach,0},
    ['entityDestroy']={Entity.destroy,0},
    ['entityDestroyAfter']={Entity.destroy_after,0},
    ['entityIsValid']={Entity.is_valid,0},
    ['componentsHas']={Components.has,0},
    ['componentsGetAll']={Components.get_all,0},
    ['componentsFindInChildren']={Components.find_in_children,0},
    ['tween']={Tween.to,0},
    ['moveTo']={Movement.moveTo,0},
    ['move']={Movement.move,0},
    ['rotateTo']={Movement.rotateTo,0},
    ['lookAt']={Movement.lookAt,0},
    ['follow']={Movement.follow,0},
    ['orbit']={Movement.orbit,0},
    ['fade']={Effects.fade,0},
    ['flash']={Effects.flash,0},
    ['shake']={Effects.shake,0},
    ['pulse']={Effects.pulse,0},
    ['impulse']={Physics.impulse,0},
    ['force']={Physics.force,0},
    ['launch']={Physics.launch,0},
    ['knockback']={Physics.knockback,0},
    ['playAnimation']={Animation.play,0},
    ['destroy']={Entity.destroy,0},
    ['destroyAfter']={Entity.destroy_after,0},
    ['isValid']={Entity.is_valid,0},
    ['addTag']={Tags.add,0},
    ['removeTag']={Tags.remove,0},
    ['setParent']={Hierarchy.set_parent,0},
    ['getChildren']={Hierarchy.get_children,0},
    ['detach']={Hierarchy.detach,0},
    ['hasComponent']={Components.has,0},
    ['getComponents']={Components.get_all,0},
    ['findInChildren']={Components.find_in_children,0},
    ['followPath']={Path.follow,0},
}
sceneMethods.clone={instance.clone,0}
sceneMethods.add={instance.add,0}
sceneMethods.setCurrentCamera={Game.setCurrentCamera,0}
sceneMethods.emit={Events.emit,2}
local globalMethods={
    ['tweenValue']={Tween.value,0},
    ['timerAfter']={Timer.after,0},
    ['timerEvery']={Timer.every,0},
    ['smoothLerp']={Smooth.lerp,0},
    ['smoothSlerp']={Smooth.slerp,0},
    ['smoothInverseLerp']={Smooth.inverseLerp,0},
    ['smoothRemap']={Smooth.remap,0},
    ['smoothSmoothDamp']={Smooth.smoothDamp,0},
    ['physicsRaycast']={Physics.raycast,0},
    ['physicsSphereCast']={Physics.sphereCast,0},
    ['physicsOverlap']={Physics.overlap,0},
    ['physicsExplosion']={Physics.explosion,0},
    ['clockTimeScale']={Clock.timeScale,0},
    ['clockPause']={Clock.pause,0},
    ['clockResume']={Clock.resume,0},
    ['clockState']={Clock.state,0},
    ['pathCreate']={Path.create,0},
    ['sequenceNew']={Sequence.new,0},
    ['timelineNew']={Timeline.new,0},
    ['eventsOn']={Events.on,0},
    ['eventsOnce']={Events.once,0},
    ['eventsEmit']={Events.emit,0},
    ['eventsDisconnect']={Events.disconnect,0},
    ['audioGetOutputDevices']={Audio.get_output_devices,0},
    ['audioGetInputDevices']={Audio.get_input_devices,0},
    ['audioSetOutputDevice']={Audio.set_output_device,0},
    ['audioSetInputDevice']={Audio.set_input_device,0},
    ['audioVolume']={Audio.volume,0},
    ['audioPitch']={Audio.pitch,0},
    ['audioFadeIn']={Audio.fadeIn,0},
    ['audioFadeOut']={Audio.fadeOut,0},
    ['audioCrossfade']={Audio.crossfade,0},
    ['animationValue']={Animation.value,0},
    ['animationStop']={Animation.stop,0},
    ['animationPause']={Animation.pause,0},
    ['animationSpeed']={Animation.speed,0},
    ['animationLoop']={Animation.loop,0},
    ['cameraCurrent']={Camera.current,0},
    ['tagsFindAll']={Tags.find_all,0},
    ['groupsCreate']={Groups.create,0},
    ['groupsGetMembers']={Groups.get_members,0},
    ['worldFindByTag']={World.find_by_tag,0},
    ['worldFindInRadius']={World.find_in_radius,0},
    ['after']={Timer.after,0},
    ['every']={Timer.every,0},
    ['lerp']={Smooth.lerp,0},
    ['slerp']={Smooth.slerp,0},
    ['inverseLerp']={Smooth.inverseLerp,0},
    ['remap']={Smooth.remap,0},
    ['smoothDamp']={Smooth.smoothDamp,0},
    ['raycast']={Physics.raycast,0},
    ['sphereCast']={Physics.sphereCast,0},
    ['overlap']={Physics.overlap,0},
    ['explosion']={Physics.explosion,0},
    ['timeScale']={Clock.timeScale,0},
    ['pause']={Clock.pause,0},
    ['resume']={Clock.resume,0},
    ['createPath']={Path.create,0},
    ['sequence']={Sequence.new,0},
    ['timeline']={Timeline.new,0},
    ['on']={Events.on,0},
    ['once']={Events.once,0},
    ['emit']={Events.emit,0},
    ['disconnect']={Events.disconnect,0},
    ['currentCamera']={Camera.current,0},
    ['getOutputDevices']={Audio.get_output_devices,0},
    ['getInputDevices']={Audio.get_input_devices,0},
    ['setOutputDevice']={Audio.set_output_device,0},
    ['setInputDevice']={Audio.set_input_device,0},
}
local fileMethods={
    ['animationLoad']={Animation.load,1},
    ['loadAnimation']={Animation.load,1},
    ['add']={instance.add,0},
    ['clone']={instance.clone,0},
    ['setSkyTexture']={Game.scene.setSkyTexture,0},
}
local function argument(value)
    if type(value)=='table' and rawget(value,'__source') and string.sub(value.__source,1,18)=='game.rustic.files.' then
        return rawget(value,'__file_path')
    end
    return value
end
function __rustic_path_method(proxy,name)
    local source=rawget(proxy,'__source')
    local methods
    local target=proxy
    if string.sub(source,1,11)=='game.scene.' then methods=sceneMethods
    elseif string.sub(source,1,18)=='game.rustic.files.' then
        methods=fileMethods;target=rawget(proxy,'__file_path')
    else return nil end
    if methods==fileMethods and (name=='startRecording' or name=='start_recording') then
        local method=function(_,opts) return rustic.game:startRecording(target,opts) end
        rawset(proxy,name,method);return method
    end
    local binding=methods[name]
    if not binding then return nil end
    local method=function(self,...)
        assert(self==proxy,'call path APIs with :')
        local args=table.pack(...)
        for i=1,args.n do args[i]=argument(args[i]) end
        args.n=math.max(args.n,binding[2])
        for i=args.n+1,binding[2]+2,-1 do args[i]=args[i-1] end
        args[binding[2]+1]=target;args.n=args.n+1
        return binding[1](table.unpack(args,1,args.n))
    end
    rawset(proxy,name,method)
    return method
end
game={scene=__rustic_object_path('game.scene'),rustic={files=__rustic_object_path('game.rustic.files')}}
globalMethods.get_output_devices=globalMethods.getOutputDevices
globalMethods.get_input_devices=globalMethods.getInputDevices
globalMethods.set_output_device=globalMethods.setOutputDevice
globalMethods.set_input_device=globalMethods.setInputDevice
for name,binding in pairs(globalMethods) do
    game.rustic[name]=function(_,...) return binding[1](...) end
end
for name,fn in pairs(rustic) do
    if type(fn)=='function' then game.rustic[name]=function(_,...) return fn(...) end end
end
for _,name in ipairs({'getEnvironment','setEnvironment'}) do
    game.rustic[name]=function(_,...) return Game.scene[name](...) end
end
function game.rustic:setCurrentCamera(object)
    return Game.setCurrentCamera(entity(object))
end
for _,name in ipairs({'startRecording','stopRecording','startHotMic','stopHotMic','start_recording','stop_recording','start_hot_mic','stop_hot_mic'}) do
    game.rustic[name]=function(_,...) return rustic.game[name](rustic.game,...) end
end
