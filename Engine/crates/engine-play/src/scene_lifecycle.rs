//! Script-visible scene ownership, shared by every language adapter.
use super::reload_behaviors;
use super::{
    Arc, BTreeMap, BTreeSet, Component, EntityId, GameplayContext, LiveEntityProperties, Mutex,
    Path, PathBuf, RuntimeBehavior, RuntimeChangeSet, SceneRuntime, SceneWorld, ScriptId,
    ScriptReference, VecDeque, load_scene, push_log,
};
use engine_world::{SceneDocument, SceneId};

#[derive(Clone, Default)]
pub(super) struct SceneIo {
    pub logs: Arc<Mutex<VecDeque<(String, String)>>>,
    pub dropped_logs: Arc<Mutex<u64>>,
    pub input_keys: Arc<Mutex<BTreeSet<String>>>,
}

#[derive(Clone)]
pub(super) struct LoadedScene {
    pub id: SceneId,
    pub name: String,
    pub world: Arc<Mutex<SceneWorld>>,
    pub gameplay: Arc<Mutex<GameplayContext>>,
    pub startup_scripts: Vec<ScriptReference>,
    pub valid: bool,
    auto_loaded: bool,
    source_path: Option<PathBuf>,
}

impl LoadedScene {
    fn from_document(document: SceneDocument) -> Result<Self, String> {
        let world = document.create_world().map_err(|e| e.to_string())?;
        Ok(Self {
            id: document.id,
            name: document.name,
            world: Arc::new(Mutex::new(world)),
            gameplay: Arc::default(),
            startup_scripts: document.startup_scripts,
            valid: true,
            auto_loaded: false,
            source_path: None,
        })
    }

    fn empty() -> Self {
        Self {
            id: SceneId::new(),
            name: String::new(),
            world: Arc::default(),
            gameplay: Arc::default(),
            startup_scripts: Vec::new(),
            valid: false,
            auto_loaded: false,
            source_path: None,
        }
    }
}

#[derive(Default)]
pub(super) struct SceneRegistry {
    pub loaded: BTreeMap<String, LoadedScene>,
    pub active: Option<String>,
    closing: bool,
    pub empty_world: Arc<Mutex<SceneWorld>>,
    pub empty_gameplay: Arc<Mutex<GameplayContext>>,
}

impl SceneRegistry {
    fn insert_saved(
        &mut self,
        document: SceneDocument,
        path: PathBuf,
        auto_loaded: bool,
    ) -> Result<String, String> {
        if self.loaded.contains_key(&document.name)
            || self.loaded.values().any(|loaded| loaded.id == document.id)
        {
            return Err("scene is already loaded (scene names and IDs must be unique)".into());
        }
        let mut loaded = LoadedScene::from_document(document)?;
        loaded.auto_loaded = auto_loaded;
        loaded.source_path = Some(path);
        let name = loaded.name.clone();
        self.loaded.insert(name.clone(), loaded);
        Ok(name)
    }

    pub fn query(
        &mut self,
        root: &Path,
        query: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if self.closing {
            return Err("scene lifecycle is unavailable during teardown".into());
        }
        let op = query["op"].as_str().ok_or("scene operation is missing")?;
        if op == "scene_active" {
            return Ok(serde_json::json!(self.active));
        }
        if op == "scene_loaded" {
            return Ok(serde_json::json!(self.loaded.keys().collect::<Vec<_>>()));
        }
        let scene = query["scene"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("scene must be a nonempty string")?;
        match op {
            "scene_load" => {
                let (document, path) = read_saved_scene(root, scene)?;
                let name = self.insert_saved(document, path, false)?;
                Ok(serde_json::json!(name))
            }
            "scene_is_loaded" => Ok(serde_json::json!(self.loaded.contains_key(scene))),
            "scene_switch" => {
                let target = if self.loaded.contains_key(scene) {
                    scene.to_owned()
                } else if let Some(loaded) = self
                    .loaded
                    .values()
                    .find(|loaded| loaded.source_path.as_deref() == Some(Path::new(scene)))
                {
                    loaded.name.clone()
                } else {
                    let (document, path) = resolve_saved_scene(root, scene)?;
                    if let Some(existing) = self.loaded.get(&document.name) {
                        if existing.id != document.id {
                            return Err("a different scene with this name is already loaded".into());
                        }
                        existing.name.clone()
                    } else {
                        self.insert_saved(document, path, true)?
                    }
                };
                // Resolve and validate the destination before releasing the current scene.
                if self.active.as_ref() != Some(&target)
                    && let Some(previous) = self.active.as_ref()
                    && self
                        .loaded
                        .get(previous)
                        .is_some_and(|loaded| loaded.auto_loaded)
                {
                    self.loaded.remove(previous);
                }
                self.active = Some(target);
                Ok(serde_json::Value::Null)
            }
            "scene_unload" => {
                self.loaded
                    .remove(scene)
                    .ok_or_else(|| format!("scene `{scene}` is not loaded"))?;
                if self.active.as_deref() == Some(scene) {
                    self.active = None;
                }
                Ok(serde_json::Value::Null)
            }
            _ => Err(format!("unknown scene operation `{op}`")),
        }
    }
}

/// Reads only a relative project file; neither documents nor bytes are cached.
fn read_saved_scene(root: &Path, source: &str) -> Result<(SceneDocument, PathBuf), String> {
    let path = Path::new(source);
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
        || source.contains('\\')
    {
        return Err("scene load requires a relative project path".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let path = root.join(path).canonicalize().map_err(|e| e.to_string())?;
    let relative = path
        .strip_prefix(&root)
        .map_err(|_| "scene path escapes the project")?
        .to_owned();
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let parsed = load_scene(&bytes).map_err(|e| e.to_string())?;
    if parsed.read_only || parsed.has_errors() {
        return Err("scene is damaged or incompatible".into());
    }
    if parsed.document.name.is_empty() {
        return Err("scene name must not be empty".into());
    }
    Ok((parsed.document, relative))
}

fn resolve_saved_scene(root: &Path, source: &str) -> Result<(SceneDocument, PathBuf), String> {
    if source.contains('/')
        || source.contains('\\')
        || Path::new(source)
            .extension()
            .is_some_and(|ext| ext == "scene" || ext == "rscene")
    {
        return read_saved_scene(root, source);
    }
    // Discover document names on demand, including files whose names differ.
    // Only the matching document survives the search; no unloaded-world cache is built.
    let mut pending = [root.join("scene"), root.join("scenes")]
        .into_iter()
        .filter(|path| {
            path.symlink_metadata()
                .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        })
        .collect::<Vec<_>>();
    let mut found = None;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file()
                && matches!(
                    path.extension().and_then(|ext| ext.to_str()),
                    Some("scene" | "rscene")
                )
            {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy();
                if let Ok((document, relative)) = read_saved_scene(root, &relative)
                    && document.name == source
                {
                    if found.is_some() {
                        return Err(format!(
                            "multiple saved scenes are named `{source}`; use a scene file path"
                        ));
                    }
                    found = Some((document, relative));
                }
            }
        }
    }
    found.ok_or_else(|| {
        format!("no valid saved scene named `{source}` was found; pass a relative scene file path")
    })
}

/// Keeps global scripts alive while swapping scene-owned worlds and behaviors.
pub(crate) struct RuntimeScripts {
    pub(super) active: SceneRuntime,
    pub(super) inactive: BTreeMap<String, SceneRuntime>,
    pub(super) globals: Vec<RuntimeBehavior>,
    pub(super) scenes: Arc<Mutex<SceneRegistry>>,
    root: PathBuf,
    io: SceneIo,
}

impl RuntimeScripts {
    pub fn load(root: &Path, bytes: &[u8]) -> Result<Self, String> {
        let loaded = match load_scene(bytes) {
            Ok(scene) => LoadedScene::from_document(scene.document)?,
            Err(error) if root.join("config/scripts.ron").is_file() => {
                return Err(error.to_string());
            }
            Err(_) => LoadedScene::empty(),
        };
        let scenes = Arc::new(Mutex::new(SceneRegistry::default()));
        {
            let mut registry = scenes.lock().map_err(|_| "scene lock poisoned")?;
            registry.active = Some(loaded.name.clone());
            registry.loaded.insert(loaded.name.clone(), loaded.clone());
        }
        let io = SceneIo::default();
        let mut active = SceneRuntime::load(root, &loaded, &scenes, &io, true)?;
        let globals = std::mem::take(&mut active.global_behaviors);
        let mut runtime = Self {
            active,
            inactive: BTreeMap::new(),
            globals,
            scenes,
            root: root.to_owned(),
            io,
        };
        runtime.sync_scenes();
        Ok(runtime)
    }

    fn empty_runtime(&self) -> SceneRuntime {
        let (world, gameplay) = self.scenes.lock().map_or_else(
            |_| (Arc::default(), Arc::default()),
            |registry| {
                (
                    Arc::clone(&registry.empty_world),
                    Arc::clone(&registry.empty_gameplay),
                )
            },
        );
        SceneRuntime {
            gameplay,
            behaviors: Vec::new(),
            world,
            input_keys: Arc::clone(&self.io.input_keys),
            logs: Arc::clone(&self.io.logs),
            dropped_logs: Arc::clone(&self.io.dropped_logs),
            valid_scene: false,
            name: String::new(),
            global_behaviors: Vec::new(),
        }
    }

    fn sync_scenes(&mut self) {
        let (loaded, selected) = match self.scenes.lock() {
            Ok(registry) => (registry.loaded.clone(), registry.active.clone()),
            Err(_) => return,
        };
        // Destroy removed/reloaded instances before releasing their worlds and VMs.
        self.inactive.retain(|name, runtime| {
            if loaded
                .get(name)
                .is_some_and(|s| Arc::ptr_eq(&s.world, &runtime.world))
            {
                true
            } else {
                runtime.stop();
                false
            }
        });
        let active_kept = loaded
            .get(&self.active.name)
            .is_some_and(|s| Arc::ptr_eq(&s.world, &self.active.world));
        let empty_selected =
            selected.is_none() && self.active.name.is_empty() && !self.active.valid_scene;
        if !empty_selected && (!active_kept || selected.as_deref() != Some(&self.active.name)) {
            let empty = self.empty_runtime();
            let mut previous = std::mem::replace(&mut self.active, empty);
            if active_kept {
                self.inactive.insert(previous.name.clone(), previous);
            } else {
                previous.stop();
            }
            if let Some(name) = selected
                && let Some(scene) = loaded.get(&name)
            {
                let runtime = self.inactive.remove(&name).map_or_else(
                    || SceneRuntime::load(&self.root, scene, &self.scenes, &self.io, false),
                    Ok,
                );
                match runtime {
                    Ok(runtime) => self.active = runtime,
                    Err(error) => {
                        push_log(
                            &self.io.logs,
                            &self.io.dropped_logs,
                            "error",
                            &format!("scene `{name}` startup failed: {error}"),
                        );
                        if let Ok(mut registry) = self.scenes.lock() {
                            registry.active = None;
                            registry.loaded.remove(&name);
                        }
                    }
                }
            }
        }
        for behavior in &mut self.globals {
            if let Ok(mut host) = behavior.host().lock() {
                host.refresh_scene_context();
            }
        }
    }

    fn tick(&mut self, delta: f64, fixed: bool) {
        self.sync_scenes();
        let mut behaviors = std::mem::take(&mut self.globals);
        behaviors.append(&mut self.active.behaviors);
        self.active.behaviors = behaviors;
        if fixed {
            self.active.fixed_update(delta);
        } else {
            self.active.frame_update(delta);
        }
        let mut scene_behaviors = Vec::new();
        for behavior in self.active.behaviors.drain(..) {
            let global = behavior
                .host()
                .lock()
                .is_ok_and(|host| host.persists_across_scenes());
            if global {
                self.globals.push(behavior);
            } else {
                scene_behaviors.push(behavior);
            }
        }
        self.active.behaviors = scene_behaviors;
        self.sync_scenes();
    }
    pub fn fixed_update(&mut self, delta: f64) {
        self.tick(delta, true);
    }
    pub fn frame_update(&mut self, delta: f64) {
        self.tick(delta, false);
    }
    pub fn stop(&mut self) {
        if let Ok(mut registry) = self.scenes.lock() {
            registry.closing = true;
        }
        for behavior in &mut self.globals {
            let _ = behavior.on_disable();
            let _ = behavior.on_destroy();
            let _ = behavior.on_stop();
            behavior.cleanup_gameplay();
        }
        self.globals.clear();
        self.active.stop();
        for runtime in self.inactive.values_mut() {
            runtime.stop();
        }
        self.active.behaviors.clear();
        for runtime in self.inactive.values_mut() {
            runtime.behaviors.clear();
        }
        // Keep still-loaded world data for the existing post-Stop change review.
        // Explicitly unloaded worlds have already been removed by sync_scenes.
    }
    pub fn render_world<T>(&self, render: impl FnOnce(&SceneWorld) -> T) -> Result<T, String> {
        self.active.render_world(render)
    }
    pub fn live_entity(&self, id: EntityId) -> Result<Option<LiveEntityProperties>, String> {
        self.active.live_entity(id)
    }
    pub fn set_input_keys(&mut self, keys: Vec<String>) {
        self.active.set_input_keys(keys);
    }
    pub fn drain_logs(&self) -> Vec<(String, String)> {
        self.active.drain_logs()
    }
    pub fn reload(&mut self, id: ScriptId, source: &[u8]) -> Result<usize, String> {
        reload_behaviors(
            self.globals
                .iter_mut()
                .chain(self.active.behaviors.iter_mut())
                .chain(
                    self.inactive
                        .values_mut()
                        .flat_map(|runtime| runtime.behaviors.iter_mut()),
                ),
            id,
            source,
        )
    }
    pub fn runtime_changes(&self, hash: &str, bytes: &[u8]) -> Result<RuntimeChangeSet, String> {
        let Ok(original) = load_scene(bytes) else {
            return Ok(RuntimeChangeSet::new(hash, Vec::new()));
        };
        let world = {
            let registry = self.scenes.lock().map_err(|_| "scene lock poisoned")?;
            registry
                .loaded
                .values()
                .find(|s| s.id == original.document.id)
                .map(|scene| Arc::clone(&scene.world))
        };
        if let Some(world) = world {
            let mut copy = self.empty_runtime();
            copy.world = world;
            copy.valid_scene = true;
            copy.runtime_changes(hash, bytes)
        } else {
            Ok(RuntimeChangeSet::new(hash, Vec::new()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_world::{EntitySnapshot, save_scene_atomic};

    fn scene_query(
        runtime: &RuntimeScripts,
        root: &Path,
        op: &str,
        scene: &str,
    ) -> Result<serde_json::Value, String> {
        runtime
            .scenes
            .lock()
            .unwrap()
            .query(root, &serde_json::json!({"op":op,"scene":scene}))
    }

    #[test]
    fn autoloaded_scenes_release_on_switch_while_explicit_scenes_stay_loaded() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("scene/nested")).unwrap();
        let mut level = SceneDocument::new("Level");
        let entity = EntitySnapshot::default();
        let id = entity.id;
        level.entities.push(entity);
        save_scene_atomic(
            temp.path().join("scene/nested/different-file.scene"),
            &level,
        )
        .unwrap();
        save_scene_atomic(
            temp.path().join("scene/other.scene"),
            &SceneDocument::new("Other"),
        )
        .unwrap();
        let mut runtime =
            RuntimeScripts::load(temp.path(), &SceneDocument::new("Menu").to_bytes().unwrap())
                .unwrap();
        scene_query(&runtime, temp.path(), "scene_switch", "Level").unwrap();
        runtime.sync_scenes();
        let old_world = Arc::downgrade(&runtime.active.world);
        let old_context = Arc::downgrade(&runtime.active.gameplay);
        runtime
            .active
            .world
            .lock()
            .unwrap()
            .destroy_entity(id)
            .unwrap();
        scene_query(
            &runtime,
            temp.path(),
            "scene_switch",
            "scene/nested/different-file.scene",
        )
        .unwrap();
        runtime.sync_scenes();
        assert!(!runtime.active.world.lock().unwrap().contains(id));
        assert!(scene_query(&runtime, temp.path(), "scene_switch", "Missing").is_err());
        assert!(scene_query(&runtime, temp.path(), "scene_switch", "../outside.scene").is_err());
        std::fs::write(temp.path().join("scene/broken.scene"), b"broken").unwrap();
        assert!(scene_query(&runtime, temp.path(), "scene_switch", "scene/broken.scene").is_err());
        assert_eq!(
            runtime.scenes.lock().unwrap().active.as_deref(),
            Some("Level")
        );
        scene_query(&runtime, temp.path(), "scene_switch", "scene/other.scene").unwrap();
        runtime.sync_scenes();
        assert!(old_world.upgrade().is_none());
        assert!(old_context.upgrade().is_none());
        assert!(!runtime.scenes.lock().unwrap().loaded.contains_key("Level"));
        assert!(runtime.scenes.lock().unwrap().loaded.contains_key("Menu"));
        scene_query(&runtime, temp.path(), "scene_switch", "Menu").unwrap();
        runtime.sync_scenes();
        assert!(!runtime.scenes.lock().unwrap().loaded.contains_key("Other"));
        scene_query(&runtime, temp.path(), "scene_switch", "Level").unwrap();
        runtime.sync_scenes();
        assert!(runtime.active.world.lock().unwrap().contains(id));
        scene_query(&runtime, temp.path(), "scene_switch", "Menu").unwrap();
        runtime.sync_scenes();
        scene_query(
            &runtime,
            temp.path(),
            "scene_load",
            "scene/nested/different-file.scene",
        )
        .unwrap();
        scene_query(&runtime, temp.path(), "scene_switch", "Level").unwrap();
        runtime.sync_scenes();
        let explicit_world = Arc::downgrade(&runtime.active.world);
        scene_query(&runtime, temp.path(), "scene_switch", "Menu").unwrap();
        runtime.sync_scenes();
        assert!(explicit_world.upgrade().is_some());
        assert!(runtime.scenes.lock().unwrap().loaded.contains_key("Level"));
    }

    #[test]
    fn duplicate_saved_names_require_a_path_and_fail_without_unloading_current() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("scene")).unwrap();
        save_scene_atomic(
            temp.path().join("scene/a.scene"),
            &SceneDocument::new("Duplicate"),
        )
        .unwrap();
        save_scene_atomic(
            temp.path().join("scene/b.scene"),
            &SceneDocument::new("Duplicate"),
        )
        .unwrap();
        let mut runtime =
            RuntimeScripts::load(temp.path(), &SceneDocument::new("Menu").to_bytes().unwrap())
                .unwrap();
        assert!(
            scene_query(&runtime, temp.path(), "scene_switch", "Duplicate")
                .unwrap_err()
                .contains("multiple")
        );
        assert_eq!(
            runtime.scenes.lock().unwrap().active.as_deref(),
            Some("Menu")
        );
        assert_eq!(runtime.scenes.lock().unwrap().loaded.len(), 1);
        scene_query(&runtime, temp.path(), "scene_switch", "scene/a.scene").unwrap();
        runtime.sync_scenes();
        assert_eq!(runtime.active.name, "Duplicate");
    }

    #[test]
    fn switching_retains_loaded_state_and_reloading_releases_previous_generation() {
        let temp = tempfile::tempdir().unwrap();
        let mut document = SceneDocument::new("Level");
        let entity = EntitySnapshot::default();
        let id = entity.id;
        document.entities.push(entity);
        let path = temp.path().join("level.scene");
        save_scene_atomic(&path, &document).unwrap();
        let mut runtime =
            RuntimeScripts::load(temp.path(), &SceneDocument::new("Menu").to_bytes().unwrap())
                .unwrap();
        let call = |op, scene: &str| {
            runtime
                .scenes
                .lock()
                .unwrap()
                .query(temp.path(), &serde_json::json!({"op":op,"scene":scene}))
        };
        assert!(call("scene_switch", "Level").is_err());
        assert!(call("scene_load", "level.scene").is_ok());
        assert!(call("scene_load", "level.scene").is_err());
        call("scene_switch", "Level").unwrap();
        runtime.sync_scenes();
        let old_world = Arc::downgrade(&runtime.active.world);
        runtime
            .active
            .world
            .lock()
            .unwrap()
            .destroy_entity(id)
            .unwrap();
        runtime
            .scenes
            .lock()
            .unwrap()
            .query(
                temp.path(),
                &serde_json::json!({"op":"scene_switch","scene":"Menu"}),
            )
            .unwrap();
        runtime.sync_scenes();
        assert!(old_world.upgrade().is_some());
        runtime
            .scenes
            .lock()
            .unwrap()
            .query(
                temp.path(),
                &serde_json::json!({"op":"scene_switch","scene":"Level"}),
            )
            .unwrap();
        runtime.sync_scenes();
        assert!(!runtime.active.world.lock().unwrap().contains(id));
        {
            let mut registry = runtime.scenes.lock().unwrap();
            registry
                .query(
                    temp.path(),
                    &serde_json::json!({"op":"scene_unload","scene":"Level"}),
                )
                .unwrap();
            assert!(
                registry
                    .query(
                        temp.path(),
                        &serde_json::json!({"op":"scene_switch","scene":"Level"})
                    )
                    .is_err()
            );
            registry
                .query(
                    temp.path(),
                    &serde_json::json!({"op":"scene_load","scene":"level.scene"}),
                )
                .unwrap();
            registry
                .query(
                    temp.path(),
                    &serde_json::json!({"op":"scene_switch","scene":"Level"}),
                )
                .unwrap();
        }
        runtime.sync_scenes();
        assert!(old_world.upgrade().is_none());
        assert!(runtime.active.world.lock().unwrap().contains(id));
    }

    #[test]
    fn invalid_loads_leave_selection_and_loaded_worlds_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let mut registry = SceneRegistry::default();
        let document = SceneDocument::new("First");
        registry.loaded.insert(
            "First".into(),
            LoadedScene::from_document(document).unwrap(),
        );
        registry.active = Some("First".into());
        for path in [
            "../outside.scene",
            "/outside.scene",
            "missing.scene",
            "scene\\bad.scene",
        ] {
            assert!(
                registry
                    .query(
                        temp.path(),
                        &serde_json::json!({"op":"scene_load","scene":path})
                    )
                    .is_err()
            );
        }
        let mut incompatible =
            String::from_utf8(SceneDocument::new("Bad").to_bytes().unwrap()).unwrap();
        incompatible = incompatible.replace("\"Bad\"", "\"Edited\"");
        std::fs::write(temp.path().join("bad.scene"), incompatible).unwrap();
        assert!(
            registry
                .query(
                    temp.path(),
                    &serde_json::json!({"op":"scene_load","scene":"bad.scene"})
                )
                .is_err()
        );
        assert!(
            registry
                .query(
                    temp.path(),
                    &serde_json::json!({"op":"scene_switch","scene":"Bad"})
                )
                .is_err()
        );
        assert_eq!(registry.active.as_deref(), Some("First"));
        assert_eq!(registry.loaded.len(), 1);
    }
}
