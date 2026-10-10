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
    def __getattr__(self, name): return ObjectPath(self.source + "." + name)
    def __getitem__(self, name): return ObjectPath(self.source + "." + str(name))
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
    def log(self, level, message): self.commands.append({"op":"log","level":str(level),"message":str(message)})
    def set_enabled(self, enabled): self.commands.append({"op":"set_enabled","enabled":bool(enabled)})

class SceneApi:
    def getEnvironment(self): return rustic.query({"op":"scene_environment"})
    def setEnvironment(self, settings): return rustic.query({"op":"scene_environment", "settings":settings})
    def setSkyTexture(self, path): return self.setEnvironment({"sky_image":path})
    def __init__(self): self.state = {}
    def Find(self, path): return self.state.get(path)
    def List(self, path="Game.scene"):
        prefix = "" if path in ("", "Game.scene") else path.rstrip("./") + "."
        return [entity for name, entity in self.state.items() if not prefix or name.startswith(prefix)]

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
