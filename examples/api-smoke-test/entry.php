<?php
while (($line = fgets(STDIN)) !== false) {
    $s = json_decode($line, true, flags: JSON_THROW_ON_ERROR);
    foreach (["callback","delta","entity_id","delta_time","fixed_delta_time","translation",
              "properties","attributes","scene_paths","actions","keys","key_events","any_key_pressed"] as $key) {
        $unused = $s[$key] ?? null;
    }
    $commands = [];
    if ($s["callback"] === "on_start") {
        $commands = [
            ["op"=>"set_translation", "value"=>$s["translation"]],
            ["op"=>"set_property", "name"=>"smoke_value", "value"=>$s["properties"]["smoke_value"] ?? 1.0],
            ["op"=>"log", "level"=>"info", "message"=>"PHP API smoke test passed"],
            ["op"=>"set_enabled", "enabled"=>true],
            ["op"=>"add_instance", "source"=>"Part", "parent"=>null],
            ["op"=>"clone_instance", "source"=>$s["entity_id"], "parent"=>null],
        ];
        if (isset($s["attributes"]["Position"]))
            $commands[] = ["op"=>"edit_attribute", "name"=>"Position", "value"=>$s["attributes"]["Position"]];
    }
    echo json_encode(["format_version"=>1, "commands"=>$commands], JSON_THROW_ON_ERROR), PHP_EOL;
    flush();
}

