let audio,rec;
globalThis.behavior={Start(){
    rustic.print("info");rustic.warn("warn");
    rustic.set_translation(3,2,1);
    const id=rustic.entity_id();
    if(rustic.find_entity(id)!==id || rustic.get_object_attribute(id,'Position')[0]!==3)throw Error('live object read');
    if(Game.scene.List().length!==1 || Game.scene.List()[0]!==id)throw Error('scene list IDs');
    if(Clock.state().scale!==1)throw Error('clock');
    const asset=rustic.game.assets["tone.wav"];
    const loaded=asset.load();
    if(loaded.load()!==loaded)throw Error('proxy load identity');
    asset.play();asset.stop();asset.unload();
    audio=Audio.load('assets/tone.wav');
    if(audio.load()!==audio)throw Error('load identity');
    audio.play({loop:true});audio.pause();audio.resume();audio.stop();audio.play({loop:true});
    rec=Game.startRecording('capture.wav');
},Update(){
    audio.play();audio.stop();audio.unload();audio.unload();
    Game.stopRecording(rec);
    const packet=rec.read();
    if(packet.sampleRate!==48000 || packet.channels!==2 || packet.active || packet.samples.length!==4 || packet.samples[0]!==0.25 || packet.samples[3]!==-0.5)throw Error('PCM');
    if(rec.read().samples.length!==0)throw Error('cursor');
    rec.unload();
}};
