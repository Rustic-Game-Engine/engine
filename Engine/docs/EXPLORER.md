# Explorer and asset management

Prerequisites: a writable project and an existing asset file. Stop Play before reorganising project files. Switch **Project → Explorer** to browse directories; **Filter** matches names and project-relative paths.

## Import a model

1. Copy the model and its companion files into the project with your operating system's file manager. Keep glTF buffers/textures and OBJ dependencies in their required relative locations.
2. Find the model in Explorer. The viewport accepts `.obj`, `.gltf`, `.glb` and `.fbx` model drops.
3. Drag the file into the authoring viewport. The editor imports it, creates/reads its `.rmeta` sidecar and uses its Asset ID for scene instances.
4. Inspect the created object, save the scene and check Console for import errors.

Expected result: an imported model appears as a scene object. A recognised extension alone does not guarantee that every file variant is supported. Keep the original source and metadata together; regenerating metadata may change identity.

## Files and scripts

The project separates assets, scenes, scripts, `ui/` Web assets, and configuration. **Programming → Object Component Script** creates and registers a script for the selected object; global and scene startup scopes also exist. Existing registered scripts can be attached through **Programming → Attach Existing Script** or Inspector. Double-clicking a script opens the external editor. Right-click (or two-finger click on a trackpad) offers **Set As → Global Startup Script**, **Scene Startup Script**, or **Object Component Script**. Select the target scene object first to enable Object Component Script. See [the scripting guides](/docs/scripting/lua) for each language's setup, including Luau registration and optional external runtimes.

## Rename, move, copy, cut and delete

Hovering over a row does not start a move. Press and hold the primary mouse button on the item name, then drag to another project folder or the root. A folder's arrow expands or collapses it; dragging its name moves only that folder, and dragging a child moves the child.

Click a file or folder name once to edit it inline. The field appears after the short double-click window so double-click can still open a file. Press **Enter** or click elsewhere to save; **Escape** cancels. Use a single filename, including its extension. Empty names, path separators and existing names are rejected; Console explains the failure.

Right-click an item, or use a two-finger trackpad click, for **Copy**, **Cut**, **Paste** and **Delete**. Copy/Cut stores an item in the editor clipboard; Paste on a folder uses that folder, and Paste on a file uses its containing folder. Right-click empty tree space for **Paste at Project Root**. Copies get a numbered name when needed. Cut leaves the source in place until a successful Paste. Delete moves the entry into `.rustic/trash/<identifier>/`; recover it with your operating system's file manager. Scene Undo does not undo filesystem actions.

These controls require a writable project and stopped Play. Moves and renames keep a file's `.rmeta` sidecar alongside it. Copies omit `.rmeta` so a new asset identity can be assigned on import or script registration. **Manifest paths, scene references and companion-file links are not rewritten.** Update affected references before Play; copied scripts must be registered using the script scope commands. A folder copy does not rewrite references within its files. Operations reject traversal, symbolic links, missing destinations, move collisions and moving/copying a folder into itself.

World/Local space applies to scene transforms: switch to **Project → Scene**, right-click the object and choose **Use World Space** or **Use Local Space**. This changes the active transform gizmo axes without changing the object's transform.

## Known limitations and diagnosis

- `.rmeta` entries and generated/build/cache directories are filtered from the tree; a hidden entry is not necessarily missing from disk.
- For missing files, check the project-relative path and filename case, then dependent buffers/textures.
- For importer failures, inspect Console and the [asset pipeline](/docs/open-source/engine/architecture) limitations.
- For attachment failures, check `config/scripts.ron`, scope, language and toolchain; copying a script into a folder alone does not register it.

Related: [Inspector](/docs/guides/editor/inspector), [Playtesting](/docs/guides/editor/playtesting).

## Scene objects

Right-click an object (or two-finger click on a trackpad) for **Copy**, **Cut**, **Paste**, **Delete**, **Use World Space** and **Use Local Space**. Copy captures the object and its descendants; Paste gives them fresh entity IDs and preserves their internal parent links. Paste on an object makes the pasted root its child; right-click empty tree space for **Paste at Scene Root**. Cut moves the original tree only after a successful Paste. Delete removes the object and its descendants. Rename, Paste, Cut/Paste and Delete support **Edit → Undo/Redo** and are disabled during Play or read-only access.

World/Local space changes the active gizmo's axes without modifying the stored transform. Copied scripts and other components keep their existing asset references; embedded entity IDs in arbitrary component data are not rewritten. There is no Delete-key shortcut. Creating a folder does not create a filesystem directory, and moving a scene object does not move its source asset.

