import json, sys

class InstanceApi:
    def __init__(self): self.commands = []
    def add(self, source, parent=None):
        self.commands.append({"op": "add_instance", "source": source, "parent": parent})
    def clone(self, source, parent=None):
        self.commands.append({"op": "clone_instance", "source": source, "parent": parent})

instance = InstanceApi()

class ObjectPath:
    def __init__(self, source="rustic.game"): self.source = source
    def __getattr__(self, name):
        if self.source=="rustic.game" and name in ("startRecording","stopRecording","startHotMic","stopHotMic"): return getattr(Game,name)
        return ObjectPath(self.source + "." + name)
    def __getitem__(self, name): return ObjectPath(self.source + "." + str(name))
    def GetAttribute(self,name): return rustic.query(dict(op="object_attribute",source=self.source,name=name))
    get_attribute=GetAttribute
    def EditAttribute(self, name, value):
        instance.commands.append({"op":"edit_attribute", "source":self.source, "name":name, "value":value})
    edit_attribute = EditAttribute

class RusticApi:
    game = ObjectPath()
    def __init__(self): self.state = {}; self.commands = instance.commands
    def query(self, request):
        commands=self.commands[:]; self.commands.clear()
        sys.stdout.write(json.dumps({"query":request,"commands":commands})+"\n"); sys.stdout.flush()
        response=json.loads(sys.stdin.readline())
        if "error" in response: raise RuntimeError(response["error"])
        return response.get("result")
    def gameplay(self, request): self.commands.append({"op":"gameplay", "request":request})
    def gameplay_connections(self): return self.state.get("gameplay_connections",[])
    def gameplay_states(self): return self.state.get("gameplay_states", [])
    def gameplay_callbacks(self): return self.state.get("gameplay_callbacks", [])
    def find_entity(self,source): return self.query(dict(op="find_entity",source=source))
    def edit_object_attribute(self,source,name,value): self.commands.append(dict(op="edit_attribute",source=source,name=name,value=value))
    def get_object_attribute(self,source,name): return self.query(dict(op="object_attribute",source=source,name=name))
    def entity_id(self): return self.state["entity_id"]
    def delta_time(self): return self.state["delta_time"]
    def fixed_delta_time(self): return self.state["fixed_delta_time"]
    def get_translation(self): return self.state["translation"][:]
    def set_translation(self, x, y, z): self.commands.append({"op":"set_translation","value":[x,y,z]})
    def get_property(self, name): return self.state["properties"].get(name)
    def set_property(self, name, value): self.commands.append({"op":"set_property","name":name,"value":value})
    def get_attribute(self, name): return self.state["attributes"].get(name)
    GetAttribute = get_attribute
    def edit_attribute(self, name, value): self.commands.append({"op":"edit_attribute","name":name,"value":value})
    EditAttribute = edit_attribute
    def input(self, name): return self.state.get("actions", {}).get(name, {"pressed":False,"released":False,"held":False,"axis":0})
    def key(self, name): return self.state["keys"].get(name, {"pressed":False,"released":False,"held":False,"axis":0})
    def key_events(self): return self.state["key_events"][:]
    def any_key_pressed(self): return self.state["any_key_pressed"]
    def print(self,*values): self.log("info","\t".join(map(str,values)))
    def warn(self,*values): self.log("warn","\t".join(map(str,values)))
    def log(self, level, message): self.commands.append({"op":"log","level":str(level),"message":str(message)})
    def set_enabled(self, enabled): self.commands.append({"op":"set_enabled","enabled":bool(enabled)})

class SceneApi:
    def load(self,scene): return rustic.query({"op":"scene_load","scene":scene})
    def unload(self,scene): return rustic.query({"op":"scene_unload","scene":scene})
    def switchTo(self,scene): return rustic.query({"op":"scene_switch","scene":scene})
    def isLoaded(self,scene): return rustic.query({"op":"scene_is_loaded","scene":scene})
    def getLoaded(self): return rustic.query({"op":"scene_loaded"})
    def getActive(self): return rustic.query({"op":"scene_active"})
    def getEnvironment(self,field=None): return rustic.query({"op":"scene_environment"} if field is None else dict(op="scene_environment_field",field=field))
    def setEnvironment(self, settings): return rustic.query({"op":"scene_environment", "settings":settings})
    def setSkyTexture(self, path): return self.setEnvironment({"sky_image":path})
    def __init__(self): self.state = {}
    def Find(self, path): return self.state.get(path)
    def List(self, path="Game.scene"):
        prefix = (path or "Game.scene").rstrip("./") + "."
        return [entity for name, entity in self.state.items() if name.startswith(prefix)]

class GameApi:
    def setCurrentCamera(self, source):
        instance.commands.append({"op":"set_current_camera","source":source})
    set_current_camera = setCurrentCamera
rustic = RusticApi()
Game = GameApi()
Game.scene = SceneApi()

def run(callbacks):
    for line in sys.stdin:
        rustic.state = json.loads(line)
        instance.commands = []
        rustic.commands = instance.commands
        Game.scene.state = rustic.state.get("scene_paths", {})
        if rustic.state["callback"] == "update": _gameplay_dispatch()
        callback = callbacks.get(rustic.state["callback"])
        if callback:
            if rustic.state["callback"] in ("update", "fixed_update"): callback(rustic.state["delta"] or 0.0)
            else: callback()
        print(json.dumps({"format_version": 1, "commands": instance.commands}, allow_nan=False), flush=True)


# All time-based behavior is scheduled in the Rust runtime.
import uuid, copy
_gameplay_callbacks = {}
_gameplay_handles = {}
_gameplay_connections=set()
class Ease:
    Linear = "Linear"
for _family in ("Sine","Quad","Cubic","Quart","Quint","Expo","Circ","Back","Elastic","Bounce"):
    for _direction in ("In","Out","InOut"):
        setattr(Ease, _direction+_family, _direction+_family)
def _token(): return str(uuid.uuid4())
def _entity(obj):
    if obj is None: return rustic.entity_id()
    if isinstance(obj,ObjectPath): obj=obj.source
    elif hasattr(obj,"id"): obj=obj.id
    if str(obj).startswith("game.scene."): return rustic.query({"op":"scene_object","source":str(obj)})
    result = Game.scene.Find(str(obj))
    if result: return result
    uuid.UUID(str(obj))
    return str(obj)
def _callback(fn, persistent=False):
    if not callable(fn): raise TypeError("expected a callback")
    token = _token(); _gameplay_callbacks[token] = (fn,persistent); return token
def _tween(obj, property, to, duration, easing):
    timing = dict(duration) if isinstance(duration,dict) else {"duration":duration}
    if "speed" in timing and "duration" in timing: raise ValueError("choose duration or speed")
    return {"kind":"tween", "target":{"entity":_entity(obj),"property":property}, "to":to, "easing":easing, **timing}
class GameplayHandle:
    def __init__(self, action, loop=False, repeats=0, ping_pong=False):
        self.id = _token(); self.finished = None; self.tokens = []
        def collect(a):
            if a["kind"] in ("callback","value"): self.tokens.append(a["token"])
            if a.get("marker_token"): self.tokens.append(a["marker_token"])
            for child in a.get("actions",[]): collect(child)
        collect(action)
        self.done = _callback(self._complete); _gameplay_handles[self.id] = self
        rustic.gameplay({"command":"start","handle":self.id,"action":copy.deepcopy(action),"playback":{"looping":loop,"repeats":repeats,"ping_pong":ping_pong},"on_finished":self.done})
    def _cleanup(self):
        for token in [self.done]+self.tokens: _gameplay_callbacks.pop(token,None)
        _gameplay_handles.pop(self.id,None)
    def _complete(self):
        self._cleanup()
        if self.finished: self.finished()
    def onFinished(self, fn):
        if not callable(fn): raise TypeError("expected callback")
        self.finished=fn; return self
    def _control(self, command):
        rustic.gameplay({"command":command,"handle":self.id})
        if command=="cancel": self._cleanup()
        return self
    def pause(self): return self._control("pause")
    def resume(self): return self._control("resume")
    def cancel(self): return self._control("cancel")
    def reverse(self): return self._control("reverse")
    def state(self): return next((s for s in rustic.gameplay_states() if s["handle"]==self.id),None)
class Tween:
    @staticmethod
    def value(from_value,to,duration,fn,easing=Ease.Linear): return GameplayHandle({"kind":"value","from":from_value,"to":to,"duration":duration,"easing":easing,"token":_callback(fn,True)})
    @staticmethod
    def to(obj, property, to, duration, easing=Ease.Linear): return GameplayHandle(_tween(obj,property,to,duration,easing))
    @staticmethod
    def move(obj, to, duration, easing=Ease.Linear): return Tween.to(obj,"Position",to,duration,easing)
    @staticmethod
    def rotate(obj, to, duration, easing=Ease.Linear): return Tween.to(obj,"Rotation",to,duration,easing)
    @staticmethod
    def scale(obj, to, duration, easing=Ease.Linear): return Tween.to(obj,"Scale",to,duration,easing)
class Movement:
    moveTo = staticmethod(Tween.move)
    rotateTo = staticmethod(Tween.rotate)
    @staticmethod
    def move(obj,offset,duration,easing=Ease.Linear):
        timing=dict(duration) if isinstance(duration,dict) else {"duration":duration}
        return GameplayHandle({"kind":"move","entity":_entity(obj),"offset":offset,"easing":easing,**timing})
    @staticmethod
    def lookAt(obj,position,duration=0,easing=Ease.Linear):
        return GameplayHandle({"kind":"look_at","entity":_entity(obj),"position":position,"duration":duration,"easing":easing})
    @staticmethod
    def follow(obj,target,duration,easing=Ease.Linear,offset=(0,0,0)):
        return GameplayHandle({"kind":"follow","entity":_entity(obj),"target":_entity(target),"duration":duration,"easing":easing,"offset":offset})
    @staticmethod
    def orbit(obj,center,radius,turns,duration,easing=Ease.Linear):
        return GameplayHandle({"kind":"orbit","entity":_entity(obj),"center":center,"radius":radius,"turns":turns,"duration":duration,"easing":easing})
class Timer:
    @staticmethod
    def after(delay,fn): return GameplayHandle({"kind":"sequence","actions":[{"kind":"wait","duration":delay},{"kind":"callback","token":_callback(fn)}]})
    @staticmethod
    def every(interval,fn,count=None):
        if interval<=0 or (count is not None and (not isinstance(count,int) or count<1)): raise ValueError("positive interval/count required")
        return GameplayHandle({"kind":"sequence","actions":[{"kind":"wait","duration":interval},{"kind":"callback","token":_callback(fn,True)}]},loop=count is None,repeats=0 if count is None else count-1)
class Sequence:
    def __init__(self): self.actions=[]
    @staticmethod
    def new(): return Sequence()
    def move(self,obj,to,duration,easing=Ease.Linear): self.actions.append(_tween(obj,"Position",to,duration,easing)); return self
    def to(self,obj,property,to,duration,easing=Ease.Linear): self.actions.append(_tween(obj,property,to,duration,easing)); return self
    def wait(self,duration): self.actions.append({"kind":"wait","duration":duration}); return self
    def call(self,fn): self.actions.append({"kind":"callback","token":_callback(fn)}); return self
    def parallel(self,sequences): self.actions.append({"kind":"parallel","actions":[{"kind":"sequence","actions":s.actions} for s in sequences]}); return self
    def play(self):
        if not self.actions: raise ValueError("sequence is empty")
        return GameplayHandle({"kind":"sequence","actions":self.actions})
Timeline = Sequence
class Path:
    @staticmethod
    def create(points,kind="Linear"): return {"kind":kind,"points":[p if isinstance(p,dict) else {"point":p,"easing":Ease.Linear} for p in points]}
    @staticmethod
    def follow(obj,path,options):
        timing={k:options[k] for k in ("duration","speed") if k in options}
        if len(timing)!=1: raise ValueError("choose duration or speed")
        return GameplayHandle({"kind":"path","entity":_entity(obj),"path":path,"easing":options.get("easing",Ease.Linear),"orient_to_path":options.get("orientToPath",False),**timing},loop=options.get("loop",False),ping_pong=options.get("pingPong",False))
class Effects:
    @staticmethod
    def fade(obj,opacity,duration,easing=Ease.Linear): return Tween.to(obj,"Opacity",opacity,duration,easing)
    @staticmethod
    def flash(obj,color,duration,easing=Ease.Linear): return GameplayHandle(_tween(obj,"Color",color,duration,easing),repeats=1,ping_pong=True)
    @staticmethod
    def shake(obj,strength,duration,easing=Ease.Linear): return GameplayHandle({"kind":"shake","entity":_entity(obj),"strength":strength,"duration":duration,"easing":easing})
class Camera:
    shake=staticmethod(Effects.shake)
    lookAt=staticmethod(Movement.lookAt)
    moveTo=staticmethod(Tween.move)
    follow=staticmethod(Movement.follow)
    orbit=staticmethod(Movement.orbit)
    @staticmethod
    def zoom(camera,fov,duration,easing=Ease.Linear): return Tween.to(camera,"Fov",fov,duration,easing)
    fov=zoom
class Connection:
    def __init__(self,name,fn,once=False,source=None):
        self.token=_callback(fn,not once)
        _gameplay_connections.add(self.token)
        rustic.gameplay({"command":"connect","token":self.token,"signal":{"name":name,"source":None if source is None else _entity(source)},"once":once})
    def disconnect(self):
        _gameplay_connections.discard(self.token)
        _gameplay_callbacks.pop(self.token,None);rustic.gameplay({"command":"disconnect","token":self.token})
class Events:
    @staticmethod
    def on(name,fn): return Connection(name,fn)
    @staticmethod
    def once(name,fn): return Connection(name,fn,True)
    @staticmethod
    def connect(obj,name,fn): return Connection(name,fn,source=obj)
    @staticmethod
    def emit(name,arguments=(),source=None): rustic.gameplay({"command":"emit","signal":{"name":name,"source":None if source is None else _entity(source)},"arguments":arguments})
    @staticmethod
    def disconnect(connection): connection.disconnect()
def _gameplay_dispatch():
    live=set(rustic.gameplay_connections())
    for token in list(_gameplay_connections):
        if token not in live: _gameplay_callbacks.pop(token,None);_gameplay_connections.discard(token)
    existing=list(_gameplay_handles.values())
    for delivery in rustic.gameplay_callbacks():
        entry=_gameplay_callbacks.get(delivery["token"])
        if entry:
            fn,persistent=entry
            if not persistent: _gameplay_callbacks.pop(delivery["token"],None)
            fn(*delivery["arguments"])
    for handle in existing:
        state=handle.state()
        if state is None or state["status"] in ("failed","cancelled"):
            handle._cleanup()
            if state and state.get("error"): rustic.log("error",state["error"])

class Smooth:
    @staticmethod
    def lerp(a,b,t,easing="Linear"): return rustic.query({"op":"lerp","from":a,"to":b,"progress":t,"easing":easing})
    @staticmethod
    def slerp(a,b,t,easing="Linear"): return rustic.query({"op":"slerp","from":a,"to":b,"progress":t,"easing":easing})
    @staticmethod
    def inverseLerp(a,b,v): return rustic.query({"op":"inverse_lerp","from":a,"to":b,"value":v})
    @staticmethod
    def remap(v,a,b,c,d): return rustic.query(dict(op="remap",value=v,in_min=a,in_max=b,out_min=c,out_max=d))
    @staticmethod
    def smoothDamp(current,target,velocity,smoothTime,delta): return rustic.query(dict(op="smooth_damp",current=current,target=target,velocity=velocity,smooth_time=smoothTime,delta=delta))
Interpolation=Smooth

class Physics:
    @staticmethod
    def raycast(origin,direction,distance,ignore=None): return rustic.query(dict(op="physics_raycast",origin=origin,direction=direction,distance=distance,ignore=ignore or []))
    @staticmethod
    def sphereCast(origin,direction,distance,radius,ignore=None): return rustic.query(dict(op="physics_sphere_cast",origin=origin,direction=direction,distance=distance,radius=radius,ignore=ignore or []))
    @staticmethod
    def overlap(center,radius,ignore=None): return rustic.query(dict(op="physics_overlap",center=center,radius=radius,ignore=ignore or []))
    @staticmethod
    def impulse(o,vector): return rustic.query(dict(op="physics_impulse",entity=_entity(o),vector=vector))
    @staticmethod
    def force(o,vector,delta=None): return rustic.query(dict(op="physics_force",entity=_entity(o),vector=vector,delta=rustic.fixed_delta_time() if delta is None else delta))
    @staticmethod
    def launch(o,vector): return rustic.query(dict(op="physics_launch",entity=_entity(o),vector=vector))
    @staticmethod
    def explosion(center,radius,strength,ignore=None): return rustic.query(dict(op="physics_explosion",center=center,radius=radius,strength=strength,ignore=ignore or []))
    knockback=impulse

class Animation:
    @staticmethod
    def addMarker(o,clip,time,name): return rustic.query(dict(op="animation_marker",entity=_entity(o),clip=clip,time=time,name=name))
    @staticmethod
    def value(keys,fn): return GameplayHandle(rustic.query(dict(op="keyframes",keys=keys,token=_callback(fn,True))))
    @staticmethod
    def load(o,source): return rustic.query(dict(op="animation_load",entity=_entity(o),source=source))
    @staticmethod
    def register(o,clip): return rustic.query(dict(op="animation_register",entity=_entity(o),clip=clip))
    @staticmethod
    def clips(o): return rustic.query(dict(op="animation_list",entity=_entity(o)))
    @staticmethod
    def action(o,name,options=None):
        opts=options or {};clip=name if isinstance(name,dict) else rustic.query(dict(op="animation_ref",entity=_entity(o),name=name))
        return dict(kind="animation_ref",entity=_entity(o),clip=clip,options=dict(speed=opts.get("speed",1),blend_in=opts.get("blendIn",0),blend_in_ease=opts.get("blendInEase",opts.get("easing","Linear")),progression_ease=opts.get("progressionEase"),blend_out=opts.get("blendOut",0),blend_out_ease=opts.get("blendOutEase",opts.get("easing","Linear")),weight=opts.get("weight",1),additive=opts.get("additive",False),layered=opts.get("layered",bool(opts.get("mask")))),blend_source=opts.get("sourceClip"),mask=opts.get("mask",[]))
    @staticmethod
    def play(o,name,options=None):
        opts=options or {};action=Animation.action(o,name,opts);markers={}
        action["marker_token"]=_callback(lambda name: markers[name]() if name in markers else None,True)
        h=GameplayHandle(action,loop=opts.get("loop",False));h.stop=h.cancel
        def on_marker(name,fn):
            if not callable(fn): raise TypeError("expected callback")
            markers[name]=fn;return h
        def speed(v): rustic.gameplay(dict(command="speed",handle=h.id,speed=v));return h
        def looping(v=True): rustic.gameplay(dict(command="loop",handle=h.id,looping=v));return h
        h.onMarker=on_marker;h.speed=speed;h.loop=looping;return h
    @staticmethod
    def blend(o,source,target,options=None):
        opts=dict(options or {});opts["blendIn"]=opts.get("duration",0.3);opts["sourceClip"]=source if isinstance(source,dict) else rustic.query(dict(op="animation_ref",entity=_entity(o),name=source));return Animation.play(o,target,opts)
    transition=blend
    stop=staticmethod(lambda h:h.cancel())
    pause=staticmethod(lambda h:h.pause())
    speed=staticmethod(lambda h,v:h.speed(v))
    loop=staticmethod(lambda h,v=True:h.loop(v))
class AudioVoice:
    def __init__(self,id): self.id=id
    def stop(self): return rustic.query(dict(op="audio_stop",entity=self.id))
    def pause(self): return rustic.query(dict(op="audio_pause",entity=self.id))
    def resume(self): return rustic.query(dict(op="audio_resume",entity=self.id))
class Audio:
    @staticmethod
    def get_output_devices(): return rustic.query(dict(op="audio_get_output_devices"))
    @staticmethod
    def get_input_devices(): return rustic.query(dict(op="audio_get_input_devices"))
    @staticmethod
    def set_output_device(device=None): return rustic.query(dict(op="audio_set_output_device",device=device))
    @staticmethod
    def set_input_device(device=None): return rustic.query(dict(op="audio_set_input_device",device=device))
    @staticmethod
    def play(source,options=None): return AudioVoice(rustic.query(dict(options or {},op="audio_play",source=source)))
    @staticmethod
    def playAt(source,position,options=None): return Audio.play(source,dict(options or {},position=position))
    @staticmethod
    def volume(voice,value): return rustic.query(dict(op="audio_volume",entity=_entity(voice),value=value))
    @staticmethod
    def pitch(voice,value): return rustic.query(dict(op="audio_pitch",entity=_entity(voice),value=value))
    @staticmethod
    def fadeIn(voice,duration,easing="Linear",volume=1): return GameplayHandle(dict(_tween(voice,"Volume",volume,duration,easing),**{"from":0}))
    @staticmethod
    def fadeOut(voice,duration,easing="Linear"): return Tween.to(voice,"Volume",0,duration,easing)
    @staticmethod
    def crossfade(a,b,duration,easing="Linear"): return GameplayHandle(dict(kind="parallel",actions=[_tween(a,"Volume",0,duration,easing),dict(_tween(b,"Volume",1,duration,easing),**{"from":0})]))

def _ik(root,middle,tip,target,options=None):
    opts=options or {};return GameplayHandle(rustic.query(dict(op="animation_ik",root=_entity(root),middle=_entity(middle),tip=_entity(tip),target=target,pole=opts.get("pole",[0,0,1]),duration=opts.get("duration",0),weight=opts.get("weight",1),easing=opts.get("easing","Linear"))))
Animation.ik=staticmethod(_ik)
Animation.footPlacement=Animation.ik
Animation.lookAt=staticmethod(Movement.lookAt)
Animation.headTracking=Animation.lookAt
Animation.recoil=staticmethod(lambda o,r,t,e="Linear":GameplayHandle(_tween(o,"Rotation",r,t,e),repeats=1,ping_pong=True))
Effects.pulse=staticmethod(lambda o,s,t,e="Linear":GameplayHandle(_tween(o,"Scale",s,t,e),repeats=1,ping_pong=True))
Camera.transition=staticmethod(lambda o,p,r,f,t,e="Linear":GameplayHandle(dict(kind="parallel",actions=[_tween(o,"Position",p,t,e),_tween(o,"Rotation",r,t,e),_tween(o,"Fov",f,t,e)])))
Sequence.animation=lambda self,o,name,options=None: (self.actions.append(Animation.action(o,name,options)) or self)

class Clock:
    @staticmethod
    def timeScale(scale=None): return rustic.query(dict(op="clock",**({} if scale is None else {"scale":scale})))["scale"]
    @staticmethod
    def pause(): rustic.query(dict(op="clock",paused=True))
    @staticmethod
    def resume(): rustic.query(dict(op="clock",paused=False))
    @staticmethod
    def state(): return rustic.query(dict(op="clock"))
Camera.current=staticmethod(lambda:rustic.query(dict(op="camera_current")))

# Entity services use the shared Rust runtime.
class Tags:
    @staticmethod
    def add(entity,name): return rustic.query({"op":"tags_add",'entity':_entity(entity),'name':name})
    @staticmethod
    def remove(entity,name): return rustic.query({"op":"tags_remove",'entity':_entity(entity),'name':name})
    @staticmethod
    def find_all(name): return rustic.query({"op":"tags_find_all",'name':name})
class Groups:
    @staticmethod
    def create(name): return rustic.query({"op":"groups_create",'name':name})
    @staticmethod
    def add(name,entity): return rustic.query({"op":"groups_add",'name':name,'entity':_entity(entity)})
    @staticmethod
    def get_members(name): return rustic.query({"op":"groups_get_members",'name':name})
class Hierarchy:
    @staticmethod
    def set_parent(entity,parent,keep_world=True): return rustic.query({"op":"hierarchy_set_parent",'entity':_entity(entity),'parent':(None if parent is None else _entity(parent)),'keep_world':keep_world})
    @staticmethod
    def get_children(entity): return rustic.query({"op":"hierarchy_get_children",'entity':_entity(entity)})
    @staticmethod
    def detach(entity,keep_world=True): return rustic.query({"op":"hierarchy_detach",'entity':_entity(entity),'keep_world':keep_world})
class Entity:
    @staticmethod
    def destroy(entity): return rustic.query({"op":"entity_destroy",'entity':_entity(entity)})
    @staticmethod
    def destroy_after(entity,delay): return rustic.query({"op":"entity_destroy_after",'entity':_entity(entity),'delay':delay})
    @staticmethod
    def is_valid(entity): return rustic.query({"op":"entity_is_valid",'entity':_entity(entity)})
class Components:
    @staticmethod
    def has(entity,name): return rustic.query({"op":"components_has",'entity':_entity(entity),'name':name})
    @staticmethod
    def get_all(entity): return rustic.query({"op":"components_get_all",'entity':_entity(entity)})
    @staticmethod
    def find_in_children(entity,name): return rustic.query({"op":"components_find_in_children",'entity':_entity(entity),'name':name})
class World:
    @staticmethod
    def find_by_tag(name): return rustic.query({"op":"world_find_by_tag",'name':name})
    @staticmethod
    def find_in_radius(center,radius): return rustic.query({"op":"world_find_in_radius",'center':center,'radius':radius})

class LoadedAudio:
    def __init__(self, source): self.source=source; self.id=None
    def load(self):
        if self.id is None: self.id=rustic.query(dict(op="audio_load",source=self.source))
        return self
    def _loaded(self):
        if self.id is None: raise RuntimeError("audio is not loaded; call load() before play()")
        return self.id
    def play(self, options=None): return AudioVoice(rustic.query(dict(options or {},op="audio_loaded_play",entity=self._loaded())))
    def stop(self): rustic.query(dict(op="audio_loaded_stop",entity=self._loaded()))
    def pause(self): rustic.query(dict(op="audio_loaded_pause",entity=self._loaded()))
    def resume(self): rustic.query(dict(op="audio_loaded_resume",entity=self._loaded()))
    def unload(self):
        if self.id is not None: rustic.query(dict(op="audio_unload",entity=self.id)); self.id=None
    Unload=unload
Audio.load=staticmethod(lambda source:LoadedAudio(source).load())
class CaptureHandle:
    def __init__(self,id,recording): self.id=id; self.recording=recording
    def read(self): return rustic.query(dict(op="capture_read",id=self.id))
    def unload(self): rustic.query(dict(op="capture_unload",id=self.id))
    def stop(self): rustic.query(dict(op="capture_recording_stop" if self.recording else "capture_mic_stop",id=self.id))
def _start_recording(path=None,options=None): return CaptureHandle(rustic.query(dict(op="capture_recording_start",path=path,source=(options or {}).get("source","game"))),True)
Game.startRecording=_start_recording
Game.stopRecording=lambda handle=None:rustic.query(dict(op="capture_recording_stop",id=None if handle is None else handle.id))
Game.startHotMic=lambda:CaptureHandle(rustic.query(dict(op="capture_mic_start")),False)
Game.stopHotMic=lambda handle=None:rustic.query(dict(op="capture_mic_stop",id=None if handle is None else handle.id))

# Canonical API paths.
from inspect import signature, Signature
_path_methods={'scene': {'tweenTo': ['Tween', 'to', 0],
           'tweenMove': ['Tween', 'move', 0],
           'tweenRotate': ['Tween', 'rotate', 0],
           'tweenScale': ['Tween', 'scale', 0],
           'tweenAction': ['Tween', 'action', 0],
           'tweenToOptions': ['Tween', 'toOptions', 0],
           'movementMoveTo': ['Movement', 'moveTo', 0],
           'movementMove': ['Movement', 'move', 0],
           'movementRotateTo': ['Movement', 'rotateTo', 0],
           'movementLookAt': ['Movement', 'lookAt', 0],
           'movementFollow': ['Movement', 'follow', 0],
           'movementOrbit': ['Movement', 'orbit', 0],
           'movementMoveToOptions': ['Movement', 'moveToOptions', 0],
           'pathFollow': ['Path', 'follow', 0],
           'effectsFade': ['Effects', 'fade', 0],
           'effectsFlash': ['Effects', 'flash', 0],
           'effectsShake': ['Effects', 'shake', 0],
           'effectsPulse': ['Effects', 'pulse', 0],
           'cameraMoveTo': ['Camera', 'moveTo', 0],
           'cameraZoom': ['Camera', 'zoom', 0],
           'cameraFov': ['Camera', 'fov', 0],
           'cameraFollow': ['Camera', 'follow', 0],
           'cameraOrbit': ['Camera', 'orbit', 0],
           'cameraLookAt': ['Camera', 'lookAt', 0],
           'cameraShake': ['Camera', 'shake', 0],
           'cameraTransition': ['Camera', 'transition', 0],
           'physicsImpulse': ['Physics', 'impulse', 0],
           'physicsForce': ['Physics', 'force', 0],
           'physicsLaunch': ['Physics', 'launch', 0],
           'physicsKnockback': ['Physics', 'knockback', 0],
           'animationRegister': ['Animation', 'register', 0],
           'animationRegisterClip': ['Animation', 'registerClip', 0],
           'animationClips': ['Animation', 'clips', 0],
           'animationAction': ['Animation', 'action', 0],
           'animationPlay': ['Animation', 'play', 0],
           'animationBlend': ['Animation', 'blend', 0],
           'animationTransition': ['Animation', 'transition', 0],
           'animationIk': ['Animation', 'ik', 0],
           'animationFootPlacement': ['Animation', 'footPlacement', 0],
           'animationLookAt': ['Animation', 'lookAt', 0],
           'animationHeadTracking': ['Animation', 'headTracking', 0],
           'animationRecoil': ['Animation', 'recoil', 0],
           'animationAddMarker': ['Animation', 'addMarker', 0],
           'eventsConnect': ['Events', 'connect', 0],
           'tagsAdd': ['Tags', 'add', 0],
           'tagsRemove': ['Tags', 'remove', 0],
           'groupsAdd': ['Groups', 'add', 1],
           'hierarchySetParent': ['Hierarchy', 'set_parent', 0],
           'hierarchyGetChildren': ['Hierarchy', 'get_children', 0],
           'hierarchyDetach': ['Hierarchy', 'detach', 0],
           'entityDestroy': ['Entity', 'destroy', 0],
           'entityDestroyAfter': ['Entity', 'destroy_after', 0],
           'entityIsValid': ['Entity', 'is_valid', 0],
           'componentsHas': ['Components', 'has', 0],
           'componentsGetAll': ['Components', 'get_all', 0],
           'componentsFindInChildren': ['Components', 'find_in_children', 0],
           'tween': ['Tween', 'to', 0],
           'moveTo': ['Movement', 'moveTo', 0],
           'move': ['Movement', 'move', 0],
           'rotateTo': ['Movement', 'rotateTo', 0],
           'lookAt': ['Movement', 'lookAt', 0],
           'follow': ['Movement', 'follow', 0],
           'orbit': ['Movement', 'orbit', 0],
           'fade': ['Effects', 'fade', 0],
           'flash': ['Effects', 'flash', 0],
           'shake': ['Effects', 'shake', 0],
           'pulse': ['Effects', 'pulse', 0],
           'impulse': ['Physics', 'impulse', 0],
           'force': ['Physics', 'force', 0],
           'launch': ['Physics', 'launch', 0],
           'knockback': ['Physics', 'knockback', 0],
           'playAnimation': ['Animation', 'play', 0],
           'destroy': ['Entity', 'destroy', 0],
           'destroyAfter': ['Entity', 'destroy_after', 0],
           'isValid': ['Entity', 'is_valid', 0],
           'addTag': ['Tags', 'add', 0],
           'removeTag': ['Tags', 'remove', 0],
           'setParent': ['Hierarchy', 'set_parent', 0],
           'getChildren': ['Hierarchy', 'get_children', 0],
           'detach': ['Hierarchy', 'detach', 0],
           'hasComponent': ['Components', 'has', 0],
           'getComponents': ['Components', 'get_all', 0],
           'findInChildren': ['Components', 'find_in_children', 0],
           'followPath': ['Path', 'follow', 0]},
 'global': {'tweenValue': ['Tween', 'value', 0],
            'timerAfter': ['Timer', 'after', 0],
            'timerEvery': ['Timer', 'every', 0],
            'smoothLerp': ['Smooth', 'lerp', 0],
            'smoothSlerp': ['Smooth', 'slerp', 0],
            'smoothInverseLerp': ['Smooth', 'inverseLerp', 0],
            'smoothRemap': ['Smooth', 'remap', 0],
            'smoothSmoothDamp': ['Smooth', 'smoothDamp', 0],
            'physicsRaycast': ['Physics', 'raycast', 0],
            'physicsSphereCast': ['Physics', 'sphereCast', 0],
            'physicsOverlap': ['Physics', 'overlap', 0],
            'physicsExplosion': ['Physics', 'explosion', 0],
            'clockTimeScale': ['Clock', 'timeScale', 0],
            'clockPause': ['Clock', 'pause', 0],
            'clockResume': ['Clock', 'resume', 0],
            'clockState': ['Clock', 'state', 0],
            'pathCreate': ['Path', 'create', 0],
            'sequenceNew': ['Sequence', 'new', 0],
            'sequenceCreate': ['Sequence', 'create', 0],
            'timelineNew': ['Timeline', 'new', 0],
            'timelineCreate': ['Timeline', 'create', 0],
            'eventsOn': ['Events', 'on', 0],
            'eventsOnce': ['Events', 'once', 0],
            'eventsEmit': ['Events', 'emit', 0],
            'eventsDisconnect': ['Events', 'disconnect', 0],
            'audioGetOutputDevices': ['Audio', 'get_output_devices', 0],
            'audioGetInputDevices': ['Audio', 'get_input_devices', 0],
            'audioSetOutputDevice': ['Audio', 'set_output_device', 0],
            'audioSetInputDevice': ['Audio', 'set_input_device', 0],
            'audioVolume': ['Audio', 'volume', 0],
            'audioPitch': ['Audio', 'pitch', 0],
            'audioFadeIn': ['Audio', 'fadeIn', 0],
            'audioFadeOut': ['Audio', 'fadeOut', 0],
            'audioCrossfade': ['Audio', 'crossfade', 0],
            'audioStop': ['Audio', 'stop', 0],
            'audioPause': ['Audio', 'pause', 0],
            'audioResume': ['Audio', 'resume', 0],
            'animationValue': ['Animation', 'value', 0],
            'animationStop': ['Animation', 'stop', 0],
            'animationPause': ['Animation', 'pause', 0],
            'animationSpeed': ['Animation', 'speed', 0],
            'animationLoop': ['Animation', 'loop', 0],
            'cameraCurrent': ['Camera', 'current', 0],
            'tagsFindAll': ['Tags', 'find_all', 0],
            'groupsCreate': ['Groups', 'create', 0],
            'groupsGetMembers': ['Groups', 'get_members', 0],
            'worldFindByTag': ['World', 'find_by_tag', 0],
            'worldFindInRadius': ['World', 'find_in_radius', 0],
            'after': ['Timer', 'after', 0],
            'every': ['Timer', 'every', 0],
            'lerp': ['Smooth', 'lerp', 0],
            'slerp': ['Smooth', 'slerp', 0],
            'inverseLerp': ['Smooth', 'inverseLerp', 0],
            'remap': ['Smooth', 'remap', 0],
            'smoothDamp': ['Smooth', 'smoothDamp', 0],
            'raycast': ['Physics', 'raycast', 0],
            'sphereCast': ['Physics', 'sphereCast', 0],
            'overlap': ['Physics', 'overlap', 0],
            'explosion': ['Physics', 'explosion', 0],
            'timeScale': ['Clock', 'timeScale', 0],
            'pause': ['Clock', 'pause', 0],
            'resume': ['Clock', 'resume', 0],
            'createPath': ['Path', 'create', 0],
            'sequence': ['Sequence', 'new', 0],
            'timeline': ['Timeline', 'new', 0],
            'on': ['Events', 'on', 0],
            'once': ['Events', 'once', 0],
            'emit': ['Events', 'emit', 0],
            'disconnect': ['Events', 'disconnect', 0],
            'currentCamera': ['Camera', 'current', 0],
            'getOutputDevices': ['Audio', 'get_output_devices', 0],
            'getInputDevices': ['Audio', 'get_input_devices', 0],
            'setOutputDevice': ['Audio', 'set_output_device', 0],
            'setInputDevice': ['Audio', 'set_input_device', 0]},
 'file': {'audioPlay': ['Audio', 'play', 0],
          'audioPlayAt': ['Audio', 'playAt', 0],
          'audioLoad': ['Audio', 'load', 0],
          'animationLoad': ['Animation', 'load', 1],
          'play': ['Audio', 'play', 0],
          'playAt': ['Audio', 'playAt', 0],
          'load': ['Audio', 'load', 0],
          'loadAnimation': ['Animation', 'load', 1],
          'add': ['instance', 'add', 0],
          'clone': ['instance', 'clone', 0],
          'setSkyTexture': ['scene', 'setSkyTexture', 0]}}
_path_methods['global'].update({'get_output_devices':_path_methods['global']['getOutputDevices'],'get_input_devices':_path_methods['global']['getInputDevices'],'set_output_device':_path_methods['global']['setOutputDevice'],'set_input_device':_path_methods['global']['setInputDevice']})
_path_methods['scene'].update({'clone':['instance','clone',0],'add':['instance','add',0],'setCurrentCamera':['Game','setCurrentCamera',0],'emit':['Events','emit',2]})
class ApiPath:
    def __init__(self,source,category,file_path=None):
        self.source=source; self.category=category; self.file_path=file_path; self.children={}
    def __getitem__(self,name):
        name=str(name)
        if name not in self.children:
            self.children[name]=ApiPath(self.source+'.'+name,self.category,(self.file_path+'/' if self.file_path else '')+name if self.category=='file' else None)
        return self.children[name]
    def __getattr__(self,name):
        if self.source=='game.scene' and name in ('load','unload','switchTo','isLoaded','getLoaded','getActive'): return getattr(Game.scene,name)
        if name in ('EditAttribute','edit_attribute'):
            return lambda attribute,value: ObjectPath(self.source).EditAttribute(attribute,value)
        binding=_path_methods[self.category].get(name)
        if binding:
            family,method,index=binding
            owner=Game.scene if family=='scene' else globals()[family]
            fn=getattr(owner,method,None)
            if fn is None: raise AttributeError('API unavailable: '+name)
            parameters=list(signature(fn).parameters.values())
            target_name=parameters[index].name
            receiver_signature=Signature(parameters[:index]+parameters[index+1:])
            def argument(value):
                if isinstance(value,ApiPath): return value.file_path if value.category=='file' else value.source
                return value
            def call(*args,**kwargs):
                bound=receiver_signature.bind(*[argument(a) for a in args],**{k:argument(v) for k,v in kwargs.items()})
                bound.arguments[target_name]=self.file_path if self.category=='file' else self.source
                return fn(**bound.arguments)
            return call
        return self[name]
class GlobalApi:
    def __init__(self): self.files=ApiPath('game.rustic.files','file','')
    def __getattr__(self,name):
        binding=_path_methods['global'].get(name)
        if binding:
            family,method,_=binding
            if family in ('Sequence','Timeline') and method=='new': method='new'
            fn=getattr(globals()[family],method,None)
            if fn is None: raise AttributeError('API unavailable: '+name)
            return fn
        if name in ('getEnvironment','setEnvironment'): return getattr(Game.scene,name)
        if name=='setCurrentCamera': return lambda obj: Game.setCurrentCamera(obj.source if isinstance(obj,ApiPath) else obj)
        return getattr(rustic,name)
class CanonicalGame:
    def __init__(self): self.scene=ApiPath('game.scene','scene'); self.rustic=GlobalApi()
game=CanonicalGame()
