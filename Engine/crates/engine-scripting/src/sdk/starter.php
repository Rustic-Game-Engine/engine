<?php
require __DIR__ . "/rustic.php";

function on_start(): void {
    global $rustic;
    $rustic->log("info", "Behavior started");
}
function fixed_update(float $dt): void {
    global $rustic;
    [$x, $y, $z] = $rustic->get_translation();
    if ($rustic->key("KeyW")["held"]) $rustic->set_translation($x, $y, $z + $dt);
}
rustic_run(["on_start"=>"on_start", "fixed_update"=>"fixed_update"]);
