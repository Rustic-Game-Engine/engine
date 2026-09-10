use crate::{RuntimeChange, RuntimeChangeSet, RuntimeChangeTarget, RuntimeValue};
use engine_scripting::{
    ActionState, EngineValue, ExternalBehavior, GameSettings, GameplayHost, JavaScriptBehavior,
    LuaBehavior, ScriptId, ScriptLanguage, WebBehavior, load_manifest,
};
use engine_world::{EntityId, Mesh, SceneWorld, WorldCommand, load_scene};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

const MAX_SCRIPT_LOGS: usize = 1_024;

enum RuntimeBehavior {
    Lua(LuaBehavior),
    JavaScript(JavaScriptBehavior),
    External(ExternalBehavior),
    Web(WebBehavior),
}

impl RuntimeBehavior {
    fn load(
        language: ScriptLanguage,
        script_id: ScriptId,
        source: &[u8],
        source_name: &str,
        host: Box<dyn GameplayHost>,
    ) -> Result<Self, String> {
        match language {
            ScriptLanguage::Lua54 => {
                LuaBehavior::load(script_id, source, source_name, host, 200_000)
                    .map(Self::Lua)
                    .map_err(|error| error.to_string())
            }
            ScriptLanguage::JavaScript => {
                JavaScriptBehavior::load(script_id, source, source_name, host, 200_000)
                    .map(Self::JavaScript)
                    .map_err(|error| error.to_string())
            }
            ScriptLanguage::Web => WebBehavior::load(script_id, source, source_name, host, 200_000)
                .map(Self::Web)
                .map_err(|error| error.to_string()),
            other => ExternalBehavior::load(script_id, other, source, source_name, host)
                .map(Self::External)
                .map_err(|error| error.to_string()),
        }
    }

    fn replacement(&self, source: &[u8]) -> Result<Self, String> {
        match self {
            Self::Lua(old) => LuaBehavior::load_shared(
                old.script_id(),
                source,
                "hot-reload.lua",
                old.host(),
                200_000,
            )
            .map(Self::Lua)
            .map_err(|error| error.to_string()),
            Self::JavaScript(old) => JavaScriptBehavior::load_shared(
                old.script_id(),
                source,
                "hot-reload.js",
                old.host(),
                200_000,
            )
            .map(Self::JavaScript)
            .map_err(|error| error.to_string()),
            Self::External(old) => ExternalBehavior::load_shared(
                old.script_id(),
                old.language(),
                source,
                "hot-reload.source",
                old.host(),
            )
            .map(Self::External)
            .map_err(|error| error.to_string()),
            Self::Web(old) => WebBehavior::load_shared(
                old.script_id(),
                source,
                "hot-reload.html",
                old.host(),
                200_000,
            )
            .map(Self::Web)
            .map_err(|error| error.to_string()),
        }
    }

    fn script_id(&self) -> ScriptId {
        match self {
            Self::Lua(value) => value.script_id(),
            Self::JavaScript(value) => value.script_id(),
            Self::External(value) => value.script_id(),
            Self::Web(value) => value.script_id(),
        }
    }

    fn on_create(&mut self) -> Result<(), String> {
        match self {
            Self::Lua(value) => value.on_create().map_err(|error| error.to_string()),
            Self::JavaScript(value) => value.on_create().map_err(|error| error.to_string()),
            Self::External(value) => value.on_create().map_err(|error| error.to_string()),
            Self::Web(value) => value.on_create().map_err(|error| error.to_string()),
        }
    }

    fn on_start(&mut self) -> Result<(), String> {
        match self {
            Self::Lua(value) => value.on_start().map_err(|error| error.to_string()),
            Self::JavaScript(value) => value.on_start().map_err(|error| error.to_string()),
            Self::External(value) => value.on_start().map_err(|error| error.to_string()),
            Self::Web(value) => value.on_start().map_err(|error| error.to_string()),
        }
    }

    fn fixed_update(&mut self, delta: f64) -> Result<(), String> {
        match self {
            Self::Lua(value) => value.fixed_update(delta).map_err(|error| error.to_string()),
            Self::JavaScript(value) => value.fixed_update(delta).map_err(|error| error.to_string()),
            Self::External(value) => value.fixed_update(delta).map_err(|error| error.to_string()),
            Self::Web(value) => value.fixed_update(delta).map_err(|error| error.to_string()),
        }
    }

    fn update(&mut self, delta: f64) -> Result<(), String> {
        match self {
            Self::Lua(value) => value.update(delta).map_err(|error| error.to_string()),
            Self::JavaScript(value) => value.update(delta).map_err(|error| error.to_string()),
            Self::External(value) => value.update(delta).map_err(|error| error.to_string()),
            Self::Web(value) => value.update(delta).map_err(|error| error.to_string()),
        }
    }

    fn on_destroy(&mut self) -> Result<(), String> {
        match self {
            Self::Lua(value) => value.on_destroy().map_err(|error| error.to_string()),
            Self::JavaScript(value) => value.on_destroy().map_err(|error| error.to_string()),
            Self::External(value) => value.on_destroy().map_err(|error| error.to_string()),
            Self::Web(value) => value.on_destroy().map_err(|error| error.to_string()),
        }
    }

    fn on_stop(&mut self) -> Result<(), String> {
        match self {
            Self::Lua(value) => value.on_stop().map_err(|error| error.to_string()),
            Self::JavaScript(value) => value.on_stop().map_err(|error| error.to_string()),
            Self::External(value) => value.on_stop().map_err(|error| error.to_string()),
            Self::Web(value) => value.on_stop().map_err(|error| error.to_string()),
        }
    }
}

pub(crate) struct RuntimeScripts {
    behaviors: Vec<RuntimeBehavior>,
    world: Arc<Mutex<SceneWorld>>,
    logs: Arc<Mutex<VecDeque<(String, String)>>>,
    dropped_logs: Arc<Mutex<u64>>,
    valid_scene: bool,
}

impl RuntimeScripts {
    pub fn load(snapshot_root: &std::path::Path, scene_bytes: &[u8]) -> Result<Self, String> {
        let manifest_path = snapshot_root.join("config/scripts.ron");
        let parsed_scene = load_scene(scene_bytes).ok();
        if manifest_path.is_file() && parsed_scene.is_none() {
            return Err("scripted snapshot contains an invalid scene".into());
        }
        let valid_scene = parsed_scene.is_some();
        let world = Arc::new(Mutex::new(match parsed_scene {
            Some(scene) => scene
                .document
                .create_world()
                .map_err(|error| error.to_string())?,
            None => SceneWorld::new(),
        }));
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        let dropped_logs = Arc::new(Mutex::new(0));
        let entries = if manifest_path.is_file() {
            load_manifest(&std::fs::read(&manifest_path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?
                .manifest
                .scripts
        } else {
            Vec::new()
        };
        let entries: BTreeMap<_, _> = entries.into_iter().map(|entry| (entry.id, entry)).collect();
        let snapshots = {
            let guard = world
                .lock()
                .map_err(|_| "runtime world lock poisoned".to_owned())?;
            guard
                .entity_ids()
                .map(|id| guard.snapshot(id).map_err(|error| error.to_string()))
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut behaviors = Vec::new();
        let settings_path = snapshot_root.join(engine_scripting::GAME_SETTINGS_FILE);
        let settings = if settings_path.is_file() {
            // Never guess an entry point from manifest ordering. In particular, UUID-keyed
            // manifest maps do not preserve the script order shown by the editor.
            Some(GameSettings::load(snapshot_root)?)
        } else {
            None
        };
        if let Some(settings) = &settings {
            let entry = entries
                .values()
                .find(|entry| entry.relative_path == settings.entry_script);
            let entry_language = entry
                .map_or_else(
                    || ScriptLanguage::from_path(&settings.entry_script),
                    |entry| Some(entry.language),
                )
                .ok_or_else(|| "entry script has an unsupported extension".to_owned())?;
            let entry_id = entry.map_or_else(ScriptId::new, |entry| entry.id);
            let entry_entity = snapshots
                .first()
                .map_or_else(EntityId::new, |entity| entity.id);
            let entry_source_path = snapshot_root.join(&settings.entry_script);
            let entry_source = std::fs::read(&entry_source_path).map_err(|error| {
                format!(
                    "could not read entry script {}: {error}",
                    entry_source_path.display()
                )
            })?;
            let entry_host = RuntimeHost {
                world: Arc::clone(&world),
                snapshot_root: snapshot_root.to_path_buf(),
                entity: entry_entity,
                properties: Arc::new(Mutex::new(BTreeMap::new())),
                logs: Arc::clone(&logs),
                dropped_logs: Arc::clone(&dropped_logs),
                enabled: true,
            };
            let mut entry_behavior = RuntimeBehavior::load(
                entry_language,
                entry_id,
                &entry_source,
                &settings.entry_script.to_string_lossy(),
                Box::new(entry_host),
            )?;
            initialize_behavior(&mut entry_behavior, &logs, &dropped_logs);
            behaviors.push(entry_behavior);
        }
        for entity in snapshots {
            for component in entity.scripts {
                if !component.enabled {
                    continue;
                }
                let entry = entries.get(&component.script_id).ok_or_else(|| {
                    format!(
                        "entity {} references missing script {}",
                        entity.id, component.script_id
                    )
                })?;
                if settings
                    .as_ref()
                    .is_some_and(|settings| entry.relative_path == settings.entry_script)
                {
                    continue;
                }
                let source_path = snapshot_root.join(&entry.relative_path);
                let source = std::fs::read(&source_path).map_err(|error| {
                    format!("could not read {}: {error}", source_path.display())
                })?;
                let host = RuntimeHost {
                    world: Arc::clone(&world),
                    snapshot_root: snapshot_root.to_path_buf(),
                    entity: entity.id,
                    properties: Arc::new(Mutex::new(component.properties)),
                    logs: Arc::clone(&logs),
                    dropped_logs: Arc::clone(&dropped_logs),
                    enabled: true,
                };
                let mut behavior = RuntimeBehavior::load(
                    entry.language,
                    component.script_id,
                    &source,
                    &entry.relative_path.to_string_lossy(),
                    Box::new(host),
                )?;
                initialize_behavior(&mut behavior, &logs, &dropped_logs);
                behaviors.push(behavior);
            }
        }
        Ok(Self {
            behaviors,
            world,
            logs,
            dropped_logs,
            valid_scene,
        })
    }

    pub fn fixed_update(&mut self, delta: f64) {
        for behavior in &mut self.behaviors {
            if let Err(error) = behavior.fixed_update(delta) {
                push_log(
                    &self.logs,
                    &self.dropped_logs,
                    "error",
                    &format!(
                        "script {} fixed_update failed and was disabled: {error}",
                        behavior.script_id()
                    ),
                );
            }
        }
    }
    pub fn frame_update(&mut self, delta: f64) {
        for behavior in &mut self.behaviors {
            if let Err(error) = behavior.update(delta) {
                push_log(
                    &self.logs,
                    &self.dropped_logs,
                    "error",
                    &format!(
                        "script {} update failed and was disabled: {error}",
                        behavior.script_id()
                    ),
                );
            }
        }
    }
    pub fn stop(&mut self) {
        for behavior in &mut self.behaviors {
            let _ = behavior.on_destroy();
            let _ = behavior.on_stop();
        }
    }
    pub fn reload(
        &mut self,
        script_id: engine_scripting::ScriptId,
        source: &[u8],
    ) -> Result<usize, String> {
        let indices: Vec<_> = self
            .behaviors
            .iter()
            .enumerate()
            .filter_map(|(index, behavior)| (behavior.script_id() == script_id).then_some(index))
            .collect();
        if indices.is_empty() {
            return Err(format!("script {script_id} has no running instances"));
        }
        let mut replacements = Vec::with_capacity(indices.len());
        for index in &indices {
            let old = &self.behaviors[*index];
            let mut replacement = old.replacement(source)?;
            replacement.on_create()?;
            replacement.on_start()?;
            replacements.push(replacement);
        }
        for (index, replacement) in indices.into_iter().zip(replacements) {
            self.behaviors[index] = replacement;
        }
        Ok(self
            .behaviors
            .iter()
            .filter(|behavior| behavior.script_id() == script_id)
            .count())
    }
    pub fn drain_logs(&self) -> Vec<(String, String)> {
        self.logs
            .lock()
            .map_or_else(|_| Vec::new(), |mut logs| logs.drain(..).collect())
    }
    pub fn runtime_changes(
        &self,
        base_hash: &str,
        original_scene: &[u8],
    ) -> Result<RuntimeChangeSet, String> {
        if !self.valid_scene {
            return Ok(RuntimeChangeSet::new(base_hash, Vec::new()));
        }
        let original = load_scene(original_scene)
            .map_err(|error| error.to_string())?
            .document;
        let before: BTreeMap<_, _> = original
            .entities
            .into_iter()
            .map(|entity| {
                (
                    entity.id,
                    entity.local_transform.translation.to_array().map(f64::from),
                )
            })
            .collect();
        let world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned")?;
        let mut changes = Vec::new();
        for (entity, original) in before {
            let current = world
                .local_transform(entity)
                .map_err(|error| error.to_string())?
                .translation
                .to_array()
                .map(f64::from);
            if current
                .iter()
                .zip(original.iter())
                .any(|(left, right)| (left - right).abs() > f64::EPSILON)
            {
                changes.push(RuntimeChange {
                    target: RuntimeChangeTarget {
                        entity_id: entity.to_string(),
                        component_schema: "transform.v1".into(),
                        property: "translation".into(),
                    },
                    before: RuntimeValue::Vector3(original),
                    after: RuntimeValue::Vector3(current),
                });
            }
        }
        Ok(RuntimeChangeSet::new(base_hash, changes))
    }
}

fn initialize_behavior(
    behavior: &mut RuntimeBehavior,
    logs: &Arc<Mutex<VecDeque<(String, String)>>>,
    dropped_logs: &Arc<Mutex<u64>>,
) {
    if let Err(error) = behavior.on_create() {
        push_log(
            logs,
            dropped_logs,
            "error",
            &format!(
                "script {} on_create failed and was disabled: {error}",
                behavior.script_id()
            ),
        );
        return;
    }
    if let Err(error) = behavior.on_start() {
        push_log(
            logs,
            dropped_logs,
            "error",
            &format!(
                "script {} on_start failed and was disabled: {error}",
                behavior.script_id()
            ),
        );
    }
}

fn push_log(
    logs: &Arc<Mutex<VecDeque<(String, String)>>>,
    dropped: &Arc<Mutex<u64>>,
    level: &str,
    message: &str,
) {
    if let Ok(mut logs) = logs.lock() {
        if logs.len() >= MAX_SCRIPT_LOGS {
            logs.pop_front();
            if let Ok(mut count) = dropped.lock() {
                *count = count.saturating_add(1);
            }
        }
        logs.push_back((level.into(), message.chars().take(4096).collect()));
    }
}

struct RuntimeHost {
    world: Arc<Mutex<SceneWorld>>,
    snapshot_root: PathBuf,
    entity: EntityId,
    properties: Arc<Mutex<BTreeMap<String, EngineValue>>>,
    logs: Arc<Mutex<VecDeque<(String, String)>>>,
    dropped_logs: Arc<Mutex<u64>>,
    enabled: bool,
}

impl GameplayHost for RuntimeHost {
    fn entity_id(&self) -> EntityId {
        self.entity
    }
    fn delta_time(&self) -> f64 {
        1.0 / 60.0
    }
    fn fixed_delta_time(&self) -> f64 {
        1.0 / 60.0
    }
    fn translation(&self) -> [f64; 3] {
        self.world
            .lock()
            .ok()
            .and_then(|world| world.local_transform(self.entity).ok())
            .map_or([0.0; 3], |value| value.translation.as_dvec3().to_array())
    }
    fn set_translation(&mut self, value: [f64; 3]) -> Result<(), String> {
        if !value.iter().all(|value| value.is_finite()) {
            return Err("transform contains a non-finite value".into());
        }
        let mut world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned")?;
        let mut transform = world
            .local_transform(self.entity)
            .map_err(|error| error.to_string())?;
        transform.translation = glam::DVec3::from_array(value).as_vec3();
        world
            .apply_commands(&[WorldCommand::SetLocalTransform {
                entity: self.entity,
                value: transform,
            }])
            .map_err(|error| error.to_string())
    }
    fn find_entity(&self, query: &str) -> Result<Option<EntityId>, String> {
        let world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned")?;
        if let Ok(id) = query.parse::<EntityId>() {
            return Ok(world.contains(id).then_some(id));
        }
        if let Some(id) = world.find_path(query).map_err(|error| error.to_string())? {
            return Ok(Some(id));
        }
        for id in world.entity_ids() {
            if world
                .snapshot(id)
                .map_err(|error| error.to_string())?
                .name
                .as_deref()
                == Some(query)
            {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }
    fn list_entities(&self, path: &str) -> Result<Vec<EntityId>, String> {
        self.world
            .lock()
            .map_err(|_| "runtime world lock poisoned".to_owned())?
            .list_path(path)
            .map_err(|error| error.to_string())
    }
    fn scene_paths(&self) -> BTreeMap<String, EntityId> {
        let Ok(world) = self.world.lock() else {
            return BTreeMap::new();
        };
        let mut paths = BTreeMap::new();
        for id in world.entity_ids() {
            if let Ok(path) = world.entity_path(id) {
                paths.insert(path.clone(), id);
                paths.insert(format!("Game.scene.{path}"), id);
            }
        }
        paths
    }
    fn add_instance(&mut self, source: &str, parent: Option<EntityId>) -> Result<EntityId, String> {
        if self.resolve_entity(source)?.is_some() {
            return self.clone_instance(source, parent);
        }
        if let Some(snapshot) = self.asset_instance(source, parent)? {
            let id = snapshot.id;
            self.world
                .lock()
                .map_err(|_| "runtime world lock poisoned".to_owned())?
                .apply_commands(&[WorldCommand::Spawn(Box::new(snapshot))])
                .map_err(|error| error.to_string())?;
            return Ok(id);
        }
        let name = source.rsplit(['/', '.']).next().unwrap_or(source);
        let primitive = match name.to_ascii_lowercase().as_str() {
            "part" | "cube" => engine_world::Primitive::Cube { size: 1.0 },
            "sphere" => engine_world::Primitive::Sphere {
                radius: 0.5,
                segments: 24,
                rings: 12,
            },
            "cylinder" => engine_world::Primitive::Cylinder {
                radius: 0.5,
                height: 1.0,
                segments: 24,
            },
            "plane" => engine_world::Primitive::Plane { size: [1.0, 1.0] },
            "rectangle" | "rectangle2d" => {
                engine_world::Primitive::Rectangle2d { size: [1.0, 1.0] }
            }
            "circle" | "circle2d" => engine_world::Primitive::Circle2d {
                radius: 0.5,
                segments: 24,
            },
            _ => {
                return Err(format!(
                    "instance source `{source}` was not found; use an entity ID, scene/explorer path, or built-in object name"
                ));
            }
        };
        let snapshot = engine_world::EntitySnapshot {
            name: Some(name.to_owned()),
            parent,
            primitive: Some(primitive),
            ..engine_world::EntitySnapshot::default()
        };
        let id = snapshot.id;
        self.world
            .lock()
            .map_err(|_| "runtime world lock poisoned".to_owned())?
            .apply_commands(&[WorldCommand::Spawn(Box::new(snapshot))])
            .map_err(|error| error.to_string())?;
        Ok(id)
    }
    fn clone_instance(
        &mut self,
        source: &str,
        parent: Option<EntityId>,
    ) -> Result<EntityId, String> {
        let source_id = self
            .resolve_entity(source)?
            .ok_or_else(|| format!("instance source `{source}` was not found"))?;
        let mut world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned".to_owned())?;
        let mut snapshot = world
            .snapshot(source_id)
            .map_err(|error| error.to_string())?;
        snapshot.id = EntityId::new();
        snapshot.parent = parent.or(snapshot.parent);
        let id = snapshot.id;
        world
            .apply_commands(&[WorldCommand::Spawn(Box::new(snapshot))])
            .map_err(|error| error.to_string())?;
        Ok(id)
    }
    fn attribute(&self, name: &str) -> Result<Option<EngineValue>, String> {
        let world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned".to_owned())?;
        let snapshot = world
            .snapshot(self.entity)
            .map_err(|error| error.to_string())?;
        let value = match name
            .to_ascii_lowercase()
            .replace(['_', '-', ' '], "")
            .as_str()
        {
            "name" => EngineValue::String(snapshot.name.unwrap_or_default()),
            "position" => EngineValue::Vec3(
                snapshot
                    .local_transform
                    .translation
                    .to_array()
                    .map(f64::from),
            ),
            "size" => EngineValue::Vec3(snapshot.local_transform.scale.to_array().map(f64::from)),
            "color" => EngineValue::Vec3(
                snapshot.part_attributes.color[..3]
                    .try_into()
                    .unwrap_or([1.0; 3])
                    .map(f64::from),
            ),
            "cantouch" => EngineValue::Boolean(snapshot.part_attributes.can_touch),
            "cancollide" => EngineValue::Boolean(snapshot.part_attributes.can_collide),
            "anchored" => EngineValue::Boolean(snapshot.part_attributes.anchored),
            "parent" => EngineValue::Entity(snapshot.parent),
            _ => return Ok(None),
        };
        Ok(Some(value))
    }
    fn edit_attribute(&mut self, name: &str, value: EngineValue) -> Result<(), String> {
        let mut world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned".to_owned())?;
        let key = name.to_ascii_lowercase().replace(['_', '-', ' '], "");
        match (key.as_str(), value) {
            ("name", EngineValue::String(value)) => {
                world.apply_commands(&[WorldCommand::SetName {
                    entity: self.entity,
                    value: Some(value),
                }])
            }
            ("position", EngineValue::Vec3(value)) | ("size", EngineValue::Vec3(value)) => {
                if !value.iter().all(|value| value.is_finite()) {
                    return Err("attribute contains a non-finite value".into());
                }
                let mut transform = world
                    .local_transform(self.entity)
                    .map_err(|error| error.to_string())?;
                let converted = glam::DVec3::from_array(value).as_vec3();
                if key == "position" {
                    transform.translation = converted;
                } else {
                    transform.scale = converted;
                }
                world.apply_commands(&[WorldCommand::SetLocalTransform {
                    entity: self.entity,
                    value: transform,
                }])
            }
            ("parent", EngineValue::Entity(parent)) => {
                world.apply_commands(&[WorldCommand::SetParent {
                    child: self.entity,
                    parent,
                }])
            }
            (property @ ("color" | "cantouch" | "cancollide" | "anchored"), value) => {
                let mut attributes = world
                    .part_attributes(self.entity)
                    .map_err(|error| error.to_string())?;
                match (property, value) {
                    ("color", EngineValue::Vec3(value)) if value.iter().all(|v| v.is_finite()) => {
                        attributes.color[..3].copy_from_slice(&value.map(|v| v as f32))
                    }
                    ("cantouch", EngineValue::Boolean(value)) => attributes.can_touch = value,
                    ("cancollide", EngineValue::Boolean(value)) => attributes.can_collide = value,
                    ("anchored", EngineValue::Boolean(value)) => attributes.anchored = value,
                    _ => return Err(format!("attribute `{name}` type mismatch")),
                }
                world.apply_commands(&[WorldCommand::SetPartAttributes {
                    entity: self.entity,
                    value: attributes,
                }])
            }
            _ => return Err(format!("unknown attribute `{name}` or type mismatch")),
        }
        .map_err(|error| error.to_string())?;
        world.propagate_transforms();
        Ok(())
    }
    fn input_action(&self, _name: &str) -> ActionState {
        ActionState::default()
    }
    fn log(&mut self, level: &str, message: &str) -> Result<(), String> {
        let mut logs = self.logs.lock().map_err(|_| "script log lock poisoned")?;
        if logs.len() >= MAX_SCRIPT_LOGS {
            logs.pop_front();
            if let Ok(mut dropped) = self.dropped_logs.lock() {
                *dropped = dropped.saturating_add(1);
            }
        }
        logs.push_back((level.to_owned(), message.chars().take(4096).collect()));
        Ok(())
    }
    fn property(&self, name: &str) -> Option<EngineValue> {
        self.properties.lock().ok()?.get(name).cloned()
    }
    fn properties(&self) -> BTreeMap<String, EngineValue> {
        self.properties
            .lock()
            .map_or_else(|_| BTreeMap::new(), |properties| properties.clone())
    }
    fn set_property(&mut self, name: &str, value: EngineValue) -> Result<(), String> {
        let mut properties = self
            .properties
            .lock()
            .map_err(|_| "property lock poisoned")?;
        let old = properties
            .get(name)
            .ok_or_else(|| format!("property `{name}` is not declared"))?;
        if !old.same_type(&value) {
            return Err(format!("property `{name}` type mismatch"));
        }
        properties.insert(name.into(), value);
        Ok(())
    }
    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

impl RuntimeHost {
    fn resolve_entity(&self, source: &str) -> Result<Option<EntityId>, String> {
        let world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned".to_owned())?;
        if let Ok(id) = source.parse::<EntityId>() {
            return Ok(world.contains(id).then_some(id));
        }
        world.find_path(source).map_err(|error| error.to_string())
    }

    fn asset_instance(
        &self,
        source: &str,
        parent: Option<EntityId>,
    ) -> Result<Option<engine_world::EntitySnapshot>, String> {
        let relative = Path::new(source);
        if relative.is_absolute()
            || relative
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            || relative
                .components()
                .next()
                .is_none_or(|part| part.as_os_str() != "assets")
        {
            return Ok(None);
        }
        let extension = relative
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "obj" | "gltf" | "glb") {
            return Err(format!(
                "asset `{source}` is not a supported model (expected .obj, .gltf, or .glb)"
            ));
        }
        let asset_path = self.snapshot_root.join(relative);
        if !asset_path.is_file() {
            return Err(format!(
                "asset `{source}` is not present in the play snapshot"
            ));
        }
        let meta_path = engine_assets::sidecar_path(&asset_path);
        let meta = engine_assets::load_meta(&meta_path)
            .map_err(|error| format!("asset `{source}` has no valid import metadata: {error}"))?;
        Ok(Some(engine_world::EntitySnapshot {
            name: relative
                .file_stem()
                .and_then(|value| value.to_str())
                .map(str::to_owned),
            parent,
            mesh: Some(Mesh {
                asset: meta.asset_id,
            }),
            ..engine_world::EntitySnapshot::default()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_scripting::{
        ScriptApiVersion, ScriptId, ScriptLanguage, ScriptManifest, ScriptManifestEntry,
        save_manifest_atomic,
    };
    use engine_world::{EntitySnapshot, SceneDocument, ScriptComponent};

    #[test]
    fn hot_reload_success_and_compile_failure_retain_last_good_state() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let script_id = ScriptId::new();
        let entity = EntitySnapshot {
            scripts: vec![ScriptComponent::new(script_id)],
            ..EntitySnapshot::default()
        };
        let entity_id = entity.id;
        let scene = SceneDocument {
            id: engine_core::SceneId::new(),
            name: "Test".into(),
            entities: vec![entity],
            instances: Vec::new(),
        };
        let scene_bytes = scene.to_bytes().unwrap();
        let relative = std::path::PathBuf::from("scripts/test.lua");
        std::fs::write(temp.path().join(&relative), b"return { fixed_update=function(dt) local x,y,z=rustic.get_translation(); rustic.set_translation(x+1,y,z) end }").unwrap();
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![ScriptManifestEntry {
                    id: script_id,
                    language: ScriptLanguage::Lua54,
                    relative_path: relative,
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                }],
            },
        )
        .unwrap();
        let mut runtime = RuntimeScripts::load(temp.path(), &scene_bytes).unwrap();
        runtime.fixed_update(0.016);
        assert!(runtime.reload(script_id, b"return { broken =").is_err());
        runtime.fixed_update(0.016);
        runtime.reload(script_id, b"return { fixed_update=function(dt) local x,y,z=rustic.get_translation(); rustic.set_translation(x+2,y,z) end }").unwrap();
        runtime.fixed_update(0.016);
        let changes = runtime.runtime_changes("base", &scene_bytes).unwrap();
        assert_eq!(changes.changes.len(), 1);
        assert_eq!(changes.changes[0].target.entity_id, entity_id.to_string());
        assert_eq!(
            changes.changes[0].after,
            RuntimeValue::Vector3([4.0, 0.0, 0.0])
        );
    }

    #[test]
    fn lua_and_javascript_behaviors_run_in_the_same_scene() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let lua_id = ScriptId::new();
        let javascript_id = ScriptId::new();
        let entity = EntitySnapshot {
            scripts: vec![
                ScriptComponent::new(lua_id),
                ScriptComponent::new(javascript_id),
            ],
            ..EntitySnapshot::default()
        };
        let scene = SceneDocument {
            id: engine_core::SceneId::new(),
            name: "Mixed".into(),
            entities: vec![entity],
            instances: Vec::new(),
        };
        let scene_bytes = scene.to_bytes().unwrap();
        let lua_path = std::path::PathBuf::from("scripts/move.lua");
        let javascript_path = std::path::PathBuf::from("scripts/move.js");
        std::fs::write(
            temp.path().join(&lua_path),
            b"return { fixed_update=function() local x,y,z=rustic.get_translation(); rustic.set_translation(x+1,y,z) end }",
        )
        .unwrap();
        std::fs::write(
            temp.path().join(&javascript_path),
            b"globalThis.behavior={fixed_update(){const [x,y,z]=rustic.get_translation();rustic.set_translation(x+2,y,z);}};",
        )
        .unwrap();
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![
                    ScriptManifestEntry {
                        id: lua_id,
                        language: ScriptLanguage::Lua54,
                        relative_path: lua_path,
                        api_version: ScriptApiVersion::CURRENT,
                        public_properties: Vec::new(),
                    },
                    ScriptManifestEntry {
                        id: javascript_id,
                        language: ScriptLanguage::JavaScript,
                        relative_path: javascript_path,
                        api_version: ScriptApiVersion::CURRENT,
                        public_properties: Vec::new(),
                    },
                ],
            },
        )
        .unwrap();
        let mut runtime = RuntimeScripts::load(temp.path(), &scene_bytes).unwrap();
        runtime.fixed_update(1.0 / 60.0);
        assert!(
            runtime
                .reload(javascript_id, b"globalThis.behavior={")
                .is_err()
        );
        runtime.fixed_update(1.0 / 60.0);
        runtime
            .reload(
                javascript_id,
                b"globalThis.behavior={fixed_update(){const [x,y,z]=rustic.get_translation();rustic.set_translation(x+4,y,z);}};",
            )
            .unwrap();
        runtime.fixed_update(1.0 / 60.0);
        let changes = runtime.runtime_changes("base", &scene_bytes).unwrap();
        assert_eq!(changes.changes.len(), 1);
        assert_eq!(
            changes.changes[0].after,
            RuntimeValue::Vector3([11.0, 0.0, 0.0])
        );
    }

    #[test]
    fn configured_entry_script_runs_top_level_without_on_start() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let scene = SceneDocument {
            id: engine_core::SceneId::new(),
            name: "Entry".into(),
            entities: vec![EntitySnapshot::default()],
            instances: Vec::new(),
        };
        GameSettings {
            entry_script: "scripts/main.lua".into(),
        }
        .save(temp.path())
        .unwrap();
        std::fs::write(
            temp.path().join("scripts/main.lua"),
            b"rustic.log('info', 'entry ran')",
        )
        .unwrap();

        let runtime = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap()).unwrap();
        assert_eq!(
            runtime.drain_logs(),
            vec![("info".to_owned(), "entry ran".to_owned())]
        );
    }

    #[test]
    fn entry_script_api_errors_are_reported_without_panicking() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let script_id = ScriptId::new();
        let scene = SceneDocument {
            id: engine_core::SceneId::new(),
            name: "Entry API error".into(),
            entities: vec![EntitySnapshot::default()],
            instances: Vec::new(),
        };
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![ScriptManifestEntry {
                    id: script_id,
                    language: ScriptLanguage::Lua54,
                    relative_path: "scripts/main.lua".into(),
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                }],
            },
        )
        .unwrap();
        GameSettings {
            entry_script: "scripts/main.lua".into(),
        }
        .save(temp.path())
        .unwrap();
        std::fs::write(
            temp.path().join("scripts/main.lua"),
            b"return { on_create=function() rustic.set_property('missing', 1.0) end }",
        )
        .unwrap();

        let runtime = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap())
            .expect("a callback error must not abort runtime startup");
        let logs = runtime.drain_logs();
        assert_eq!(logs.len(), 1);
        assert!(logs[0].1.contains("on_create failed and was disabled"));
        assert!(logs[0].1.contains("not declared"));
    }

    #[test]
    fn configured_entry_script_wins_over_other_manifest_scripts() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let scene = SceneDocument {
            id: engine_core::SceneId::new(),
            name: "Configured entry".into(),
            entities: vec![EntitySnapshot::default()],
            instances: Vec::new(),
        };
        let configured_id = ScriptId::new();
        let other_id = ScriptId::new();
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![
                    ScriptManifestEntry {
                        id: other_id,
                        language: ScriptLanguage::Lua54,
                        relative_path: "scripts/other.lua".into(),
                        api_version: ScriptApiVersion::CURRENT,
                        public_properties: Vec::new(),
                    },
                    ScriptManifestEntry {
                        id: configured_id,
                        language: ScriptLanguage::Lua54,
                        relative_path: "scripts/chosen.lua".into(),
                        api_version: ScriptApiVersion::CURRENT,
                        public_properties: Vec::new(),
                    },
                ],
            },
        )
        .unwrap();
        std::fs::write(
            temp.path().join("scripts/other.lua"),
            b"rustic.log('info', 'wrong entry')",
        )
        .unwrap();
        std::fs::write(
            temp.path().join("scripts/chosen.lua"),
            b"rustic.log('info', 'chosen entry')",
        )
        .unwrap();
        GameSettings {
            entry_script: "scripts/chosen.lua".into(),
        }
        .save(temp.path())
        .unwrap();

        let runtime = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap()).unwrap();

        assert_eq!(
            runtime.drain_logs(),
            vec![("info".to_owned(), "chosen entry".to_owned())]
        );
    }

    #[test]
    fn invalid_settings_do_not_fall_back_to_an_arbitrary_manifest_script() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        std::fs::write(temp.path().join("settings.json"), b"not json").unwrap();
        std::fs::write(temp.path().join("scripts/other.lua"), b"return {}").unwrap();
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![ScriptManifestEntry {
                    id: ScriptId::new(),
                    language: ScriptLanguage::Lua54,
                    relative_path: "scripts/other.lua".into(),
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                }],
            },
        )
        .unwrap();
        let scene = SceneDocument {
            id: engine_core::SceneId::new(),
            name: "Invalid settings".into(),
            entities: vec![EntitySnapshot::default()],
            instances: Vec::new(),
        };

        let error = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap())
            .err()
            .expect("invalid settings must stop runtime startup");

        assert!(error.contains("invalid"));
        assert!(error.contains("settings.json"));
    }
}
