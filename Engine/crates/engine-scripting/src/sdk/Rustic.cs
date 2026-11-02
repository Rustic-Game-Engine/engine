global using Interpolation = Rustic.Smooth;
global using Timer = Rustic.Timer;
global using Path = Rustic.Path;
using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;

record RusticActionState(bool pressed=false,bool released=false,bool held=false,double axis=0);
record RusticKeyEvent(string key,string state,bool repeat);
static class Rustic {
    public static readonly InstanceApi instance = new();
    public static readonly RusticApi rustic = new(instance.Commands);
    public static readonly GameApi Game = new(instance.Commands);
    public static readonly CanonicalGame game = new();
    internal static JsonElement Query(object query){
        Console.WriteLine(JsonSerializer.Serialize(new{query,commands=instance.Commands.ToArray()}));instance.Commands.Clear();
        using var response=JsonDocument.Parse(Console.ReadLine()??throw new InvalidOperationException("query host exited"));
        if(response.RootElement.TryGetProperty("error",out var error))throw new InvalidOperationException(error.GetString());
        return response.RootElement.GetProperty("result").Clone();
    }

    public static class Tags {
        public static bool add(string? entity,string name)=>Query(new{op="tags_add",entity=Gameplay.Entity(entity),name=name}).GetBoolean();
        public static bool remove(string? entity,string name)=>Query(new{op="tags_remove",entity=Gameplay.Entity(entity),name=name}).GetBoolean();
        public static string[] find_all(string name)=>Query(new{op="tags_find_all",name=name}).EnumerateArray().Select(v=>v.GetString()!).ToArray();
    }
    public static class Groups {
        public static bool create(string name)=>Query(new{op="groups_create",name=name}).GetBoolean();
        public static bool add(string name,string? entity)=>Query(new{op="groups_add",name=name,entity=Gameplay.Entity(entity)}).GetBoolean();
        public static string[] get_members(string name)=>Query(new{op="groups_get_members",name=name}).EnumerateArray().Select(v=>v.GetString()!).ToArray();
    }
    public static class Hierarchy {
        public static void set_parent(string? entity,string? parent,bool keep_world=true)=>Query(new{op="hierarchy_set_parent",entity=Gameplay.Entity(entity),parent=(parent is null?null:Gameplay.Entity(parent)),keep_world=keep_world});
        public static string[] get_children(string? entity)=>Query(new{op="hierarchy_get_children",entity=Gameplay.Entity(entity)}).EnumerateArray().Select(v=>v.GetString()!).ToArray();
        public static void detach(string? entity,bool keep_world=true)=>Query(new{op="hierarchy_detach",entity=Gameplay.Entity(entity),keep_world=keep_world});
    }
    public static class Entity {
        public static void destroy(string? entity)=>Query(new{op="entity_destroy",entity=Gameplay.Entity(entity)});
        public static void destroy_after(string? entity,double delay)=>Query(new{op="entity_destroy_after",entity=Gameplay.Entity(entity),delay=delay});
        public static bool is_valid(string? entity)=>Query(new{op="entity_is_valid",entity=Gameplay.Entity(entity)}).GetBoolean();
    }
    public static class Components {
        public static bool has(string? entity,string name)=>Query(new{op="components_has",entity=Gameplay.Entity(entity),name=name}).GetBoolean();
        public static string[] get_all(string? entity)=>Query(new{op="components_get_all",entity=Gameplay.Entity(entity)}).EnumerateArray().Select(v=>v.GetString()!).ToArray();
        public static string[] find_in_children(string? entity,string name)=>Query(new{op="components_find_in_children",entity=Gameplay.Entity(entity),name=name}).EnumerateArray().Select(v=>v.GetString()!).ToArray();
    }
    public static class World {
        public static string[] find_by_tag(string name)=>Query(new{op="world_find_by_tag",name=name}).EnumerateArray().Select(v=>v.GetString()!).ToArray();
        public static string[] find_in_radius(double[] center,double radius)=>Query(new{op="world_find_in_radius",center=center,radius=radius}).EnumerateArray().Select(v=>v.GetString()!).ToArray();
    }

    internal static object Ordinary(JsonElement v)=>v.ValueKind switch{JsonValueKind.String=>v.GetString()!,JsonValueKind.True=>true,JsonValueKind.False=>false,JsonValueKind.Number=>v.GetDouble(),JsonValueKind.Array=>v.EnumerateArray().Select(n=>n.GetDouble()).ToArray(),_=>throw new InvalidOperationException("unsupported gameplay value")};
    public sealed record MotionOptions(double? duration=null,double? speed=null,string easing="Linear");
    public static class Smooth{
        public static double lerp(double a,double b,double t,string easing="Linear")=>Query(new{op="lerp",from=a,to=b,progress=t,easing}).GetDouble();
        public static double[] lerp(double[] a,double[] b,double t,string easing="Linear")=>Query(new{op="lerp",from=a,to=b,progress=t,easing}).EnumerateArray().Select(v=>v.GetDouble()).ToArray();
        public static double[] slerp(double[] a,double[] b,double t,string easing="Linear")=>Query(new{op="slerp",from=a,to=b,progress=t,easing}).EnumerateArray().Select(v=>v.GetDouble()).ToArray();
        public static double inverseLerp(double a,double b,double value)=>Query(new{op="inverse_lerp",from=a,to=b,value}).GetDouble();
        public static double remap(double value,double a,double b,double c,double d)=>Query(new{op="remap",value,in_min=a,in_max=b,out_min=c,out_max=d}).GetDouble();
        public static (double value,double velocity) smoothDamp(double current,double target,double velocity,double smoothTime,double delta){var r=Query(new{op="smooth_damp",current,target,velocity,smooth_time=smoothTime,delta});return(r.GetProperty("value").GetDouble(),r.GetProperty("velocity").GetDouble());}
    }
    public static class Input{
        public static JsonElement touches()=>Query(new{op="input_touches"});
        public static JsonElement touchEvents()=>Query(new{op="input_touch_events"});
        public static JsonElement clicks(string? entity=null)=>Query(new{op="input_clicks",entity=entity is null?null:Gameplay.Entity(entity)});
        public static Events.Connection onClick(string entity,Action<object[]> fn)=>Events.connect(entity,"Click",fn);
        public static Events.Connection onTouchStart(Action<object[]> fn)=>Events.on("TouchStart",fn);
        public static Events.Connection onTouchMove(Action<object[]> fn)=>Events.on("TouchMove",fn);
        public static Events.Connection onTouchEnd(Action<object[]> fn)=>Events.on("TouchEnd",fn);
        public static Events.Connection onTouchCancel(Action<object[]> fn)=>Events.on("TouchCancel",fn);
    }
    public static class Physics{
        public static JsonElement ground(string? e,double maxAngle=45)=>Query(new{op="physics_ground",entity=Gameplay.Entity(e),max_angle=maxAngle});
        public static Events.Connection onCollisionEnter(string e,Action<object[]> fn)=>Events.connect(e,"CollisionEnter",fn);
        public static Events.Connection onCollisionStay(string e,Action<object[]> fn)=>Events.connect(e,"CollisionStay",fn);
        public static Events.Connection onCollisionExit(string e,Action<object[]> fn)=>Events.connect(e,"CollisionExit",fn);
        public sealed record Hit(string entity,double[] point,double[] normal,double distance);
        public static Hit? raycast(double[] origin,double[] direction,double distance)=>JsonSerializer.Deserialize<Hit>(Query(new{op="physics_raycast",origin,direction,distance}).GetRawText());
        public static Hit? sphereCast(double[] origin,double[] direction,double distance,double radius)=>JsonSerializer.Deserialize<Hit>(Query(new{op="physics_sphere_cast",origin,direction,distance,radius}).GetRawText());
        public static string[] overlap(double[] center,double radius)=>Query(new{op="physics_overlap",center,radius}).EnumerateArray().Select(v=>v.GetString()!).ToArray();
        public static void impulse(string? e,double[] vector)=>Query(new{op="physics_impulse",entity=Gameplay.Entity(e),vector});
        public static void launch(string? e,double[] vector)=>Query(new{op="physics_launch",entity=Gameplay.Entity(e),vector});
        public static void force(string? e,double[] vector,double delta)=>Query(new{op="physics_force",entity=Gameplay.Entity(e),vector,delta});
        public static void knockback(string? e,double[] vector)=>impulse(e,vector);
        public static string[] explosion(double[] center,double radius,double strength)=>Query(new{op="physics_explosion",center,radius,strength}).EnumerateArray().Select(v=>v.GetString()!).ToArray();
    }
    public static class Gameplay {
        private static readonly Dictionary<string,(Action<JsonElement[]> fn,bool once)> callbacks=new();
        private static readonly Dictionary<string,Handle> handles=new();
        internal static string Callback(Action fn)=>Register(_=>fn(),true);
        internal static string Register(Action<JsonElement[]> fn,bool once=false){var id=Guid.NewGuid().ToString();callbacks[id]=(fn,once);return id;}
        internal static readonly HashSet<string> Connections=new();
        internal static void Disconnect(string token){Connections.Remove(token);callbacks.Remove(token);}
        internal static void Release(Handle h){foreach(var t in h.Tokens)callbacks.Remove(t);callbacks.Remove(h.Token);handles.Remove(h.Id);}
        private static void Collect(JsonElement a,List<string> tokens){if(a.TryGetProperty("token",out var token))tokens.Add(token.GetString()!);if(a.TryGetProperty("marker_token",out token)&&token.ValueKind==JsonValueKind.String)tokens.Add(token.GetString()!);if(a.TryGetProperty("actions",out var actions))foreach(var child in actions.EnumerateArray())Collect(child,tokens);}
        internal static void Send(object request)=>instance.Commands.Add(new {op="gameplay",request});
        internal static string Entity(string? id)=>id is null?rustic.entity_id():id.StartsWith("game.scene.",StringComparison.Ordinal)?Query(new{op="scene_object",source=id}).GetString()!:Game.scene.Find(id)??id;
        public static Handle Start(object action, bool loop=false, uint repeats=0, bool pingPong=false){
            var h=new Handle(Guid.NewGuid().ToString());handles[h.Id]=h;
            using(var doc=JsonDocument.Parse(JsonSerializer.Serialize(action)))Collect(doc.RootElement,h.Tokens);
            h.Token=Callback(()=>{Release(h);h.Finished?.Invoke();});
            Send(new {command="start",handle=h.Id,action,playback=new {looping=loop,repeats,ping_pong=pingPong},on_finished=h.Token});return h;
        }
        internal static void Dispatch(){
            var live=rustic.State.GetProperty("gameplay_connections").EnumerateArray().Select(v=>v.GetString()!).ToHashSet();
            foreach(var token in Connections.ToArray())if(!live.Contains(token))Disconnect(token);
            var existing=handles.Values.ToArray();
            var present=new HashSet<string>();
            foreach(var d in rustic.State.GetProperty("gameplay_callbacks").EnumerateArray()){
                var token=d.GetProperty("token").GetString()!;if(callbacks.TryGetValue(token,out var cb)){if(cb.once)callbacks.Remove(token);cb.fn(d.GetProperty("arguments").EnumerateArray().Select(v=>v.Clone()).ToArray());}
            }
            foreach(var state in rustic.State.GetProperty("gameplay_states").EnumerateArray()){
                var id=state.GetProperty("handle").GetString()!;present.Add(id);var status=state.GetProperty("status").GetString();
                if((status=="failed"||status=="cancelled")&&handles.Remove(id,out var h)){Release(h);if(state.TryGetProperty("error",out var error)&&error.ValueKind==JsonValueKind.String)rustic.log("error",error.GetString()!);}
            }
            foreach(var h in existing)if(!present.Contains(h.Id))Release(h);
        }
        public record OperationInfo(string status,string? error);
        public sealed class Handle {
            public OperationInfo? state(){var result=Query(new{op="operation_state",handle=Id});return result.ValueKind==JsonValueKind.Null?null:new(result.GetProperty("status").GetString()!,result.GetProperty("error").ValueKind==JsonValueKind.Null?null:result.GetProperty("error").GetString());}
            public string Id {get;}
            internal readonly List<string> Tokens=new();
            public readonly Dictionary<string,Action> Markers=new();
            internal string Token="";internal Action? Finished;
            internal Handle(string id)=>Id=id;
            public Handle onFinished(Action fn){Finished=fn;return this;}
            public Handle pause(){Send(new{command="pause",handle=Id});return this;}
            public Handle resume(){Send(new{command="resume",handle=Id});return this;}
            public Handle cancel(){Send(new{command="cancel",handle=Id});Release(this);return this;}
            public Handle stop()=>cancel();
            public Handle speed(double speed){Send(new{command="speed",handle=Id,speed});return this;}
            public Handle loop(bool looping=true){Send(new{command="loop",handle=Id,looping});return this;}
            public Handle onMarker(string name,Action fn){Markers[name]=fn;return this;}
            public Handle reverse(){Send(new{command="reverse",handle=Id});return this;}
        }
    }
    public static class Ease {
        public const string Linear="Linear";
        public const string InSine="InSine";
        public const string OutSine="OutSine";
        public const string InOutSine="InOutSine";
        public const string InQuad="InQuad";
        public const string OutQuad="OutQuad";
        public const string InOutQuad="InOutQuad";
        public const string InCubic="InCubic";
        public const string OutCubic="OutCubic";
        public const string InOutCubic="InOutCubic";
        public const string InQuart="InQuart";
        public const string OutQuart="OutQuart";
        public const string InOutQuart="InOutQuart";
        public const string InQuint="InQuint";
        public const string OutQuint="OutQuint";
        public const string InOutQuint="InOutQuint";
        public const string InExpo="InExpo";
        public const string OutExpo="OutExpo";
        public const string InOutExpo="InOutExpo";
        public const string InCirc="InCirc";
        public const string OutCirc="OutCirc";
        public const string InOutCirc="InOutCirc";
        public const string InBack="InBack";
        public const string OutBack="OutBack";
        public const string InOutBack="InOutBack";
        public const string InElastic="InElastic";
        public const string OutElastic="OutElastic";
        public const string InOutElastic="InOutElastic";
        public const string InBounce="InBounce";
        public const string OutBounce="OutBounce";
        public const string InOutBounce="InOutBounce";
    }
    public static class Tween {
        internal static object Action(string? e,string p,object to,MotionOptions opts){if((opts.duration is null)==(opts.speed is null))throw new ArgumentException("choose duration or speed");return new{kind="tween",target=new{entity=Gameplay.Entity(e),property=p},to,duration=opts.duration,speed=opts.speed,easing=opts.easing};}
        public static Gameplay.Handle to(string? e,string p,object to,MotionOptions opts)=>Gameplay.Start(Action(e,p,to,opts));
        public static Gameplay.Handle move(string? e,double[] p,MotionOptions opts)=>to(e,"Position",p,opts);
        public static Gameplay.Handle rotate(string? e,double[] p,MotionOptions opts)=>to(e,"Rotation",p,opts);
        public static Gameplay.Handle value(object from,object to,double duration,Action<object> sample,string easing="Linear")=>Gameplay.Start(new{kind="value",from,to,duration,easing,token=Gameplay.Register(a=>sample(Ordinary(a[0])))});
        internal static object Action(string? entity,string property,object to,double duration,string easing)=>new {kind="tween",target=new {entity=Gameplay.Entity(entity),property},to,duration,easing};
        public static Gameplay.Handle to(string? entity,string property,object value,double duration,string easing="Linear")=>Gameplay.Start(Action(entity,property,value,duration,easing));
        public static Gameplay.Handle move(string? entity,double[] position,double duration,string easing="Linear")=>to(entity,"Position",position,duration,easing);
        public static Gameplay.Handle rotate(string? entity,double[] rotation,double duration,string easing="Linear")=>to(entity,"Rotation",rotation,duration,easing);
        public static Gameplay.Handle scale(string? entity,double[] scale,double duration,string easing="Linear")=>to(entity,"Scale",scale,duration,easing);
    }
    public static class Movement {
        public static Gameplay.Handle moveTo(string? e,double[] p,MotionOptions opts)=>Tween.move(e,p,opts);
        public static Gameplay.Handle rotateTo(string? e,double[] p,MotionOptions opts)=>Tween.rotate(e,p,opts);
        public static Gameplay.Handle moveTo(string? e,double[] p,double d,string ease="Linear")=>Tween.move(e,p,d,ease);
        public static Gameplay.Handle rotateTo(string? e,double[] q,double d,string ease="Linear")=>Tween.rotate(e,q,d,ease);
        public static Gameplay.Handle move(string? e,double[] offset,double duration,string easing="Linear")=>Gameplay.Start(new{kind="move",entity=Gameplay.Entity(e),offset,duration,easing});
        public static Gameplay.Handle lookAt(string? e,double[] position,double duration=0,string easing="Linear")=>Gameplay.Start(new{kind="look_at",entity=Gameplay.Entity(e),position,duration,easing});
        public static Gameplay.Handle follow(string? e,string target,double duration,string easing="Linear",double[]? offset=null)=>Gameplay.Start(new{kind="follow",entity=Gameplay.Entity(e),target=Gameplay.Entity(target),duration,easing,offset=offset??new double[3]});
        public static Gameplay.Handle orbit(string? e,double[] center,double radius,double turns,double duration,string easing="Linear")=>Gameplay.Start(new{kind="orbit",entity=Gameplay.Entity(e),center,radius,turns,duration,easing});
    }
    public static class Sequence {
        public static Builder New()=>new();
        public sealed class Builder {
            internal List<object> actions=new();
            public Builder move(string? e,double[] p,double d,string ease="Linear"){actions.Add(Tween.Action(e,"Position",p,d,ease));return this;}
            public Builder to(string? e,string p,object value,double duration,string easing="Linear"){actions.Add(Tween.Action(e,p,value,duration,easing));return this;}
            public Builder call(Action fn){actions.Add(new{kind="callback",token=Gameplay.Callback(fn)});return this;}
            public Builder animation(string? e,string name,Animation.Options? opts=null){actions.Add(Animation.Action(e,name,opts));return this;}
            public Builder parallel(params Builder[] branches){actions.Add(new{kind="parallel",actions=branches.Select(b=>new{kind="sequence",actions=b.actions.ToArray()}).ToArray()});return this;}
            public Builder wait(double duration){actions.Add(new{kind="wait",duration});return this;}
            public Gameplay.Handle play()=>Gameplay.Start(new{kind="sequence",actions=actions.ToArray()});
        }
    }
    public static class Timeline{public static Sequence.Builder New()=>Sequence.New();}
    public static class Timer {
        public static Gameplay.Handle every(double interval,Action fn,uint? count=null){if(interval<=0||count==0)throw new ArgumentException("positive interval and count required");return Gameplay.Start(new{kind="sequence",actions=new object[]{new{kind="wait",duration=interval},new{kind="callback",token=Gameplay.Register(_=>fn())}}},count is null,(count??1)-1);}
        public static Gameplay.Handle after(double delay,Action fn)=>Gameplay.Start(new{kind="wait",duration=delay}).onFinished(fn);
    }
    public static class Path{
        public record Point(double[] point,string easing="Linear");
        public record Curve(string kind,Point[] points);
        public static Curve create(double[][] points,string kind="Linear")=>new(kind,points.Select(p=>new Point(p)).ToArray());
        public static Curve create(Point[] points,string kind="Linear")=>new(kind,points);
        public static Gameplay.Handle follow(string? e,Curve path,double duration,string easing="Linear",bool loop=false,bool pingPong=false,bool orientToPath=false)=>Gameplay.Start(new{kind="path",entity=Gameplay.Entity(e),path,duration,easing,orient_to_path=orientToPath},loop,0,pingPong);
        public static Gameplay.Handle followSpeed(string? e,Curve path,double speed,string easing="Linear",bool loop=false,bool pingPong=false,bool orientToPath=false)=>Gameplay.Start(new{kind="path",entity=Gameplay.Entity(e),path,speed,easing,orient_to_path=orientToPath},loop,0,pingPong);
    }
    public static class Effects {
        public static Gameplay.Handle flash(string? e,double[] color,double d,string ease="Linear")=>Gameplay.Start(Tween.Action(e,"Color",color,d,ease),false,1,true);
        public static Gameplay.Handle pulse(string? e,double[] scale,double d,string ease="Linear")=>Gameplay.Start(Tween.Action(e,"Scale",scale,d,ease),false,1,true);
        public static Gameplay.Handle shake(string? e,double strength,double duration,string easing="Linear")=>Gameplay.Start(new{kind="shake",entity=Gameplay.Entity(e),strength,duration,easing});
        public static Gameplay.Handle fade(string? entity,double opacity,double duration,string easing="Linear")=>Tween.to(entity,"Opacity",opacity,duration,easing);
    }
    public static class Clock {
        public static JsonElement state()=>Query(new{op="clock"});
        public static double timeScale(double? scale=null)=>Query(scale.HasValue?new {op="clock",scale=scale.Value}:(object)new{op="clock"}).GetProperty("scale").GetDouble();
        public static void pause()=>Query(new{op="clock",paused=true});public static void resume()=>Query(new{op="clock",paused=false});
    }
    public static class Camera {
        public static string current()=>Query(new{op="camera_current"}).GetString()!;
        public static Gameplay.Handle follow(string? e,string target,double d,string ease="Linear")=>Movement.follow(e,target,d,ease);
        public static Gameplay.Handle lookAt(string? e,double[] p,double d,string ease="Linear")=>Movement.lookAt(e,p,d,ease);
        public static Gameplay.Handle orbit(string? e,double[] c,double r,double turns,double d,string ease="Linear")=>Movement.orbit(e,c,r,turns,d,ease);
        public static Gameplay.Handle shake(string? e,double s,double d,string ease="Linear")=>Effects.shake(e,s,d,ease);
        public static Gameplay.Handle fov(string e,double v,double d,string ease="Linear")=>zoom(e,v,d,ease);
        public static Gameplay.Handle transition(string e,double[] p,double[] r,double f,double d,string ease="Linear")=>Gameplay.Start(new{kind="parallel",actions=new[]{Tween.Action(e,"Position",p,d,ease),Tween.Action(e,"Rotation",r,d,ease),Tween.Action(e,"Fov",f,d,ease)}});
        public static Gameplay.Handle moveTo(string entity,double[] p,double duration,string easing="Linear")=>Tween.move(entity,p,duration,easing);
        public static Gameplay.Handle zoom(string entity,double fov,double duration,string easing="Linear")=>Tween.to(entity,"Fov",fov,duration,easing);
    }
    public static class Animation{
        public static void addMarker(string? e,string clip,double time,string name)=>Query(new{op="animation_marker",entity=Gameplay.Entity(e),clip,time,name});
        public static Gameplay.Handle value(Key[] keys,Action<object> sample)=>Gameplay.Start(Query(new{op="keyframes",keys,token=Gameplay.Register(a=>sample(Ordinary(a[0])))}));
        public record Key(double time,object value,string easing="Linear");
        public record Track(string target,Key[] keys);
        public record Marker(double time,string name);
        public record Clip(string name,double duration,Track[] tracks,Marker[] markers);
        public record Options(double speed=1,double blendIn=0,double blendOut=0,string easing="Linear",string? blendInEase=null,string? blendOutEase=null,string? progressionEase=null,bool loop=false,double weight=1,bool additive=false,string[]? mask=null);
        public static string[] load(string? e,string source)=>Query(new{op="animation_load",entity=Gameplay.Entity(e),source}).EnumerateArray().Select(v=>v.GetString()!).ToArray();
        public static void register(string? e,Clip clip)=>Query(new{op="animation_register",entity=Gameplay.Entity(e),clip});
        public static string[] clips(string? e)=>Query(new{op="animation_list",entity=Gameplay.Entity(e)}).EnumerateArray().Select(v=>v.GetString()!).ToArray();
        internal static object Action(string? e,string name,Options? opts=null,string? token=null,JsonElement? source=null){opts??=new();return new{kind="animation_ref",entity=Gameplay.Entity(e),clip=Query(new{op="animation_ref",entity=Gameplay.Entity(e),name}),options=new{speed=opts.speed,blend_in=opts.blendIn,blend_in_ease=opts.blendInEase??opts.easing,blend_out=opts.blendOut,blend_out_ease=opts.blendOutEase??opts.easing,progression_ease=opts.progressionEase,weight=opts.weight,additive=opts.additive,layered=opts.mask is {Length: >0}},mask=opts.mask??Array.Empty<string>(),marker_token=token,blend_source=source};}
        public static Gameplay.Handle play(string? e,string name,Options? opts=null){Gameplay.Handle? h=null;var token=Gameplay.Register(args=>{if(h!=null&&h.Markers.TryGetValue(args[0].GetString()!,out var fn))fn();});h=Gameplay.Start(Action(e,name,opts,token),opts?.loop??false);return h;}
        public static Gameplay.Handle blend(string? e,string from,string to,double duration,string easing="Linear")=>Gameplay.Start(Action(e,to,new(blendIn:duration,easing:easing),source:Query(new{op="animation_ref",entity=Gameplay.Entity(e),name=from})));
        public static Gameplay.Handle transition(string? e,string from,string to,double duration,string easing="Linear")=>blend(e,from,to,duration,easing);
        public static void stop(Gameplay.Handle h)=>h.cancel();public static void pause(Gameplay.Handle h)=>h.pause();public static void speed(Gameplay.Handle h,double v)=>h.speed(v);public static void loop(Gameplay.Handle h,bool v=true)=>h.loop(v);
        public static Gameplay.Handle ik(string root,string middle,string tip,double[] target,double duration=0,string easing="Linear",double weight=1,double[]? pole=null)=>Gameplay.Start(Query(new{op="animation_ik",root=Gameplay.Entity(root),middle=Gameplay.Entity(middle),tip=Gameplay.Entity(tip),target,duration,easing,weight,pole=pole??new double[]{0,0,1}}));
        public static Gameplay.Handle footPlacement(string root,string middle,string tip,double[] target,double duration=0,string easing="Linear")=>ik(root,middle,tip,target,duration,easing);
        public static Gameplay.Handle lookAt(string e,double[] target,double duration=0,string easing="Linear")=>Movement.lookAt(e,target,duration,easing);
        public static Gameplay.Handle headTracking(string e,double[] target,double duration=0,string easing="Linear")=>Movement.lookAt(e,target,duration,easing);
        public static Gameplay.Handle recoil(string e,double[] rotation,double duration,string easing="Linear")=>Gameplay.Start(Tween.Action(e,"Rotation",rotation,duration,easing),false,1,true);
    }
    public sealed class CaptureHandle {
        public string Id {get;}
        private readonly bool recording;
        internal CaptureHandle(string id,bool recording){Id=id;this.recording=recording;}
        public JsonElement read()=>Query(new{op="capture_read",id=Id});
        public void unload()=>Query(new{op="capture_unload",id=Id});
        public void stop()=>Query(new{op=recording?"capture_recording_stop":"capture_mic_stop",id=Id});
    }
    public static class Audio{
        public sealed class LoadedAudio {
            private readonly string source;
            private string? id;
            internal LoadedAudio(string source){this.source=source;}
            public LoadedAudio load(){if(id is null)id=Query(new{op="audio_load",source}).GetString()!;return this;}
            private string Loaded()=>id??throw new InvalidOperationException("audio is not loaded; call load() before play()");
            public Voice play(double volume=1,double pitch=1,bool loop=false,double[]? position=null)=>new(Query(new{op="audio_loaded_play",entity=Loaded(),volume,pitch,loop,position}).GetString()!);
            public void stop()=>Query(new{op="audio_loaded_stop",entity=Loaded()});
            public void pause()=>Query(new{op="audio_loaded_pause",entity=Loaded()});
            public void resume()=>Query(new{op="audio_loaded_resume",entity=Loaded()});
            public void unload(){if(id is not null){Query(new{op="audio_unload",entity=id});id=null;}}
            public void Unload()=>unload();
        }
        public static LoadedAudio load(string source)=>new LoadedAudio(source).load();
        public static JsonElement get_output_devices()=>Query(new{op="audio_get_output_devices"});
        public static JsonElement get_input_devices()=>Query(new{op="audio_get_input_devices"});
        public static void set_output_device(string? device=null)=>Query(new{op="audio_set_output_device",device});
        public static void set_input_device(string? device=null)=>Query(new{op="audio_set_input_device",device});
        public sealed record Voice(string Id){public void stop()=>Query(new{op="audio_stop",entity=Id});public void pause()=>Query(new{op="audio_pause",entity=Id});public void resume()=>Query(new{op="audio_resume",entity=Id});}
        public static Voice play(string source,double volume=1,double pitch=1,bool loop=false)=>new(Query(new{op="audio_play",source,volume,pitch,loop}).GetString()!);
        public static Voice playAt(string source,double[] position,double volume=1,double pitch=1,bool loop=false)=>new(Query(new{op="audio_play",source,position,volume,pitch,loop}).GetString()!);
        public static void volume(Voice v,double value)=>Query(new{op="audio_volume",entity=v.Id,value});public static void pitch(Voice v,double value)=>Query(new{op="audio_pitch",entity=v.Id,value});
        public static Gameplay.Handle fadeIn(Voice v,double duration,string easing="Linear")=>Gameplay.Start(new{kind="tween",target=new{entity=v.Id,property="Volume"},from=0,to=1,duration,easing});
        public static Gameplay.Handle fadeOut(Voice v,double duration,string easing="Linear")=>Tween.to(v.Id,"Volume",0,duration,easing);
        public static Gameplay.Handle crossfade(Voice a,Voice b,double duration,string easing="Linear")=>Gameplay.Start(new{kind="parallel",actions=new object[]{Tween.Action(a.Id,"Volume",0,duration,easing),new{kind="tween",target=new{entity=b.Id,property="Volume"},from=0,to=1,duration,easing}}});
    }
    public static class Events{
        public sealed record Connection(string Token){public void disconnect(){Gameplay.Send(new{command="disconnect",token=Token});callbacksRemove(Token);}private static void callbacksRemove(string token)=>Gameplay.Disconnect(token);}
        private static Connection Connect(string name,Action<JsonElement[]> fn,bool once=false,string? source=null){var token=Gameplay.Register(fn,once);Gameplay.Connections.Add(token);Gameplay.Send(new{command="connect",token,signal=new{name,source},once});return new(token);}
        public static Connection on(string name,Action<object[]> fn)=>Connect(name,a=>fn(a.Select(Ordinary).ToArray()));
        public static Connection once(string name,Action<object[]> fn)=>Connect(name,a=>fn(a.Select(Ordinary).ToArray()),true);
        public static Connection connect(string entity,string name,Action<object[]> fn)=>Connect(name,a=>fn(a.Select(Ordinary).ToArray()),false,Gameplay.Entity(entity));
        public static void emit(string name,object[]? arguments=null,string? source=null)=>Gameplay.Send(new{command="emit",signal=new{name,source=source is null?null:Gameplay.Entity(source)},arguments=arguments??Array.Empty<object>()});
        public static void disconnect(Connection c)=>c.disconnect();
    }
    public static void Run(Action<string,double> callback) {
        string? line;
        while ((line=Console.ReadLine()) is not null) {
            instance.Commands.Clear();
            using var document=JsonDocument.Parse(line);
            rustic.State=document.RootElement;
            Game.scene.State=rustic.State.GetProperty("scene_paths");
            var delta=rustic.State.GetProperty("delta");
            if (rustic.State.GetProperty("callback").GetString()=="update") Gameplay.Dispatch();
            callback(rustic.State.GetProperty("callback").GetString()!, delta.ValueKind==JsonValueKind.Null ? 0 : delta.GetDouble());
            Console.WriteLine(JsonSerializer.Serialize(new {format_version=1, commands=instance.Commands}));
        }
    }
}
sealed class InstanceApi {
    public List<object> Commands { get; } = new();
    public void Add(string source, string? parent = null) => Commands.Add(new { op = "add_instance", source, parent });
    public void Clone(string source, string? parent = null) => Commands.Add(new { op = "clone_instance", source, parent });
    public void add(string source, string? parent = null) => Add(source, parent);
    public void clone(string source, string? parent = null) => Clone(source, parent);
}

sealed class ObjectPath : System.Dynamic.DynamicObject {
    private readonly List<object> commands;
    private readonly string source;
    public ObjectPath(List<object> commands, string source="rustic.game") { this.commands=commands; this.source=source; }
    public override bool TryGetMember(System.Dynamic.GetMemberBinder binder, out object? result) { result=new ObjectPath(commands,source+"."+binder.Name); return true; }
    public ObjectPath this[string name] => new(commands,source+"."+name);
    public object? GetAttribute(string name)=>RusticApi.ConvertValue(Rustic.Query(new{op="object_attribute",source,name}));
    public object? get_attribute(string name)=>GetAttribute(name);
    public Rustic.CaptureHandle startRecording(string? path=null,string source="game")=>Rustic.Game.startRecording(path,source);
    public void stopRecording(Rustic.CaptureHandle? handle=null)=>Rustic.Game.stopRecording(handle);
    public Rustic.CaptureHandle startHotMic()=>Rustic.Game.startHotMic();
    public void stopHotMic(Rustic.CaptureHandle? handle=null)=>Rustic.Game.stopHotMic(handle);
    public void EditAttribute(string name, object? value) => commands.Add(new {op="edit_attribute",source,name,value});
    public void edit_attribute(string name, object? value) => EditAttribute(name,value);
}
sealed class RusticApi {
    public JsonElement State { get; set; }
    public List<object> Commands { get; }
    public dynamic game { get; }
    public RusticApi(List<object> commands) { Commands = commands; game=new ObjectPath(commands); }
    private object? Read(string group, string name) => State.GetProperty(group).TryGetProperty(name, out var v) ? ConvertValue(v) : null;
    internal static object? ConvertValue(JsonElement v) => v.ValueKind switch {
        JsonValueKind.True => true, JsonValueKind.False => false,
        JsonValueKind.String => v.GetString(), JsonValueKind.Number => v.TryGetInt64(out var n) ? (object)n : v.GetDouble(),
        JsonValueKind.Array => v.EnumerateArray().Select(x=>x.GetDouble()).ToArray(), _ => null };
    private RusticActionState Action(string group,string name) => State.GetProperty(group).TryGetProperty(name,out var v) ? JsonSerializer.Deserialize<RusticActionState>(v)! : new();
    public string? find_entity(string source)=>Rustic.Query(new{op="find_entity",source}).GetString();
    public void edit_object_attribute(string source,string name,object? value)=>Commands.Add(new{op="edit_attribute",source,name,value});
    public object? get_object_attribute(string source,string name)=>ConvertValue(Rustic.Query(new{op="object_attribute",source,name}));
    public string entity_id() => State.GetProperty("entity_id").GetString()!;
    public double delta_time() => State.GetProperty("delta_time").GetDouble();
    public double fixed_delta_time() => State.GetProperty("fixed_delta_time").GetDouble();
    public double[] get_translation() => State.GetProperty("translation").EnumerateArray().Select(x => x.GetDouble()).ToArray();
    public void set_translation(double x,double y,double z) => Commands.Add(new { op="set_translation", value=new[]{x,y,z} });
    public object? get_property(string name) => Read("properties", name);
    public void set_property(string name, object value) => Commands.Add(new { op="set_property", name, value });
    public object? GetAttribute(string name) => Read("attributes", name);
    public void EditAttribute(string name, object? value) => Commands.Add(new { op="edit_attribute", name, value });
    public object? get_attribute(string name) => GetAttribute(name);
    public void edit_attribute(string name, object? value) => EditAttribute(name, value);
    public RusticActionState input(string name) => Action("actions", name);
    public RusticActionState key(string name) => Action("keys", name);
    public RusticKeyEvent[] key_events() => JsonSerializer.Deserialize<RusticKeyEvent[]>(State.GetProperty("key_events"))!;
    public bool any_key_pressed() => State.GetProperty("any_key_pressed").GetBoolean();
    public void print(string message)=>log("info",message);
    public void warn(string message)=>log("warn",message);
    public void log(string level,string message) => Commands.Add(new { op="log", level, message });
    public void set_enabled(bool enabled) => Commands.Add(new { op="set_enabled", enabled });
}

sealed class SceneApi {
    public string load(string scene)=>Rustic.Query(new{op="scene_load",scene}).GetString()!;
    public void unload(string scene)=>Rustic.Query(new{op="scene_unload",scene});
    public void switchTo(string scene)=>Rustic.Query(new{op="scene_switch",scene});
    public bool isLoaded(string scene)=>Rustic.Query(new{op="scene_is_loaded",scene}).GetBoolean();
    public string[] getLoaded()=>Rustic.Query(new{op="scene_loaded"}).EnumerateArray().Select(v=>v.GetString()!).ToArray();
    public string? getActive()=>Rustic.Query(new{op="scene_active"}).GetString();
    public JsonElement getEnvironment(string field)=>Rustic.Query(new{op="scene_environment_field",field});
    public JsonElement getEnvironment() => Rustic.Query(new {op="scene_environment"});
    public JsonElement setEnvironment(object settings) => Rustic.Query(new {op="scene_environment", settings});
    public JsonElement setSkyTexture(string path) => setEnvironment(new {sky_image=path});
    public JsonElement State { get; set; }
    public string? Find(string path) => State.TryGetProperty(path, out var value) ? value.GetString() : null;
    public IEnumerable<string> List(string path="Game.scene") => State.EnumerateObject().Where(x => x.Name.StartsWith((string.IsNullOrEmpty(path)?"Game.scene":path)+".")).Select(x => x.Value.GetString()!);
}
sealed class GameApi {
    public Rustic.CaptureHandle startRecording(string? path=null,string source="game")=>new(Rustic.Query(new{op="capture_recording_start",path,source}).GetString()!,true);
    public void stopRecording(Rustic.CaptureHandle? handle=null)=>Rustic.Query(new{op="capture_recording_stop",id=handle?.Id});
    public Rustic.CaptureHandle startHotMic()=>new(Rustic.Query(new{op="capture_mic_start"}).GetString()!,false);
    public void stopHotMic(Rustic.CaptureHandle? handle=null)=>Rustic.Query(new{op="capture_mic_stop",id=handle?.Id});
    public SceneApi scene { get; } = new();
    private readonly List<object> commands;
    public GameApi(List<object> commands) => this.commands = commands;
    public void SetCurrentCamera(string source) => commands.Add(new { op="set_current_camera", source });
    public void setCurrentCamera(string source) => SetCurrentCamera(source);
}


// Canonical API paths.
sealed class CanonicalScenePath : System.Dynamic.DynamicObject {
    private readonly string source;
    public string load(string scene)=>Rustic.Game.scene.load(scene);
    public void unload(string scene)=>Rustic.Game.scene.unload(scene);
    public void switchTo(string scene)=>Rustic.Game.scene.switchTo(scene);
    public bool isLoaded(string scene)=>Rustic.Game.scene.isLoaded(scene);
    public string[] getLoaded()=>Rustic.Game.scene.getLoaded();
    public string? getActive()=>Rustic.Game.scene.getActive();
    public CanonicalScenePath(string source="game.scene") { this.source=source; }
    public CanonicalScenePath this[string name] => new(source+"."+name);
    public override bool TryGetMember(System.Dynamic.GetMemberBinder binder,out object? result) { result=this[binder.Name];return true; }
    public static implicit operator string(CanonicalScenePath path) => path.source;
    public void clone(string? parent=null) => Rustic.instance.clone(source,parent);
    public void add(string? parent=null) => Rustic.instance.add(source,parent);
    public void setCurrentCamera() => Rustic.Game.setCurrentCamera(source);
    public void emit(string name,object[] arguments) => Rustic.Events.emit(name,arguments,source);
    public void EditAttribute(string name,object? value) => Rustic.instance.Commands.Add(new{op="edit_attribute",this.source,name,value});
    public void edit_attribute(string name,object? value) => EditAttribute(name,value);
    public Rustic.Gameplay.Handle tweenTo(string p,object to,Rustic.MotionOptions opts) => Rustic.Tween.to(this.source,p,to,opts);
    public Rustic.Gameplay.Handle tweenTo(string property,object value,double duration,string easing="Linear") => Rustic.Tween.to(this.source,property,value,duration,easing);
    public Rustic.Gameplay.Handle tweenMove(double[] p,Rustic.MotionOptions opts) => Rustic.Tween.move(this.source,p,opts);
    public Rustic.Gameplay.Handle tweenMove(double[] position,double duration,string easing="Linear") => Rustic.Tween.move(this.source,position,duration,easing);
    public Rustic.Gameplay.Handle tweenRotate(double[] p,Rustic.MotionOptions opts) => Rustic.Tween.rotate(this.source,p,opts);
    public Rustic.Gameplay.Handle tweenRotate(double[] rotation,double duration,string easing="Linear") => Rustic.Tween.rotate(this.source,rotation,duration,easing);
    public Rustic.Gameplay.Handle tweenScale(double[] scale,double duration,string easing="Linear") => Rustic.Tween.scale(this.source,scale,duration,easing);
    public Rustic.Gameplay.Handle movementMoveTo(double[] p,Rustic.MotionOptions opts) => Rustic.Movement.moveTo(this.source,p,opts);
    public Rustic.Gameplay.Handle movementMoveTo(double[] p,double d,string ease="Linear") => Rustic.Movement.moveTo(this.source,p,d,ease);
    public Rustic.Gameplay.Handle movementMove(double[] offset,double duration,string easing="Linear") => Rustic.Movement.move(this.source,offset,duration,easing);
    public Rustic.Gameplay.Handle movementRotateTo(double[] p,Rustic.MotionOptions opts) => Rustic.Movement.rotateTo(this.source,p,opts);
    public Rustic.Gameplay.Handle movementRotateTo(double[] q,double d,string ease="Linear") => Rustic.Movement.rotateTo(this.source,q,d,ease);
    public Rustic.Gameplay.Handle movementLookAt(double[] position,double duration=0,string easing="Linear") => Rustic.Movement.lookAt(this.source,position,duration,easing);
    public Rustic.Gameplay.Handle movementFollow(string target,double duration,string easing="Linear",double[]? offset=null) => Rustic.Movement.follow(this.source,target,duration,easing,offset);
    public Rustic.Gameplay.Handle movementOrbit(double[] center,double radius,double turns,double duration,string easing="Linear") => Rustic.Movement.orbit(this.source,center,radius,turns,duration,easing);
    public Rustic.Gameplay.Handle pathFollow(Rustic.Path.Curve path,double duration,string easing="Linear",bool loop=false,bool pingPong=false,bool orientToPath=false) => Rustic.Path.follow(this.source,path,duration,easing,loop,pingPong,orientToPath);
    public Rustic.Gameplay.Handle pathFollowSpeed(Rustic.Path.Curve path,double speed,string easing="Linear",bool loop=false,bool pingPong=false,bool orientToPath=false) => Rustic.Path.followSpeed(this.source,path,speed,easing,loop,pingPong,orientToPath);
    public Rustic.Gameplay.Handle effectsFade(double opacity,double duration,string easing="Linear") => Rustic.Effects.fade(this.source,opacity,duration,easing);
    public Rustic.Gameplay.Handle effectsFlash(double[] color,double d,string ease="Linear") => Rustic.Effects.flash(this.source,color,d,ease);
    public Rustic.Gameplay.Handle effectsShake(double strength,double duration,string easing="Linear") => Rustic.Effects.shake(this.source,strength,duration,easing);
    public Rustic.Gameplay.Handle effectsPulse(double[] scale,double d,string ease="Linear") => Rustic.Effects.pulse(this.source,scale,d,ease);
    public Rustic.Gameplay.Handle cameraMoveTo(double[] p,double duration,string easing="Linear") => Rustic.Camera.moveTo(this.source,p,duration,easing);
    public Rustic.Gameplay.Handle cameraZoom(double fov,double duration,string easing="Linear") => Rustic.Camera.zoom(this.source,fov,duration,easing);
    public Rustic.Gameplay.Handle cameraFov(double v,double d,string ease="Linear") => Rustic.Camera.fov(this.source,v,d,ease);
    public Rustic.Gameplay.Handle cameraFollow(string target,double d,string ease="Linear") => Rustic.Camera.follow(this.source,target,d,ease);
    public Rustic.Gameplay.Handle cameraOrbit(double[] c,double r,double turns,double d,string ease="Linear") => Rustic.Camera.orbit(this.source,c,r,turns,d,ease);
    public Rustic.Gameplay.Handle cameraLookAt(double[] p,double d,string ease="Linear") => Rustic.Camera.lookAt(this.source,p,d,ease);
    public Rustic.Gameplay.Handle cameraShake(double s,double d,string ease="Linear") => Rustic.Camera.shake(this.source,s,d,ease);
    public Rustic.Gameplay.Handle cameraTransition(double[] p,double[] r,double f,double d,string ease="Linear") => Rustic.Camera.transition(this.source,p,r,f,d,ease);
    public void physicsImpulse(double[] vector) => Rustic.Physics.impulse(this.source,vector);
    public void physicsForce(double[] vector,double delta) => Rustic.Physics.force(this.source,vector,delta);
    public void physicsLaunch(double[] vector) => Rustic.Physics.launch(this.source,vector);
    public void physicsKnockback(double[] vector) => Rustic.Physics.knockback(this.source,vector);
    public void animationRegister(Rustic.Animation.Clip clip) => Rustic.Animation.register(this.source,clip);
    public string[] animationClips() => Rustic.Animation.clips(this.source);
    public Rustic.Gameplay.Handle animationPlay(string name,Rustic.Animation.Options? opts=null) => Rustic.Animation.play(this.source,name,opts);
    public Rustic.Gameplay.Handle animationBlend(string from,string to,double duration,string easing="Linear") => Rustic.Animation.blend(this.source,from,to,duration,easing);
    public Rustic.Gameplay.Handle animationTransition(string from,string to,double duration,string easing="Linear") => Rustic.Animation.transition(this.source,from,to,duration,easing);
    public Rustic.Gameplay.Handle animationIk(string middle,string tip,double[] target,double duration=0,string easing="Linear",double weight=1,double[]? pole=null) => Rustic.Animation.ik(this.source,middle,tip,target,duration,easing,weight,pole);
    public Rustic.Gameplay.Handle animationFootPlacement(string middle,string tip,double[] target,double duration=0,string easing="Linear") => Rustic.Animation.footPlacement(this.source,middle,tip,target,duration,easing);
    public Rustic.Gameplay.Handle animationLookAt(double[] target,double duration=0,string easing="Linear") => Rustic.Animation.lookAt(this.source,target,duration,easing);
    public Rustic.Gameplay.Handle animationHeadTracking(double[] target,double duration=0,string easing="Linear") => Rustic.Animation.headTracking(this.source,target,duration,easing);
    public Rustic.Gameplay.Handle animationRecoil(double[] rotation,double duration,string easing="Linear") => Rustic.Animation.recoil(this.source,rotation,duration,easing);
    public void animationAddMarker(string clip,double time,string name) => Rustic.Animation.addMarker(this.source,clip,time,name);
    public Rustic.Events.Connection eventsConnect(string name,Action<object[]> fn) => Rustic.Events.connect(this.source,name,fn);
    public bool tagsAdd(string name) => Rustic.Tags.add(this.source,name);
    public bool tagsRemove(string name) => Rustic.Tags.remove(this.source,name);
    public bool groupsAdd(string name) => Rustic.Groups.add(name,source);
    public void hierarchySetParent(string? parent,bool keep_world=true) => Rustic.Hierarchy.set_parent(this.source,parent,keep_world);
    public string[] hierarchyGetChildren() => Rustic.Hierarchy.get_children(this.source);
    public void hierarchyDetach(bool keep_world=true) => Rustic.Hierarchy.detach(this.source,keep_world);
    public void entityDestroy() => Rustic.Entity.destroy(this.source);
    public void entityDestroyAfter(double delay) => Rustic.Entity.destroy_after(this.source,delay);
    public bool entityIsValid() => Rustic.Entity.is_valid(this.source);
    public bool componentsHas(string name) => Rustic.Components.has(this.source,name);
    public string[] componentsGetAll() => Rustic.Components.get_all(this.source);
    public string[] componentsFindInChildren(string name) => Rustic.Components.find_in_children(this.source,name);
    public Rustic.Gameplay.Handle tween(string p,object to,Rustic.MotionOptions opts) => Rustic.Tween.to(this.source,p,to,opts);
    public Rustic.Gameplay.Handle tween(string property,object value,double duration,string easing="Linear") => Rustic.Tween.to(this.source,property,value,duration,easing);
    public Rustic.Gameplay.Handle moveTo(double[] p,Rustic.MotionOptions opts) => Rustic.Movement.moveTo(this.source,p,opts);
    public Rustic.Gameplay.Handle moveTo(double[] p,double d,string ease="Linear") => Rustic.Movement.moveTo(this.source,p,d,ease);
    public Rustic.Gameplay.Handle move(double[] offset,double duration,string easing="Linear") => Rustic.Movement.move(this.source,offset,duration,easing);
    public Rustic.Gameplay.Handle rotateTo(double[] p,Rustic.MotionOptions opts) => Rustic.Movement.rotateTo(this.source,p,opts);
    public Rustic.Gameplay.Handle rotateTo(double[] q,double d,string ease="Linear") => Rustic.Movement.rotateTo(this.source,q,d,ease);
    public Rustic.Gameplay.Handle lookAt(double[] position,double duration=0,string easing="Linear") => Rustic.Movement.lookAt(this.source,position,duration,easing);
    public Rustic.Gameplay.Handle follow(string target,double duration,string easing="Linear",double[]? offset=null) => Rustic.Movement.follow(this.source,target,duration,easing,offset);
    public Rustic.Gameplay.Handle orbit(double[] center,double radius,double turns,double duration,string easing="Linear") => Rustic.Movement.orbit(this.source,center,radius,turns,duration,easing);
    public Rustic.Gameplay.Handle fade(double opacity,double duration,string easing="Linear") => Rustic.Effects.fade(this.source,opacity,duration,easing);
    public Rustic.Gameplay.Handle flash(double[] color,double d,string ease="Linear") => Rustic.Effects.flash(this.source,color,d,ease);
    public Rustic.Gameplay.Handle shake(double strength,double duration,string easing="Linear") => Rustic.Effects.shake(this.source,strength,duration,easing);
    public Rustic.Gameplay.Handle pulse(double[] scale,double d,string ease="Linear") => Rustic.Effects.pulse(this.source,scale,d,ease);
    public void impulse(double[] vector) => Rustic.Physics.impulse(this.source,vector);
    public void force(double[] vector,double delta) => Rustic.Physics.force(this.source,vector,delta);
    public void launch(double[] vector) => Rustic.Physics.launch(this.source,vector);
    public void knockback(double[] vector) => Rustic.Physics.knockback(this.source,vector);
    public Rustic.Gameplay.Handle playAnimation(string name,Rustic.Animation.Options? opts=null) => Rustic.Animation.play(this.source,name,opts);
    public void destroy() => Rustic.Entity.destroy(this.source);
    public void destroyAfter(double delay) => Rustic.Entity.destroy_after(this.source,delay);
    public bool isValid() => Rustic.Entity.is_valid(this.source);
    public bool addTag(string name) => Rustic.Tags.add(this.source,name);
    public bool removeTag(string name) => Rustic.Tags.remove(this.source,name);
    public void setParent(string? parent,bool keep_world=true) => Rustic.Hierarchy.set_parent(this.source,parent,keep_world);
    public string[] getChildren() => Rustic.Hierarchy.get_children(this.source);
    public void detach(bool keep_world=true) => Rustic.Hierarchy.detach(this.source,keep_world);
    public bool hasComponent(string name) => Rustic.Components.has(this.source,name);
    public string[] getComponents() => Rustic.Components.get_all(this.source);
    public string[] findInChildren(string name) => Rustic.Components.find_in_children(this.source,name);
    public Rustic.Gameplay.Handle followPath(Rustic.Path.Curve path,double duration,string easing="Linear",bool loop=false,bool pingPong=false,bool orientToPath=false) => Rustic.Path.follow(this.source,path,duration,easing,loop,pingPong,orientToPath);
}
sealed class CanonicalFilePath {
    private readonly string filePath;
    public CanonicalFilePath(string filePath="") { this.filePath=filePath; }
    public CanonicalFilePath this[string name] => new((filePath.Length==0?"":filePath+"/")+name);
    public static implicit operator string(CanonicalFilePath path) => path.filePath;
    public Rustic.Audio.Voice audioPlay(double volume=1,double pitch=1,bool loop=false) => Rustic.Audio.play(filePath,volume,pitch,loop);
    public Rustic.Audio.Voice audioPlayAt(double[] position,double volume=1,double pitch=1,bool loop=false) => Rustic.Audio.playAt(filePath,position,volume,pitch,loop);
    public string[] animationLoad(string? e) => Rustic.Animation.load(e,filePath);
    public Rustic.Audio.Voice play(double volume=1,double pitch=1,bool loop=false) => Rustic.Audio.play(filePath,volume,pitch,loop);
    public Rustic.Audio.Voice playAt(double[] position,double volume=1,double pitch=1,bool loop=false) => Rustic.Audio.playAt(filePath,position,volume,pitch,loop);
    public string[] loadAnimation(string? e) => Rustic.Animation.load(e,filePath);
    public void add(string? parent=null) => Rustic.instance.add(filePath,parent);
    public void clone(string? parent=null) => Rustic.instance.clone(filePath,parent);
    public JsonElement setSkyTexture() => Rustic.Game.scene.setSkyTexture(filePath);
}
sealed class CanonicalGlobalApi {
    public CanonicalFilePath files { get; } = new();
    public Rustic.Gameplay.Handle tweenValue(object from,object to,double duration,Action<object> sample,string easing="Linear") => Rustic.Tween.value(from,to,duration,sample,easing);
    public Rustic.Gameplay.Handle timerAfter(double delay,Action fn) => Rustic.Timer.after(delay,fn);
    public Rustic.Gameplay.Handle timerEvery(double interval,Action fn,uint? count=null) => Rustic.Timer.every(interval,fn,count);
    public double smoothLerp(double a,double b,double t,string easing="Linear") => Rustic.Smooth.lerp(a,b,t,easing);
    public double[] smoothLerp(double[] a,double[] b,double t,string easing="Linear") => Rustic.Smooth.lerp(a,b,t,easing);
    public double[] smoothSlerp(double[] a,double[] b,double t,string easing="Linear") => Rustic.Smooth.slerp(a,b,t,easing);
    public double smoothInverseLerp(double a,double b,double value) => Rustic.Smooth.inverseLerp(a,b,value);
    public double smoothRemap(double value,double a,double b,double c,double d) => Rustic.Smooth.remap(value,a,b,c,d);
    public (double value,double velocity) smoothSmoothDamp(double current,double target,double velocity,double smoothTime,double delta) => Rustic.Smooth.smoothDamp(current,target,velocity,smoothTime,delta);
    public Rustic.Physics.Hit? physicsRaycast(double[] origin,double[] direction,double distance) => Rustic.Physics.raycast(origin,direction,distance);
    public Rustic.Physics.Hit? physicsSphereCast(double[] origin,double[] direction,double distance,double radius) => Rustic.Physics.sphereCast(origin,direction,distance,radius);
    public string[] physicsOverlap(double[] center,double radius) => Rustic.Physics.overlap(center,radius);
    public string[] physicsExplosion(double[] center,double radius,double strength) => Rustic.Physics.explosion(center,radius,strength);
    public double clockTimeScale(double? scale=null) => Rustic.Clock.timeScale(scale);
    public void clockPause() => Rustic.Clock.pause();
    public void clockResume() => Rustic.Clock.resume();
    public Rustic.Path.Curve pathCreate(double[][] points,string kind="Linear") => Rustic.Path.create(points,kind);
    public Rustic.Path.Curve pathCreate(Rustic.Path.Point[] points,string kind="Linear") => Rustic.Path.create(points,kind);
    public Rustic.Sequence.Builder sequenceNew() => Rustic.Sequence.New();
    public Rustic.Sequence.Builder sequenceCreate() => Rustic.Sequence.New();
    public Rustic.Sequence.Builder timelineNew() => Rustic.Timeline.New();
    public Rustic.Sequence.Builder timelineCreate() => Rustic.Timeline.New();
    public Rustic.Events.Connection eventsOn(string name,Action<object[]> fn) => Rustic.Events.on(name,fn);
    public Rustic.Events.Connection eventsOnce(string name,Action<object[]> fn) => Rustic.Events.once(name,fn);
    public void eventsEmit(string name,object[]? arguments=null,string? source=null) => Rustic.Events.emit(name,arguments,source);
    public void eventsDisconnect(Rustic.Events.Connection c) => Rustic.Events.disconnect(c);
    public JsonElement audioGetOutputDevices() => Rustic.Audio.get_output_devices();
    public JsonElement audioGetInputDevices() => Rustic.Audio.get_input_devices();
    public void audioSetOutputDevice(string? device=null) => Rustic.Audio.set_output_device(device);
    public void audioSetInputDevice(string? device=null) => Rustic.Audio.set_input_device(device);
    public void audioVolume(Rustic.Audio.Voice v,double value) => Rustic.Audio.volume(v,value);
    public void audioPitch(Rustic.Audio.Voice v,double value) => Rustic.Audio.pitch(v,value);
    public Rustic.Gameplay.Handle audioFadeIn(Rustic.Audio.Voice v,double duration,string easing="Linear") => Rustic.Audio.fadeIn(v,duration,easing);
    public Rustic.Gameplay.Handle audioFadeOut(Rustic.Audio.Voice v,double duration,string easing="Linear") => Rustic.Audio.fadeOut(v,duration,easing);
    public Rustic.Gameplay.Handle audioCrossfade(Rustic.Audio.Voice a,Rustic.Audio.Voice b,double duration,string easing="Linear") => Rustic.Audio.crossfade(a,b,duration,easing);
    public Rustic.Gameplay.Handle animationValue(Rustic.Animation.Key[] keys,Action<object> sample) => Rustic.Animation.value(keys,sample);
    public void animationStop(Rustic.Gameplay.Handle h) => Rustic.Animation.stop(h);
    public void animationPause(Rustic.Gameplay.Handle h) => Rustic.Animation.pause(h);
    public void animationSpeed(Rustic.Gameplay.Handle h,double v) => Rustic.Animation.speed(h,v);
    public void animationLoop(Rustic.Gameplay.Handle h,bool v=true) => Rustic.Animation.loop(h,v);
    public string cameraCurrent() => Rustic.Camera.current();
    public string[] tagsFindAll(string name) => Rustic.Tags.find_all(name);
    public bool groupsCreate(string name) => Rustic.Groups.create(name);
    public string[] groupsGetMembers(string name) => Rustic.Groups.get_members(name);
    public string[] worldFindByTag(string name) => Rustic.World.find_by_tag(name);
    public string[] worldFindInRadius(double[] center,double radius) => Rustic.World.find_in_radius(center,radius);
    public Rustic.Gameplay.Handle after(double delay,Action fn) => Rustic.Timer.after(delay,fn);
    public Rustic.Gameplay.Handle every(double interval,Action fn,uint? count=null) => Rustic.Timer.every(interval,fn,count);
    public double lerp(double a,double b,double t,string easing="Linear") => Rustic.Smooth.lerp(a,b,t,easing);
    public double[] lerp(double[] a,double[] b,double t,string easing="Linear") => Rustic.Smooth.lerp(a,b,t,easing);
    public double[] slerp(double[] a,double[] b,double t,string easing="Linear") => Rustic.Smooth.slerp(a,b,t,easing);
    public double inverseLerp(double a,double b,double value) => Rustic.Smooth.inverseLerp(a,b,value);
    public double remap(double value,double a,double b,double c,double d) => Rustic.Smooth.remap(value,a,b,c,d);
    public (double value,double velocity) smoothDamp(double current,double target,double velocity,double smoothTime,double delta) => Rustic.Smooth.smoothDamp(current,target,velocity,smoothTime,delta);
    public Rustic.Physics.Hit? raycast(double[] origin,double[] direction,double distance) => Rustic.Physics.raycast(origin,direction,distance);
    public Rustic.Physics.Hit? sphereCast(double[] origin,double[] direction,double distance,double radius) => Rustic.Physics.sphereCast(origin,direction,distance,radius);
    public string[] overlap(double[] center,double radius) => Rustic.Physics.overlap(center,radius);
    public string[] explosion(double[] center,double radius,double strength) => Rustic.Physics.explosion(center,radius,strength);
    public double timeScale(double? scale=null) => Rustic.Clock.timeScale(scale);
    public void pause() => Rustic.Clock.pause();
    public void resume() => Rustic.Clock.resume();
    public Rustic.Path.Curve createPath(double[][] points,string kind="Linear") => Rustic.Path.create(points,kind);
    public Rustic.Path.Curve createPath(Rustic.Path.Point[] points,string kind="Linear") => Rustic.Path.create(points,kind);
    public Rustic.Sequence.Builder sequence() => Rustic.Sequence.New();
    public Rustic.Sequence.Builder timeline() => Rustic.Timeline.New();
    public Rustic.Events.Connection on(string name,Action<object[]> fn) => Rustic.Events.on(name,fn);
    public Rustic.Events.Connection once(string name,Action<object[]> fn) => Rustic.Events.once(name,fn);
    public void emit(string name,object[]? arguments=null,string? source=null) => Rustic.Events.emit(name,arguments,source);
    public void disconnect(Rustic.Events.Connection c) => Rustic.Events.disconnect(c);
    public string currentCamera() => Rustic.Camera.current();
    public JsonElement getOutputDevices() => Rustic.Audio.get_output_devices();
    public JsonElement get_output_devices() => Rustic.Audio.get_output_devices();
    public JsonElement getInputDevices() => Rustic.Audio.get_input_devices();
    public JsonElement get_input_devices() => Rustic.Audio.get_input_devices();
    public void setOutputDevice(string? device=null) => Rustic.Audio.set_output_device(device);
    public void set_output_device(string? device=null) => Rustic.Audio.set_output_device(device);
    public void setInputDevice(string? device=null) => Rustic.Audio.set_input_device(device);
    public void set_input_device(string? device=null) => Rustic.Audio.set_input_device(device);
    public JsonElement getEnvironment() => Rustic.Game.scene.getEnvironment();
    public JsonElement setEnvironment(object settings) => Rustic.Game.scene.setEnvironment(settings);
    public void setCurrentCamera(string path) => Rustic.Game.setCurrentCamera(path);
    public string entity_id() => Rustic.rustic.entity_id();
    public double delta_time() => Rustic.rustic.delta_time();
    public double fixed_delta_time() => Rustic.rustic.fixed_delta_time();
    public double[] get_translation() => Rustic.rustic.get_translation();
    public void set_translation(double x,double y,double z) => Rustic.rustic.set_translation(x,y,z);
    public object? get_property(string name) => Rustic.rustic.get_property(name);
    public void set_property(string name, object value) => Rustic.rustic.set_property(name,value);
    public object? GetAttribute(string name) => Rustic.rustic.GetAttribute(name);
    public void EditAttribute(string name, object? value) => Rustic.rustic.EditAttribute(name,value);
    public object? get_attribute(string name) => Rustic.rustic.get_attribute(name);
    public void edit_attribute(string name, object? value) => Rustic.rustic.edit_attribute(name,value);
    public RusticActionState input(string name) => Rustic.rustic.input(name);
    public RusticActionState key(string name) => Rustic.rustic.key(name);
    public RusticKeyEvent[] key_events() => Rustic.rustic.key_events();
    public bool any_key_pressed() => Rustic.rustic.any_key_pressed();
    public void log(string level,string message) => Rustic.rustic.log(level,message);
    public void set_enabled(bool enabled) => Rustic.rustic.set_enabled(enabled);
}
sealed class CanonicalGame {
    public CanonicalScenePath scene { get; } = new();
    public CanonicalGlobalApi rustic { get; } = new();
}
