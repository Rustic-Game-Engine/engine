//! Native egui authoring for reusable `.material` PBR sources and scene overrides.
use crate::{EditorViewer, Severity, egui, simple_console};
use engine_assets::{AssetMeta, MaterialAsset, load_meta, save_meta, sidecar_path};
use engine_core::AssetId;
use engine_world::{EntitySnapshot, TextureMap};
use std::path::{Path, PathBuf};
#[derive(Clone)]
struct Draft {
    path: PathBuf,
    material: MaterialAsset,
    error: Option<String>,
}
fn draft_key() -> egui::Id {
    egui::Id::new("rustic-material-source-editor")
}

pub(super) fn create_material(root: &Path) -> Result<Option<(PathBuf, AssetId)>, String> {
    let Some(path) = rfd::FileDialog::new()
        .set_directory(root.join("assets"))
        .set_file_name("Material.material")
        .add_filter("PBR material", &["material"])
        .save_file()
    else {
        return Ok(None);
    };
    validate_destination(root, &path)?;
    let material = MaterialAsset {
        name: path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        ..MaterialAsset::default()
    };
    write_material(&path, &material)?;
    let id = ensure_meta(&path, &material)?;
    Ok(Some((path, id)))
}
fn validate_destination(root: &Path, path: &Path) -> Result<(), String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let parent = path
        .parent()
        .ok_or("Choose a project assets folder")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !parent.starts_with(root) {
        return Err("Save materials inside the project assets folder".into());
    }
    Ok(())
}
fn ensure_meta(path: &Path, material: &MaterialAsset) -> Result<AssetId, String> {
    let meta_path = sidecar_path(path);
    let mut meta = if meta_path.exists() {
        load_meta(&meta_path).map_err(|e| e.to_string())?
    } else {
        AssetMeta::new("rustic.material", 1)
    };
    meta.dependencies = material.dependencies();
    save_meta(&meta_path, &meta).map_err(|e| e.to_string())?;
    Ok(meta.asset_id)
}
fn write_material(path: &Path, material: &MaterialAsset) -> Result<(), String> {
    let bytes = material.to_bytes().map_err(|e| e.to_string())?;
    engine_platform::AtomicFileService::new()
        .write(path, &bytes)
        .map_err(|e| e.to_string())?;
    ensure_meta(path, material)?;
    Ok(())
}
fn texture_source(root: &Path) -> Result<Option<AssetId>, String> {
    let Some(path) = rfd::FileDialog::new()
        .set_directory(root)
        .add_filter("Texture image", &["png", "jpg", "jpeg", "tga", "hdr"])
        .pick_file()
    else {
        return Ok(None);
    };
    let relative = crate::textures::import_texture(root, &path)?;
    let path = root.join(relative);
    let meta_path = sidecar_path(&path);
    let meta = if meta_path.exists() {
        load_meta(&meta_path).map_err(|e| e.to_string())?
    } else {
        let meta = AssetMeta::new("rustic.image", 1);
        save_meta(&meta_path, &meta).map_err(|e| e.to_string())?;
        meta
    };
    Ok(Some(meta.asset_id))
}
pub(super) fn find_material(root: &Path, id: AssetId) -> Option<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
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
                pending.push(path);
            } else if kind.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("material"))
                && load_meta(&sidecar_path(&path)).is_ok_and(|meta| meta.asset_id == id)
            {
                return Some(path);
            }
        }
    }
    None
}
impl EditorViewer<'_> {
    #[allow(
        clippy::too_many_lines,
        reason = "source assignment and instance overrides commit as one validated material edit"
    )]
    pub(super) fn material_inspector(&mut self, ui: &mut egui::Ui, snapshot: &EntitySnapshot) {
        if snapshot.primitive.is_none() && snapshot.mesh.is_none() {
            return;
        }
        let mut surface = snapshot.textures.clone().unwrap_or_default();
        let before_material = snapshot.material.map(|material| material.asset);
        let mut material = before_material.or(surface.material);
        let before = surface.clone();
        ui.collapsing("PBR material", |ui| {
            ui.add_enabled_ui(!self.document.is_read_only() && !self.playing, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Create material…").clicked() {
                        match create_material(self.project_root) {
                            Ok(Some((path, id))) => {
                                material = Some(id);
                                surface.material = None;
                                self.open_material(ui.ctx(), path);
                            }
                            Ok(None) => {}
                            Err(error) => self.material_error(error),
                        }
                    }
                    if ui.button("Choose material…").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .set_directory(self.project_root)
                            .add_filter("PBR material", &["material"])
                            .pick_file()
                    {
                        match (|| {
                            validate_destination(self.project_root, &path)?;
                            let source = MaterialAsset::from_bytes(
                                &std::fs::read(&path).map_err(|e| e.to_string())?,
                            )
                            .map_err(|e| e.to_string())?;
                            ensure_meta(&path, &source)
                        })() {
                            Ok(id) => {
                                material = Some(id);
                                surface.material = None;
                            }
                            Err(error) => self.material_error(error),
                        }
                    }
                    if material.is_some() && ui.button("Clear material").clicked() {
                        material = None;
                        surface.material = None;
                    }
                });
                if let Some(id) = material {
                    if let Some(path) = find_material(self.project_root, id) {
                        ui.label(
                            path.strip_prefix(self.project_root)
                                .unwrap_or(&path)
                                .display()
                                .to_string(),
                        );
                        if ui.button("Edit shared material…").clicked() {
                            self.open_material(ui.ctx(), path);
                        }
                    } else {
                        ui.weak("Material source is missing. Choose a project .material asset.");
                    }
                }
                ui.checkbox(&mut surface.pbr, "PBR lighting for scalar overrides");
                ui.horizontal(|ui| {
                    ui.label("Base color multiplier");
                    ui.color_edit_button_rgba_unmultiplied(&mut surface.base_color);
                });
                scalar_override(ui, "Roughness", &mut surface.roughness, 0.5);
                scalar_override(ui, "Metallic", &mut surface.metallic, 0.0);
                ui.add(
                    egui::Slider::new(&mut surface.normal_scale, 0.0..=10.0)
                        .text("Normal strength"),
                );
                ui.add(
                    egui::Slider::new(&mut surface.occlusion_strength, 0.0..=1.0)
                        .text("AO strength"),
                );
                ui.checkbox(&mut surface.cast_shadows, "Cast shadows");
                ui.checkbox(&mut surface.receive_shadows, "Receive shadows");
                let mut wind_override = surface.wind_response.is_some();
                if ui
                    .checkbox(&mut wind_override, "Override wind response")
                    .changed()
                {
                    surface.wind_response = wind_override.then_some(0.0);
                }
                if let Some(response) = &mut surface.wind_response {
                    ui.add(egui::Slider::new(response, 0.0..=10.0).text("Wind response"));
                }
            });
        });
        if surface != before || material != before_material {
            let mut edits = Vec::new();
            if material != before_material {
                edits.push(engine_world::SceneEdit::Material {
                    entity: snapshot.id,
                    before: before_material,
                    after: material,
                });
            }
            if surface != before {
                edits.push(engine_world::SceneEdit::Textures {
                    entity: snapshot.id,
                    before: snapshot.textures.clone(),
                    after: Some(surface),
                });
            }
            match self.document.edit_transaction("PBR material", edits) {
                Ok(()) => self.request_preview = true,
                Err(error) => self.material_error(error.to_string()),
            }
        }
    }
    pub(super) fn open_material(&mut self, context: &egui::Context, path: PathBuf) {
        match std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| MaterialAsset::from_bytes(&bytes).map_err(|e| e.to_string()))
        {
            Ok(material) => context.data_mut(|data| {
                data.insert_temp(
                    draft_key(),
                    Draft {
                        path,
                        material,
                        error: None,
                    },
                );
            }),
            Err(error) => self.material_error(error),
        }
    }
    fn material_error(&mut self, error: String) {
        self.console
            .push(simple_console(Severity::Error, "material", error));
    }
    pub(super) fn material_editor(&mut self, context: &egui::Context) {
        let Some(mut draft) = context.data_mut(|data| data.get_temp::<Draft>(draft_key())) else {
            return;
        };
        let mut open = true;
        let mut saved = false;
        egui::Window::new("PBR material asset")
            .id(draft_key())
            .open(&mut open)
            .vscroll(true)
            .show(context, |ui| {
                ui.label(draft.path.file_name().unwrap_or_default().to_string_lossy());
                ui.add_enabled_ui(!self.document.is_read_only() && !self.playing, |ui| {
                    edit_source(ui, &mut draft.material, self.project_root, &mut draft.error);
                    if ui.button("Save material").clicked() {
                        match write_material(&draft.path, &draft.material) {
                            Ok(()) => {
                                saved = true;
                                draft.error = None;
                            }
                            Err(error) => draft.error = Some(error),
                        }
                    }
                });
                if let Some(error) = &draft.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
            });
        if open {
            context.data_mut(|data| data.insert_temp(draft_key(), draft));
        } else {
            context.data_mut(|data| data.remove::<Draft>(draft_key()));
        }
        self.request_preview |= saved;
    }
}
fn scalar_override(ui: &mut egui::Ui, name: &str, value: &mut Option<f32>, default: f32) {
    let mut enabled = value.is_some();
    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut enabled, format!("Override {name}"))
            .changed()
        {
            *value = enabled.then_some(default);
        }
        if let Some(value) = value {
            ui.add(egui::Slider::new(value, 0.0..=1.0).text(name));
        }
    });
}
fn edit_source(
    ui: &mut egui::Ui,
    material: &mut MaterialAsset,
    root: &Path,
    error: &mut Option<String>,
) {
    ui.text_edit_singleline(&mut material.name);
    ui.horizontal(|ui| {
        ui.label("Base color and opacity");
        ui.color_edit_button_rgba_unmultiplied(&mut material.base_color);
    });
    ui.add(egui::Slider::new(&mut material.roughness, 0.0..=1.0).text("Roughness factor"));
    ui.add(egui::Slider::new(&mut material.metallic, 0.0..=1.0).text("Metallic factor"));
    ui.horizontal(|ui| {
        ui.label("Emission color");
        ui.color_edit_button_rgb(&mut material.emissive_color);
    });
    ui.add(
        egui::DragValue::new(&mut material.emissive_strength)
            .range(0.0..=10000.0)
            .speed(0.1)
            .prefix("Emission "),
    );
    ui.add(egui::Slider::new(&mut material.normal_scale, 0.0..=10.0).text("Normal strength"));
    ui.add(egui::Slider::new(&mut material.occlusion_strength, 0.0..=1.0).text("AO strength"));
    for (name, value) in [
        ("UV scale", &mut material.uv_scale),
        ("UV offset", &mut material.uv_offset),
    ] {
        ui.horizontal(|ui| {
            ui.label(name);
            for axis in value {
                ui.add(egui::DragValue::new(axis).speed(0.01));
            }
        });
    }
    ui.add(
        egui::DragValue::new(&mut material.height_scale)
            .range(-100.0..=100.0)
            .speed(0.001)
            .prefix("Height scale "),
    );
    scalar_override(ui, "Alpha cutoff", &mut material.alpha_cutoff, 0.5);
    ui.checkbox(&mut material.cast_shadows, "Cast shadows");
    ui.checkbox(&mut material.receive_shadows, "Receive shadows");
    ui.add(egui::Slider::new(&mut material.wind_response, 0.0..=10.0).text("Wind response"));
    ui.collapsing("PBR texture maps",|ui|{
        for kind in TextureMap::ALL {
            ui.push_id(kind,|ui|{
                ui.horizontal(|ui|{
                    ui.label(kind.label());
                    ui.label(material.maps.get(&kind).map_or_else(||"None".into(),ToString::to_string));
                    if ui.button("Choose…").clicked(){match texture_source(root){Ok(Some(id))=>{material.maps.insert(kind,id);},Ok(None)=>{},Err(message)=>*error=Some(message)}}
                    if ui.button("Clear").clicked(){material.maps.remove(&kind);}
                });
            });
        }
        ui.horizontal(|ui|{
            ui.label("Packed ORM (R=AO, G=roughness, B=metallic)");
            if ui.button("Choose…").clicked(){match texture_source(root){Ok(Some(id))=>material.orm_map=Some(id),Ok(None)=>{},Err(message)=>*error=Some(message)}}
            if material.orm_map.is_some()&&ui.button("Clear").clicked(){material.orm_map=None;}
        });
        ui.small("Individual AO, roughness and metallic maps override their corresponding packed ORM channel. Map channels multiply the material factors.");
    });
}
