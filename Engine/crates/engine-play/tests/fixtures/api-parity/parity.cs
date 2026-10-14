using System;
using static Rustic;
class Program {
    static Audio.LoadedAudio audio=null!;
    static CaptureHandle rec=null!;
    static void Main()=>Rustic.Run((callback,dt)=>{
        if(callback=="on_start"){
            rustic.print("info");rustic.warn("warn");
    rustic.set_translation(3,2,1);
            string id=rustic.entity_id();
            if(rustic.find_entity(id)!=id || ((double[])rustic.get_object_attribute(id,"Position")!)[0]!=3)throw new Exception("live object read");
            if(System.Linq.Enumerable.Count(Game.scene.List())!=1)throw new Exception("scene list");
            if(Clock.state().GetProperty("scale").GetDouble()!=1)throw new Exception("clock");
            audio=Audio.load("assets/tone.wav");
            if(audio.load()!=audio)throw new Exception("load identity");
            audio.play(loop:true);audio.pause();audio.resume();audio.stop();audio.play(loop:true);
            rec=Game.startRecording("capture.wav");
        }else if(callback=="update"){
            audio.play();audio.stop();audio.unload();audio.unload();
            Game.stopRecording(rec);
            var packet=rec.read();
            var samples=packet.GetProperty("samples");
            if(packet.GetProperty("sampleRate").GetInt32()!=48000 || packet.GetProperty("channels").GetInt32()!=2 || packet.GetProperty("active").GetBoolean() || samples.GetArrayLength()!=4 || samples[0].GetDouble()!=0.25 || samples[3].GetDouble()!=-0.5)throw new Exception("PCM");
            if(rec.read().GetProperty("samples").GetArrayLength()!=0)throw new Exception("cursor");
            rec.unload();
        }
    });
}
