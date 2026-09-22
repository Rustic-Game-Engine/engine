# Scripting Rustic games with Java

Java scripts are isolated protocol programs compiled and run by Rustic. The source is
always staged as `RusticBehavior.java`, so the entry class must be named
`RusticBehavior`; keep it package-less. Rustic requires both `java` and `javac` from
OpenJDK 11 or newer on `PATH` and compiles into a temporary classes directory.

Create the `.java` asset and select global, scene, or component scope in the editor.
The attachment stores the Asset ID. Do not add manifest JSON or edit a `.scene` file.

## Current Java adapter level

The generated starter supplies a minimal `InstanceApi` and `GameApi`, but deliberately
does not include a general JSON parser or the full `rustic` API. It reads one request
line, clears its command list, and emits one response. To react to callbacks,
translations, properties, input, or scene paths, add a bounds-safe JSON parser to the
single source file and inspect the request fields described below.

The required shape remains:

```java
class RusticBehavior {
    public static void main(String[] args) throws Exception {
        var input = new java.io.BufferedReader(
            new java.io.InputStreamReader(System.in));
        String request;
        while ((request = input.readLine()) != null) {
            // Parse request and create zero or more command JSON objects.
            System.out.println(
                "{\"format_version\":1,\"commands\":[]}");
            System.out.flush();
        }
    }
}
```

Use a proper serializer for dynamic strings. Standard output must contain only the
one-line responses; send diagnostics to standard error or return a `log` command.

## Requests and lifecycle

Each request contains `format_version`, `callback`, nullable `delta`, `entity_id`,
`delta_time`, `fixed_delta_time`, `translation`, `properties`, `actions`, `attributes`,
`scene_paths`, `keys`, `key_events`, and `any_key_pressed`. External callback names
are `on_create`, `on_start`, `on_enable`, `fixed_update`, `update`, `on_disable`,
`on_destroy`, and `on_stop`. Always answer callbacks you ignore. `actions` is currently
empty for external adapters; raw key state is populated. Collision callbacks are not
currently represented by the process protocol.

The populated raw key state is currently limited to held WASD, arrow, and Shift
keys from the editor's embedded Play viewport. Press/release events and other
key names are not forwarded. See the [Lua input guide](scriptingLua.md#input)
for the supported names and focus steps.

## Commands and generated helpers

Valid response operations are:

```json
{"format_version":1,"commands":[
  {"op":"set_translation","value":[1,2,3]},
  {"op":"set_property","name":"health","value":90},
  {"op":"edit_attribute","name":"Color","value":[1,1,1]},
  {"op":"log","level":"info","message":"started"},
  {"op":"set_enabled","enabled":false},
  {"op":"set_current_camera","source":"Room.Camera"},
  {"op":"add_instance","source":"Cube","parent":null},
  {"op":"clone_instance","source":"Room.Table","parent":null}
]}
```

The starter's `InstanceApi` implements `instance.add(source)`,
`instance.add(source, parent)`, `instance.clone(source)`, and
`instance.clone(source, parent)`. Its `GameApi` implements
`Game.setCurrentCamera(source)`. These append already-serialized commands. The starter
does not currently expose `Game.scene.Find`, property accessors, translation accessors,
logging helpers, or typed input wrappers; obtain reads from the parsed request and
append the documented commands for writes.

Property and attribute writes must preserve the existing engine type. Built-ins are
`Name`, `Position`, `Size`, `Color`, `CanTouch`, `CanCollide`, `Anchored`, and
`Parent`. Instance sources accept stable IDs, scene paths, Explorer model paths, or
built-in object names; `parent` must be null or a stable entity ID.

## Runtime containment

Java cannot call another behavior process directly. API 1.0 does not yet expose an
Engine Event `emit`/`subscribe` command; cross-language coordination must use shared
engine state.

The process persists, so static state belongs to one behavior instance and survives
between callbacks. Global scripts start before scene scripts, followed by component
scripts; explicit order and stable IDs resolve ties. The working directory and class
output are temporary, and the environment is reduced to a safe allowlist. A callback
must answer within three seconds and under 1 MiB. Invalid JSON, an unsupported command,
a timeout, or process exit disables the instance. Compilation/reload failure preserves
the last-known-good instance.
