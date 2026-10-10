# Scene sky and atmosphere

Each scene can save its own sky image, ambient light, sun light and haze. The settings affect the 3D editor viewport and the game renderer in Play, Play Solo and Run. They are scene settings, so you do not attach a sky to an entity. Inserted scene instances use the containing scene's environment.

## Set up a sky

1. Copy a **2:1 equirectangular panorama** into your project, for example `assets/sky/sunset.hdr`. Supported formats are Radiance HDR (`.hdr`), PNG, JPEG and TGA. A 4096 × 2048 image is a typical choice; each dimension must be at most 8192 pixels. Six separate cubemap faces, EXR and model files are not supported.
2. Open the Inspector and expand **Scene sky & atmosphere**, above the selected entity's properties. You can also use it with nothing selected.
3. Turn on **Enable scene environment** and click **Choose image…**. Select a file inside the project folder. The scene stores its project-relative path. Keep the file at that path when moving the project.
4. Adjust **Sky rotation** to turn the panorama and **Sky exposure (stops)** to brighten or darken it. HDR images use simple Reinhard tone mapping after exposure, then sRGB display encoding. PNG, JPEG and TGA colors are decoded before filtering and exposure, then encoded for display; at zero exposure their image colors are preserved. With no image selected, **Sky color** supplies a solid background.
5. Save the scene. Undo and redo work for environment changes. The image is included in the immutable snapshot when starting Play, including unsaved environment changes.

The panorama surrounds the camera at infinity: rotating the camera changes the view of the sky; moving it does not move the sky closer. **Clear image** switches to the solid color. **Reset environment** restores the disabled defaults. Disabling the environment restores the original background, ambient term and object lighting.

## Lighting and haze

- **Ambient color / intensity** illuminate every surface uniformly. A value of zero removes ambient light.
- **Sun color / intensity** add directional light independently of entity lights. The **Direction towards sun (X, Y, Z)** vector points from a surface towards the sun; it must be nonzero. For example, `(0.3, 0.8, 0.4)` lights upward-facing surfaces. Raising sun intensity from `0` to `1` makes it visible on geometry. Sky rotation does not rotate this direction.
- **Haze color / density** blend distant geometry towards the haze color. **Haze starts at** sets the distance from the camera before that blend begins, in world units. Density zero disables haze. The factor is `1 - exp(-density * max(distance - start, 0))`.
- Haze also tints the sky near the horizon. This horizon tint depends on density; the start distance controls geometry only.

Inspector color values and imported material factors are linear. Lighting stays linear, lit highlights are compressed with Reinhard tone mapping, and the sky, lit surfaces and haze are encoded to sRGB for display. This preserves ambient detail and avoids immediate white clipping when ambient or sun intensity is high.

For a simple daytime setup, choose a panorama, leave ambient intensity at `0.12`, set sun intensity to `1`, set haze density to `0.01` and haze start to `20`. Distant objects should gradually blend into the haze while close objects stay clear. Entity lights continue to contribute alongside the scene ambient and sun.

The image is a visible background; it does not automatically produce image-based lighting or reflections. Set ambient and sun values to match it. Volumetric clouds, physical atmosphere scattering, shadow casting, and a visible sun disc are not implemented. Editor grid, selection outlines and object guides remain unlit. The game needs an active camera to show the scene and its sky. The 2D viewport does not render the sky. Environment controls are disabled during play and in read-only documents.

## Diagnose problems

If the image cannot be selected, copy it into the project folder first; files outside the project are rejected. If the viewport reports a sky-image error, check that the file still exists, is readable, has a supported format, and is exactly twice as wide as it is tall. Invalid or oversized images stop that frame and show the renderer error; clear the image to return to a solid sky. Changes to an image are picked up on the next rendered frame. Restart Play to include changed image bytes in its snapshot.

If the sky is visible but objects are dark, increase ambient or sun intensity; the panorama itself does not illuminate objects. If haze is invisible, increase density or reduce the start distance. Scenes saved before environment settings were added open with the environment disabled. The corrected display encoding and highlight compression also apply to those scenes, so their lighting can look different from older builds. Damaged environment settings open read-only with a scene diagnostic.
