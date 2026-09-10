import json
import sys

def commands_for(state):
    # Read every protocol-v1 state field.
    for key in ("callback", "delta", "entity_id", "delta_time", "fixed_delta_time",
                "translation", "properties", "attributes", "scene_paths", "actions",
                "keys", "key_events", "any_key_pressed"):
        state.get(key)
    commands = []
    if state["callback"] == "on_start":
        position = state.get("attributes", {}).get("Position")
        commands.extend([
            {"op":"set_translation", "value":state["translation"]},
            {"op":"set_property", "name":"smoke_value", "value":state.get("properties", {}).get("smoke_value", 1.0)},
            {"op":"log", "level":"info", "message":"Python API smoke test passed"},
            {"op":"set_enabled", "enabled":True},
            {"op":"add_instance", "source":"Part", "parent":None},
            {"op":"clone_instance", "source":state["entity_id"], "parent":None},
        ])
        if position is not None:
            commands.append({"op":"edit_attribute", "name":"Position", "value":position})
    return commands

for line in sys.stdin:
    request = json.loads(line)
    print(json.dumps({"format_version":1, "commands":commands_for(request)}, separators=(",", ":")), flush=True)

