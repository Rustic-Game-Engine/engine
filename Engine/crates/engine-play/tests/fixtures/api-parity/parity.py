from rustic import rustic, Audio, Game, Clock, run
audio=rec=None
def on_start():
    global audio,rec
    rustic.print("info");rustic.warn("warn")
    rustic.set_translation(3,2,1)
    id=rustic.entity_id()
    assert rustic.find_entity(id)==id
    assert rustic.get_object_attribute(id,'Position')[0]==3
    assert Game.scene.List()==[id]
    assert Clock.state()['scale']==1
    audio=Audio.load('assets/tone.wav')
    assert audio.load() is audio
    audio.play({'loop':True});audio.pause();audio.resume();audio.stop();audio.play({'loop':True})
    rec=Game.startRecording('capture.wav')
def update(dt):
    audio.play();audio.stop();audio.unload();audio.unload()
    Game.stopRecording(rec)
    packet=rec.read()
    assert packet['sampleRate']==48000 and packet['channels']==2 and not packet['active']
    assert packet['samples']==[0.25,-0.25,0.5,-0.5]
    assert rec.read()['samples']==[]
    rec.unload()
run(globals())
