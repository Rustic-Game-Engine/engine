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
    internal static JsonElement Query(object query){
        Console.WriteLine(JsonSerializer.Serialize(new{query,commands=instance.Commands.ToArray()}));instance.Commands.Clear();
        using var response=JsonDocument.Parse(Console.ReadLine()??throw new InvalidOperationException("query host exited"));
        if(response.RootElement.TryGetProperty("error",out var error))throw new InvalidOperationException(error.GetString());
        return response.RootElement.GetProperty("result").Clone();
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
    public static class Physics{
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
        internal static string Entity(string? id)=>id is null?rustic.entity_id():Game.scene.Find(id)??id;
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
    public static class Audio{
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
    public void EditAttribute(string name, object? value) => commands.Add(new {op="edit_attribute",source,name,value});
    public void edit_attribute(string name, object? value) => EditAttribute(name,value);
}
sealed class RusticApi {
    public JsonElement State { get; set; }
    public List<object> Commands { get; }
    public dynamic game { get; }
    public RusticApi(List<object> commands) { Commands = commands; game=new ObjectPath(commands); }
    private object? Read(string group, string name) => State.GetProperty(group).TryGetProperty(name, out var v) ? ConvertValue(v) : null;
    private static object? ConvertValue(JsonElement v) => v.ValueKind switch {
        JsonValueKind.True => true, JsonValueKind.False => false,
        JsonValueKind.String => v.GetString(), JsonValueKind.Number => v.TryGetInt64(out var n) ? (object)n : v.GetDouble(),
        JsonValueKind.Array => v.EnumerateArray().Select(x=>x.GetDouble()).ToArray(), _ => null };
    private RusticActionState Action(string group,string name) => State.GetProperty(group).TryGetProperty(name,out var v) ? JsonSerializer.Deserialize<RusticActionState>(v)! : new();
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
    public void log(string level,string message) => Commands.Add(new { op="log", level, message });
    public void set_enabled(bool enabled) => Commands.Add(new { op="set_enabled", enabled });
}

sealed class SceneApi {
    public JsonElement State { get; set; }
    public string? Find(string path) => State.TryGetProperty(path, out var value) ? value.GetString() : null;
    public IEnumerable<string> List(string path="Game.scene") => State.EnumerateObject().Where(x => path=="Game.scene" || x.Name.StartsWith(path+".")).Select(x => x.Value.GetString()!);
}
sealed class GameApi {
    public SceneApi scene { get; } = new();
    private readonly List<object> commands;
    public GameApi(List<object> commands) => this.commands = commands;
    public void SetCurrentCamera(string source) => commands.Add(new { op="set_current_camera", source });
    public void setCurrentCamera(string source) => SetCurrentCamera(source);
}

