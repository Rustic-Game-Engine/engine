//! Agent operations execute on the editor thread, sharing its lock, document and undo history.
use super::{EditorApp, checked_project_path, import_model_asset};
use base64::Engine as _;
use eframe::egui;
use engine_assets::{DerivedArtifact, ImportRequest, ImporterRegistry};
use engine_core::EntityId;
use engine_editor::{AuthoringDocument, EditorCamera};
use engine_play::{ControlRequest, PlayMode};
use engine_project::VirtualDirectory;
use engine_scripting::GameSettings;
use engine_world::LocalTransform;
use glam::{Mat4, Vec3};
use renderer_wgpu::{
    BackendRequest, SceneViewportRenderer, ViewportMesh, ViewportScene, ViewportVertex,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key]
        .as_str()
        .ok_or_else(|| format!("missing string: {key}"))
}
fn decode<T: DeserializeOwned>(value: &Value) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|e| e.to_string())
}
fn entity_id(args: &Value) -> Result<EntityId, String> {
    required(args, "entity_id")?
        .parse()
        .map_err(|_| "invalid entity_id".into())
}
fn png(width: u32, height: u32, pixels: &[u8]) -> Result<String, String> {
    let image =
        image::RgbaImage::from_raw(width, height, pixels.to_vec()).ok_or("invalid image extent")?;
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes.into_inner()))
}

impl EditorApp {
    pub(super) fn poll_agent_tools(&mut self, context: &egui::Context) {
        context.request_repaint_after(Duration::from_millis(50));
        for _ in 0..4 {
            let Some(request) = self
                .agent_bridge
                .as_ref()
                .and_then(engine_scripting::AgentBridge::try_recv)
            else {
                break;
            };
            if request.deadline < Instant::now() {
                continue;
            }
            let result = self.agent_tool(&request.tool, &request.arguments, context);
            let _ = request.reply.send(result);
        }
    }

    fn agent_tool(
        &mut self,
        tool: &str,
        args: &Value,
        context: &egui::Context,
    ) -> Result<Value, String> {
        let result = self.agent_tool_inner(tool, args, context);
        result.map_err(|e| format!("{tool}: {e}"))
    }

    #[allow(
        clippy::too_many_lines,
        reason = "dispatch the editor tool protocol through the existing document and runtime services"
    )]
    fn agent_tool_inner(
        &mut self,
        tool: &str,
        args: &Value,
        context: &egui::Context,
    ) -> Result<Value, String> {
        let inspecting = matches!(
            tool,
            "editor_status"
                | "scene_list"
                | "texture_inspect"
                | "viewport_capture"
                | "model_preview"
                | "playtest_control"
                | "playtest_keys"
        );
        if !inspecting && (self.play.is_some() || self.pending_runtime_changes.is_some()) {
            return Err(
                "Stop the playtest and resolve the runtime-change review before editing".into(),
            );
        }
        match tool {
            "editor_status" => {
                let objects = self.document.entity_snapshots().map_err(|e| e.to_string())?.iter().map(|s| json!({
                    "entity_id":s.id,"name":s.name,"folder":s.folder,"parent":s.parent,"translation":s.local_transform.translation.to_array(),
                    "rotation":s.local_transform.rotation.to_array(),"scale":s.local_transform.scale.to_array(),
                    "part_attributes":s.part_attributes,"camera":s.camera,"light":s.light,"scripts":s.scripts,
                    "mesh_asset":s.mesh.map(|m| m.asset),"material_asset":s.material.map(|m| m.asset),"primitive":s.primitive,
                    "textures":s.textures,"resolved_textures":self.document.world().resolved_textures(s.id)
                })).collect::<Vec<_>>();
                Ok(
                    json!({"scene":self.document.scene_path(),"environment":self.document.world().environment(),"modified":self.document.is_modified(),"objects":objects,
                    "global_scripts":self.game_settings.global_scripts,"scene_scripts":self.document.scene_startup_scripts(),
                    "playtest":self.play.as_ref().map(|p| format!("{:?}", p.state())),"runtime_review_pending":self.pending_runtime_changes.is_some(),
                    "logs":self.console.entries().iter().rev().take(30).map(|e| json!({"severity":format!("{:?}",e.severity),"message":e.message})).collect::<Vec<_>>() }),
                )
            }
            "environment_update" => {
                let result = edit_environment(&mut self.document, args)?;
                self.refresh_preview();
                self.agent_frame = None;
                Ok(result)
            }
            "texture_inspect" => texture_inspect(&self.document, entity_id(args)?),
            "texture_update" | "texture_create_child" => {
                let result =
                    edit_textures(&mut self.document, args, tool == "texture_create_child")?;
                self.refresh_preview();
                self.agent_frame = None;
                Ok(result)
            }
            "scene_list" => {
                let scenes = self
                    .document
                    .project()
                    .directory(VirtualDirectory::Scenes)
                    .map_err(|e| e.to_string())?;
                let mut paths = Vec::new();
                collect_scene_paths(&scenes, &scenes, &mut paths, 0)?;
                paths.sort();
                Ok(
                    json!({"scenes":paths,"active":self.document.scene_path().strip_prefix(&scenes).ok().map(slash_path),"start":self.document.project().start_scene_path().ok().and_then(|p| p.strip_prefix(&scenes).ok().map(slash_path))}),
                )
            }
            "scene_create" => {
                self.document
                    .create_scene(Path::new(required(args, "path")?), required(args, "name")?)
                    .map_err(|e| e.to_string())?;
                Ok(json!({"created":args["path"]}))
            }
            "scene_switch" => {
                let path = Path::new(required(args, "path")?);
                // Validate before invoking the common UI refresh path, which logs errors.
                AuthoringDocument::open_scene(self.document.project().clone(), path, false)
                    .map_err(|e| e.to_string())?;
                self.document.save().map_err(|e| e.to_string())?;
                self.change_scene(path, context);
                if !self.document.scene_path().ends_with(path) {
                    return Err("scene switch failed; inspect editor logs".into());
                }
                self.agent_frame = None;
                Ok(json!({"active":self.document.scene_path()}))
            }
            "scene_set_start" => {
                self.document
                    .set_start_scene(Path::new(required(args, "path")?))
                    .map_err(|e| e.to_string())?;
                Ok(json!({"start":args["path"]}))
            }
            "scene_save" => {
                self.agent_save()?;
                Ok(json!({"saved":self.document.scene_path()}))
            }
            "object_update" => {
                let id = entity_id(args)?;
                let before = self
                    .document
                    .world()
                    .snapshot(id)
                    .map_err(|e| e.to_string())?;
                let properties = args["properties"]
                    .as_object()
                    .ok_or("properties must be an object")?;
                let mut edits = Vec::new();
                let mut transform = before.local_transform;
                for (key, value) in properties {
                    match key.as_str() {
                        "name" => edits.push(engine_world::SceneEdit::Name {
                            entity: id,
                            before: before.name.clone(),
                            after: Some(decode(value)?),
                        }),
                        "translation" => transform.translation = Vec3::from_array(decode(value)?),
                        "rotation" => transform.rotation = glam::Quat::from_array(decode(value)?),
                        "scale" => transform.scale = Vec3::from_array(decode(value)?),
                        "parent" => edits.push(engine_world::SceneEdit::Reparent {
                            entity: id,
                            before: before.parent,
                            after: decode(value)?,
                        }),
                        "part_attributes" => {
                            let mut merged = serde_json::to_value(before.part_attributes)
                                .map_err(|e| e.to_string())?;
                            for (k, v) in value
                                .as_object()
                                .ok_or("part_attributes must be an object")?
                            {
                                if !matches!(
                                    k.as_str(),
                                    "color" | "anchored" | "can_touch" | "can_collide" | "friction"
                                ) {
                                    return Err(format!("unknown part attribute: {k}"));
                                }
                                merged[k] = v.clone();
                            }
                            let after: engine_world::PartAttributes = decode(&merged)?;
                            if after
                                .color
                                .iter()
                                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                            {
                                return Err("color channels must be 0..1".into());
                            }
                            edits.push(engine_world::SceneEdit::PartAttributes {
                                entity: id,
                                before: before.part_attributes,
                                after,
                            });
                        }
                        "camera" => edits.push(engine_world::SceneEdit::Camera {
                            entity: id,
                            before: before.camera,
                            after: decode(value)?,
                        }),
                        "light" => edits.push(engine_world::SceneEdit::Light {
                            entity: id,
                            before: before.light,
                            after: decode(value)?,
                        }),
                        "scripts" => edits.push(engine_world::SceneEdit::Scripts {
                            entity: id,
                            before: before.scripts.clone(),
                            after: decode(value)?,
                        }),
                        _ => return Err(format!("unsupported object property: {key}")),
                    }
                }
                if transform != before.local_transform {
                    edits.push(engine_world::SceneEdit::Transform {
                        entity: id,
                        before: before.local_transform,
                        after: transform,
                    });
                }
                self.document
                    .edit_transaction("Agent object properties", edits)
                    .map_err(|e| e.to_string())?;
                self.refresh_preview();
                self.agent_frame = None;
                Ok(json!({"updated":id,"saved":false}))
            }
            "object_create" => {
                let name = required(args, "name")?;
                let kind = args["kind"].as_str().unwrap_or("empty");
                let mut object = engine_world::EntitySnapshot {
                    name: Some(name.into()),
                    parent: args
                        .get("parent")
                        .map(decode::<Option<EntityId>>)
                        .transpose()?
                        .flatten(),
                    ..Default::default()
                };
                match kind {
                    "empty" => {}
                    "folder" => object.folder = true,
                    "camera" => object.camera = Some(engine_world::Camera::default()),
                    "light" => object.light = Some(engine_world::Light::default()),
                    "blockout" => {
                        object.primitive = Some(engine_world::Primitive::Cube { size: 1.0 });
                    }
                    _ => return Err("kind must be empty, folder, camera, light or blockout".into()),
                }
                let id = object.id;
                self.document
                    .edit_transaction(
                        "Agent create object",
                        vec![engine_world::SceneEdit::Create { snapshot: object }],
                    )
                    .map_err(|e| e.to_string())?;
                self.refresh_preview();
                self.agent_frame = None;
                Ok(json!({"entity_id":id,"saved":false}))
            }
            "object_duplicate" => {
                let source = entity_id(args)?;
                let mut snapshots = self
                    .document
                    .snapshot_tree(source)
                    .map_err(|e| e.to_string())?;
                let original = snapshots.first_mut().ok_or("object subtree is empty")?;
                let parent = args
                    .get("parent")
                    .map(decode::<Option<EntityId>>)
                    .transpose()?
                    .unwrap_or(original.parent);
                if let Some(name) = args.get("name") {
                    original.name = Some(decode(name)?);
                }
                let id = self
                    .document
                    .paste_tree(&snapshots, parent)
                    .map_err(|e| e.to_string())?
                    .ok_or("object subtree is empty")?;
                let ids: Vec<_> = self
                    .document
                    .snapshot_tree(id)
                    .map_err(|e| e.to_string())?
                    .iter()
                    .map(|s| s.id)
                    .collect();
                self.refresh_preview();
                Ok(json!({"entity_id":id,"entity_ids":ids,"source_entity_id":source,"saved":false}))
            }
            "object_delete" => {
                let id = entity_id(args)?;
                let ids: Vec<_> = self
                    .document
                    .snapshot_tree(id)
                    .map_err(|e| e.to_string())?
                    .iter()
                    .map(|s| s.id)
                    .collect();
                self.document.delete_tree(id).map_err(|e| e.to_string())?;
                self.refresh_preview();
                Ok(json!({"deleted_entity_ids":ids,"saved":false}))
            }
            "script_attach" => self.agent_script(args),
            "model_create" => {
                let root = self.document.project().root().to_path_buf();
                let relative = Path::new(required(args, "path")?);
                let name = required(args, "name")?;
                let path = agent_path(&root, relative, args.get("source").is_some())?;
                if self.document.is_read_only() {
                    return Err("document is read-only".into());
                }
                if let Some(source) = args["source"].as_str() {
                    if relative.extension().and_then(|e| e.to_str()) != Some("obj") {
                        return Err("mesh scripts must save an .obj asset".into());
                    }
                    if path.exists() {
                        return Err("choose a new model path; asset already exists".into());
                    }
                    let obj = engine_scripting::build_agent_model(source)?;
                    engine_platform::AtomicFileService::new()
                        .write(&path, obj.as_bytes())
                        .map_err(|e| e.to_string())?;
                }
                let (id, model) = import_model_asset(&root, relative)?;
                self.model_assets.insert(id, model);
                let entity = self
                    .document
                    .add_model(name, id, None, LocalTransform::IDENTITY)
                    .map_err(|e| e.to_string())?;
                self.refresh_preview();
                self.agent_frame = None;
                Ok(json!({"entity_id":entity,"asset_id":id,"path":relative,"saved":false}))
            }
            "model_preview" => self.agent_model_preview(args),
            "playtest_control" => {
                match required(args, "action")? {
                    "start" => {
                        if self.play.is_some() {
                            return Err("playtest is already active".into());
                        }
                        if self.pending_runtime_changes.is_some() {
                            return Err("resolve the runtime-change review first".into());
                        }
                        self.agent_save()?;
                        self.start_play(PlayMode::Play);
                        if self.play.is_none() {
                            return Err("playtest did not start; inspect editor_status logs and missing runtime dependencies".into());
                        }
                        self.agent_frame = None;
                    }
                    "stop" => {
                        self.stop_play();
                        self.agent_frame = None;
                    }
                    action => {
                        let control = match action {
                            "pause" => ControlRequest::Pause,
                            "resume" => ControlRequest::Resume,
                            "step" => ControlRequest::FrameAdvance,
                            _ => return Err("invalid playtest action".into()),
                        };
                        self.play
                            .as_mut()
                            .ok_or("no active playtest")?
                            .control(control)?;
                    }
                }
                Ok(
                    json!({"state":self.play.as_ref().map(|p| format!("{:?}",p.state())),"runtime_review_pending":self.pending_runtime_changes.is_some()}),
                )
            }
            "playtest_keys" => {
                let keys: Vec<String> = decode(&args["keys"])?;
                let duration = args["duration_ms"].as_u64().unwrap_or(100);
                if keys.len() > 32
                    || !(1..=5000).contains(&duration)
                    || keys.iter().any(|k| !valid_key(k))
                {
                    return Err("use at most 32 physical key names and duration_ms 1..5000".into());
                }
                let play = self.play.as_mut().ok_or("no active playtest")?;
                if play.state() != engine_play::RuntimeState::Running {
                    return Err("resume the playtest before sending keys".into());
                }
                play.agent_keys(keys.into_iter().collect(), Duration::from_millis(duration))?;
                Ok(json!({"held_for_ms":duration,"auto_release":true}))
            }
            "viewport_capture" => {
                if self.play.is_none()
                    && (self.preview_dirty || self.preview_status == "Rendering active scene…")
                {
                    return Err(
                        "render pending; retry after the current scene frame is ready".into(),
                    );
                }
                let (width, height, pixels, sequence) = self.agent_frame.as_ref().ok_or(
                    "render pending; retry viewport_capture after the editor renders a frame",
                )?;
                if self.play.is_none() && *sequence != self.agent_frame_sequence {
                    return Err(
                        "render pending; the completed frame is from an earlier scene revision"
                            .into(),
                    );
                }
                Ok(
                    json!({"image_base64":png(*width,*height,pixels)?,"width":width,"height":height,"sequence":sequence,"scene":self.document.scene_path(),"playtest":self.play.is_some()}),
                )
            }
            "editor_undo" | "editor_redo" => {
                let changed = if tool == "editor_undo" {
                    self.document.undo()
                } else {
                    self.document.redo()
                }
                .map_err(|e| e.to_string())?;
                self.refresh_preview();
                self.agent_frame = None;
                Ok(json!({"changed":changed}))
            }
            _ => Err(format!("unknown editor tool: {tool}")),
        }
    }

    fn agent_save(&mut self) -> Result<(), String> {
        self.document.save().map_err(|e| e.to_string())?;
        self.scene_disk_snapshot = std::fs::read(self.document.scene_path()).ok();
        self.external_scene_change_pending = false;
        Ok(())
    }

    #[allow(
        clippy::too_many_lines,
        reason = "validate and route the three script attachment lifetimes in one operation"
    )]
    fn agent_script(&mut self, args: &Value) -> Result<Value, String> {
        let root = self.document.project().root().to_path_buf();
        if self.document.is_read_only() {
            return Err("document is read-only".into());
        }
        let path = Path::new(required(args, "path")?);
        checked_project_path(&root, path, false)?;
        agent_path(&root, Path::new("config/scripts.ron"), true)?;
        agent_path(&root, Path::new(engine_scripting::GAME_SETTINGS_FILE), true)?;
        let scope = required(args, "scope")?;
        if !matches!(scope, "global" | "scene" | "object") {
            return Err("scope must be global, scene or object".into());
        }
        let object = if scope == "object" {
            Some(
                self.document
                    .world()
                    .snapshot(entity_id(args)?)
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        let order = args
            .get("execution_order")
            .map(decode::<i32>)
            .transpose()?
            .unwrap_or(0);
        let enabled = args
            .get("enabled")
            .map(decode::<bool>)
            .transpose()?
            .unwrap_or(true);
        let properties = args
            .get("properties")
            .map(decode::<BTreeMap<String, engine_scripting::EngineValue>>)
            .transpose()?
            .unwrap_or_default();
        let id = engine_scripting::register_script_asset(&root, path).map_err(|e| e.to_string())?;
        let detach = args["detach"].as_bool().unwrap_or(false);
        let reference = engine_scripting::ScriptReference {
            asset_id: id,
            enabled,
            execution_order: order,
        };
        match scope {
            "global" => {
                let mut settings = if root.join(engine_scripting::GAME_SETTINGS_FILE).exists() {
                    GameSettings::load(&root)?
                } else {
                    GameSettings::default()
                };
                settings.global_scripts.retain(|r| r.asset_id != id);
                if !detach {
                    settings.global_scripts.push(reference);
                }
                settings.save(&root)?;
                self.game_settings = settings;
            }
            "scene" => {
                let mut scripts = self.document.scene_startup_scripts().to_vec();
                scripts.retain(|r| r.asset_id != id);
                if !detach {
                    scripts.push(reference);
                }
                self.document
                    .set_scene_startup_scripts(scripts)
                    .map_err(|e| e.to_string())?;
            }
            "object" => {
                let object = object.ok_or("missing object")?;
                let mut scripts = object.scripts;
                scripts.retain(|s| s.script_id != id);
                if !detach {
                    let mut script = engine_world::ScriptComponent::new(id);
                    script.enabled = enabled;
                    script.properties = properties;
                    script.validate()?;
                    scripts.push(script);
                }
                self.document
                    .set_scripts(object.id, scripts)
                    .map_err(|e| e.to_string())?;
            }
            _ => unreachable!(),
        }
        Ok(json!({"script_id":id,"scope":scope,"detached":detach,"saved":scope == "global"}))
    }

    #[allow(
        clippy::too_many_lines,
        reason = "import, frame and render an isolated model without mutating the authoring document"
    )]
    fn agent_model_preview(&self, args: &Value) -> Result<Value, String> {
        let root = self.document.project().root();
        let model = if let Some(source) = args["source"].as_str() {
            let obj = engine_scripting::build_agent_model(source)?;
            match ImporterRegistry::import(&ImportRequest::new("preview.obj", obj.into_bytes()))
                .map_err(|e| e.to_string())?
            {
                DerivedArtifact::Model(model) => model,
                _ => return Err("model importer returned no mesh".into()),
            }
        } else {
            let path = Path::new(required(args, "path")?);
            checked_project_path(root, path, false)?;
            {
                let source = std::fs::read(root.join(path)).map_err(|e| e.to_string())?;
                match ImporterRegistry::import(&ImportRequest::new(path, source))
                    .map_err(|e| e.to_string())?
                {
                    DerivedArtifact::Model(model) => model,
                    _ => return Err("path is not a supported model".into()),
                }
            }
        };
        let mut minimum = Vec3::splat(f32::INFINITY);
        let mut maximum = Vec3::splat(f32::NEG_INFINITY);
        for p in model.meshes.iter().flat_map(|m| &m.positions) {
            let v = Vec3::from_array(*p);
            minimum = minimum.min(v);
            maximum = maximum.max(v);
        }
        if !minimum.is_finite() || !maximum.is_finite() {
            return Err("model has no finite geometry".into());
        }
        let mut camera = EditorCamera::default();
        camera.frame_sphere(
            (minimum + maximum) * 0.5,
            (maximum - minimum).length() * 0.5,
        );
        if let Some(center) = args.get("center") {
            camera.focus = Vec3::from_array(decode(center)?);
        }
        if let Some(value) = args.get("yaw_degrees") {
            camera.yaw = decode::<f32>(value)?.to_radians();
        }
        if let Some(value) = args.get("pitch_degrees") {
            camera.pitch = decode::<f32>(value)?.clamp(-89.0, 89.0).to_radians();
        }
        if let Some(value) = args.get("distance") {
            camera.distance = decode(value)?;
        }
        if !camera.focus.is_finite()
            || !camera.yaw.is_finite()
            || !camera.pitch.is_finite()
            || !camera.distance.is_finite()
            || !(0.05..=100_000.0).contains(&camera.distance)
        {
            return Err("invalid model preview camera".into());
        }
        let meshes = model
            .meshes
            .iter()
            .enumerate()
            .map(|(i, m)| ViewportMesh {
                textures: engine_world::SurfaceTextures::default(),
                instance_key: i as u64,
                mesh_key: i as u64,
                vertices: m
                    .positions
                    .iter()
                    .enumerate()
                    .map(|(j, p)| ViewportVertex {
                        uv: m.texcoords.get(j).copied().unwrap_or([0.0; 2]),
                        position: *p,
                        normal: m.normals.get(j).copied().unwrap_or([0.0, 1.0, 0.0]),
                    })
                    .collect(),
                indices: m.indices.clone(),
                model: Mat4::IDENTITY.to_cols_array(),
                color: [0.65, 0.65, 0.65, 1.0],
                selected: false,
            })
            .collect();
        let scene = ViewportScene {
            meshes,
            view_projection: camera.view_projection(1.0).to_cols_array(),
            camera_position: camera.position().to_array(),
            environment: engine_world::SceneEnvironment::default(),
            lights: Vec::new(),
            guides: Vec::new(),
            grid_vertices: Vec::new(),
            clear_color: [0.045, 0.06, 0.085, 1.0],
        };
        let mut renderer =
            SceneViewportRenderer::new(BackendRequest::Auto).map_err(|e| e.to_string())?;
        renderer.set_environment_root(root.to_path_buf());
        let frame = renderer
            .render(512, 512, &scene)
            .map_err(|e| e.to_string())?
            .ok_or("model preview suspended")?;
        Ok(
            json!({"image_base64":png(frame.width,frame.height,&frame.rgba8)?,"center":camera.focus.to_array(),"distance":camera.distance,"yaw_degrees":camera.yaw.to_degrees(),"pitch_degrees":camera.pitch.to_degrees(),"placed":false}),
        )
    }
}

fn valid_key(key: &str) -> bool {
    matches!(
        key,
        "Space"
            | "Enter"
            | "Escape"
            | "Tab"
            | "Backspace"
            | "ArrowLeft"
            | "ArrowRight"
            | "ArrowUp"
            | "ArrowDown"
            | "ShiftLeft"
            | "ShiftRight"
            | "ControlLeft"
            | "ControlRight"
            | "AltLeft"
            | "AltRight"
    ) || key
        .strip_prefix("Key")
        .is_some_and(|v| v.len() == 1 && v.bytes().all(|c| c.is_ascii_uppercase()))
        || key
            .strip_prefix("Digit")
            .is_some_and(|v| v.len() == 1 && v.bytes().all(|c| c.is_ascii_digit()))
}
fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn collect_scene_paths(
    root: &Path,
    path: &Path,
    paths: &mut Vec<String>,
    depth: usize,
) -> Result<(), String> {
    if depth > 16 {
        return Err("scene folders exceed 16 levels".into());
    }
    for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            collect_scene_paths(root, &path, paths, depth + 1)?;
        } else if path
            .extension()
            .is_some_and(|e| e == "scene" || e == "rscene")
        {
            paths.push(slash_path(
                path.strip_prefix(root).map_err(|e| e.to_string())?,
            ));
        }
    }
    Ok(())
}

// Authoring destinations may be missing; all existing ancestors must be real project entries.
fn agent_path(root: &Path, relative: &Path, allow_missing: bool) -> Result<PathBuf, String> {
    let text = relative.to_string_lossy();
    if relative.as_os_str().is_empty()
        || text.contains(['\\', ':'])
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        || relative.components().any(|c| c.as_os_str() == ".git")
    {
        return Err("path must stay inside the project using slash-separated components".into());
    }
    let mut path = root.canonicalize().map_err(|e| e.to_string())?;
    for c in relative.components() {
        path.push(c);
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err("symbolic links are not supported".into());
            }
            Ok(_) => {}
            Err(e) if allow_missing && e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_accept_new_assets_but_reject_escapes() {
        let root = tempfile::tempdir().unwrap();
        for path in [
            "../evil.obj",
            "/tmp/evil.obj",
            "C:/evil.obj",
            "assets\\evil.obj",
            ".git/config",
        ] {
            assert!(
                agent_path(root.path(), Path::new(path), true).is_err(),
                "{path}"
            );
        }
        assert_eq!(
            agent_path(root.path(), Path::new("assets/models/new.obj"), true).unwrap(),
            root.path()
                .canonicalize()
                .unwrap()
                .join("assets/models/new.obj")
        );
        assert!(agent_path(root.path(), Path::new("missing.obj"), false).is_err());
    }
    #[test]
    fn accepts_runtime_physical_keys_only() {
        for key in ["KeyW", "Space", "Digit3", "ArrowLeft"] {
            assert!(valid_key(key));
        }
        for key in ["W", "KeyWW", "cmd.exe", "Key3", ""] {
            assert!(!valid_key(key));
        }
    }
    #[test]
    fn captures_real_pixels_as_png() {
        let encoded = png(1, 1, &[255, 0, 0, 255]).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        assert_eq!(
            image::load_from_memory(&bytes)
                .unwrap()
                .to_rgba8()
                .get_pixel(0, 0)
                .0,
            [255, 0, 0, 255]
        );
    }
}

#[cfg(test)]
mod authoring_flow_tests {
    use super::*;
    use engine_project::Project;
    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "exercise a complete agent authoring flow across models, scripts and scenes"
    )]
    fn direct_tools_author_models_properties_scopes_and_scenes() {
        let temp = tempfile::tempdir().unwrap();
        let project = Project::create(
            temp.path().join("game"),
            "Agent flow",
            engine_project::ProjectTemplate::Blank,
        )
        .unwrap();
        let context = egui::Context::default();
        let mut app = EditorApp::new(project, &context).unwrap();
        let object = app
            .agent_tool(
                "object_create",
                &json!({"name":"Player","kind":"empty"}),
                &context,
            )
            .unwrap();
        let id = object["entity_id"].clone();
        app.agent_tool("object_update",&json!({"entity_id":id,"properties":{"translation":[2,3,4],"part_attributes":{"anchored":true}}}),&context).unwrap();
        let object_id: EntityId = serde_json::from_value(id.clone()).unwrap();
        assert_eq!(
            app.document
                .world()
                .snapshot(object_id)
                .unwrap()
                .local_transform
                .translation,
            Vec3::new(2.0, 3.0, 4.0)
        );
        assert!(
            app.document
                .world()
                .snapshot(object_id)
                .unwrap()
                .part_attributes
                .anchored
        );
        assert!(
            app.agent_tool(
                "object_update",
                &json!({"entity_id":id,"properties":{"name":"Wrong","scale":[0,0,0]}}),
                &context
            )
            .is_err()
        );
        assert_eq!(
            app.document
                .world()
                .snapshot(object_id)
                .unwrap()
                .name
                .as_deref(),
            Some("Player")
        );
        let root = app.document.project().root().to_path_buf();
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::write(root.join("scripts/behavior.lua"), "return {}").unwrap();
        for scope in ["global", "scene", "object"] {
            app.agent_tool(
                "script_attach",
                &json!({"path":"scripts/behavior.lua","scope":scope,"entity_id":id}),
                &context,
            )
            .unwrap();
        }
        assert_eq!(app.game_settings.global_scripts.len(), 1);
        assert_eq!(app.document.scene_startup_scripts().len(), 1);
        assert_eq!(
            app.document
                .world()
                .snapshot(object_id)
                .unwrap()
                .scripts
                .len(),
            1
        );
        assert_eq!(GameSettings::load(&root).unwrap().global_scripts.len(), 1);
        app.agent_tool(
            "script_attach",
            &json!({"path":"scripts/behavior.lua","scope":"object","entity_id":id,"detach":true}),
            &context,
        )
        .unwrap();
        assert!(
            app.document
                .world()
                .snapshot(object_id)
                .unwrap()
                .scripts
                .is_empty()
        );
        assert_eq!(app.document.scene_startup_scripts().len(), 1);
        let mesh = app.agent_tool("model_create",&json!({"path":"assets/models/triangle.obj","name":"Mesh","source":"return {vertices={{0,0,0},{1,0,0},{0,1,0}},triangles={{1,2,3}}}"}),&context).unwrap();
        let mesh_id: EntityId = serde_json::from_value(mesh["entity_id"].clone()).unwrap();
        assert!(
            app.document
                .world()
                .snapshot(mesh_id)
                .unwrap()
                .mesh
                .is_some()
        );
        assert!(root.join("assets/models/triangle.obj").is_file());
        app.agent_tool("scene_save", &json!({}), &context).unwrap();
        let saved =
            engine_world::load_scene(&std::fs::read(app.document.scene_path()).unwrap()).unwrap();
        assert!(
            saved
                .document
                .entities
                .iter()
                .any(|e| e.id == mesh_id && e.mesh.is_some())
        );
        assert_eq!(saved.document.startup_scripts.len(), 1);
        app.agent_tool(
            "scene_create",
            &json!({"path":"levels/second.scene","name":"Second"}),
            &context,
        )
        .unwrap();
        let list = app.agent_tool("scene_list", &json!({}), &context).unwrap();
        assert!(
            list["scenes"]
                .as_array()
                .unwrap()
                .contains(&json!("levels/second.scene"))
        );
        app.agent_tool(
            "scene_switch",
            &json!({"path":"levels/second.scene"}),
            &context,
        )
        .unwrap();
        assert_eq!(app.document.entity_snapshots().unwrap().len(), 0);
        app.agent_tool(
            "scene_set_start",
            &json!({"path":"levels/second.scene"}),
            &context,
        )
        .unwrap();
        assert!(
            Project::open(&root)
                .unwrap()
                .start_scene_path()
                .unwrap()
                .ends_with("levels/second.scene")
        );
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TextureRequest {
    entity_id: EntityId,
    #[serde(default)]
    maps: BTreeMap<engine_world::TextureMap, Option<String>>,
    #[serde(default)]
    clear_maps: bool,
    uv_scale: Option<[f32; 2]>,
    uv_offset: Option<[f32; 2]>,
    height_scale: Option<f32>,
    emissive_strength: Option<f32>,
    name: Option<String>,
}
fn texture_inspect(document: &AuthoringDocument, id: EntityId) -> Result<Value, String> {
    let world = document.world();
    let direct = world.surface_textures(id).map_err(|e| e.to_string())?;
    let children: Vec<_> = world
        .children(id)
        .map_err(|e| e.to_string())?
        .iter()
        .filter_map(|child| {
            let snapshot = world.snapshot(*child).ok()?;
            (snapshot.textures.is_some() && snapshot.mesh.is_none() && snapshot.primitive.is_none())
                .then(
                    || json!({"entity_id":child,"name":snapshot.name,"textures":snapshot.textures}),
                )
        })
        .collect();
    Ok(
        json!({"entity_id":id,"textures":direct,"resolved_textures":world.resolved_textures(id),"texture_children":children}),
    )
}
fn edit_textures(
    document: &mut AuthoringDocument,
    args: &Value,
    child: bool,
) -> Result<Value, String> {
    if document.is_read_only() {
        return Err("document is read-only".into());
    }
    let request: TextureRequest = decode(args)?;
    if !child && request.name.is_some() {
        return Err("name is only supported by texture_create_child".into());
    }
    let before = document
        .world()
        .snapshot(request.entity_id)
        .map_err(|e| e.to_string())?;
    let renderable = before.mesh.is_some() || before.primitive.is_some();
    if !renderable && (child || before.textures.is_none()) {
        return Err("Choose a mesh or primitive, or update an existing Texture child".into());
    }
    let mut textures = if child {
        engine_world::SurfaceTextures::default()
    } else {
        before.textures.clone().unwrap_or_default()
    };
    if request.clear_maps {
        textures.maps.clear();
    }
    if let Some(v) = request.uv_scale {
        textures.uv_scale = v;
    }
    if let Some(v) = request.uv_offset {
        textures.uv_offset = v;
    }
    if let Some(v) = request.height_scale {
        textures.height_scale = v;
    }
    if let Some(v) = request.emissive_strength {
        textures.emissive_strength = v;
    }
    // Reject unsafe paths and controls before importing any assets or committing edits.
    for (kind, path) in &request.maps {
        if let Some(path) = path {
            textures.maps.insert(*kind, path.clone());
        } else {
            textures.maps.remove(kind);
        }
    }
    textures.validate()?;
    let root = document.project().root().to_path_buf();
    for path in textures.maps.values() {
        agent_path(&root, Path::new(path), false)?;
    }
    for (kind, path) in request.maps {
        if let Some(path) = path {
            let source = agent_path(&root, Path::new(&path), false)?;
            textures
                .maps
                .insert(kind, super::textures::import_texture(&root, &source)?);
        }
    }
    let id = if child {
        let snapshot = engine_world::EntitySnapshot {
            parent: Some(request.entity_id),
            name: Some(request.name.unwrap_or_else(|| "Texture".into())),
            textures: Some(textures.clone()),
            ..Default::default()
        };
        let id = snapshot.id;
        document
            .edit_transaction(
                "Agent create Texture child",
                vec![engine_world::SceneEdit::Create { snapshot }],
            )
            .map_err(|e| e.to_string())?;
        document.select(Some(id));
        id
    } else {
        document
            .set_textures(request.entity_id, Some(textures.clone()))
            .map_err(|e| e.to_string())?;
        request.entity_id
    };
    Ok(json!({"entity_id":id,"textures":textures,"saved":false}))
}

fn edit_environment(document: &mut AuthoringDocument, args: &Value) -> Result<Value, String> {
    if document.is_read_only() {
        return Err("document is read-only".into());
    }
    let object = args.as_object().ok_or("arguments must be an object")?;
    if object.len() != 1 || !object.contains_key("settings") {
        return Err("provide only settings".into());
    }
    let settings = args["settings"]
        .as_object()
        .ok_or("settings must be an object")?;
    let before = document.world().environment().clone();
    let mut merged = serde_json::to_value(&before).map_err(|e| e.to_string())?;
    for (key, value) in settings {
        if merged.get(key).is_none() {
            return Err(format!("unknown environment field: {key}"));
        }
        merged[key] = value.clone();
    }
    let mut after: engine_world::SceneEnvironment = decode(&merged)?;
    if !after.is_valid() {
        return Err("invalid environment settings".into());
    }
    if settings.contains_key("sky_image") && !after.sky_image.is_empty() {
        let root = document.project().root();
        let source = agent_path(root, Path::new(&after.sky_image), false)?;
        let (width, height) = image::image_dimensions(&source).map_err(|e| e.to_string())?;
        if width != height.saturating_mul(2) {
            return Err("sky image must be a 2:1 equirectangular panorama".into());
        }
        after.sky_image = super::textures::import_texture(root, &source)?;
    }
    document
        .set_environment(after.clone())
        .map_err(|e| e.to_string())?;
    Ok(json!({"environment":after,"saved":false}))
}

#[cfg(test)]
mod texture_flow_tests {
    use super::*;
    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "exercise live texture tools, failure atomicity, persistence and edit guards together"
    )]
    fn live_texture_tools_validate_patch_create_undo_save_and_inspect() {
        let temp = tempfile::tempdir().unwrap();
        let project = engine_project::Project::create(
            temp.path().join("game"),
            "Texture tools",
            engine_project::ProjectTemplate::Blank,
        )
        .unwrap();
        let root = project.root().to_path_buf();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([128u8, 128, 255, 255]))
            .save(root.join("assets/normal.png"))
            .unwrap();
        image::codecs::hdr::HdrEncoder::new(
            std::fs::File::create(root.join("assets/sky.hdr")).unwrap(),
        )
        .encode(&[image::Rgb([4.0, 1.0, 0.0]); 2], 2, 1)
        .unwrap();
        std::fs::write(root.join("assets/broken.png"), b"not an image").unwrap();
        let context = egui::Context::default();
        let mut app = EditorApp::new(project.clone(), &context).unwrap();
        let created = app
            .agent_tool(
                "object_create",
                &json!({"name":"Mesh","kind":"blockout"}),
                &context,
            )
            .unwrap();
        let id = created["entity_id"].clone();
        app.agent_tool("texture_update",&json!({"entity_id":id,"maps":{"Normal":"assets/normal.png","Emissive":"assets/sky.hdr"}}),&context).unwrap();
        app.agent_tool(
            "texture_update",
            &json!({"entity_id":id,"maps":{"Normal":null},"uv_scale":[2,3]}),
            &context,
        )
        .unwrap();
        let before = app
            .agent_tool("texture_inspect", &json!({"entity_id":id}), &context)
            .unwrap();
        assert!(before["textures"]["maps"].get("Normal").is_none());
        assert_eq!(before["textures"]["maps"]["Emissive"], "assets/sky.hdr");
        assert_eq!(before["textures"]["uv_scale"], json!([2.0, 3.0]));
        // Mixed valid/invalid requests must leave all scene properties unchanged.
        for bad in [
            json!({"maps":{"BaseColor":"assets/normal.png","Normal":"assets/broken.png"}}),
            json!({"maps":{"BaseColor":"../outside.png"}}),
            json!({"maps":{"Unknown":"assets/normal.png"}}),
            json!({"emissive_strength":-1}),
            json!({"uv_scale":[1,2,3]}),
            json!({"unexpected":true}),
        ] {
            let mut args = bad;
            args["entity_id"] = id.clone();
            assert!(app.agent_tool("texture_update", &args, &context).is_err());
            assert_eq!(
                app.agent_tool("texture_inspect", &json!({"entity_id":id}), &context)
                    .unwrap(),
                before
            );
        }
        let child = app
            .agent_tool(
                "texture_create_child",
                &json!({"entity_id":id,"name":"Coating","maps":{"Normal":"assets/normal.png"}}),
                &context,
            )
            .unwrap();
        let child_id: EntityId = decode(&child["entity_id"]).unwrap();
        app.agent_tool("editor_undo", &json!({}), &context).unwrap();
        assert!(!app.document.world().contains(child_id));
        app.agent_tool("editor_redo", &json!({}), &context).unwrap();
        let inspected = app
            .agent_tool("texture_inspect", &json!({"entity_id":id}), &context)
            .unwrap();
        assert_eq!(
            inspected["texture_children"][0]["entity_id"],
            child["entity_id"]
        );
        assert_eq!(
            inspected["resolved_textures"]["maps"]["Normal"],
            "assets/normal.png"
        );
        app.agent_tool(
            "texture_update",
            &json!({"entity_id":child_id,"clear_maps":true}),
            &context,
        )
        .unwrap();
        assert!(
            app.document
                .world()
                .surface_textures(child_id)
                .unwrap()
                .unwrap()
                .maps
                .is_empty()
        );
        let status = app
            .agent_tool("editor_status", &json!({}), &context)
            .unwrap();
        assert!(
            status["objects"]
                .as_array()
                .unwrap()
                .iter()
                .any(|o| o["entity_id"] == id && o["textures"].is_object())
        );
        app.agent_tool(
            "environment_update",
            &json!({"settings":{"enabled":true,"sky_image":"assets/sky.hdr","exposure":1}}),
            &context,
        )
        .unwrap();
        assert_eq!(
            app.document.world().environment().sky_image,
            "assets/sky.hdr"
        );
        let environment = app.document.world().environment().clone();
        assert!(
            app.agent_tool(
                "environment_update",
                &json!({"settings":{"sky_image":"assets/broken.png","exposure":2}}),
                &context
            )
            .is_err()
        );
        assert_eq!(app.document.world().environment(), &environment);
        assert!(
            app.agent_tool(
                "environment_update",
                &json!({"settings":{"sky_image":"assets/normal.png"}}),
                &context
            )
            .is_err()
        );
        assert_eq!(app.document.world().environment(), &environment);
        app.agent_tool("editor_undo", &json!({}), &context).unwrap();
        assert_eq!(
            app.document.world().environment(),
            &engine_world::SceneEnvironment::default()
        );
        app.agent_tool("editor_redo", &json!({}), &context).unwrap();
        app.agent_tool("scene_save", &json!({}), &context).unwrap();
        let loaded =
            engine_world::load_scene(&std::fs::read(app.document.scene_path()).unwrap()).unwrap();
        assert_eq!(loaded.document.environment, environment);
        assert!(
            loaded
                .document
                .entities
                .iter()
                .any(|e| e.id == child_id && e.textures.is_some())
        );
        app.pending_runtime_changes = Some(engine_play::RuntimeChangeSet::new("test", Vec::new()));
        assert!(
            app.agent_tool(
                "texture_update",
                &json!({"entity_id":id,"clear_maps":true}),
                &context
            )
            .is_err()
        );
        assert!(
            app.agent_tool("texture_inspect", &json!({"entity_id":id}), &context)
                .is_ok()
        );
        app.pending_runtime_changes = None;
        app.document = AuthoringDocument::open(project, true).unwrap();
        for tool in [
            "texture_update",
            "texture_create_child",
            "environment_update",
        ] {
            assert!(
                app.agent_tool(tool, &json!({"entity_id":id,"settings":{}}), &context)
                    .is_err()
            );
        }
        assert!(
            app.agent_tool("texture_inspect", &json!({"entity_id":id}), &context)
                .is_ok()
        );
    }
}

#[cfg(test)]
mod scene_explorer_tests {
    use super::*;
    use engine_project::{Project, ProjectTemplate};

    fn setup() -> (tempfile::TempDir, EditorApp, egui::Context) {
        let temp = tempfile::tempdir().unwrap();
        let project =
            Project::create(temp.path().join("game"), "Explorer", ProjectTemplate::Blank).unwrap();
        let context = egui::Context::default();
        let app = EditorApp::new(project, &context).unwrap();
        (temp, app, context)
    }
    fn create(app: &mut EditorApp, context: &egui::Context, args: &Value) -> EntityId {
        decode(&app.agent_tool("object_create", args, context).unwrap()["entity_id"]).unwrap()
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "verify hierarchy organization, independent copied IDs, undo and saved scene data through actual agent tools"
    )]
    fn folders_duplicate_delete_undo_and_persist() {
        let (_temp, mut app, context) = setup();
        let world = create(&mut app, &context, &json!({"name":"World","kind":"folder"}));
        let buildings = create(
            &mut app,
            &context,
            &json!({"name":"Buildings","kind":"folder","parent":world}),
        );
        let child = create(
            &mut app,
            &context,
            &json!({"name":"Camera","kind":"camera","parent":buildings}),
        );
        app.agent_tool(
            "object_update",
            &json!({"entity_id":world,"properties":{"translation":[100,200,300]}}),
            &context,
        )
        .unwrap();
        let script = engine_world::ScriptComponent::new(engine_core::ScriptId::new());
        app.agent_tool(
            "object_update",
            &json!({"entity_id":child,"properties":{"translation":[1,2,3],"scripts":[script]}}),
            &context,
        )
        .unwrap();
        let status = app
            .agent_tool("editor_status", &json!({}), &context)
            .unwrap();
        assert_eq!(
            status["objects"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["entity_id"] == json!(buildings))
                .unwrap()["folder"],
            true
        );
        let result = app
            .agent_tool(
                "object_duplicate",
                &json!({"entity_id":buildings,"name":"Buildings copy"}),
                &context,
            )
            .unwrap();
        let copy: EntityId = decode(&result["entity_id"]).unwrap();
        let copied = app.document.snapshot_tree(copy).unwrap();
        assert_eq!(result["entity_ids"].as_array().unwrap().len(), 2);
        assert!(copied.iter().all(|s| s.id != child && s.id != buildings));
        assert_eq!(copied[0].parent, Some(world));
        assert!(copied[0].folder);
        assert_eq!(copied[0].name.as_deref(), Some("Buildings copy"));
        assert_eq!(copied[1].parent, Some(copy));
        assert_eq!(copied[1].scripts, vec![script]);
        assert!(copied[1].camera.is_some());
        assert_eq!(
            copied[1].local_transform.translation,
            Vec3::new(1.0, 2.0, 3.0)
        );
        assert_eq!(
            app.document
                .world()
                .snapshot(buildings)
                .unwrap()
                .name
                .as_deref(),
            Some("Buildings")
        );
        app.agent_tool("editor_undo", &json!({}), &context).unwrap();
        assert!(!app.document.world().contains(copy));
        assert!(app.document.world().contains(child));
        app.agent_tool("editor_redo", &json!({}), &context).unwrap();
        assert!(app.document.world().contains(copied[1].id));
        app.agent_tool(
            "object_update",
            &json!({"entity_id":copy,"properties":{"parent":null}}),
            &context,
        )
        .unwrap();
        let deleted = app
            .agent_tool("object_delete", &json!({"entity_id":world}), &context)
            .unwrap();
        assert_eq!(deleted["deleted_entity_ids"].as_array().unwrap().len(), 3);
        assert!(!app.document.world().contains(world));
        assert!(!app.document.world().contains(child));
        assert!(app.document.world().contains(copy));
        assert!(app.document.world().contains(copied[1].id));
        app.agent_tool("editor_undo", &json!({}), &context).unwrap();
        assert_eq!(
            app.document.world().snapshot(child).unwrap().parent,
            Some(buildings)
        );
        app.agent_tool("scene_save", &json!({}), &context).unwrap();
        let saved = engine_world::load_scene(&std::fs::read(app.document.scene_path()).unwrap())
            .unwrap()
            .document;
        assert_eq!(saved.entities.iter().filter(|s| s.folder).count(), 3);
        let saved_world = saved.create_world().unwrap();
        assert_eq!(
            saved_world
                .world_transform(child)
                .unwrap()
                .0
                .transform_point3(Vec3::ZERO),
            Vec3::new(1.0, 2.0, 3.0)
        );
        assert_eq!(
            saved.entities.iter().find(|s| s.id == copy).unwrap().parent,
            None
        );
        assert_eq!(
            saved
                .entities
                .iter()
                .find(|s| s.id == copied[1].id)
                .unwrap()
                .scripts
                .len(),
            1
        );
        // An explicit parent:null duplicates at scene root; omitting it uses the old parent.
        let root_copy = app
            .agent_tool(
                "object_duplicate",
                &json!({"entity_id":child,"parent":null}),
                &context,
            )
            .unwrap();
        let root_copy_id: EntityId = decode(&root_copy["entity_id"]).unwrap();
        assert_eq!(
            app.document.world().snapshot(root_copy_id).unwrap().parent,
            None
        );
    }

    #[test]
    fn bad_parents_cycles_missing_ids_and_pending_review_do_not_mutate_hierarchy() {
        let (_temp, mut app, context) = setup();
        let root = create(&mut app, &context, &json!({"name":"Root","kind":"folder"}));
        let child = create(&mut app, &context, &json!({"name":"Child","parent":root}));
        let missing = EntityId::new();
        let before = app.document.entity_snapshots().unwrap();
        for (tool, args) in [
            (
                "object_create",
                json!({"name":"Bad","kind":"folder","parent":missing}),
            ),
            (
                "object_duplicate",
                json!({"entity_id":root,"parent":missing}),
            ),
            ("object_duplicate", json!({"entity_id":missing})),
            ("object_delete", json!({"entity_id":missing})),
            (
                "object_update",
                json!({"entity_id":root,"properties":{"parent":child}}),
            ),
        ] {
            assert!(app.agent_tool(tool, &args, &context).is_err(), "{tool}");
            assert_eq!(app.document.entity_snapshots().unwrap(), before);
        }
        app.pending_runtime_changes = Some(engine_play::RuntimeChangeSet::new("test", Vec::new()));
        for (tool, args) in [
            ("object_create", json!({"name":"Blocked","kind":"folder"})),
            ("object_duplicate", json!({"entity_id":root})),
            ("object_delete", json!({"entity_id":root})),
        ] {
            assert!(app.agent_tool(tool, &args, &context).is_err());
            assert_eq!(app.document.entity_snapshots().unwrap(), before);
        }
    }
}
