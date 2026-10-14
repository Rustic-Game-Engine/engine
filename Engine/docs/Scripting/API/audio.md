# Audio API

Play engine-decoded WAV, MP3, AAC, FLAC, and OGG voices, adjust parameters and schedule fades.

## Setup and language support

These engine-owned services work in Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript. Follow [Use gameplay actions](/docs/guides/gameplay-actions) to create and attach a script, install external toolchains, and choose the correct language binding. PHP and HTML scripts belong under `ui/`; CSS alone runs no gameplay code.

The signatures and complete example below use JavaScript. Attach a JavaScript Object Component script to the specified target and press **Play**. Globals are supplied by the engine. Schedule actions from `Start` or a callback, rather than at script top level. Native languages use typed options and their own builder conventions; see the guide's language bindings.

## Supported audio files

| Format | Extensions | Supported encoding |
| --- | --- | --- |
| WAV | `.wav` | Integer PCM or floating-point PCM |
| MP3 | `.mp3` | MPEG Layer III |
| AAC | `.aac`, `.m4a` | AAC-LC in ADTS (`.aac`) or MP4 (`.m4a`) |
| FLAC | `.flac` | Native FLAC |
| OGG Vorbis | `.ogg`, `.oga` | Vorbis |
| OGG Opus | `.ogg`, `.oga`, `.opus` | Mono or stereo Opus (mapping family 0) |

Put the file inside your project's `assets/` folder. Extensions are case
insensitive. The same formats work with loaded audio handles in every gameplay language and
`Audio.play` / `Audio.playAt` in every supported script language. For example,
`rustic.game.assets.music["theme.mp3"]:load()` or
`Audio.play("assets/music/theme.flac")` uses the usual audio controls.

Decoding is built into the engine; players do not need FFmpeg or external codec
libraries. AAC support is AAC-LC; HE-AAC and encrypted M4A are unsupported. Opus
surround mappings and OGG codecs other than Vorbis/Opus are unsupported. A file's
container and encoding must match these supported combinations; renaming a file
does not convert it. Malformed files fail at import/load and report decoder
errors in the Console.

Compressed files decode fully into PCM at load/play time, so file size does not
represent playback memory use. The importer accepts at most 256 MiB of source
bytes and 200 million decoded samples per clip. Opus decodes at 48 kHz and
respects header gain, encoder delay, and end padding; other formats preserve the
decoded sample rate. Lossy decoder peaks are clamped to the mixer's [-1, 1] range.

## Load and control audio objects

Create `assets/sounds/` in your project and place `crash.wav` there (WAV, MP3, AAC, FLAC, and OGG are supported). Create a Lua or Luau Object Component script, attach it to an
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

Audio.load and loaded playback/control/unload are available in every gameplay language.
Asset path proxies retain their Lua/Luau syntax and are also available in JavaScript;
other languages use Audio.load(source). See [APIs in every language](/docs/guides/api-languages)
for native signatures and examples. Existing
`Audio.play(source, options?)` and `Audio.playAt` in all languages still decode
and play immediately; their voice controls continue to work.

## Choose audio devices

The four device methods are available on `Audio` in every supported script
language (including HTML inline JavaScript). Native enumeration and output
selection currently work in the Windows player. Other platforms return an empty
list and report an unavailable-backend error when selecting a device.

- `Audio.get_output_devices()` lists playback devices.
- `Audio.get_input_devices()` lists microphone/input devices.
- Each entry contains an opaque string `id` and a display `name`. Names can be
  duplicated; use the ID to select a device.
- `Audio.set_output_device(id)` switches this scene's mixed audio to that output.
- `Audio.set_input_device(id)` validates and retains the scene's selected input
  device. This call does not start capture or route microphone sound to playback.
  Recording and hot mic capture currently use the system default input
  independently of this selection; see the recording section below.
- Pass `null` (Lua/Luau `nil`, Python `None`, C `NULL`) to either setter to select
  the current system default. Java requires an explicit `null` argument; other
  bindings except C also allow omitting the argument. Setters return no value.
- Lists are snapshots. IDs belong to the current Play scene and the latest list
  for that direction; listing that direction again invalidates older IDs. Do
  not save IDs in project settings. Selection is shared by scripts in the scene
  and resets on Stop/new Play. Output and input IDs are not interchangeable.
- Output switching retains mixer voices and their cursors but discards samples
  queued on the previous device; a brief interruption is possible. A failed
  switch keeps the previous output. Device removal is not automatically recovered;
  list again and choose another device or the default.

Create and attach a JavaScript Object Component script to an enabled scene
object as described above. Press **Play** on Windows:

```javascript
globalThis.behavior = {
  Start() {
    const outputs = Audio.get_output_devices();
    if (outputs.length > 0) {
      Audio.set_output_device(outputs[0].id);
    }
    const inputs = Audio.get_input_devices();
    if (inputs.length > 0) {
      Audio.set_input_device(inputs[0].id);
    }
  }
};
```

The scene uses the first listed output, and retains the first input when one is
available. Play an existing audio asset to hear the selected output. To restore
system output, call `Audio.set_output_device(null)` from a callback.

Dynamic bindings return arrays/lists/tables of device records. C returns
`RusticAudioDevices` with `count` and `items[i].id` / `items[i].name`; these values
last until the next callback. C++ returns a `RusticValue` array (`.array()`);
C# returns a `JsonElement` array (`EnumerateArray()`); Java returns a JSON list
of maps; PHP returns an array of associative arrays.

Unknown/stale IDs, IDs from the other direction, unavailable devices, missing
system defaults and native driver failures raise a script error. A non-string
argument also raises an error. Check the Console; list devices again after
plugging/unplugging hardware. Empty lists are valid on hosts without devices.

## Calls

- `Audio.get_output_devices() / get_input_devices()`
- `Audio.set_output_device(id = null) / set_input_device(id = null)`
- `Audio.load(source)` (every gameplay language; returns a reusable audio handle)
- `Audio.play(source, options?)`
- `Audio.playAt(source, position, options?)`
- `voice.pause() / resume() / stop()`
- `Audio.volume(voice, value)`
- `Audio.pitch(voice, value)`
- `Audio.fadeIn(voice, duration, easing?, volume = 1)`
- `Audio.fadeOut(voice, duration, easing?)`
- `Audio.crossfade(oldVoice, newVoice, duration, easing?)`

## Parameters

Source is a project-relative WAV, MP3, AAC, FLAC, and OGG asset path. JavaScript options include loop, volume and pitch. Position is a world vector for distance attenuation. Volume is finite non-negative within f32 range; pitch is finite positive. Durations are scene-clock seconds.

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
- Windows player output starts on the default audio device; `Audio.set_output_device` can change it. Other platforms currently mix PCM headlessly, so successful playback may produce no device sound.
- FadeOut reaches zero volume but does not stop the voice; stop explicitly or allow natural completion. Crossfade fades the old voice to zero and the new voice from zero to one.
- PlayAt attenuates distance relative to the active camera. The mixer renders 48 kHz stereo PCM with linear resampling for pitch.
- Clock scaling changes source playback rate; Clock.pause and voice.pause retain their source cursors. Natural completion releases non-looping voice PCM; loaded audio handles retain PCM until unload or owner cleanup. Stopping cancels voice-targeting actions on the next scheduler tick.
- Voices are owner-bound, with at most 1024 retained voices.
- Check script attachment, enabled state and the Console when an operation fails. Asynchronous errors also appear in operation state. Callback exceptions disable the owner and clean up its actions.
- Owner destruction, disable, successful reload or Stop cleans up owned actions and subscriptions. Failed reload retains the previous behavior's operations. See [Operation handles](/docs/api/operation-handles) for controls and lifetime details.

## Record audio and read a hot microphone

Create a Lua or Luau Object Component script in `scripts/`, attach it to an
**enabled** scene object, and press **Play**. Start capture inside `Start` or
another callback. The example below uses Lua/Luau colon syntax. All gameplay languages expose
Game.startRecording, stopRecording, startHotMic, stopHotMic and capture
handle read/stop/unload; see [APIs in every language](/docs/guides/api-languages)
for copyable native bindings:

```lua
local rec
return {
  Start = function()
    rec = rustic.game:startRecording()
  end,
  OnStop = function()
    rec:stopRecording()
    local packet = rec:read()
    -- packet.samples contains interleaved floating-point PCM.
    -- Keep reading until #packet.samples == 0 to consume the recording.
    print(packet.sampleRate, packet.channels, #packet.samples)
    rec:unload()
  end
}
```

`rustic.game:startRecording(path?, options?)` returns a script-owned recording
handle. With no path it retains audio in memory, so the variable remains usable
after `:stopRecording()`. The default source is game mixer output: 48,000 Hz,
stereo, interleaved float PCM. This records audio, not video. Scene clock scaling
and paused playback affect the game audio being recorded.

To write a WAV file as well, pass a new `.wav` filename:

```lua
local rec = rustic.game:startRecording("C:/Users/YourName/Recordings/session.wav")
-- Later, inside a callback:
rec:stopRecording() -- finalizes the WAV header; returns rec
```

Use an absolute path to keep the file outside the temporary play snapshot.
Relative paths resolve inside the runtime snapshot directory. Parent directories
must already exist. Capture creates a new file and refuses to overwrite an
existing file. The output is a 32-bit floating-point WAV, retaining the same PCM
in the handle. A missing directory, invalid extension or unwritable destination
raises a script error at start; catch expected failures with `pcall`.

On Windows, microphone recording uses the default input device:

```lua
local rec = rustic.game:startRecording(nil, {source = "microphone"})
-- Or pass a .wav path as the first argument to also write it to disk.
rec:stopRecording()
```

Hot mic capture exposes local input immediately, without retaining a full recording:

```lua
local mic
return {
  Start = function()
    mic = rustic.game:startHotMic()
  end,
  Update = function()
    local packet = mic:read()
    local peak = 0
    for _, sample in ipairs(packet.samples) do
      peak = math.max(peak, math.abs(sample))
    end
    -- Feed packet.samples, packet.sampleRate and packet.channels to your
    -- audio processing or relay code. peak can drive a microphone meter.
  end,
  OnStop = function()
    mic:stopHotMic()
    mic:unload()
  end
}
```

- `rec:read()` and `mic:read()` return `{samples, sampleRate, channels, active,
  droppedSamples}`. Each call consumes at most 4,096 samples, preserving complete
  channel frames. An empty sample list means no queued data is available.
- Samples are interleaved by channel. Microphone rate/channel count come from the
  default device; input is converted to finite float samples in `[-1, 1]`.
- Hot mic queues at most two seconds of input. Read regularly, using multiple
  reads if needed. `droppedSamples` reports input discarded since the preceding
  read. Stopping hot mic closes the device and discards unread input.
- `rustic.game:stopRecording(rec)` and `rustic.game:stopHotMic(mic)` stop one
  handle. Omit the handle to stop all matching captures owned by the script.
  Handles also support `:stop()`; stopping repeatedly is safe.
- Recordings retain at most 64 MiB of PCM per handle (about 175 seconds of game
  audio), including recordings written to files. Exceeding the limit stops
  capture; `read`/`stopRecording` report the error. Split long recordings into
  separate handles and call `:unload()` after consuming them. A script may retain
  at most four capture handles at once.
- Destroying an owner, ending Play or successful reload closes its microphone,
  finalizes its WAV files and releases its capture handles. Other scripts cannot
  use those handles. Explicit stop callbacks let you read samples before cleanup.
- Microphone capture currently requires Windows, an available default microphone,
  and OS microphone access. Default F32, I16 and U16 device formats are supported.
  Missing devices, permission failures, unsupported formats, startup timeouts and
  device disconnection raise errors. Game mixer recording works headlessly too.
- These services are available in every gameplay language. Hot mic supplies
  local PCM; automatic network transport and speaker monitoring are not provided.
