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
    public static function entity(?string $id): string {global $rustic,$Game;return $id===null?$rustic->entity_id():($Game->scene->Find($id)??$id);}
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
