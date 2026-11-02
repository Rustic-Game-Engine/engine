use serde_json::{Value, json};

pub fn tools() -> Vec<Value> {
    let empty = json!({"type":"object","properties":{},"additionalProperties":false});
    let scene = json!({"type":"object","required":["path"],"properties":{"path":{"type":"string","description":"Relative to the configured scenes folder, e.g. levels/forest.scene"}},"additionalProperties":false});
    let mut tools = texture_tools();
    for (name, description, schema) in [
        (
            "editor_status",
            "Read active scene, unsaved status, full object properties, script scopes and play state from the open editor.",
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
            "Create an authored empty object, camera or light. For visible finished objects use model_create. Primitives are only for deliberate blockouts/colliders.",
            json!({"type":"object","required":["name"],"properties":{"name":{"type":"string"},"kind":{"type":"string","enum":["empty","camera","light","blockout"]}}}),
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

fn texture_tools() -> Vec<Value> {
    let slots = [
        "BaseColor",
        "Normal",
        "Roughness",
        "Metallic",
        "AmbientOcclusion",
        "Height",
        "Emissive",
        "Clearcoat",
        "ClearcoatRoughness",
        "ClearcoatNormal",
        "Transmission",
        "Thickness",
        "SheenColor",
        "SheenRoughness",
        "Specular",
        "SpecularColor",
        "Anisotropy",
    ];
    let maps: serde_json::Map<String, Value> = slots
        .into_iter()
        .map(|slot| (slot.into(), json!({"type":["string","null"]})))
        .collect();
    let pair = json!({"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2});
    let edit = json!({"type":"object","required":["entity_id"],"additionalProperties":false,"properties":{
        "entity_id":{"type":"string"},
        "maps":{"type":"object","properties":maps,"additionalProperties":false,"description":"Partial slot update. Project-relative PNG/JPEG/TGA/HDR paths; null clears a slot. Unspecified slots remain."},
        "clear_maps":{"type":"boolean","description":"Clear existing slots before applying maps; retains the texture component and controls."},
        "uv_scale":pair,"uv_offset":pair,
        "height_scale":{"type":"number","minimum":-100,"maximum":100},
        "emissive_strength":{"type":"number","minimum":0,"maximum":10000}
    }});
    let mut child = edit.clone();
    child["properties"]["name"] =
        json!({"type":"string","description":"Optional child name; defaults to Texture."});
    let color =
        json!({"type":"array","items":{"type":"number","minimum":0},"minItems":3,"maxItems":3});
    let vector = json!({"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3});
    let positive = json!({"type":"number","minimum":0});
    vec![
        json!({"name":"environment_update","description":"Patch the scene sky/atmosphere through editor undo. Use editor_status.environment for current settings. A project-relative 2:1 HDR panorama belongs in sky_image; empty restores solid sky. Validates and imports the image before editing. Save with scene_save.","inputSchema":{"type":"object","required":["settings"],"additionalProperties":false,"properties":{"settings":{"type":"object","additionalProperties":false,"properties":{
            "enabled":{"type":"boolean"},"sky_image":{"type":"string"},"sky_color":color,"rotation_degrees":{"type":"number"},"exposure":{"type":"number","minimum":-20,"maximum":20},
            "ambient_color":color,"ambient_intensity":positive,"sun_color":color,"sun_intensity":positive,"sun_direction":vector,"haze_color":color,"haze_density":positive,"haze_start":positive
        }}}}}),
        json!({"name":"texture_inspect","description":"Inspect direct texture maps, effective maps after Texture-child overrides, and immediate texture children for an object in the live editor.","inputSchema":{"type":"object","required":["entity_id"],"properties":{"entity_id":{"type":"string"}},"additionalProperties":false}}),
        json!({"name":"texture_update","description":"Atomically edit optional HDR/PBR slots and UV/intensity controls on a mesh, primitive or existing Texture child through editor undo. Paths must be existing project-relative images. Validates all images before attaching; use null to clear slots. Save with scene_save.","inputSchema":edit}),
        json!({"name":"texture_create_child","description":"Create a Texture child of the mesh/primitive identified by entity_id, optionally with maps and controls, as one undoable scene edit. Child maps apply only to the immediate parent. Save with scene_save.","inputSchema":child}),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn texture_schema_tracks_renderer_slots_and_exposes_partial_updates() {
        let tools = tools();
        let update = tools
            .iter()
            .find(|t| t["name"] == "texture_update")
            .unwrap();
        let properties = &update["inputSchema"]["properties"];
        let maps = properties["maps"]["properties"].as_object().unwrap();
        assert_eq!(maps.len(), engine_world::TextureMap::ALL.len());
        for map in engine_world::TextureMap::ALL {
            let name = serde_json::to_value(map).unwrap();
            assert_eq!(
                maps[name.as_str().unwrap()]["type"],
                json!(["string", "null"])
            );
        }
        assert_eq!(update["inputSchema"]["additionalProperties"], false);
        for name in [
            "texture_inspect",
            "texture_create_child",
            "environment_update",
        ] {
            assert!(contains(name));
        }
    }
}
