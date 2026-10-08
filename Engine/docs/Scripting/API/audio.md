# Audio API

Play engine-decoded WAV/OGG voices, adjust parameters and schedule fades.

## Setup and language support

These engine-owned services work in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript. Follow [Use gameplay actions](/docs/guides/gameplay-actions) to create and attach a script, install external toolchains, and choose the correct language binding. PHP and HTML scripts belong under `ui/`; CSS alone runs no gameplay code.

The signatures and complete example below use JavaScript. Attach a JavaScript Object Component script to the specified target and press **Play**. Globals are supplied by the engine. Schedule actions from `Start` or a callback, rather than at script top level. Native languages use typed options and their own builder conventions; see the guide's language bindings.

## Calls

- `Audio.play(source, options?)`
- `Audio.playAt(source, position, options?)`
- `voice.pause() / resume() / stop()`
- `Audio.volume(voice, value)`
- `Audio.pitch(voice, value)`
- `Audio.fadeIn(voice, duration, easing?, volume = 1)`
- `Audio.fadeOut(voice, duration, easing?)`
- `Audio.crossfade(oldVoice, newVoice, duration, easing?)`

## Parameters

Source is a project-relative WAV/OGG asset path. JavaScript options include loop, volume and pitch. Position is a world vector for distance attenuation. Volume is finite non-negative within f32 range; pitch is finite positive. Durations are scene-clock seconds.

## Return and timing

Play/playAt return voice objects with an ID and controls. Fades/crossfades return operation handles. Volume/pitch and voice controls update playback directly.

## Example

```javascript
globalThis.behavior = {
  Start() {
    const music = Audio.play("assets/music.ogg", {loop: true, volume: 0});
    Audio.fadeIn(music, 2, Ease.InSine);
    Timer.after(5, () => {
      Audio.fadeOut(music, 1, Ease.OutSine)
        .onFinished(() => music.stop());
    });
  }
};
```

## Behavior and limitations

- Provide assets/music.ogg before Play. The voice fades in over two seconds, starts fading out at five seconds and stops at six.
- Windows player output uses the default audio device. Other platforms currently mix PCM headlessly, so successful playback may produce no device sound.
- FadeOut reaches zero volume but does not stop the voice; stop explicitly or allow natural completion. Crossfade fades the old voice to zero and the new voice from zero to one.
- PlayAt attenuates distance relative to the active camera. The mixer renders 48 kHz stereo PCM with linear resampling for pitch.
- Clock scaling changes source playback rate; Clock.pause and voice.pause retain their source cursors. Natural completion releases non-looping PCM. Stopping cancels voice-targeting actions on the next scheduler tick.
- Voices are owner-bound, with at most 1024 retained voices.
- Check script attachment, enabled state and the Console when an operation fails. Asynchronous errors also appear in operation state. Callback exceptions disable the owner and clean up its actions.
- Owner destruction, disable, successful reload or Stop cleans up owned actions and subscriptions. Failed reload retains the previous behavior's operations. See [Operation handles](/docs/api/operation-handles) for controls and lifetime details.
