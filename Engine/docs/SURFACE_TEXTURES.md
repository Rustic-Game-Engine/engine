# Surface textures and PBR maps

Surface maps work on built-in primitives and imported mesh objects in the 3D editor and native Play renderer. Every map is optional. PNG, JPEG, TGA and Radiance HDR (`.hdr`) files are supported. A surface HDR map is sampled using the object's UVs; use **Scene sky & atmosphere** for a panoramic sky instead.

## Browse for maps

1. Select a primitive or imported mesh in the scene hierarchy.
2. Open **Inspector → Surface textures → Standard maps**.
3. Click **Browse…** beside a map and select its image in the file dialog. Expand **Advanced maps** for the remaining slots.
4. Inspect the result in the 3D viewport. **Clear** removes one slot; **Remove direct textures** removes the object's direct texture component. Both operations support undo/redo.

Images outside the project's `assets` directory are copied into `assets/textures` with a content-derived suffix. Existing project assets keep their relative paths. Save the scene to persist maps, UV settings and Texture children. Keep the original files inside the project when moving or distributing it; Play snapshots and packaged games include `assets`.

## Drop files onto an object

Drag an image or a selection of PBR images from the project's Explorer onto the visible surface of the target object in the 3D viewport. You can also drag files from the operating system's file manager onto the surface, a scene hierarchy row, or an individual map slot. Project Explorer images can be dropped onto hierarchy rows and map slots too. A viewport drop targets the object under the pointer, regardless of the current selection. Empty space does not assign a map. Editing is disabled during Play and for read-only scenes.

Names containing `normal`, `rough`, `metal`, `occlusion`/`_ao`, `height`/`displacement`, `emiss`, `clearcoat`, `transmission`, `thickness`, `sheen`, `specular` or `anisotrop` select the matching map. Compound names such as `paint_clearcoat_roughness.png` select the advanced slot first. Unrecognized names use Base Color. Each slot holds one image; a later file for the same slot replaces the earlier one. Use Browse or drop onto a specific slot when a filename is ambiguous. Packed ORM images are not automatically split.

## Use a Texture child

Select a mesh or primitive and choose **+ Add → Texture (child of selected object)** in the scene hierarchy, or click **Add Texture child** in its Inspector. Select the new **Texture** child to browse its maps. A Texture child has no visible geometry. Drag it in the hierarchy onto another object, or onto another object's visible surface in the 3D viewport, to move its effect there.

A Texture child affects its immediate parent only. Its supplied slots override direct slots on that parent; empty slots leave the parent's slots intact. Maps do not apply recursively to siblings or nested meshes. With several Texture children, stable entity-ID order resolves duplicate slots, so use one child per object for predictable editing. The last nonempty child's UV and intensity settings apply to the combined material. Adding, editing, moving, copying and deleting children participates in the scene's undo history.

## Map slots

| Map | Input and effect |
| --- | --- |
| Base Color (Albedo) | RGB surface color, multiplied by the object's material color. Set the object color to white for an untinted image. |
| Normal | Tangent-space RGB normal, OpenGL/+Y convention. |
| Roughness | Red: black is smooth, white is rough. |
| Metallic | Red: black is dielectric, white is metallic. |
| Ambient Occlusion (AO) | Red: attenuates ambient illumination. |
| Height / Displacement | Red: vertex displacement centered at 0.5, scaled by Displacement. |
| Emissive | RGB emission multiplied by Emissive strength. |
| Clearcoat | Red: secondary glossy coating amount. |
| Clearcoat Roughness | Red: coating roughness. |
| Clearcoat Normal | RGB independent tangent-space coating normal. |
| Transmission | Red: thin-surface light transmission amount. |
| Thickness | Red: absorption thickness for transmitted light. |
| Sheen Color | RGB fabric-like grazing sheen. |
| Sheen Roughness | Red: sheen falloff. |
| Specular | Red: dielectric reflection intensity. |
| Specular Color | RGB dielectric reflection tint. |
| Anisotropy | Red/green: tangent-space direction encoded from -1..1 into 0..1; blue: strength. |

Color maps (albedo, emissive, sheen color, specular color) in PNG/JPEG/TGA decode from sRGB to linear. Scalar and normal maps remain raw data. HDR maps remain linear and preserve values above one in half-float GPU storage. HDR identifies a file's dynamic range; it does not turn an image into a complete PBR material.

**UV scale** controls repetition; **UV offset** moves the maps. All maps in a material share these settings. Imported meshes use their first UV set; primitives use generated UVs. Meshes without UVs cannot show a useful mapped surface.

## Limits and troubleshooting

- Source images are limited to 8192 × 8192 and bounded decode memory. GPU maps are resized to at most 1024 × 1024 per layer and use bilinear repeat sampling. Originals remain untouched. Mipmaps and texture streaming are not implemented.
- Height displaces existing vertices without tessellation. Use a sufficiently subdivided mesh for detail. Picking, colliders and selection outlines use the original geometry.
- Transmission and thickness approximate thin-surface transmitted lighting. They do not render refracted views of objects behind glass. Sheen, clearcoat and anisotropy are direct-light shading approximations. There is no image-based material reflection or automatic emissive scene lighting.
- Map edits request a new preview. To reload an image overwritten on disk, clear its slot, let the viewport refresh, and browse it again.
- If a map is missing, inspect the Inspector path and texture/preview diagnostic. Restore the file in the project or browse a replacement. Decode errors are reported before a new map is attached; renderer load failures appear in the preview diagnostic.
- If colors appear tinted, set the object's color to white. If normals look inverted, export an OpenGL/+Y normal map. For a featureless image, check the mesh's UVs and use a subdivided mesh for height maps.
- This workflow authors scene texture components. It does not add a scripting `Material.set_texture()` API, a material graph, or automatic extraction of embedded imported-model texture images.

## Agent tools

Agents can use `texture_inspect`, `texture_update` and `texture_create_child` for maps, or `environment_update` for an HDR sky. Invoke `rustic-textures` or call `texture_guide` for current instructions. See [Author HDR/PBR textures](AGENT_TEXTURES.md) for setup and complete examples.
