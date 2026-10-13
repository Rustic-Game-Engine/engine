use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Default)]
pub(super) struct ExplorerSelection {
    pub highlighted: BTreeSet<PathBuf>,
    pub anchor: Option<PathBuf>,
    pub visible: Vec<PathBuf>,
    pub rendered: Vec<PathBuf>,
    pub press: Option<(PathBuf, bool)>,
    pub focused: bool,
    pub renaming: bool,
    pub brushing: bool,
}

impl ExplorerSelection {
    pub fn begin_frame(&mut self) {
        self.visible = std::mem::take(&mut self.rendered);
        self.renaming = false;
    }

    pub fn select(&mut self, path: &Path, shift: bool, ctrl: bool) {
        if ctrl {
            let start = self
                .anchor
                .as_ref()
                .and_then(|anchor| self.visible.iter().position(|item| item == anchor));
            let end = self.visible.iter().position(|item| item == path);
            if !shift {
                self.highlighted.clear();
            }
            if let (Some(start), Some(end)) = (start, end) {
                self.highlighted.extend(
                    self.visible[start.min(end)..=start.max(end)]
                        .iter()
                        .cloned(),
                );
            } else {
                self.highlighted.insert(path.to_path_buf());
            }
        } else {
            if !shift {
                self.highlighted.clear();
            }
            self.highlighted.insert(path.to_path_buf());
        }
        self.anchor = Some(path.to_path_buf());
        self.focused = true;
    }

    // Eligibility is captured before highlighting. An unselected press can never
    // become a drag merely because that same press just selected the row.
    pub fn press(&mut self, path: &Path, shift: bool, ctrl: bool) {
        let drag = self.highlighted.contains(path) && !shift && !ctrl;
        self.brushing = shift && !ctrl;
        if !drag {
            self.select(path, shift, ctrl);
        }
        self.press = Some((path.to_path_buf(), drag));
        self.focused = true;
    }

    pub fn relocate(&mut self, source: &Path, destination: &Path) {
        let remap = |path: PathBuf| {
            path.strip_prefix(source).map_or_else(
                |_| path.clone(),
                |suffix| {
                    if suffix.as_os_str().is_empty() {
                        destination.to_path_buf()
                    } else {
                        destination.join(suffix)
                    }
                },
            )
        };
        self.highlighted = std::mem::take(&mut self.highlighted)
            .into_iter()
            .map(remap)
            .collect();
        self.anchor = self.anchor.take().map(remap);
    }

    pub fn can_drag(&self, path: &Path) -> bool {
        self.highlighted.contains(path)
            && self
                .press
                .as_ref()
                .is_some_and(|(origin, armed)| *armed && origin == path)
    }
}

// Selecting a folder and its descendants copies/moves that subtree once.
pub(super) fn selection_roots(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let paths: BTreeSet<_> = paths.into_iter().collect();
    paths
        .iter()
        .filter(|path| {
            !paths
                .iter()
                .any(|parent| *path != parent && path.starts_with(parent))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_adds_and_ctrl_selects_visible_range_from_last_highlight() {
        let mut selection = ExplorerSelection {
            visible: ["a", "b", "c", "d"].map(PathBuf::from).to_vec(),
            ..Default::default()
        };
        selection.select(Path::new("a"), false, false);
        selection.select(Path::new("c"), true, false);
        assert_eq!(selection.highlighted, ["a", "c"].map(PathBuf::from).into());
        selection.select(Path::new("a"), false, true);
        assert_eq!(
            selection.highlighted,
            ["a", "b", "c"].map(PathBuf::from).into()
        );
        selection.select(Path::new("d"), true, true);
        assert_eq!(selection.highlighted.len(), 4);
    }

    #[test]
    fn first_press_only_highlights_and_second_press_arms_that_item() {
        let mut selection = ExplorerSelection::default();
        selection.press(Path::new("a"), false, false);
        assert!(selection.highlighted.contains(Path::new("a")));
        assert!(!selection.can_drag(Path::new("a")));
        selection.press(Path::new("a"), false, false);
        assert!(selection.can_drag(Path::new("a")));
        assert!(!selection.can_drag(Path::new("b")));
        selection.press(Path::new("b"), true, false);
        assert_eq!(selection.highlighted.len(), 2);
        assert!(!selection.can_drag(Path::new("b")));
        assert!(!selection.can_drag(Path::new("a")));
    }

    #[test]
    fn selected_parent_suppresses_duplicate_descendant_operations() {
        assert_eq!(
            selection_roots(["folder/a", "folder", "other", "folderish"].map(PathBuf::from)),
            ["folder", "folderish", "other"].map(PathBuf::from)
        );
    }
    #[test]
    fn renamed_folder_retains_selection_and_anchor_at_new_paths() {
        let mut selection = ExplorerSelection {
            highlighted: ["folder", "folder/child", "other"]
                .map(PathBuf::from)
                .into(),
            anchor: Some("folder/child".into()),
            ..Default::default()
        };
        selection.relocate(Path::new("folder"), Path::new("renamed"));
        assert_eq!(
            selection.highlighted,
            ["renamed", "renamed/child", "other"]
                .map(PathBuf::from)
                .into()
        );
        assert_eq!(selection.anchor, Some("renamed/child".into()));
    }
}
