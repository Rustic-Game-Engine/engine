<?php
require_once 'rustic.php';
$audio=$rec=null;
function parity($callback,$dt){
    global $rustic,$Game,$audio,$rec;
    if($callback==='on_start'){
        $rustic->print("info");$rustic->warn("warn");
        $rustic->set_translation(3,2,1);
        $id=$rustic->entity_id();
        assert($rustic->find_entity($id)===$id);
        assert($rustic->get_object_attribute($id,'Position')[0]==3);
        if($Game->scene->List()!==[$id])throw new Exception('scene list');
        assert(Clock::state()['scale']==1);
        $audio=Audio::load('assets/tone.wav');
        assert($audio->load()===$audio);
        $audio->play(['loop'=>true]);$audio->pause();$audio->resume();$audio->stop();$audio->play(['loop'=>true]);
        $rec=$Game->startRecording('capture.wav');
    }elseif($callback==='update'){
        $audio->play();$audio->stop();$audio->unload();$audio->unload();
        $Game->stopRecording($rec);
        $packet=$rec->read();
        if($packet['sampleRate']!==48000 || $packet['channels']!==2 || $packet['active'] || $packet['samples']!==[0.25,-0.25,0.5,-0.5])throw new Exception('PCM');
        if($rec->read()['samples']!==[])throw new Exception('cursor');
        $rec->unload();
    }
}
rustic_run(["on_start"=>fn()=>parity("on_start",0),"update"=>fn($dt)=>parity("update",$dt)]);
