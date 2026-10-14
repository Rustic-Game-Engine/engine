# APIs in every supported language

Lua 5.4, Luau, JavaScript, Python, C, C++, C#, Java, PHP, and HTML inline JavaScript
use the same engine services. CSS supplies styling and executes no API calls.

## Create and attach a script

1. Open a project and create an **Object Component Script** from
   **Programming > New Script**, choosing your language.
2. Select an enabled scene object and use **+ Add Component** to attach the script.
3. External languages need their toolchain on PATH: Python 3, a C17 or C++20
   compiler, .NET SDK 10+, Java with javac, or PHP CLI. Lua, JavaScript and the
   isolated Luau host ship with Rustic.
4. Generate the programming workspace from the editor before using an external
   editor. Include/import the engine-owned SDK shown in your
   [language guide](/docs/guides/scripting). PHP and HTML assets belong under ui/.
5. Put tone.wav in assets/, then run the examples below from **Start**.
   Press **Play**. Playback is audible on Windows; other platforms currently mix
   without a native output device.

## Shared API families

| Service | Available operations |
| --- | --- |
| Lifecycle and owner state | Construction, startup, enable/disable, fixed/frame updates, destroy/stop, translation, properties, built-in attributes, input, logging |
| Scene and object services | Find/list, live entity lookup, read/edit other objects, instance add/clone, current camera selection, environment/sky settings |
| Entity foundations | Tags, groups, hierarchy, destruction/lifetime, components, radius/tag queries |
| Gameplay actions | Tween/value, easing, interpolation, movement, paths, timers, sequence/timeline, effects, camera actions |
| Animation | Load/register clips, play/blend, speed/loop/pause/stop, markers, keyframes, IK and procedural actions |
| Events and time | Signals, connections, operation handles, scene clock read/pause/resume/time scale |
| Audio | Play/spatial play, reusable loaded clips, voice controls, volume/pitch/fades, device selection |
| Capture | Record game mix or microphone, hot mic, stop/read/unload script-owned PCM handles |

All these families are available in every gameplay language. Names and signatures
follow each language's SDK; native bindings use typed options and handles.
HTML shares the JavaScript sandbox. This does not add browser APIs or DOM access.

## Load audio and record the mix

These are callback-body examples, not standalone programs. Use the callback
registration from your language guide. They play the same clip, pause/resume it,
stop and replay it, then release it. Recording starts before playback and stops
afterward; a read returns the PCM mixed between callbacks, so stopping immediately
within Start can produce an empty packet. Retain the handles across callbacks and
stop/read in Update or a Timer callback when recording a longer session.
The immediate stop/unload calls below run before a mixed frame, so these short
snippets produce no audible output. Omit those controls until a later callback to
hear playback and collect a nonempty recording.

### Lua 5.4 / Luau

```lua
local rec = Game.startRecording()
local clip = Audio.load('assets/tone.wav')
clip:play({loop=true})
clip:pause(); clip:resume(); clip:stop(); clip:play()
clip:unload()
Game.stopRecording(rec)
local packet = rec:read()
rec:unload()
```

### JavaScript / HTML inline JavaScript

```javascript
const rec = Game.startRecording();
const clip = Audio.load('assets/tone.wav');
clip.play({loop:true});
clip.pause(); clip.resume(); clip.stop(); clip.play();
clip.unload();
Game.stopRecording(rec);
const packet = rec.read();
rec.unload();
```

### Python

```python
# from rustic import Audio, Game
rec = Game.startRecording()
clip = Audio.load('assets/tone.wav')
clip.play({'loop':True})
clip.pause(); clip.resume(); clip.stop(); clip.play()
clip.unload()
Game.stopRecording(rec)
packet = rec.read()
rec.unload()
```

### C

```c
/* #include "rustic.h" */
RusticCaptureHandle rec = Game.startRecording(NULL, NULL);
RusticLoadedAudio clip = Audio.load("assets/tone.wav");
RusticAudioOptions options = {.volume=1, .pitch=1, .loop=true};
LoadedAudio.play(clip, &options);
LoadedAudio.pause(clip); LoadedAudio.resume(clip); LoadedAudio.stop(clip);
LoadedAudio.play(clip, NULL); LoadedAudio.unload(&clip);
Game.stopRecording(&rec);
RusticCaptureData packet = Capture.read(rec);
Capture.unload(rec);
```

### C++

```cpp
// #include "rustic.hpp"
auto rec = Game.startRecording();
auto clip = Audio::load("assets/tone.wav");
clip.play(1,1,true);
clip.pause(); clip.resume(); clip.stop(); clip.play();
clip.unload();
Game.stopRecording(&rec);
auto packet = rec.read();
rec.unload();
```

### C#

```csharp
// using static Rustic;
var rec = Game.startRecording();
var clip = Audio.load("assets/tone.wav");
clip.play(loop:true);
clip.pause(); clip.resume(); clip.stop(); clip.play();
clip.unload();
Game.stopRecording(rec);
var packet = rec.read();
rec.unload();
```

### Java

```java
// Inside a behavior extending Rustic.
var rec = Game.startRecording();
var clip = Audio.load("assets/tone.wav");
clip.play(1,1,true);
clip.pause(); clip.resume(); clip.stop(); clip.play();
clip.unload();
Game.stopRecording(rec);
var packet = (java.util.Map<?,?>)rec.read();
rec.unload();
```

### PHP

```php
// Inside a callback after require_once 'rustic.php'.
global $Game;
$rec = $Game->startRecording();
$clip = Audio::load('assets/tone.wav');
$clip->play(['loop'=>true]);
$clip->pause(); $clip->resume(); $clip->stop(); $clip->play();
$clip->unload();
$Game->stopRecording($rec);
$packet = $rec->read();
$rec->unload();
```

Audio.load decodes once. play reuses retained PCM even if the source file is
removed afterward. Stop retains PCM; unload frees it and stops its playback.
Unload twice is harmless. Lua/Luau, JavaScript, Python, C++, C#, Java and PHP can
call clip.load() (Lua uses a colon) to load the same object again after unload.
C calls Audio.load(source) to obtain a new handle. C++ copies share loaded state.
A voice returned by loaded playback supports the same fades and controls as an
Audio.play voice.

## Microphone capture

Use Game.startHotMic() instead of startRecording() to obtain a microphone
handle, then use its read, stop, and unload methods. C uses
Game.startHotMic(), Capture.read(mic), Capture.stop(mic) and
Capture.unload(mic). Stop all owned hot mics with Game.stopHotMic(); C passes
NULL, C++ omits the pointer, and Java passes null or omits the argument.

To record microphone PCM, use Game.startRecording(null, {source:'microphone'})
in JavaScript, Game.startRecording(None, {'source':'microphone'}) in Python,
Game.startRecording(nil, {source='microphone'}) in Lua/Luau, or
$Game->startRecording(null, ['source'=>'microphone']) in PHP. C, C++, C# and Java
pass "microphone" as the second argument. The existing rustic.game entry point
also exposes the capture methods in each SDK.

Capture packets contain samples, sampleRate, channels, active, and
droppedSamples. Samples are interleaved floating-point PCM. Reads advance the
cursor; repeated reads do not repeat samples. C uses packet.count for the number
of samples; its sample storage remains valid until the next script callback.
Recording and microphone capture retain the existing platform limits:
Windows supplies microphone input; other platforms report an unavailable-backend
error. Device selection does not currently change capture's system-default input.
See the [Audio API](/docs/api/audio) for file paths, limits and cleanup.

## Read another object's current attributes

Use rustic.get_object_attribute(source, name) in every language, with native
member syntax ($rustic-> in PHP). source accepts a stable entity ID or
rustic.game.Scene.Root.Child. C returns RusticValue; C++ returns RusticValue;
C#/Java return ordinary scalar or vector values. JavaScript/Python/Lua return
their ordinary values. Path proxies also expose GetAttribute and get_attribute;
C uses rustic.game.GetAttribute(source, name).

Edits queue in JavaScript and external adapters. A live query flushes earlier
commands before reading, so an edit followed by get_object_attribute in the same
callback observes the new value. Owner snapshot getters such as get_translation
and get_attribute retain their callback snapshot in those adapters. Lua applies
writes immediately. rustic.find_entity(source) performs live lookup; scene
Find/List retain their existing snapshot semantics in external adapters. List returns\none entity ID per matched object, supports a subtree path and defaults to Game.scene.

## Diagnose failures

- Missing external SDK/toolchain: regenerate the workspace and check the Console's
  toolchain diagnostic. Do not write your own protocol loop.
- Missing clip: check the exact assets/ path and supported encoding. Playing after
  unload requires load first. A C handle must be replaced by Audio.load.
- Silent output or microphone unavailable: check the platform, Windows device
  permissions and the Console. A stopped recording still permits reading.
- Missing/stale object: use a stable entity ID or verify the scene/hierarchy path;
  a destroyed object cannot supply attributes.
- Capture/action lifetime: handles belong to the script and clean up on disable,
  destruction, successful reload or Stop.

Input remains limited to held WASD, arrows and Shift in embedded Play. Collision
and touch callbacks are not dispatched. These are shared runtime limits across
languages.
