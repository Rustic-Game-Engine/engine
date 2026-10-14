import java.io.*;
import java.nio.charset.StandardCharsets;
import java.util.*;

/* Engine-owned SDK: scripts receive ordinary Java values, never transport objects. */
class Rustic {
    static BufferedReader input;
    static PrintWriter output;
    static Map<?,?> state;
    static final List<Object> commands = new ArrayList<>();
    static final RusticApi rustic = new RusticApi();
    static final InstanceApi instance = new InstanceApi();
    static final GameApi Game = new GameApi();
    static final CanonicalGame game = new CanonicalGame();

    static class Tags {
        static boolean add(String entity,String name){return (Boolean)query(args("tags_add","entity",Gameplay.entity(entity),"name",name));}
        static boolean remove(String entity,String name){return (Boolean)query(args("tags_remove","entity",Gameplay.entity(entity),"name",name));}
        static String[] find_all(String name){return ((List<?>)query(args("tags_find_all","name",name))).stream().map(v->(String)v).toArray(String[]::new);}
    }
    static class Groups {
        static boolean create(String name){return (Boolean)query(args("groups_create","name",name));}
        static boolean add(String name,String entity){return (Boolean)query(args("groups_add","name",name,"entity",Gameplay.entity(entity)));}
        static String[] get_members(String name){return ((List<?>)query(args("groups_get_members","name",name))).stream().map(v->(String)v).toArray(String[]::new);}
    }
    static class Hierarchy {
        static void set_parent(String entity,String parent,boolean keep_world){query(args("hierarchy_set_parent","entity",Gameplay.entity(entity),"parent",(parent==null?null:Gameplay.entity(parent)),"keep_world",keep_world));}
        static void set_parent(String entity,String parent){set_parent(entity,parent,true);}
        static String[] get_children(String entity){return ((List<?>)query(args("hierarchy_get_children","entity",Gameplay.entity(entity)))).stream().map(v->(String)v).toArray(String[]::new);}
        static void detach(String entity,boolean keep_world){query(args("hierarchy_detach","entity",Gameplay.entity(entity),"keep_world",keep_world));}
        static void detach(String entity){detach(entity,true);}
    }
    static class Entity {
        static void destroy(String entity){query(args("entity_destroy","entity",Gameplay.entity(entity)));}
        static void destroy_after(String entity,double delay){query(args("entity_destroy_after","entity",Gameplay.entity(entity),"delay",delay));}
        static boolean is_valid(String entity){return (Boolean)query(args("entity_is_valid","entity",Gameplay.entity(entity)));}
    }
    static class Components {
        static boolean has(String entity,String name){return (Boolean)query(args("components_has","entity",Gameplay.entity(entity),"name",name));}
        static String[] get_all(String entity){return ((List<?>)query(args("components_get_all","entity",Gameplay.entity(entity)))).stream().map(v->(String)v).toArray(String[]::new);}
        static String[] find_in_children(String entity,String name){return ((List<?>)query(args("components_find_in_children","entity",Gameplay.entity(entity),"name",name))).stream().map(v->(String)v).toArray(String[]::new);}
    }
    static class World {
        static String[] find_by_tag(String name){return ((List<?>)query(args("world_find_by_tag","name",name))).stream().map(v->(String)v).toArray(String[]::new);}
        static String[] find_in_radius(double[] center,double radius){return ((List<?>)query(args("world_find_in_radius","center",center,"radius",radius))).stream().map(v->(String)v).toArray(String[]::new);}
    }

    interface Callback { void invoke(String callback, double dt) throws Exception; }
    static void run(Callback callback) throws Exception {
        input = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        output = new PrintWriter(new OutputStreamWriter(System.out, StandardCharsets.UTF_8), true);
        String line;
        while ((line=input.readLine())!=null) {
            state=(Map<?,?>)new Parser(line).parse(); commands.clear();
            if ("update".equals(state.get("callback"))) Gameplay.dispatch();
            Number delta=(Number)state.get("delta");
            callback.invoke((String)state.get("callback"),delta==null?0:delta.doubleValue());
            output.println(encode(Map.of("format_version",1,"commands",commands)));
        }
    }
    static Object query(Map<String,Object> request){
        try{output.println(encode(Map.of("query",request,"commands",new ArrayList<>(commands))));commands.clear();
            var response=(Map<?,?>)new Parser(input.readLine()).parse();if(response.containsKey("error"))throw new IllegalArgumentException(response.get("error").toString());return response.get("result");
        }catch(IOException e){throw new IllegalStateException(e);}
    }
    static Map<String,Object> args(String op,Object... pairs){var m=new LinkedHashMap<String,Object>();m.put("op",op);for(int i=0;i<pairs.length;i+=2)m.put((String)pairs[i],pairs[i+1]);return m;}
    static class Smooth{
        static Object lerp(Object a,Object b,double t){return lerp(a,b,t,"Linear");}
        static Object lerp(Object a,Object b,double t,String ease){return value(query(args("lerp","from",a,"to",b,"progress",t,"easing",ease)));}
        static double[] slerp(double[] a,double[] b,double t){return slerp(a,b,t,"Linear");}
        static double[] slerp(double[] a,double[] b,double t,String ease){return vector(query(args("slerp","from",a,"to",b,"progress",t,"easing",ease)));}
        static double inverseLerp(double a,double b,double v){return ((Number)query(args("inverse_lerp","from",a,"to",b,"value",v))).doubleValue();}
        static double remap(double v,double a,double b,double c,double d){return ((Number)query(args("remap","value",v,"in_min",a,"in_max",b,"out_min",c,"out_max",d))).doubleValue();}
        static double[] smoothDamp(double c,double t,double v,double s,double dt){var r=(Map<?,?>)query(args("smooth_damp","current",c,"target",t,"velocity",v,"smooth_time",s,"delta",dt));return new double[]{((Number)r.get("value")).doubleValue(),((Number)r.get("velocity")).doubleValue()};}
    }
    static class Interpolation extends Smooth {}
    static class Physics{
        static Object raycast(double[] origin,double[] direction,double distance){return query(args("physics_raycast","origin",origin,"direction",direction,"distance",distance));}
        static Object sphereCast(double[] origin,double[] direction,double distance,double radius){return query(args("physics_sphere_cast","origin",origin,"direction",direction,"distance",distance,"radius",radius));}
        static Object overlap(double[] center,double radius){return query(args("physics_overlap","center",center,"radius",radius));}
        static void impulse(String e,double[] v){query(args("physics_impulse","entity",Gameplay.entity(e),"vector",v));}
        static void launch(String e,double[] v){query(args("physics_launch","entity",Gameplay.entity(e),"vector",v));}
        static void force(String e,double[] v,double dt){query(args("physics_force","entity",Gameplay.entity(e),"vector",v,"delta",dt));}
        static void knockback(String e,double[] v){impulse(e,v);}
        static Object explosion(double[] center,double radius,double strength){return query(args("physics_explosion","center",center,"radius",radius,"strength",strength));}
    }
    static Object field(String group,String name) { return ((Map<?,?>)state.get(group)).get(name); }
    static double[] vector(Object value) { var a=(List<?>)value; var out=new double[a.size()];for(int i=0;i<out.length;i++)out[i]=((Number)a.get(i)).doubleValue();return out; }
    static Object value(Object v) { return v instanceof List<?> ? vector(v) : v; }
    static void command(String op,Object... pairs) {Map<String,Object> c=new LinkedHashMap<>();c.put("op",op);for(int i=0;i<pairs.length;i+=2)c.put((String)pairs[i],pairs[i+1]);commands.add(c);}
    static class ActionState { final boolean pressed,released,held;final double axis;
        ActionState(Object v) {var m=v instanceof Map<?,?>?(Map<?,?>)v:Map.of();pressed=Boolean.TRUE.equals(m.get("pressed"));released=Boolean.TRUE.equals(m.get("released"));held=Boolean.TRUE.equals(m.get("held"));axis=m.get("axis") instanceof Number?((Number)m.get("axis")).doubleValue():0;}
    }
    static class KeyEvent { final String key,state;final boolean repeat;KeyEvent(Map<?,?> m){key=(String)m.get("key");state=(String)m.get("state");repeat=Boolean.TRUE.equals(m.get("repeat"));} }
    static class ObjectPath {
        final String source;
        ObjectPath(String source){this.source=source;}
        ObjectPath scene(String name){return new ObjectPath(source+"."+name);}
        ObjectPath object(String path){return new ObjectPath(source+"."+path);}
        Object GetAttribute(String name){return value(query(args("object_attribute","source",source,"name",name)));}
        Object get_attribute(String name){return GetAttribute(name);}
        CaptureHandle startRecording(){return Game.startRecording();}
        CaptureHandle startRecording(String path){return Game.startRecording(path);}
        CaptureHandle startRecording(String path,String source){return Game.startRecording(path,source);}
        void stopRecording(CaptureHandle handle){Game.stopRecording(handle);}
        CaptureHandle startHotMic(){return Game.startHotMic();}
        void stopHotMic(CaptureHandle handle){Game.stopHotMic(handle);}
        void EditAttribute(String name,Object value){command("edit_attribute","source",source,"name",name,"value",value);}
        void edit_attribute(String name,Object value){EditAttribute(name,value);}
    }
    static class Gameplay {
        static final Map<String,java.util.function.Consumer<Object[]>> callbacks=new LinkedHashMap<>();
        static final Set<String> once=new HashSet<>(),connections=new HashSet<>();
        static final Map<String,Handle> handles=new LinkedHashMap<>();
        static String entity(String id){return id==null?rustic.entity_id():id.startsWith("game.scene.")?(String)query(args("scene_object","source",id)):Optional.ofNullable(Game.scene.Find(id)).orElse(id);}
        static String register(java.util.function.Consumer<Object[]> fn,boolean single){String id=UUID.randomUUID().toString();callbacks.put(id,fn);if(single)once.add(id);return id;}
        static void send(Object request){command("gameplay","request",request);}
        static void release(Handle h){for(String token:h.tokens){callbacks.remove(token);once.remove(token);}handles.remove(h.id);callbacks.remove(h.id);once.remove(h.id);}
        static void collect(Object a,List<String> tokens){if(a instanceof Map<?,?>){var m=(Map<?,?>)a;if(m.get("token") instanceof String)tokens.add((String)m.get("token"));if(m.get("marker_token") instanceof String)tokens.add((String)m.get("marker_token"));if(m.get("actions") instanceof Iterable<?>)for(Object child:(Iterable<?>)m.get("actions"))collect(child,tokens);}}
        static Handle start(Object action){return start(action,false,0,false);}
        static Handle start(Object action,boolean loop,int repeats,boolean pingPong){var h=new Handle(UUID.randomUUID().toString());handles.put(h.id,h);collect(action,h.tokens);
            callbacks.put(h.id,args->{release(h);if(h.finished!=null)h.finished.run();});once.add(h.id);
            send(Map.of("command","start","handle",h.id,"action",action,"on_finished",h.id,"playback",Map.of("looping",loop,"repeats",repeats,"ping_pong",pingPong)));return h;}
        static void dispatch(){var live=new HashSet<>((List<?>)state.get("gameplay_connections"));for(String token:new ArrayList<>(connections))if(!live.contains(token)){callbacks.remove(token);once.remove(token);connections.remove(token);}var existing=new ArrayList<>(handles.values());
            for(Object v:(List<?>)state.get("gameplay_callbacks")){var d=(Map<?,?>)v;String token=(String)d.get("token");var fn=callbacks.get(token);if(fn!=null){if(once.remove(token))callbacks.remove(token);fn.accept(((List<?>)d.get("arguments")).toArray());}}
            var states=new LinkedHashMap<String,Map<?,?>>();for(Object v:(List<?>)state.get("gameplay_states")){var m=(Map<?,?>)v;states.put((String)m.get("handle"),m);}
            for(var h:existing){var m=states.get(h.id);if(m==null||"failed".equals(m.get("status"))||"cancelled".equals(m.get("status"))){release(h);if(m!=null&&m.get("error")!=null)rustic.log("error",m.get("error").toString());}}
        }
        record OperationInfo(String status,String error){}
        static class Handle {
            OperationInfo state(){var r=(Map<?,?>)query(args("operation_state","handle",id));return r==null?null:new OperationInfo((String)r.get("status"),(String)r.get("error"));}
            final String id;Runnable finished;final List<String> tokens=new ArrayList<>();final Map<String,Runnable> markers=new LinkedHashMap<>();
            Handle(String id){this.id=id;}
            Handle onFinished(Runnable fn){finished=fn;return this;}
            Handle onMarker(String name,Runnable fn){markers.put(name,fn);return this;}
            Handle control(String cmd){send(Map.of("command",cmd,"handle",id));if("cancel".equals(cmd))release(this);return this;}
            Handle pause(){return control("pause");}Handle resume(){return control("resume");}Handle cancel(){return control("cancel");}Handle reverse(){return control("reverse");}Handle stop(){return cancel();}
            Handle speed(double speed){send(Map.of("command","speed","handle",id,"speed",speed));return this;}
            Handle loop(boolean looping){send(Map.of("command","loop","handle",id,"looping",looping));return this;}
        }
    }
    static class Timer {
        static Gameplay.Handle after(double delay,Runnable fn){return Gameplay.start(Map.of("kind","wait","duration",delay)).onFinished(fn);}
        static Gameplay.Handle every(double interval,Runnable fn){return every(interval,fn,null);}
        static Gameplay.Handle every(double interval,Runnable fn,Integer count){if(interval<=0||(count!=null&&count<1))throw new IllegalArgumentException("positive interval/count required");String token=Gameplay.register(args->fn.run(),false);return Gameplay.start(Map.of("kind","sequence","actions",List.of(Map.of("kind","wait","duration",interval),Map.of("kind","callback","token",token))),count==null,count==null?0:count-1,false);}
    }
    record MotionOptions(Double duration,Double speed,String easing){MotionOptions{if((duration==null)==(speed==null))throw new IllegalArgumentException("choose duration or speed");}static MotionOptions speed(double v,String ease){return new MotionOptions(null,v,ease);}static MotionOptions duration(double v,String ease){return new MotionOptions(v,null,ease);}}
    static class Tween {
        static Map<String,Object> action(String e,String p,Object to,MotionOptions opts){return args("","kind","tween","target",Map.of("entity",Gameplay.entity(e),"property",p),"to",to,"duration",opts.duration(),"speed",opts.speed(),"easing",opts.easing());}
        static Gameplay.Handle to(String e,String p,Object to,MotionOptions opts){return Gameplay.start(action(e,p,to,opts));}
        static Gameplay.Handle move(String e,double[] p,MotionOptions opts){return to(e,"Position",p,opts);}
        static Gameplay.Handle rotate(String e,double[] p,MotionOptions opts){return to(e,"Rotation",p,opts);}
        static Gameplay.Handle value(Object from,Object to,double duration,java.util.function.Consumer<Object> sample){return value(from,to,duration,sample,"Linear");}
        static Gameplay.Handle value(Object from,Object to,double duration,java.util.function.Consumer<Object> sample,String easing){return Gameplay.start(Map.of("kind","value","from",from,"to",to,"duration",duration,"easing",easing,"token",Gameplay.register(a->sample.accept(Rustic.value(a[0])),false)));}
        static Map<String,Object> action(String e,String property,Object to,double duration,String easing){return args("","kind","tween","target",Map.of("entity",Gameplay.entity(e),"property",property),"to",to,"duration",duration,"easing",easing);}
        static Gameplay.Handle to(String e,String property,Object value,double duration){return to(e,property,value,duration,"Linear");}
        static Gameplay.Handle to(String e,String property,Object value,double duration,String easing){return Gameplay.start(action(e,property,value,duration,easing));}
        static Gameplay.Handle move(String e,double[] p,double duration){return move(e,p,duration,"Linear");}
        static Gameplay.Handle move(String e,double[] p,double duration,String easing){return to(e,"Position",p,duration,easing);}
        static Gameplay.Handle rotate(String e,double[] q,double duration){return rotate(e,q,duration,"Linear");}
        static Gameplay.Handle rotate(String e,double[] q,double duration,String easing){return to(e,"Rotation",q,duration,easing);}
        static Gameplay.Handle scale(String e,double[] p,double duration){return scale(e,p,duration,"Linear");}
        static Gameplay.Handle scale(String e,double[] p,double duration,String easing){return to(e,"Scale",p,duration,easing);}
    }
    static class Movement {
        static Gameplay.Handle moveTo(String e,double[] p,MotionOptions opts){return Tween.move(e,p,opts);}
        static Gameplay.Handle rotateTo(String e,double[] p,MotionOptions opts){return Tween.rotate(e,p,opts);}
        static Gameplay.Handle move(String e,double[] offset,double duration){return move(e,offset,duration,"Linear");}
        static Gameplay.Handle move(String e,double[] offset,double duration,String easing){return Gameplay.start(Map.of("kind","move","entity",Gameplay.entity(e),"offset",offset,"duration",duration,"easing",easing));}
        static Gameplay.Handle lookAt(String e,double[] position,double duration){return lookAt(e,position,duration,"Linear");}
        static Gameplay.Handle lookAt(String e,double[] position,double duration,String easing){return Gameplay.start(Map.of("kind","look_at","entity",Gameplay.entity(e),"position",position,"duration",duration,"easing",easing));}
        static Gameplay.Handle follow(String e,String target,double duration,String easing,double[] offset){return Gameplay.start(Map.of("kind","follow","entity",Gameplay.entity(e),"target",Gameplay.entity(target),"duration",duration,"easing",easing,"offset",offset));}
        static Gameplay.Handle orbit(String e,double[] center,double radius,double turns,double duration){return orbit(e,center,radius,turns,duration,"Linear");}
        static Gameplay.Handle orbit(String e,double[] center,double radius,double turns,double duration,String easing){return Gameplay.start(Map.of("kind","orbit","entity",Gameplay.entity(e),"center",center,"radius",radius,"turns",turns,"duration",duration,"easing",easing));}
        static Gameplay.Handle moveTo(String e,double[] p,double d){return moveTo(e,p,d,"Linear");}
        static Gameplay.Handle moveTo(String e,double[] p,double d,String easing){return Tween.move(e,p,d,easing);}
        static Gameplay.Handle rotateTo(String e,double[] q,double d){return rotateTo(e,q,d,"Linear");}
        static Gameplay.Handle rotateTo(String e,double[] q,double d,String easing){return Tween.rotate(e,q,d,easing);}
    }
    static class Sequence {
        static Builder create(){return new Builder();}
        static class Builder {
            final List<Object> actions=new ArrayList<>();
            Builder move(String e,double[] p,double d,String ease){actions.add(Tween.action(e,"Position",p,d,ease));return this;}
            Builder to(String e,String p,Object v,double d,String ease){actions.add(Tween.action(e,p,v,d,ease));return this;}
            Builder call(Runnable fn){actions.add(Map.of("kind","callback","token",Gameplay.register(args->fn.run(),true)));return this;}
            Builder animation(String e,String name,Animation.Options opts){actions.add(Animation.action(e,name,opts,null,null));return this;}
            Builder parallel(Builder... branches){actions.add(Map.of("kind","parallel","actions",Arrays.stream(branches).map(b->Map.of("kind","sequence","actions",new ArrayList<>(b.actions))).toList()));return this;}
            Builder waitFor(double duration){actions.add(Map.of("kind","wait","duration",duration));return this;}
            Gameplay.Handle play(){return Gameplay.start(Map.of("kind","sequence","actions",new ArrayList<>(actions)));}
        }
    }
    static class Timeline{static Sequence.Builder create(){return Sequence.create();}}
    static class Path{
        record Point(double[] point,String easing){}
        record Curve(String kind,List<Point> points){}
        static Curve create(double[][] points,String kind){return new Curve(kind,Arrays.stream(points).map(p->new Point(p,"Linear")).toList());}
        static Curve create(List<Point> points,String kind){return new Curve(kind,points);}
        static Gameplay.Handle follow(String e,Curve path,double duration,String easing,boolean loop,boolean pingPong,boolean orientToPath){return Gameplay.start(Map.of("kind","path","entity",Gameplay.entity(e),"path",path,"duration",duration,"easing",easing,"orient_to_path",orientToPath),loop,0,pingPong);}
        static Gameplay.Handle followSpeed(String e,Curve path,double speed,String easing,boolean loop,boolean pingPong,boolean orientToPath){return Gameplay.start(Map.of("kind","path","entity",Gameplay.entity(e),"path",path,"speed",speed,"easing",easing,"orient_to_path",orientToPath),loop,0,pingPong);}
    }
    static class Effects{
        static Gameplay.Handle fade(String e,double opacity,double duration){return fade(e,opacity,duration,"Linear");}
        static Gameplay.Handle fade(String e,double opacity,double duration,String easing){return Tween.to(e,"Opacity",opacity,duration,easing);}
        static Gameplay.Handle flash(String e,double[] color,double duration){return flash(e,color,duration,"Linear");}
        static Gameplay.Handle flash(String e,double[] color,double duration,String easing){return Gameplay.start(Tween.action(e,"Color",color,duration,easing),false,1,true);}
        static Gameplay.Handle pulse(String e,double[] scale,double duration){return pulse(e,scale,duration,"Linear");}
        static Gameplay.Handle pulse(String e,double[] scale,double duration,String easing){return Gameplay.start(Tween.action(e,"Scale",scale,duration,easing),false,1,true);}
        static Gameplay.Handle shake(String e,double strength,double duration){return shake(e,strength,duration,"Linear");}
        static Gameplay.Handle shake(String e,double strength,double duration,String easing){return Gameplay.start(Map.of("kind","shake","entity",Gameplay.entity(e),"strength",strength,"duration",duration,"easing",easing));}
    }
    static class Clock { static Object state(){return query(args("clock"));} static double timeScale(){return ((Number)((Map<?,?>)state()).get("scale")).doubleValue();} static double timeScale(double scale){return ((Number)((Map<?,?>)query(args("clock","scale",scale))).get("scale")).doubleValue();} static void pause(){query(args("clock","paused",true));} static void resume(){query(args("clock","paused",false));} }
    static class Camera{
        static String current(){return (String)query(args("camera_current"));}
        static Gameplay.Handle moveTo(String e,double[] p,double d){return moveTo(e,p,d,"Linear");}
        static Gameplay.Handle moveTo(String e,double[] p,double d,String ease){return Tween.move(e,p,d,ease);}
        static Gameplay.Handle zoom(String e,double f,double d){return zoom(e,f,d,"Linear");}
        static Gameplay.Handle zoom(String e,double f,double d,String ease){return Tween.to(e,"Fov",f,d,ease);}
        static Gameplay.Handle fov(String e,double f,double d){return fov(e,f,d,"Linear");}
        static Gameplay.Handle fov(String e,double f,double d,String ease){return zoom(e,f,d,ease);}
        static Gameplay.Handle lookAt(String e,double[] p,double d){return lookAt(e,p,d,"Linear");}
        static Gameplay.Handle lookAt(String e,double[] p,double d,String ease){return Movement.lookAt(e,p,d,ease);}
        static Gameplay.Handle follow(String e,String target,double d){return follow(e,target,d,"Linear");}
        static Gameplay.Handle follow(String e,String target,double d,String ease){return Movement.follow(e,target,d,ease,new double[3]);}
        static Gameplay.Handle orbit(String e,double[] c,double r,double turns,double d){return orbit(e,c,r,turns,d,"Linear");}
        static Gameplay.Handle orbit(String e,double[] c,double r,double turns,double d,String ease){return Movement.orbit(e,c,r,turns,d,ease);}
        static Gameplay.Handle shake(String e,double s,double d){return shake(e,s,d,"Linear");}
        static Gameplay.Handle shake(String e,double s,double d,String ease){return Effects.shake(e,s,d,ease);}
        static Gameplay.Handle transition(String e,double[] p,double[] r,double f,double d){return transition(e,p,r,f,d,"Linear");}
        static Gameplay.Handle transition(String e,double[] p,double[] r,double f,double d,String ease){return Gameplay.start(Map.of("kind","parallel","actions",List.of(Tween.action(e,"Position",p,d,ease),Tween.action(e,"Rotation",r,d,ease),Tween.action(e,"Fov",f,d,ease))));}
    }
    static class Animation{
        static void addMarker(String e,String clip,double time,String name){query(args("animation_marker","entity",Gameplay.entity(e),"clip",clip,"time",time,"name",name));}
        static Gameplay.Handle value(List<Key> keys,java.util.function.Consumer<Object> sample){return Gameplay.start(query(args("keyframes","keys",keys,"token",Gameplay.register(a->sample.accept(Rustic.value(a[0])),false))));}
        record Key(double time,Object value,String easing){Key(double time,Object value){this(time,value,"Linear");}}
        record Track(String target,List<Key> keys){}
        record Marker(double time,String name){}
        record Clip(String name,double duration,List<Track> tracks,List<Marker> markers){}
        record Options(double speed,double blendIn,double blendOut,String easing,String progressionEase,boolean loop,double weight,boolean additive,List<String> mask,String blendInEase,String blendOutEase){
            Options(){this(1,0,0,"Linear",null,false,1,false,List.of(),null,null);}
            Options(double speed,double blendIn,double blendOut,String easing,String progressionEase,boolean loop,double weight,boolean additive,List<String> mask){this(speed,blendIn,blendOut,easing,progressionEase,loop,weight,additive,mask,null,null);}
        }
        static Object load(String e,String source){return query(args("animation_load","entity",Gameplay.entity(e),"source",source));}
        static void register(String e,Clip clip){query(args("animation_register","entity",Gameplay.entity(e),"clip",clip));}
        static Object clips(String e){return query(args("animation_list","entity",Gameplay.entity(e)));}
        static Map<String,Object> action(String e,String name,Options opts,String marker,Object source){if(opts==null)opts=new Options();var m=args("","kind","animation_ref","entity",Gameplay.entity(e),"clip",query(args("animation_ref","entity",Gameplay.entity(e),"name",name)),"options",args("","speed",opts.speed(),"blend_in",opts.blendIn(),"blend_out",opts.blendOut(),"blend_in_ease",opts.blendInEase()==null?opts.easing():opts.blendInEase(),"blend_out_ease",opts.blendOutEase()==null?opts.easing():opts.blendOutEase(),"progression_ease",opts.progressionEase(),"weight",opts.weight(),"additive",opts.additive(),"layered",!opts.mask().isEmpty()),"mask",opts.mask());if(marker!=null)m.put("marker_token",marker);if(source!=null)m.put("blend_source",source);return m;}
        static Gameplay.Handle play(String e,String name){return play(e,name,null);}
        static Gameplay.Handle play(String e,String name,Options opts){var holder=new Gameplay.Handle[1];String token=Gameplay.register(a->{var fn=holder[0].markers.get(a[0].toString());if(fn!=null)fn.run();},false);holder[0]=Gameplay.start(action(e,name,opts,token,null),opts!=null&&opts.loop(),0,false);return holder[0];}
        static Gameplay.Handle blend(String e,String from,String to,double duration){return blend(e,from,to,duration,"Linear");}
        static Gameplay.Handle blend(String e,String from,String to,double duration,String ease){return Gameplay.start(action(e,to,new Options(1,duration,0,ease,null,false,1,false,List.of()),null,query(args("animation_ref","entity",Gameplay.entity(e),"name",from))));}
        static Gameplay.Handle transition(String e,String from,String to,double duration){return transition(e,from,to,duration,"Linear");}
        static Gameplay.Handle transition(String e,String from,String to,double duration,String ease){return blend(e,from,to,duration,ease);}
        static void stop(Gameplay.Handle h){h.cancel();}static void pause(Gameplay.Handle h){h.pause();}static void speed(Gameplay.Handle h,double v){h.speed(v);}static void loop(Gameplay.Handle h,boolean v){h.loop(v);}
        static Gameplay.Handle ik(String root,String middle,String tip,double[] target,double duration,String easing,double weight,double[] pole){return Gameplay.start(query(args("animation_ik","root",Gameplay.entity(root),"middle",Gameplay.entity(middle),"tip",Gameplay.entity(tip),"target",target,"duration",duration,"easing",easing,"weight",weight,"pole",pole)));}
        static Gameplay.Handle footPlacement(String root,String middle,String tip,double[] target,double duration){return footPlacement(root,middle,tip,target,duration,"Linear");}
        static Gameplay.Handle footPlacement(String root,String middle,String tip,double[] target,double duration,String ease){return ik(root,middle,tip,target,duration,ease,1,new double[]{0,0,1});}
        static Gameplay.Handle lookAt(String e,double[] target,double d,String ease){return Movement.lookAt(e,target,d,ease);}
        static Gameplay.Handle lookAt(String e,double[] target,double d){return lookAt(e,target,d,"Linear");}
        static Gameplay.Handle headTracking(String e,double[] target,double duration){return headTracking(e,target,duration,"Linear");}
        static Gameplay.Handle headTracking(String e,double[] target,double duration,String ease){return Movement.lookAt(e,target,duration,ease);}
        static Gameplay.Handle recoil(String e,double[] rotation,double duration){return recoil(e,rotation,duration,"Linear");}
        static Gameplay.Handle recoil(String e,double[] rotation,double duration,String ease){return Gameplay.start(Tween.action(e,"Rotation",rotation,duration,ease),false,1,true);}
    }
    static class CaptureHandle {
        final String id; final boolean recording;
        CaptureHandle(String id,boolean recording){this.id=id;this.recording=recording;}
        Object read(){return query(args("capture_read","id",id));}
        void unload(){query(args("capture_unload","id",id));}
        void stop(){query(args(recording?"capture_recording_stop":"capture_mic_stop","id",id));}
    }
    static class Audio{
        static class LoadedAudio {
            final String source; String id;
            LoadedAudio(String source){this.source=source;}
            LoadedAudio load(){if(id==null)id=(String)query(args("audio_load","source",source));return this;}
            String loaded(){if(id==null)throw new IllegalStateException("audio is not loaded; call load() before play()");return id;}
            Voice play(){return play(1,1,false,null);}
            Voice play(double volume,double pitch,boolean loop){return play(volume,pitch,loop,null);}
            Voice play(double volume,double pitch,boolean loop,double[] position){return new Voice((String)query(args("audio_loaded_play","entity",loaded(),"volume",volume,"pitch",pitch,"loop",loop,"position",position)));}
            void stop(){query(args("audio_loaded_stop","entity",loaded()));}
            void pause(){query(args("audio_loaded_pause","entity",loaded()));}
            void resume(){query(args("audio_loaded_resume","entity",loaded()));}
            void unload(){if(id!=null){query(args("audio_unload","entity",id));id=null;}}
            void Unload(){unload();}
        }
        static LoadedAudio load(String source){return new LoadedAudio(source).load();}
        static Object get_output_devices(){return query(args("audio_get_output_devices"));}
        static Object get_input_devices(){return query(args("audio_get_input_devices"));}
        static void set_output_device(String device){query(args("audio_set_output_device","device",device));}
        static void set_input_device(String device){query(args("audio_set_input_device","device",device));}
        record Voice(String id){void stop(){query(args("audio_stop","entity",id));}void pause(){query(args("audio_pause","entity",id));}void resume(){query(args("audio_resume","entity",id));}}
        static Voice play(String source,double volume,double pitch,boolean loop){return new Voice((String)query(args("audio_play","source",source,"volume",volume,"pitch",pitch,"loop",loop)));}
        static Voice playAt(String source,double[] position,double volume,double pitch,boolean loop){return new Voice((String)query(args("audio_play","source",source,"position",position,"volume",volume,"pitch",pitch,"loop",loop)));}
        static void volume(Voice v,double value){query(args("audio_volume","entity",v.id(),"value",value));}static void pitch(Voice v,double value){query(args("audio_pitch","entity",v.id(),"value",value));}
        static Gameplay.Handle fadeIn(Voice v,double d){return fadeIn(v,d,"Linear");}
        static Gameplay.Handle fadeIn(Voice v,double d,String ease){var a=Tween.action(v.id(),"Volume",1,d,ease);a.put("from",0);return Gameplay.start(a);}
        static Gameplay.Handle fadeOut(Voice v,double d){return fadeOut(v,d,"Linear");}
        static Gameplay.Handle fadeOut(Voice v,double d,String ease){return Tween.to(v.id(),"Volume",0,d,ease);}
        static Gameplay.Handle crossfade(Voice a,Voice b,double d){return crossfade(a,b,d,"Linear");}
        static Gameplay.Handle crossfade(Voice a,Voice b,double d,String ease){var fade=Tween.action(b.id(),"Volume",1,d,ease);fade.put("from",0);return Gameplay.start(Map.of("kind","parallel","actions",List.of(Tween.action(a.id(),"Volume",0,d,ease),fade)));}
    }
    static class Events{
        record Connection(String token){void disconnect(){Gameplay.connections.remove(token);Gameplay.callbacks.remove(token);Gameplay.once.remove(token);Gameplay.send(Map.of("command","disconnect","token",token));}}
        static Connection connectInternal(String name,java.util.function.Consumer<Object[]> fn,boolean once,String source){String token=Gameplay.register(fn,once);Gameplay.connections.add(token);Gameplay.send(args("","command","connect","token",token,"signal",args("","name",name,"source",source),"once",once));return new Connection(token);}
        static Connection on(String name,java.util.function.Consumer<Object[]> fn){return connectInternal(name,fn,false,null);}
        static Connection once(String name,java.util.function.Consumer<Object[]> fn){return connectInternal(name,fn,true,null);}
        static Connection connect(String e,String name,java.util.function.Consumer<Object[]> fn){return connectInternal(name,fn,false,Gameplay.entity(e));}
        static void emit(String name,Object[] arguments,String source){Gameplay.send(args("","command","emit","signal",args("","name",name,"source",source==null?null:Gameplay.entity(source)),"arguments",arguments));}
        static void disconnect(Connection c){c.disconnect();}
    }
    static class Ease {
        static final String Linear="Linear";
        static final String InSine="InSine";
        static final String OutSine="OutSine";
        static final String InOutSine="InOutSine";
        static final String InQuad="InQuad";
        static final String OutQuad="OutQuad";
        static final String InOutQuad="InOutQuad";
        static final String InCubic="InCubic";
        static final String OutCubic="OutCubic";
        static final String InOutCubic="InOutCubic";
        static final String InQuart="InQuart";
        static final String OutQuart="OutQuart";
        static final String InOutQuart="InOutQuart";
        static final String InQuint="InQuint";
        static final String OutQuint="OutQuint";
        static final String InOutQuint="InOutQuint";
        static final String InExpo="InExpo";
        static final String OutExpo="OutExpo";
        static final String InOutExpo="InOutExpo";
        static final String InCirc="InCirc";
        static final String OutCirc="OutCirc";
        static final String InOutCirc="InOutCirc";
        static final String InBack="InBack";
        static final String OutBack="OutBack";
        static final String InOutBack="InOutBack";
        static final String InElastic="InElastic";
        static final String OutElastic="OutElastic";
        static final String InOutElastic="InOutElastic";
        static final String InBounce="InBounce";
        static final String OutBounce="OutBounce";
        static final String InOutBounce="InOutBounce";
    }
    static class RusticApi {
        final ObjectPath game=new ObjectPath("rustic.game");
        String find_entity(String source){return (String)query(args("find_entity","source",source));}
        void edit_object_attribute(String source,String name,Object value){command("edit_attribute","source",source,"name",name,"value",value);}
        Object get_object_attribute(String source,String name){return value(query(args("object_attribute","source",source,"name",name)));}
        String entity_id(){return (String)state.get("entity_id");}
        double delta_time(){return ((Number)state.get("delta_time")).doubleValue();}
        double fixed_delta_time(){return ((Number)state.get("fixed_delta_time")).doubleValue();}
        double[] get_translation(){return vector(state.get("translation"));}
        void set_translation(double x,double y,double z){command("set_translation","value",new double[]{x,y,z});}
        Object get_property(String n){return value(field("properties",n));}
        void set_property(String n,Object v){command("set_property","name",n,"value",v);}
        Object get_attribute(String n){return value(field("attributes",n));}
        Object GetAttribute(String n){return get_attribute(n);}
        void edit_attribute(String n,Object v){command("edit_attribute","name",n,"value",v);}
        void EditAttribute(String n,Object v){edit_attribute(n,v);}
        ActionState input(String n){return new ActionState(field("actions",n));}
        ActionState key(String n){return new ActionState(field("keys",n));}
        List<KeyEvent> key_events(){var out=new ArrayList<KeyEvent>();for(Object v:(List<?>)state.get("key_events"))out.add(new KeyEvent((Map<?,?>)v));return out;}
        boolean any_key_pressed(){return Boolean.TRUE.equals(state.get("any_key_pressed"));}
        void print(String message){log("info",message);}
        void warn(String message){log("warn",message);}
        void log(String l,String m){command("log","level",l,"message",m);}
        void set_enabled(boolean e){command("set_enabled","enabled",e);}
    }
    static class InstanceApi {
        void add(String s){add(s,null);}void add(String s,String p){command("add_instance","source",s,"parent",p);}
        void clone(String s){clone(s,null);}void clone(String s,String p){command("clone_instance","source",s,"parent",p);}
    }
    static class SceneApi {
        Object getEnvironment(String field){return query(args("scene_environment_field","field",field));}
        Object getEnvironment(){return query(args("scene_environment"));}
        Object setEnvironment(Map<String,Object> settings){return query(args("scene_environment","settings",settings));}
        Object setSkyTexture(String path){return setEnvironment(Map.of("sky_image",path));}
        String Find(String p){return (String)field("scene_paths",p);}
        List<String> List(){return List("Game.scene");}
        List<String> List(String p){var out=new ArrayList<String>();for(var e:((Map<?,?>)state.get("scene_paths")).entrySet())if(((String)e.getKey()).startsWith((p.isEmpty()?"Game.scene":p)+"."))out.add((String)e.getValue());return out;}
    }
    static class GameApi {
        CaptureHandle startRecording(){return startRecording(null,"game");}
        CaptureHandle startRecording(String path){return startRecording(path,"game");}
        CaptureHandle startRecording(String path,String source){return new CaptureHandle((String)query(args("capture_recording_start","path",path,"source",source)),true);}
        void stopRecording(){stopRecording(null);}
        void stopRecording(CaptureHandle handle){query(args("capture_recording_stop","id",handle==null?null:handle.id));}
        CaptureHandle startHotMic(){return new CaptureHandle((String)query(args("capture_mic_start")),false);}
        void stopHotMic(){stopHotMic(null);}
        void stopHotMic(CaptureHandle handle){query(args("capture_mic_stop","id",handle==null?null:handle.id));}
        final SceneApi scene=new SceneApi();void setCurrentCamera(String s){command("set_current_camera","source",s);} }
    private static String quote(String s){StringBuilder b=new StringBuilder("\"");for(char c:s.toCharArray()){if(c<32||c=='"'||c=='\\')b.append(String.format("\\u%04x",(int)c));else b.append(c);}return b.append('"').toString();}
    private static String encode(Object v){
        if(v instanceof Record){var fields=new LinkedHashMap<String,Object>();try{for(var c:v.getClass().getRecordComponents())fields.put(c.getName(),c.getAccessor().invoke(v));}catch(ReflectiveOperationException e){throw new IllegalArgumentException(e);}return encode(fields);}
        if(v==null)return "null";if(v instanceof String)return quote((String)v);if(v instanceof Boolean)return v.toString();
        if(v instanceof Number){if(!Double.isFinite(((Number)v).doubleValue()))throw new IllegalArgumentException("non-finite number");return v.toString();}
        if(v instanceof Map<?,?>){var a=new ArrayList<String>();for(var e:((Map<?,?>)v).entrySet())a.add(quote((String)e.getKey())+":"+encode(e.getValue()));return "{"+String.join(",",a)+"}";}
        if(v instanceof Iterable<?>){var a=new ArrayList<String>();for(Object x:(Iterable<?>)v)a.add(encode(x));return "["+String.join(",",a)+"]";}
        if(v.getClass().isArray()){var a=new ArrayList<String>();for(int i=0;i<java.lang.reflect.Array.getLength(v);i++)a.add(encode(java.lang.reflect.Array.get(v,i)));return "["+String.join(",",a)+"]";}
        throw new IllegalArgumentException("unsupported engine value");
    }
    private static class Parser {
        final String text;int at=0;Parser(String text){this.text=text;}
        void ws(){while(at<text.length()&&Character.isWhitespace(text.charAt(at)))at++;}
        char take(){return text.charAt(at++);}void expect(char c){if(take()!=c)throw new IllegalArgumentException("invalid state");}
        String string(){expect('"');var b=new StringBuilder();while(true){char c=take();if(c=='"')return b.toString();if(c=='\\'){c=take();switch(c){case 'u':c=(char)Integer.parseInt(text.substring(at,at+4),16);at+=4;break;case 'n':c='\n';break;case 'r':c='\r';break;case 't':c='\t';break;case 'b':c='\b';break;case 'f':c='\f';break;default:break;}}b.append(c);}}
        Object parse(){Object v=read(0);ws();if(at!=text.length())throw new IllegalArgumentException("trailing state");return v;}
        Object read(int depth){if(depth>64)throw new IllegalArgumentException("state nesting limit");ws();char c=text.charAt(at);
            if(c=='"')return string();
            if(c=='{'){take();var m=new LinkedHashMap<String,Object>();ws();if(text.charAt(at)=='}'){take();return m;}while(true){ws();String key=string();ws();expect(':');m.put(key,read(depth+1));ws();c=take();if(c=='}')return m;if(c!=',')throw new IllegalArgumentException("invalid object");}}
            if(c=='['){take();var a=new ArrayList<Object>();ws();if(text.charAt(at)==']'){take();return a;}while(true){a.add(read(depth+1));ws();c=take();if(c==']')return a;if(c!=',')throw new IllegalArgumentException("invalid array");}}
            int start=at;while(at<text.length()&&!Character.isWhitespace(text.charAt(at))&&",]}".indexOf(text.charAt(at))<0)at++;String n=text.substring(start,at);
            switch(n){case "null":return null;case "true":return true;case "false":return false;default:if(n.indexOf('.')<0&&n.indexOf('e')<0&&n.indexOf('E')<0)return Long.valueOf(n);return Double.valueOf(n);}
        }
    }
    // Canonical API paths.
    static class CanonicalScenePath {
        final String source;
        CanonicalScenePath(String source){this.source=source;}
        CanonicalScenePath scene(String name){return new CanonicalScenePath(source+"."+name);}
        CanonicalScenePath object(String path){return new CanonicalScenePath(source+"."+path);}
        void clone(String parent){instance.clone(source,parent);}
        void add(String parent){instance.add(source,parent);}
        void setCurrentCamera(){Game.setCurrentCamera(source);}
        void emit(String name,Object[] arguments){Events.emit(name,arguments,source);}
        void EditAttribute(String name,Object value){command("edit_attribute","source",this.source,"name",name,"value",value);}
        void edit_attribute(String name,Object value){EditAttribute(name,value);}
        Gameplay.Handle tweenTo(String p,Object to,MotionOptions opts){return Tween.to(this.source,p,to,opts);}
        Gameplay.Handle tweenTo(String property,Object value,double duration){return Tween.to(this.source,property,value,duration);}
        Gameplay.Handle tweenTo(String property,Object value,double duration,String easing){return Tween.to(this.source,property,value,duration,easing);}
        Gameplay.Handle tweenMove(double[] p,MotionOptions opts){return Tween.move(this.source,p,opts);}
        Gameplay.Handle tweenMove(double[] p,double duration){return Tween.move(this.source,p,duration);}
        Gameplay.Handle tweenMove(double[] p,double duration,String easing){return Tween.move(this.source,p,duration,easing);}
        Gameplay.Handle tweenRotate(double[] p,MotionOptions opts){return Tween.rotate(this.source,p,opts);}
        Gameplay.Handle tweenRotate(double[] q,double duration){return Tween.rotate(this.source,q,duration);}
        Gameplay.Handle tweenRotate(double[] q,double duration,String easing){return Tween.rotate(this.source,q,duration,easing);}
        Gameplay.Handle tweenScale(double[] p,double duration){return Tween.scale(this.source,p,duration);}
        Gameplay.Handle tweenScale(double[] p,double duration,String easing){return Tween.scale(this.source,p,duration,easing);}
        Map<String,Object> tweenAction(String p,Object to,MotionOptions opts){return Tween.action(this.source,p,to,opts);}
        Map<String,Object> tweenAction(String property,Object to,double duration,String easing){return Tween.action(this.source,property,to,duration,easing);}
        Gameplay.Handle movementMoveTo(double[] p,MotionOptions opts){return Movement.moveTo(this.source,p,opts);}
        Gameplay.Handle movementMoveTo(double[] p,double d){return Movement.moveTo(this.source,p,d);}
        Gameplay.Handle movementMoveTo(double[] p,double d,String easing){return Movement.moveTo(this.source,p,d,easing);}
        Gameplay.Handle movementMove(double[] offset,double duration){return Movement.move(this.source,offset,duration);}
        Gameplay.Handle movementMove(double[] offset,double duration,String easing){return Movement.move(this.source,offset,duration,easing);}
        Gameplay.Handle movementRotateTo(double[] p,MotionOptions opts){return Movement.rotateTo(this.source,p,opts);}
        Gameplay.Handle movementRotateTo(double[] q,double d){return Movement.rotateTo(this.source,q,d);}
        Gameplay.Handle movementRotateTo(double[] q,double d,String easing){return Movement.rotateTo(this.source,q,d,easing);}
        Gameplay.Handle movementLookAt(double[] position,double duration){return Movement.lookAt(this.source,position,duration);}
        Gameplay.Handle movementLookAt(double[] position,double duration,String easing){return Movement.lookAt(this.source,position,duration,easing);}
        Gameplay.Handle movementFollow(String target,double duration,String easing,double[] offset){return Movement.follow(this.source,target,duration,easing,offset);}
        Gameplay.Handle movementOrbit(double[] center,double radius,double turns,double duration){return Movement.orbit(this.source,center,radius,turns,duration);}
        Gameplay.Handle movementOrbit(double[] center,double radius,double turns,double duration,String easing){return Movement.orbit(this.source,center,radius,turns,duration,easing);}
        Gameplay.Handle pathFollow(Path.Curve path,double duration,String easing,boolean loop,boolean pingPong,boolean orientToPath){return Path.follow(this.source,path,duration,easing,loop,pingPong,orientToPath);}
        Gameplay.Handle pathFollowSpeed(Path.Curve path,double speed,String easing,boolean loop,boolean pingPong,boolean orientToPath){return Path.followSpeed(this.source,path,speed,easing,loop,pingPong,orientToPath);}
        Gameplay.Handle effectsFade(double opacity,double duration){return Effects.fade(this.source,opacity,duration);}
        Gameplay.Handle effectsFade(double opacity,double duration,String easing){return Effects.fade(this.source,opacity,duration,easing);}
        Gameplay.Handle effectsFlash(double[] color,double duration){return Effects.flash(this.source,color,duration);}
        Gameplay.Handle effectsFlash(double[] color,double duration,String easing){return Effects.flash(this.source,color,duration,easing);}
        Gameplay.Handle effectsShake(double strength,double duration){return Effects.shake(this.source,strength,duration);}
        Gameplay.Handle effectsShake(double strength,double duration,String easing){return Effects.shake(this.source,strength,duration,easing);}
        Gameplay.Handle effectsPulse(double[] scale,double duration){return Effects.pulse(this.source,scale,duration);}
        Gameplay.Handle effectsPulse(double[] scale,double duration,String easing){return Effects.pulse(this.source,scale,duration,easing);}
        Gameplay.Handle cameraMoveTo(double[] p,double d){return Camera.moveTo(this.source,p,d);}
        Gameplay.Handle cameraMoveTo(double[] p,double d,String ease){return Camera.moveTo(this.source,p,d,ease);}
        Gameplay.Handle cameraZoom(double f,double d){return Camera.zoom(this.source,f,d);}
        Gameplay.Handle cameraZoom(double f,double d,String ease){return Camera.zoom(this.source,f,d,ease);}
        Gameplay.Handle cameraFov(double f,double d){return Camera.fov(this.source,f,d);}
        Gameplay.Handle cameraFov(double f,double d,String ease){return Camera.fov(this.source,f,d,ease);}
        Gameplay.Handle cameraFollow(String target,double d){return Camera.follow(this.source,target,d);}
        Gameplay.Handle cameraFollow(String target,double d,String ease){return Camera.follow(this.source,target,d,ease);}
        Gameplay.Handle cameraOrbit(double[] c,double r,double turns,double d){return Camera.orbit(this.source,c,r,turns,d);}
        Gameplay.Handle cameraOrbit(double[] c,double r,double turns,double d,String ease){return Camera.orbit(this.source,c,r,turns,d,ease);}
        Gameplay.Handle cameraLookAt(double[] p,double d){return Camera.lookAt(this.source,p,d);}
        Gameplay.Handle cameraLookAt(double[] p,double d,String ease){return Camera.lookAt(this.source,p,d,ease);}
        Gameplay.Handle cameraShake(double s,double d){return Camera.shake(this.source,s,d);}
        Gameplay.Handle cameraShake(double s,double d,String ease){return Camera.shake(this.source,s,d,ease);}
        Gameplay.Handle cameraTransition(double[] p,double[] r,double f,double d){return Camera.transition(this.source,p,r,f,d);}
        Gameplay.Handle cameraTransition(double[] p,double[] r,double f,double d,String ease){return Camera.transition(this.source,p,r,f,d,ease);}
        void physicsImpulse(double[] v){Physics.impulse(this.source,v);}
        void physicsForce(double[] v,double dt){Physics.force(this.source,v,dt);}
        void physicsLaunch(double[] v){Physics.launch(this.source,v);}
        void physicsKnockback(double[] v){Physics.knockback(this.source,v);}
        void animationRegister(Animation.Clip clip){Animation.register(this.source,clip);}
        Object animationClips(){return Animation.clips(this.source);}
        Map<String,Object> animationAction(String name,Animation.Options opts,String marker,Object source){return Animation.action(this.source,name,opts,marker,source);}
        Gameplay.Handle animationPlay(String name){return Animation.play(this.source,name);}
        Gameplay.Handle animationPlay(String name,Animation.Options opts){return Animation.play(this.source,name,opts);}
        Gameplay.Handle animationBlend(String from,String to,double duration){return Animation.blend(this.source,from,to,duration);}
        Gameplay.Handle animationBlend(String from,String to,double duration,String ease){return Animation.blend(this.source,from,to,duration,ease);}
        Gameplay.Handle animationTransition(String from,String to,double duration){return Animation.transition(this.source,from,to,duration);}
        Gameplay.Handle animationTransition(String from,String to,double duration,String ease){return Animation.transition(this.source,from,to,duration,ease);}
        Gameplay.Handle animationIk(String middle,String tip,double[] target,double duration,String easing,double weight,double[] pole){return Animation.ik(this.source,middle,tip,target,duration,easing,weight,pole);}
        Gameplay.Handle animationFootPlacement(String middle,String tip,double[] target,double duration){return Animation.footPlacement(this.source,middle,tip,target,duration);}
        Gameplay.Handle animationFootPlacement(String middle,String tip,double[] target,double duration,String ease){return Animation.footPlacement(this.source,middle,tip,target,duration,ease);}
        Gameplay.Handle animationLookAt(double[] target,double d,String ease){return Animation.lookAt(this.source,target,d,ease);}
        Gameplay.Handle animationLookAt(double[] target,double d){return Animation.lookAt(this.source,target,d);}
        Gameplay.Handle animationHeadTracking(double[] target,double duration){return Animation.headTracking(this.source,target,duration);}
        Gameplay.Handle animationHeadTracking(double[] target,double duration,String ease){return Animation.headTracking(this.source,target,duration,ease);}
        Gameplay.Handle animationRecoil(double[] rotation,double duration){return Animation.recoil(this.source,rotation,duration);}
        Gameplay.Handle animationRecoil(double[] rotation,double duration,String ease){return Animation.recoil(this.source,rotation,duration,ease);}
        void animationAddMarker(String clip,double time,String name){Animation.addMarker(this.source,clip,time,name);}
        Events.Connection eventsConnect(String name,java.util.function.Consumer<Object[]> fn){return Events.connect(this.source,name,fn);}
        boolean tagsAdd(String name){return Tags.add(this.source,name);}
        boolean tagsRemove(String name){return Tags.remove(this.source,name);}
        boolean groupsAdd(String name){return Groups.add(name,source);}
        void hierarchySetParent(String parent,boolean keep_world){Hierarchy.set_parent(this.source,parent,keep_world);}
        void hierarchySetParent(String parent){Hierarchy.set_parent(this.source,parent);}
        String[] hierarchyGetChildren(){return Hierarchy.get_children(this.source);}
        void hierarchyDetach(boolean keep_world){Hierarchy.detach(this.source,keep_world);}
        void hierarchyDetach(){Hierarchy.detach(this.source);}
        void entityDestroy(){Entity.destroy(this.source);}
        void entityDestroyAfter(double delay){Entity.destroy_after(this.source,delay);}
        boolean entityIsValid(){return Entity.is_valid(this.source);}
        boolean componentsHas(String name){return Components.has(this.source,name);}
        String[] componentsGetAll(){return Components.get_all(this.source);}
        String[] componentsFindInChildren(String name){return Components.find_in_children(this.source,name);}
        Gameplay.Handle tween(String p,Object to,MotionOptions opts){return Tween.to(this.source,p,to,opts);}
        Gameplay.Handle tween(String property,Object value,double duration){return Tween.to(this.source,property,value,duration);}
        Gameplay.Handle tween(String property,Object value,double duration,String easing){return Tween.to(this.source,property,value,duration,easing);}
        Gameplay.Handle moveTo(double[] p,MotionOptions opts){return Movement.moveTo(this.source,p,opts);}
        Gameplay.Handle moveTo(double[] p,double d){return Movement.moveTo(this.source,p,d);}
        Gameplay.Handle moveTo(double[] p,double d,String easing){return Movement.moveTo(this.source,p,d,easing);}
        Gameplay.Handle move(double[] offset,double duration){return Movement.move(this.source,offset,duration);}
        Gameplay.Handle move(double[] offset,double duration,String easing){return Movement.move(this.source,offset,duration,easing);}
        Gameplay.Handle rotateTo(double[] p,MotionOptions opts){return Movement.rotateTo(this.source,p,opts);}
        Gameplay.Handle rotateTo(double[] q,double d){return Movement.rotateTo(this.source,q,d);}
        Gameplay.Handle rotateTo(double[] q,double d,String easing){return Movement.rotateTo(this.source,q,d,easing);}
        Gameplay.Handle lookAt(double[] position,double duration){return Movement.lookAt(this.source,position,duration);}
        Gameplay.Handle lookAt(double[] position,double duration,String easing){return Movement.lookAt(this.source,position,duration,easing);}
        Gameplay.Handle follow(String target,double duration,String easing,double[] offset){return Movement.follow(this.source,target,duration,easing,offset);}
        Gameplay.Handle orbit(double[] center,double radius,double turns,double duration){return Movement.orbit(this.source,center,radius,turns,duration);}
        Gameplay.Handle orbit(double[] center,double radius,double turns,double duration,String easing){return Movement.orbit(this.source,center,radius,turns,duration,easing);}
        Gameplay.Handle fade(double opacity,double duration){return Effects.fade(this.source,opacity,duration);}
        Gameplay.Handle fade(double opacity,double duration,String easing){return Effects.fade(this.source,opacity,duration,easing);}
        Gameplay.Handle flash(double[] color,double duration){return Effects.flash(this.source,color,duration);}
        Gameplay.Handle flash(double[] color,double duration,String easing){return Effects.flash(this.source,color,duration,easing);}
        Gameplay.Handle shake(double strength,double duration){return Effects.shake(this.source,strength,duration);}
        Gameplay.Handle shake(double strength,double duration,String easing){return Effects.shake(this.source,strength,duration,easing);}
        Gameplay.Handle pulse(double[] scale,double duration){return Effects.pulse(this.source,scale,duration);}
        Gameplay.Handle pulse(double[] scale,double duration,String easing){return Effects.pulse(this.source,scale,duration,easing);}
        void impulse(double[] v){Physics.impulse(this.source,v);}
        void force(double[] v,double dt){Physics.force(this.source,v,dt);}
        void launch(double[] v){Physics.launch(this.source,v);}
        void knockback(double[] v){Physics.knockback(this.source,v);}
        Gameplay.Handle playAnimation(String name){return Animation.play(this.source,name);}
        Gameplay.Handle playAnimation(String name,Animation.Options opts){return Animation.play(this.source,name,opts);}
        void destroy(){Entity.destroy(this.source);}
        void destroyAfter(double delay){Entity.destroy_after(this.source,delay);}
        boolean isValid(){return Entity.is_valid(this.source);}
        boolean addTag(String name){return Tags.add(this.source,name);}
        boolean removeTag(String name){return Tags.remove(this.source,name);}
        void setParent(String parent,boolean keep_world){Hierarchy.set_parent(this.source,parent,keep_world);}
        void setParent(String parent){Hierarchy.set_parent(this.source,parent);}
        String[] getChildren(){return Hierarchy.get_children(this.source);}
        void detach(boolean keep_world){Hierarchy.detach(this.source,keep_world);}
        void detach(){Hierarchy.detach(this.source);}
        boolean hasComponent(String name){return Components.has(this.source,name);}
        String[] getComponents(){return Components.get_all(this.source);}
        String[] findInChildren(String name){return Components.find_in_children(this.source,name);}
        Gameplay.Handle followPath(Path.Curve path,double duration,String easing,boolean loop,boolean pingPong,boolean orientToPath){return Path.follow(this.source,path,duration,easing,loop,pingPong,orientToPath);}
    }
    static class CanonicalFilePath {
        final String filePath;
        CanonicalFilePath(String filePath){this.filePath=filePath;}
        CanonicalFilePath object(String path){return new CanonicalFilePath(filePath.isEmpty()?path:filePath+"/"+path);}
        Audio.Voice audioPlay(double volume,double pitch,boolean loop){return Audio.play(filePath,volume,pitch,loop);}
        Audio.Voice audioPlayAt(double[] position,double volume,double pitch,boolean loop){return Audio.playAt(filePath,position,volume,pitch,loop);}
        Object animationLoad(String e){return Animation.load(e,filePath);}
        Audio.Voice play(double volume,double pitch,boolean loop){return Audio.play(filePath,volume,pitch,loop);}
        Audio.Voice playAt(double[] position,double volume,double pitch,boolean loop){return Audio.playAt(filePath,position,volume,pitch,loop);}
        Object loadAnimation(String e){return Animation.load(e,filePath);}
        void add(String parent){instance.add(filePath,parent);}
        void clone(String parent){instance.clone(filePath,parent);}
        Object setSkyTexture(){return Game.scene.setSkyTexture(filePath);}
    }
    static class CanonicalGlobalApi {
        final CanonicalFilePath files=new CanonicalFilePath("");
        Gameplay.Handle tweenValue(Object from,Object to,double duration,java.util.function.Consumer<Object> sample){return Tween.value(from,to,duration,sample);}
        Gameplay.Handle tweenValue(Object from,Object to,double duration,java.util.function.Consumer<Object> sample,String easing){return Tween.value(from,to,duration,sample,easing);}
        Gameplay.Handle timerAfter(double delay,Runnable fn){return Timer.after(delay,fn);}
        Gameplay.Handle timerEvery(double interval,Runnable fn){return Timer.every(interval,fn);}
        Gameplay.Handle timerEvery(double interval,Runnable fn,Integer count){return Timer.every(interval,fn,count);}
        Object smoothLerp(Object a,Object b,double t){return Smooth.lerp(a,b,t);}
        Object smoothLerp(Object a,Object b,double t,String ease){return Smooth.lerp(a,b,t,ease);}
        double[] smoothSlerp(double[] a,double[] b,double t){return Smooth.slerp(a,b,t);}
        double[] smoothSlerp(double[] a,double[] b,double t,String ease){return Smooth.slerp(a,b,t,ease);}
        double smoothInverseLerp(double a,double b,double v){return Smooth.inverseLerp(a,b,v);}
        double smoothRemap(double v,double a,double b,double c,double d){return Smooth.remap(v,a,b,c,d);}
        double[] smoothSmoothDamp(double c,double t,double v,double s,double dt){return Smooth.smoothDamp(c,t,v,s,dt);}
        Object physicsRaycast(double[] origin,double[] direction,double distance){return Physics.raycast(origin,direction,distance);}
        Object physicsSphereCast(double[] origin,double[] direction,double distance,double radius){return Physics.sphereCast(origin,direction,distance,radius);}
        Object physicsOverlap(double[] center,double radius){return Physics.overlap(center,radius);}
        Object physicsExplosion(double[] center,double radius,double strength){return Physics.explosion(center,radius,strength);}
        double clockTimeScale(double scale){return Clock.timeScale(scale);}
        void clockPause(){Clock.pause();}
        void clockResume(){Clock.resume();}
        Path.Curve pathCreate(double[][] points,String kind){return Path.create(points,kind);}
        Path.Curve pathCreate(List<Path.Point> points,String kind){return Path.create(points,kind);}
        Sequence.Builder sequenceNew(){return Sequence.create();}
        Sequence.Builder sequenceCreate(){return Sequence.create();}
        Sequence.Builder timelineNew(){return Sequence.create();}
        Sequence.Builder timelineCreate(){return Sequence.create();}
        Events.Connection eventsOn(String name,java.util.function.Consumer<Object[]> fn){return Events.on(name,fn);}
        Events.Connection eventsOnce(String name,java.util.function.Consumer<Object[]> fn){return Events.once(name,fn);}
        void eventsEmit(String name,Object[] arguments,String source){Events.emit(name,arguments,source);}
        void eventsDisconnect(Events.Connection c){Events.disconnect(c);}
        Object audioGetOutputDevices(){return Audio.get_output_devices();}
        Object audioGetInputDevices(){return Audio.get_input_devices();}
        void audioSetOutputDevice(String device){Audio.set_output_device(device);}
        void audioSetInputDevice(String device){Audio.set_input_device(device);}
        void audioVolume(Audio.Voice v,double value){Audio.volume(v,value);}
        void audioPitch(Audio.Voice v,double value){Audio.pitch(v,value);}
        Gameplay.Handle audioFadeIn(Audio.Voice v,double d){return Audio.fadeIn(v,d);}
        Gameplay.Handle audioFadeIn(Audio.Voice v,double d,String ease){return Audio.fadeIn(v,d,ease);}
        Gameplay.Handle audioFadeOut(Audio.Voice v,double d){return Audio.fadeOut(v,d);}
        Gameplay.Handle audioFadeOut(Audio.Voice v,double d,String ease){return Audio.fadeOut(v,d,ease);}
        Gameplay.Handle audioCrossfade(Audio.Voice a,Audio.Voice b,double d){return Audio.crossfade(a,b,d);}
        Gameplay.Handle audioCrossfade(Audio.Voice a,Audio.Voice b,double d,String ease){return Audio.crossfade(a,b,d,ease);}
        Gameplay.Handle animationValue(List<Animation.Key> keys,java.util.function.Consumer<Object> sample){return Animation.value(keys,sample);}
        void animationStop(Gameplay.Handle h){Animation.stop(h);}
        void animationPause(Gameplay.Handle h){Animation.pause(h);}
        void animationSpeed(Gameplay.Handle h,double v){Animation.speed(h,v);}
        void animationLoop(Gameplay.Handle h,boolean v){Animation.loop(h,v);}
        String cameraCurrent(){return Camera.current();}
        String[] tagsFindAll(String name){return Tags.find_all(name);}
        boolean groupsCreate(String name){return Groups.create(name);}
        String[] groupsGetMembers(String name){return Groups.get_members(name);}
        String[] worldFindByTag(String name){return World.find_by_tag(name);}
        String[] worldFindInRadius(double[] center,double radius){return World.find_in_radius(center,radius);}
        Gameplay.Handle after(double delay,Runnable fn){return Timer.after(delay,fn);}
        Gameplay.Handle every(double interval,Runnable fn){return Timer.every(interval,fn);}
        Gameplay.Handle every(double interval,Runnable fn,Integer count){return Timer.every(interval,fn,count);}
        Object lerp(Object a,Object b,double t){return Smooth.lerp(a,b,t);}
        Object lerp(Object a,Object b,double t,String ease){return Smooth.lerp(a,b,t,ease);}
        double[] slerp(double[] a,double[] b,double t){return Smooth.slerp(a,b,t);}
        double[] slerp(double[] a,double[] b,double t,String ease){return Smooth.slerp(a,b,t,ease);}
        double inverseLerp(double a,double b,double v){return Smooth.inverseLerp(a,b,v);}
        double remap(double v,double a,double b,double c,double d){return Smooth.remap(v,a,b,c,d);}
        double[] smoothDamp(double c,double t,double v,double s,double dt){return Smooth.smoothDamp(c,t,v,s,dt);}
        Object raycast(double[] origin,double[] direction,double distance){return Physics.raycast(origin,direction,distance);}
        Object sphereCast(double[] origin,double[] direction,double distance,double radius){return Physics.sphereCast(origin,direction,distance,radius);}
        Object overlap(double[] center,double radius){return Physics.overlap(center,radius);}
        Object explosion(double[] center,double radius,double strength){return Physics.explosion(center,radius,strength);}
        double timeScale(double scale){return Clock.timeScale(scale);}
        void pause(){Clock.pause();}
        void resume(){Clock.resume();}
        Path.Curve createPath(double[][] points,String kind){return Path.create(points,kind);}
        Path.Curve createPath(List<Path.Point> points,String kind){return Path.create(points,kind);}
        Sequence.Builder sequence(){return Sequence.create();}
        Sequence.Builder timeline(){return Sequence.create();}
        Events.Connection on(String name,java.util.function.Consumer<Object[]> fn){return Events.on(name,fn);}
        Events.Connection once(String name,java.util.function.Consumer<Object[]> fn){return Events.once(name,fn);}
        void emit(String name,Object[] arguments,String source){Events.emit(name,arguments,source);}
        void disconnect(Events.Connection c){Events.disconnect(c);}
        String currentCamera(){return Camera.current();}
        Object getOutputDevices(){return Audio.get_output_devices();}
        Object get_output_devices(){return Audio.get_output_devices();}
        Object getInputDevices(){return Audio.get_input_devices();}
        Object get_input_devices(){return Audio.get_input_devices();}
        void setOutputDevice(String device){Audio.set_output_device(device);}
        void set_output_device(String device){Audio.set_output_device(device);}
        void setInputDevice(String device){Audio.set_input_device(device);}
        void set_input_device(String device){Audio.set_input_device(device);}
        Object getEnvironment(){return Game.scene.getEnvironment();}
        Object setEnvironment(Map<String,Object> settings){return Game.scene.setEnvironment(settings);}
        void setCurrentCamera(String path){Game.setCurrentCamera(path);}
        String entity_id(){return rustic.entity_id();}
        double delta_time(){return rustic.delta_time();}
        double fixed_delta_time(){return rustic.fixed_delta_time();}
        double[] get_translation(){return rustic.get_translation();}
        void set_translation(double x,double y,double z){rustic.set_translation(x,y,z);}
        Object get_property(String n){return rustic.get_property(n);}
        void set_property(String n,Object v){rustic.set_property(n,v);}
        Object get_attribute(String n){return rustic.get_attribute(n);}
        Object GetAttribute(String n){return rustic.GetAttribute(n);}
        void edit_attribute(String n,Object v){rustic.edit_attribute(n,v);}
        void EditAttribute(String n,Object v){rustic.EditAttribute(n,v);}
        ActionState input(String n){return rustic.input(n);}
        ActionState key(String n){return rustic.key(n);}
        List<KeyEvent> key_events(){return rustic.key_events();}
        boolean any_key_pressed(){return rustic.any_key_pressed();}
        void log(String l,String m){rustic.log(l,m);}
        void set_enabled(boolean e){rustic.set_enabled(e);}
    }
    static class CanonicalGame {
        final CanonicalScenePath scene=new CanonicalScenePath("game.scene");
        final CanonicalGlobalApi rustic=new CanonicalGlobalApi();
    }
}
