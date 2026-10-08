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
