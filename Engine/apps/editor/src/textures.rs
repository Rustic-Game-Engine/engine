use super::{
    EditorViewer, EntityId, Path, PathBuf, Severity, Vec2, ViewportDrop, egui, hash_value,
    pick_meshes, pick_scene, simple_console,
};
use engine_world::TextureMap;

pub(super) fn is_texture(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["png", "jpg", "jpeg", "tga", "hdr"]
            .iter()
            .any(|kind| e.eq_ignore_ascii_case(kind))
    })
}

/// Import external images into the project so saved scenes and builds remain portable.
fn import_texture(root: &Path, source: &Path) -> Result<String, String> {
    let source = if source.is_absolute() {
        source.to_path_buf()
    } else {
        root.join(source)
    };
    if !is_texture(&source) {
        return Err("Use PNG, JPEG, TGA or Radiance HDR images".into());
    }
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    // Decode before changing the document. Never accept a broken slot silently.
    let reader = image::ImageReader::open(&source).map_err(|e| e.to_string())?;
    let mut reader = reader.with_guessed_format().map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|e| format!("Cannot load {}: {e}", source.display()))?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let destination = if source.starts_with(root.join("assets")) {
        source
    } else {
        let bytes = std::fs::read(&source).map_err(|e| e.to_string())?;
        let folder = root.join("assets/textures");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let stem = source.file_stem().unwrap_or_default().to_string_lossy();
        let extension = source.extension().unwrap_or_default().to_string_lossy();
        let destination = folder.join(format!("{stem}-{:016x}.{extension}", hash_value(&bytes)));
        if !destination.exists() {
            std::fs::write(&destination, bytes).map_err(|e| e.to_string())?;
        }
        destination
    };
    Ok(destination
        .strip_prefix(root)
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .replace('\\', "/"))
}

impl EditorViewer<'_> {
    pub(super) fn texture_drop_target(
        &self,
        ui: &egui::Ui,
        response: &egui::Response,
    ) -> Option<EntityId> {
        let p = ui.input(|i| i.pointer.interact_pos())?;
        let ray = self.camera.world_ray(
            Vec2::new(p.x - response.rect.left(), p.y - response.rect.top()),
            Vec2::new(response.rect.width(), response.rect.height()),
        )?;
        pick_meshes(ray, &pick_scene(self.document, self.model_assets))
    }

    pub(super) fn drop_viewport_textures(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        paths: &[PathBuf],
    ) {
        if self.playing || self.document.is_read_only() {
            return;
        }
        if let Some(target) = self.texture_drop_target(ui, response) {
            self.assign_texture_files(target, paths);
        } else {
            self.console.push(simple_console(
                Severity::Warning,
                "textures",
                "Drop textures onto the surface of a mesh or primitive.",
            ));
        }
    }

    pub(super) fn assign_texture_files(&mut self, entity: EntityId, paths: &[PathBuf]) {
        if self.playing || self.document.is_read_only() {
            return;
        }
        let result = (|| -> Result<(), String> {
            let s = self
                .document
                .world()
                .snapshot(entity)
                .map_err(|e| e.to_string())?;
            if s.mesh.is_none() && s.primitive.is_none() && s.textures.is_none() {
                return Err("Choose a mesh, primitive or Texture child".into());
            }
            let mut textures = s.textures.unwrap_or_default();
            for path in paths {
                let relative = import_texture(self.project_root, path)?;
                textures
                    .maps
                    .insert(TextureMap::from_filename(path), relative);
            }
            self.document
                .set_textures(entity, Some(textures))
                .map_err(|e| e.to_string())
        })();
        self.texture_result(result);
    }

    fn texture_result(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => self.request_preview = true,
            Err(error) => self
                .console
                .push(simple_console(Severity::Error, "textures", error)),
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "map slots and controls commit one undoable material edit"
    )]
    pub(super) fn texture_inspector(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &engine_world::EntitySnapshot,
    ) {
        let renderable = snapshot.mesh.is_some() || snapshot.primitive.is_some();
        if !renderable && snapshot.textures.is_none() {
            return;
        }
        ui.separator();
        ui.heading("Surface textures");
        ui.small("All maps are optional. Browse or drop an image onto a slot.");
        let mut textures = snapshot.textures.clone().unwrap_or_default();
        let before = textures.clone();
        let mut remove = false;
        ui.add_enabled_ui(!self.document.is_read_only() && !self.playing, |ui| {
            if renderable && ui.button("Add Texture child").clicked() {
                let result = self
                    .document
                    .add_texture_child(snapshot.id)
                    .map(|_| ())
                    .map_err(|e| e.to_string());
                self.texture_result(result);
            }
            for (group, maps) in [
                ("Standard maps", &TextureMap::ALL[..7]),
                ("Advanced maps", &TextureMap::ALL[7..]),
            ] {
                egui::CollapsingHeader::new(group)
                    .default_open(group == "Standard maps")
                    .show(ui, |ui| {
                        for &kind in maps {
                            ui.push_id(kind, |ui| {
                                ui.label(kind.label());
                                let (row, payload) = ui.dnd_drop_zone::<ViewportDrop, _>(
                                    egui::Frame::group(ui.style()),
                                    |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                textures
                                                    .maps
                                                    .get(&kind)
                                                    .map_or("None", String::as_str),
                                            );
                                            if ui.button("Browse…").clicked()
                                                && let Some(path) = rfd::FileDialog::new()
                                                    .set_directory(self.project_root)
                                                    .add_filter(
                                                        "Texture image",
                                                        &["png", "jpg", "jpeg", "tga", "hdr"],
                                                    )
                                                    .pick_file()
                                            {
                                                match import_texture(self.project_root, &path) {
                                                    Ok(path) => {
                                                        textures.maps.insert(kind, path);
                                                    }
                                                    Err(error) => self.texture_result(Err(error)),
                                                }
                                            }
                                            if textures.maps.contains_key(&kind)
                                                && ui.button("Clear").clicked()
                                            {
                                                textures.maps.remove(&kind);
                                            }
                                        });
                                    },
                                );
                                let mut path = payload.as_deref().and_then(|p| {
                                    if let ViewportDrop::File(p) = p {
                                        Some(p.clone())
                                    } else {
                                        None
                                    }
                                });
                                if row.response.contains_pointer() {
                                    let files = ui
                                        .ctx()
                                        .input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
                                    path = path.or_else(|| {
                                        files.into_iter().next().map(|f| f.path().to_path_buf())
                                    });
                                }
                                if let Some(path) = path {
                                    match import_texture(self.project_root, &path) {
                                        Ok(path) => {
                                            textures.maps.insert(kind, path);
                                        }
                                        Err(error) => self.texture_result(Err(error)),
                                    }
                                }
                            });
                        }
                    });
            }
            transform_texture_pair(ui, "UV scale", &mut textures.uv_scale);
            transform_texture_pair(ui, "UV offset", &mut textures.uv_offset);
            ui.add(
                egui::DragValue::new(&mut textures.height_scale)
                    .speed(0.001)
                    .range(-100.0..=100.0)
                    .prefix("Displacement "),
            );
            ui.add(
                egui::DragValue::new(&mut textures.emissive_strength)
                    .speed(0.1)
                    .range(0.0..=10000.0)
                    .prefix("Emissive strength "),
            );
            if renderable && snapshot.textures.is_some() {
                remove = ui.button("Remove direct textures").clicked();
            }
        });
        if remove || textures != before {
            let result = self
                .document
                .set_textures(snapshot.id, if remove { None } else { Some(textures) })
                .map_err(|e| e.to_string());
            self.texture_result(result);
        }
        if renderable {
            ui.small("Texture children override supplied slots on this object only.");
        }
    }
}
fn transform_texture_pair(ui: &mut egui::Ui, label: &str, values: &mut [f32; 2]) {
    ui.horizontal(|ui| {
        ui.label(label);
        for value in values {
            ui.add(egui::DragValue::new(value).speed(0.01));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_textures_are_copied_and_invalid_images_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let file = external.path().join("brick_normal.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([128u8, 128, 255, 255]))
            .save(&file)
            .unwrap();
        let path = import_texture(root.path(), &file).unwrap();
        assert!(path.starts_with("assets/textures/"));
        assert_eq!(
            std::fs::read(&file).unwrap(),
            std::fs::read(root.path().join(&path)).unwrap()
        );
        assert_eq!(import_texture(root.path(), &file).unwrap(), path);
        assert_eq!(import_texture(root.path(), Path::new(&path)).unwrap(), path);
        let invalid = external.path().join("broken.hdr");
        std::fs::write(&invalid, b"not an image").unwrap();
        assert!(import_texture(root.path(), &invalid).is_err());
    }
}
