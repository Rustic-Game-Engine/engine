<?php
final class InstanceApi {
    public array $commands = [];
    public function add(string $source, ?string $parent = null): void {
        $this->commands[] = ["op" => "add_instance", "source" => $source, "parent" => $parent];
    }
    public function clone(string $source, ?string $parent = null): void {
        $this->commands[] = ["op" => "clone_instance", "source" => $source, "parent" => $parent];
    }
}
final class ObjectPath {
    public function __construct(private InstanceApi $instance, private string $source="rustic.game") {}
    public function __get(string $name): ObjectPath { return new ObjectPath($this->instance, $this->source.".".$name); }
    public function EditAttribute(string $name, mixed $value): void { $this->instance->commands[]=["op"=>"edit_attribute","source"=>$this->source,"name"=>$name,"value"=>$value]; }
    public function edit_attribute(string $name, mixed $value): void { $this->EditAttribute($name,$value); }
}
final class RusticApi {
    public array $state = [];
    public ObjectPath $game;
    public function __construct(public InstanceApi $instance) { $this->game=new ObjectPath($instance); }
    public function entity_id(): string { return $this->state["entity_id"]; }
    public function delta_time(): float { return $this->state["delta_time"]; }
    public function fixed_delta_time(): float { return $this->state["fixed_delta_time"]; }
    public function get_translation(): array { return $this->state["translation"]; }
    public function set_translation(float $x,float $y,float $z): void { $this->instance->commands[]=["op"=>"set_translation","value"=>[$x,$y,$z]]; }
    public function get_property(string $name): mixed { return $this->state["properties"][$name] ?? null; }
    public function set_property(string $name,mixed $value): void { $this->instance->commands[]=["op"=>"set_property","name"=>$name,"value"=>$value]; }
    public function GetAttribute(string $name): mixed { return $this->state["attributes"][$name] ?? null; }
    public function get_attribute(string $name): mixed { return $this->GetAttribute($name); }
    public function EditAttribute(string $name,mixed $value): void { $this->instance->commands[]=["op"=>"edit_attribute","name"=>$name,"value"=>$value]; }
    public function edit_attribute(string $name,mixed $value): void { $this->EditAttribute($name,$value); }
    public function input(string $name): array { return $this->state["actions"][$name] ?? ["pressed"=>false,"released"=>false,"held"=>false,"axis"=>0]; }
    public function key(string $name): array { return $this->state["keys"][$name] ?? ["pressed"=>false,"released"=>false,"held"=>false,"axis"=>0]; }
    public function key_events(): array { return $this->state["key_events"]; }
    public function any_key_pressed(): bool { return $this->state["any_key_pressed"]; }
    public function log(string $level,string $message): void { $this->instance->commands[]=["op"=>"log","level"=>$level,"message"=>$message]; }
    public function set_enabled(bool $enabled): void { $this->instance->commands[]=["op"=>"set_enabled","enabled"=>$enabled]; }
}
final class SceneApi {
    public array $state=[];
    public function Find(string $path): ?string { return $this->state[$path] ?? null; }
    public function List(string $path="Game.scene"): array { return array_values(array_filter($this->state, fn($id,$name)=>$path==="Game.scene" || str_starts_with($name,$path."."), ARRAY_FILTER_USE_BOTH)); }
}
final class GameApi {
    public SceneApi $scene;
    public function __construct(private InstanceApi $instance){ $this->scene=new SceneApi(); }
    public function setCurrentCamera(string $source): void { $this->instance->commands[]=["op"=>"set_current_camera","source"=>$source]; }
}
$instance = new InstanceApi();
$rustic = new RusticApi($instance);
$Game = new GameApi($instance);
function rustic_run(array $callbacks): void {
    global $instance, $rustic, $Game;
    while (($line = fgets(STDIN)) !== false) {
        $rustic->state = json_decode($line, true, flags: JSON_THROW_ON_ERROR);
        $instance->commands = [];
        $Game->scene->state = $rustic->state["scene_paths"];
        $name = $rustic->state["callback"];
        if (isset($callbacks[$name])) {
            if ($name === "update" || $name === "fixed_update") $callbacks[$name]($rustic->state["delta"] ?? 0.0);
            else $callbacks[$name]();
        }
        echo json_encode(["format_version"=>1,"commands"=>$instance->commands], JSON_THROW_ON_ERROR), PHP_EOL;
        flush();
    }
}

