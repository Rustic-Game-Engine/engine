-- Thin Lua binding: Rust owns all progress, curves, paths, and composition.
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
