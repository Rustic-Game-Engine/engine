---
name: rustic-scene-explorer
description: Organize the Rustic scene explorer, create folders, parent objects, and add, duplicate or delete scene objects and folder subtrees through plugin tools. Use for hierarchy layout, scene cleanup and grouped scene authoring.
---

# Lay out the scene explorer

The scene explorer (Hierarchy panel) represents the active scene's objects and parent links. A folder is an organizational scene entity with `folder:true`, not a disk directory or a separate scene. Creating a directory through file tools does not create an explorer folder. This skill organizes scene content; dock/tab layout is a separate editor preference.

## Inspect and plan

1. Call `workspace_info`, read project instructions, and open that project in its writable editor. Read `editor_status` for live IDs, names, `folder` flags, parents and transforms. Saved-file `scene_summary` also reports folder flags, but does not include unsaved edits.
2. Match the hierarchy to the game. For a small scene, a few root groups such as World, Gameplay, Lighting and UI may suffice. Within World, group by area or responsibility (Town/Buildings, Forest/Vegetation); within Gameplay, group related actors, triggers and spawn points. Use only useful groups: do not add an elaborate empty hierarchy to a simple scene.
3. Use short, descriptive names and shallow nesting. Preserve existing naming conventions and keep asset filenames, object names and folder purpose understandable. Work with UUIDs from the tools; names need not be unique. Keep objects from different scenes separate and refresh IDs after switching scenes.
4. Folders organize the hierarchy and contribute NO transform to their descendants: changing a folder's translation, rotation or scale does not move its contents. Use an ordinary empty root object when an assembly must move as a group. Reparenting preserves the child's LOCAL transform, so changing its non-folder ancestors can change world placement. Inspect the whole ancestor chain first; root-level folders add no transform of their own, but a folder nested under a transformed ordinary object still inherits that ancestor's transform. Calculate and apply the intended local transform when needed.

## Add folders and objects directly

Use the plugin rather than computer use for supported operations. Create a root folder:

```json
{"name":"World","kind":"folder"}
```

Pass that object to `object_create`. Use the returned `entity_id` as `parent` for nested folders or objects:

```json
{"name":"Buildings","kind":"folder","parent":"<World UUID>"}
```

`object_create` supports `folder`, `empty`, `camera`, `light` and deliberate `blockout` objects. Omit `parent` or pass null to create at the scene root. Visible finished objects should be modeled and placed with `model_create`; then use `object_update` to parent them under the appropriate folder:

```json
{"entity_id":"<model UUID>","properties":{"parent":"<Buildings UUID>"}}
```

Use `object_update` with `name` to rename a folder or object. Use `parent:null` to move it back to scene root. Cycle, missing-parent and invalid-property edits fail without changing the hierarchy. Existing objects can be renamed or regrouped without recreating them. Folder grouping alone does not change script lifetime or make its children global.

## Duplicate a scene object or folder

Call `object_duplicate` with the source `entity_id`. It copies the entire subtree as one undoable operation: every copied object receives a fresh UUID; internal parent links and folder flags are preserved; transforms, component values and script attachments are copied. Mesh/material assets and script source IDs remain shared, and arbitrary entity IDs stored inside script properties are not rewritten automatically. Inspect those references if the copy needs independent targets.

```json
{"entity_id":"<Buildings UUID>","name":"Buildings copy"}
```

By default the copy is a sibling under the source's existing parent. An explicit `parent` selects another destination; `parent:null` creates the copy at scene root. `name` renames only the copied root. Local transforms are preserved, so a copy overlaps the original unless moved with `object_update`. The result contains the new root `entity_id`, all new `entity_ids`, and the original `source_entity_id`. Do not reuse source IDs to modify the copy.

## Delete and recover

Read `editor_status` to understand the target's descendants, then call `object_delete` with its `entity_id`. Deleting a folder or object removes ALL of its descendants as one undoable edit. To keep children, move them to a valid surviving parent before deleting their former container. Do not delete unrelated scene content as part of organizational cleanup.

```json
{"entity_id":"<obsolete object or folder UUID>"}
```

The result lists `deleted_entity_ids`. Asset files, script source files, global scripts and scene startup script lists are not deleted. Use `editor_undo` to restore the subtree, or `editor_redo` to repeat the deletion. These operations also support ordinary objects, not just folders.

## Verify and persist

Re-read `editor_status` after changes to confirm folder flags, fresh IDs and parent relationships, then use `viewport_capture` to inspect the scene for unintended placement changes. Save with `scene_save`; create, duplicate, reparent and delete operations initially affect the unsaved editor document. Stop play and resolve pending runtime-change review before editing. A read-only editor cannot perform authoring operations. If tools are missing, update/reopen Rustic and reload its MCP connection; use `scene_explorer_guide` to obtain the current bundled skill even when older customized instructions have been preserved. Never invent success or edit a stale saved scene behind an unsaved document as a fallback.
