use std::collections::BTreeMap;

use eframe::egui;
use egui_snarl::ui::{PinInfo, SnarlPin, SnarlStyle, SnarlViewer};
use egui_snarl::{InPin, InPinId, NodeId, OutPin, OutPinId, Snarl};
use engine_editor::AuthoringDocument;
use engine_world::{EntityId, EntitySnapshot};

#[derive(Clone, Debug, Eq, PartialEq)]
struct SceneNode {
    entity: EntityId,
    parent: Option<EntityId>,
    name: String,
}

#[derive(Default)]
pub struct SceneGraph {
    graph: Snarl<SceneNode>,
    snapshot: Vec<SceneNode>,
}

impl SceneGraph {
    pub fn show(&mut self, ui: &mut egui::Ui, document: &mut AuthoringDocument) {
        let snapshots = match document.entity_snapshots() {
            Ok(snapshots) => snapshots,
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                return;
            }
        };
        ui.label("Scene hierarchy · Select a node to inspect it. Reparent objects in Project.");
        self.sync(&snapshots);
        if self.snapshot.is_empty() {
            ui.label("The scene is empty. Add an object in Project to see its node here.");
            return;
        }
        self.graph.show(
            &mut SceneViewer { document },
            &SnarlStyle::default(),
            "scene-hierarchy-graph",
            ui,
        );
    }

    fn sync(&mut self, snapshots: &[EntitySnapshot]) {
        let nodes: Vec<_> = snapshots
            .iter()
            .map(|snapshot| SceneNode {
                entity: snapshot.id,
                parent: snapshot.parent,
                name: snapshot
                    .name
                    .clone()
                    .unwrap_or_else(|| snapshot.id.to_string()),
            })
            .collect();
        if nodes == self.snapshot {
            return;
        }
        let positions: BTreeMap<_, _> = self
            .graph
            .nodes_pos()
            .map(|(position, node)| (node.entity, position))
            .collect();
        let parents: BTreeMap<_, _> = nodes
            .iter()
            .map(|node| (node.entity, node.parent))
            .collect();
        self.graph = Snarl::new();
        let mut ids = BTreeMap::new();
        let mut rows = BTreeMap::<usize, u16>::new();
        for node in &nodes {
            let mut depth = 0;
            let mut parent = node.parent;
            while let Some(id) = parent {
                depth += 1;
                if depth >= nodes.len() {
                    break;
                }
                parent = parents.get(&id).copied().flatten();
            }
            let row = rows.entry(depth).or_default();
            let position = positions.get(&node.entity).copied().unwrap_or_else(|| {
                egui::pos2(
                    f32::from(u16::try_from(depth).unwrap_or(u16::MAX)) * 260.0,
                    f32::from(*row) * 130.0,
                )
            });
            *row = row.saturating_add(1);
            ids.insert(node.entity, self.graph.insert_node(position, node.clone()));
        }
        for node in &nodes {
            if let Some(parent) = node.parent.and_then(|id| ids.get(&id)) {
                self.graph.connect(
                    OutPinId {
                        node: *parent,
                        output: 0,
                    },
                    InPinId {
                        node: ids[&node.entity],
                        input: 0,
                    },
                );
            }
        }
        self.snapshot = nodes;
    }
}

struct SceneViewer<'a> {
    document: &'a mut AuthoringDocument,
}

impl SnarlViewer<SceneNode> for SceneViewer<'_> {
    fn title(&mut self, node: &SceneNode) -> String {
        node.name.clone()
    }

    fn show_header(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<SceneNode>,
    ) {
        let node = &snarl[node];
        if ui
            .selectable_label(self.document.selected() == Some(node.entity), &node.name)
            .on_hover_text(node.entity.to_string())
            .clicked()
        {
            self.document.select(Some(node.entity));
        }
    }

    fn inputs(&mut self, node: &SceneNode) -> usize {
        usize::from(node.parent.is_some())
    }
    fn outputs(&mut self, _node: &SceneNode) -> usize {
        1
    }

    fn show_input(
        &mut self,
        _pin: &InPin,
        ui: &mut egui::Ui,
        _snarl: &mut Snarl<SceneNode>,
    ) -> impl SnarlPin + 'static {
        ui.label("Parent");
        PinInfo::circle()
    }

    fn show_output(
        &mut self,
        _pin: &OutPin,
        ui: &mut egui::Ui,
        _snarl: &mut Snarl<SceneNode>,
    ) -> impl SnarlPin + 'static {
        ui.label("Children");
        PinInfo::circle()
    }

    // Wires reflect authored parent links; graph gestures must not mutate the scene.
    fn connect(&mut self, _from: &OutPin, _to: &InPin, _snarl: &mut Snarl<SceneNode>) {}
    fn disconnect(&mut self, _from: &OutPin, _to: &InPin, _snarl: &mut Snarl<SceneNode>) {}
    fn drop_inputs(&mut self, _pin: &InPin, _snarl: &mut Snarl<SceneNode>) {}
    fn drop_outputs(&mut self, _pin: &OutPin, _snarl: &mut Snarl<SceneNode>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_edits_refresh_wires_and_preserve_arranged_node_positions() {
        let parent = EntitySnapshot::default();
        let mut child = EntitySnapshot {
            parent: Some(parent.id),
            ..EntitySnapshot::default()
        };
        let mut graph = SceneGraph::default();
        graph.sync(&[parent.clone(), child.clone()]);
        assert_eq!(graph.graph.wires().count(), 1);
        let id = graph
            .graph
            .node_ids()
            .find(|(_, node)| node.entity == child.id)
            .unwrap()
            .0;
        let position = egui::pos2(41.0, 89.0);
        graph.graph.get_node_info_mut(id).unwrap().pos = position;
        child.name = Some("Renamed child".into());
        child.parent = None;
        graph.sync(&[parent, child.clone()]);
        assert_eq!(graph.graph.wires().count(), 0);
        assert_eq!(
            graph
                .graph
                .nodes_pos()
                .find(|(_, node)| node.entity == child.id)
                .unwrap()
                .0,
            position
        );
        graph.sync(&[]);
        assert_eq!(graph.graph.nodes().count(), 0);
    }
}
