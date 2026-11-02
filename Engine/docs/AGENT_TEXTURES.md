# Author HDR/PBR textures with an AI agent

The Rustic agent plugin can inspect and edit all 17 optional surface map slots, create Texture children and configure an HDR sky in the open editor. These are scene-authoring tools with validation, undo/redo and save support. They do not add a gameplay material scripting API or generate/download images.

## Setup

1. Install the updated engine, open the project in its writable editor, and reload the Rustic MCP connection in Codex or Claude. Opening the editor installs the `rustic-textures` skill alongside `rustic-workspace`; existing customized stored skills are preserved.
2. Invoke `rustic-textures` in Codex or `/rustic-textures` in Claude, or call `texture_guide` with `{}` to read the current built-in guidance. The guide tool works even when older installed skills were customized.
3. Call `workspace_info` for the selected project, read its agent instructions, then call `editor_status`. Use actual returned object IDs. Stop Play and resolve any pending runtime-change review before editing.
4. Put final PNG, JPEG, TGA or Radiance HDR files inside the project. Paths passed to texture tools must be project-relative and use `/`; absolute paths, traversal and symlinks are rejected. Images outside `assets` are copied into `assets/textures`. Keep asset generators and intermediate files outside the game directory.

The tools execute on the editor thread. Read-only projects reject edits; texture inspection remains available. `texture_guide` needs no live editor, but scene tools require the selected project's writable editor to be open. Use authorized host asset tools to create or obtain images, then copy the final assets into the project before assignment.

## Tools

| Tool | Arguments | Result |
| --- | --- | --- |
| `texture_guide` | `{}` | Current PBR/HDR workflow, exact slot names, conventions, examples and limitations. |
| `texture_inspect` | `{"entity_id":"<object-id>"}` | Direct `textures`, effective `resolved_textures`, and immediate `texture_children` in override order. |
| `texture_update` | `entity_id`, optional `maps`, `clear_maps`, `uv_scale`, `uv_offset`, `height_scale`, `emissive_strength` | One undoable material edit on a mesh, primitive or existing Texture child. |
| `texture_create_child` | Parent object's `entity_id`, optional `name`, and the same map/control arguments | Creates the child and initial material in one undoable edit; returns the child's `entity_id`. |
| `environment_update` | `{"settings":{...}}` | Patches sky/atmosphere fields in one undoable edit. Read existing fields in `editor_status.environment`. |

`editor_status.objects` includes direct and resolved texture data. `scene_summary` reports saved texture components; it does not include unsaved changes.

## Assign surface maps

Call `texture_update` with the target mesh or primitive's ID:

```json
{
  "entity_id": "<mesh-id>",
  "maps": {
    "BaseColor": "assets/stone_albedo.png",
    "Normal": "assets/stone_normal.png",
    "Roughness": "assets/stone_roughness.png"
  },
  "uv_scale": [2, 2],
  "uv_offset": [0, 0]
}
```

`maps` patches only supplied slots; unmentioned slots and controls remain unchanged. `null` clears one slot. `clear_maps:true` clears all previous slots before applying any supplied maps and retains the texture component and controls. For example:

```json
{"entity_id":"<mesh-id>","maps":{"Normal":null},"emissive_strength":2}
```

Use the exact case-sensitive keys `BaseColor`, `Normal`, `Roughness`, `Metallic`, `AmbientOcclusion`, `Height`, `Emissive`, `Clearcoat`, `ClearcoatRoughness`, `ClearcoatNormal`, `Transmission`, `Thickness`, `SheenColor`, `SheenRoughness`, `Specular`, `SpecularColor`, and `Anisotropy`. Every slot is optional. Tools use explicit slot keys rather than filename inference; packed ORM images are not automatically split. See [Surface textures and PBR maps](SURFACE_TEXTURES.md) for channels, color spaces and rendering limits.

Images are decoded before assignment. Unknown slots, invalid controls, missing/corrupt images and invalid targets fail without partially changing the scene. A failed multi-image import can leave an unused copied asset; do not remove existing user assets to clean it up.

## Attach a Texture child

Call `texture_create_child` with the parent mesh/primitive's ID:

```json
{
  "entity_id": "<mesh-id>",
  "name": "Stone surface",
  "maps": {
    "BaseColor": "assets/stone_albedo.png",
    "Emissive": "assets/glow.hdr"
  },
  "emissive_strength": 2
}
```

The returned child ID is the target for later `texture_update` calls. To move its effect, call `object_update` with the child's `entity_id` and `{"parent":"<other-mesh-id>"}` in `properties`.

A Texture child applies only to its immediate parent. Its supplied maps override direct slots; empty slots retain the parent's maps. With several children, stable entity-ID order resolves duplicate slots, and the last nonempty child's controls apply to the combined material. Prefer one Texture child per object for predictable editing. `texture_inspect` shows the resulting maps and children.

## Set an HDR sky

A surface map follows object UVs. A 2:1 equirectangular panorama belongs in the scene environment. Call `environment_update`:

```json
{
  "settings": {
    "enabled": true,
    "sky_image": "assets/studio.hdr",
    "exposure": 1,
    "ambient_intensity": 0.2
  }
}
```

Other fields include `sky_color`, `rotation_degrees`, `ambient_color`, `sun_color`, `sun_intensity`, `sun_direction`, `haze_color`, `haze_density` and `haze_start`. Unspecified fields stay unchanged. Set `sky_image` to `""` to restore solid sky color. Environment validation rejects invalid directions, energy, exposure, colors and paths.

HDR preserves linear image values above one; it does not make one image into a complete PBR material. The sky currently does not supply image-based reflections on materials. Transmission/thickness approximate thin-surface lighting rather than refracted scene rendering; emissive maps do not light other objects. Height displaces existing vertices without tessellation or collider/picking changes.

## Inspect, save and verify

Texture and environment edits return `saved:false`. Request a fresh `viewport_capture`, inspect `editor_status` logs, and call `scene_save` to persist them. Set the object's `part_attributes.color` to `[1,1,1,1]` for untinted albedo. Start a real playtest with `playtest_control`, capture frames and inspect logs, then stop before further edits. Use `editor_undo` and `editor_redo` to reverse edits or child creation/movement.

If a tool fails, check the project-relative filename, extension, decode diagnostic, target ID and edit state. Imported models need authored UVs; the agent-only Lua mesh recipe exports positions/triangles and cannot provide useful UV mapping. Source images are limited to 8192 pixels per axis and bounded decode memory; GPU maps are resized to at most 1024 pixels per axis. Maps share one UV transform and use bilinear repeat sampling without mipmaps or streaming. To reload an overwritten image, clear its slot, wait for a fresh viewport frame, then reassign it.

Stored skill files can be read or edited through `read_agent_file`/`write_agent_file`, scope `user`, using `.agents/skills/rustic-textures/SKILL.md` for Codex or `.claude/skills/rustic-textures/SKILL.md` for Claude. Reopening the editor preserves these edits; `texture_guide` always returns the bundled guidance.
