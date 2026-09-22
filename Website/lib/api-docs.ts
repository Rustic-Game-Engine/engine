type Parameter = { name: string; type: string; required: boolean; description: string };
type ApiDoc = { title: string; summary: string; when: string; calls: string[]; parameters: Parameter[]; returns: string; examples: Record<string, string>; notes?: string[] };

const languages = ["Lua 5.4", "JavaScript", "Python", "C++", "C#", "Luau", "C", "Java", "PHP", "HTML / inline JS"];
const protocolNote = "Luau, C, Java, and PHP use the external version-1 JSON host protocol. Their examples show request fields or response commands; each callback must read one JSON line and write one JSON response line.";
const p = (name: string, type: string, required: boolean, description: string): Parameter => ({ name, type, required, description });

const docs: Record<string, ApiDoc> = {
  entity: {
    title: "Entity & time API", summary: "Identify the behavior owner and read engine-supplied frame intervals.",
    when: "Read the entity ID when another system needs a stable reference to this owner. Use `delta_time` with `Update` for frame-rate-independent presentation and `fixed_delta_time` with `FixedUpdate` for simulation.",
    calls: ["rustic.entity_id()", "rustic.delta_time()", "rustic.fixed_delta_time()"], parameters: [],
    returns: "The ID is a stable string. Time values are finite seconds.",
    examples: {
      "Lua 5.4": "local id = rustic.entity_id()\nlocal dt = rustic.delta_time()\nlocal fixedDt = rustic.fixed_delta_time()",
      JavaScript: "const id = rustic.entity_id();\nconst dt = rustic.delta_time();\nconst fixedDt = rustic.fixed_delta_time();",
      Python: "entity_id = rustic.entity_id()\ndt = rustic.delta_time()\nfixed_dt = rustic.fixed_delta_time()",
      "C++": "std::string id = rustic.entity_id();\ndouble dt = rustic.delta_time();\ndouble fixedDt = rustic.fixed_delta_time();",
      "C#": "string id = rustic.entity_id();\ndouble dt = rustic.delta_time();\ndouble fixedDt = rustic.fixed_delta_time();",
      Luau: "local id = request.entity_id\nlocal dt = request.delta_time\nlocal fixedDt = request.fixed_delta_time",
      C: "const char *id = json_string(request, \"entity_id\");\ndouble dt = json_number(request, \"delta_time\");",
      Java: "String id = request.getString(\"entity_id\");\ndouble dt = request.getDouble(\"delta_time\");",
      PHP: "$id = $request['entity_id'];\n$dt = $request['delta_time'];\n$fixedDt = $request['fixed_delta_time'];",
      "HTML / inline JS": "const id = rustic.entity_id();\nconst dt = rustic.delta_time();",
    }, notes: [protocolNote],
  },
  transforms: {
    title: "Transform API", summary: "Read or queue a world-space translation for the behavior owner.",
    when: "Read before applying relative movement. Write from `Update` for presentation movement or `FixedUpdate` for simulation movement. Do not rewrite an unchanged position every frame.",
    calls: ["rustic.get_translation()", "rustic.set_translation(x, y, z)"],
    parameters: [p("x", "finite number", true, "World-space X."), p("y", "finite number", true, "World-space Y."), p("z", "finite number", true, "World-space Z.")],
    returns: "Get returns three numbers. Set queues a mutation applied after the callback; it returns no entity value.",
    examples: {
      "Lua 5.4": "local x, y, z = rustic.get_translation()\nrustic.set_translation(x + 1, y, z)",
      JavaScript: "const [x, y, z] = rustic.get_translation();\nrustic.set_translation(x + 1, y, z);",
      Python: "x, y, z = rustic.get_translation()\nrustic.set_translation(x + 1, y, z)",
      "C++": "auto pos = rustic.get_translation();\nrustic.set_translation(pos.x + 1.0, pos.y, pos.z);",
      "C#": "double[] pos = rustic.get_translation();\nrustic.set_translation(pos[0] + 1, pos[1], pos[2]);",
      Luau: "local pos = request.translation\ncommands[#commands+1] = {op='set_translation', value={pos[1]+1,pos[2],pos[3]}}",
      C: "double x = json_array_number(request, \"translation\", 0);\ncommand_set_translation(commands, x + 1, y, z);",
      Java: "var pos = request.getArray(\"translation\");\ncommands.add(setTranslation(pos.getDouble(0)+1, pos.getDouble(1), pos.getDouble(2)));",
      PHP: "$pos = $request['translation'];\n$commands[] = ['op'=>'set_translation','value'=>[$pos[0]+1,$pos[1],$pos[2]]];",
      "HTML / inline JS": "const [x, y, z] = rustic.get_translation();\nrustic.set_translation(x + 1, y, z);",
    }, notes: ["NaN and infinity are rejected.", protocolNote],
  },
  properties: {
    title: "Public property API", summary: "Read and update typed properties declared for this behavior.",
    when: "Use properties for designer-configurable values such as speed, health, and damage. Read at setup or when needed; write only when gameplay changes the value.",
    calls: ["rustic.get_property(name)", "rustic.set_property(name, value)"],
    parameters: [p("name", "string", true, "Exact declared property name."), p("value", "EngineValue", true, "Value matching the property's existing type.")],
    returns: "Get returns the stored value. Set queues an update and cannot create a missing property.",
    examples: {
      "Lua 5.4": "local health = rustic.get_property('health')\nrustic.set_property('health', health - 10)",
      JavaScript: "const health = rustic.get_property('health');\nrustic.set_property('health', health - 10);",
      Python: "health = rustic.get_property('health')\nrustic.set_property('health', health - 10)",
      "C++": "double health = rustic.get_property(\"health\").number();\nrustic.set_property(\"health\", RusticValue{health - 10});",
      "C#": "double health = rustic.get_property(\"health\").GetDouble();\nrustic.set_property(\"health\", health - 10);",
      Luau: "local health = request.properties.health\ncommands[#commands+1] = {op='set_property',name='health',value=health-10}",
      C: "double health = json_object_number(request, \"properties\", \"health\");\ncommand_set_property_number(commands, \"health\", health - 10);",
      Java: "double health = request.getObject(\"properties\").getDouble(\"health\");\ncommands.add(setProperty(\"health\", health - 10));",
      PHP: "$health = $request['properties']['health'];\n$commands[] = ['op'=>'set_property','name'=>'health','value'=>$health-10];",
      "HTML / inline JS": "const health = rustic.get_property('health');\nrustic.set_property('health', health - 10);",
    }, notes: ["Unknown names and type changes are rejected. Boundary values are booleans, integers, finite numbers, strings, vectors, and optional entity IDs.", protocolNote],
  },
  attributes: {
    title: "Built-in attribute API", summary: "Read or edit engine-owned attributes on the behavior entity.",
    when: "Use attributes for built-in entity state. Prefer `set_translation` for Position and `set_enabled` for Enabled when those dedicated calls better express intent.",
    calls: ["rustic.get_attribute(name)", "rustic.edit_attribute(name, value)"],
    parameters: [p("name", "string", true, "Name, Position, Size, Color, Material, Parent, or Enabled."), p("value", "attribute-specific", true, "Value matching the attribute type.")],
    returns: "Get returns the current value. Edit queues a typed mutation.",
    examples: {
      "Lua 5.4": "local name = rustic.get_attribute('Name')\nrustic.edit_attribute('Color', {1, .5, 0, 1})",
      JavaScript: "const name = rustic.get_attribute('Name');\nrustic.edit_attribute('Color', [1, .5, 0, 1]);",
      Python: "name = rustic.get_attribute('Name')\nrustic.edit_attribute('Color', [1, .5, 0, 1])",
      "C++": "auto name = rustic.get_attribute(\"Name\");\nrustic.edit_attribute(\"Color\", color);",
      "C#": "var name = rustic.GetAttribute(\"Name\");\nrustic.EditAttribute(\"Color\", color);",
      Luau: "local name = request.attributes.Name\ncommands[#commands+1]={op='edit_attribute',name='Color',value={1,.5,0,1}}",
      C: "const char *name = json_object_string(request, \"attributes\", \"Name\");",
      Java: "String name = request.getObject(\"attributes\").getString(\"Name\");",
      PHP: "$name=$request['attributes']['Name'];\n$commands[]=['op'=>'edit_attribute','name'=>'Color','value'=>[1,.5,0,1]];",
      "HTML / inline JS": "const name = rustic.get_attribute('Name');\nrustic.edit_attribute('Color', [1, .5, 0, 1]);",
    }, notes: ["Unknown attributes and type changes are rejected. Parent accepts an optional entity value.", protocolNote],
  },
  input: {
    title: "Input API", summary: "Read held keys forwarded from the editor's embedded Play viewport.",
    when: "For movement, read `rustic.key(name).held` in `FixedUpdate`. Hover or click the Play viewport first. This build forwards WASD, arrow keys, and Shift only. Named actions and press/release events are not connected yet.",
    calls: ["rustic.input(action)", "rustic.key(key)", "rustic.key_events()", "rustic.any_key_pressed()"],
    parameters: [p("action", "string", true, "Configured action name; currently returns an inactive state."), p("key", "string", true, "KeyW, KeyA, KeyS, KeyD, an Arrow direction, ShiftLeft, or ShiftRight.")],
    returns: "Key reads return `{ pressed, released, held, axis }`. Only `held` and `axis` are populated for forwarded keys; pressed/released stay false, key events stay empty, and any-key stays false.",
    examples: {
      "Lua 5.4": "if rustic.key('KeyW').held then\n  print('forward key is held')\nend",
      JavaScript: "if (rustic.key('KeyW').held) console.debug('forward key is held');",
      Python: "held = request['keys'].get('KeyW', {}).get('held', False)",
      "C++": "RusticActionState forward=rustic.key(\"KeyW\");\nif (forward.held) rustic.log(\"debug\", \"moving\");",
      "C#": "var forward=rustic.key(\"KeyW\");\n// Check forward.GetProperty(\"held\").GetBoolean()",
      Luau: "-- The stock Luau CLI adapter cannot handle repeated input requests yet.",
      C: "bool held=json_key_held(request, \"KeyW\");",
      Java: "boolean held=request.getObject(\"keys\").getObject(\"KeyW\").getBoolean(\"held\");",
      PHP: "$held=$request['keys']['KeyW']['held'] ?? false;",
      "HTML / inline JS": "if (rustic.key('KeyW').held) console.debug('forward key is held');",
    }, notes: ["The embedded Play viewport forwards held KeyW, KeyA, KeyS, KeyD, ArrowUp, ArrowDown, ArrowLeft, ArrowRight, ShiftLeft, and ShiftRight. It does not forward other keys or input from New Window or Standalone. Follow the [Lua controller guide](scriptingLua.md#input) for attachment steps, a complete example, and troubleshooting.", protocolNote],
  },
  logging: {
    title: "Logging API", summary: "Send a bounded diagnostic message to the live game console.",
    when: "Log lifecycle transitions and recoverable errors. Avoid per-frame logging outside short debugging sessions because bounded queues can drop excessive output.",
    calls: ["rustic.log(level, message, fields?)", "print(...) — Lua and JavaScript info", "warn(...) — Lua and JavaScript warning"],
    parameters: [p("level", "string", true, "debug, info, warn, or error."), p("message", "string", true, "Human-readable diagnostic text."), p("fields", "table / object", false, "Optional structured fields where supported; omit for portability.")],
    returns: "No value. The message is queued for the bounded console sink.",
    examples: simpleMutationExamples("rustic.log('info', 'player spawned')", "rustic.log(\"info\", \"player spawned\");", "log", "level", "info", "message", "player spawned"),
    notes: ["In Lua and JavaScript, print(...) writes an info entry and warn(...) writes a warning entry to the Rustic Console. Lua separates multiple arguments with tabs; JavaScript separates them with spaces. Use rustic.log for an explicit level. External programs must not print diagnostics to stdout; stdout is reserved for protocol responses.", protocolNote],
  },
  enabled: {
    title: "Enabled state API", summary: "Queue an enabled-state change for the behavior owner.",
    when: "Disable an entity when its behavior should stop after the current callback. Re-enable it from a controlling behavior or editor action. Use a local boolean instead when the entity should remain active.",
    calls: ["rustic.set_enabled(enabled)"], parameters: [p("enabled", "boolean", true, "True to enable; false to disable.")],
    returns: "No value. The change applies after the callback and can trigger enable/disable lifecycle callbacks.",
    examples: simpleMutationExamples("rustic.set_enabled(false)", "rustic.set_enabled(false);", "set_enabled", "enabled", false), notes: [protocolNote],
  },
  scene: {
    title: "Scene lookup API", summary: "Resolve stable scene paths to entity IDs and list paths in the play snapshot.",
    when: "Resolve references during `Start` and cache the stable ID when possible. List paths for discovery and tooling, not every frame.",
    calls: ["Game.scene.Find(path)", "Game.scene.List()", "rustic.find_entity(name_or_id)"], parameters: [p("path", "string", true, "Stable scene path, name, or entity ID accepted by the adapter.")],
    returns: "Find returns an entity ID or no value. List returns path strings.",
    examples: {
      "Lua 5.4": "local id,err=rustic.find_entity('Room.Player')\nlocal paths=Game.scene.List()", JavaScript: "const id=Game.scene.Find('Room.Player');\nconst paths=Game.scene.List();", Python: "entity_id=Game.scene.Find('Room.Player')\npaths=Game.scene.List()", "C++": "auto id=Game.scene.Find(\"Room.Player\");\nauto paths=Game.scene.List();", "C#": "string? id=Game.scene.Find(\"Room.Player\");\nvar paths=Game.scene.List();", Luau: "local id=request.scene_paths['Room.Player']", C: "const char *id=json_object_string(request,\"scene_paths\",\"Room.Player\");", Java: "String id=request.getObject(\"scene_paths\").getString(\"Room.Player\");", PHP: "$id=$request['scene_paths']['Room.Player']??null;\n$paths=array_keys($request['scene_paths']);", "HTML / inline JS": "const id=Game.scene.Find('Room.Player');\nconst paths=Game.scene.List();",
    }, notes: [protocolNote],
  },
  instances: {
    title: "Instance creation API", summary: "Queue adding a source instance or cloning an existing entity.",
    when: "Call for bounded gameplay events such as spawning a projectile, enemy, pickup, or effect. Avoid unbounded per-frame creation.",
    calls: ["instance.add(source, parent?)", "instance.clone(source, parent?)"], parameters: [p("source", "string", true, "Source asset or stable scene path."), p("parent", "string or null", false, "Destination parent path/ID; omit for the default root.")],
    returns: "No new entity ID. Creation is queued and applies after the callback in issue order.",
    examples: {
      "Lua 5.4": "instance.add('Game.scene.Prefabs.Crate', nil)\ninstance.clone('Game.scene.Enemy','Game.scene.Room')", JavaScript: "instance.add('Game.scene.Prefabs.Crate');\ninstance.clone('Game.scene.Enemy','Game.scene.Room');", Python: "instance.add('Game.scene.Prefabs.Crate',None)\ninstance.clone('Game.scene.Enemy','Game.scene.Room')", "C++": "instance.add(\"Game.scene.Prefabs.Crate\",std::nullopt);", "C#": "instance.add(\"Game.scene.Prefabs.Crate\",null);", Luau: "commands[#commands+1]={op='add_instance',source='Game.scene.Prefabs.Crate'}", C: "command_add_instance(commands,\"Game.scene.Prefabs.Crate\",NULL);", Java: "commands.add(addInstance(\"Game.scene.Prefabs.Crate\",null));", PHP: "$commands[]=['op'=>'add_instance','source'=>'Game.scene.Prefabs.Crate'];", "HTML / inline JS": "instance.add('Game.scene.Prefabs.Crate');",
    }, notes: [protocolNote],
  },
  camera: {
    title: "Current camera API", summary: "Select the camera used for the next game render.",
    when: "Call when gameplay changes viewpoints—vehicles, players, cutscenes, or returning to the main camera. Do not call every frame when unchanged.",
    calls: ["Game.setCurrentCamera(source)"], parameters: [p("source", "string or camera reference", true, "Stable camera entity ID or scene path.")],
    returns: "No camera object. Selection is queued for the next render.",
    examples: simpleMutationExamples("Game.setCurrentCamera('Game.scene.Room.Camera')", "Game.setCurrentCamera(\"Game.scene.Room.Camera\");", "set_current_camera", "source", "Game.scene.Room.Camera"),
    notes: ["The target must resolve to a camera in the play snapshot.", protocolNote],
  },
};

function simpleMutationExamples(dynamic: string, compiled: string, op: string, ...pairs: unknown[]): Record<string, string> {
  const command = `{\"op\":\"${op}\"${Array.from({ length: pairs.length / 2 }, (_, i) => `,\"${pairs[i * 2]}\":${JSON.stringify(pairs[i * 2 + 1])}`).join("")}}`;
  return { "Lua 5.4": dynamic, JavaScript: dynamic.replaceAll("'", "'") + (dynamic.endsWith(")") ? ";" : ""), Python: dynamic.replace("false", "False"), "C++": compiled, "C#": compiled, Luau: `commands[#commands+1]=${command}`, C: `commands_push_json(commands, ${JSON.stringify(command)});`, Java: `commands.add(parseCommand(${JSON.stringify(command)}));`, PHP: `$commands[] = json_decode(${JSON.stringify(command)}, true);`, "HTML / inline JS": dynamic + ";" };
}

export function buildApiMarkdown(id: string) {
  if (id === "overview") return overview();
  if (id === "callbacks") return callbacks();
  const doc = docs[id];
  if (!doc) throw new Error(`Unknown API document: ${id}`);
  const variables = doc.parameters.length ? `| Variable | Type | Required | Description |\n| --- | --- | --- | --- |\n${doc.parameters.map((x) => `| \`${x.name}\` | ${x.type} | ${x.required ? "Yes" : "No"} | ${x.description} |`).join("\n")}` : "This API has no arguments.";
  const examples = languages.map((lang) => `### ${lang}\n\n\`\`\`${codeLanguage(lang)}\n${doc.examples[lang]}\n\`\`\``).join("\n\n");
  const notes = doc.notes?.length ? `\n\n## Constraints and behavior\n\n${doc.notes.map((x) => `- ${x}`).join("\n")}` : "";
  return `# ${doc.title}\n\n${doc.summary}\n\n## When to call it\n\n${doc.when}\n\n## Calls\n\n${doc.calls.map((x) => `- \`${x}\``).join("\n")}\n\n## Variables\n\n${variables}\n\n## Return and timing\n\n${doc.returns}\n\n## Language examples\n\n${examples}${notes}`;
}

function codeLanguage(lang: string) { return ({ "Lua 5.4": "lua", JavaScript: "javascript", Python: "python", "C++": "cpp", "C#": "csharp", Luau: "lua", C: "c", Java: "java", PHP: "php", "HTML / inline JS": "javascript" } as Record<string, string>)[lang]; }
function overview() { return `# Rustic scripting API\n\nThe reference is organized by what game code needs to do. Every operation page includes purpose, timing, variables, return behavior, and equivalent calls for all ten supported script types.\n\n## Execution model\n\nGlobal scripts run before Scene scripts, then Object Component scripts. Mutations are queued and applied after the callback in issue order. Lua, JavaScript, and Web scripts call embedded APIs; external languages exchange newline-delimited version-1 JSON.\n\n## Language support\n\n| Languages | Integration |\n| --- | --- |\n| Lua 5.4, JavaScript, HTML / inline JS | bundled embedded API |\n| Python, C#, C++ | external host with helper surface |\n| Luau, C, Java, PHP | external JSON host protocol |\n\n## Safety limits\n\nSource and responses are limited to 1 MiB. External callbacks have a three-second deadline. Boundary values are booleans, integers, finite numbers, strings, vectors, and optional stable entity IDs.`; }
function callbacks() { return `# Lifecycle callbacks\n\nCallbacks are entry points invoked by Rustic. Define only those a behavior needs.\n\n## When each callback runs\n\n| Callback | When to use it | Variables |\n| --- | --- | --- |\n| \`OnCreate\` | One-time construction before startup. | None |\n| \`Start\` | Resolve references and initialize gameplay state. | None |\n| \`OnEnable\` | Resume state when the owner becomes enabled. | None |\n| \`FixedUpdate\` | Physics and deterministic simulation. | \`dt\`: required fixed-step seconds |\n| \`Update\` | Input, presentation, timers, and per-frame logic. | \`dt\`: required frame seconds |\n| \`OnDisable\` | Pause state when the owner becomes disabled. | None |\n| \`OnDestroy\` | Release instance resources before destruction. | None |\n| \`OnStop\` | Final play-session cleanup. | None |\n\n## Lua 5.4\n\n\`\`\`lua\nreturn { Start=function() end, Update=function(dt) end, FixedUpdate=function(dt) end, OnDestroy=function() end }\n\`\`\`\n\n## JavaScript and HTML / inline JS\n\n\`\`\`javascript\nglobalThis.behavior={Start(){},Update(dt){},FixedUpdate(dt){},OnDestroy(){}};\n\`\`\`\n\n## External languages\n\nPython, C#, C++, Luau, C, Java, and PHP dispatch \`request.callback\` with optional \`request.delta\`. Wire names are \`on_create\`, \`on_start\`, \`on_enable\`, \`fixed_update\`, \`update\`, \`on_disable\`, \`on_destroy\`, and \`on_stop\`. Always answer ignored callbacks with \`{"format_version":1,"commands":[]}\`. Collision callbacks are not currently sent to external hosts.`; }
