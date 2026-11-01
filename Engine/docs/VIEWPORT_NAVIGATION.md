# 3D editor navigation

Open a project and stop Play before navigating the authoring camera. Open a missing
3D viewport through **View**. Hover the viewport to navigate without a prior click,
or click it to give it keyboard focus. A focused viewport keeps receiving navigation
keys when the pointer moves outside it. Text fields in other panels keep their own
keyboard input until you click the viewport. Navigation pauses when the editor
window loses focus.

| Control | Action |
| --- | --- |
| W / S | Move forward / backward. |
| A / D | Move left / right. |
| Q / E | Pan left / right horizontally. |
| Middle, right or left drag away from a gizmo | Orbit. |
| Shift + middle drag | Pan. |
| Mouse wheel | Dolly. |
| F | Frame the selected object. |
| Home | Reset the editor camera. |

Hold movement keys for continuous navigation; release them to stop. Only the
authoring camera moves. These controls do not move objects or the scene camera,
and keyboard navigation is unavailable in the 2D authoring viewport. During Play,
WASD is forwarded to gameplay scripts instead of navigating the authoring camera.

If WASD or Q/E does not respond, stop Play, activate the editor window and click
inside the 3D viewport to leave any Inspector or Explorer rename field. Use
**Home** to reset the view or select an object in the hierarchy and press **F**.

The corresponding website guide is **Editor overview > Viewport tools**.

## Standard UI and transform handles

The editor and Project Manager use egui's built-in widget styling and themes.
Select dark, light, or system appearance in the editor's **View** menu. Docking
uses egui_tiles with its standard tab and split interactions.
The dock workspace uses the active theme's standard panel background, including
empty panels and split gaps. In light mode, Inspector, Explorer and Console use
light backgrounds with dark text. Rendered scene colors still follow the scene.

3D transform handles use transform-gizmo-egui with its default visuals. Select an
object, choose **Move**, **Rotate**, or **Scale**, then choose **World** or **Local**
axes. Enable **Snap** and adjust the adjacent increment for stepped changes. Drag a
handle to preview the change and release to record one Undo entry, even outside
the viewport. Handle interaction owns pointer input while manipulating objects.
Read-only projects do not expose editable handles. Transforms that require shear
beneath nonuniformly scaled parents are rejected; use a uniformly scaled parent
or edit local values in Inspector.

## Scene Graph

Choose **View → Scene Graph** to inspect the loaded hierarchy using standard
egui-snarl nodes, pins, wires, pan, and zoom. Each entity is a node; wires run from
**Children** to **Parent**. Click a node name to select it in Inspector. Drag node
headers to arrange the view. Node positions last while the scene remains loaded. To change parent
links, use **Project → Scene**; the graph refreshes when shown. Wire gestures do
not modify the scene. An empty scene prompts you to add objects in Project. This
is a hierarchy viewer, not an executable visual scripting system.

## Resizing and display scaling

The authoring viewport renders at its physical pixel size, including display DPI and editor UI scaling. While a resized frame renders in the background, the last completed frame retains its aspect ratio. The editor repaints when the renderer finishes and skips queued obsolete resize requests. To check sharpness, drag a viewport divider between wide and tall shapes and repeat after changing display scaling. Play, New Window, and Standalone retain a 16:9 game view and render at the physical size of the letterboxed area, up to 4096 pixels per axis.
