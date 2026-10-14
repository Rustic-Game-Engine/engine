#include "rustic.hpp"
#include <cassert>
std::optional<Audio::LoadedAudio> audio;
RusticCaptureHandle rec;
void start(){
    rustic.print("info");rustic.warn("warn");
    rustic.set_translation(3,2,1);
    auto id=rustic.entity_id();
    assert(rustic.find_entity(id)==id);
    assert(rustic.get_object_attribute(id,"Position").array()[0].number()==3);
    assert(Game.scene.List()==std::vector<std::string>{id});
    assert(Clock::state().at("scale").number()==1);
    audio=Audio::load("assets/tone.wav");
    assert(&audio->load()==&*audio);
    audio->play(1,1,true);audio->pause();audio->resume();audio->stop();audio->play(1,1,true);
    rec=Game.startRecording("capture.wav");
}
void update(double){
    audio->play();audio->stop();audio->unload();audio->unload();
    Game.stopRecording(&rec);
    auto packet=rec.read();
    assert(packet.at("sampleRate").number()==48000 && packet.at("channels").number()==2 && !packet.at("active").boolean());
    assert(packet.at("samples").array().size()==4 && packet.at("samples").array()[0].number()==0.25 && packet.at("samples").array()[3].number()==-0.5);
    assert(rec.read().at("samples").array().empty());
    rec.unload();
}
int main(){return rustic_run(RusticBehavior{.on_start=start,.update=update});}
