# Scene sky and atmosphere

Each scene can save its own sky image, ambient light, sun light and haze. The settings affect the 3D editor viewport and the game renderer in Play, Play Solo and Run. They are scene settings, so you do not attach a sky to an entity. Inserted scene instances use the containing scene's environment.

## Set up a sky

1. Copy a **2:1 equirectangular panorama** into your project, for example `assets/sky/sunset.hdr`. Supported formats are Radiance HDR (`.hdr`), PNG, JPEG and TGA. A 4096 × 2048 image is a typical choice; each dimension must be at most 8192 pixels. Six separate cubemap faces, EXR and model files are not supported.
2. Open the Inspector and expand **Scene sky & atmosphere**, above the selected entity's properties. You can also use it with nothing selected.
3. Turn on **Enable scene environment** and click **Choose image…**. Select a file inside the project folder. The scene stores its project-relative path. Keep the file at that path when moving the project.
4. Adjust **Sky rotation** to turn the panorama and **Sky exposure (stops)** to brighten or darken it. HDR images use simple Reinhard tone mapping after exposure. With no image selected, **Sky color** supplies a solid background.
5. Save the scene. Undo and redo work for environment changes. The image is included in the immutable snapshot when starting Play, including unsaved environment changes.

The panorama surrounds the camera at infinity: rotating the camera changes the view of the sky; moving it does not move the sky closer. **Clear image** switches to the solid color. **Reset environment** restores the disabled defaults. Disabling the environment restores the original background, ambient term and object lighting.

## Lighting and haze

- **Ambient color / intensity** illuminate every surface uniformly. A value of zero removes ambient light.
- **Sun color / intensity** add directional light independently of entity lights. The **Direction towards sun (X, Y, Z)** vector points from a surface towards the sun; it must be nonzero. For example, `(0.3, 0.8, 0.4)` lights upward-facing surfaces. Raising sun intensity from `0` to `1` makes it visible on geometry. Sky rotation does not rotate this direction.
- **Haze color / density** blend distant geometry towards the haze color. **Haze starts at** sets the distance from the camera before that blend begins, in world units. Density zero disables haze. The factor is `1 - exp(-density * max(distance - start, 0))`.
- Haze also tints the sky near the horizon. This horizon tint depends on density; the start distance controls geometry only.

For a simple daytime setup, choose a panorama, leave ambient intensity at `0.12`, set sun intensity to `1`, set haze density to `0.01` and haze start to `20`. Distant objects should gradually blend into the haze while close objects stay clear. Entity lights continue to contribute alongside the scene ambient and sun.

The image is a visible background; it does not automatically produce image-based lighting or reflections. Set ambient and sun values to match it. Volumetric clouds, physical atmosphere scattering, shadow casting, and a visible sun disc are not implemented. Editor grid, selection outlines and object guides remain unlit. The game needs an active camera to show the scene and its sky. The 2D viewport does not render the sky. Environment controls are disabled during play and in read-only documents.

## Diagnose problems

If the image cannot be selected, copy it into the project folder first; files outside the project are rejected. If the viewport reports a sky-image error, check that the file still exists, is readable, has a supported format, and is exactly twice as wide as it is tall. Invalid or oversized images stop that frame and show the renderer error; clear the image to return to a solid sky. Changes to an image are picked up on the next rendered frame. Restart Play to include changed image bytes in its snapshot.

If the sky is visible but objects are dark, increase ambient or sun intensity; the panorama itself does not illuminate objects. If haze is invisible, increase density or reduce the start distance. Existing scenes open with the environment disabled and preserve their previous appearance. Damaged environment settings open read-only with a scene diagnostic.

## Change the environment from scripts

All ten scripting adapters expose `Game.scene.getEnvironment()`,
`Game.scene.setEnvironment(settings)` and `Game.scene.setSkyTexture(path)`.
These calls target the loaded scene, including from global startup scripts, scene
startup scripts, and object component scripts. They work in Play, Play Solo and Run.
CSS-only assets have no executable callbacks; HTML uses inline JavaScript.

1. Copy both daytime and nighttime 2:1 panoramas into your project, for example
   `assets/skies/day.hdr` and `assets/skies/night.hdr`, **before starting Play**.
   Runtime scripts use the immutable play snapshot; they cannot download an image
   or select a file outside it.
2. Create a Lua script using Explorer's scripting commands. Attach it as a
   **Scene Startup Script** for the scene, or select an object and use
   **+ Add Component** to attach the script. See the
   [language guides](Scripting/README.md) for other languages and their toolchains.
3. Use the following script and start Play with an active camera and visible geometry:

```lua
return {
  Start = function()
    Game.scene.setEnvironment({
      enabled = true,
      sky_image = 'assets/skies/day.hdr',
      rotation_degrees = 45,
      exposure = 0,
      ambient_color = {1, 1, 1},
      ambient_intensity = 0.12,
      sun_color = {1, 0.95, 0.85},
      sun_intensity = 1,
      sun_direction = {0.3, 0.8, 0.4},
      haze_color = {0.6, 0.7, 0.8},
      haze_density = 0.01,
      haze_start = 20
    })
    print(Game.scene.getEnvironment().sky_image)
  end
}
```

The next game frame shows the daytime sky, sun lighting and distance haze. The
console prints `assets/skies/day.hdr`. To switch skies during a later callback:

```lua
Game.scene.setSkyTexture('assets/skies/night.hdr')
Game.scene.setEnvironment({sun_intensity = 0, ambient_intensity = 0.06})
```

`setEnvironment` applies a partial update: omitted fields keep their current values.
It validates the entire resulting environment and applies all supplied fields
atomically. Unknown fields, wrong types and invalid values raise a script error
without changing the environment. `getEnvironment` returns a fresh copy of the live
settings; editing that copy alone has no effect. Reads immediately see successful
updates, including earlier updates from other scripts. Nine adapters return all
settings from setters too; C setters return void.

`setSkyTexture` changes only `sky_image`; it does not enable a disabled environment
or change lighting. Use an empty string to clear the panorama and show `sky_color`.
Use `setEnvironment({enabled=false})` in Lua (or `{enabled:false}` in JavaScript)
to disable the environment while keeping its settings. These runtime changes do
not save back to the editor scene or appear in Apply Runtime Changes. Stop and
restart Play to restore the editor's settings.

| Setting | Type and accepted values |
| --- | --- |
| `enabled` | Boolean; enables the sky, scene ambient/sun and haze. |
| `sky_image` | Project-relative HDR, PNG, JPEG or TGA path with `/` separators; empty clears it. Absolute paths, backslashes, drive prefixes, empty path segments, `.` and `..` are rejected. |
| `sky_color` | Three finite, non-negative RGB numbers; used without an image. |
| `rotation_degrees` | Finite panorama rotation in degrees. |
| `exposure` | Finite sky-image exposure in stops, from -20 to 20. |
| `ambient_color` | Three finite, non-negative RGB numbers. |
| `ambient_intensity` | Finite, non-negative ambient strength. |
| `sun_color` | Three finite, non-negative RGB numbers. |
| `sun_intensity` | Finite, non-negative directional light strength. |
| `sun_direction` | Three finite numbers pointing from a surface towards the sun; squared length must exceed 0.000001 and remain finite. |
| `haze_color` | Three finite, non-negative RGB numbers. |
| `haze_density` | Finite, non-negative density; zero disables haze. |
| `haze_start` | Finite, non-negative starting distance in world units. |

Colors may exceed 1 for HDR lighting. Values must fit the engine's 32-bit float
representation. Sky rotation does not rotate the sun. The sky remains a background,
so changing it does not generate reflections or image-based lighting.

### Other language calls

JavaScript and HTML inline JavaScript:

```javascript
globalThis.behavior = {
  Start() {
    Game.scene.setEnvironment({enabled: true, sun_intensity: 1});
    Game.scene.setSkyTexture('assets/skies/day.hdr');
    print(Game.scene.getEnvironment().sky_image);
  }
};
```

Python:

```python
from rustic import Game, run

def on_start():
    Game.scene.setEnvironment({"enabled": True, "sun_intensity": 1})
    Game.scene.setSkyTexture("assets/skies/day.hdr")
    print(Game.scene.getEnvironment()["sky_image"])

run(globals())
```

Luau uses the same calls and tables as Lua. In C++, pass a `RusticValue::Object`
(e.g. `{{"enabled",true},{"sun_intensity",1.0}}`); reads return a `RusticValue`
object. In C#, pass an anonymous object such as
`new {enabled=true,sun_intensity=1}`; reads return a `JsonElement`. In Java, pass
`Map.of("enabled",true,"sun_intensity",1)`; reads return an object containing a
map. PHP uses `$Game->scene->setEnvironment(["enabled"=>true])` and returns an
associative array. See the [environment API](/docs/api/environment) for copyable
calls for each language.

C reads and writes one named field per call, using `RusticValue`:

```c
Game.scene.setEnvironment("enabled", (RusticValue){.type=RUSTIC_BOOL,.boolean=true});
Game.scene.setEnvironment("sun_intensity", (RusticValue){.type=RUSTIC_NUMBER,.number=1});
Game.scene.setSkyTexture("assets/skies/day.hdr");
RusticValue intensity = Game.scene.getEnvironment("sun_intensity");
```

A C field update is atomic; several calls are separate updates. Returned C strings
last until the next callback. Other language calls accept a whole partial settings
object, so multiple fields can be changed in one atomic update.

### Diagnose script changes

If nothing changes, check `enabled`, the active camera, the callback name and the
script attachment. Read `getEnvironment()` to verify the live values. An invalid
update raises an error and an uncaught callback error disables that behavior;
inspect the game console for the diagnostic. Setting a texture validates the path
syntax; file existence, containment within the snapshot, supported format, 2:1
aspect ratio and image limits are checked by the renderer on the next frame.
Missing or invalid images show a renderer error. Clear `sky_image` or choose a
valid snapshot image, and restart Play after adding or changing image files.
