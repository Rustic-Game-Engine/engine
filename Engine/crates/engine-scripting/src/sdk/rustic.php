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
    public function query(array $query): mixed {
        echo json_encode(["query"=>$query,"commands"=>$this->instance->commands],JSON_THROW_ON_ERROR),PHP_EOL;flush();$this->instance->commands=[];
        $r=json_decode(fgets(STDIN),true,flags:JSON_THROW_ON_ERROR);if(isset($r["error"]))throw new RuntimeException($r["error"]);return $r["result"];
    }
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
    public function getEnvironment(): array { global $rustic; return $rustic->query(["op"=>"scene_environment"]); }
    public function setEnvironment(array $settings): array { global $rustic; return $rustic->query(["op"=>"scene_environment","settings"=>(object)$settings]); }
    public function setSkyTexture(string $path): array { return $this->setEnvironment(["sky_image"=>$path]); }
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
        if($name==="update")Gameplay::dispatch();
        if (isset($callbacks[$name])) {
            if ($name === "update" || $name === "fixed_update") $callbacks[$name]($rustic->state["delta"] ?? 0.0);
            else $callbacks[$name]();
        }
        echo json_encode(["format_version"=>1,"commands"=>$instance->commands], JSON_THROW_ON_ERROR), PHP_EOL;
        flush();
    }
}


// Structured requests stay inside the SDK; the engine owns the action clock.
final class GameplayHandle {
    public function state():?array{global $rustic;return $rustic->query(["op"=>"operation_state","handle"=>$this->id]);}
    public array $tokens=[],$markers=[];
    public ?Closure $finished=null;
    public function __construct(public string $id) {}
    public function onFinished(Closure $fn): self {$this->finished=$fn;return $this;}
    private function control(string $command): self {
        Gameplay::send(["command"=>$command,"handle"=>$this->id]);
        if($command==="cancel")Gameplay::release($this);return $this;
    }
    public function pause(): self {return $this->control("pause");}
    public function resume(): self {return $this->control("resume");}
    public function cancel(): self {return $this->control("cancel");}
    public function onMarker(string $name,callable $fn):self{$this->markers[$name]=$fn;return $this;}
    public function speed(float $speed):self{Gameplay::send(["command"=>"speed","handle"=>$this->id,"speed"=>$speed]);return $this;}
    public function loop(bool $looping=true):self{Gameplay::send(["command"=>"loop","handle"=>$this->id,"looping"=>$looping]);return $this;}
    public function stop():self{return $this->cancel();}
    public function reverse(): self {return $this->control("reverse");}
}
final class Gameplay {
    public static array $handles=[],$callbacks=[],$once=[],$connections=[];
    public static function callback(callable $fn,bool $once=false):string{$id=bin2hex(random_bytes(16));self::$callbacks[$id]=$fn;self::$once[$id]=$once;return $id;}
    public static function release(GameplayHandle $h):void{foreach($h->tokens as $t){unset(self::$callbacks[$t],self::$once[$t]);}unset(self::$handles[$h->id]);}
    private static function collect(array $a,array &$tokens):void{foreach(["token","marker_token"] as $key)if(isset($a[$key]))$tokens[]=$a[$key];foreach($a["actions"]??[] as $child)self::collect($child,$tokens);}
    public static function send(array $request): void {global $instance;$instance->commands[]=["op"=>"gameplay","request"=>$request];}
    public static function entity(?string $id): string {global $rustic,$Game;return $id===null?$rustic->entity_id():(str_starts_with($id,"game.scene.")?$rustic->query(["op"=>"scene_object","source"=>$id]):($Game->scene->Find($id)??$id));}
    public static function start(array $action,bool $loop=false,int $repeats=0,bool $pingPong=false): GameplayHandle {
        $id=bin2hex(random_bytes(16));$h=new GameplayHandle($id);self::$handles[$id]=$h;self::collect($action,$h->tokens);
        self::send(["command"=>"start","handle"=>$id,"action"=>$action,"on_finished"=>$id,"playback"=>["looping"=>$loop,"repeats"=>$repeats,"ping_pong"=>$pingPong]]);return $h;
    }
    public static function dispatch(): void {
        global $rustic;$live=array_flip($rustic->state["gameplay_connections"]??[]);foreach(self::$connections as $id=>$_)if(!isset($live[$id]))unset(self::$callbacks[$id],self::$once[$id],self::$connections[$id]);$existing=self::$handles;
        foreach($rustic->state["gameplay_callbacks"]??[] as $d){
            $id=$d["token"];$h=self::$handles[$id]??null;
            if($h){self::release($h);if($h->finished)($h->finished)();}elseif(isset(self::$callbacks[$id])){$fn=self::$callbacks[$id];if(self::$once[$id])unset(self::$callbacks[$id],self::$once[$id]);$fn(...$d["arguments"]);}
        }
        $states=[];foreach($rustic->state["gameplay_states"]??[] as $s)$states[$s["handle"]]=$s;
        foreach($existing as $id=>$h){$s=$states[$id]??null;if(!$s||in_array($s["status"],["failed","cancelled"],true)){self::release($h);if($s&&$s["error"])$rustic->log("error",$s["error"]);}}
    }
}
final class Tween {
    static function value(mixed $from,mixed $to,float $duration,callable $sample,string $easing="Linear"):GameplayHandle{return Gameplay::start(["kind"=>"value","from"=>$from,"to"=>$to,"duration"=>$duration,"easing"=>$easing,"token"=>Gameplay::callback($sample)]);}
    public static function action(?string $entity,string $property,mixed $to,float|array $duration,string $easing="Linear"): array {
        return ["kind"=>"tween","target"=>["entity"=>Gameplay::entity($entity),"property"=>$property],"to"=>$to,"easing"=>is_array($duration)?($duration["easing"]??$easing):$easing]+(is_array($duration)?array_intersect_key($duration,array_flip(["duration","speed"])):["duration"=>$duration]);
    }
    public static function to(?string $entity,string $property,mixed $to,float|array $duration,string $easing="Linear"): GameplayHandle {return Gameplay::start(self::action($entity,$property,$to,$duration,$easing));}
    public static function move(?string $entity,array $position,float|array $duration,string $easing="Linear"): GameplayHandle {return self::to($entity,"Position",$position,$duration,$easing);}
    public static function rotate(?string $entity,array $rotation,float|array $duration,string $easing="Linear"): GameplayHandle {return self::to($entity,"Rotation",$rotation,$duration,$easing);}
    public static function scale(?string $entity,array $scale,float|array $duration,string $easing="Linear"): GameplayHandle {return self::to($entity,"Scale",$scale,$duration,$easing);}
}
final class Movement {
    static function move(?string $e,array $offset,float $duration,string $easing="Linear"):GameplayHandle{return Gameplay::start(["kind"=>"move","entity"=>Gameplay::entity($e),"offset"=>$offset,"duration"=>$duration,"easing"=>$easing]);}
    static function lookAt(?string $e,array $position,float $duration=0,string $easing="Linear"):GameplayHandle{return Gameplay::start(["kind"=>"look_at","entity"=>Gameplay::entity($e),"position"=>$position,"duration"=>$duration,"easing"=>$easing]);}
    static function follow(?string $e,string $target,float $duration,string $easing="Linear",array $offset=[0,0,0]):GameplayHandle{return Gameplay::start(["kind"=>"follow","entity"=>Gameplay::entity($e),"target"=>Gameplay::entity($target),"offset"=>$offset,"duration"=>$duration,"easing"=>$easing]);}
    static function orbit(?string $e,array $center,float $radius,float $turns,float $duration,string $easing="Linear"):GameplayHandle{return Gameplay::start(["kind"=>"orbit","entity"=>Gameplay::entity($e),"center"=>$center,"radius"=>$radius,"turns"=>$turns,"duration"=>$duration,"easing"=>$easing]);}
    public static function moveTo(?string $entity,array $position,float|array $duration,string $easing="Linear"): GameplayHandle {return Tween::move($entity,$position,$duration,$easing);}
    public static function rotateTo(?string $entity,array $rotation,float|array $duration,string $easing="Linear"): GameplayHandle {return Tween::rotate($entity,$rotation,$duration,$easing);}
}
final class Sequence {
    private array $actions=[];
    public static function create(): self {return new self();}
    public function move(?string $e,array $p,float $d,string $ease="Linear"): self {$this->actions[]=Tween::action($e,"Position",$p,$d,$ease);return $this;}
    public function to(?string $e,string $p,mixed $value,float $d,string $ease="Linear"):self{$this->actions[]=Tween::action($e,$p,$value,$d,$ease);return $this;}
    public function call(callable $fn):self{$this->actions[]=["kind"=>"callback","token"=>Gameplay::callback($fn,true)];return $this;}
    public function animation(?string $e,string $name,array $opts=[]):self{$this->actions[]=Animation::action($e,$name,$opts);return $this;}
    public function parallel(array $branches):self{$this->actions[]=["kind"=>"parallel","actions"=>array_map(fn($b)=>["kind"=>"sequence","actions"=>$b->actions],$branches)];return $this;}
    public function wait(float $duration): self {$this->actions[]=["kind"=>"wait","duration"=>$duration];return $this;}
    public function play(): GameplayHandle {return Gameplay::start(["kind"=>"sequence","actions"=>$this->actions]);}
}

final class Ease {
    public const Linear="Linear";
    public const InSine="InSine";
    public const OutSine="OutSine";
    public const InOutSine="InOutSine";
    public const InQuad="InQuad";
    public const OutQuad="OutQuad";
    public const InOutQuad="InOutQuad";
    public const InCubic="InCubic";
    public const OutCubic="OutCubic";
    public const InOutCubic="InOutCubic";
    public const InQuart="InQuart";
    public const OutQuart="OutQuart";
    public const InOutQuart="InOutQuart";
    public const InQuint="InQuint";
    public const OutQuint="OutQuint";
    public const InOutQuint="InOutQuint";
    public const InExpo="InExpo";
    public const OutExpo="OutExpo";
    public const InOutExpo="InOutExpo";
    public const InCirc="InCirc";
    public const OutCirc="OutCirc";
    public const InOutCirc="InOutCirc";
    public const InBack="InBack";
    public const OutBack="OutBack";
    public const InOutBack="InOutBack";
    public const InElastic="InElastic";
    public const OutElastic="OutElastic";
    public const InOutElastic="InOutElastic";
    public const InBounce="InBounce";
    public const OutBounce="OutBounce";
    public const InOutBounce="InOutBounce";
}

final class Timer {
    static function every(float $interval,callable $fn,?int $count=null):GameplayHandle{if($interval<=0||($count!==null&&$count<1))throw new InvalidArgumentException("positive interval/count required");return Gameplay::start(["kind"=>"sequence","actions"=>[["kind"=>"wait","duration"=>$interval],["kind"=>"callback","token"=>Gameplay::callback($fn)]]],$count===null,$count===null?0:$count-1);}
    public static function after(float $delay,Closure $fn): GameplayHandle {return Gameplay::start(["kind"=>"wait","duration"=>$delay])->onFinished($fn);}
}

final class Smooth{
    static function lerp(mixed $a,mixed $b,float $t,string $easing="Linear"):mixed{global $rustic;return $rustic->query(["op"=>"lerp","from"=>$a,"to"=>$b,"progress"=>$t,"easing"=>$easing]);}
    static function slerp(array $a,array $b,float $t,string $easing="Linear"):array{global $rustic;return $rustic->query(["op"=>"slerp","from"=>$a,"to"=>$b,"progress"=>$t,"easing"=>$easing]);}
    static function inverseLerp(float $a,float $b,float $v):float{global $rustic;return $rustic->query(["op"=>"inverse_lerp","from"=>$a,"to"=>$b,"value"=>$v]);}
    static function remap(float $v,float $a,float $b,float $c,float $d):float{global $rustic;return $rustic->query(["op"=>"remap","value"=>$v,"in_min"=>$a,"in_max"=>$b,"out_min"=>$c,"out_max"=>$d]);}
    static function smoothDamp(float $c,float $t,float $v,float $s,float $dt):array{global $rustic;return $rustic->query(["op"=>"smooth_damp","current"=>$c,"target"=>$t,"velocity"=>$v,"smooth_time"=>$s,"delta"=>$dt]);}
}
final class Physics{
    static function raycast(array $origin,array $direction,float $distance):?array{global $rustic;return $rustic->query(["op"=>"physics_raycast","origin"=>$origin,"direction"=>$direction,"distance"=>$distance]);}
    static function sphereCast(array $origin,array $direction,float $distance,float $radius):?array{global $rustic;return $rustic->query(["op"=>"physics_sphere_cast","origin"=>$origin,"direction"=>$direction,"distance"=>$distance,"radius"=>$radius]);}
    static function overlap(array $center,float $radius):array{global $rustic;return $rustic->query(["op"=>"physics_overlap","center"=>$center,"radius"=>$radius]);}
    static function impulse(?string $e,array $v):void{global $rustic;$rustic->query(["op"=>"physics_impulse","entity"=>Gameplay::entity($e),"vector"=>$v]);}
    static function launch(?string $e,array $v):void{global $rustic;$rustic->query(["op"=>"physics_launch","entity"=>Gameplay::entity($e),"vector"=>$v]);}
    static function force(?string $e,array $v,float $dt):void{global $rustic;$rustic->query(["op"=>"physics_force","entity"=>Gameplay::entity($e),"vector"=>$v,"delta"=>$dt]);}
    static function knockback(?string $e,array $v):void{self::impulse($e,$v);}
    static function explosion(array $center,float $radius,float $strength):array{global $rustic;return $rustic->query(["op"=>"physics_explosion","center"=>$center,"radius"=>$radius,"strength"=>$strength]);}
}

final class Path{
    static function create(array $points,string $kind="Linear"):array{return ["kind"=>$kind,"points"=>array_map(fn($p)=>isset($p["point"])?$p:["point"=>$p,"easing"=>"Linear"],$points)];}
    static function follow(?string $e,array $path,array $opts):GameplayHandle{return Gameplay::start(["kind"=>"path","entity"=>Gameplay::entity($e),"path"=>$path,"easing"=>$opts["easing"]??"Linear","orient_to_path"=>$opts["orientToPath"]??false]+array_intersect_key($opts,array_flip(["duration","speed"])),$opts["loop"]??false,0,$opts["pingPong"]??false);}
}
final class Effects{
    static function fade(?string $e,float $opacity,float $d,string $ease="Linear"):GameplayHandle{return Tween::to($e,"Opacity",$opacity,$d,$ease);}
    static function flash(?string $e,array $color,float $d,string $ease="Linear"):GameplayHandle{return Gameplay::start(Tween::action($e,"Color",$color,$d,$ease),false,1,true);}
    static function pulse(?string $e,array $scale,float $d,string $ease="Linear"):GameplayHandle{return Gameplay::start(Tween::action($e,"Scale",$scale,$d,$ease),false,1,true);}
    static function shake(?string $e,float $strength,float $duration,string $easing="Linear"):GameplayHandle{return Gameplay::start(["kind"=>"shake","entity"=>Gameplay::entity($e),"strength"=>$strength,"duration"=>$duration,"easing"=>$easing]);}
}
final class Clock{static function timeScale(?float $scale=null):float{global $rustic;return $rustic->query($scale===null?["op"=>"clock"]:["op"=>"clock","scale"=>$scale])["scale"];}static function pause():void{global $rustic;$rustic->query(["op"=>"clock","paused"=>true]);}static function resume():void{global $rustic;$rustic->query(["op"=>"clock","paused"=>false]);}}
final class Camera{
    static function current():string{global $rustic;return $rustic->query(["op"=>"camera_current"]);}
    static function moveTo(?string $e,array $p,float $d,string $ease="Linear"):GameplayHandle{return Tween::move($e,$p,$d,$ease);}
    static function zoom(?string $e,float $f,float $d,string $ease="Linear"):GameplayHandle{return Tween::to($e,"Fov",$f,$d,$ease);}
    static function fov(?string $e,float $f,float $d,string $ease="Linear"):GameplayHandle{return self::zoom($e,$f,$d,$ease);}
    static function lookAt(?string $e,array $p,float $d,string $ease="Linear"):GameplayHandle{return Movement::lookAt($e,$p,$d,$ease);}
    static function follow(?string $e,string $target,float $d,string $ease="Linear"):GameplayHandle{return Movement::follow($e,$target,$d,$ease);}
    static function orbit(?string $e,array $c,float $r,float $turns,float $d,string $ease="Linear"):GameplayHandle{return Movement::orbit($e,$c,$r,$turns,$d,$ease);}
    static function shake(?string $e,float $s,float $d,string $ease="Linear"):GameplayHandle{return Effects::shake($e,$s,$d,$ease);}
    static function transition(?string $e,array $p,array $r,float $f,float $d,string $ease="Linear"):GameplayHandle{return Gameplay::start(["kind"=>"parallel","actions"=>[Tween::action($e,"Position",$p,$d,$ease),Tween::action($e,"Rotation",$r,$d,$ease),Tween::action($e,"Fov",$f,$d,$ease)]]);}
}
final class Animation{
    static function addMarker(?string $e,string $clip,float $time,string $name):void{global $rustic;$rustic->query(["op"=>"animation_marker","entity"=>Gameplay::entity($e),"clip"=>$clip,"time"=>$time,"name"=>$name]);}
    static function value(array $keys,callable $sample):GameplayHandle{global $rustic;return Gameplay::start($rustic->query(["op"=>"keyframes","keys"=>$keys,"token"=>Gameplay::callback($sample,false)]));}
    static function load(?string $e,string $source):array{global $rustic;return $rustic->query(["op"=>"animation_load","entity"=>Gameplay::entity($e),"source"=>$source]);}
    static function register(?string $e,array $clip):void{global $rustic;$rustic->query(["op"=>"animation_register","entity"=>Gameplay::entity($e),"clip"=>$clip]);}
    static function clips(?string $e):array{global $rustic;return $rustic->query(["op"=>"animation_list","entity"=>Gameplay::entity($e)]);}
    static function action(?string $e,string|array $name,array $opts=[]):array{global $rustic;return ["kind"=>"animation_ref","entity"=>Gameplay::entity($e),"clip"=>is_array($name)?$name:$rustic->query(["op"=>"animation_ref","entity"=>Gameplay::entity($e),"name"=>$name]),"options"=>["speed"=>$opts["speed"]??1,"blend_in"=>$opts["blendIn"]??0,"blend_out"=>$opts["blendOut"]??0,"blend_in_ease"=>$opts["blendInEase"]??$opts["easing"]??"Linear","blend_out_ease"=>$opts["blendOutEase"]??$opts["easing"]??"Linear","progression_ease"=>$opts["progressionEase"]??null,"weight"=>$opts["weight"]??1,"additive"=>$opts["additive"]??false,"layered"=>$opts["layered"]??!empty($opts["mask"])],"mask"=>$opts["mask"]??[],"blend_source"=>$opts["sourceClip"]??null];}
    static function play(?string $e,string|array $name,array $opts=[]):GameplayHandle{$h=null;$a=self::action($e,$name,$opts);$a["marker_token"]=Gameplay::callback(function($name)use(&$h){if(isset($h->markers[$name]))($h->markers[$name])();});$h=Gameplay::start($a,$opts["loop"]??false);return $h;}
    static function blend(?string $e,string $from,string $to,array $opts=[]):GameplayHandle{global $rustic;$opts["sourceClip"]=$rustic->query(["op"=>"animation_ref","entity"=>Gameplay::entity($e),"name"=>$from]);$opts["blendIn"]=$opts["duration"]??0.3;return self::play($e,$to,$opts);}
    static function transition(?string $e,string $from,string $to,array $opts=[]):GameplayHandle{return self::blend($e,$from,$to,$opts);}
    static function stop(GameplayHandle $h):void{$h->cancel();}static function pause(GameplayHandle $h):void{$h->pause();}static function speed(GameplayHandle $h,float $v):void{$h->speed($v);}static function loop(GameplayHandle $h,bool $v=true):void{$h->loop($v);}
    static function ik(string $root,string $middle,string $tip,array $target,array $opts=[]):GameplayHandle{global $rustic;return Gameplay::start($rustic->query(["op"=>"animation_ik","root"=>Gameplay::entity($root),"middle"=>Gameplay::entity($middle),"tip"=>Gameplay::entity($tip),"target"=>$target,"duration"=>$opts["duration"]??0,"easing"=>$opts["easing"]??"Linear","weight"=>$opts["weight"]??1,"pole"=>$opts["pole"]??[0,0,1]]));}
    static function footPlacement(string $root,string $middle,string $tip,array $target,array $opts=[]):GameplayHandle{return self::ik($root,$middle,$tip,$target,$opts);}
    static function lookAt(string $e,array $target,float $duration=0,string $easing="Linear"):GameplayHandle{return Movement::lookAt($e,$target,$duration,$easing);}
    static function headTracking(string $e,array $target,float $duration=0,string $easing="Linear"):GameplayHandle{return Movement::lookAt($e,$target,$duration,$easing);}
    static function recoil(string $e,array $rotation,float|array $duration,string $easing="Linear"):GameplayHandle{return Gameplay::start(Tween::action($e,"Rotation",$rotation,$duration,$easing),false,1,true);}
}
final class AudioVoice{
    function __construct(public string $id){}function stop():void{global $rustic;$rustic->query(["op"=>"audio_stop","entity"=>$this->id]);}function pause():void{global $rustic;$rustic->query(["op"=>"audio_pause","entity"=>$this->id]);}function resume():void{global $rustic;$rustic->query(["op"=>"audio_resume","entity"=>$this->id]);}
}
final class Audio{
    static function get_output_devices():array{global $rustic;return $rustic->query(["op"=>"audio_get_output_devices"]);}
    static function get_input_devices():array{global $rustic;return $rustic->query(["op"=>"audio_get_input_devices"]);}
    static function set_output_device(?string $device=null):void{global $rustic;$rustic->query(["op"=>"audio_set_output_device","device"=>$device]);}
    static function set_input_device(?string $device=null):void{global $rustic;$rustic->query(["op"=>"audio_set_input_device","device"=>$device]);}
    static function play(string $source,array $opts=[]):AudioVoice{global $rustic;return new AudioVoice($rustic->query(["op"=>"audio_play","source"=>$source]+$opts));}
    static function playAt(string $source,array $position,array $opts=[]):AudioVoice{return self::play($source,["position"=>$position]+$opts);}
    static function volume(AudioVoice $v,float $value):void{global $rustic;$rustic->query(["op"=>"audio_volume","entity"=>$v->id,"value"=>$value]);}static function pitch(AudioVoice $v,float $value):void{global $rustic;$rustic->query(["op"=>"audio_pitch","entity"=>$v->id,"value"=>$value]);}
    static function fadeIn(AudioVoice $v,float $duration,string $easing="Linear"):GameplayHandle{return Gameplay::start(Tween::action($v->id,"Volume",1,$duration,$easing)+["from"=>0]);}
    static function fadeOut(AudioVoice $v,float $duration,string $easing="Linear"):GameplayHandle{return Tween::to($v->id,"Volume",0,$duration,$easing);}
    static function crossfade(AudioVoice $a,AudioVoice $b,float $duration,string $easing="Linear"):GameplayHandle{return Gameplay::start(["kind"=>"parallel","actions"=>[Tween::action($a->id,"Volume",0,$duration,$easing),Tween::action($b->id,"Volume",1,$duration,$easing)+["from"=>0]]]);}
}
final class Events{
    private static function subscribe(string $name,callable $fn,bool $once=false,?string $source=null):EventConnection{$token=Gameplay::callback($fn,$once);Gameplay::$connections[$token]=true;Gameplay::send(["command"=>"connect","token"=>$token,"signal"=>["name"=>$name,"source"=>$source],"once"=>$once]);return new EventConnection($token);}
    static function on(string $name,callable $fn):EventConnection{return self::subscribe($name,$fn);}static function once(string $name,callable $fn):EventConnection{return self::subscribe($name,$fn,true);}
    static function connect(string $e,string $name,callable $fn):EventConnection{return self::subscribe($name,$fn,false,Gameplay::entity($e));}
    static function emit(string $name,array $arguments=[],?string $source=null):void{Gameplay::send(["command"=>"emit","signal"=>["name"=>$name,"source"=>$source===null?null:Gameplay::entity($source)],"arguments"=>$arguments]);}
    static function disconnect(EventConnection $c):void{$c->disconnect();}
}
final class EventConnection{function __construct(public string $token){}function disconnect():void{unset(Gameplay::$callbacks[$this->token],Gameplay::$once[$this->token],Gameplay::$connections[$this->token]);Gameplay::send(["command"=>"disconnect","token"=>$this->token]);}}

class_alias(Sequence::class,"Timeline");
class_alias(Smooth::class,"Interpolation");

// Entity services use the shared Rust runtime.
final class Tags {
    static function add(?string $entity,string $name):bool {global $rustic;return $rustic->query(["op"=>"tags_add","entity"=>Gameplay::entity($entity),"name"=>$name]);}
    static function remove(?string $entity,string $name):bool {global $rustic;return $rustic->query(["op"=>"tags_remove","entity"=>Gameplay::entity($entity),"name"=>$name]);}
    static function find_all(string $name):array {global $rustic;return $rustic->query(["op"=>"tags_find_all","name"=>$name]);}
}
final class Groups {
    static function create(string $name):bool {global $rustic;return $rustic->query(["op"=>"groups_create","name"=>$name]);}
    static function add(string $name,?string $entity):bool {global $rustic;return $rustic->query(["op"=>"groups_add","name"=>$name,"entity"=>Gameplay::entity($entity)]);}
    static function get_members(string $name):array {global $rustic;return $rustic->query(["op"=>"groups_get_members","name"=>$name]);}
}
final class Hierarchy {
    static function set_parent(?string $entity,?string $parent,bool $keep_world=true):void {global $rustic;$rustic->query(["op"=>"hierarchy_set_parent","entity"=>Gameplay::entity($entity),"parent"=>($parent===null?null:Gameplay::entity($parent)),"keep_world"=>$keep_world]);}
    static function get_children(?string $entity):array {global $rustic;return $rustic->query(["op"=>"hierarchy_get_children","entity"=>Gameplay::entity($entity)]);}
    static function detach(?string $entity,bool $keep_world=true):void {global $rustic;$rustic->query(["op"=>"hierarchy_detach","entity"=>Gameplay::entity($entity),"keep_world"=>$keep_world]);}
}
final class Entity {
    static function destroy(?string $entity):void {global $rustic;$rustic->query(["op"=>"entity_destroy","entity"=>Gameplay::entity($entity)]);}
    static function destroy_after(?string $entity,float $delay):void {global $rustic;$rustic->query(["op"=>"entity_destroy_after","entity"=>Gameplay::entity($entity),"delay"=>$delay]);}
    static function is_valid(?string $entity):bool {global $rustic;return $rustic->query(["op"=>"entity_is_valid","entity"=>Gameplay::entity($entity)]);}
}
final class Components {
    static function has(?string $entity,string $name):bool {global $rustic;return $rustic->query(["op"=>"components_has","entity"=>Gameplay::entity($entity),"name"=>$name]);}
    static function get_all(?string $entity):array {global $rustic;return $rustic->query(["op"=>"components_get_all","entity"=>Gameplay::entity($entity)]);}
    static function find_in_children(?string $entity,string $name):array {global $rustic;return $rustic->query(["op"=>"components_find_in_children","entity"=>Gameplay::entity($entity),"name"=>$name]);}
}
final class World {
    static function find_by_tag(string $name):array {global $rustic;return $rustic->query(["op"=>"world_find_by_tag","name"=>$name]);}
    static function find_in_radius(array $center,float $radius):array {global $rustic;return $rustic->query(["op"=>"world_find_in_radius","center"=>$center,"radius"=>$radius]);}
}

// Canonical API paths.
final class ApiPath {
    private array $children=[];
    public function __construct(public string $source,private string $category,public ?string $filePath=null){}
    public function __get(string $name): ApiPath { return $this->children[$name]??=new ApiPath($this->source.'.'.$name,$this->category,$this->category==='file'?($this->filePath? $this->filePath.'/':'').$name:null); }
    public function __call(string $name,array $args):mixed {
        global $Game,$instance;
        if($name==='EditAttribute'||$name==='edit_attribute'){$instance->commands[]=['op'=>'edit_attribute','source'=>$this->source,'name'=>$args[0],'value'=>$args[1]];return null;}
        $methods=$this->category==='scene'?["tweenTo"=>["Tween","to",0],"tweenMove"=>["Tween","move",0],"tweenRotate"=>["Tween","rotate",0],"tweenScale"=>["Tween","scale",0],"tweenAction"=>["Tween","action",0],"tweenToOptions"=>["Tween","toOptions",0],"movementMoveTo"=>["Movement","moveTo",0],"movementMove"=>["Movement","move",0],"movementRotateTo"=>["Movement","rotateTo",0],"movementLookAt"=>["Movement","lookAt",0],"movementFollow"=>["Movement","follow",0],"movementOrbit"=>["Movement","orbit",0],"movementMoveToOptions"=>["Movement","moveToOptions",0],"pathFollow"=>["Path","follow",0],"effectsFade"=>["Effects","fade",0],"effectsFlash"=>["Effects","flash",0],"effectsShake"=>["Effects","shake",0],"effectsPulse"=>["Effects","pulse",0],"cameraMoveTo"=>["Camera","moveTo",0],"cameraZoom"=>["Camera","zoom",0],"cameraFov"=>["Camera","fov",0],"cameraFollow"=>["Camera","follow",0],"cameraOrbit"=>["Camera","orbit",0],"cameraLookAt"=>["Camera","lookAt",0],"cameraShake"=>["Camera","shake",0],"cameraTransition"=>["Camera","transition",0],"physicsImpulse"=>["Physics","impulse",0],"physicsForce"=>["Physics","force",0],"physicsLaunch"=>["Physics","launch",0],"physicsKnockback"=>["Physics","knockback",0],"animationRegister"=>["Animation","register",0],"animationRegisterClip"=>["Animation","registerClip",0],"animationClips"=>["Animation","clips",0],"animationAction"=>["Animation","action",0],"animationPlay"=>["Animation","play",0],"animationBlend"=>["Animation","blend",0],"animationTransition"=>["Animation","transition",0],"animationIk"=>["Animation","ik",0],"animationFootPlacement"=>["Animation","footPlacement",0],"animationLookAt"=>["Animation","lookAt",0],"animationHeadTracking"=>["Animation","headTracking",0],"animationRecoil"=>["Animation","recoil",0],"animationAddMarker"=>["Animation","addMarker",0],"eventsConnect"=>["Events","connect",0],"tagsAdd"=>["Tags","add",0],"tagsRemove"=>["Tags","remove",0],"groupsAdd"=>["Groups","add",1],"hierarchySetParent"=>["Hierarchy","set_parent",0],"hierarchyGetChildren"=>["Hierarchy","get_children",0],"hierarchyDetach"=>["Hierarchy","detach",0],"entityDestroy"=>["Entity","destroy",0],"entityDestroyAfter"=>["Entity","destroy_after",0],"entityIsValid"=>["Entity","is_valid",0],"componentsHas"=>["Components","has",0],"componentsGetAll"=>["Components","get_all",0],"componentsFindInChildren"=>["Components","find_in_children",0],"tween"=>["Tween","to",0],"moveTo"=>["Movement","moveTo",0],"move"=>["Movement","move",0],"rotateTo"=>["Movement","rotateTo",0],"lookAt"=>["Movement","lookAt",0],"follow"=>["Movement","follow",0],"orbit"=>["Movement","orbit",0],"fade"=>["Effects","fade",0],"flash"=>["Effects","flash",0],"shake"=>["Effects","shake",0],"pulse"=>["Effects","pulse",0],"impulse"=>["Physics","impulse",0],"force"=>["Physics","force",0],"launch"=>["Physics","launch",0],"knockback"=>["Physics","knockback",0],"playAnimation"=>["Animation","play",0],"destroy"=>["Entity","destroy",0],"destroyAfter"=>["Entity","destroy_after",0],"isValid"=>["Entity","is_valid",0],"addTag"=>["Tags","add",0],"removeTag"=>["Tags","remove",0],"setParent"=>["Hierarchy","set_parent",0],"getChildren"=>["Hierarchy","get_children",0],"detach"=>["Hierarchy","detach",0],"hasComponent"=>["Components","has",0],"getComponents"=>["Components","get_all",0],"findInChildren"=>["Components","find_in_children",0],"followPath"=>["Path","follow",0]]:["audioPlay"=>["Audio","play",0],"audioPlayAt"=>["Audio","playAt",0],"audioLoad"=>["Audio","load",0],"animationLoad"=>["Animation","load",1],"play"=>["Audio","play",0],"playAt"=>["Audio","playAt",0],"load"=>["Audio","load",0],"loadAnimation"=>["Animation","load",1],"add"=>["instance","add",0],"clone"=>["instance","clone",0],"setSkyTexture"=>["scene","setSkyTexture",0]];
        if($this->category==='scene')$methods+=['clone'=>['instance','clone',0],'add'=>['instance','add',0],'setCurrentCamera'=>['Game','setCurrentCamera',0],'emit'=>['Events','emit',2]];
        if(!isset($methods[$name]))throw new BadMethodCallException('Unknown path API: '.$name);
        [$family,$method,$index]=$methods[$name];
        $args=array_map(fn($a)=>$a instanceof ApiPath?($a->filePath??$a->source):$a,$args);
        $owner=$family==='scene'?$Game->scene:($family==='Game'?$Game:($family==='instance'?$instance:$family));
        $parameters=(new ReflectionMethod($owner,$method))->getParameters();
        $targetName=$parameters[$index]->getName();
        array_splice($parameters,$index,1);
        $bound=[];
        foreach($args as $key=>$value){
            $name=is_int($key)?($parameters[$key]??throw new InvalidArgumentException('Too many path API arguments'))->getName():$key;
            if($name===$targetName)throw new InvalidArgumentException('The path supplies the target argument');
            $bound[$name]=$value;
        }
        $bound[$targetName]=$this->filePath??$this->source;
        $args=$bound;
        if($family==='Game')return $Game->$method(...$args);
        if($family==='scene')return $Game->scene->$method(...$args);
        if($family==='instance')return $instance->$method(...$args);
        return $family::$method(...$args);
    }
}
final class CanonicalGlobalApi {
    public ApiPath $files;
    public function __construct(){$this->files=new ApiPath('game.rustic.files','file','');}
    public function __call(string $name,array $args):mixed {
        global $rustic,$Game;
        $name=['get_output_devices'=>'getOutputDevices','get_input_devices'=>'getInputDevices','set_output_device'=>'setOutputDevice','set_input_device'=>'setInputDevice'][$name]??$name;
        $methods=["tweenValue"=>["Tween","value",0],"timerAfter"=>["Timer","after",0],"timerEvery"=>["Timer","every",0],"smoothLerp"=>["Smooth","lerp",0],"smoothSlerp"=>["Smooth","slerp",0],"smoothInverseLerp"=>["Smooth","inverseLerp",0],"smoothRemap"=>["Smooth","remap",0],"smoothSmoothDamp"=>["Smooth","smoothDamp",0],"physicsRaycast"=>["Physics","raycast",0],"physicsSphereCast"=>["Physics","sphereCast",0],"physicsOverlap"=>["Physics","overlap",0],"physicsExplosion"=>["Physics","explosion",0],"clockTimeScale"=>["Clock","timeScale",0],"clockPause"=>["Clock","pause",0],"clockResume"=>["Clock","resume",0],"clockState"=>["Clock","state",0],"pathCreate"=>["Path","create",0],"sequenceNew"=>["Sequence","new",0],"sequenceCreate"=>["Sequence","create",0],"timelineNew"=>["Timeline","new",0],"timelineCreate"=>["Timeline","create",0],"eventsOn"=>["Events","on",0],"eventsOnce"=>["Events","once",0],"eventsEmit"=>["Events","emit",0],"eventsDisconnect"=>["Events","disconnect",0],"audioGetOutputDevices"=>["Audio","get_output_devices",0],"audioGetInputDevices"=>["Audio","get_input_devices",0],"audioSetOutputDevice"=>["Audio","set_output_device",0],"audioSetInputDevice"=>["Audio","set_input_device",0],"audioVolume"=>["Audio","volume",0],"audioPitch"=>["Audio","pitch",0],"audioFadeIn"=>["Audio","fadeIn",0],"audioFadeOut"=>["Audio","fadeOut",0],"audioCrossfade"=>["Audio","crossfade",0],"audioStop"=>["Audio","stop",0],"audioPause"=>["Audio","pause",0],"audioResume"=>["Audio","resume",0],"animationValue"=>["Animation","value",0],"animationStop"=>["Animation","stop",0],"animationPause"=>["Animation","pause",0],"animationSpeed"=>["Animation","speed",0],"animationLoop"=>["Animation","loop",0],"cameraCurrent"=>["Camera","current",0],"tagsFindAll"=>["Tags","find_all",0],"groupsCreate"=>["Groups","create",0],"groupsGetMembers"=>["Groups","get_members",0],"worldFindByTag"=>["World","find_by_tag",0],"worldFindInRadius"=>["World","find_in_radius",0],"after"=>["Timer","after",0],"every"=>["Timer","every",0],"lerp"=>["Smooth","lerp",0],"slerp"=>["Smooth","slerp",0],"inverseLerp"=>["Smooth","inverseLerp",0],"remap"=>["Smooth","remap",0],"smoothDamp"=>["Smooth","smoothDamp",0],"raycast"=>["Physics","raycast",0],"sphereCast"=>["Physics","sphereCast",0],"overlap"=>["Physics","overlap",0],"explosion"=>["Physics","explosion",0],"timeScale"=>["Clock","timeScale",0],"pause"=>["Clock","pause",0],"resume"=>["Clock","resume",0],"createPath"=>["Path","create",0],"sequence"=>["Sequence","new",0],"timeline"=>["Timeline","new",0],"on"=>["Events","on",0],"once"=>["Events","once",0],"emit"=>["Events","emit",0],"disconnect"=>["Events","disconnect",0],"currentCamera"=>["Camera","current",0],"getOutputDevices"=>["Audio","get_output_devices",0],"getInputDevices"=>["Audio","get_input_devices",0],"setOutputDevice"=>["Audio","set_output_device",0],"setInputDevice"=>["Audio","set_input_device",0]];
        if(isset($methods[$name])){[$family,$method]=$methods[$name];if(($family==="Sequence"||$family==="Timeline")&&$method==="new")$method="create";return $family::$method(...$args);}
        if($name==='getEnvironment'||$name==='setEnvironment')return $Game->scene->$name(...$args);
        if($name==='setCurrentCamera')return $Game->setCurrentCamera($args[0] instanceof ApiPath?$args[0]->source:$args[0]);
        return $rustic->$name(...$args);
    }
}
final class CanonicalGame {
    public ApiPath $scene;
    public CanonicalGlobalApi $rustic;
    public function __construct(){$this->scene=new ApiPath('game.scene','scene');$this->rustic=new CanonicalGlobalApi();}
}
$game=new CanonicalGame();
