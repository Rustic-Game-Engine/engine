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
        void EditAttribute(String name,Object value){command("edit_attribute","source",source,"name",name,"value",value);}
        void edit_attribute(String name,Object value){EditAttribute(name,value);}
    }
    static class Gameplay {
        static final Map<String,java.util.function.Consumer<Object[]>> callbacks=new LinkedHashMap<>();
        static final Set<String> once=new HashSet<>(),connections=new HashSet<>();
        static final Map<String,Handle> handles=new LinkedHashMap<>();
        static String entity(String id){return id==null?rustic.entity_id():Optional.ofNullable(Game.scene.Find(id)).orElse(id);}
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
    static class Clock { static double timeScale(double scale){return ((Number)((Map<?,?>)query(args("clock","scale",scale))).get("scale")).doubleValue();} static void pause(){query(args("clock","paused",true));} static void resume(){query(args("clock","paused",false));} }
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
    static class Audio{
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
        void log(String l,String m){command("log","level",l,"message",m);}
        void set_enabled(boolean e){command("set_enabled","enabled",e);}
    }
    static class InstanceApi {
        void add(String s){add(s,null);}void add(String s,String p){command("add_instance","source",s,"parent",p);}
        void clone(String s){clone(s,null);}void clone(String s,String p){command("clone_instance","source",s,"parent",p);}
    }
    static class SceneApi {
        String Find(String p){return (String)field("scene_paths",p);}
        List<String> List(){return List("Game.scene");}
        List<String> List(String p){var out=new ArrayList<String>();for(var e:((Map<?,?>)state.get("scene_paths")).entrySet())if(p.isEmpty()||p.equals("Game.scene")||((String)e.getKey()).startsWith(p+"."))out.add((String)e.getValue());return out;}
    }
    static class GameApi {final SceneApi scene=new SceneApi();void setCurrentCamera(String s){command("set_current_camera","source",s);} }
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
}
