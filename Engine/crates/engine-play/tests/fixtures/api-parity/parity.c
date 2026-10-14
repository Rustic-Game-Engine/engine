#include "rustic.h"
#include <assert.h>
RusticLoadedAudio audio;
RusticCaptureHandle rec;
void start(void){
    rustic.print("info");rustic.warn("warn");
    rustic.set_translation(3,2,1);
    const char *id=rustic.entity_id();
    assert(strcmp(rustic.find_entity(id),id)==0);
    assert(rustic.get_object_attribute(id,"Position").vector[0]==3);
    RusticList ids=Game.scene.List(NULL);assert(ids.count==1 && strcmp(ids.items[0],id)==0);
    assert(Clock.state().scale==1);
    audio=Audio.load("assets/tone.wav");
    RusticAudioOptions options={.volume=1,.pitch=1,.loop=true};
    LoadedAudio.play(audio,&options);LoadedAudio.pause(audio);LoadedAudio.resume(audio);LoadedAudio.stop(audio);LoadedAudio.play(audio,&options);
    rec=Game.startRecording("capture.wav",NULL);
}
void update(double dt){
    (void)dt;
    LoadedAudio.play(audio,NULL);LoadedAudio.stop(audio);LoadedAudio.unload(&audio);LoadedAudio.unload(&audio);
    Game.stopRecording(&rec);
    RusticCaptureData packet=Capture.read(rec);
    assert(packet.sampleRate==48000 && packet.channels==2 && !packet.active);
    assert(packet.count==4 && packet.samples[0]==0.25 && packet.samples[3]==-0.5);
    assert(Capture.read(rec).count==0);
    assert(packet.samples[0]==0.25); /* Reads retain earlier packets during this callback. */
    Capture.unload(rec);
}
int main(void){return rustic_run((RusticBehavior){.on_start=start,.update=update});}
