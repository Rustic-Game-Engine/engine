# Scripting Rustic games with C#

C# scripts run as isolated .NET console processes. Rustic creates a temporary SDK
project, chooses `net{major}.0` from the discovered `dotnet --version`, builds Release,
and keeps one process alive per behavior instance. Install the .NET SDK 10 or newer
on `PATH`; confirm it with `cargo xtask doctor`.

Create a `.cs` file and its global, scene, or object-component attachment through the
editor. Scenes store its persistent Asset ID, not source or path. Editor moves and
renames preserve references.

## Dispatching callbacks

The generated top-level starter already creates `instance`, `rustic`, and `Game` and
contains the required line loop. Add dispatch before serializing the response:

```csharp
string? line;
while ((line = Console.ReadLine()) is not null) {
    instance.Commands.Clear();
    rustic.State = JsonDocument.Parse(line).RootElement.Clone();
    Game.scene.State = rustic.State.GetProperty("scene_paths");

    var callback = rustic.State.GetProperty("callback").GetString();
    if (callback == "on_start") {
        rustic.log("info", $"started {rustic.entity_id()}");
    } else if (callback == "fixed_update") {
        var dtElement = rustic.State.GetProperty("delta");
        var dt = dtElement.ValueKind == JsonValueKind.Null
            ? 0.0 : dtElement.GetDouble();
        var key = rustic.key("KeyW");
        if (key.ValueKind != JsonValueKind.Undefined &&
            key.GetProperty("held").GetBoolean()) {
            var p = rustic.get_translation();
            rustic.set_translation(p[0], p[1], p[2] + 4.0 * dt);
        }
    }

    Console.WriteLine(JsonSerializer.Serialize(new {
        format_version = 1,
        commands = instance.Commands
    }));
}
```

The callback wire names are `on_create`, `on_start`, `on_enable`, `fixed_update`,
`update`, `on_disable`, `on_destroy`, and `on_stop`. Always produce one response,
even for an unimplemented callback. The external protocol does not yet carry
collision callbacks.

## Generated helper surface

```csharp
string id = rustic.entity_id();
double dt = rustic.delta_time();
double fixedDt = rustic.fixed_delta_time();
double[] p = rustic.get_translation();
rustic.set_translation(p[0], p[1] + 1, p[2]);

JsonElement health = rustic.get_property("health");
rustic.set_property("health", 90);
JsonElement color = rustic.GetAttribute("Color");
rustic.EditAttribute("Anchored", true);
JsonElement key = rustic.key("KeyW");
IEnumerable<JsonElement> events = rustic.key_events();
bool any = rustic.any_key_pressed();
rustic.log("info", "message");
rustic.set_enabled(false);

string? table = Game.scene.Find("Room.Table");
IEnumerable<string> room = Game.scene.List("Room");
Game.SetCurrentCamera("Room.Camera");
Game.setCurrentCamera("Room.Camera"); // alias

instance.Add("Cube");
instance.Clone("assets/models/chair.glb", table);
instance.add("Cube");       // lower-case aliases
instance.clone("Room.Table");
```

`get_property` uses `GetProperty` and therefore throws if the property is absent;
`GetAttribute` likewise expects a populated built-in name. `input(name)` exists, but
named actions currently arrive as an empty map, so it returns an undefined
`JsonElement`; only held WASD, arrow, and Shift keys from the embedded Play
viewport are populated. Press/release and key events remain empty. See the
[Lua input guide](scriptingLua.md#input) for exact names and setup. Inspect
`ValueKind` before reading an
optional value.

Commands are queued and applied in list order after the callback. Public writes must
preserve existing types. Built-in attributes are `Name`, `Position`, `Size`, `Color`,
`CanTouch`, `CanCollide`, `Anchored`, and `Parent`. An instance source can be an ID,
scene path, model asset, or built-in name; the optional parent is a stable ID.

## Process rules

C# cannot invoke another behavior instance. API 1.0 does not yet include public
Engine Event `emit`/`subscribe` commands, so cross-language coordination must use
shared engine state.

Standard output is exclusively for one compact JSON response per request. Use
`rustic.log` or standard error for diagnostics. The process working directory is a
temporary build directory and its environment is reduced to a safe allowlist. State
in C# fields/locals outside the loop survives for the behavior lifetime. Each callback
has a three-second deadline and 1 MiB response cap; protocol, command, or process
errors disable only that instance. A failed hot build does not replace a running
last-known-good behavior.
