import java.io.*;
import java.nio.charset.StandardCharsets;
import java.util.*;

/* Engine-owned SDK: scripts receive ordinary Java values, never transport objects. */
class Rustic {
    static Map<?,?> state;
    static final List<Object> commands = new ArrayList<>();
    static final RusticApi rustic = new RusticApi();
    static final InstanceApi instance = new InstanceApi();
    static final GameApi Game = new GameApi();
    interface Callback { void invoke(String callback, double dt) throws Exception; }
    static void run(Callback callback) throws Exception {
        var input = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        var output = new PrintWriter(new OutputStreamWriter(System.out, StandardCharsets.UTF_8), true);
        String line;
        while ((line=input.readLine())!=null) {
            state=(Map<?,?>)new Parser(line).parse(); commands.clear();
            Number delta=(Number)state.get("delta");
            callback.invoke((String)state.get("callback"),delta==null?0:delta.doubleValue());
            output.println(encode(Map.of("format_version",1,"commands",commands)));
        }
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
