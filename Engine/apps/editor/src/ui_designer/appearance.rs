use super::{NodeRef, css_color, css_hex, pixels, set_style, style};
use eframe::egui;
use egui::epaint::{Mesh, Vertex};
use std::ops::Add as _;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(super) struct Gradient {
    angle: f32,
    stops: Vec<(f32, egui::Color32)>,
}

impl Gradient {
    fn new(color: egui::Color32) -> Self {
        Self {
            angle: 90.0,
            stops: vec![(0.0, color), (1.0, egui::Color32::WHITE)],
        }
    }
    fn css(&self) -> String {
        format!(
            "linear-gradient({}deg, {})",
            self.angle,
            self.stops
                .iter()
                .map(|(position, color)| format!("{} {}%", css_hex(*color), position * 100.0))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
    pub(super) fn parse(source: &str) -> Option<Self> {
        let body = source
            .trim()
            .strip_prefix("linear-gradient(")?
            .strip_suffix(')')?;
        let mut parts = body.split(',').map(str::trim).peekable();
        let first = *parts.peek()?;
        let angle = if let Some(angle) = first.strip_suffix("deg") {
            parts.next();
            angle.parse::<f32>().ok()?.rem_euclid(360.0)
        } else {
            180.0
        };
        if !angle.is_finite() {
            return None;
        }
        let raw: Vec<_> = parts.collect();
        if raw.len() < 2 {
            return None;
        }
        let mut stops = Vec::new();
        for (index, part) in raw.iter().enumerate() {
            let mut values = part.split_whitespace();
            let color = css_color(values.next()?)?;
            let denominator = u16::try_from(raw.len() - 1).ok()?;
            let offset = f32::from(u16::try_from(index).ok()?) / f32::from(denominator);
            let position = values.next().map_or(Some(offset), |value| {
                value
                    .strip_suffix('%')?
                    .parse::<f32>()
                    .ok()
                    .map(|p| p / 100.0)
            })?;
            if !position.is_finite() || !(0.0..=1.0).contains(&position) || values.next().is_some()
            {
                return None;
            }
            stops.push((position, color));
        }
        stops.sort_by(|a, b| a.0.total_cmp(&b.0));
        Some(Self { angle, stops })
    }
    fn parameter(&self, bounds: egui::Rect, point: egui::Pos2) -> f32 {
        let angle = self.angle.to_radians();
        let direction = egui::vec2(angle.sin(), -angle.cos());
        let extent = direction.x.abs() * bounds.width() + direction.y.abs() * bounds.height();
        (point - bounds.center()).dot(direction) / extent.max(0.001) + 0.5
    }
    fn color(&self, position: f32) -> egui::Color32 {
        let first = self.stops[0];
        if position <= first.0 {
            return first.1;
        }
        for pair in self.stops.windows(2) {
            if position <= pair[1].0 {
                let amount = (position - pair[0].0) / (pair[1].0 - pair[0].0).max(0.00001);
                return egui::Rgba::from(pair[0].1)
                    .multiply(1.0 - amount)
                    .add(egui::Rgba::from(pair[1].1).multiply(amount))
                    .into();
            }
        }
        self.stops.last().expect("at least two stops").1
    }
}

fn node_gradient(node: &NodeRef) -> Option<Gradient> {
    let image = style(node, "background-image");
    if image.is_empty() {
        Gradient::parse(&style(node, "background"))
    } else {
        Gradient::parse(&image)
    }
}

// Split triangles at stops so middle colors and hard transitions survive tessellation.
fn clip(
    vertices: &[Vertex],
    gradient: &Gradient,
    bounds: egui::Rect,
    level: f32,
    keep_above: bool,
) -> Vec<Vertex> {
    let mut result = Vec::new();
    for index in 0..vertices.len() {
        let a = vertices[index];
        let b = vertices[(index + 1) % vertices.len()];
        let ta = gradient.parameter(bounds, a.pos);
        let tb = gradient.parameter(bounds, b.pos);
        let inside = |t: f32| if keep_above { t >= level } else { t <= level };
        if inside(ta) {
            result.push(a);
        }
        if inside(ta) != inside(tb) {
            let t = (level - ta) / (tb - ta);
            result.push(Vertex {
                pos: a.pos.lerp(b.pos, t),
                uv: a.uv.lerp(b.uv, t),
                color: egui::Rgba::from(a.color)
                    .multiply(1.0 - t)
                    .add(egui::Rgba::from(b.color).multiply(t))
                    .into(),
            });
        }
    }
    result
}

fn gradient_mesh(mesh: &Mesh, gradient: &Gradient, bounds: egui::Rect) -> Mesh {
    let mut result = Mesh::with_texture(mesh.texture_id);
    let mut levels = vec![f32::NEG_INFINITY];
    levels.extend(gradient.stops.iter().map(|s| s.0));
    levels.push(f32::INFINITY);
    for triangle in mesh.indices.as_chunks::<3>().0 {
        let vertices = triangle
            .iter()
            .map(|i| mesh.vertices[*i as usize])
            .collect::<Vec<_>>();
        for interval in levels.windows(2) {
            let polygon = clip(
                &clip(&vertices, gradient, bounds, interval[0], true),
                gradient,
                bounds,
                interval[1],
                false,
            );
            if polygon.len() < 3 {
                continue;
            }
            let Ok(start) = u32::try_from(result.vertices.len()) else {
                return result;
            };
            for mut vertex in polygon {
                let coverage = f32::from(vertex.color.a()) / 255.0;
                let parameter = gradient.parameter(bounds, vertex.pos);
                let sample = if parameter <= interval[0] {
                    parameter + 0.00001
                } else if parameter >= interval[1] {
                    parameter - 0.00001
                } else {
                    parameter
                };
                vertex.color = gradient.color(sample).linear_multiply(coverage);
                result.vertices.push(vertex);
            }
            let count = u32::try_from(result.vertices.len()).unwrap_or(start) - start;
            for i in 1..count - 1 {
                result.indices.extend([start, start + i, start + i + 1]);
            }
        }
    }
    result
}

pub(super) fn radii(node: &NodeRef) -> [f64; 4] {
    let shorthand = style(node, "border-radius");
    let values: Vec<_> = shorthand
        .split_whitespace()
        .filter_map(|s| s.strip_suffix("px").unwrap_or(s).parse::<f64>().ok())
        .collect();
    let base = match values.as_slice() {
        [a] => [*a; 4],
        [a, b] => [*a, *b, *a, *b],
        [a, b, c] => [*a, *b, *c, *b],
        [a, b, c, d] => [*a, *b, *c, *d],
        _ => [0.0; 4],
    };
    let mut result = base;
    for (index, key) in CORNERS.iter().enumerate() {
        result[index] = super::number(node, key, base[index]).clamp(0.0, 255.0);
    }
    result
}
const CORNERS: [&str; 4] = [
    "border-top-left-radius",
    "border-top-right-radius",
    "border-bottom-right-radius",
    "border-bottom-left-radius",
];

pub(super) fn corner_radius(node: &NodeRef, zoom: f32) -> egui::CornerRadius {
    use num_traits::ToPrimitive as _;
    let values = radii(node).map(|r| (pixels(r) * zoom).round().to_u8().unwrap_or(255));
    egui::CornerRadius {
        nw: values[0],
        ne: values[1],
        se: values[2],
        sw: values[3],
    }
}

pub(super) fn paint_background(
    painter: &egui::Painter,
    bounds: egui::Rect,
    node: &NodeRef,
    radius: egui::CornerRadius,
    fallback: egui::Color32,
) {
    let text_only = style(node, "background-clip") == "text"
        || style(node, "-webkit-background-clip") == "text";
    if text_only {
        return;
    }
    let gradient = node_gradient(node);
    if let Some(gradient) = gradient {
        let mut mesh = Mesh::default();
        let shape = egui::epaint::RectShape::filled(bounds, radius, egui::Color32::WHITE);
        let mut tessellator = egui::epaint::Tessellator::new(
            painter.ctx().pixels_per_point(),
            egui::epaint::TessellationOptions::default(),
            [1, 1],
            vec![],
        );
        tessellator.tessellate_rect(&shape, &mut mesh);
        painter.add(gradient_mesh(&mesh, &gradient, bounds));
    } else {
        painter.rect_filled(
            bounds,
            radius,
            css_color(&style(node, "background-color")).unwrap_or(fallback),
        );
    }
}

pub(super) fn paint_text(
    painter: &egui::Painter,
    bounds: egui::Rect,
    node: &NodeRef,
    text: String,
    font: egui::FontId,
    fallback: egui::Color32,
) {
    let clipped = painter.with_clip_rect(bounds.intersect(painter.clip_rect()));
    let position = bounds.left_top() + egui::vec2(4.0, 4.0);
    let gradient = (style(node, "background-clip") == "text"
        || style(node, "-webkit-background-clip") == "text")
        .then(|| node_gradient(node))
        .flatten();
    // Text gradients live on a dedicated span, so a parent can retain its own background.
    let text_gradient = node.children().find(|child| {
        child
            .as_element()
            .is_some_and(|e| e.attributes.borrow().get("data-rustic-text-gradient") == Some("true"))
    });
    let gradient = text_gradient
        .as_ref()
        .and_then(|span| Gradient::parse(&style(span, "background-image")))
        .or(gradient);
    if let Some(gradient) = gradient {
        let mut galley = clipped.layout_no_wrap(text, font, egui::Color32::WHITE);
        let gradient_bounds = if text_gradient.is_some() {
            galley.rect.translate(position.to_vec2())
        } else {
            bounds
        };
        for row in &mut Arc::make_mut(&mut galley).rows {
            let offset = row.pos.to_vec2() + position.to_vec2();
            let mut mesh = row.row.visuals.mesh.clone();
            for vertex in &mut mesh.vertices {
                vertex.pos += offset;
            }
            let mut mesh = gradient_mesh(&mesh, &gradient, gradient_bounds);
            for vertex in &mut mesh.vertices {
                vertex.pos -= offset;
            }
            let row = Arc::make_mut(&mut row.row);
            row.visuals.glyph_vertex_range = 0..mesh.vertices.len();
            row.visuals.mesh = mesh;
        }
        let generated = Arc::make_mut(&mut galley);
        generated.num_vertices = generated
            .rows
            .iter()
            .map(|r| r.visuals.mesh.vertices.len())
            .sum();
        generated.num_indices = generated
            .rows
            .iter()
            .map(|r| r.visuals.mesh.indices.len())
            .sum();
        clipped.galley(position, galley, egui::Color32::WHITE);
    } else {
        clipped.text(
            position,
            egui::Align2::LEFT_TOP,
            text,
            font,
            css_color(&style(text_gradient.as_ref().unwrap_or(node), "color"))
                .or_else(|| css_color(&style(node, "color")))
                .unwrap_or(fallback),
        );
    }
}

fn gradient_controls(ui: &mut egui::Ui, gradient: &mut Gradient) -> bool {
    let mut changed = ui
        .add(
            egui::Slider::new(&mut gradient.angle, 0.0..=360.0)
                .text("Angle")
                .suffix("°"),
        )
        .changed();
    let mut remove = None;
    let can_remove = gradient.stops.len() > 2;
    for (index, (position, color)) in gradient.stops.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            ui.horizontal(|ui| {
                changed |= ui.color_edit_button_srgba(color).changed();
                let mut percent = *position * 100.0;
                if ui
                    .add(
                        egui::DragValue::new(&mut percent)
                            .range(0.0..=100.0)
                            .suffix("% position"),
                    )
                    .changed()
                {
                    *position = percent / 100.0;
                    changed = true;
                }
                if ui
                    .add_enabled(can_remove, egui::Button::new("Remove").small())
                    .clicked()
                {
                    remove = Some(index);
                }
            })
        });
    }
    if gradient.stops.len() > 2
        && let Some(index) = remove
    {
        gradient.stops.remove(index);
        changed = true;
    }
    if gradient.stops.len() < 16 && ui.button("Add color stop").clicked() {
        gradient.stops.push((0.5, gradient.color(0.5)));
        changed = true;
    }
    if changed {
        gradient.stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    }
    changed
}

pub(super) fn fill_controls(
    ui: &mut egui::Ui,
    node: &NodeRef,
    preview: &NodeRef,
    text: bool,
) -> bool {
    let key = if text { "color" } else { "background-color" };
    let mut color = css_color(&style(preview, key)).unwrap_or(if text {
        egui::Color32::WHITE
    } else {
        egui::Color32::TRANSPARENT
    });
    let existing = node_gradient(preview);
    let mut gradient_enabled = existing.is_some();
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= ui
            .selectable_value(&mut gradient_enabled, false, "Solid")
            .changed();
        changed |= ui
            .selectable_value(&mut gradient_enabled, true, "Gradient")
            .changed();
    });
    if gradient_enabled {
        let mut gradient = existing.unwrap_or_else(|| Gradient::new(color));
        changed |= gradient_controls(ui, &mut gradient);
        if changed {
            set_style(node, "background-image", &gradient.css());
        }
    } else {
        changed |= ui.color_edit_button_srgba(&mut color).changed();
        if changed {
            set_style(node, key, &css_hex(color));
            set_style(node, "background-image", "none");
        }
    }
    if changed && text {
        for key in ["background-clip", "-webkit-background-clip"] {
            set_style(
                node,
                key,
                if gradient_enabled {
                    "text"
                } else {
                    "border-box"
                },
            );
        }
        let value = if gradient_enabled {
            "transparent".to_owned()
        } else {
            css_hex(color)
        };
        set_style(node, "color", &value);
    }
    changed
}

pub(super) fn radius_controls(
    ui: &mut egui::Ui,
    node: &NodeRef,
    preview: &NodeRef,
    separate: &mut bool,
) -> bool {
    let mut values = radii(preview);
    let mut linked = !*separate && values.iter().all(|r| (*r - values[0]).abs() < 0.001);
    let mut changed = false;
    if ui.checkbox(&mut linked, "Link all corners").changed() {
        *separate = !linked;
        if linked {
            values = [values[0]; 4];
            changed = true;
        }
    }
    if linked {
        if ui
            .add(
                egui::DragValue::new(&mut values[0])
                    .range(0.0..=255.0)
                    .suffix(" px radius"),
            )
            .changed()
        {
            values = [values[0]; 4];
            changed = true;
        }
    } else {
        for (index, title) in ["Top left", "Top right", "Bottom right", "Bottom left"]
            .iter()
            .enumerate()
        {
            ui.horizontal(|ui| {
                ui.label(*title);
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut values[index])
                            .range(0.0..=255.0)
                            .suffix(" px"),
                    )
                    .changed();
            });
        }
    }
    if changed {
        set_style(node, "border-radius", "");
        for (key, value) in CORNERS.iter().zip(values) {
            set_style(node, key, &format!("{value}px"));
        }
    }
    changed
}

pub(super) fn text_target(node: &NodeRef) -> NodeRef {
    node.children()
        .find(is_text_wrapper)
        .unwrap_or_else(|| node.clone())
}

pub(super) fn is_text_wrapper(node: &NodeRef) -> bool {
    node.as_element()
        .is_some_and(|e| e.attributes.borrow().get("data-rustic-text-gradient") == Some("true"))
}

pub(super) fn text_controls(ui: &mut egui::Ui, node: &NodeRef, preview: &NodeRef) -> bool {
    let target = text_target(node);
    let preview_target = text_target(preview);
    if target != *node {
        return fill_controls(ui, &target, &preview_target, true);
    }
    let fragment = super::parse(r#"<span data-rustic-text-gradient="true"></span>"#);
    let wrapper = fragment
        .select_first("span")
        .expect("generated span")
        .as_node()
        .clone();
    set_style(&wrapper, "color", &style(preview, "color"));
    let imported = style(preview, "background-clip") == "text"
        || style(preview, "-webkit-background-clip") == "text";
    if imported {
        let gradient = node_gradient(preview);
        if let Some(gradient) = gradient {
            set_style(&wrapper, "background-image", &gradient.css());
        }
        set_style(&wrapper, "background-clip", "text");
    }
    if !fill_controls(ui, &wrapper, &wrapper.clone(), true) {
        return false;
    }
    if imported {
        for key in [
            "background-image",
            "background",
            "background-clip",
            "-webkit-background-clip",
        ] {
            set_style(node, key, "");
        }
    }
    apply_text_fill(node, wrapper);
    true
}

fn apply_text_fill(node: &NodeRef, wrapper: NodeRef) {
    if style(&wrapper, "background-clip") == "text" {
        for child in node.children().collect::<Vec<_>>() {
            wrapper.append(child);
        }
        node.append(wrapper);
    } else {
        set_style(node, "color", &style(&wrapper, "color"));
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "exact integer pixel radii are preserved through CSS serialization"
)]
mod tests {
    use super::*;
    use crate::ui_designer::{elements, parse};
    #[test]
    fn gradient_stops_angles_alpha_and_mesh_keep_the_middle_color() {
        let gradient =
            Gradient::parse("linear-gradient(90deg, #000000 0%, #ffffff80 50%, #000000 100%)")
                .unwrap();
        assert_eq!(
            Gradient::parse(&gradient.css()).unwrap().css(),
            gradient.css()
        );
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 50.0));
        let mut mesh = Mesh::default();
        mesh.add_colored_rect(rect, egui::Color32::WHITE);
        let colored = gradient_mesh(&mesh, &gradient, rect);
        assert!(colored.is_valid());
        assert!(
            colored
                .vertices
                .iter()
                .any(|v| (v.pos.x - 50.0).abs() < 0.01 && v.color.a() == 128 && v.color.r() > 100)
        );
        assert!(Gradient::parse("linear-gradient(NaNdeg, #000000, #ffffff)").is_none());
    }
    #[test]
    fn background_and_text_gradients_coexist_without_becoming_extra_editor_elements() {
        let tree = parse(
            "<button style='background-image:linear-gradient(90deg,#000000,#ffffff);border-radius:8px'>Hello</button>",
        );
        let node = elements(&tree)[0].clone();
        let fragment = parse(
            "<span data-rustic-text-gradient='true' style='background-image:linear-gradient(180deg,#ff0000,#0000ff);background-clip:text;color:transparent'></span>",
        );
        let wrapper = fragment.select_first("span").unwrap().as_node().clone();
        apply_text_fill(&node, wrapper);
        let reopened = parse(&tree.to_string());
        let nodes = elements(&reopened);
        assert_eq!(nodes.len(), 1);
        assert_eq!(node.text_contents(), "Hello");
        assert!(style(&nodes[0], "background-image").contains("90deg"));
        assert!(style(&text_target(&nodes[0]), "background-image").contains("180deg"));
        assert_eq!(radii(&nodes[0]), [8.0; 4]);
        let context = egui::Context::default();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            let bounds = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(200.0, 100.0));
            paint_background(
                ui.painter(),
                bounds,
                &nodes[0],
                corner_radius(&nodes[0], 1.0),
                egui::Color32::BLACK,
            );
            paint_text(
                ui.painter(),
                bounds,
                &nodes[0],
                "Hello".into(),
                egui::FontId::proportional(30.0),
                egui::Color32::WHITE,
            );
        });
        output.textures_delta.clear();
        let primitives = context.tessellate(output.shapes, context.pixels_per_point());
        assert!(primitives.iter().all(|p| match &p.primitive {
            egui::epaint::Primitive::Mesh(mesh) => mesh.is_valid(),
            egui::epaint::Primitive::Callback(_) => true,
        }));
        assert!(primitives.iter().any(|p| {
            match &p.primitive {
                egui::epaint::Primitive::Mesh(mesh) => mesh
                    .vertices
                    .iter()
                    .any(|v| v.color.r() > v.color.b() && v.color.b() > 0),
                egui::epaint::Primitive::Callback(_) => false,
            }
        }));
        assert!(
            primitives.iter().any(|p| match &p.primitive {
                egui::epaint::Primitive::Mesh(mesh) => mesh
                    .vertices
                    .iter()
                    .any(|v| v.color.b() > 200 && v.color.r() < 120),
                egui::epaint::Primitive::Callback(_) => false,
            }),
            "text gradient must reach its final color within the text bounds"
        );
    }
    #[test]
    fn shorthand_and_separate_corner_radii_survive_serialization() {
        for (value, expected) in [
            ("10px", [10.0; 4]),
            ("10px 20px", [10.0, 20.0, 10.0, 20.0]),
            ("10px 20px 30px", [10.0, 20.0, 30.0, 20.0]),
            ("1px 2px 3px 4px", [1.0, 2.0, 3.0, 4.0]),
        ] {
            let tree = parse(&format!("<div style='border-radius:{value}'></div>"));
            let node = elements(&tree)[0].clone();
            assert_eq!(radii(&node), expected);
            set_style(&node, "border-bottom-left-radius", "42px");
            assert_eq!(radii(&elements(&parse(&tree.to_string()))[0])[3], 42.0);
        }
    }
}

#[cfg(test)]
mod control_tests {
    use super::*;
    use crate::ui_designer::{elements, parse};
    #[test]
    fn linking_corners_updates_every_radius_and_unlinking_preserves_values() {
        let tree = parse("<div style='border-radius:1px 2px 3px 4px'></div>");
        let node = elements(&tree)[0].clone();
        let context = egui::Context::default();
        let mut separate = false;
        let mut frame = |events| {
            let mut changed = false;
            let mut output = context.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    changed = radius_controls(ui, &node, &node, &mut separate);
                },
            );
            output.textures_delta.clear();
            let target = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(text)
                        if text.galley.job.text == "Link all corners" =>
                    {
                        Some(text.pos + egui::vec2(10.0, 5.0))
                    }
                    _ => None,
                })
                .unwrap();
            (changed, target)
        };
        let (changed, position) = frame(vec![]);
        assert!(!changed);
        let click = |pressed| egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(position), click(true)]);
        assert!(frame(vec![click(false)]).0);
        assert!(
            radii(&node)
                .iter()
                .all(|value| (*value - 1.0).abs() < 0.0001)
        );
        frame(vec![click(true)]);
        frame(vec![click(false)]);
        assert!(separate);
        assert!(
            radii(&node)
                .iter()
                .all(|value| (*value - 1.0).abs() < 0.0001)
        );
    }
}
