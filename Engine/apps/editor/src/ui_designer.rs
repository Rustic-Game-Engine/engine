//! Native, opt-in HTML layout authoring. Never executes project JavaScript.
use eframe::egui;
use engine_platform::AtomicFileService;
use kuchikiki::{NodeRef, traits::TendrilSink};
use num_traits::ToPrimitive as _;
use std::path::{Path, PathBuf};
use transform_gizmo_egui::{
    Gizmo, GizmoConfig, GizmoExt, GizmoMode,
    math::{DMat4, DQuat, DVec3, Transform},
};

const LIMIT: usize = 1024 * 1024;

pub fn supported(path: &Path) -> bool {
    path.components()
        .next()
        .is_some_and(|part| part.as_os_str() == "ui")
        && path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "html" | "htm" | "css"))
}

fn checked_path(root: &Path, path: &Path) -> Result<PathBuf, String> {
    if !supported(path) {
        return Err("Choose an HTML or CSS file inside ui/".into());
    }
    let ui_root = root.join("ui").canonicalize().map_err(|e| e.to_string())?;
    let project = root.canonicalize().map_err(|e| e.to_string())?;
    let target = root.join(path).canonicalize().map_err(|e| e.to_string())?;
    if !ui_root.starts_with(&project) || !target.starts_with(&ui_root) {
        return Err("UI file must remain inside the project's ui folder".into());
    }
    Ok(target)
}

fn read_source(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut source = String::new();
    file.take((LIMIT + 1) as u64)
        .read_to_string(&mut source)
        .map_err(|e| e.to_string())?;
    if source.len() > LIMIT {
        return Err("UI source exceeds 1 MiB".into());
    }
    Ok(source)
}

fn parse(source: &str) -> NodeRef {
    kuchikiki::parse_html().one(source).document_node
}

fn elements(document: &NodeRef) -> Vec<NodeRef> {
    document
        .select("body *")
        .into_iter()
        .flatten()
        .filter(|node| {
            !matches!(
                node.name.local.as_ref(),
                "script" | "style" | "link" | "meta" | "template"
            )
        })
        .filter(|node| {
            !node.as_node().ancestors().any(|parent| {
                parent
                    .as_element()
                    .is_some_and(|e| e.name.local.as_ref() == "template")
            })
        })
        .map(|node| node.as_node().clone())
        .collect()
}

// Split only outside quoted strings and parentheses; preserve unrelated declarations.
fn declarations(source: &str) -> Vec<String> {
    let mut result = Vec::new();
    let (mut start, mut quote, mut depth, mut escaped) = (0, None, 0u32, false);
    for (index, ch) in source.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => {
                result.push(source[start..index].trim().to_owned());
                start = index + 1;
            }
            _ => {}
        }
    }
    result.push(source[start..].trim().to_owned());
    result.into_iter().filter(|s| !s.is_empty()).collect()
}

fn style(node: &NodeRef, key: &str) -> String {
    node.as_element()
        .and_then(|e| e.attributes.borrow().get("style").map(str::to_owned))
        .map(|s| declarations(&s))
        .unwrap_or_default()
        .iter()
        .rev()
        .find_map(|s| {
            s.split_once(':')
                .filter(|(k, _)| k.trim().eq_ignore_ascii_case(key))
                .map(|(_, v)| v.trim().to_owned())
        })
        .unwrap_or_default()
}

fn set_style(node: &NodeRef, key: &str, value: &str) {
    let Some(element) = node.as_element() else {
        return;
    };
    let mut attrs = element.attributes.borrow_mut();
    let mut values = declarations(attrs.get("style").unwrap_or_default());
    values.retain(|s| {
        s.split_once(':')
            .is_none_or(|(k, _)| !k.trim().eq_ignore_ascii_case(key))
    });
    if !value.trim().is_empty() {
        values.push(format!("{key}: {}", value.trim()));
    }
    attrs.insert("style", values.join("; "));
}

fn number(node: &NodeRef, key: &str, default: f64) -> f64 {
    let value = style(node, key);
    value
        .trim_end_matches("px")
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .unwrap_or(default)
}

fn label(node: &NodeRef) -> String {
    let Some(element) = node.as_element() else {
        return String::new();
    };
    let attrs = element.attributes.borrow();
    format!(
        "{}{}",
        element.name.local,
        attrs.get("id").map_or(String::new(), |id| format!("#{id}"))
    )
}

struct Document {
    root: PathBuf,
    relative: PathBuf,
    saved: String,
    source: String,
    undo: Vec<String>,
    redo: Vec<String>,
}

impl Document {
    fn open(root: &Path, relative: &Path) -> Result<Self, String> {
        let source = read_source(&checked_path(root, relative)?)?;
        Ok(Self {
            root: root.to_path_buf(),
            relative: relative.to_path_buf(),
            saved: source.clone(),
            source,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }
    fn dirty(&self) -> bool {
        self.source != self.saved
    }
    fn change(&mut self, source: String) {
        if source == self.source {
            return;
        }
        self.undo.push(std::mem::replace(&mut self.source, source));
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    fn undo(&mut self) {
        if let Some(source) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.source, source));
        }
    }
    fn redo(&mut self) {
        if let Some(source) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.source, source));
        }
    }
    fn save(&mut self) -> Result<(), String> {
        let path = checked_path(&self.root, &self.relative)?;
        if read_source(&path)? != self.saved {
            return Err("File changed on disk. Keep your changes open, or discard and reopen to load the external version.".into());
        }
        if self.source.len() > LIMIT {
            return Err("UI source exceeds 1 MiB".into());
        }
        AtomicFileService::new()
            .write(&path, self.source.as_bytes())
            .map_err(|e| e.to_string())?;
        self.saved.clone_from(&self.source);
        Ok(())
    }
}

#[derive(Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "tool selection, inspector routing, and independent confirmation states"
)]
pub struct UiDesigner {
    document: Option<Document>,
    selected: Option<usize>,
    gizmo: Gizmo,
    scale_tool: bool,
    gesture_before: Option<String>,
    pub properties_active: bool,
    error: Option<String>,
    candidates: Vec<PathBuf>,
    candidate_root: PathBuf,
    confirm_discard: bool,
    close_requested: bool,
}

impl UiDesigner {
    pub fn dirty(&self) -> bool {
        self.document.as_ref().is_some_and(Document::dirty)
    }

    pub fn open(&mut self, root: &Path, path: &Path) -> Result<(), String> {
        if self.dirty() {
            return Err("Save or discard the open UI document before opening another file.".into());
        }
        let target = checked_path(root, path)?;
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("css"))
        {
            let mut candidates = Vec::new();
            find_html(&root.join("ui"), &mut candidates);
            self.candidates = candidates
                .into_iter()
                .filter(|html| {
                    read_source(html).is_ok_and(|source| {
                        parse(&source)
                            .select("link[href]")
                            .into_iter()
                            .flatten()
                            .any(|link| {
                                let attrs = link.attributes.borrow();
                                let href = attrs
                                    .get("href")
                                    .unwrap_or_default()
                                    .split(['?', '#'])
                                    .next()
                                    .unwrap_or_default();
                                html.parent()
                                    .unwrap_or(root)
                                    .join(href)
                                    .canonicalize()
                                    .ok()
                                    .as_ref()
                                    == Some(&target)
                            })
                    })
                })
                .filter_map(|html| html.strip_prefix(root).ok().map(Path::to_path_buf))
                .collect();
            self.candidate_root = root.to_path_buf();
            if self.candidates.is_empty() {
                return Err("No HTML document in ui/ references this stylesheet. Add a <link href=\"...\"> to an HTML document, then open it visually.".into());
            }
            self.document = None;
        } else {
            self.document = Some(Document::open(root, path)?);
            self.candidates.clear();
        }
        self.selected = None;
        self.properties_active = true;
        self.gizmo = Gizmo::default();
        self.error = None;
        Ok(())
    }

    pub fn finish_gesture(&mut self, context: &egui::Context) {
        if !context.input(|i| i.pointer.primary_down())
            && let Some(before) = self.gesture_before.take()
            && let Some(document) = &mut self.document
            && before != document.source
        {
            document.undo.push(before);
            document.redo.clear();
        }
    }

    pub fn guard_close(&mut self, ui: &mut egui::Ui) {
        if self.dirty() && ui.input(|i| i.viewport().close_requested()) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_requested = true;
        }
        if self.close_requested {
            egui::Window::new("Unsaved UI changes")
                .collapsible(false)
                .show(ui, |ui| {
                    ui.label("Save the UI document before closing the editor?");
                    ui.horizontal(|ui| {
                        if ui.button("Save and close").clicked() {
                            match self.document.as_mut().map(Document::save).transpose() {
                                Ok(_) => {
                                    self.close_requested = false;
                                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                                Err(error) => self.error = Some(error),
                            }
                        }
                        if ui.button("Discard and close").clicked() {
                            self.document = None;
                            self.close_requested = false;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui.button("Cancel").clicked() {
                            self.close_requested = false;
                        }
                    });
                    if let Some(error) = &self.error {
                        ui.label(error);
                    }
                });
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "canvas toolbar, painting, and gizmo share one UI lifetime"
    )]
    pub fn canvas(&mut self, ui: &mut egui::Ui, editable: bool) {
        if !self.candidates.is_empty() {
            ui.label("Choose an HTML document that references this stylesheet:");
            let choice = self
                .candidates
                .iter()
                .find(|path| ui.button(path.display().to_string()).clicked())
                .cloned();
            if let Some(path) = choice {
                let root = self.candidate_root.clone();
                if let Err(error) = self.open(&root, &path) {
                    self.error = Some(error);
                }
            }
            return;
        }
        let Some(document) = &mut self.document else {
            ui.label("Right-click an HTML or CSS file inside ui/ and choose Edit UI Visually.");
            return;
        };
        ui.label(format!(
            "{}{}",
            document.relative.display(),
            if document.dirty() { " *" } else { "" }
        ));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.scale_tool, false, "Move");
            ui.selectable_value(&mut self.scale_tool, true, "Scale");
            if ui
                .add_enabled(editable && document.dirty(), egui::Button::new("Save UI"))
                .clicked()
            {
                self.error = document.save().err();
            }
            if ui
                .add_enabled(
                    editable && !document.undo.is_empty(),
                    egui::Button::new("Undo UI"),
                )
                .clicked()
            {
                document.undo();
                self.selected = None;
            }
            if ui
                .add_enabled(
                    editable && !document.redo.is_empty(),
                    egui::Button::new("Redo UI"),
                )
                .clicked()
            {
                document.redo();
                self.selected = None;
            }
            if ui.button("Scene properties").clicked() {
                self.properties_active = false;
            }
            if ui.button("Discard changes").clicked() {
                self.confirm_discard = true;
            }
        });
        if self.confirm_discard {
            ui.horizontal(|ui| {
                ui.label("Discard all unsaved UI changes?");
                if ui.button("Discard").clicked() {
                    document.source.clone_from(&document.saved);
                    document.undo.clear();
                    document.redo.clear();
                    self.selected = None;
                    self.confirm_discard = false;
                }
                if ui.button("Keep editing").clicked() {
                    self.confirm_discard = false;
                }
            });
        }
        ui.weak("Layout preview · inline pixel positions/sizes · scripts are not executed");
        if let Some(error) = &self.error {
            ui.label(error);
        }
        let parsed = parse(&document.source);
        let nodes = elements(&parsed);
        let preview = preview_tree(document);
        let preview_nodes = elements(&preview);
        let (response, painter) = ui.allocate_painter(
            ui.available_size().max(egui::vec2(100.0, 100.0)),
            egui::Sense::click(),
        );
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
        let zoom = (rect.width() / 1280.0).min(rect.height() / 720.0).min(1.0);
        let origin = rect.left_top();
        let positions = layout(&preview_nodes);
        let order = paint_order(&preview_nodes);
        let mut clicked = None;
        for index in order {
            let node = &preview_nodes[index];
            if node
                .inclusive_ancestors()
                .any(|ancestor| style(&ancestor, "display") == "none")
            {
                continue;
            }
            let [x, y, width, height] = positions[index];
            let bounds = egui::Rect::from_min_size(
                origin + egui::vec2(pixels(x), pixels(y)) * zoom,
                egui::vec2(pixels(width), pixels(height)) * zoom,
            );
            let fill = css_color(&style(node, "background-color"))
                .unwrap_or(ui.visuals().widgets.inactive.bg_fill);
            painter.rect_filled(bounds, 0.0, fill);
            painter.rect_stroke(
                bounds,
                0.0,
                if self.selected == Some(index) {
                    ui.visuals().selection.stroke
                } else {
                    ui.visuals().widgets.inactive.bg_stroke
                },
                egui::StrokeKind::Inside,
            );
            let text = if node.children().all(|child| child.as_text().is_some()) {
                node.text_contents()
            } else {
                label(node)
            };
            painter.with_clip_rect(bounds.intersect(rect)).text(
                bounds.left_top() + egui::vec2(4.0, 4.0),
                egui::Align2::LEFT_TOP,
                text,
                egui::FontId::proportional(
                    (pixels(number(node, "font-size", 16.0)) * zoom).max(6.0),
                ),
                css_color(&style(node, "color")).unwrap_or(ui.visuals().text_color()),
            );
            if response.clicked()
                && !self.gizmo.is_focused()
                && response
                    .interact_pointer_pos()
                    .is_some_and(|p| bounds.contains(p))
            {
                clicked = Some(index);
            }
        }
        if let Some(index) = clicked {
            self.properties_active = true;
            self.selected = Some(index);
            self.gizmo = Gizmo::default();
        }
        if !editable {
            return;
        }
        let Some(index) = self.selected.filter(|index| *index < nodes.len()) else {
            return;
        };
        let [x, y, width, height] = positions[index];
        let projection = DMat4::orthographic_rh(
            0.0,
            f64::from(rect.width() / zoom),
            -f64::from(rect.height() / zoom),
            0.0,
            -1000.0,
            1000.0,
        );
        self.gizmo.update_config(GizmoConfig {
            view_matrix: DMat4::IDENTITY.into(),
            projection_matrix: projection.into(),
            viewport: rect,
            modes: if self.scale_tool {
                [GizmoMode::ScaleX, GizmoMode::ScaleY, GizmoMode::ScaleXY].into()
            } else {
                [
                    GizmoMode::TranslateX,
                    GizmoMode::TranslateY,
                    GizmoMode::TranslateXY,
                ]
                .into()
            },
            ..Default::default()
        });
        let target = Transform::from_scale_rotation_translation(
            DVec3::new(width, height, 1.0),
            DQuat::IDENTITY,
            DVec3::new(x + width / 2.0, -y - height / 2.0, 0.0),
        );
        if let Some((_, transforms)) = self.gizmo.interact(ui, &[target])
            && let Some(after) = transforms.first()
        {
            self.gesture_before
                .get_or_insert_with(|| document.source.clone());
            let node = &nodes[index];
            let parent = parent_offset(&nodes, index, &positions);
            set_style(node, "position", "absolute");
            set_style(node, "left", &format!("{}px", x - parent.0));
            set_style(node, "top", &format!("{}px", y - parent.1));
            if self.scale_tool {
                let scale = DVec3::from(after.scale);
                set_style(
                    node,
                    "width",
                    &format!("{}px", scale.x.abs().clamp(1.0, 100_000.0)),
                );
                set_style(
                    node,
                    "height",
                    &format!("{}px", scale.y.abs().clamp(1.0, 100_000.0)),
                );
            } else {
                let translation = DVec3::from(after.translation);
                let parent = parent_offset(&nodes, index, &positions);
                set_style(
                    node,
                    "left",
                    &format!("{}px", translation.x - width / 2.0 - parent.0),
                );
                set_style(
                    node,
                    "top",
                    &format!("{}px", -translation.y - height / 2.0 - parent.1),
                );
            }
            document.source = parsed.to_string();
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "property widgets and their authoring transaction share the selected HTML node"
    )]
    pub fn properties(&mut self, ui: &mut egui::Ui, editable: bool) {
        ui.heading("UI Properties");
        if ui.button("Show scene properties").clicked() {
            self.properties_active = false;
        }
        let Some(document) = &mut self.document else {
            ui.label("Open an HTML document in Visual UI.");
            return;
        };
        let parsed = parse(&document.source);
        let nodes = elements(&parsed);
        let preview = preview_tree(document);
        let preview_nodes = elements(&preview);
        let mut changed = false;
        ui.add_enabled_ui(editable, |ui| {
            ui.horizontal(|ui| {
                for tag in ["div", "button", "span", "img"] {
                    if ui.button(format!("Add {tag}")).clicked() {
                        let fragment = parse(&format!("<body><{tag} style=\"position: absolute; left: 20px; top: 20px; width: 160px; height: 48px\">{}</{tag}>", if tag == "img" { "" } else { "New element" }));
                        if let (Ok(body), Some(node)) = (parsed.select_first("body"), elements(&fragment).first()) {
                            body.as_node().append(node.clone());
                            self.selected = Some(nodes.len()); changed = true;
                        }
                    }
                }
            });
            ui.collapsing("Layers (back to front within each parent)", |ui| {
                for index in paint_order(&preview_nodes) {
                    let node = &nodes[index];
                    let depth = node.ancestors().count().saturating_sub(3);
                    if ui.selectable_label(self.selected == Some(index), format!("{}{} · z {}", "  ".repeat(depth), label(node), style(&preview_nodes[index], "z-index"))).clicked() {
                        self.selected = Some(index); self.gizmo = Gizmo::default();
                    }
                }
            });
            let Some(index) = self.selected.filter(|i| *i < nodes.len()) else { ui.label("Select an element on the canvas or in Layers."); return; };
            let node = &nodes[index];
            ui.label(label(node));
            let element = node.as_element().expect("element list contains only elements");
            for key in ["id", "class", "src", "alt"] {
                if matches!(key, "src" | "alt") && element.name.local.as_ref() != "img" { continue; }
                let mut value = element.attributes.borrow().get(key).unwrap_or_default().to_owned();
                ui.horizontal(|ui| {
                    ui.label(key);
                    if ui.text_edit_singleline(&mut value).changed() { element.attributes.borrow_mut().insert(key, value); changed = true; }
                });
            }
            if node.children().all(|child| child.as_text().is_some()) && element.name.local.as_ref() != "img" {
                let mut text = node.text_contents();
                ui.label("Text");
                if ui.text_edit_multiline(&mut text).changed() {
                    for child in node.children().collect::<Vec<_>>() { child.detach(); }
                    node.append(NodeRef::new_text(text)); changed = true;
                }
            }
            for key in ["left", "top", "width", "height", "z-index", "font-size"] {
                let mut value = number(&preview_nodes[index], key, match key { "width" => 160.0, "height" => 48.0, "font-size" => 16.0, _ => 0.0 });
                ui.horizontal(|ui| {
                    ui.label(key);
                    if ui.add(egui::DragValue::new(&mut value).speed(1.0).clamp_existing_to_range(false).range(if matches!(key, "width" | "height" | "font-size") { 1.0..=100_000.0 } else { -100_000.0..=100_000.0 })).changed() {
                        set_style(node, key, &if key == "z-index" { format!("{value:.0}") } else { format!("{value}px") });
                        if matches!(key, "left" | "top" | "z-index") { set_style(node, "position", "absolute"); }
                        changed = true;
                    }
                });
            }
            for key in ["background-color", "color"] {
                let mut value = css_color(&style(&preview_nodes[index], key)).unwrap_or(egui::Color32::TRANSPARENT);
                ui.horizontal(|ui| {
                    ui.label(key);
                    if ui.color_edit_button_srgba(&mut value).changed() {
                        set_style(node, key, &css_hex(value)); changed = true;
                    }
                });
            }
            let mut hidden = style(&preview_nodes[index], "display") == "none";
            if ui.checkbox(&mut hidden, "Hidden").changed() { set_style(node, "display", if hidden { "none" } else { "block" }); changed = true; }
            ui.horizontal(|ui| {
                for (caption, direction) in [("Send to back", -1.0), ("Bring to front", 1.0)] {
                    if ui.button(caption).clicked() {
                        let siblings: Vec<_> = preview_nodes.iter().filter(|other| other.parent() == preview_nodes[index].parent()).collect();
                        let edge = siblings.iter().map(|other| number(other, "z-index", 0.0)).fold(number(&preview_nodes[index], "z-index", 0.0), |a,b| if direction > 0.0 { a.max(b) } else { a.min(b) });
                        set_style(node, "position", "absolute"); set_style(node, "z-index", &format!("{:.0}", edge + direction)); changed = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Duplicate").clicked() {
                    let fragment = parse(&format!("<body>{node}</body>"));
                    if let Some(copy) = elements(&fragment).first() {
                        for node in copy.inclusive_descendants() {
                            if let Some(e) = node.as_element() { e.attributes.borrow_mut().remove("id"); }
                        }
                        node.insert_after(copy.clone()); changed = true; self.selected = None;
                    }
                }
                if ui.button("Delete element").clicked() { node.detach(); self.selected = None; changed = true; }
            });
        });
        if changed {
            document.change(parsed.to_string());
        }
    }
}

// Supported flat stylesheet rules are a layout aid, not a browser CSS engine.
// Inline declarations take precedence; the source tree is never modified here.
fn preview_tree(document: &Document) -> NodeRef {
    let tree = parse(&document.source);
    let nodes = elements(&tree);
    let inline: Vec<_> = nodes
        .iter()
        .map(|node| {
            node.as_element()
                .and_then(|e| e.attributes.borrow().get("style").map(str::to_owned))
                .unwrap_or_default()
        })
        .collect();
    let mut sheets = Vec::new();
    for node in tree.descendants() {
        let Some(element) = node.as_element() else {
            continue;
        };
        if element.name.local.as_ref() == "style" {
            sheets.push(node.text_contents());
        }
        if element.name.local.as_ref() == "link" {
            let attrs = element.attributes.borrow();
            if let Some(href) = attrs.get("href") {
                let relative = document
                    .relative
                    .parent()
                    .unwrap_or(Path::new("ui"))
                    .join(href);
                if let Ok(path) = checked_path(&document.root, &relative)
                    && path
                        .extension()
                        .is_some_and(|s| s.eq_ignore_ascii_case("css"))
                    && let Ok(source) = read_source(&path)
                {
                    sheets.push(source);
                }
            }
        }
    }
    for source in sheets {
        for (selector, body) in flat_rules(&source) {
            if let Ok(matches) = tree.select(&selector) {
                for node in matches {
                    for declaration in declarations(&body) {
                        if let Some((key, value)) = declaration.split_once(':') {
                            set_style(node.as_node(), key.trim(), value);
                        }
                    }
                }
            }
        }
    }
    for (node, source) in nodes.iter().zip(inline) {
        for declaration in declarations(&source) {
            if let Some((key, value)) = declaration.split_once(':') {
                set_style(node, key.trim(), value);
            }
        }
    }
    tree
}

fn flat_rules(source: &str) -> Vec<(String, String)> {
    // Ignore nested at-rules and braces inside comments/strings.
    let (mut clean, mut rest) = (String::new(), source);
    while let Some(start) = rest.find("/*") {
        clean.push_str(&rest[..start]);
        let Some(end) = rest[start + 2..].find("*/") else {
            rest = "";
            break;
        };
        rest = &rest[start + 2 + end + 2..];
    }
    clean.push_str(rest);
    let (mut depth, mut quote, mut escaped, mut start, mut body_start) = (0u32, None, false, 0, 0);
    let mut rules = Vec::new();
    for (index, ch) in clean.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '{' => {
                if depth == 0 {
                    body_start = index + 1;
                }
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && body_start > start {
                    let selector = clean[start..body_start - 1].trim();
                    let body = &clean[body_start..index];
                    if !selector.starts_with('@') && !body.contains('{') {
                        rules.push((selector.to_owned(), body.to_owned()));
                    }
                    start = index + 1;
                }
            }
            _ => {}
        }
    }
    rules
}

fn paint_order(nodes: &[NodeRef]) -> Vec<usize> {
    fn visit(
        parent: Option<usize>,
        nodes: &[NodeRef],
        parents: &[Option<usize>],
        result: &mut Vec<usize>,
    ) {
        let mut children: Vec<_> = parents
            .iter()
            .enumerate()
            .filter_map(|(i, p)| (*p == parent).then_some(i))
            .collect();
        children.sort_by(|a, b| {
            number(&nodes[*a], "z-index", 0.0).total_cmp(&number(&nodes[*b], "z-index", 0.0))
        });
        for child in children {
            result.push(child);
            visit(Some(child), nodes, parents, result);
        }
    }
    let parents: Vec<_> = nodes
        .iter()
        .map(|node| {
            node.ancestors()
                .find_map(|parent| nodes.iter().position(|candidate| *candidate == parent))
        })
        .collect();
    let mut result = Vec::new();
    visit(None, nodes, &parents, &mut result);
    result
}

fn parent_offset(nodes: &[NodeRef], index: usize, positions: &[[f64; 4]]) -> (f64, f64) {
    for parent in nodes[index].ancestors() {
        if let Some(i) = nodes[..index].iter().position(|node| *node == parent) {
            return (positions[i][0], positions[i][1]);
        }
    }
    (0.0, 0.0)
}

fn layout(nodes: &[NodeRef]) -> Vec<[f64; 4]> {
    let mut result = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        let parent = parent_offset(nodes, index, &result);
        result.push([
            parent.0 + number(node, "left", 0.0),
            parent.1
                + number(
                    node,
                    "top",
                    f64::from(u32::try_from(index).unwrap_or(u32::MAX)) * 36.0,
                ),
            number(node, "width", 160.0).max(1.0),
            number(node, "height", 48.0).max(1.0),
        ]);
    }
    result
}

fn pixels(value: f64) -> f32 {
    value
        .clamp(-1_000_000.0, 1_000_000.0)
        .to_f32()
        .unwrap_or_default()
}

fn css_hex(color: egui::Color32) -> String {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
}

fn css_color(value: &str) -> Option<egui::Color32> {
    let hex = value.strip_prefix('#')?;
    let value = u32::from_str_radix(hex, 16).ok()?;
    let [a, b, c, d] = value.to_be_bytes();
    match hex.len() {
        6 => Some(egui::Color32::from_rgb(b, c, d)),
        8 => Some(egui::Color32::from_rgba_unmultiplied(a, b, c, d)),
        _ => None,
    }
}

fn find_html(directory: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            find_html(&entry.path(), paths);
        } else if kind.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("html") || s.eq_ignore_ascii_case("htm"))
        {
            paths.push(entry.path());
        }
    }
    paths.sort();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_preserve_scripts_styles_attributes_and_nested_content() {
        let html = "<!doctype html><html><head><style>.hud{color:red}</style></head><body><div id='hud' data-extra='keep' style='background-image:url(\"a;b.png\");left:1px;left:2px'><button>Score &amp; lives</button></div><script>globalThis.behavior={Update(dt){rustic.log('info', '<tag>');}};</script></body></html>";
        let parsed = parse(html);
        let nodes = elements(&parsed);
        assert_eq!(nodes.len(), 2);
        set_style(&nodes[0], "left", "42px");
        let reopened = parse(&parsed.to_string());
        assert_eq!(style(&elements(&reopened)[0], "left"), "42px");
        assert!(style(&elements(&reopened)[0], "background-image").contains("a;b.png"));
        assert_eq!(
            reopened.select_first("script").unwrap().text_contents(),
            parsed.select_first("script").unwrap().text_contents()
        );
        assert_eq!(
            reopened.select_first("button").unwrap().text_contents(),
            "Score & lives"
        );
        assert_eq!(
            reopened
                .select_first("#hud")
                .unwrap()
                .attributes
                .borrow()
                .get("data-extra"),
            Some("keep")
        );
    }
    #[test]
    fn history_and_parented_layout_survive_round_trip() {
        let source =
            "<div style='left:10px;top:20px'><button style='left:4px;top:8px'>Hi</button></div>";
        let nodes = elements(&parse(source));
        let positions = layout(&nodes);
        assert_eq!(&positions[1][..2], &[14.0, 28.0]);
        let mut document = Document {
            root: PathBuf::new(),
            relative: PathBuf::new(),
            saved: source.into(),
            source: source.into(),
            undo: vec![],
            redo: vec![],
        };
        document.change("changed".into());
        assert!(document.dirty());
        document.undo();
        assert!(!document.dirty());
        document.redo();
        assert_eq!(document.source, "changed");
        document.undo();
        document.change("new branch".into());
        assert!(document.redo.is_empty());
    }
    #[test]
    fn eligibility_is_limited_to_web_assets_under_ui() {
        for path in ["ui/hud.html", "ui/nested/HUD.HTM", "ui/hud.css"] {
            assert!(supported(Path::new(path)));
        }
        for path in ["scripts/hud.html", "ui.js", "ui/game.js", "/ui/hud.html"] {
            assert!(!supported(Path::new(path)));
        }
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    fn fixture() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("ui")).unwrap();
        std::fs::write(temp.path().join("ui/hud.html"), "<link rel='stylesheet' href='hud.css'><div id='hud'>Hello</div><script>globalThis.behavior={};</script>").unwrap();
        std::fs::write(
            temp.path().join("ui/hud.css"),
            "/* layout */ #hud { left: 12px; width: 210px; } @media print { #hud { width: 2px; } }",
        )
        .unwrap();
        temp
    }
    #[test]
    fn stylesheet_preview_does_not_bake_styles_into_saved_html() {
        let temp = fixture();
        let document = Document::open(temp.path(), Path::new("ui/hud.html")).unwrap();
        let preview = preview_tree(&document);
        assert_eq!(style(&elements(&preview)[0], "width"), "210px");
        assert_eq!(style(&elements(&parse(&document.source))[0], "width"), "");
        let mut designer = UiDesigner::default();
        designer.open(temp.path(), Path::new("ui/hud.css")).unwrap();
        assert_eq!(designer.candidates, vec![PathBuf::from("ui/hud.html")]);
    }
    #[test]
    fn atomic_save_keeps_backup_and_refuses_external_changes() {
        let temp = fixture();
        let path = temp.path().join("ui/hud.html");
        let mut document = Document::open(temp.path(), Path::new("ui/hud.html")).unwrap();
        let original = document.source.clone();
        document.change("<button>Changed</button>".into());
        document.save().unwrap();
        assert!(!document.dirty());
        assert_eq!(
            std::fs::read_to_string(AtomicFileService::backup_path(&path)).unwrap(),
            original
        );
        std::fs::write(&path, "external edit").unwrap();
        document.change("<button>Another edit</button>".into());
        assert!(document.save().unwrap_err().contains("changed on disk"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "external edit");
        let mut designer = UiDesigner::default();
        designer
            .open(temp.path(), Path::new("ui/hud.html"))
            .unwrap();
        designer.document.as_mut().unwrap().change("unsaved".into());
        assert!(
            designer
                .open(temp.path(), Path::new("ui/hud.html"))
                .is_err()
        );
    }
    #[cfg(unix)]
    #[test]
    fn traversal_and_symlinks_cannot_escape_ui_root() {
        let temp = fixture();
        std::fs::write(temp.path().join("outside.html"), "outside").unwrap();
        assert!(Document::open(temp.path(), Path::new("ui/../outside.html")).is_err());
        std::os::unix::fs::symlink(
            temp.path().join("outside.html"),
            temp.path().join("ui/escape.html"),
        )
        .unwrap();
        assert!(Document::open(temp.path(), Path::new("ui/escape.html")).is_err());
    }
    #[test]
    fn native_canvas_and_properties_work_in_both_themes_and_read_only() {
        let temp = fixture();
        for dark in [true, false] {
            for editable in [true, false] {
                let context = egui::Context::default();
                context.set_visuals(if dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
                let mut designer = UiDesigner::default();
                designer
                    .open(temp.path(), Path::new("ui/hud.html"))
                    .unwrap();
                designer.selected = Some(0);
                let mut output = context.run_ui(egui::RawInput::default(), |ui| {
                    designer.canvas(ui, editable);
                    designer.properties(ui, editable);
                });
                output.textures_delta.clear();
                assert!(!designer.dirty());
            }
        }
    }
}

#[cfg(test)]
mod gesture_tests {
    use super::*;

    fn frame(
        context: &egui::Context,
        designer: &mut UiDesigner,
        events: Vec<egui::Event>,
        editable: bool,
    ) -> egui::FullOutput {
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 900.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                designer.finish_gesture(ui.ctx());
                designer.canvas(ui, editable);
            },
        );
        output.textures_delta.clear();
        output
    }
    fn button(position: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }
    #[test]
    fn move_and_scale_handles_commit_one_undo_step_and_obey_read_only() {
        for scale in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            std::fs::create_dir(temp.path().join("ui")).unwrap();
            let source = "<div style='position:absolute;left:100px;top:100px;width:200px;height:100px'>Target</div>";
            std::fs::write(temp.path().join("ui/hud.html"), source).unwrap();
            let mut designer = UiDesigner::default();
            designer
                .open(temp.path(), Path::new("ui/hud.html"))
                .unwrap();
            designer.selected = Some(0);
            designer.scale_tool = scale;
            let context = egui::Context::default();
            let output = frame(&context, &mut designer, vec![], true);
            let mut bounds = None;
            for shape in output.shapes {
                match shape.shape {
                    egui::epaint::Shape::Rect(rect) => bounds = Some(rect.rect),
                    egui::epaint::Shape::Text(text) if text.galley.job.text == "Target" => break,
                    _ => {}
                }
            }
            let center = bounds.expect("target rectangle").center();
            let start = center + egui::vec2(40.0, 0.0);
            frame(
                &context,
                &mut designer,
                vec![egui::Event::PointerMoved(start)],
                true,
            );
            frame(&context, &mut designer, vec![], true);
            assert!(
                designer.gizmo.is_focused(),
                "upstream handle must own the pointer, scale={scale}"
            );
            frame(&context, &mut designer, vec![button(start, true)], true);
            frame(
                &context,
                &mut designer,
                vec![egui::Event::PointerMoved(start + egui::vec2(24.0, 0.0))],
                true,
            );
            frame(
                &context,
                &mut designer,
                vec![egui::Event::PointerMoved(start + egui::vec2(48.0, 0.0))],
                true,
            );
            frame(
                &context,
                &mut designer,
                vec![button(start + egui::vec2(48.0, 0.0), false)],
                true,
            );
            let document = designer.document.as_mut().unwrap();
            assert!(document.dirty());
            assert_eq!(document.undo.len(), 1);
            let parsed = parse(&document.source);
            let nodes = elements(&parsed);
            assert!(
                number(&nodes[0], if scale { "width" } else { "left" }, 0.0)
                    > if scale { 200.0 } else { 100.0 }
            );
            document.undo();
            assert_eq!(document.source, source);
            designer.gizmo = Gizmo::default();
            frame(
                &context,
                &mut designer,
                vec![egui::Event::PointerMoved(start), button(start, true)],
                false,
            );
            frame(
                &context,
                &mut designer,
                vec![egui::Event::PointerMoved(start + egui::vec2(48.0, 0.0))],
                false,
            );
            frame(&context, &mut designer, vec![button(start, false)], false);
            assert!(!designer.dirty());
        }
    }
}
