# Scripting Rustic games with PHP

PHP is supported as a UI-scoped script asset: `.php` files must live under `ui/`.
Rustic syntax-checks them with `php -l`, then runs one PHP CLI process per behavior
instance. Install PHP CLI on `PATH` and verify discovery with `cargo xtask doctor`.

Create and attach the file through the Explorer as global, scene, or component scope.
Its Asset ID survives editor moves/renames within `ui/`; moving it outside `ui/`
makes it invalid. Never edit the script registry or scene metadata.

## Dispatch lifecycle requests

The generated starter defines `$instance`, `$rustic`, and `$Game`. Add callback
dispatch inside its persistent loop:

```php
<?php
$speed = 4.0;

while (($line = fgets(STDIN)) !== false) {
    $request = json_decode($line, true, flags: JSON_THROW_ON_ERROR);
    $instance->commands = [];
    $rustic->state = $request;
    $Game->scene->state = $request["scene_paths"] ?? [];

    switch ($request["callback"]) {
        case "on_start":
            $rustic->log("info", "started " . $rustic->entity_id());
            break;
        case "fixed_update":
            $dt = (float)($request["delta"] ?? 0.0);
            if ($rustic->key("KeyW")["held"]) {
                [$x, $y, $z] = $rustic->get_translation();
                $rustic->set_translation($x, $y, $z + $speed * $dt);
            }
            break;
    }

    echo json_encode(["format_version" => 1,
                      "commands" => $instance->commands],
                     JSON_THROW_ON_ERROR), PHP_EOL;
    flush();
}
```

Wire callbacks are `on_create`, `on_start`, `on_enable`, `fixed_update`, `update`,
`on_disable`, `on_destroy`, and `on_stop`. Send one response for every request.
Collision callbacks are not yet in the external protocol.

## Generated API

```php
$id = $rustic->entity_id();
$dt = $rustic->delta_time();
$fixedDt = $rustic->fixed_delta_time();
[$x, $y, $z] = $rustic->get_translation();
$rustic->set_translation($x + 1, $y, $z);

$health = $rustic->get_property("health");
$rustic->set_property("health", 90);
$color = $rustic->GetAttribute("Color");
$rustic->EditAttribute("Anchored", true);
$key = $rustic->key("Space");
$events = $rustic->key_events();
$any = $rustic->any_key_pressed();
$rustic->log("info", "message");
$rustic->set_enabled(false);

$table = $Game->scene->Find("Room.Table");
$room = $Game->scene->List("Room");
$Game->setCurrentCamera("Room.Camera");
$instance->add("Cube");
$instance->clone("assets/models/chair.glb", $table);
```

Lower-case attribute aliases are available. `get_property`/`GetAttribute` return
`null` when missing. Named actions have no generated PHP helper and the external
`actions` object is currently empty; use raw `key` and `key_events`.

Writes are queued and applied in response order. They must preserve property and
attribute types. Built-ins are `Name`, `Position`, `Size`, `Color`, `CanTouch`,
`CanCollide`, `Anchored`, and `Parent`. An instance source may be an ID, scene path,
model path, or built-in name; optional parents must be stable IDs.

## Protocol and safety

PHP cannot call another behavior process directly. API 1.0 does not yet expose an
Engine Event `emit`/`subscribe` command, so cross-language coordination must use
shared engine state.

Requests also include `format_version`, nullable `delta`, timing fields, translation,
properties, scene paths, and raw input. Responses use protocol version 1 and may
contain only supported Script API commands. Standard output is reserved for the
response; use `$rustic->log()` or STDERR for diagnostics.

The CLI process has a cleared/reduced environment, temporary working directory,
three-second callback deadline, and 1 MiB response cap. Its globals persist only for
that behavior instance. Invalid JSON, uncaught exceptions, timeouts, or rejected
commands disable the instance. A syntax or reload failure leaves the previous valid
instance running.
