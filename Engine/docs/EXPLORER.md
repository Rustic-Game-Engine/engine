# Explorer and asset management

Prerequisites: a writable project and an existing asset file. Stop Play before reorganising project files. Switch **Explorer** to browse directories; **Filter** matches names and project-relative paths.

**Explorer** and **Scene Explorer** are independent dock tabs. Drag a tab header to group it elsewhere, or drop on a docking preview at a panel edge to split the view. **Properties** edits the selected object; **Terminal** displays runtime logs. Use the **View → Panels** checkboxes to show or hide any panel without changing its dock position. Visibility choices survive reopening the project. Use **View → Reset dock layout** to clear hidden-panel choices and recover the default arrangement. Dock geometry resets on project reopening.

## Import a model

1. Copy the model and its companion files into the project with your operating system's file manager. Keep glTF buffers/textures and OBJ dependencies in their required relative locations.
2. Find the model in Explorer. The viewport accepts `.obj`, `.gltf`, `.glb` and `.fbx` model drops.
3. Click the file once to highlight it, then press and hold on its highlighted name and drag it into the authoring viewport. The editor imports it, creates/reads its `.rmeta` sidecar and uses its Asset ID for scene instances.
4. Inspect the created object, save the scene and check Console for import errors.

Expected result: an imported model appears as a scene object. A recognised extension alone does not guarantee that every file variant is supported. Keep the original source and metadata together; regenerating metadata may change identity.

## Files and scripts

The project separates assets, scenes, scripts, `ui/` Web assets, and configuration. **Programming → Object Component Script** creates and registers a script for the selected object; global and scene startup scopes also exist. Existing registered scripts can be attached through **Programming → Attach Existing Script** or Inspector. Right-click a script and choose **Open** to launch the external editor. Right-click (or two-finger click on a trackpad) offers **Set As → Global Startup Script**, **Scene Startup Script**, or **Object Component Script**. Select the target scene object first to enable Object Component Script. See [the scripting guides](/docs/scripting/lua) for each language's setup, including Luau registration and optional external runtimes.

## Rename, move, copy, cut and delete

Click an Explorer file or folder name once to highlight it. Double-click the name to edit it inline. Press **Enter** or click elsewhere to save; **Escape** cancels. Use a single filename, including its extension. Empty names, path separators and existing names are rejected; Console explains the failure. Scene Explorer selection and rename controls keep their existing behavior.

**Shift-click** adds files or folders to the selection. Holding Shift and the primary mouse button while moving over names also adds those items. **Ctrl-click** highlights the visible range between the last highlighted item and the new item, including both ends. The range follows the tree's displayed order and skips collapsed/filtered entries. Ctrl+Shift adds that range to the existing selection.

Hovering does not start a move. To drag, the item must already be highlighted before you press and hold on its name. Pressing and holding on an unhighlighted item highlights it; that press cannot start a drag. Release, then press and drag the highlighted name. Dragging a highlighted item moves the highlighted group to the destination folder or project root. Shift/Ctrl selection gestures do not start drags. A folder's arrow expands or collapses it. A selected folder and its selected descendants are moved/copied as one subtree.

Press **Ctrl+C** to copy highlighted items. Highlight a destination folder and press **Ctrl+V** to paste. If several folders are highlighted, each receives a copy of every copied item. This includes nested selected destinations. Without a selected folder, Paste uses the last selected file's containing folder, or the project root when the selection is empty. The editor also accepts Command+C/V on macOS. Shortcuts apply while Explorer has focus; a focused rename/filter text field uses normal text copy/paste. Copies remain in the clipboard for repeated Paste and get numbered names on collisions.

Right-click an item, or use a two-finger trackpad click, for **Copy**, **Cut**, **Paste**, **Delete**, and file **Open**. Copy/Cut/Delete uses the highlighted group when the clicked item is highlighted; otherwise the clicked item becomes the selection. Menu Paste on a folder uses that folder, and Paste on a file uses its containing folder. Right-click empty tree space for **Paste at Project Root**. Cut leaves sources in place until a successful Paste and accepts a single destination folder; use Copy for multiple destinations. A batch reports individual failures in Console and refreshes completed operations. Delete moves entries into `.rustic/trash/<identifier>/`; recover them with your operating system's file manager. The engine-owned `.rustic` directory is hidden from Explorer, including generated files and recovery trash. Scene Undo does not undo filesystem actions.

Select one model or script at a time for viewport drops; grouped drags organize files within Explorer.

These controls require a writable project and stopped Play. Moves and renames keep a file's `.rmeta` sidecar alongside it. Copies omit `.rmeta` so a new asset identity can be assigned on import or script registration. **Manifest paths, scene references and companion-file links are not rewritten.** Update affected references before Play; copied scripts must be registered using the script scope commands. A folder copy does not rewrite references within its files. Operations reject traversal, symbolic links, missing destinations, move collisions and moving/copying a folder into itself. A dangling destination symlink or an existing destination `.rmeta` also blocks a move or rename, even when the source has no metadata; inspect the destination with your file manager and resolve the conflict before retrying. Do not reuse orphaned metadata for an unrelated asset.

Successful Explorer file operations reload imported models and refresh the viewport. Moving or renaming a model to another supported model extension preserves its Asset ID through the sidecar. Deleting a model or changing its extension to an unsupported one removes its cached geometry from rendering and picking; the scene object remains with its unresolved asset reference. Restore the source and sidecar to a supported path, then restart the editor or perform another Explorer file operation to reload it.

World/Local space applies to scene transforms: switch to **Scene Explorer**, right-click the object and choose **Use World Space** or **Use Local Space**. This changes the active transform gizmo axes without changing the object's transform.

## Known limitations and diagnosis

- `.rmeta` entries and generated/build/cache directories are filtered from the tree; a hidden entry is not necessarily missing from disk.
- For missing files, check the project-relative path and filename case, then dependent buffers/textures.
- For importer failures, inspect Console and the [asset pipeline](/docs/open-source/engine/architecture) limitations.
- For attachment failures, check `config/scripts.ron`, scope, language and toolchain; copying a script into a folder alone does not register it.

Related: [Inspector](/docs/guides/editor/inspector), [Playtesting](/docs/guides/editor/playtesting).

## Scene objects

Right-click an object (or two-finger click on a trackpad) for **Copy**, **Cut**, **Paste**, **Delete**, **Use World Space** and **Use Local Space**. Copy captures the object and its descendants; Paste gives them fresh entity IDs and preserves their internal parent links. Paste on an object makes the pasted root its child; right-click empty tree space for **Paste at Scene Root**. Cut moves the original tree only after a successful Paste. Delete removes the object and its descendants. Rename, Paste, Cut/Paste and Delete support **Edit → Undo/Redo** and are disabled during Play or read-only access.

World/Local space changes the active gizmo's axes without modifying the stored transform. Copied scripts and other components keep their existing asset references; embedded entity IDs in arbitrary component data are not rewritten. There is no Delete-key shortcut. Creating a folder does not create a filesystem directory, and moving a scene object does not move its source asset.


## Surface textures

See [Surface textures and PBR maps](SURFACE_TEXTURES.md) for map browsing, HDR/PBR file drops, and Texture child objects.
