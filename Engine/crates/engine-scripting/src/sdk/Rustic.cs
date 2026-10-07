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
    public static void Run(Action<string,double> callback) {
        string? line;
        while ((line=Console.ReadLine()) is not null) {
            instance.Commands.Clear();
            using var document=JsonDocument.Parse(line);
            rustic.State=document.RootElement;
            Game.scene.State=rustic.State.GetProperty("scene_paths");
            var delta=rustic.State.GetProperty("delta");
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

