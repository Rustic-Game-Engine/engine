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
