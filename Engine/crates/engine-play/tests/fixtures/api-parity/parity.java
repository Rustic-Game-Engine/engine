class RusticBehavior extends Rustic {
    static Audio.LoadedAudio audio;
    static CaptureHandle rec;
    public static void main(String[] args) throws Exception {
        run((callback,dt)->{
            if(callback.equals("on_start")){
                rustic.print("info");rustic.warn("warn");
    rustic.set_translation(3,2,1);
                String id=rustic.entity_id();
                if(!id.equals(rustic.find_entity(id)) || ((double[])rustic.get_object_attribute(id,"Position"))[0]!=3)throw new AssertionError("live object read");
                if(!Game.scene.List().equals(java.util.List.of(id)))throw new AssertionError("scene list");
                if(((Number)((java.util.Map<?,?>)Clock.state()).get("scale")).doubleValue()!=1)throw new AssertionError("clock");
                audio=Audio.load("assets/tone.wav");
                if(audio.load()!=audio)throw new AssertionError("load identity");
                audio.play(1,1,true);audio.pause();audio.resume();audio.stop();audio.play(1,1,true);
                rec=Game.startRecording("capture.wav");
            }else if(callback.equals("update")){
                audio.play();audio.stop();audio.unload();audio.unload();
                Game.stopRecording(rec);
                var packet=(java.util.Map<?,?>)rec.read();
                var samples=(java.util.List<?>)packet.get("samples");
                if(((Number)packet.get("sampleRate")).intValue()!=48000 || ((Number)packet.get("channels")).intValue()!=2 || (Boolean)packet.get("active") || samples.size()!=4 || ((Number)samples.get(0)).doubleValue()!=0.25 || ((Number)samples.get(3)).doubleValue()!=-0.5)throw new AssertionError("PCM");
                if(!((java.util.List<?>)((java.util.Map<?,?>)rec.read()).get("samples")).isEmpty())throw new AssertionError("cursor");
                rec.unload();
            }
        });
    }
}
