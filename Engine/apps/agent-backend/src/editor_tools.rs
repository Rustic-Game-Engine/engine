use serde_json::{Value, json};

pub fn tools() -> Vec<Value> {
    let empty = json!({"type":"object","properties":{},"additionalProperties":false});
    let scene = json!({"type":"object","required":["path"],"properties":{"path":{"type":"string","description":"Relative to the configured scenes folder, e.g. levels/forest.scene"}},"additionalProperties":false});
    let mut tools = Vec::new();
    for (name, description, schema) in [
        (
            "editor_status",
            "Read active scene, unsaved status, scene explorer objects (IDs, parent and folder flags), full properties, script scopes and play state from the open editor.",
            empty.clone(),
        ),
        (
            "scene_list",
            "List all project scenes, active and start scene, including nested scene folders.",
            empty.clone(),
        ),
        (
            "scene_create",
            "Create an empty persisted scene; does not switch or overwrite. Path is relative to the scenes folder.",
            json!({"type":"object","required":["path","name"],"properties":{"path":{"type":"string"},"name":{"type":"string"}}}),
        ),
        (
            "scene_switch",
            "Save pending edits and switch the editor to an existing scene. Stop playtesting first.",
            scene.clone(),
        ),
        (
            "scene_set_start",
            "Set the project launch scene without switching the editor.",
            scene,
        ),
        (
            "scene_save",
            "Save the complete authored scene before playtesting.",
            empty.clone(),
        ),
        (
            "object_update",
            "Atomically update scene object properties through editor undo. Use editor_status for IDs and values. Rotation is quaternion xyzw; vectors are xyz. Components use the returned JSON structure.",
            json!({"type": "object", "required": ["entity_id", "properties"], "properties": {"entity_id": {"type": "string"}, "properties": {"type": "object", "additionalProperties": false, "properties": {"name": {"type": "string"}, "parent": {"type": ["string", "null"]}, "part_attributes": {"type": "object"}, "camera": {"type": ["object", "null"]}, "light": {"type": ["object", "null"]}, "scripts": {"type": "array", "items": {"type": "object"}}, "translation": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, "rotation": {"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4}, "scale": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}}}}}),
        ),
        (
            "object_create",
            "Create an authored folder, empty object, camera or light, optionally inside parent (null means scene root). Folders organize the scene explorer. For finished meshes use model_create; primitives are for deliberate blockouts/colliders.",
            json!({"type":"object","required":["name"],"properties":{"name":{"type":"string"},"kind":{"type":"string","enum":["empty","folder","camera","light","blockout"]},"parent":{"type":["string","null"],"description":"Existing destination object/folder UUID, or null for scene root; defaults to root."}}}),
        ),
        (
            "object_duplicate",
            "Duplicate an object or folder and all descendants as one undoable edit, with fresh IDs. Defaults to the original parent; parent:null places the copy at scene root. Optional name renames the copy root. Local transforms, folder flags, assets and script attachments are retained.",
            json!({"type":"object","required":["entity_id"],"properties":{"entity_id":{"type":"string"},"parent":{"type":["string","null"]},"name":{"type":"string"}},"additionalProperties":false}),
        ),
        (
            "object_delete",
            "Delete an object or folder and every descendant from the active scene as one undoable edit. Inspect editor_status first to understand the subtree. Asset and source files remain available. Save with scene_save; editor_undo restores the subtree.",
            json!({"type":"object","required":["entity_id"],"properties":{"entity_id":{"type":"string"}},"additionalProperties":false}),
        ),
        (
            "script_attach",
            "Attach or detach a registered source script at global, scene or object scope. Scope belongs to each attachment, not the source. Global persists across scenes; scene follows scene lifetime; object is a component on entity_id. Set enabled/order for startup references or properties for an object attachment.",
            json!({"type":"object","required":["path","scope"],"properties":{"path":{"type":"string"},"scope":{"type":"string","enum":["global","scene","object"]},"entity_id":{"type":"string"},"enabled":{"type":"boolean"},"execution_order":{"type":"integer"},"properties":{"type":"object"},"detach":{"type":"boolean"}}}),
        ),
        (
            "model_create",
            "Import a Blender-exported model via path, or run an agent-only Lua mesh script via source and save its OBJ at path. Creates a real mesh object in the authored scene. Preview first with model_preview. Never attach mesh recipes to gameplay.",
            json!({"type":"object","required":["path","name"],"properties":{"path":{"type":"string"},"name":{"type":"string"},"source":{"type":"string"}}}),
        ),
        (
            "model_preview",
            "Return a PNG of a model BEFORE placing it. Supply project asset path or bounded Lua mesh source. Orbit about the bounds center using yaw_degrees, pitch_degrees and distance (zoom); optional center overrides the target. Does not change the scene.",
            json!({"type":"object","properties":{"path":{"type":"string"},"source":{"type":"string"},"yaw_degrees":{"type":"number"},"pitch_degrees":{"type":"number"},"distance":{"type":"number","exclusiveMinimum":0},"center":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3}}}),
        ),
        (
            "playtest_control",
            "Start, pause, resume, step or stop the real editor playtest. Start saves the authored scene. Runtime changes are never silently applied to authoring.",
            json!({"type":"object","required":["action"],"properties":{"action":{"type":"string","enum":["start","pause","resume","step","stop"]}}}),
        ),
        (
            "playtest_keys",
            "Hold game keyboard keys through the plugin for duration_ms (1..5000, default 100). Use keys such as KeyW, Space, ArrowLeft. Empty keys releases all. Input targets only the playtest, releases automatically, and never types into desktop applications.",
            json!({"type":"object","required":["keys"],"properties":{"keys":{"type":"array","items":{"type":"string"},"maxItems":32},"duration_ms":{"type":"integer","minimum":1,"maximum":5000}}}),
        ),
        (
            "viewport_capture",
            "Return the latest actual editor/playtest PNG plus frame sequence and scene metadata. Retry if rendering is pending; does not generate a mock image.",
            empty.clone(),
        ),
        (
            "editor_undo",
            "Undo the most recent authored scene edit.",
            empty.clone(),
        ),
        (
            "editor_redo",
            "Redo the most recently undone scene edit.",
            empty,
        ),
    ] {
        tools.push(json!({"name":name,"description":description,"inputSchema":schema}));
    }
    tools
}

pub fn contains(name: &str) -> bool {
    tools().iter().any(|tool| tool["name"] == name)
}
