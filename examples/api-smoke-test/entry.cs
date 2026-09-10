using System.Text.Json;

string? line;
while ((line = Console.ReadLine()) is not null) {
    using var document = JsonDocument.Parse(line);
    var state = document.RootElement;
    foreach (var name in new[] { "callback","delta","entity_id","delta_time","fixed_delta_time",
        "translation","properties","attributes","scene_paths","actions","keys","key_events","any_key_pressed" })
        state.TryGetProperty(name, out _);
    var commands = new List<object>();
    if (state.GetProperty("callback").GetString() == "on_start") {
        commands.Add(new { op="set_translation", value=state.GetProperty("translation") });
        var properties = state.GetProperty("properties");
        commands.Add(new { op="set_property", name="smoke_value", value=properties.TryGetProperty("smoke_value", out var v) ? v : JsonSerializer.SerializeToElement(1.0) });
        var attributes = state.GetProperty("attributes");
        if (attributes.TryGetProperty("Position", out var p)) commands.Add(new { op="edit_attribute", name="Position", value=p });
        commands.Add(new { op="log", level="info", message="C# API smoke test passed" });
        commands.Add(new { op="set_enabled", enabled=true });
        commands.Add(new { op="add_instance", source="Part", parent=(string?)null });
        commands.Add(new { op="clone_instance", source=state.GetProperty("entity_id").GetString()!, parent=(string?)null });
    }
    Console.WriteLine(JsonSerializer.Serialize(new { format_version=1, commands }));
}

