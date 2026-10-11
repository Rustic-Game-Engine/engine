# Audio API

Play engine-decoded WAV/OGG voices, adjust parameters and schedule fades.

## Setup and language support

These engine-owned services work in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript. Follow [Use gameplay actions](/docs/guides/gameplay-actions) to create and attach a script, install external toolchains, and choose the correct language binding. PHP and HTML scripts belong under `ui/`; CSS alone runs no gameplay code.

The signatures and complete example below use JavaScript. Attach a JavaScript Object Component script to the specified target and press **Play**. Globals are supplied by the engine. Schedule actions from `Start` or a callback, rather than at script top level. Native languages use typed options and their own builder conventions; see the guide's language bindings.

## Load and control audio objects (Lua and Luau)

Create `assets/sounds/` in your project and place `crash.wav` there (WAV and OGG
are supported). Create a Lua or Luau Object Component script, attach it to an
enabled scene object, and press **Play**. Use `Start` to load audio before playing:

```lua
local crashA
return {
  Start = function()
    crashA = rustic.game.assets.sounds["crash.wav"]:load()
    crashA:play()
    Timer.after(0.1, function() crashA:pause() end)
    Timer.after(0.2, function() crashA:resume() end)
    Timer.after(1, function() crashA:stop() end)
    Timer.after(2, function() crashA:play() end)
    Timer.after(3, function() crashA:Unload() end)
  end
}
```

You should hear the clip, pause briefly, then hear it again at two seconds if
running the Windows player. Other platforms currently mix without device sound.
A short clip may finish before the pause timer fires; controls on a loaded clip
with no active playback are harmless.

- Directory keys follow the project `assets/` directory. Brackets preserve file
  extensions, spaces, and dots: `rustic.game.assets["sound effects"]["crash.wav"]`.
  These are asset paths; audio scene components are not required.
- `:load()` decodes and retains the audio without playing, returning a reusable
  audio handle. `Audio.load("assets/sounds/crash.wav")` returns the same kind of
  handle. Calling `:load()` on an already loaded handle returns that handle.
- `:play(options?)` requires a successful load and starts from the beginning,
  replacing that handle's previous playback. Options are `loop`, `volume`,
  `pitch`, and optional world `position`. It returns a voice compatible with
  `Audio.volume`, `Audio.pitch`, and fades.
- `:stop()` stops playback but keeps the clip loaded. `:pause()` preserves the
  current playback position; `:resume()` continues it. Resume after stop or
  natural completion does not restart the clip; use `:play()`.
- `:Unload()` (also `:unload()`) stops playback and frees the handle's decoded PCM.
  Repeated unloads are harmless. Call `:load()` again before playing that handle.
  Stopping or natural completion retains loaded PCM for replay until unload or
  owner cleanup. Separate calls on an asset path create separate handles.
- Loaded clips and their voices clean up with the owning script on destruction,
  disable, successful reload, or Stop. A scene supports at most 1024 loaded clips
  and 1024 playback voices.
- Playing an asset proxy directly or a handle after unload raises a load-first
  error. Missing, unsupported, or invalid files fail during `load()`. Check the
  Console, attachment, file name and extension. Paths must stay inside `assets/`.

The object methods and `Audio.load` are available in Lua 5.4 and Luau. Existing
`Audio.play(source, options?)` and `Audio.playAt` in all languages still decode
and play immediately; their voice controls continue to work.

## Calls

- `Audio.load(source)` (Lua/Luau; returns a reusable audio handle)
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
- Clock scaling changes source playback rate; Clock.pause and voice.pause retain their source cursors. Natural completion releases non-looping voice PCM; loaded audio handles retain PCM until unload or owner cleanup. Stopping cancels voice-targeting actions on the next scheduler tick.
- Voices are owner-bound, with at most 1024 retained voices.
- Check script attachment, enabled state and the Console when an operation fails. Asynchronous errors also appear in operation state. Callback exceptions disable the owner and clean up its actions.
- Owner destruction, disable, successful reload or Stop cleans up owned actions and subscriptions. Failed reload retains the previous behavior's operations. See [Operation handles](/docs/api/operation-handles) for controls and lifetime details.
