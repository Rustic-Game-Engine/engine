//! Native property editing for the serialized, typed effects schema.
//!
//! Edits are decoded back into the typed component and validated by the world
//! before they enter the document's undo history.

use eframe::egui;
use num_traits::ToPrimitive as _;
use serde_json::Value;
use std::path::Path;

fn label(name: &str) -> String {
    let mut text = name.replace('_', " ");
    if let Some(first) = text.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    text
}

/// Render the fields in a typed effect's serde representation with native egui
/// widgets. The representation is transient; the scene stores the typed value.
pub(super) fn properties(
    ui: &mut egui::Ui,
    value: &mut Value,
    root: &Path,
    references: &[(String, String)],
) -> bool {
    edit_value(ui, "", value, root, references)
}

#[allow(
    clippy::too_many_lines,
    reason = "native widget dispatch for recursive typed property data"
)]
fn edit_value(
    ui: &mut egui::Ui,
    name: &str,
    value: &mut Value,
    root: &Path,
    references: &[(String, String)],
) -> bool {
    ui.push_id(name, |ui| {
        if let Value::Object(fields) = value
            && fields.contains_key("rate")
            && fields.contains_key("max_particles")
        {
            return egui::CollapsingHeader::new(label(name))
                .default_open(true)
                .show(ui, |ui| edit_particle_modules(ui, fields, root, references))
                .body_returned
                .unwrap_or(false);
        }
        if name == "color_parameter" {
            let before=value.clone();
            let mut mode=if value.is_null(){0}else if value.get("BetweenGradients").is_some(){2}else{1};
            egui::ComboBox::from_id_salt("gradient-mode").selected_text(["Legacy color gradient","Gradient","Random between gradients"][mode]).show_ui(ui,|ui|{for (index,label) in ["Legacy color gradient","Gradient","Random between gradients"].iter().enumerate(){ui.selectable_value(&mut mode,index,*label);}});
            let previous=if before.is_null(){0}else if before.get("BetweenGradients").is_some(){2}else{1};
            if mode!=previous {let gradient=serde_json::to_value(engine_world::effects::Gradient::default()).unwrap_or(Value::Null);*value=match mode {0=>Value::Null,1=>serde_json::json!({"Gradient":gradient}),_=>serde_json::json!({"BetweenGradients":{"min":gradient,"max":gradient}})};}
            if !value.is_null() { edit_value(ui,"Gradient values",value,root,references); }
            return before!=*value;
        }
        if name == "velocity_over_lifetime" && let Some(axes)=value.as_array_mut() {
            let mut changed=false;
            for (axis,curve) in ["X velocity","Y velocity","Z velocity"].iter().zip(axes) {changed|=edit_value(ui,axis,curve,root,references);}
            return changed;
        }
        if name == "parameters"
            && let Value::Object(parameters) = value
        {
            return edit_parameters(ui, parameters, root, references);
        }
        if matches!(name, "source" | "source_baseline" | "child_entities" | "surface_triangles" | "mesh_spawn_triangles") { return false; }
        if name == "end_position" {
            let mut enabled = !value.is_null();
            if ui.checkbox(&mut enabled, "Override end position").changed() { *value=if enabled {serde_json::json!([0.0,0.0,0.0])} else {Value::Null};return true; }
            if !enabled {return false;}
        }
        if matches!(name, "start_width" | "end_width" | "expansion_speed") {
            let mut enabled = !value.is_null();
            if ui.checkbox(&mut enabled, format!("Override {}", label(name))).changed() {
                *value = if enabled { Value::from(1.0) } else { Value::Null };
                return true;
            }
            if !enabled { return false; }
        }
        if name == "bounds" && value.is_object() && ui.button("Clear simulation bounds").clicked() {
            *value = Value::Null;
            return true;
        }
        if name == "bounds" && value.is_null() {
            if ui.button("Add simulation bounds").clicked() {
                *value = serde_json::json!({"center":[0.0,0.0,0.0],"half_extents":[1.0,1.0,1.0]});
                return true;
            }
            return false;
        }
        if is_reference(name) {
            let before = value.clone();
            ui.horizontal(|ui| {
                ui.label(label(name)).on_hover_text(field_hint(name));
                egui::ComboBox::from_id_salt("entity-reference")
                    .selected_text(
                        references
                            .iter()
                            .find(|(id, _)| value.as_str() == Some(id))
                            .map_or("None", |(_, name)| name.as_str()),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(value, Value::Null, "None");
                        for (id, name) in references {
                            ui.selectable_value(value, Value::String(id.clone()), name);
                        }
                    });
            });
            return before != *value;
        }
        if is_asset(name) && (value.is_null() || value.is_string()) {
            let before = value.clone();
            ui.horizontal(|ui| {
                ui.label(label(name)).on_hover_text(field_hint(name));
                ui.label(value.as_str().unwrap_or("None"));
                if ui.button("Choose…").clicked()
                    && let Some(path) = rfd::FileDialog::new().set_directory(root).pick_file()
                    && let (Ok(root), Ok(path)) = (root.canonicalize(), path.canonicalize())
                    && path.starts_with(root)
                    && let Ok(asset_id) = ensure_asset_meta(&path)
                    && let Ok(id) = serde_json::to_value(asset_id)
                {
                    *value = id;
                }
                if ui.button("Clear").clicked() {
                    *value = Value::Null;
                }
            });
            return before != *value;
        }
        match value {
            Value::Bool(value) => ui.checkbox(value, label(name)).changed(),
            Value::Number(value) => {
                ui.horizontal(|ui| {
                    ui.label(label(name)).on_hover_text(field_hint(name));
                    if let Some(mut number) = value.as_u64() {
                        let changed = ui
                            .add(egui::DragValue::new(&mut number).speed(1.0))
                            .changed();
                        if changed {
                            *value = number.into();
                        }
                        changed
                    } else if let Some(mut number) = value.as_i64() {
                        let changed = ui
                            .add(egui::DragValue::new(&mut number).speed(1.0))
                            .changed();
                        if changed {
                            *value = number.into();
                        }
                        changed
                    } else {
                        let mut number = value.as_f64().unwrap_or_default();
                        let changed = ui
                            .add(egui::DragValue::new(&mut number).speed(0.01))
                            .changed();
                        if changed && let Some(number) = serde_json::Number::from_f64(number) {
                            *value = number;
                        }
                        changed
                    }
                })
                .inner
            }
            Value::String(value) => {
                ui.horizontal(|ui| {
                    ui.label(label(name)).on_hover_text(field_hint(name));
                    let choices = if name == "quality" && matches!(value.as_str(),"Discrete"|"Swept") { &["Discrete","Swept"][..] } else { enum_choices(name) };
                    if choices.is_empty() {
                        ui.text_edit_singleline(value).changed()
                    } else {
                        let before = value.clone();
                        egui::ComboBox::from_id_salt("value")
                            .selected_text(label(value))
                            .show_ui(ui, |ui| {
                                for choice in choices {
                                    ui.selectable_value(value, (*choice).to_owned(), label(choice));
                                }
                            });
                        before != *value
                    }
                })
                .inner
            }
            Value::Array(values)
                if values.len() >= 2
                    && values.len() <= 4
                    && values.iter().all(Value::is_number) =>
            {
                let mut numbers: Vec<f32> = values
                    .iter()
                    .map(|v| v.as_f64().unwrap_or_default().to_f32().unwrap_or_default())
                    .collect();
                let changed = ui
                    .horizontal(|ui| {
                        ui.label(label(name)).on_hover_text(field_hint(name));
                        if is_color(name) && numbers.len() == 4 {
                            let mut color = [numbers[0], numbers[1], numbers[2], numbers[3]];
                            let changed =
                                ui.color_edit_button_rgba_unmultiplied(&mut color).changed();
                            numbers.copy_from_slice(&color);
                            changed
                        } else if is_color(name) && numbers.len() == 3 {
                            let mut color = [numbers[0], numbers[1], numbers[2]];
                            let changed = ui.color_edit_button_rgb(&mut color).changed();
                            numbers.copy_from_slice(&color);
                            changed
                        } else {
                            numbers.iter_mut().fold(false, |changed, number| {
                                ui.add(egui::DragValue::new(number).speed(0.01)).changed() | changed
                            })
                        }
                    })
                    .inner;
                if changed {
                    for (value, number) in values.iter_mut().zip(numbers) {
                        *value = if value.as_u64().is_some() {
                            Value::from(number.max(0.0).to_u64().unwrap_or_default())
                        } else {
                            Value::from(number)
                        };
                    }
                }
                changed
            }
            Value::Array(values) => ui
                .collapsing(format!("{} ({})", label(name), values.len()), |ui| {
                    let mut changed = false;
                    let mut remove = None;
                    let mut duplicate = None;
                    let mut reorder = None;
                    let values_len = values.len();
                    for (index, value) in values.iter_mut().enumerate() {
                        ui.push_id(index, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(format!(
                                    "{} {}",
                                    if name == "keys" { "Key" } else { "Item" },
                                    index + 1
                                ));
                                if ui.small_button("Duplicate").clicked() {
                                    duplicate = Some(index);
                                }
                                if index > 0 && ui.small_button("↑").clicked() {
                                    reorder = Some(index);
                                }
                                if ui
                                    .add_enabled(
                                        name != "keys" || values_len > 1,
                                        egui::Button::new("Remove").small(),
                                    )
                                    .clicked()
                                {
                                    remove = Some(index);
                                }
                            });
                            changed |= edit_value(
                                ui,
                                if name == "targets" {
                                    "target"
                                } else if name == "meshes" {
                                    "mesh"
                                } else {
                                    ""
                                },
                                value,
                                root,
                                references,
                            );
                        });
                    }
                    if let Some(index) = remove {
                        values.remove(index);
                        changed = true;
                    } else if let Some(index) = duplicate {
                        values.insert(index + 1, values[index].clone());
                        changed = true;
                    } else if let Some(index) = reorder {
                        values.swap(index - 1, index);
                        changed = true;
                    }
                    if matches!(name, "meshes" | "mesh_variants" | "weighted_meshes")
                        && ui.button("Add mesh asset…").clicked()
                        && let Some(path) = rfd::FileDialog::new().set_directory(root).pick_file()
                        && let (Ok(root), Ok(path)) = (root.canonicalize(), path.canonicalize())
                        && path.starts_with(root)
                        && let Ok(asset_id) = ensure_asset_meta(&path)
                        && let Ok(value) = serde_json::to_value(asset_id)
                    {
                        values.push(if name == "meshes" {value} else {serde_json::json!({"mesh":value,"weight":1.0,"material":null})});
                        changed = true;
                    }
                    if name == "sound_cues" && ui.button("Add audio cue…").clicked()
                        && let Some(path) = rfd::FileDialog::new().set_directory(root).add_filter("Audio", &["wav", "ogg", "mp3"]).pick_file()
                        && let (Ok(root), Ok(path)) = (root.canonicalize(), path.canonicalize())
                        && path.starts_with(root)
                        && let Ok(asset_id) = ensure_asset_meta(&path)
                    {
                        values.push(serde_json::json!({"asset":asset_id,"delay":0.0,"volume":1.0,"pitch":1.0,"spatial":false,"looping":false}));
                        changed = true;
                    }
                    if !matches!(name,"meshes"|"mesh_variants"|"weighted_meshes"|"sound_cues")
                        && ui.button("Add key / item").clicked()
                        && let Some(value) = values
                            .last()
                            .cloned()
                            .or_else(|| default_item(name, references))
                    {
                        values.push(value);
                        changed = true;
                    }
                    changed
                })
                .body_returned
                .unwrap_or(false),
            Value::Object(values) => {
                let edit = |ui: &mut egui::Ui, values: &mut serde_json::Map<String, Value>| {
                    let graph_changed = values
                        .get_mut("keys")
                        .and_then(Value::as_array_mut)
                        .is_some_and(|keys| edit_key_graph(ui, keys));
                    let particle_shape = values.contains_key("rate");
                    values
                        .iter_mut()
                        .fold(graph_changed, |changed, (name, value)| {
                            if name == "shape" {
                                let before = value.clone();
                                ui.horizontal(|ui| {
                                    ui.label("Shape");
                                    egui::ComboBox::from_id_salt("shape")
                                        .selected_text(value.as_str().unwrap_or_default())
                                        .show_ui(ui, |ui| {
                                            let choices: &[&str] = if particle_shape {
                                                &[
                                                    "Point",
                                                    "Sphere",
                                                    "Hemisphere",
                                                    "Box",
                                                    "Cone",
                                                    "Circle",
                                                    "Cylinder",
                                                    "MeshSurface",
                                                ]
                                            } else {
                                                &["Box", "Sphere", "Plane", "Cylinder"]
                                            };
                                            for choice in choices {
                                                ui.selectable_value(
                                                    value,
                                                    Value::String((*choice).into()),
                                                    *choice,
                                                );
                                            }
                                        });
                                });
                                (before != *value) | changed
                            } else if name == "source" {
                                changed
                            } else {
                                edit_value(ui, name, value, root, references) | changed
                            }
                        })
                };
                if name.is_empty() {
                    edit(ui, values)
                } else {
                    egui::CollapsingHeader::new(label(name))
                        .default_open(
                            name == "common"
                                || name == "kind"
                                || name.chars().next().is_some_and(char::is_uppercase),
                        )
                        .show(ui, |ui| edit(ui, values))
                        .body_returned
                        .unwrap_or(false)
                }
            }
            Value::Null => {
                ui.weak(format!("{}: none", label(name)));
                false
            }
        }
    })
    .inner
}

fn is_color(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("color") || name.contains("colour") || name.contains("tint")
}

fn is_asset(name: &str) -> bool {
    name.contains("texture")
        || name.contains("material")
        || name.contains("asset")
        || name.contains("cubemap")
        || name.ends_with("_map")
        || name.ends_with("_mask")
        || name.ends_with("_effect")
        || name == "mesh"
        || name == "sub_emitter"
        || name == "impact_effect"
        || name == "effect"
}

fn enum_choices(name: &str) -> &'static [&'static str] {
    match name {
        "shape" => &[
            "Point",
            "Sphere",
            "Hemisphere",
            "Box",
            "Cone",
            "Circle",
            "Cylinder",
            "MeshSurface",
        ],
        "stacking" => &["Composite", "Replace", "Maximum"],
        "quality" => &["Low", "Medium", "High"],
        "blend" | "blend_mode" => &["Alpha", "Additive", "Multiply"],
        "weather" => &["Rain", "Snow", "Hail", "Dust", "Sand", "Leaves"],
        "force_type" => &[
            "Directional",
            "Attraction",
            "Repulsion",
            "Vortex",
            "Turbulence",
        ],
        "collision_response" | "response" => &["Bounce", "Stick", "Kill"],
        "refresh" => &["Baked", "OnEnable", "Interval"],
        "sorting" => &["Distance", "OldestFirst", "YoungestFirst", "None"],
        "simulation_mode" => &["Cpu", "Auto", "Gpu"],
        "offscreen_behavior" => &["Simulate", "Pause", "Clear"],
        "collision_quality" => &["Discrete", "Swept"],
        "event" => &["Birth", "Collision", "Death", "Completed"],
        "action" => &["Play", "Pause", "Resume", "Stop", "Restart", "Reset"],
        "trigger" => &["Birth", "Collision", "Death"],
        "simulation_space" => &["Local", "World"],
        _ => &[],
    }
}

fn is_reference(name: &str) -> bool {
    name == "target" || name == "light" || name == "camera" || name.ends_with("_target")
}

impl crate::EditorViewer<'_> {
    #[allow(
        clippy::too_many_lines,
        reason = "keep component editing and validated commit together"
    )]
    pub(super) fn effect_inspector(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &engine_world::EntitySnapshot,
    ) {
        use engine_world::effects::{EffectComponent, EffectKind};
        ui.separator();
        ui.heading("Scene effects");
        let writable = !self.document.is_read_only() && !self.playing;
        ui.add_enabled_ui(writable, |ui| self.composite_timeline(ui, snapshot));
        let mut next = snapshot.effect.clone();
        ui.add_enabled_ui(writable, |ui| {
            ui.menu_button("+ Add / replace effect", |ui| {
                for kind in EffectKind::all_defaults() {
                    if ui.button(kind.name()).clicked() {
                        next = Some(EffectComponent::new(kind));
                        ui.close();
                    }
                }
            });
            if ui.button("Assign single-effect asset…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_directory(self.project_root)
                    .add_filter("Effect asset", &["effect"])
                    .pick_file()
            {
                match load_effect_asset(self.project_root, &path) {
                    Ok(asset) if asset.effects.len() == 1 => {
                        next = asset.effects.into_iter().next();
                    }
                    Ok(_) => self.effect_error(
                        "Composite assets must be added through the preset / asset browser".into(),
                    ),
                    Err(error) => self.effect_error(error),
                }
            }
            if let Some(effect) = &mut next {
                ui.strong(effect.kind.name());
                let handle_key = egui::Id::new(("effect-handle", snapshot.id));
                let mut handle = ui
                    .ctx()
                    .data_mut(|data| data.get_temp::<u8>(handle_key))
                    .unwrap_or_default();
                let old_handle = handle;
                egui::ComboBox::from_id_salt("effect-handle-choice")
                    .selected_text(effect_handle_name(handle))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut handle, 0, "Entity transform");
                        if effect_dimensions(&effect.kind).is_some()
                            || effect.common.bounds.is_some()
                        {
                            ui.selectable_value(&mut handle, 1, "Emission / volume size");
                        }
                        if matches!(&effect.kind, EffectKind::Beam(_)) {
                            ui.selectable_value(&mut handle, 2, "Beam start");
                            ui.selectable_value(&mut handle, 3, "Beam end");
                        }
                        if matches!(
                            &effect.kind,
                            EffectKind::Wind(_) | EffectKind::ForceField(_)
                        ) {
                            ui.selectable_value(&mut handle, 4, "Force / wind direction");
                        }
                    });
                if old_handle != handle {
                    self.gizmo_widget.reset();
                }
                ui.ctx()
                    .data_mut(|data| data.insert_temp(handle_key, handle));
                ui.small("Position, rotation and parenting use this entity's Transform.");
                let references: Vec<_> = self
                    .document
                    .world()
                    .entity_ids()
                    .filter_map(|id| {
                        self.document.world().snapshot(id).ok().map(|entity| {
                            (
                                id.to_string(),
                                entity.name.unwrap_or_else(|| id.to_string()),
                            )
                        })
                    })
                    .collect();
                if let EffectKind::ReflectionProbe(probe) = &mut effect.kind
                    && ui.button("Capture now").clicked()
                {
                    probe.refresh_revision = probe.refresh_revision.wrapping_add(1);
                }
                if let Some(source) = &effect.source {
                    let source_id = source.asset;
                    if let Some(path) = find_asset_path(self.project_root, source_id) {
                        ui.label(format!(
                            "Shared asset: {}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        ));
                        if ui.button("Restore asset defaults").clicked() {
                            let result = std::fs::read(&path)
                                .map_err(|e| e.to_string())
                                .and_then(|bytes| {
                                    engine_world::effects::EffectAsset::from_bytes(&bytes)
                                })
                                .and_then(|asset| {
                                    restore_instance_defaults(
                                        &asset,
                                        effect,
                                        snapshot.id,
                                        self.document,
                                    )
                                });
                            if let Err(error) = result {
                                self.effect_error(error);
                            }
                        }
                        if ui
                            .button("Update shared asset from this instance")
                            .clicked()
                        {
                            let result =
                                update_shared_asset(&path, effect, snapshot.id, self.document);
                            if let Err(error) = result {
                                self.effect_error(error);
                            } else {
                                self.request_preview = true;
                            }
                        }
                    } else {
                        ui.weak("Shared effect asset is missing; the saved override still works.");
                    }
                }
                if let Ok(mut value) = serde_json::to_value(&*effect)
                    && properties(ui, &mut value, self.project_root, &references)
                {
                    match serde_json::from_value::<EffectComponent>(value) {
                        Ok(edited) => *effect = edited,
                        Err(error) => self.console.push(crate::simple_console(
                            engine_core::logging::Severity::Error,
                            "effects.inspector",
                            error.to_string(),
                        )),
                    }
                }
            }
            if next.is_some() && ui.button("Remove effect").clicked() {
                next = None;
            }
        });
        if next != snapshot.effect {
            let load_meshes = next
                .as_ref()
                .is_some_and(|effect| missing_effect_meshes(effect, self.model_assets));
            if load_meshes {
                self.model_assets
                    .extend(crate::load_project_model_assets(self.project_root));
            }
            if let Some(effect) = &mut next {
                prepare_mesh_emission(effect, self.model_assets);
                if let engine_world::effects::EffectKind::FoliageScatter(scatter) = &mut effect.kind
                {
                    if let Some(target) = scatter.surface_target {
                        scatter.surface_triangles =
                            crate::pick_scene(self.document, self.model_assets)
                                .into_iter()
                                .filter(|mesh| mesh.entity == target)
                                .flat_map(|mesh| {
                                    mesh.indices
                                        .as_chunks::<3>()
                                        .0
                                        .iter()
                                        .filter_map(|face| {
                                            Some([
                                                *mesh.positions.get(face[0] as usize)?,
                                                *mesh.positions.get(face[1] as usize)?,
                                                *mesh.positions.get(face[2] as usize)?,
                                            ])
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .take(100_000)
                                .collect();
                    } else {
                        scatter.surface_triangles.clear();
                    }
                }
            }
            match self.document.set_effect(snapshot.id, next) {
                Ok(()) => self.request_preview = true,
                Err(error) => self.console.push(crate::simple_console(
                    engine_core::logging::Severity::Error,
                    "effects.inspector",
                    error.to_string(),
                )),
            }
        }
    }
}

fn default_item(name: &str, references: &[(String, String)]) -> Option<Value> {
    match name {
        "bursts" => serde_json::to_value(engine_world::effects::Burst::default()).ok(),
        "trigger_bindings" => {
            serde_json::to_value(engine_world::effects::GroupTriggerBinding::default()).ok()
        }
        "bindings" => {
            serde_json::to_value(engine_world::effects::GroupParameterBinding::default()).ok()
        }
        "lod" => serde_json::to_value(engine_world::effects::ScatterLod::default()).ok(),
        "mesh_variants" | "weighted_meshes" => {
            serde_json::to_value(engine_world::effects::WeightedParticleMesh::default()).ok()
        }
        "sub_emitters" => {
            serde_json::to_value(engine_world::effects::ParticleSubEmitter::default()).ok()
        }
        "points" => Some(serde_json::json!([0.0, 0.0, 0.0])),
        "keys" => Some(serde_json::json!({"time": 0.0, "value": 1.0})),
        "targets" => references.first().map(|(id, _)| Value::String(id.clone())),
        _ => None,
    }
}

#[derive(Clone)]
pub(super) struct PresetDrop(pub engine_world::effects::EffectAsset);

/// A separate scene uses exactly the runtime sub-emitter, collision and asset
/// expansion path without mutating the user's authored document.
#[derive(Default)]
pub(super) struct PreviewRuntime {
    world: engine_world::SceneWorld,
    authored: Vec<engine_world::EntitySnapshot>,
    initialized: bool,
}
impl PreviewRuntime {
    pub fn frame(&self) -> &engine_world::effects::EffectFrame {
        self.world.effect_frame()
    }
    pub fn particle_count(&self) -> usize {
        let counts: std::collections::BTreeMap<_, _> = self
            .frame()
            .gpu_emitters
            .iter()
            .map(|emitter| (emitter.entity, emitter.live_count as usize))
            .collect();
        self.frame().particles.len() + counts.values().sum::<usize>()
    }
    fn clear(&mut self) {
        *self = Self::default();
    }
    fn synchronize(&mut self, document: &engine_editor::AuthoringDocument) -> bool {
        let Ok(current) = document.entity_snapshots() else {
            return false;
        };
        if self.initialized && current == self.authored {
            return false;
        }
        let compatible = self.initialized
            && current.len() == self.authored.len()
            && current.iter().zip(&self.authored).all(|(after, before)| {
                if before
                    .effect
                    .as_ref()
                    .and_then(|effect| effect.source.as_ref())
                    != after
                        .effect
                        .as_ref()
                        .and_then(|effect| effect.source.as_ref())
                {
                    return false;
                }
                let mut before = before.clone();
                before.local_transform = after.local_transform;
                before.parent = after.parent;
                before.effect.clone_from(&after.effect);
                before.name.clone_from(&after.name);
                before == *after
            });
        if compatible {
            let mut edits = Vec::new();
            for (after, before) in current.iter().zip(&self.authored) {
                if after.local_transform != before.local_transform {
                    edits.push(engine_world::WorldCommand::SetLocalTransform {
                        entity: after.id,
                        value: after.local_transform,
                    });
                }
                if after.parent != before.parent {
                    edits.push(engine_world::WorldCommand::SetParent {
                        child: after.id,
                        parent: after.parent,
                    });
                }
                if after.effect != before.effect {
                    edits.push(engine_world::WorldCommand::SetEffect {
                        entity: after.id,
                        value: after.effect.clone().map(Box::new),
                    });
                }
            }
            if self.world.apply_commands(&edits).is_ok() {
                self.world.propagate_transforms();
                self.authored = current;
                return false;
            }
        }
        if let Ok(world) = document.effect_preview_world() {
            self.world = world;
            self.authored = current;
            self.initialized = true;
            return true;
        }
        false
    }
}

pub(super) struct EffectPreview {
    pub runtime: PreviewRuntime,
    pub playing: bool,
    pub speed: f32,
    pub time: f32,
    pub last_tick: std::time::Instant,
    pub show_bounds: bool,
    pub search: String,
    pub category: String,
    scene_path: std::path::PathBuf,
    pub simulation_millis: f32,
    audio: Option<engine_play::EffectAudioPreview>,
    audio_enabled: bool,
    audio_error: Option<String>,
}

impl Default for EffectPreview {
    fn default() -> Self {
        Self {
            runtime: PreviewRuntime::default(),
            playing: false,
            speed: 1.0,
            time: 0.0,
            last_tick: std::time::Instant::now(),
            show_bounds: true,
            search: String::new(),
            category: "All".into(),
            scene_path: std::path::PathBuf::new(),
            simulation_millis: 0.0,
            audio: None,
            audio_enabled: false,
            audio_error: None,
        }
    }
}

impl EffectPreview {
    pub fn submit_gpu_feedback(
        &self,
        context: Option<u64>,
        acks: &[(engine_world::EntityId, u64)],
    ) {
        self.runtime
            .world
            .submit_gpu_particle_feedback(context, acks);
    }
    pub fn update(&mut self, document: &engine_editor::AuthoringDocument, camera: glam::Vec3) {
        if self.scene_path != document.scene_path() {
            if let Some(audio) = &mut self.audio {
                audio.stop();
            }
            self.runtime.clear();
            self.time = 0.0;
            self.scene_path = document.scene_path().to_path_buf();
        }
        let real_delta = self.last_tick.elapsed().as_secs_f32().min(0.1);
        let delta = if self.playing {
            real_delta * self.speed
        } else {
            0.0
        };
        self.last_tick = std::time::Instant::now();
        self.advance(document, camera, delta);
        if self.audio_enabled {
            if self.audio.is_none() {
                match engine_play::EffectAudioPreview::new(document.project().root()) {
                    Ok(audio) => {
                        self.audio = Some(audio);
                        self.audio_error = None;
                    }
                    Err(error) => {
                        self.audio_error = Some(error);
                        self.audio_enabled = false;
                    }
                }
            }
            if let Some(audio) = &mut self.audio
                && let Err(error) = audio.step_scaled(
                    &mut self.runtime.world,
                    if self.playing {
                        f64::from(real_delta)
                    } else {
                        0.0
                    },
                    f64::from(self.speed),
                )
            {
                self.audio_error = Some(error);
            }
        } else {
            self.runtime.world.take_effect_audio_events();
            if let Some(audio) = &mut self.audio {
                audio.stop();
            }
        }
    }
    fn advance(
        &mut self,
        document: &engine_editor::AuthoringDocument,
        camera: glam::Vec3,
        delta: f32,
    ) {
        let start = std::time::Instant::now();
        if self.runtime.synchronize(document) {
            if let Some(audio) = &mut self.audio {
                audio.stop();
            }
            self.time = 0.0;
            if delta == 0.0 && !self.playing {
                let roots: Vec<_> = self
                    .runtime
                    .world
                    .entity_ids()
                    .filter(|id| self.runtime.world.parent(*id).ok().flatten().is_none())
                    .collect();
                for root in roots {
                    let _ = self
                        .runtime
                        .world
                        .control_effect(root, engine_world::effects::PlaybackCommand::Pause);
                }
            }
        }
        let mut remaining = delta;
        while remaining > 0.0 {
            let step = remaining.min(1.0 / 60.0);
            self.runtime
                .world
                .update_effects_from_camera(step, camera, None);
            remaining -= step;
        }
        self.runtime
            .world
            .update_effects_from_camera(0.0, camera, None);
        self.time += delta;
        self.simulation_millis = start.elapsed().as_secs_f32() * 1_000.0;
    }
    fn control(
        &mut self,
        document: &engine_editor::AuthoringDocument,
        command: engine_world::effects::PlaybackCommand,
    ) {
        if matches!(
            command,
            engine_world::effects::PlaybackCommand::Restart
                | engine_world::effects::PlaybackCommand::Reset
                | engine_world::effects::PlaybackCommand::Stop
        ) {
            if let Some(audio) = &mut self.audio {
                audio.stop();
            }
            self.runtime.clear();
        }
        if self.runtime.synchronize(document) {
            self.time = 0.0;
        }
        let roots: Vec<_> = self
            .runtime
            .world
            .entity_ids()
            .filter(|id| self.runtime.world.parent(*id).ok().flatten().is_none())
            .collect();
        for entity in roots {
            let command = if command == engine_world::effects::PlaybackCommand::Resume
                && self.runtime.world.effect_state(entity).ok().flatten()
                    == Some(engine_world::effects::PlaybackState::Stopped)
            {
                engine_world::effects::PlaybackCommand::Play
            } else {
                command
            };
            let _ = self.runtime.world.control_effect(entity, command);
        }
        self.last_tick = std::time::Instant::now();
    }
    fn scrub(
        &mut self,
        document: &engine_editor::AuthoringDocument,
        camera: glam::Vec3,
        seconds: f32,
    ) {
        if let Some(audio) = &mut self.audio {
            audio.stop();
        }
        self.runtime.clear();
        self.time = 0.0;
        self.control(document, engine_world::effects::PlaybackCommand::Play);
        self.advance(document, camera, seconds);
        self.control(document, engine_world::effects::PlaybackCommand::Pause);
        self.playing = false;
        self.runtime.world.take_effect_audio_events();
    }
    pub fn controls(
        &mut self,
        ui: &mut egui::Ui,
        document: &engine_editor::AuthoringDocument,
        camera: glam::Vec3,
    ) -> bool {
        use engine_world::effects::PlaybackCommand;
        let mut changed = false;
        ui.collapsing("Effect preview", |ui| {
            ui.small("Preview has separate simulation state. Scene data is unchanged.");
            ui.horizontal_wrapped(|ui| {
                for (label, command) in [
                    ("Play", PlaybackCommand::Play),
                    ("Pause", PlaybackCommand::Pause),
                    ("Resume", PlaybackCommand::Resume),
                    ("Stop", PlaybackCommand::Stop),
                    ("Restart", PlaybackCommand::Restart),
                    ("Reset", PlaybackCommand::Reset),
                ] {
                    if ui.button(label).clicked() {
                        self.control(document, command);
                        self.playing = matches!(
                            command,
                            PlaybackCommand::Play
                                | PlaybackCommand::Resume
                                | PlaybackCommand::Restart
                        );
                        if matches!(
                            command,
                            PlaybackCommand::Stop
                                | PlaybackCommand::Restart
                                | PlaybackCommand::Reset
                        ) {
                            self.time = 0.0;
                        }
                        changed = true;
                    }
                }
                if ui.button("Step 1/60 s").clicked() {
                    self.control(document, PlaybackCommand::Resume);
                    self.advance(document, camera, 1.0 / 60.0);
                    self.control(document, PlaybackCommand::Pause);
                    self.playing = false;
                    changed = true;
                }
            });
            if ui
                .checkbox(&mut self.audio_enabled, "Preview audio")
                .changed()
            {
                self.runtime.world.take_effect_audio_events();
                if let Some(audio) = &mut self.audio {
                    audio.stop();
                }
            }
            if let Some(error) = &self.audio_error {
                ui.label(error);
            }
            ui.add(egui::Slider::new(&mut self.speed, 0.0..=4.0).text("Preview speed"));
            let mut time = self.time;
            if ui
                .add(egui::Slider::new(&mut time, 0.0..=30.0).text("Time (seconds)"))
                .changed()
            {
                self.scrub(document, camera, time);
                changed = true;
            }
            changed |= ui
                .checkbox(&mut self.show_bounds, "Show effect bounds")
                .changed();
            if self.runtime.frame().particle_budget_exhausted {
                ui.label("Particle birth budget reached (100,000 live particles).");
            }
            ui.label(format!(
                "{} particles · {:.2} ms CPU simulation",
                self.runtime.particle_count(),
                self.simulation_millis
            ));
        });
        changed
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "shape guides share the selected effect transform and bound geometry contract"
)]
pub(super) fn append_bounds(
    scene: &mut renderer_wgpu::ViewportScene,
    document: &engine_editor::AuthoringDocument,
) {
    use engine_world::effects::EffectKind;
    use glam::Vec3;
    let Some(entity) = document.selected() else {
        return;
    };
    let Ok(snapshot) = document.world().snapshot(entity) else {
        return;
    };
    let Some(effect) = snapshot.effect else {
        return;
    };
    let Ok(transform) = document.world().world_transform(entity) else {
        return;
    };
    let bounds_center = effect
        .common
        .bounds
        .as_ref()
        .map_or(Vec3::ZERO, |bounds| Vec3::from(bounds.center));
    let dimensions = if let Some(bounds) = &effect.common.bounds {
        (Vec3::from(bounds.half_extents) * 2.0).to_array()
    } else {
        match &effect.kind {
            EffectKind::Particle(p) | EffectKind::MeshParticle(p) => p.dimensions,
            EffectKind::ForceField(v) | EffectKind::Wind(v) => v.dimensions,
            EffectKind::CollisionVolume(v) => v.dimensions,
            EffectKind::Fog(v) => v.dimensions,
            EffectKind::Decal(v) => v.dimensions,
            EffectKind::Weather(v) => v.emitter.dimensions,
            EffectKind::FoliageScatter(v) => v.dimensions,
            EffectKind::PostProcess(v) => v.dimensions,
            EffectKind::ReflectionProbe(v) => v.dimensions,
            EffectKind::HeatHaze(v) => [v.radius * 2.0; 3],
            EffectKind::Beam(v) => {
                scene.guides.push(renderer_wgpu::ViewportGuide {
                    key: crate::hash_value(&(entity, "effect-endpoints")),
                    vertices: vec![
                        v.start_target
                            .and_then(|id| document.world().world_transform(id).ok())
                            .map_or(transform.0, |t| t.0)
                            .transform_point3(Vec3::from(v.start))
                            .to_array(),
                        v.end_target
                            .and_then(|id| document.world().world_transform(id).ok())
                            .map_or(transform.0, |t| t.0)
                            .transform_point3(Vec3::from(v.end))
                            .to_array(),
                    ],
                    color: [0.18, 0.82, 1.0, 1.0],
                });
                return;
            }
            _ => return,
        }
    };
    let half = Vec3::from(dimensions) * 0.5;
    let shape = if effect.common.bounds.is_some() {
        "Box".to_owned()
    } else {
        match &effect.kind {
            EffectKind::Particle(value) | EffectKind::MeshParticle(value) => {
                format!("{:?}", value.shape)
            }
            EffectKind::Weather(value) => format!("{:?}", value.emitter.shape),
            EffectKind::ForceField(value) | EffectKind::Wind(value) => format!("{:?}", value.shape),
            EffectKind::CollisionVolume(value) => format!("{:?}", value.shape),
            EffectKind::Fog(value) => format!("{:?}", value.shape),
            EffectKind::PostProcess(value) => format!("{:?}", value.shape),
            EffectKind::HeatHaze(_) => "Sphere".to_owned(),
            _ => "Box".to_owned(),
        }
    };
    let mut local = Vec::new();
    if matches!(
        shape.as_str(),
        "Sphere" | "Hemisphere" | "Circle" | "Cylinder" | "Cone"
    ) {
        for axis in 0..3 {
            if matches!(shape.as_str(), "Circle" | "Cylinder" | "Cone") && axis != 1 {
                continue;
            }
            for segment in 0_u16..48 {
                let point = |angle: f32, height: f32| {
                    let (sin, cos) = angle.sin_cos();
                    (match axis {
                        0 => Vec3::new(height, sin * half.y, cos * half.z),
                        1 => Vec3::new(sin * half.x, height, cos * half.z),
                        _ => Vec3::new(sin * half.x, cos * half.y, height),
                    }) + bounds_center
                };
                let a = f32::from(segment) / 48.0 * std::f32::consts::TAU;
                let b = f32::from(segment + 1) / 48.0 * std::f32::consts::TAU;
                if shape == "Cylinder" {
                    for height in [-half.y, half.y] {
                        local.extend([point(a, height), point(b, height)]);
                    }
                } else {
                    local.extend([
                        point(a, if shape == "Cone" { half.y } else { 0.0 }),
                        point(b, if shape == "Cone" { half.y } else { 0.0 }),
                    ]);
                }
            }
        }
        if matches!(shape.as_str(), "Cylinder" | "Cone") {
            for (x, z) in [(-half.x, 0.0), (half.x, 0.0), (0.0, -half.z), (0.0, half.z)] {
                local.extend([
                    bounds_center
                        + if shape == "Cone" {
                            Vec3::new(0.0, -half.y, 0.0)
                        } else {
                            Vec3::new(x, -half.y, z)
                        },
                    bounds_center + Vec3::new(x, half.y, z),
                ]);
            }
        }
    } else {
        let corners: Vec<_> = (0..8)
            .map(|i| {
                bounds_center
                    + Vec3::new(
                        if i & 1 == 0 { -half.x } else { half.x },
                        if i & 2 == 0 { -half.y } else { half.y },
                        if i & 4 == 0 { -half.z } else { half.z },
                    )
            })
            .collect();
        for index in 0..8 {
            for bit in [1, 2, 4] {
                if index & bit == 0 {
                    local.extend([corners[index], corners[index | bit]]);
                }
            }
        }
    }
    if let EffectKind::Wind(field) | EffectKind::ForceField(field) = &effect.kind {
        let direction = Vec3::from_array(field.direction).normalize_or_zero();
        let length = half.max_element().max(0.5);
        let end = bounds_center + direction * length;
        let side = if direction == Vec3::ZERO {
            Vec3::X
        } else {
            direction.any_orthonormal_vector()
        } * length
            * 0.15;
        local.extend([
            bounds_center,
            end,
            end,
            end - direction * length * 0.25 + side,
            end,
            end - direction * length * 0.25 - side,
        ]);
    }
    let vertices = local
        .into_iter()
        .map(|point| transform.0.transform_point3(point).to_array())
        .collect();
    scene.guides.push(renderer_wgpu::ViewportGuide {
        key: crate::hash_value(&(entity, "effect-bounds")),
        vertices,
        color: [0.18, 0.82, 1.0, 1.0],
    });
}

impl crate::EditorViewer<'_> {
    pub(super) fn effects_panel(&mut self, ui: &mut egui::Ui) {
        self.request_preview |=
            self.effect_preview
                .controls(ui, self.document, self.camera.position());
        ui.collapsing("Effects presets & assets", |ui| {
            ui.add_enabled_ui(!self.document.is_read_only() && !self.playing, |ui| {
                ui.text_edit_singleline(&mut self.effect_preview.search)
                    .on_hover_text("Search built-in effect presets");
                egui::ComboBox::from_id_salt("effects-category")
                    .selected_text(&self.effect_preview.category)
                    .show_ui(ui, |ui| {
                        for category in [
                            "All",
                            "Fire and Explosions",
                            "Magic and Energy",
                            "Vehicles",
                            "Environment",
                            "Character and Combat",
                        ] {
                            ui.selectable_value(
                                &mut self.effect_preview.category,
                                category.to_owned(),
                                category,
                            );
                        }
                    });
                let search = self.effect_preview.search.to_lowercase();
                let category = self.effect_preview.category.clone();
                let mut chosen = None;
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .id_salt("effects-presets")
                    .show(ui, |ui| {
                        for asset in engine_world::effects::presets().into_iter().filter(|a| {
                            a.name.to_lowercase().contains(&search)
                                && (category == "All" || preset_category(&a.name) == category)
                        }) {
                            let response = ui
                                .button(&asset.name)
                                .interact(egui::Sense::click_and_drag());
                            if response.drag_started() {
                                response.dnd_set_drag_payload(PresetDrop(asset.clone()));
                            }
                            if response.clicked() {
                                chosen = Some(asset);
                            }
                        }
                    });
                if ui.button("Load .effect asset…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_directory(self.project_root)
                        .add_filter("Effect asset", &["effect"])
                        .pick_file()
                {
                    match load_effect_asset(self.project_root, &path) {
                        Ok(asset) => chosen = Some(asset),
                        Err(error) => self.effect_error(error),
                    }
                }
                if let Some(asset) = chosen {
                    if asset
                        .effects
                        .iter()
                        .any(|effect| missing_effect_meshes(effect, self.model_assets))
                    {
                        self.model_assets
                            .extend(crate::load_project_model_assets(self.project_root));
                    }
                    match self
                        .document
                        .add_effect_asset(&asset, self.document.selected())
                    {
                        Ok(_) => {
                            self.model_assets
                                .extend(crate::load_project_model_assets(self.project_root));
                            self.request_preview = true;
                        }
                        Err(error) => self.effect_error(error.to_string()),
                    }
                }
                if ui.button("Save selection as .effect asset…").clicked() {
                    let result = self.save_effect_selection();
                    if let Err(error) = result {
                        self.effect_error(error);
                    }
                }
            });
        });
    }

    fn effect_error(&mut self, error: String) {
        self.console.push(crate::simple_console(
            engine_core::logging::Severity::Error,
            "effects.asset",
            error,
        ));
    }

    #[allow(
        clippy::too_many_lines,
        reason = "capture ordered component and group metadata before one atomic asset write"
    )]
    fn save_effect_selection(&mut self) -> Result<(), String> {
        let entity = self
            .document
            .selected()
            .ok_or("Select an effect or a composite group first")?;
        let snapshot = self
            .document
            .world()
            .snapshot(entity)
            .map_err(|e| e.to_string())?;
        let mut effects = Vec::new();
        let mut entity_ids = Vec::new();
        let mut transforms = Vec::new();
        let mut lights = Vec::new();
        let mut captured = Vec::new();
        let group = snapshot.effect_group.clone();
        if snapshot.effect.is_some() {
            captured.push(snapshot.clone());
        }
        if let Some(effect) = snapshot.effect {
            effects.push(effect);
            lights.push(snapshot.light);
            entity_ids.push(snapshot.id);
            transforms.push(glam::Mat4::IDENTITY.to_cols_array());
        }
        let ordered_children = group
            .as_ref()
            .map(|group| group.child_entities.clone())
            .filter(|children| !children.is_empty())
            .unwrap_or(
                self.document
                    .world()
                    .children(entity)
                    .map_err(|e| e.to_string())?
                    .to_vec(),
            );
        for child in &ordered_children {
            if !self.document.world().contains(*child) {
                continue;
            }
            let child = self
                .document
                .world()
                .snapshot(*child)
                .map_err(|e| e.to_string())?;
            if child.effect.is_some() {
                captured.push(child.clone());
            }
            if let Some(effect) = child.effect {
                effects.push(effect);
                lights.push(child.light);
                entity_ids.push(child.id);
                transforms.push(child.local_transform.matrix().to_cols_array());
            }
        }
        for effect in &mut effects {
            effect.source = None;
        }
        let mut asset = engine_world::effects::EffectAsset::new(
            snapshot.name.unwrap_or_else(|| "Effect".into()),
            effects,
        );
        asset.entity_ids = entity_ids;
        asset.transforms = transforms;
        asset.lights = lights;
        asset.capture_child_components(&captured)?;
        if let Some(group) = group {
            asset.group_settings = Some((&group).into());
            asset.sound_cues = group.sound_cues;
            asset.parameters = group.parameters;
            let remap: std::collections::BTreeMap<_, _> = group
                .child_entities
                .iter()
                .enumerate()
                .filter_map(|(old, id)| {
                    asset
                        .entity_ids
                        .iter()
                        .position(|candidate| candidate == id)
                        .map(|new| {
                            (
                                u32::try_from(old).unwrap_or_default(),
                                u32::try_from(new).unwrap_or_default(),
                            )
                        })
                })
                .collect();
            asset.bindings = group
                .bindings
                .into_iter()
                .filter_map(|mut binding| {
                    binding.child = *remap.get(&binding.child)?;
                    Some(binding)
                })
                .collect();
            asset.trigger_bindings = group
                .trigger_bindings
                .into_iter()
                .filter_map(|mut binding| {
                    binding.source_child = *remap.get(&binding.source_child)?;
                    binding.target_child = *remap.get(&binding.target_child)?;
                    Some(binding)
                })
                .collect();
        }
        let bytes = asset.to_bytes()?;
        if let Some(path) = rfd::FileDialog::new()
            .set_directory(self.project_root.join("assets"))
            .set_file_name(format!("{}.effect", asset.name))
            .add_filter("Effect asset", &["effect"])
            .save_file()
        {
            let parent = path
                .parent()
                .ok_or("Choose a folder inside the project")?
                .canonicalize()
                .map_err(|e| e.to_string())?;
            if !parent.starts_with(
                self.project_root
                    .canonicalize()
                    .map_err(|e| e.to_string())?,
            ) {
                return Err("Effect assets must be saved inside the project".into());
            }
            engine_platform::AtomicFileService::new()
                .write(&path, &bytes)
                .map_err(|e| e.to_string())?;
            ensure_effect_meta(&path)?;
        }
        Ok(())
    }
}

fn ensure_effect_meta(path: &Path) -> Result<engine_core::AssetId, String> {
    let sidecar = engine_assets::sidecar_path(path);
    let meta = if sidecar.is_file() {
        engine_assets::load_meta(&sidecar).map_err(|e| e.to_string())?
    } else {
        let meta = engine_assets::AssetMeta::new("rustic.effect", 1);
        engine_assets::save_meta(&sidecar, &meta).map_err(|e| e.to_string())?;
        meta
    };
    Ok(meta.asset_id)
}

pub(super) fn load_effect_asset(
    root: &Path,
    path: &Path,
) -> Result<engine_world::effects::EffectAsset, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    if !path.starts_with(&root) {
        return Err("Copy effect assets into the project before loading them".into());
    }
    let mut asset = engine_world::effects::EffectAsset::from_bytes(
        &std::fs::read(&path).map_err(|e| e.to_string())?,
    )?;
    let id = ensure_effect_meta(&path)?;
    asset.effects = asset.instantiate_from(id);
    Ok(asset)
}

fn asset_instance_map(
    asset: &engine_world::effects::EffectAsset,
    source: &engine_world::effects::EffectSource,
    entity: engine_world::EntityId,
    document: &engine_editor::AuthoringDocument,
) -> std::collections::BTreeMap<engine_world::EntityId, engine_world::EntityId> {
    let mut mapping = std::collections::BTreeMap::new();
    let parent = document.world().parent(entity).ok().flatten();
    for candidate in document.world().entity_ids() {
        if document.world().parent(candidate).ok().flatten() != parent {
            continue;
        }
        if let Ok(Some(effect)) = document.world().effect(candidate)
            && let Some(other) = effect.source
            && other.asset == source.asset
            && let Ok(index) = usize::try_from(other.child)
            && let Some(original) = asset.entity_ids.get(index)
        {
            mapping.insert(*original, candidate);
        }
    }
    if let Ok(index) = usize::try_from(source.child)
        && let Some(original) = asset.entity_ids.get(index)
    {
        mapping.insert(*original, entity);
    }
    mapping
}

fn restore_instance_defaults(
    asset: &engine_world::effects::EffectAsset,
    effect: &mut engine_world::effects::EffectComponent,
    entity: engine_world::EntityId,
    document: &engine_editor::AuthoringDocument,
) -> Result<(), String> {
    let mapping = asset_instance_map(
        asset,
        effect
            .source
            .as_ref()
            .ok_or("This effect has no shared asset")?,
        entity,
        document,
    );
    asset.restore(effect)?;
    effect.remap_entities(&mapping);
    let baseline = engine_world::effects::EffectDefaults::from_component(effect);
    if let Some(source) = &mut effect.source {
        source.baseline = Some(Box::new(baseline));
    }
    Ok(())
}

fn update_shared_asset(
    path: &Path,
    effect: &engine_world::effects::EffectComponent,
    entity: engine_world::EntityId,
    document: &mut engine_editor::AuthoringDocument,
) -> Result<(), String> {
    let source = effect
        .source
        .as_ref()
        .ok_or("This effect has no shared asset")?;
    let mut asset = engine_world::effects::EffectAsset::from_bytes(
        &std::fs::read(path).map_err(|e| e.to_string())?,
    )?;
    let child_index = usize::try_from(source.child).map_err(|e| e.to_string())?;
    let old_defaults = asset
        .effects
        .get(child_index)
        .ok_or("Shared asset child no longer exists")?
        .clone();
    // Persist asset-local target identities, rather than references to the scene
    // instance from which the defaults were edited.
    let inverse = asset_instance_map(&asset, source, entity, document)
        .into_iter()
        .map(|(original, live)| (live, original))
        .collect();
    let mut new_defaults = effect.clone();
    new_defaults.source = None;
    new_defaults.remap_entities(&inverse);
    asset.effects[child_index] = new_defaults.clone();
    let mut edits = Vec::new();
    for entity in document.world().entity_ids() {
        if let Some(instance) = document.world().effect(entity).map_err(|e| e.to_string())?
            && let Some(other) = &instance.source
            && other.asset == source.asset
            && other.child == source.child
        {
            let mapping = asset_instance_map(&asset, other, entity, document);
            let mut live_defaults = new_defaults.clone();
            live_defaults.remap_entities(&mapping);
            let baseline = other.baseline.as_ref().map_or_else(
                || {
                    let mut old = old_defaults.clone();
                    old.remap_entities(&mapping);
                    old
                },
                |baseline| baseline.as_ref().clone().into_component(),
            );
            let mut merged = engine_world::effects::merge_instance_defaults(
                &instance,
                &baseline,
                &live_defaults,
            )?;
            if let Some(source) = &mut merged.source {
                source.baseline = Some(Box::new(
                    engine_world::effects::EffectDefaults::from_component(&live_defaults),
                ));
            }
            if merged != instance {
                edits.push(engine_world::SceneEdit::Effect {
                    entity,
                    before: Some(instance),
                    after: Some(merged),
                });
            }
        }
    }
    let bytes = asset.to_bytes()?;
    engine_platform::AtomicFileService::new()
        .write(path, &bytes)
        .map_err(|e| e.to_string())?;
    if !edits.is_empty() {
        document
            .edit_transaction("Refresh shared effect instances", edits)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn find_asset_path(root: &Path, id: engine_core::AssetId) -> Option<std::path::PathBuf> {
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() && !crate::is_hidden_project_directory(&entry.file_name()) {
                folders.push(path);
            } else if kind.is_file()
                && path.extension().is_some_and(|ext| ext == "effect")
                && engine_assets::load_meta(&engine_assets::sidecar_path(&path))
                    .is_ok_and(|meta| meta.asset_id == id)
            {
                return Some(path);
            }
        }
    }
    None
}

impl crate::EditorViewer<'_> {
    #[allow(
        clippy::too_many_lines,
        reason = "timeline controls share one undoable operation dispatch"
    )]
    fn composite_timeline(&mut self, ui: &mut egui::Ui, snapshot: &engine_world::EntitySnapshot) {
        if let Some(group) = &snapshot.effect_group {
            ui.collapsing("Composite playback, parameters and audio", |ui| {
                let references = self
                    .document
                    .entity_snapshots()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|entity| {
                        (
                            entity.id.to_string(),
                            entity.name.unwrap_or_else(|| entity.id.to_string()),
                        )
                    })
                    .collect::<Vec<_>>();
                let mut edited = group.clone();
                let mut value = serde_json::to_value(&edited).unwrap_or(Value::Null);
                if let Some(fields) = value.as_object_mut() {
                    fields.remove("bindings");
                    fields.remove("trigger_bindings");
                }
                let mut changed = properties(ui, &mut value, self.project_root, &references);
                if changed
                    && let Ok(mut decoded) =
                        serde_json::from_value::<engine_world::effects::EffectGroup>(value)
                {
                    decoded.bindings.clone_from(&edited.bindings);
                    decoded
                        .trigger_bindings
                        .clone_from(&edited.trigger_bindings);
                    edited = decoded;
                }
                let tracks = group
                    .child_entities
                    .iter()
                    .filter_map(|id| self.document.world().effect(*id).ok().flatten())
                    .collect::<Vec<_>>();
                changed |= edit_group_links(ui, &mut edited, &tracks);
                if changed {
                    match self.document.set_effect_group(snapshot.id, Some(edited)) {
                        Ok(()) => self.request_preview = true,
                        Err(error) => self.effect_error(error.to_string()),
                    }
                }
            });
        }
        let mut children: Vec<_> = self
            .document
            .world()
            .children(snapshot.id)
            .unwrap_or_default()
            .iter()
            .filter_map(|id| self.document.world().snapshot(*id).ok())
            .filter(|child| child.effect.is_some())
            .collect();
        if children.is_empty() {
            return;
        }
        children.sort_by(|a, b| {
            a.effect
                .as_ref()
                .unwrap()
                .common
                .delay
                .total_cmp(&b.effect.as_ref().unwrap().common.delay)
        });
        ui.collapsing("Composite timeline (seconds)", |ui| {
            let mut move_up = None;
            let mut duplicate = None;
            let mut delete = None;
            for (index, child) in children.iter().enumerate() {
                let mut effect = child.effect.clone().unwrap();
                let before = effect.clone();
                ui.push_id(child.id, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut effect.common.enabled, "");
                        if ui
                            .selectable_label(
                                false,
                                child.name.as_deref().unwrap_or(effect.kind.name()),
                            )
                            .clicked()
                        {
                            self.document.select(Some(child.id));
                        }
                        if ui.small_button("Duplicate").clicked() {
                            duplicate = Some(child.id);
                        }
                        if index > 0 && ui.small_button("Earlier").clicked() {
                            move_up = Some(index);
                        }
                        if ui.small_button("Delete").clicked() {
                            delete = Some(child.id);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut effect.common.delay)
                                .speed(0.01)
                                .range(0.0..=10000.0)
                                .prefix("Start ")
                                .suffix(" s"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut effect.common.duration)
                                .speed(0.01)
                                .range(0.01..=10000.0)
                                .prefix("Duration ")
                                .suffix(" s"),
                        );
                    });
                    let elapsed = ((self.effect_preview.time - effect.common.delay)
                        / effect.common.duration)
                        .clamp(0.0, 1.0);
                    ui.add(egui::ProgressBar::new(elapsed).text(format!(
                        "{:.2}–{:.2} s",
                        effect.common.delay,
                        effect.common.delay + effect.common.duration
                    )));
                });
                if effect != before {
                    match self.document.set_effect(child.id, Some(effect)) {
                        Ok(()) => self.request_preview = true,
                        Err(error) => self.effect_error(error.to_string()),
                    }
                }
            }
            if let Some(index) = move_up {
                let previous = &children[index - 1];
                let current = &children[index];
                let mut earlier = current.effect.clone().unwrap();
                let mut later = previous.effect.clone().unwrap();
                let prior = later.common.delay;
                later.common.delay = earlier.common.delay.max(prior + 0.01);
                earlier.common.delay = prior;
                let result = self.document.edit_transaction(
                    "Reorder effect timeline",
                    vec![
                        engine_world::SceneEdit::Effect {
                            entity: current.id,
                            before: current.effect.clone(),
                            after: Some(earlier),
                        },
                        engine_world::SceneEdit::Effect {
                            entity: previous.id,
                            before: previous.effect.clone(),
                            after: Some(later),
                        },
                    ],
                );
                if let Err(error) = result {
                    self.effect_error(error.to_string());
                } else {
                    self.request_preview = true;
                }
            }
            if let Some(entity) = duplicate {
                let result = self
                    .document
                    .snapshot_tree(entity)
                    .and_then(|tree| self.document.paste_tree(&tree, Some(snapshot.id)));
                if let Err(error) = result {
                    self.effect_error(error.to_string());
                } else {
                    self.request_preview = true;
                }
            }
            if let Some(entity) = delete {
                if let Err(error) = self.document.delete_tree(entity) {
                    self.effect_error(error.to_string());
                } else {
                    self.request_preview = true;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_effect_configuration_renders_with_native_widgets_and_preserves_typed_data() {
        let temporary = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        for dark in [true, false] {
            context.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            for kind in engine_world::effects::EffectKind::all_defaults() {
                let effect = engine_world::effects::EffectComponent::new(kind);
                let before = serde_json::to_value(&effect).unwrap();
                let mut value = before.clone();
                let mut output = context.run_ui(egui::RawInput::default(), |ui| {
                    assert!(!properties(ui, &mut value, temporary.path(), &[]));
                });
                output.textures_delta.clear();
                assert_eq!(
                    serde_json::from_value::<engine_world::effects::EffectComponent>(value)
                        .unwrap(),
                    effect
                );
            }
        }
    }

    #[test]
    fn deterministic_preview_scrub_preserves_the_saved_authoring_document() {
        let temporary = tempfile::tempdir().unwrap();
        let project = engine_project::Project::create(
            temporary.path().join("game"),
            "Game",
            engine_project::ProjectTemplate::Blank,
        )
        .unwrap();
        let mut document = engine_editor::AuthoringDocument::open(project, false).unwrap();
        let id = document
            .add_effect(
                engine_world::effects::EffectKind::Particle(
                    engine_world::effects::ParticleEmitter::default(),
                ),
                None,
            )
            .unwrap();
        document.save().unwrap();
        let before = document.world().snapshot(id).unwrap();
        let mut preview = EffectPreview::default();
        preview.scrub(&document, glam::Vec3::Z * 5.0, 1.0);
        let positions: Vec<_> = preview
            .runtime
            .frame()
            .particles
            .iter()
            .map(|p| p.position)
            .collect();
        assert!(!positions.is_empty());
        preview.scrub(&document, glam::Vec3::Z * 5.0, 1.0);
        assert_eq!(
            positions,
            preview
                .runtime
                .frame()
                .particles
                .iter()
                .map(|p| p.position)
                .collect::<Vec<_>>()
        );
        assert_eq!(document.world().snapshot(id).unwrap(), before);
        assert!(!document.is_modified());
    }
}

fn ensure_asset_meta(path: &Path) -> Result<engine_core::AssetId, String> {
    let sidecar = engine_assets::sidecar_path(path);
    if sidecar.is_file() {
        return engine_assets::load_meta(&sidecar)
            .map(|meta| meta.asset_id)
            .map_err(|e| e.to_string());
    }
    let contract = engine_assets::ImporterRegistry::contract(path).map_err(|e| e.to_string())?;
    let meta = engine_assets::AssetMeta::new(contract.id, contract.version);
    engine_assets::save_meta(&sidecar, &meta).map_err(|e| e.to_string())?;
    Ok(meta.asset_id)
}

fn preset_category(name: &str) -> &'static str {
    match name {
        "Campfire" | "Fire sparks" | "Smoke plume" | "Explosion" | "Muzzle flash" | "Embers"
        | "Firework burst" => "Fire and Explosions",
        "Magic projectile" | "Lightning beam" | "Energy shield" | "Healing aura" | "Teleport"
        | "Portal" | "Glowing trail" | "Shockwave" => "Magic and Energy",
        "Tire smoke" | "Skid mark" | "Exhaust smoke" | "Speed boost trail" | "Collision sparks"
        | "Rocket exhaust" => "Vehicles",
        "Rain" | "Snow" | "Dust" | "Leaves" | "Fireflies" | "Mist" | "Water splash"
        | "Ground ripple" => "Environment",
        _ => "Character and Combat",
    }
}

/// Curves are scene data rather than a node graph. Native egui responses own
/// pointer capture, focus and context menus; paint uses the active widget style.
#[allow(
    clippy::too_many_lines,
    reason = "one native interaction pass owns curve interpolation, handles and key operations"
)]
fn edit_key_graph(ui: &mut egui::Ui, keys: &mut Vec<Value>) -> bool {
    if keys.is_empty() {
        return false;
    }
    let gradient = keys[0].get("color").is_some();
    if !gradient && keys[0].get("value").is_none() {
        return false;
    }
    let (response, painter) = ui.allocate_painter(
        egui::vec2(ui.available_width().clamp(120.0, 320.0), 100.0),
        egui::Sense::click_and_drag(),
    );
    let rect = response.rect.shrink(9.0);
    painter.rect_filled(
        response.rect,
        ui.visuals().widgets.noninteractive.corner_radius,
        ui.visuals().extreme_bg_color,
    );
    let minimum = keys
        .iter()
        .filter_map(|key| key.get("value").and_then(Value::as_f64))
        .fold(0.0f64, f64::min)
        .to_f32()
        .unwrap_or(0.0);
    let maximum = keys
        .iter()
        .filter_map(|key| key.get("value").and_then(Value::as_f64))
        .fold(1.0f64, f64::max)
        .to_f32()
        .unwrap_or(1.0);
    let span = (maximum - minimum).max(0.001);
    let mut samples: Vec<_> = keys
        .iter()
        .filter_map(|key| {
            Some((
                key.get("time")?.as_f64()?.to_f32()?,
                if gradient {
                    key.get("color")?.clone()
                } else {
                    key.get("value")?.clone()
                },
            ))
        })
        .collect();
    samples.sort_by(|a, b| a.0.total_cmp(&b.0));
    let sample = |time: f32| -> Value {
        let before = samples
            .iter()
            .rev()
            .find(|(t, _)| *t <= time)
            .unwrap_or(&samples[0]);
        let after = samples
            .iter()
            .find(|(t, _)| *t >= time)
            .unwrap_or(samples.last().unwrap());
        let weight = if after.0 > before.0 {
            ((time - before.0) / (after.0 - before.0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let interpolate = |a: f64, b: f64| a + (b - a) * f64::from(weight);
        if gradient {
            Value::Array(
                (0..4)
                    .map(|i| {
                        Value::from(interpolate(
                            before.1[i].as_f64().unwrap_or(1.0),
                            after.1[i].as_f64().unwrap_or(1.0),
                        ))
                    })
                    .collect(),
            )
        } else {
            Value::from(interpolate(
                before.1.as_f64().unwrap_or(1.0),
                after.1.as_f64().unwrap_or(1.0),
            ))
        }
    };
    let color = |value: &Value| {
        let channel = |i| {
            (value[i].as_f64().unwrap_or(1.0).clamp(0.0, 1.0) * 255.0)
                .round()
                .to_u8()
                .unwrap_or(255)
        };
        egui::Color32::from_rgba_unmultiplied(channel(0), channel(1), channel(2), channel(3))
    };
    if samples.is_empty() {
        return false;
    }
    if gradient {
        for index in 0..128_u16 {
            let t = f32::from(index) / 128.0;
            let x = rect.left() + rect.width() * t;
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x, rect.top()),
                    egui::pos2(x + rect.width() / 128.0 + 1.0, rect.bottom()),
                ),
                0.0,
                color(&sample(t)),
            );
        }
    } else {
        let points = (0..=64_u16)
            .map(|index| {
                let time = f32::from(index) / 64.0;
                let value = sample(time).as_f64().unwrap_or(1.0).to_f32().unwrap_or(1.0);
                egui::pos2(
                    rect.left() + time * rect.width(),
                    rect.bottom() - ((value - minimum) / span) * rect.height(),
                )
            })
            .collect();
        painter.add(egui::Shape::line(points, ui.visuals().selection.stroke));
    }
    let mut changed = false;
    if response.double_clicked()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let time = ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        let value = sample(time);
        let key = if gradient {
            serde_json::json!({"time":time,"color":value})
        } else {
            serde_json::json!({"time":time,"value":value})
        };
        keys.push(key);
        changed = true;
    }
    let mut remove = None;
    let key_count = keys.len();
    for (index, key) in keys.iter_mut().enumerate() {
        let time = key["time"].as_f64().unwrap_or(0.0).to_f32().unwrap_or(0.0);
        let value = key["value"].as_f64().unwrap_or(0.0).to_f32().unwrap_or(0.0);
        let center = egui::pos2(
            rect.left() + time.clamp(0.0, 1.0) * rect.width(),
            if gradient {
                rect.center().y
            } else {
                rect.bottom() - ((value - minimum) / span) * rect.height()
            },
        );
        let handle = ui.interact(
            egui::Rect::from_center_size(center, egui::vec2(14.0, 14.0)),
            response.id.with(index),
            egui::Sense::drag(),
        );
        let style = ui.style().interact(&handle);
        painter.circle_filled(
            center,
            5.0,
            if gradient {
                color(&key["color"])
            } else {
                style.bg_fill
            },
        );
        painter.circle_stroke(center, 5.0, style.fg_stroke);
        if handle.dragged()
            && let Some(pointer) = handle.interact_pointer_pos()
        {
            key["time"] = Value::from(((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0));
            if !gradient {
                key["value"] = Value::from(
                    minimum + ((rect.bottom() - pointer.y) / rect.height()).clamp(0.0, 1.0) * span,
                );
            }
            changed = true;
        }
        handle.context_menu(|ui| {
            if ui
                .add_enabled(key_count > 1, egui::Button::new("Remove key"))
                .clicked()
            {
                remove = Some(index);
                ui.close();
            }
        });
    }
    if let Some(index) = remove {
        keys.remove(index);
        changed = true;
    }
    response.on_hover_text(
        "Drag keys to move them. Double-click to add a key. Right-click a key to remove it.",
    );
    changed
}

fn edit_parameters(
    ui: &mut egui::Ui,
    parameters: &mut serde_json::Map<String, Value>,
    root: &Path,
    references: &[(String, String)],
) -> bool {
    ui.collapsing("Runtime parameters", |ui| {
        ui.small("Bindings: enabled, visible, emission_rate, intensity, color, speed, size, gravity, force, density, progress, width, radius.");
        let id = ui.make_persistent_id("new-effect-parameter");
        let mut name = ui.ctx().data_mut(|data|data.get_temp::<String>(id)).unwrap_or_default();
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut name).hint_text("Parameter name"));
            ui.add_enabled_ui(!name.trim().is_empty() && !parameters.contains_key(name.trim()), |ui| {
                ui.menu_button("Add parameter", |ui| {
                    for (label,value) in [("Scalar",serde_json::json!({"Scalar":1.0})),("Vector",serde_json::json!({"Vector":[0.0,0.0,0.0]})),
                        ("Color",serde_json::json!({"Color":[1.0,1.0,1.0,1.0]})),("Boolean",serde_json::json!({"Boolean":true}))] {
                        if ui.button(label).clicked() {parameters.insert(name.trim().into(),value);changed=true;ui.close();}
                    }
                });
            });
        });
        ui.ctx().data_mut(|data|data.insert_temp(id,name));
        let mut remove = None;
        for (name,value) in parameters.iter_mut() {
            ui.horizontal(|ui| {ui.strong(name);if ui.small_button("Remove").clicked(){remove=Some(name.clone());}});
            changed |= edit_value(ui,name,value,root,references);
        }
        if let Some(name)=remove {parameters.remove(&name);changed=true;}
        changed
    }).body_returned.unwrap_or(false)
}

fn field_hint(name: &str) -> &'static str {
    match name {
        "duration" | "delay" | "lifetime" | "fade_in" | "fade_out" | "interval" | "time"
        | "update_interval" => "Seconds. Curve/gradient key times use normalized lifetime (0–1).",
        "rate" | "emission_rate" => {
            "Particles per second; global and per-emitter capacity still apply."
        }
        "rate_over_distance" => "Particles per world unit of emitter movement.",
        "width"
        | "radius"
        | "dimensions"
        | "length"
        | "half_extents"
        | "render_distance"
        | "thickness"
        | "min_vertex_distance" => {
            "World units; entity parenting and transforms follow the scene hierarchy."
        }
        "gravity" | "force" => "Acceleration in world units per second squared.",
        "random_seed" => {
            "Stable deterministic seed. Changing the seed resets this simulation instance."
        }
        "opacity" | "weight" | "probability" | "progress" | "sub_emitter_probability" => {
            "Normalized value from 0 to 1."
        }
        "visibility_layers" | "layers" | "collision_layers" => {
            "Bit mask: each bit selects an affected scene/effect layer."
        }
        "time_scale" => "Simulation speed multiplier. Zero freezes this effect.",
        "material" | "texture" | "mesh" | "cubemap" | "normal_map" | "orm_map" => {
            "Project asset stored by stable .rmeta identity; use Choose to assign an asset."
        }
        _ => "Changes are validated and saved through the scene undo history.",
    }
}

#[cfg(test)]
mod curve_interaction_tests {
    use super::*;

    #[test]
    fn dragging_curve_key_changes_serialized_time_and_value() {
        let context = egui::Context::default();
        let mut keys = vec![
            serde_json::json!({"time":0.0,"value":0.0}),
            serde_json::json!({"time":1.0,"value":1.0}),
        ];
        let screen = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(640.0, 480.0),
        ));
        let mut first = context.run_ui(
            egui::RawInput {
                screen_rect: screen,
                ..Default::default()
            },
            |ui| {
                edit_key_graph(ui, &mut keys);
            },
        );
        first.textures_delta.clear();
        let center = first
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Circle(circle) => Some(circle.center),
                _ => None,
            })
            .expect("curve graph paints key handles");
        let destination = center + egui::vec2(45.0, -20.0);
        for events in [
            vec![
                egui::Event::PointerMoved(center),
                egui::Event::PointerButton {
                    pos: center,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            vec![egui::Event::PointerMoved(destination)],
            vec![egui::Event::PointerButton {
                pos: destination,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ] {
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: screen,
                    events,
                    ..Default::default()
                },
                |ui| {
                    edit_key_graph(ui, &mut keys);
                },
            );
            output.textures_delta.clear();
        }
        assert!(keys[0]["time"].as_f64().unwrap() > 0.05);
        assert!(keys[0]["value"].as_f64().unwrap() > 0.05);
        assert_eq!(keys[1], serde_json::json!({"time":1.0,"value":1.0}));
    }
}

#[cfg(test)]
mod preview_child_tests {
    use super::*;
    use engine_world::effects::*;

    #[test]
    fn editor_preview_runs_real_subemitters_and_stop_releases_them() {
        let temporary = tempfile::tempdir().unwrap();
        let project = engine_project::Project::create(
            temporary.path().join("game"),
            "Game",
            engine_project::ProjectTemplate::Blank,
        )
        .unwrap();
        let path = project.root().join("assets/impact.effect");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let meta = engine_assets::AssetMeta::new("rustic.effect", 1);
        let asset = EffectAsset::new(
            "Impact",
            vec![EffectComponent::new(EffectKind::Shockwave(
                RingEffect::default(),
            ))],
        );
        std::fs::write(&path, asset.to_bytes().unwrap()).unwrap();
        engine_assets::save_meta(&engine_assets::sidecar_path(&path), &meta).unwrap();
        let mut document = engine_editor::AuthoringDocument::open(project, false).unwrap();
        document
            .add_effect(
                EffectKind::Particle(ParticleEmitter {
                    rate: 0.0,
                    bursts: vec![Burst {
                        count: 1,
                        ..Default::default()
                    }],
                    sub_emitter: Some(meta.asset_id),
                    ..Default::default()
                }),
                None,
            )
            .unwrap();
        document.save().unwrap();
        let count = document.world().entity_count();
        let mut preview = EffectPreview::default();
        preview.control(&document, PlaybackCommand::Play);
        preview.advance(&document, glam::Vec3::Z * 5.0, 0.1);
        assert!(
            preview.runtime.world.entity_count() > count,
            "preview must instantiate child assets through the runtime world"
        );
        assert_eq!(document.world().entity_count(), count);
        assert!(!document.is_modified());
        preview.control(&document, PlaybackCommand::Stop);
        assert_eq!(preview.runtime.world.entity_count(), count);
    }
}

fn missing_effect_meshes(
    effect: &engine_world::effects::EffectComponent,
    models: &std::collections::BTreeMap<engine_core::AssetId, engine_assets::ModelArtifact>,
) -> bool {
    use engine_world::effects::EffectKind;
    match &effect.kind {
        EffectKind::Particle(p) | EffectKind::MeshParticle(p) => {
            p.mesh.is_some_and(|id| !models.contains_key(&id))
        }
        EffectKind::Weather(w) => w.emitter.mesh.is_some_and(|id| !models.contains_key(&id)),
        EffectKind::FoliageScatter(f) => {
            f.surface_target.is_some()
                || f.meshes
                    .iter()
                    .chain(f.weighted_meshes.iter().map(|mesh| &mesh.mesh))
                    .any(|id| !models.contains_key(id))
        }
        _ => false,
    }
}

fn particle_module(name: &str) -> &'static str {
    match name {
        "rate"
        | "rate_over_distance"
        | "bursts"
        | "shape"
        | "dimensions"
        | "surface_only"
        | "mesh_spawn_triangles" => "Emission",
        "lifetime" | "speed" | "direction" | "start_size" | "angular_velocity"
        | "inherit_velocity" => "Initialization",
        "gravity" | "force" | "drag" | "wind_influence" | "noise" | "velocity_limit" => "Movement",
        "appearance" | "mesh" | "mesh_variants" | "flipbook" | "flipbook_fps" | "stretch" => {
            "Appearance"
        }
        "collision" | "collision_layers" | "bounce" | "friction" | "collision_response" => {
            "Collision"
        }
        "sub_emitter" | "sub_emitter_probability" | "sub_emitters" | "max_generations" => {
            "Sub-emitters"
        }
        _ => "Capacity and simulation",
    }
}

fn edit_particle_modules(
    ui: &mut egui::Ui,
    fields: &mut serde_json::Map<String, Value>,
    root: &Path,
    references: &[(String, String)],
) -> bool {
    let mut changed = false;
    for module in [
        "Emission",
        "Initialization",
        "Movement",
        "Appearance",
        "Collision",
        "Sub-emitters",
        "Capacity and simulation",
    ] {
        egui::CollapsingHeader::new(module)
            .default_open(module == "Emission")
            .show(ui, |ui| {
                for (name, value) in fields
                    .iter_mut()
                    .filter(|(name, _)| particle_module(name) == module)
                {
                    if name == "mesh_spawn_triangles" {
                        ui.small(format!(
                            "{} mesh-surface triangles loaded from the assigned mesh",
                            value.as_array().map_or(0, Vec::len)
                        ));
                    } else {
                        changed |= edit_value(ui, name, value, root, references);
                    }
                }
            });
    }
    changed
}

fn prepare_mesh_emission(
    effect: &mut engine_world::effects::EffectComponent,
    models: &std::collections::BTreeMap<engine_core::AssetId, engine_assets::ModelArtifact>,
) {
    use engine_world::effects::EffectKind;
    let particle = match &mut effect.kind {
        EffectKind::Particle(p) | EffectKind::MeshParticle(p) => p,
        EffectKind::Weather(w) => &mut w.emitter,
        _ => return,
    };
    if particle.shape != engine_world::effects::SpawnShape::MeshSurface {
        return;
    }
    let Some(model) = particle.mesh.and_then(|id| models.get(&id)) else {
        return;
    };
    particle.mesh_spawn_triangles = model
        .meshes
        .iter()
        .flat_map(|mesh| {
            mesh.indices
                .as_chunks::<3>()
                .0
                .iter()
                .filter_map(move |face| {
                    Some([
                        *mesh.positions.get(usize::try_from(face[0]).ok()?)?,
                        *mesh.positions.get(usize::try_from(face[1]).ok()?)?,
                        *mesh.positions.get(usize::try_from(face[2]).ok()?)?,
                    ])
                })
        })
        .take(100_000)
        .collect();
}

fn binding_paths(value: &Value, prefix: &str, result: &mut Vec<String>) {
    match value {
        Value::Object(fields) => {
            for (name, value) in fields {
                let path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                binding_paths(value, &path, result);
            }
        }
        Value::Number(_) | Value::Bool(_) | Value::Array(_)
            if prefix.starts_with("kind.") || prefix.starts_with("common.") =>
        {
            result.push(prefix.into());
        }
        _ => {}
    }
}
#[allow(
    clippy::too_many_lines,
    reason = "validated native parameter and event link editors share one undoable group change"
)]
fn edit_group_links(
    ui: &mut egui::Ui,
    group: &mut engine_world::effects::EffectGroup,
    tracks: &[engine_world::effects::EffectComponent],
) -> bool {
    use engine_world::effects::{
        GroupParameterBinding, GroupTriggerBinding, validate_group_bindings,
    };
    let mut changed = false;
    ui.collapsing("Parameter bindings", |ui| {
        let mut remove = None;
        for (index, binding) in group.bindings.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!(
                        "{} → track {} / {}",
                        binding.parameter,
                        binding.child + 1,
                        binding.property_path
                    ));
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut binding.scale)
                                .speed(0.01)
                                .prefix("Scale "),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut binding.offset)
                                .speed(0.01)
                                .prefix("Offset "),
                        )
                        .changed();
                    if ui.small_button("Remove").clicked() {
                        remove = Some(index);
                    }
                })
            });
        }
        if let Some(index) = remove {
            group.bindings.remove(index);
            changed = true;
        }
        ui.menu_button("Add parameter binding", |ui| {
            for parameter in group.parameters.keys() {
                ui.menu_button(parameter, |ui| {
                    for (child, effect) in tracks.iter().enumerate() {
                        ui.menu_button(
                            format!("Track {} · {}", child + 1, effect.kind.name()),
                            |ui| {
                                let mut paths = Vec::new();
                                binding_paths(
                                    &serde_json::to_value(effect).unwrap_or(Value::Null),
                                    "",
                                    &mut paths,
                                );
                                for path in paths {
                                    let binding = GroupParameterBinding {
                                        parameter: parameter.clone(),
                                        child: u32::try_from(child).unwrap_or_default(),
                                        property_path: path.clone(),
                                        scale: 1.0,
                                        offset: 0.0,
                                    };
                                    if validate_group_bindings(
                                        &group.parameters,
                                        std::slice::from_ref(&binding),
                                        tracks,
                                    )
                                    .is_ok()
                                        && ui.button(&path).clicked()
                                    {
                                        group.bindings.push(binding);
                                        changed = true;
                                        ui.close();
                                    }
                                }
                            },
                        );
                    }
                });
            }
        });
    });
    ui.collapsing("Event links", |ui| {
        let mut remove = None;
        for (index, binding) in group.trigger_bindings.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                ui.horizontal_wrapped(|ui| {
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut binding.source_child)
                                .range(
                                    0..=u32::try_from(tracks.len().saturating_sub(1))
                                        .unwrap_or_default(),
                                )
                                .prefix("Source track index "),
                        )
                        .changed();
                    egui::ComboBox::from_id_salt("event")
                        .selected_text(format!("{:?}", binding.event))
                        .show_ui(ui, |ui| {
                            for event in [
                                engine_world::effects::CompositeTriggerEvent::Birth,
                                engine_world::effects::CompositeTriggerEvent::Collision,
                                engine_world::effects::CompositeTriggerEvent::Death,
                                engine_world::effects::CompositeTriggerEvent::Completed,
                            ] {
                                changed |= ui
                                    .selectable_value(
                                        &mut binding.event,
                                        event,
                                        format!("{event:?}"),
                                    )
                                    .changed();
                            }
                        });
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut binding.target_child)
                                .range(
                                    0..=u32::try_from(tracks.len().saturating_sub(1))
                                        .unwrap_or_default(),
                                )
                                .prefix("Target track index "),
                        )
                        .changed();
                    egui::ComboBox::from_id_salt("action")
                        .selected_text(format!("{:?}", binding.action))
                        .show_ui(ui, |ui| {
                            for action in [
                                engine_world::effects::CompositeTriggerAction::Play,
                                engine_world::effects::CompositeTriggerAction::Pause,
                                engine_world::effects::CompositeTriggerAction::Resume,
                                engine_world::effects::CompositeTriggerAction::Stop,
                                engine_world::effects::CompositeTriggerAction::Restart,
                                engine_world::effects::CompositeTriggerAction::Reset,
                            ] {
                                changed |= ui
                                    .selectable_value(
                                        &mut binding.action,
                                        action,
                                        format!("{action:?}"),
                                    )
                                    .changed();
                            }
                        });
                    if ui.small_button("Remove").clicked() {
                        remove = Some(index);
                    }
                })
            });
        }
        if let Some(index) = remove {
            group.trigger_bindings.remove(index);
            changed = true;
        }
        if !tracks.is_empty() && ui.button("Add event link").clicked() {
            group.trigger_bindings.push(GroupTriggerBinding::default());
            changed = true;
        }
    });
    changed
}

fn effect_handle_name(handle: u8) -> &'static str {
    match handle {
        1 => "Emission / volume size",
        2 => "Beam start",
        3 => "Beam end",
        4 => "Force / wind direction",
        _ => "Entity transform",
    }
}
fn effect_dimensions(kind: &engine_world::effects::EffectKind) -> Option<[f32; 3]> {
    use engine_world::effects::EffectKind;
    match kind {
        EffectKind::Particle(value) | EffectKind::MeshParticle(value) => Some(value.dimensions),
        EffectKind::Weather(value) => Some(value.emitter.dimensions),
        EffectKind::ForceField(value) | EffectKind::Wind(value) => Some(value.dimensions),
        EffectKind::CollisionVolume(value) => Some(value.dimensions),
        EffectKind::Fog(value) => Some(value.dimensions),
        EffectKind::Decal(value) => Some(value.dimensions),
        EffectKind::FoliageScatter(value) => Some(value.dimensions),
        EffectKind::PostProcess(value) => Some(value.dimensions),
        EffectKind::ReflectionProbe(value) => Some(value.dimensions),
        _ => None,
    }
}
fn set_effect_dimensions(kind: &mut engine_world::effects::EffectKind, dimensions: [f32; 3]) {
    use engine_world::effects::EffectKind;
    match kind {
        EffectKind::Particle(value) | EffectKind::MeshParticle(value) => {
            value.dimensions = dimensions
        }
        EffectKind::Weather(value) => value.emitter.dimensions = dimensions,
        EffectKind::ForceField(value) | EffectKind::Wind(value) => value.dimensions = dimensions,
        EffectKind::CollisionVolume(value) => value.dimensions = dimensions,
        EffectKind::Fog(value) => value.dimensions = dimensions,
        EffectKind::Decal(value) => value.dimensions = dimensions,
        EffectKind::FoliageScatter(value) => value.dimensions = dimensions,
        EffectKind::PostProcess(value) => value.dimensions = dimensions,
        EffectKind::ReflectionProbe(value) => value.dimensions = dimensions,
        _ => {}
    }
}
#[derive(Clone)]
struct EffectHandleDrag {
    entity: engine_world::EntityId,
    before: engine_world::effects::EffectComponent,
    after: engine_world::effects::EffectComponent,
}
impl crate::EditorViewer<'_> {
    #[allow(
        clippy::too_many_lines,
        reason = "standard transform-gizmo gesture previews and commits one effect property transaction"
    )]
    pub(super) fn effect_handle_gizmo(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        snapshot: &engine_world::EntitySnapshot,
        world: glam::Mat4,
    ) -> bool {
        use engine_world::effects::EffectKind;
        use glam::{Mat4, Vec3};
        let drag_key = egui::Id::new("effect-handle-drag");
        let mut drag = ui
            .ctx()
            .data_mut(|data| data.get_temp::<EffectHandleDrag>(drag_key));
        let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
        if drag.as_ref().is_some_and(|drag| {
            drag.entity != snapshot.id || !ui.input(|input| input.pointer.primary_down()) || escape
        }) {
            let finished = drag.take().unwrap();
            let result = if escape {
                self.document
                    .preview_effect(finished.entity, finished.before)
            } else {
                self.document.edit_transaction(
                    "Edit effect handle",
                    vec![engine_world::SceneEdit::Effect {
                        entity: finished.entity,
                        before: Some(finished.before),
                        after: Some(finished.after),
                    }],
                )
            };
            if let Err(error) = result {
                self.effect_error(error.to_string());
            }
            self.request_preview = true;
            ui.ctx()
                .data_mut(|data| data.remove::<EffectHandleDrag>(drag_key));
            if escape {
                self.gizmo_widget.reset();
                return true;
            }
        }
        let handle = ui
            .ctx()
            .data_mut(|data| data.get_temp::<u8>(egui::Id::new(("effect-handle", snapshot.id))))
            .unwrap_or_default();
        let Some(effect) = &snapshot.effect else {
            return false;
        };
        if handle == 0 {
            return false;
        }
        let mut parent = world;
        let point = match (handle, &effect.kind) {
            (1, _) => effect
                .common
                .bounds
                .as_ref()
                .map(|bounds| {
                    Vec3::from_array(bounds.center) + Vec3::from_array(bounds.half_extents)
                })
                .or_else(|| {
                    effect_dimensions(&effect.kind)
                        .map(|dimensions| Vec3::from_array(dimensions) * 0.5)
                }),
            (2 | 3, EffectKind::Beam(beam)) => {
                let target = if handle == 2 {
                    beam.start_target
                } else {
                    beam.end_target
                };
                parent = target
                    .and_then(|target| self.document.world().world_transform(target).ok())
                    .map_or(world, |target| target.0);
                Some(Vec3::from_array(if handle == 2 {
                    beam.start
                } else {
                    beam.end
                }))
            }
            (4, EffectKind::Wind(field) | EffectKind::ForceField(field)) => Some(
                Vec3::from_array(field.direction).normalize_or_zero()
                    * Vec3::from_array(field.dimensions).max_element().max(1.0)
                    * 0.5,
            ),
            _ => None,
        };
        let Some(point) = point else {
            return false;
        };
        let mut settings = *self.gizmo;
        settings.operation = engine_editor::GizmoOperation::Translate;
        if let Some(after) = self.gizmo_widget.interact(
            ui,
            rect,
            *self.camera,
            crate::transform_widget::Target {
                entity: snapshot.id,
                world: parent * Mat4::from_translation(point),
                parent,
            },
            settings,
        ) {
            let mut edited = effect.clone();
            match (handle, &mut edited.kind) {
                (1, kind) => {
                    if let Some(bounds) = &mut edited.common.bounds {
                        bounds.half_extents = (after.translation - Vec3::from_array(bounds.center))
                            .abs()
                            .max(Vec3::splat(0.001))
                            .to_array();
                    } else {
                        set_effect_dimensions(
                            kind,
                            (after.translation.abs() * 2.0)
                                .max(Vec3::splat(0.001))
                                .to_array(),
                        );
                    }
                }
                (2, EffectKind::Beam(beam)) => beam.start = after.translation.to_array(),
                (3, EffectKind::Beam(beam)) => beam.end = after.translation.to_array(),
                (4, EffectKind::Wind(field) | EffectKind::ForceField(field)) => {
                    field.direction = after.translation.normalize_or_zero().to_array()
                }
                _ => {}
            }
            if edited != *effect
                && self
                    .document
                    .preview_effect(snapshot.id, edited.clone())
                    .is_ok()
            {
                let active = drag.get_or_insert(EffectHandleDrag {
                    entity: snapshot.id,
                    before: effect.clone(),
                    after: effect.clone(),
                });
                active.after = edited;
                self.request_preview = true;
            }
        }
        if let Some(drag) = drag {
            ui.ctx().data_mut(|data| data.insert_temp(drag_key, drag));
        }
        true
    }
}
