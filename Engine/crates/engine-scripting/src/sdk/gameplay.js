// Thin bindings. The engine schedules every action and samples every curve.
(() => {
  const callbacks = new Map(), handles = new Map(), connections=new Set();
  let serial = 0;
  const prefix = Math.random().toString(36).slice(2);
  const token = () => prefix + '-' + (++serial);
  const callback = (fn, persistent=false) => {
    if (typeof fn !== 'function') throw new TypeError('expected a callback');
    const id=token(); callbacks.set(id,{fn,persistent}); return id;
  };
  const entity = object => {
    if (object == null) return rustic.entity_id();
    if (typeof object === 'object') object=object.__source||object.id||object.source;
    if (typeof object !== 'string') throw new TypeError('expected entity ID or scene path');
    if(object.startsWith('game.scene.'))return rustic.query({op:'scene_object',source:object});
    const id = Game.scene.Find(object) || (/^[0-9a-f]{8}-/i.test(object) ? object : null);
    if (!id) throw new Error('object was not found: '+object); return id;
  };
  const options = (timing=0,easing='Linear') => {
    if (typeof timing !== 'object') return {duration:timing,easing};
    if (timing.duration != null && timing.speed != null) throw new Error('choose duration or speed');
    if (timing.duration == null && timing.speed == null) throw new Error('duration or speed is required');
    return {...timing,easing:timing.easing || easing};
  };
  const tween = (object,property,to,timing,easing,from) => ({kind:'tween',target:{entity:entity(object),property},to,from,...options(timing,easing)});
  const actionTokens = (action,result=[]) => {
    if (action.kind==='callback'||action.kind==='value') result.push(action.token);
    if(action.marker_token)result.push(action.marker_token);
    for (const child of action.actions || []) actionTokens(child,result);
    return result;
  };
  const start = (action,opts={}) => {
    const id=token(), owned=actionTokens(action);
    const cleanup=()=>{owned.forEach(t=>callbacks.delete(t));handles.delete(id);};
    const h={id, onFinished(fn){if(typeof fn!=='function')throw new TypeError('expected callback');h.finished=fn;return h;},state(){return rustic.gameplay_states().find(s=>s.handle===id);}};
    const done=callback(()=>{cleanup();if(h.finished)h.finished();});
    handles.set(id,{done,cleanup});
    for (const command of ['pause','resume','cancel','reverse']) h[command]=()=>{
      rustic.gameplay({command,handle:id});if(command==='cancel'){callbacks.delete(done);cleanup();}return h;
    };
    rustic.gameplay({command:'start',handle:id,action:JSON.parse(JSON.stringify(action)),playback:{repeats:opts.repeats || 0,looping:!!opts.loop,ping_pong:!!opts.pingPong},on_finished:done});
    return h;
  };
  const Ease={Linear:'Linear'};
  for(const family of ['Sine','Quad','Cubic','Quart','Quint','Expo','Circ','Back','Elastic','Bounce'])
    for(const direction of ['In','Out','InOut']) Ease[direction+family]=direction+family;
  const Tween={to:(object,property,to,timing,easing)=>start(tween(object,property,to,timing,easing))};
  Tween.value=(from,to,t,fn,e)=>start({kind:'value',from,to,...options(t,e),token:callback(fn,true)});
  Tween.move=(o,p,t,e)=>Tween.to(o,'Position',p,t,e);
  Tween.rotate=(o,p,t,e)=>Tween.to(o,'Rotation',p,t,e);
  Tween.scale=(o,p,t,e)=>Tween.to(o,'Scale',p,t,e);
  const Movement={moveTo:Tween.move,rotateTo:Tween.rotate,
    move:(object,offset,t,e)=>start({kind:'move',entity:entity(object),offset,...options(t,e)}),
    lookAt:(object,position,duration=0,easing='Linear')=>start({kind:'look_at',entity:entity(object),position,duration,easing}),
    follow:(object,target,duration,easing='Linear',offset=[0,0,0])=>start({kind:'follow',entity:entity(object),target:entity(target),duration,easing,offset}),
    orbit:(object,center,radius,turns,duration,easing='Linear')=>start({kind:'orbit',entity:entity(object),center,radius,turns,duration,easing})
  };
  const Path={create:(points,kind='Linear')=>({kind,points:points.map(p=>p.point?p:{point:p,easing:'Linear'})}),
    follow:(object,path,opts)=>start({kind:'path',entity:entity(object),path,...options(opts),orient_to_path:!!opts.orientToPath},opts)};
  const Timer={after:(delay,fn)=>start({kind:'sequence',actions:[{kind:'wait',duration:delay},{kind:'callback',token:callback(fn)}]}),
    every:(interval,fn,count)=>{
      if(!(interval>0))throw new Error('repeat interval must be positive');
      if(count!=null&&(!Number.isInteger(count)||count<1))throw new Error('count must be a positive integer');
      return start({kind:'sequence',actions:[{kind:'wait',duration:interval},{kind:'callback',token:callback(fn,true)}]},{loop:count==null,repeats:count==null?0:count-1});
    }};
  const Sequence={new:()=>{
    const b={actions:[],move(o,p,t,e){b.actions.push(tween(o,'Position',p,t,e));return b;},to(o,p,v,t,e){b.actions.push(tween(o,p,v,t,e));return b;},
      animation(o,name,opts){b.actions.push(animationAction(o,name,opts));return b;},
      wait(duration){b.actions.push({kind:'wait',duration});return b;},call(fn){b.actions.push({kind:'callback',token:callback(fn)});return b;},
      parallel(builders){b.actions.push({kind:'parallel',actions:builders.map(b=>({kind:'sequence',actions:b.actions}))});return b;},
      play(){if(!b.actions.length)throw new Error('sequence is empty');return start({kind:'sequence',actions:b.actions});}};
    return b;
  }};
  const Effects={fade:(o,v,t,e)=>Tween.to(o,'Opacity',v,t,e),flash:(o,color,duration,easing='Linear')=>start(tween(o,'Color',color,duration,easing),{repeats:1,pingPong:true})};
  Effects.pulse=(o,scale,duration,easing)=>start(tween(o,"Scale",scale,duration,easing),{repeats:1,pingPong:true});
  const Camera={moveTo:Tween.move,zoom:(c,f,t,e)=>Tween.to(c,'Fov',f,t,e),follow:Movement.follow,orbit:Movement.orbit};Camera.fov=Camera.zoom;Camera.lookAt=Movement.lookAt;
  Effects.shake=(object,strength,duration,easing='Linear')=>start({kind:'shake',entity:entity(object),strength,duration,easing});Camera.shake=Effects.shake;
  Camera.transition=(o,p,r,f,t,e)=>start({kind:"parallel",actions:[tween(o,"Position",p,t,e),tween(o,"Rotation",r,t,e),tween(o,"Fov",f,t,e)]});
  const connect=(name,fn,once=false,source=null)=>{
    const id=callback(fn,!once);rustic.gameplay({command:'connect',token:id,signal:{name,source:source==null?null:entity(source)},once});
    connections.add(id);return {disconnect(){connections.delete(id);callbacks.delete(id);rustic.gameplay({command:'disconnect',token:id});}};
  };
  const Events={on:(n,f)=>connect(n,f),once:(n,f)=>connect(n,f,true),connect:(o,n,f)=>connect(n,f,false,o),disconnect:c=>c.disconnect(),
    emit:(name,args=[],source=null)=>rustic.gameplay({command:'emit',signal:{name,source:source==null?null:entity(source)},arguments:args})};
  const Physics={
    raycast:(origin,direction,distance,ignore=[])=>rustic.query({op:'physics_raycast',origin,direction,distance,ignore}),
    sphereCast:(origin,direction,distance,radius,ignore=[])=>rustic.query({op:'physics_sphere_cast',origin,direction,distance,radius,ignore}),
    overlap:(center,radius,ignore=[])=>rustic.query({op:'physics_overlap',center,radius,ignore}),
    impulse:(o,vector)=>rustic.query({op:'physics_impulse',entity:entity(o),vector}),
    force:(o,vector,delta=rustic.fixed_delta_time())=>rustic.query({op:'physics_force',entity:entity(o),vector,delta}),
    launch:(o,vector)=>rustic.query({op:'physics_launch',entity:entity(o),vector}),
    explosion:(center,radius,strength,ignore=[])=>rustic.query({op:'physics_explosion',center,radius,strength,ignore})
  };Physics.knockback=Physics.impulse;globalThis.Physics=Physics;
  const animationAction=(o,name,opts={})=>({kind:'animation_ref',entity:entity(o),clip:typeof name==='object'?name:rustic.query({op:'animation_ref',entity:entity(o),name}),options:{speed:opts.speed||1,blend_in:opts.blendIn||0,blend_in_ease:opts.blendInEase||opts.easing||'Linear',progression_ease:opts.progressionEase,blend_out:opts.blendOut||0,blend_out_ease:opts.blendOutEase||opts.easing||"Linear",weight:opts.weight==null?1:opts.weight,additive:!!opts.additive,layered:opts.layered??(opts.mask?.length>0)},blend_source:opts.sourceClip,mask:opts.mask||[]});
  const Animation={addMarker:(o,clip,time,name)=>rustic.query({op:"animation_marker",entity:entity(o),clip,time,name}),value:(keys,fn)=>start(rustic.query({op:"keyframes",keys,token:callback(fn,true)})),load:(o,source)=>rustic.query({op:"animation_load",entity:entity(o),source}),register:(o,clip)=>rustic.query({op:'animation_register',entity:entity(o),clip}),clips:o=>rustic.query({op:'animation_list',entity:entity(o)}),
    play:(o,name,opts={})=>{const action=animationAction(o,name,opts),markers=new Map();action.marker_token=callback(name=>{const fn=markers.get(name);if(fn)fn();},true);const h=start(action,opts);
      h.onMarker=(name,fn)=>{if(typeof fn!=='function')throw new TypeError('expected callback');markers.set(name,fn);return h;};h.stop=h.cancel;
      h.speed=speed=>{rustic.gameplay({command:'speed',handle:h.id,speed});return h;};h.loop=(looping=true)=>{rustic.gameplay({command:'loop',handle:h.id,looping});return h;};return h;
    },stop:h=>h.cancel(),pause:h=>h.pause(),speed:(h,v)=>h.speed(v),loop:(h,v)=>h.loop(v),
    blend:(o,from,to,opts={})=>Animation.play(o,to,{...opts,blendIn:opts.duration||0.3,sourceClip:typeof from==="object"?from:rustic.query({op:"animation_ref",entity:entity(o),name:from})})};Animation.transition=Animation.blend;globalThis.Animation=Animation;
  Animation.ik=(root,middle,tip,target,opts={})=>start(rustic.query({op:'animation_ik',root:entity(root),middle:entity(middle),tip:entity(tip),target,pole:opts.pole,duration:opts.duration||0,weight:opts.weight==null?1:opts.weight,easing:opts.easing||'Linear'}));
  Animation.footPlacement=Animation.ik;Animation.lookAt=Movement.lookAt;Animation.headTracking=Movement.lookAt;Animation.recoil=(o,r,t,e)=>start(tween(o,'Rotation',r,t,e),{repeats:1,pingPong:true});
  const Audio={play:(source,opts={})=>{const id=rustic.query({...opts,op:'audio_play',source});return {id,stop:()=>rustic.query({op:'audio_stop',entity:id}),pause:()=>rustic.query({op:'audio_pause',entity:id}),resume:()=>rustic.query({op:'audio_resume',entity:id})};},
    get_output_devices:()=>rustic.query({op:'audio_get_output_devices'}),
    get_input_devices:()=>rustic.query({op:'audio_get_input_devices'}),
    set_output_device:(device=null)=>rustic.query({op:'audio_set_output_device',device}),
    set_input_device:(device=null)=>rustic.query({op:'audio_set_input_device',device}),
    playAt:(source,position,opts={})=>Audio.play(source,{...opts,position}),volume:(v,value)=>rustic.query({op:'audio_volume',entity:entity(v),value}),pitch:(v,value)=>rustic.query({op:'audio_pitch',entity:entity(v),value}),
    fadeIn:(v,t,e,volume=1)=>start(tween(v,'Volume',volume,t,e,0)),fadeOut:(v,t,e)=>Tween.to(v,'Volume',0,t,e),
    crossfade:(a,b,t,e)=>start({kind:'parallel',actions:[tween(a,'Volume',0,t,e),tween(b,'Volume',1,t,e,0)]})};globalThis.Audio=Audio;
  const dispatch=()=>{
    const live=new Set(rustic.gameplay_connections());for(const id of connections)if(!live.has(id)){callbacks.delete(id);connections.delete(id);}
    const existing=new Map(handles);
    for(const delivery of rustic.gameplay_callbacks()){
      const cb=callbacks.get(delivery.token);if(cb){if(!cb.persistent)callbacks.delete(delivery.token);cb.fn(...delivery.arguments);}
    }
    const states=rustic.gameplay_states();
    for(const state of states)if(state.status==='failed'||state.status==='cancelled'){
      const h=handles.get(state.handle);if(h){callbacks.delete(h.done);h.cleanup();if(state.error)rustic.log('error',state.error);}
    }
    const present=new Set(states.map(s=>s.handle));
    for(const [id,h] of existing)if(!present.has(id)){callbacks.delete(h.done);h.cleanup();}
  };
  Object.assign(globalThis,{Ease:Object.freeze(Ease),Tween,Movement,Path,Timer,Sequence,Timeline:Sequence,Effects,Camera,Events,__rustic_gameplay_dispatch:dispatch});
})();

globalThis.Smooth={
  lerp:(a,b,t,easing='Linear')=>rustic.query({op:'lerp',from:a,to:b,progress:t,easing}),
  slerp:(a,b,t,easing='Linear')=>rustic.query({op:'slerp',from:a,to:b,progress:t,easing}),
  inverseLerp:(a,b,value)=>rustic.query({op:'inverse_lerp',from:a,to:b,value}),
  remap:(value,in_min,in_max,out_min,out_max)=>rustic.query({op:'remap',value,in_min,in_max,out_min,out_max}),
  smoothDamp:(current,target,velocity,smooth_time,delta)=>rustic.query({op:'smooth_damp',current,target,velocity,smooth_time,delta})
};globalThis.Interpolation=Smooth;

globalThis.Clock={timeScale:scale=>rustic.query({op:"clock",scale}).scale,pause:()=>rustic.query({op:"clock",paused:true}),resume:()=>rustic.query({op:"clock",paused:false}),state:()=>rustic.query({op:"clock"})};
Camera.current=()=>rustic.query({op:"camera_current"});
{const move=Camera.moveTo,zoom=Camera.zoom;Camera.moveTo=(a,b,c,d)=>Array.isArray(a)?move(Camera.current(),a,b,c):move(a,b,c,d);Camera.zoom=(a,b,c,d)=>typeof a==="number"?zoom(Camera.current(),a,b,c):zoom(a,b,c,d);Camera.fov=Camera.zoom;}

const _rusticEntity = object => {
 if(object==null)return rustic.entity_id();
 if(typeof object==="object")object=object.__source||object.id||object.source;
 if(typeof object!=="string")throw new TypeError("expected entity ID or path");
 return object.startsWith('game.scene.')?rustic.query({op:'scene_object',source:object}):Game.scene.Find(object)||object;
};

// Entity services use the live shared Rust world.
globalThis.Tags={
add(entity,name){return rustic.query({op:"tags_add",entity:_rusticEntity(entity),name:name});},
remove(entity,name){return rustic.query({op:"tags_remove",entity:_rusticEntity(entity),name:name});},
find_all(name){return rustic.query({op:"tags_find_all",name:name});},
};
globalThis.Groups={
create(name){return rustic.query({op:"groups_create",name:name});},
add(name,entity){return rustic.query({op:"groups_add",name:name,entity:_rusticEntity(entity)});},
get_members(name){return rustic.query({op:"groups_get_members",name:name});},
};
globalThis.Hierarchy={
set_parent(entity,parent,keep_world=true){return rustic.query({op:"hierarchy_set_parent",entity:_rusticEntity(entity),parent:(parent==null?null:_rusticEntity(parent)),keep_world:keep_world});},
get_children(entity){return rustic.query({op:"hierarchy_get_children",entity:_rusticEntity(entity)});},
detach(entity,keep_world=true){return rustic.query({op:"hierarchy_detach",entity:_rusticEntity(entity),keep_world:keep_world});},
};
globalThis.Entity={
destroy(entity){return rustic.query({op:"entity_destroy",entity:_rusticEntity(entity)});},
destroy_after(entity,delay){return rustic.query({op:"entity_destroy_after",entity:_rusticEntity(entity),delay:delay});},
is_valid(entity){return rustic.query({op:"entity_is_valid",entity:_rusticEntity(entity)});},
};
globalThis.Components={
has(entity,name){return rustic.query({op:"components_has",entity:_rusticEntity(entity),name:name});},
get_all(entity){return rustic.query({op:"components_get_all",entity:_rusticEntity(entity)});},
find_in_children(entity,name){return rustic.query({op:"components_find_in_children",entity:_rusticEntity(entity),name:name});},
};
globalThis.World={
find_by_tag(name){return rustic.query({op:"world_find_by_tag",name:name});},
find_in_radius(center,radius){return rustic.query({op:"world_find_in_radius",center:center,radius:radius});},
};

// Canonical API paths.
(() => {
  const sceneMethods={
    tweenTo:[Tween.to,0],
    tweenMove:[Tween.move,0],
    tweenRotate:[Tween.rotate,0],
    tweenScale:[Tween.scale,0],
    tweenAction:[Tween.action,0],
    movementMoveTo:[Movement.moveTo,0],
    movementMove:[Movement.move,0],
    movementRotateTo:[Movement.rotateTo,0],
    movementLookAt:[Movement.lookAt,0],
    movementFollow:[Movement.follow,0],
    movementOrbit:[Movement.orbit,0],
    pathFollow:[Path.follow,0],
    effectsFade:[Effects.fade,0],
    effectsFlash:[Effects.flash,0],
    effectsShake:[Effects.shake,0],
    effectsPulse:[Effects.pulse,0],
    cameraMoveTo:[Camera.moveTo,0],
    cameraZoom:[Camera.zoom,0],
    cameraFov:[Camera.fov,0],
    cameraFollow:[Camera.follow,0],
    cameraOrbit:[Camera.orbit,0],
    cameraLookAt:[Camera.lookAt,0],
    cameraShake:[Camera.shake,0],
    cameraTransition:[Camera.transition,0],
    physicsImpulse:[Physics.impulse,0],
    physicsForce:[Physics.force,0],
    physicsLaunch:[Physics.launch,0],
    physicsKnockback:[Physics.knockback,0],
    animationRegister:[Animation.register,0],
    animationClips:[Animation.clips,0],
    animationAction:[Animation.action,0],
    animationPlay:[Animation.play,0],
    animationBlend:[Animation.blend,0],
    animationTransition:[Animation.transition,0],
    animationIk:[Animation.ik,0],
    animationFootPlacement:[Animation.footPlacement,0],
    animationLookAt:[Animation.lookAt,0],
    animationHeadTracking:[Animation.headTracking,0],
    animationRecoil:[Animation.recoil,0],
    animationAddMarker:[Animation.addMarker,0],
    eventsConnect:[Events.connect,0],
    tagsAdd:[Tags.add,0],
    tagsRemove:[Tags.remove,0],
    groupsAdd:[Groups.add,1],
    hierarchySetParent:[Hierarchy.set_parent,0],
    hierarchyGetChildren:[Hierarchy.get_children,0],
    hierarchyDetach:[Hierarchy.detach,0],
    entityDestroy:[Entity.destroy,0],
    entityDestroyAfter:[Entity.destroy_after,0],
    entityIsValid:[Entity.is_valid,0],
    componentsHas:[Components.has,0],
    componentsGetAll:[Components.get_all,0],
    componentsFindInChildren:[Components.find_in_children,0],
    tween:[Tween.to,0],
    moveTo:[Movement.moveTo,0],
    move:[Movement.move,0],
    rotateTo:[Movement.rotateTo,0],
    lookAt:[Movement.lookAt,0],
    follow:[Movement.follow,0],
    orbit:[Movement.orbit,0],
    fade:[Effects.fade,0],
    flash:[Effects.flash,0],
    shake:[Effects.shake,0],
    pulse:[Effects.pulse,0],
    impulse:[Physics.impulse,0],
    force:[Physics.force,0],
    launch:[Physics.launch,0],
    knockback:[Physics.knockback,0],
    playAnimation:[Animation.play,0],
    destroy:[Entity.destroy,0],
    destroyAfter:[Entity.destroy_after,0],
    isValid:[Entity.is_valid,0],
    addTag:[Tags.add,0],
    removeTag:[Tags.remove,0],
    setParent:[Hierarchy.set_parent,0],
    getChildren:[Hierarchy.get_children,0],
    detach:[Hierarchy.detach,0],
    hasComponent:[Components.has,0],
    getComponents:[Components.get_all,0],
    findInChildren:[Components.find_in_children,0],
    followPath:[Path.follow,0],
  };
  Object.assign(sceneMethods,{clone:[instance.clone,0],add:[instance.add,0],setCurrentCamera:[Game.setCurrentCamera,0],emit:[Events.emit,2]});
  const globalMethods={
    tweenValue:[Tween.value,0],
    timerAfter:[Timer.after,0],
    timerEvery:[Timer.every,0],
    smoothLerp:[Smooth.lerp,0],
    smoothSlerp:[Smooth.slerp,0],
    smoothInverseLerp:[Smooth.inverseLerp,0],
    smoothRemap:[Smooth.remap,0],
    smoothSmoothDamp:[Smooth.smoothDamp,0],
    physicsRaycast:[Physics.raycast,0],
    physicsSphereCast:[Physics.sphereCast,0],
    physicsOverlap:[Physics.overlap,0],
    physicsExplosion:[Physics.explosion,0],
    clockTimeScale:[Clock.timeScale,0],
    clockPause:[Clock.pause,0],
    clockResume:[Clock.resume,0],
    clockState:[Clock.state,0],
    pathCreate:[Path.create,0],
    sequenceNew:[Sequence.new,0],
    timelineNew:[Timeline.new,0],
    eventsOn:[Events.on,0],
    eventsOnce:[Events.once,0],
    eventsEmit:[Events.emit,0],
    eventsDisconnect:[Events.disconnect,0],
    audioGetOutputDevices:[Audio.get_output_devices,0],
    audioGetInputDevices:[Audio.get_input_devices,0],
    audioSetOutputDevice:[Audio.set_output_device,0],
    audioSetInputDevice:[Audio.set_input_device,0],
    audioVolume:[Audio.volume,0],
    audioPitch:[Audio.pitch,0],
    audioFadeIn:[Audio.fadeIn,0],
    audioFadeOut:[Audio.fadeOut,0],
    audioCrossfade:[Audio.crossfade,0],
    audioStop:[Audio.stop,0],
    audioPause:[Audio.pause,0],
    audioResume:[Audio.resume,0],
    animationValue:[Animation.value,0],
    animationStop:[Animation.stop,0],
    animationPause:[Animation.pause,0],
    animationSpeed:[Animation.speed,0],
    animationLoop:[Animation.loop,0],
    cameraCurrent:[Camera.current,0],
    tagsFindAll:[Tags.find_all,0],
    groupsCreate:[Groups.create,0],
    groupsGetMembers:[Groups.get_members,0],
    worldFindByTag:[World.find_by_tag,0],
    worldFindInRadius:[World.find_in_radius,0],
    after:[Timer.after,0],
    every:[Timer.every,0],
    lerp:[Smooth.lerp,0],
    slerp:[Smooth.slerp,0],
    inverseLerp:[Smooth.inverseLerp,0],
    remap:[Smooth.remap,0],
    smoothDamp:[Smooth.smoothDamp,0],
    raycast:[Physics.raycast,0],
    sphereCast:[Physics.sphereCast,0],
    overlap:[Physics.overlap,0],
    explosion:[Physics.explosion,0],
    timeScale:[Clock.timeScale,0],
    pause:[Clock.pause,0],
    resume:[Clock.resume,0],
    createPath:[Path.create,0],
    sequence:[Sequence.new,0],
    timeline:[Timeline.new,0],
    on:[Events.on,0],
    once:[Events.once,0],
    emit:[Events.emit,0],
    disconnect:[Events.disconnect,0],
    currentCamera:[Camera.current,0],
    getOutputDevices:[Audio.get_output_devices,0],
    getInputDevices:[Audio.get_input_devices,0],
    setOutputDevice:[Audio.set_output_device,0],
    setInputDevice:[Audio.set_input_device,0],
  };
  const fileMethods={
    audioPlay:[Audio.play,0],
    audioPlayAt:[Audio.playAt,0],
    audioLoad:[Audio.load,0],
    animationLoad:[Animation.load,1],
    play:[Audio.play,0],
    playAt:[Audio.playAt,0],
    load:[Audio.load,0],
    loadAnimation:[Animation.load,1],
    add:[instance.add,0],
    clone:[instance.clone,0],
    setSkyTexture:[Game.scene.setSkyTexture,0],
  };
  const argument=v=>v && typeof v==='object' && typeof v.__file_path==='string'?v.__file_path:v;
  const path=(source,filePath=null)=>{
    const cache=Object.create(null);
    return new Proxy(cache,{get:(cache,key)=>{
      if(key==='__source')return source;
      if(key==='__file_path')return filePath;
      if(typeof key!=='string')return undefined;
      if(Object.hasOwn(cache,key))return cache[key];
      if(key==='EditAttribute'||key==='edit_attribute')return cache[key]=(name,value)=>rustic.edit_object_attribute(source,name,value);
      const binding=(filePath===null?sceneMethods:fileMethods)[key];
      if(binding){
        if(typeof binding[0]!=='function')throw new TypeError('API unavailable: '+key);
        return cache[key]=(...args)=>{args=args.map(argument);while(args.length<binding[1])args.push(undefined);args.splice(binding[1],0,filePath===null?source:filePath);return binding[0](...args);};
      }
      return cache[key]=path(source+'.'+key,filePath===null?null:(filePath?filePath+'/':'')+key);
    }});
  };
  Object.assign(globalMethods,{get_output_devices:globalMethods.getOutputDevices,get_input_devices:globalMethods.getInputDevices,set_output_device:globalMethods.setOutputDevice,set_input_device:globalMethods.setInputDevice});
  const api={...rustic,files:path('game.rustic.files','')};
  for(const [name,[fn]] of Object.entries(globalMethods))if(typeof fn==='function')api[name]=(...args)=>fn(...args);
  api.getEnvironment=()=>Game.scene.getEnvironment();
  api.setEnvironment=settings=>Game.scene.setEnvironment(settings);
  api.setCurrentCamera=object=>Game.setCurrentCamera(object && object.__source || object);
  globalThis.game={scene:path('game.scene'),rustic:api};
})();
