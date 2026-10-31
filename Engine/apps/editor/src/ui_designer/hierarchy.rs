use super::{NodeRef, elements, label, layout, set_style, style};
use eframe::egui;

pub(super) fn name(node: &NodeRef) -> String {
    let tag = node.as_element().map_or("", |e| e.name.local.as_ref());
    let kind = match tag {
        "div" => "Panel",
        "span" => "Text",
        "button" => "Button",
        "img" => "Image",
        _ => tag,
    };
    let id = node
        .as_element()
        .and_then(|e| e.attributes.borrow().get("id").map(str::to_owned));
    id.map_or_else(|| kind.to_owned(), |id| format!("{kind} · {id}"))
}

pub(super) fn accepts(parent: &NodeRef, child: &NodeRef) -> bool {
    let tag = |node: &NodeRef| {
        node.as_element()
            .map(|e| e.name.local.to_string())
            .unwrap_or_default()
    };
    let child_tag = tag(child);
    match tag(parent).as_str() {
        "body" | "div" | "section" | "article" | "main" | "aside" | "nav" | "header" | "footer"
        | "form" | "fieldset" | "li" | "td" | "th" => true,
        "span" | "p" | "label" | "button" | "a" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            child.inclusive_descendants().all(|node| {
                node.as_element().is_none_or(|e| {
                    matches!(
                        e.name.local.as_ref(),
                        "span" | "img" | "br" | "em" | "strong" | "small" | "b" | "i" | "u"
                    )
                })
            })
        }
        "ul" | "ol" => child_tag == "li",
        _ => false,
    }
}

pub(super) fn can_reparent(node: &NodeRef, parent: &NodeRef) -> bool {
    accepts(parent, node)
        && !parent
            .inclusive_ancestors()
            .any(|ancestor| ancestor == *node)
}

pub(super) fn reparent(
    tree: &NodeRef,
    nodes: &[NodeRef],
    preview: &[NodeRef],
    index: usize,
    parent: Option<usize>,
) -> Result<usize, String> {
    let node = &nodes[index];
    let target = parent
        .map(|i| nodes[i].clone())
        .or_else(|| tree.select_first("body").ok().map(|e| e.as_node().clone()))
        .ok_or("Missing page body")?;
    if !can_reparent(node, &target) {
        return Err("Choose a container outside this element's children.".into());
    }
    if node.parent().as_ref() == Some(&target) {
        return Ok(index);
    }
    let positions = layout(preview);
    let offset = parent.map_or([0.0, 0.0], |i| [positions[i][0], positions[i][1]]);
    let position = positions[index];
    target.append(node.clone());
    set_style(node, "position", "absolute");
    set_style(node, "left", &format!("{}px", position[0] - offset[0]));
    set_style(node, "top", &format!("{}px", position[1] - offset[1]));
    if let Some(parent) = parent
        && matches!(style(&preview[parent], "position").as_str(), "" | "static")
    {
        set_style(&target, "position", "relative");
    }
    elements(tree)
        .iter()
        .position(|candidate| candidate == node)
        .ok_or_else(|| "Element left the page".into())
}

pub(super) fn tree(ui: &mut egui::Ui, nodes: &[NodeRef], selected: &mut Option<usize>) {
    fn row(ui: &mut egui::Ui, nodes: &[NodeRef], index: usize, selected: &mut Option<usize>) {
        let children: Vec<_> = super::paint_order(nodes)
            .into_iter()
            .filter(|i| nodes[*i].parent().as_ref() == Some(&nodes[index]))
            .collect();
        let title = name(&nodes[index]);
        if children.is_empty() {
            if ui
                .selectable_label(*selected == Some(index), title)
                .on_hover_text(label(&nodes[index]))
                .clicked()
            {
                *selected = Some(index);
            }
        } else {
            egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.make_persistent_id(("ui-parent", index)),
                true,
            )
            .show_header(ui, |ui| {
                if ui
                    .selectable_label(*selected == Some(index), title)
                    .clicked()
                {
                    *selected = Some(index);
                }
            })
            .body(|ui| {
                for child in children {
                    row(ui, nodes, child, selected);
                }
            });
        }
    }
    for index in super::paint_order(nodes) {
        if !nodes[index]
            .ancestors()
            .any(|parent| nodes.contains(&parent))
        {
            row(ui, nodes, index, selected);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_designer::{Document, parse};
    #[test]
    fn moving_a_subtree_preserves_canvas_position_and_round_trips_with_history() {
        let source = "<div id='a' style='left:10px;top:20px'><div id='child' style='left:5px;top:8px'><span>Nested</span></div></div><div id='b' style='left:200px;top:100px'></div>";
        let tree = parse(source);
        let nodes = elements(&tree);
        let before = layout(&nodes)[1];
        let descendant_before = layout(&nodes)[2];
        let index = reparent(&tree, &nodes, &nodes, 1, Some(3)).unwrap();
        let moved = elements(&tree);
        assert_eq!(&layout(&moved)[index][..2], &before[..2]);
        assert_eq!(&layout(&moved)[index + 1][..2], &descendant_before[..2]);
        assert_eq!(moved[index].parent(), Some(moved[1].clone()));
        assert_eq!(moved[index].text_contents(), "Nested");
        assert!(!can_reparent(&moved[1], &moved[index]));
        let mut document = Document {
            root: std::path::PathBuf::default(),
            relative: std::path::PathBuf::default(),
            saved: source.into(),
            source: source.into(),
            undo: vec![],
            redo: vec![],
        };
        document.change(tree.to_string());
        let reopened = parse(&document.source);
        assert!(reopened.select_first("#b > #child > span").is_ok());
        document.undo();
        assert_eq!(document.source, source);
        document.redo();
        assert!(parse(&document.source).select_first("#b > #child").is_ok());
        let current = elements(&reopened);
        let child = current
            .iter()
            .position(|node| {
                node.as_element().unwrap().attributes.borrow().get("id") == Some("child")
            })
            .unwrap();
        let root_index = reparent(&reopened, &current, &current, child, None).unwrap();
        assert_eq!(&layout(&elements(&reopened))[root_index][..2], &before[..2]);
    }
    #[test]
    fn containers_accept_children_but_cycles_voids_and_invalid_inline_nesting_are_rejected() {
        let tree = parse("<div><button><span>Text</span></button><img></div>");
        let nodes = elements(&tree);
        assert!(accepts(&nodes[0], &nodes[1]));
        assert!(accepts(&nodes[1], &nodes[2]));
        assert!(!accepts(&nodes[1], &nodes[0]));
        assert!(!accepts(&nodes[3], &nodes[2]));
        assert!(!can_reparent(&nodes[0], &nodes[2]));
        assert!(!can_reparent(&nodes[0], &nodes[0]));
    }
}
