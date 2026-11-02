# Scene Effects research and acceptance record

## Architecture verified before implementation

The scene model uses stable entity IDs, typed Bevy components, schema-specific component payloads, recoverable loading and a checksum-protected RON scene envelope. Inspector transactions operate on document commands and undo records, independently of widget state. Effect configuration follows that path; playback state belongs to an isolated runtime.

The existing renderer has PBR mesh/surface-map support, a retained native wgpu viewport and per-fragment tone mapping into an LDR target. It did not provide an HDR postprocessing graph or GPU particle simulation. New display-range postprocessing therefore cannot be described as HDR bloom. Existing assets use adjacent authoritative `.rmeta` files, stable asset IDs and a disposable derived cache; `.effect` imports extend that registry.

## Sources and lessons

- [Official Rustic documentation source](https://github.com/Rustic-Game-Engine/docs/blob/main/content/user/editor.md): editor/Play isolation, transform conventions (+Z camera direction), hierarchy, Inspector and attachment workflows. Read using GitHub API after the hosted documentation returned HTTP 403.
- [Official Godot particle properties](https://github.com/godotengine/godot-docs/blob/master/tutorials/3d/particles/properties.rst): distinguish emission enablement from capacity, lifetime and playback speed; cap visible particles, use normalized lifetime appearance and explicit one-shot emission. Read through GitHub API because the documentation site returned HTTP 403.
- [Unreal Niagara overview](https://dev.epicgames.com/documentation/en-us/unreal-engine/overview-of-niagara-effects-for-unreal-engine): requested official endpoint returned HTTP 403; no claim is made that the remote text was successfully read. The public EpicGames/UnrealEngine repository was unavailable to this credential (HTTP 404). No proprietary source was copied.
- Checked-in `ARCHITECTURE.md`, `SURFACE_TEXTURES.md`, `SCENE_ENVIRONMENT.md`, `SCRIPTING_EXPANSION_STATUS.md`, scene/world/render/undo source, asset importer/metadata source and the native editor implementation establish actual target-branch behavior.

Implementation uses the existing editor UI stack and material conventions. Specialized render extraction is shared with CPU simulation rather than forcing fog, overlays and ribbons through a single emitter backend.

## Acceptance audit

The user requested a production-quality implementation of every listed property, polished presets, all 26 effects, native editor tools, script controls, persistence, graphics verification and a Windows installer. Registration, serialization and configuration tests alone do not satisfy that whole specification.

Implemented modules and their exact defaults are listed in `SCENE_EFFECTS_REFERENCE.md`. Renderer boundaries, unsupported advanced capabilities and setup instructions are listed in `SCENE_EFFECTS.md`. The generated showcase scene is produced by `cargo run -p engine-project --example scene_effects_showcase -- <empty-destination>` through public scene/project APIs; it is not a disconnected renderer mockup.

Backend limitations to report in the final delivery include GPU particle simulation, runtime reflection capture, six-face cubemap layout, temporal volumetric history and motion-vector blur. These optional unsupported paths do not establish missing support for an unrelated implemented effect. The new reusable WaterSurface sampler supplies analytical displacement, normals and foam; a full fluid solver is not implied. Display-range bloom cannot satisfy a request for HDR bloom. Interactive editor workflows and native/device/browser visual coverage require their own evidence, and a Linux cargo test run cannot establish that a fresh Windows installer was produced.

The final delivery must distinguish supported effects, optional backend limitations and required acceptance gates that remain unverified. In particular, component registration and CPU tests do not establish visual quality, complete interactive editor behavior or a working Windows installer.
