# Cameras and lights

Use **+ Add > Camera** or **+ Add > Light** in the hierarchy. Select the object to edit its component in the inspector. Component changes save with the scene and support Undo/Redo.

## Camera

Play, New Window, and Standalone render the live scene through its current active camera. The highest Priority wins; equal priorities use stable entity ID order. Scripts can select the current camera during startup or gameplay with `setCurrentCamera` (or the language-specific equivalent). This activates the selected camera and deactivates the others. Until a camera is active, the game view is empty.

**Align to editor view** copies the editor viewpoint into the selected camera. New cameras start aligned to the editor. Cameras look along local +Z with +Y up; parent transforms affect their position and orientation. Camera scale does not zoom the lens.

- Active: makes the camera eligible for the game view.
- Priority: selects the camera when more than one is active.
- Zoom: optical magnification. Values above 1 zoom in and values below 1 zoom out.
- Perspective field of view: changes how much of the scene is visible before zoom is applied.
- Orthographic vertical size: changes the visible world-space height without perspective foreshortening.
- Near/Far clip: limits visible depth.

The game view currently renders at 640 by 360, with a fixed 16:9 aspect ratio and letterboxing. Editor navigation and gizmos are separate from the game camera. Stop restores the editor view. Simulation changes to camera or parent transforms are reflected in subsequent game frames.

Selecting a camera displays a cyan frustum and center trajectory in the editor viewport. The guide follows the camera transform, projection, aspect ratio, clipping settings, and zoom. Very long far clipping distances are shortened in the editor guide so the scene remains usable; this does not change rendering.

## Light

Scene lights illuminate primitives in both the editor and game view. A small ambient term keeps unlit surfaces visible by default. An enabled [scene environment](SCENE_ENVIRONMENT.md) replaces that term with configurable ambient light and adds sun lighting and haze. The renderer uses up to 32 lights in stable entity order.

- Directional: shines along local +Z throughout the scene; rotate it to change illumination.
- Point: emits from its world position with inverse-square distance falloff, fading smoothly to zero at Range.
- Spot: combines point-light range with a cone along local +Z. Cone half-angle controls coverage, with a soft edge.
- Light color and Intensity affect illumination. Intensity zero turns the light off.

Point and spot Intensity is a relative source strength: away from the range boundary, doubling the source-to-surface distance gives approximately one quarter of the direct illumination. Range is a cutoff, not a brightness control; keep it well beyond the surfaces you want to illuminate. Falloff is capped within 0.1 world units to avoid a singularity at the source. Directional intensity is independent of distance. All lights use diffuse surface-angle shading; surfaces facing away receive no direct contribution. These are relative engine units, not calibrated lumens or lux.

To check distance falloff, add a point light in front of a flat surface, set Range to `100` and Intensity to `0.5`, and compare distances of `1` and `2` world units from the surface. The direct contribution should fall to roughly one quarter; ambient light remains, so the displayed pixel will not become exactly four times darker. If a surface clips to white, lower Intensity. Older scenes may need higher point/spot intensity after this correction, especially for lights several world units away.

Parent transforms move and rotate lights. Range and cone controls appear only for the light types that use them. Shadow casting is not implemented and has no editable control.

Selecting a light displays an amber editor guide: an arrow for directional lights, the range sphere for point lights, or the complete range cone and center trajectory for spot lights. Guides are editor-only and do not appear in the game view.

## Move and scale handles

Select an object in the Scene viewport and choose **Move** or **Scale** in the
editor toolbar. Drag a colored axis handle to change that axis. **World** uses
scene axes; **Local** uses the selected object's rotation. Enable **Snap** in the
viewport toolbar to use the displayed increment. Each completed drag creates one
Undo entry. Read-only projects show handles but do not allow dragging.

As you orbit, handles shorten when their axes point toward the camera. An almost
end-on handle is hidden because it has no useful screen direction; orbit slightly
to reveal it, or edit that component in the inspector. Move and Scale keep the
screen direction captured at the start of a drag so moving the pivot does not
reverse the drag. No component or script setup is required.

## Scene selection outline

In the editor Scene viewport, select a mesh to display its orange outline, then
orbit the camera around it. The outline should remain a narrow border when the
camera lines up with any object axis. No material or script setup is required.

The current renderer expands a back-face mesh shell using camera depth and viewport
resolution, targeting roughly three pixels of padding along each object axis.
Nonuniform object scale is accounted for in world space. This is an approximate
outline: corners, perspective across large meshes, and irregular mesh shapes can
vary in thickness. If a selection fills the viewport with orange at a particular
orbit angle, that is a rendering fault; it is not an outline or material setting.

## Verification

GPU tests compare actual rendered pixels for camera projection/clipping and light intensity, color, range, and direction. World tests cover component persistence, validation, and undo/redo. Runtime process tests transport the rendered scene through authenticated IPC in all three play modes and check pause, step, resume, and shutdown.

IPC generation 2 transports pixel buffers as bounded binary frames instead of decimal text. Editor and runtime must come from the same build.
