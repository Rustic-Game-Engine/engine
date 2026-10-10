use crate::{
    LiveEntityProperties, RuntimeChange, RuntimeChangeSet, RuntimeChangeTarget, RuntimeValue,
};
use engine_core::gameplay::{Delivery, GameplayRuntime, OperationState, Owner, Request};
use engine_scripting::{
    ActionState, EngineValue, ExternalBehavior, GameSettings, GameplayHost, InputFrame,
    JavaScriptBehavior, LuaBehavior, ScriptId, ScriptLanguage, ScriptReference, WebBehavior,
    load_manifest,
};
use engine_world::{EntityId, Mesh, SceneWorld, WorldCommand, load_scene};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
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

    fn host(&self) -> Arc<Mutex<Box<dyn GameplayHost>>> {
        match self {
            Self::Lua(v) => v.host(),
            Self::JavaScript(v) => v.host(),
            Self::External(v) => v.host(),
            Self::Web(v) => v.host(),
        }
    }
    fn replacement(&self, source: &[u8]) -> Result<Self, String> {
        let original = self.host();
        let host = original
            .lock()
            .map_err(|_| "gameplay host lock poisoned")?
            .fork_for_reload()
            .map_or_else(|| Arc::clone(&original), |host| Arc::new(Mutex::new(host)));
        let result = match self {
            Self::Lua(old) => LuaBehavior::load_shared(
                old.script_id(),
                source,
                "hot-reload.lua",
                Arc::clone(&host),
                200_000,
            )
            .map(Self::Lua)
            .map_err(|e| e.to_string()),
            Self::JavaScript(old) => JavaScriptBehavior::load_shared(
                old.script_id(),
                source,
                "hot-reload.js",
                Arc::clone(&host),
                200_000,
            )
            .map(Self::JavaScript)
            .map_err(|e| e.to_string()),
            Self::External(old) => ExternalBehavior::load_shared(
                old.script_id(),
                old.language(),
                source,
                "hot-reload.source",
                Arc::clone(&host),
            )
            .map(Self::External)
            .map_err(|e| e.to_string()),
            Self::Web(old) => WebBehavior::load_shared(
                old.script_id(),
                source,
                "hot-reload.html",
                Arc::clone(&host),
                200_000,
            )
            .map(Self::Web)
            .map_err(|e| e.to_string()),
        };
        if result.is_err()
            && !Arc::ptr_eq(&host, &original)
            && let Ok(mut host) = host.lock()
        {
            host.cleanup_gameplay();
        }
        result
    }

    fn script_id(&self) -> ScriptId {
        match self {
            Self::Lua(value) => value.script_id(),
            Self::JavaScript(value) => value.script_id(),
            Self::External(value) => value.script_id(),
            Self::Web(value) => value.script_id(),
        }
    }

    fn cleanup_gameplay(&mut self) {
        let host = match self {
            Self::Lua(v) => v.host(),
            Self::JavaScript(v) => v.host(),
            Self::External(v) => v.host(),
            Self::Web(v) => v.host(),
        };
        if let Ok(mut host) = host.lock() {
            host.cleanup_gameplay();
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

    fn on_enable(&mut self) -> Result<(), String> {
        match self {
            Self::Lua(value) => value.on_enable().map_err(|error| error.to_string()),
            Self::JavaScript(value) => value.on_enable().map_err(|error| error.to_string()),
            Self::External(value) => value.on_enable().map_err(|error| error.to_string()),
            Self::Web(value) => value.on_enable().map_err(|error| error.to_string()),
        }
    }

    fn on_disable(&mut self) -> Result<(), String> {
        match self {
            Self::Lua(value) => value.on_disable().map_err(|error| error.to_string()),
            Self::JavaScript(value) => value.on_disable().map_err(|error| error.to_string()),
            Self::External(value) => value.on_disable().map_err(|error| error.to_string()),
            Self::Web(value) => value.on_disable().map_err(|error| error.to_string()),
        }
    }

    fn active(&self) -> bool {
        let (runtime_enabled, host) = match self {
            Self::Lua(value) => (value.enabled(), value.host()),
            Self::JavaScript(value) => (value.enabled(), value.host()),
            Self::External(value) => (value.enabled(), value.host()),
            Self::Web(value) => (value.enabled(), value.host()),
        };
        runtime_enabled && host.lock().is_ok_and(|host| host.enabled())
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

struct GameplayContext {
    scene: engine_core::SceneId,
    runtime: GameplayRuntime,
    physics: crate::physics::PhysicsWorld,
    audio: engine_core::gameplay::audio::Mixer,
    audio_output: crate::audio_output::AudioOutput,
    clips: BTreeMap<(EntityId, String), engine_core::gameplay::animation::Clip>,
    frame_delta: f64,
    fixed_delta: f64,
}

impl Default for GameplayContext {
    fn default() -> Self {
        Self {
            scene: engine_core::SceneId::new(),
            runtime: GameplayRuntime::default(),
            physics: crate::physics::PhysicsWorld::default(),
            audio: engine_core::gameplay::audio::Mixer::default(),
            audio_output: crate::audio_output::AudioOutput::new(),
            clips: BTreeMap::new(),
            frame_delta: 1.0 / 60.0,
            fixed_delta: 1.0 / 60.0,
        }
    }
}

pub(crate) struct RuntimeScripts {
    gameplay: Arc<Mutex<GameplayContext>>,
    behaviors: Vec<RuntimeBehavior>,
    world: Arc<Mutex<SceneWorld>>,
    input_keys: Arc<Mutex<BTreeSet<String>>>,
    logs: Arc<Mutex<VecDeque<(String, String)>>>,
    dropped_logs: Arc<Mutex<u64>>,
    valid_scene: bool,
}

impl RuntimeScripts {
    pub fn live_entity(&self, id: EntityId) -> Result<Option<LiveEntityProperties>, String> {
        let world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned")?;
        if !world.contains(id) {
            return Ok(None);
        }
        let snapshot = world.snapshot(id).map_err(|error| error.to_string())?;
        drop(world);
        let mut scripts = snapshot.scripts;
        for behavior in &self.behaviors {
            let host = match behavior {
                RuntimeBehavior::Lua(value) => value.host(),
                RuntimeBehavior::JavaScript(value) => value.host(),
                RuntimeBehavior::External(value) => value.host(),
                RuntimeBehavior::Web(value) => value.host(),
            };
            if let Ok(host) = host.lock()
                && host.entity_id() == id
                && let Some(script) = scripts
                    .iter_mut()
                    .find(|script| script.script_id == behavior.script_id())
            {
                script.properties = host.properties();
                script.enabled = host.enabled();
            }
        }
        Ok(Some(LiveEntityProperties {
            entity_id: id.to_string(),
            name: snapshot.name,
            translation: snapshot.local_transform.translation.to_array(),
            rotation: snapshot.local_transform.rotation.to_array(),
            scale: snapshot.local_transform.scale.to_array(),
            part_attributes: snapshot.part_attributes,
            scripts,
        }))
    }
    #[allow(clippy::too_many_lines)]
    pub fn load(snapshot_root: &std::path::Path, scene_bytes: &[u8]) -> Result<Self, String> {
        let manifest_path = snapshot_root.join("config/scripts.ron");
        let parsed_scene = load_scene(scene_bytes).ok();
        if manifest_path.is_file() && parsed_scene.is_none() {
            return Err("scripted snapshot contains an invalid scene".into());
        }
        let valid_scene = parsed_scene.is_some();
        let scene_name = parsed_scene
            .as_ref()
            .map(|scene| scene.document.name.clone())
            .unwrap_or_default();
        let mut scene_startup_scripts = parsed_scene
            .as_ref()
            .map(|scene| scene.document.startup_scripts.clone())
            .unwrap_or_default();
        sort_script_references(&mut scene_startup_scripts);
        let world = Arc::new(Mutex::new(match parsed_scene {
            Some(scene) => scene
                .document
                .create_world()
                .map_err(|error| error.to_string())?,
            None => SceneWorld::new(),
        }));
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        let dropped_logs = Arc::new(Mutex::new(0));
        let input_keys = Arc::new(Mutex::new(BTreeSet::new()));
        let gameplay = Arc::new(Mutex::new(GameplayContext::default()));
        let model_library =
            engine_assets::load_model_library(snapshot_root).map_err(|e| e.to_string())?;
        {
            let mut world = world.lock().map_err(|_| "world lock poisoned")?;
            let mut context = gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
            let entities = world.entity_ids().collect::<Vec<_>>();
            for entity in entities {
                let Some(mesh) = world.mesh(entity).map_err(|e| e.to_string())? else {
                    continue;
                };
                let Some((_, model)) = model_library.get(&mesh.asset) else {
                    continue;
                };
                bind_model(&mut world, &mut context, entity, model)?;
            }
        }
        let entries = if manifest_path.is_file() {
            load_manifest(&std::fs::read(&manifest_path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?
                .manifest
                .scripts
        } else {
            Vec::new()
        };
        let entries: BTreeMap<_, _> = entries.into_iter().map(|entry| (entry.id, entry)).collect();
        let mut snapshots = {
            let guard = world
                .lock()
                .map_err(|_| "runtime world lock poisoned".to_owned())?;
            guard
                .entity_ids()
                .map(|id| guard.snapshot(id).map_err(|error| error.to_string()))
                .collect::<Result<Vec<_>, _>>()?
        };
        snapshots.sort_by_key(|snapshot| snapshot.id);
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
            let mut global_scripts = settings.global_scripts.clone();
            sort_script_references(&mut global_scripts);
            load_startup_references(
                &global_scripts,
                &scene_name,
                &entries,
                snapshot_root,
                &world,
                &input_keys,
                &logs,
                &dropped_logs,
                &gameplay,
                &mut behaviors,
            )?;
        }
        load_startup_references(
            &scene_startup_scripts,
            &scene_name,
            &entries,
            snapshot_root,
            &world,
            &input_keys,
            &logs,
            &dropped_logs,
            &gameplay,
            &mut behaviors,
        )?;
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
                let source_path = snapshot_root.join(&entry.relative_path);
                let mut source = std::fs::read(&source_path).map_err(|error| {
                    format!("could not read {}: {error}", source_path.display())
                })?;
                if entry.language == ScriptLanguage::Web {
                    source =
                        resolve_linked_web_scripts(snapshot_root, &entry.relative_path, &source)?;
                }
                let host = RuntimeHost {
                    gameplay: Arc::clone(&gameplay),
                    owner_script: ScriptId::new(),
                    entity_bound: true,
                    scene_name: scene_name.clone(),
                    world: Arc::clone(&world),
                    input_keys: Arc::clone(&input_keys),
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
            gameplay,
            behaviors,
            world,
            input_keys,
            logs,
            dropped_logs,
            valid_scene,
        })
    }

    pub fn render_world<T>(
        &self,
        render: impl FnOnce(&engine_world::SceneWorld) -> T,
    ) -> Result<T, String> {
        let mut world = self.world.lock().map_err(|_| "world lock poisoned")?;
        world.propagate_transforms();
        Ok(render(&world))
    }
    pub fn set_input_keys(&mut self, keys: Vec<String>) {
        if let Ok(mut held) = self.input_keys.lock() {
            *held = keys
                .into_iter()
                .take(128)
                .filter(|key| key.len() <= 64)
                .collect();
        }
    }
    pub fn fixed_update(&mut self, delta: f64) {
        if let Ok(mut context) = self.gameplay.lock() {
            context.fixed_delta = delta;
        }
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
        self.unregister_disabled();
        if let Ok(mut world) = self.world.lock()
            && let Err(error) = self
                .gameplay
                .lock()
                .map_err(|_| "gameplay lock poisoned".to_owned())
                .and_then(|mut context| {
                    let scaled = context.runtime.scaled_delta(delta)?;
                    context
                        .physics
                        .step(&mut world, scaled)
                        .map_err(|e| e.to_string())
                })
        {
            push_log(
                &self.logs,
                &self.dropped_logs,
                "error",
                &format!("physics step failed: {error}"),
            );
        }
    }
    pub fn frame_update(&mut self, delta: f64) {
        if let Ok(mut context) = self.gameplay.lock() {
            context.frame_delta = delta;
        }
        // Tick once for the scene, independent of how many behaviors are attached.
        if let (Ok(mut gameplay), Ok(mut world)) = (self.gameplay.lock(), self.world.lock()) {
            gameplay
                .audio
                .cleanup(|owner| owner.entity_bound && !world.contains(owner.entity));
            gameplay
                .clips
                .retain(|(entity, _), _| world.contains(*entity));
            world.propagate_transforms();
            if let Some(camera) = world
                .entity_ids()
                .filter(|id| world.camera(*id).ok().flatten().is_some_and(|c| c.active))
                .last()
                && let Ok(transform) = world.world_transform(camera)
            {
                gameplay.audio.listener = transform.0.w_axis.truncate().as_dvec3().to_array();
            }
            let GameplayContext { runtime, audio, .. } = &mut *gameplay;
            if let Err(error) = runtime.tick(
                delta,
                &mut crate::gameplay_world::GameplayScene(&mut world, audio),
            ) {
                push_log(&self.logs, &self.dropped_logs, "error", &error);
            }
        }
        if let Ok(mut context) = self.gameplay.lock() {
            let scale = if context.runtime.paused {
                0.0
            } else {
                context.runtime.time_scale()
            };
            match context.audio.mix_scaled(delta, scale) {
                Ok(samples) => context.audio_output.submit(samples),
                Err(error) => push_log(&self.logs, &self.dropped_logs, "error", &error),
            }
        }
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
        self.unregister_disabled();
    }
    pub fn stop(&mut self) {
        for behavior in &mut self.behaviors {
            let _ = behavior.on_disable();
            let _ = behavior.on_destroy();
            let _ = behavior.on_stop();
            behavior.cleanup_gameplay();
        }
    }
    fn unregister_disabled(&mut self) {
        let mut active = Vec::with_capacity(self.behaviors.len());
        for mut behavior in self.behaviors.drain(..) {
            if behavior.active() {
                active.push(behavior);
            } else {
                let _ = behavior.on_disable();
                behavior.cleanup_gameplay();
            }
        }
        self.behaviors = active;
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
        let mut replacements: Vec<RuntimeBehavior> = Vec::with_capacity(indices.len());
        for index in &indices {
            let result = (|| {
                let mut replacement = self.behaviors[*index].replacement(source)?;
                if let Err(error) = replacement
                    .on_create()
                    .and_then(|()| replacement.on_enable())
                    .and_then(|()| replacement.on_start())
                {
                    replacement.cleanup_gameplay();
                    return Err(error);
                }
                Ok(replacement)
            })();
            match result {
                Ok(replacement) => replacements.push(replacement),
                Err(error) => {
                    for staged in &mut replacements {
                        staged.cleanup_gameplay();
                    }
                    return Err(error);
                }
            }
        }
        for (index, replacement) in indices.into_iter().zip(replacements) {
            self.behaviors[index].cleanup_gameplay();
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

fn sort_script_references(references: &mut [ScriptReference]) {
    // Stable sort preserves editor list order when explicit orders are equal.
    references.sort_by_key(|reference| reference.execution_order);
}

#[allow(
    clippy::too_many_arguments,
    reason = "startup loading shares the existing scene, input, logs, and gameplay state"
)]
fn load_startup_references(
    references: &[ScriptReference],
    scene_name: &str,
    entries: &BTreeMap<ScriptId, engine_scripting::ScriptManifestEntry>,
    snapshot_root: &Path,
    world: &Arc<Mutex<SceneWorld>>,
    input_keys: &Arc<Mutex<BTreeSet<String>>>,
    logs: &Arc<Mutex<VecDeque<(String, String)>>>,
    dropped_logs: &Arc<Mutex<u64>>,
    gameplay: &Arc<Mutex<GameplayContext>>,
    behaviors: &mut Vec<RuntimeBehavior>,
) -> Result<(), String> {
    let entity = world
        .lock()
        .map_err(|_| "runtime world lock poisoned".to_owned())?
        .entity_ids()
        .next()
        .unwrap_or_else(EntityId::new);
    for reference in references.iter().filter(|reference| reference.enabled) {
        let entry = entries.get(&reference.asset_id).ok_or_else(|| {
            format!(
                "startup scope references missing script {}",
                reference.asset_id
            )
        })?;
        let source_path = snapshot_root.join(&entry.relative_path);
        let mut source = std::fs::read(&source_path)
            .map_err(|error| format!("could not read {}: {error}", source_path.display()))?;
        if entry.language == ScriptLanguage::Web {
            source = resolve_linked_web_scripts(snapshot_root, &entry.relative_path, &source)?;
        }
        let host = RuntimeHost {
            gameplay: Arc::clone(gameplay),
            owner_script: ScriptId::new(),
            entity_bound: false,
            scene_name: scene_name.to_owned(),
            world: Arc::clone(world),
            input_keys: Arc::clone(input_keys),
            snapshot_root: snapshot_root.to_path_buf(),
            entity,
            properties: Arc::new(Mutex::new(BTreeMap::new())),
            logs: Arc::clone(logs),
            dropped_logs: Arc::clone(dropped_logs),
            enabled: true,
        };
        let mut behavior = RuntimeBehavior::load(
            entry.language,
            reference.asset_id,
            &source,
            &entry.relative_path.to_string_lossy(),
            Box::new(host),
        )?;
        initialize_behavior(&mut behavior, logs, dropped_logs);
        behaviors.push(behavior);
    }
    Ok(())
}

fn resolve_linked_web_scripts(
    snapshot_root: &Path,
    document_path: &Path,
    source: &[u8],
) -> Result<Vec<u8>, String> {
    let text = std::str::from_utf8(source).map_err(|error| error.to_string())?;
    let lower = text.to_ascii_lowercase();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(relative_open) = lower[cursor..].find("<script") {
        let open = cursor + relative_open;
        let tag_end = lower[open..]
            .find('>')
            .map(|offset| open + offset)
            .ok_or_else(|| "unterminated <script> tag".to_owned())?;
        let close = lower[tag_end + 1..]
            .find("</script>")
            .map(|offset| tag_end + 1 + offset)
            .ok_or_else(|| "missing </script> tag".to_owned())?;
        output.push_str(&text[cursor..=tag_end]);
        let tag = &text[open..=tag_end];
        if let Some(link) = script_src(tag) {
            let relative = document_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(link);
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|part| matches!(part, Component::ParentDir))
            {
                return Err(format!(
                    "linked script path escapes the play snapshot: {}",
                    relative.display()
                ));
            }
            let linked_path = snapshot_root.join(&relative);
            let linked = std::fs::read_to_string(&linked_path).map_err(|error| {
                format!(
                    "could not read linked script {}: {error}",
                    linked_path.display()
                )
            })?;
            output.push_str(&linked);
        } else {
            output.push_str(&text[tag_end + 1..close]);
        }
        output.push_str(&text[close..close + "</script>".len()]);
        cursor = close + "</script>".len();
    }
    output.push_str(&text[cursor..]);
    Ok(output.into_bytes())
}

fn script_src(tag: &str) -> Option<&str> {
    let lower = tag.to_ascii_lowercase();
    let index = lower.find("src")? + 3;
    let rest = tag.get(index..)?.trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value = &rest[quote.len_utf8()..];
    let end = value.find(quote)?;
    Some(&value[..end])
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
    if let Err(error) = behavior.on_enable() {
        push_log(
            logs,
            dropped_logs,
            "error",
            &format!(
                "script {} on_enable failed and was disabled: {error}",
                behavior.script_id()
            ),
        );
    } else if let Err(error) = behavior.on_start() {
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

#[derive(Clone)]
struct RuntimeHost {
    gameplay: Arc<Mutex<GameplayContext>>,
    owner_script: ScriptId,
    entity_bound: bool,
    scene_name: String,
    world: Arc<Mutex<SceneWorld>>,
    input_keys: Arc<Mutex<BTreeSet<String>>>,
    snapshot_root: PathBuf,
    entity: EntityId,
    properties: Arc<Mutex<BTreeMap<String, EngineValue>>>,
    logs: Arc<Mutex<VecDeque<(String, String)>>>,
    dropped_logs: Arc<Mutex<u64>>,
    enabled: bool,
}

impl GameplayHost for RuntimeHost {
    fn fork_for_reload(&self) -> Option<Box<dyn GameplayHost>> {
        let mut host = self.clone();
        host.owner_script = ScriptId::new();
        Some(Box::new(host))
    }
    #[allow(
        clippy::too_many_lines,
        reason = "keep exhaustive gameplay query dispatch in one match"
    )]
    fn gameplay_query(&mut self, query: serde_json::Value) -> Result<serde_json::Value, String> {
        #[derive(serde::Deserialize)]
        struct Args {
            #[serde(default)]
            entity: Option<EntityId>,
            #[serde(default)]
            origin: [f64; 3],
            #[serde(default)]
            direction: [f64; 3],
            #[serde(default)]
            center: [f64; 3],
            #[serde(default)]
            vector: [f64; 3],
            #[serde(default)]
            distance: f64,
            #[serde(default)]
            radius: f64,
            #[serde(default)]
            strength: f64,
            #[serde(default)]
            delta: f64,
            #[serde(default)]
            ignore: Vec<EntityId>,
        }
        let op = query
            .get("op")
            .and_then(serde_json::Value::as_str)
            .ok_or("query operation is missing")?;
        if op == "scene_environment_field" {
            let world = self
                .world
                .lock()
                .map_err(|_| "runtime world lock poisoned")?;
            let value = serde_json::to_value(world.environment()).map_err(|e| e.to_string())?;
            let field = query["field"]
                .as_str()
                .ok_or("environment field must be a string")?;
            return value
                .get(field)
                .cloned()
                .ok_or_else(|| format!("unknown environment setting `{field}`"));
        }
        if op == "scene_environment" {
            let mut world = self
                .world
                .lock()
                .map_err(|_| "runtime world lock poisoned")?;
            let mut value = serde_json::to_value(world.environment()).map_err(|e| e.to_string())?;
            if let Some(patch) = query.get("settings") {
                let fields = patch
                    .as_object()
                    .ok_or("environment settings must be an object")?;
                for (name, setting) in fields {
                    let slot = value
                        .get_mut(name)
                        .ok_or_else(|| format!("unknown environment setting `{name}`"))?;
                    *slot = setting.clone();
                }
                let environment: engine_world::SceneEnvironment = serde_json::from_value(value)
                    .map_err(|e| format!("invalid environment settings: {e}"))?;
                world
                    .apply_commands(&[WorldCommand::SetEnvironment(environment)])
                    .map_err(|e| e.to_string())?;
            }
            return serde_json::to_value(world.environment()).map_err(|e| e.to_string());
        }
        if op == "operation_state" {
            let context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
            let owner = Owner {
                entity_bound: self.entity_bound,
                scene: context.scene,
                entity: self.entity,
                script: self.owner_script,
            };
            let handle = query["handle"]
                .as_str()
                .ok_or("operation handle is missing")?;
            return serde_json::to_value(
                context
                    .runtime
                    .states(owner)
                    .into_iter()
                    .find(|state| state.handle == handle),
            )
            .map_err(|e| e.to_string());
        }
        if op == "clock" {
            let mut context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
            if let Some(scale) = query.get("scale") {
                context
                    .runtime
                    .set_time_scale(scale.as_f64().ok_or("invalid time scale")?)?;
            }
            if let Some(paused) = query.get("paused") {
                context.runtime.paused = paused.as_bool().ok_or("invalid pause value")?;
            }
            return Ok(
                serde_json::json!({"scale":context.runtime.time_scale(), "paused":context.runtime.paused}),
            );
        }
        if op == "camera_current" {
            let world = self.world.lock().map_err(|_| "world lock poisoned")?;
            let camera = world
                .entity_ids()
                .filter(|id| world.camera(*id).ok().flatten().is_some_and(|c| c.active))
                .last()
                .ok_or("scene has no active camera")?;
            return Ok(serde_json::json!(camera));
        }
        if op == "animation_ik" {
            let decode = |key: &str| {
                serde_json::from_value::<EntityId>(query[key].clone()).map_err(|e| e.to_string())
            };
            let root = decode("root")?;
            let middle = decode("middle")?;
            let tip = decode("tip")?;
            let target: [f64; 3] =
                serde_json::from_value(query["target"].clone()).map_err(|e| e.to_string())?;
            let pole: [f64; 3] = query
                .get("pole")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or([0., 0., 1.]);
            let duration = query["duration"].as_f64().unwrap_or(0.0);
            let weight = query["weight"].as_f64().unwrap_or(1.0);
            if !weight.is_finite() || !(0.0..=1.0).contains(&weight) {
                return Err("IK weight must be in [0,1]".into());
            }
            let ease: engine_core::gameplay::Ease = query
                .get("easing")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
            let mut world = self.world.lock().map_err(|_| "world lock poisoned")?;
            world.propagate_transforms();
            if world.parent(middle).map_err(|e| e.to_string())? != Some(root)
                || world.parent(tip).map_err(|e| e.to_string())? != Some(middle)
            {
                return Err("IK requires a direct two-bone hierarchy".into());
            }
            let matrix = |id| {
                world
                    .world_transform(id)
                    .map(|v| v.0.as_dmat4())
                    .map_err(|e| e.to_string())
            };
            let root_world = matrix(root)?;
            let middle_world = matrix(middle)?;
            let solution = engine_core::gameplay::procedural::two_bone(
                root_world.w_axis.truncate(),
                middle_world.w_axis.truncate(),
                matrix(tip)?.w_axis.truncate(),
                glam::DVec3::from_array(target),
                glam::DVec3::from_array(pole),
            )?;
            let parent_rotation = world
                .parent(root)
                .map_err(|e| e.to_string())?
                .map(|id| matrix(id).map(|m| m.to_scale_rotation_translation().1))
                .transpose()?
                .unwrap_or(glam::DQuat::IDENTITY);
            let root_rotation = root_world.to_scale_rotation_translation().1;
            let middle_rotation = middle_world.to_scale_rotation_translation().1;
            let desired_root = parent_rotation.inverse() * solution.root_delta * root_rotation;
            let desired_middle = (solution.root_delta * root_rotation).inverse()
                * solution.middle_delta
                * solution.root_delta
                * middle_rotation;
            let actions = [(root, desired_root), (middle, desired_middle)]
                .into_iter()
                .map(|(entity, rotation)| {
                    let current = world
                        .local_transform(entity)
                        .map_err(|e| e.to_string())?
                        .rotation
                        .as_dquat();
                    Ok(engine_core::gameplay::Action::tween(
                        engine_core::gameplay::Target {
                            entity,
                            property: "Rotation".into(),
                        },
                        engine_core::gameplay::Value::Rotation(
                            current.slerp(rotation, weight).normalize().to_array(),
                        ),
                        duration,
                        ease,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let action = engine_core::gameplay::Action::Parallel { actions };
            action.validate()?;
            return serde_json::to_value(action).map_err(|e| e.to_string());
        }
        if op.starts_with("audio_") {
            let mut context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
            let owner = Owner {
                entity_bound: self.entity_bound,
                scene: context.scene,
                entity: self.entity,
                script: self.owner_script,
            };
            if op == "audio_play" {
                let source = query["source"]
                    .as_str()
                    .ok_or("audio source must be an assets-relative file path")?;
                let clip = match self.import_source(source)? {
                    engine_assets::DerivedArtifact::Audio(a) => {
                        engine_core::gameplay::audio::AudioClip {
                            channels: a.channels,
                            sample_rate: a.sample_rate,
                            samples: a.samples,
                        }
                    }
                    _ => return Err("source is not an audio clip".into()),
                };
                let position = query
                    .get("position")
                    .filter(|v| !v.is_null())
                    .map(|v| serde_json::from_value(v.clone()))
                    .transpose()
                    .map_err(|e| e.to_string())?;
                let id = context.audio.play(
                    owner,
                    Arc::new(clip),
                    query["volume"].as_f64().unwrap_or(1.0),
                    query["pitch"].as_f64().unwrap_or(1.0),
                    query["loop"].as_bool().unwrap_or(false),
                    position,
                )?;
                return Ok(serde_json::json!(id));
            }
            let id: EntityId =
                serde_json::from_value(query["entity"].clone()).map_err(|e| e.to_string())?;
            if op == "audio_stop" {
                context.audio.voices.remove(&id);
                return Ok(serde_json::Value::Null);
            }
            let voice = context
                .audio
                .voices
                .get_mut(&id)
                .ok_or("audio voice has finished or was stopped")?;
            match op {
                "audio_pause" => voice.paused = true,
                "audio_resume" => voice.paused = false,
                "audio_volume" => {
                    let v = query["value"].as_f64().ok_or("missing volume")?;
                    if !v.is_finite() || v < 0.0 || v > f64::from(f32::MAX) {
                        return Err("invalid volume".into());
                    }
                    voice.volume = v;
                }
                "audio_pitch" => {
                    let v = query["value"].as_f64().ok_or("missing pitch")?;
                    if !v.is_finite() || v <= 0.0 {
                        return Err("invalid pitch".into());
                    }
                    voice.pitch = v;
                }
                _ => return Err("unknown audio operation".into()),
            }
            return Ok(serde_json::Value::Null);
        }
        if op.starts_with("animation_") {
            let entity = query
                .get("entity")
                .map(|v| serde_json::from_value::<EntityId>(v.clone()))
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or(self.entity);
            if op == "animation_load" {
                let source = query["source"]
                    .as_str()
                    .ok_or("animation source path is missing")?;
                let engine_assets::DerivedArtifact::Model(model) = self.import_source(source)?
                else {
                    return Err("animation source is not a model".into());
                };
                let mut context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
                let mut world = self.world.lock().map_err(|_| "world lock poisoned")?;
                if !world.contains(entity) {
                    return Err("animation owner no longer exists".into());
                }
                let names = model
                    .animations
                    .iter()
                    .map(|c| c.name.clone())
                    .collect::<Vec<_>>();
                if context.clips.keys().any(|(id, _)| *id == entity) {
                    return Err(
                        "model animations are already bound; use Animation.play or a new target"
                            .into(),
                    );
                }
                bind_model(&mut world, &mut context, entity, &model)?;
                return Ok(serde_json::json!(names));
            }
            let mut context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
            match op {
                "animation_marker" => {
                    let name = query["clip"].as_str().ok_or("clip name is missing")?;
                    let marker = query["name"].as_str().ok_or("marker name is missing")?;
                    let time = query["time"].as_f64().ok_or("marker time is missing")?;
                    context
                        .clips
                        .get_mut(&(entity, name.into()))
                        .ok_or("clip is not registered")?
                        .add_marker(time, marker.into())?;
                    return Ok(serde_json::Value::Null);
                }
                "animation_register" => {
                    let clip: engine_core::gameplay::animation::Clip =
                        serde_json::from_value(query["clip"].clone()).map_err(|e| e.to_string())?;
                    clip.validate()?;
                    if context.clips.len() >= 4096 {
                        return Err("clip registry limit exceeded".into());
                    }
                    context.clips.insert((entity, clip.name.clone()), clip);
                    return Ok(serde_json::Value::Null);
                }
                "animation_ref" => {
                    let name = query["name"].as_str().ok_or("clip name is missing")?;
                    if !context.clips.contains_key(&(entity, name.into())) {
                        return Err(format!("clip {name} is not registered for {entity}"));
                    }
                    return Ok(serde_json::json!(name));
                }
                "animation_clip" => {
                    let name = query["name"].as_str().ok_or("clip name is missing")?;
                    return serde_json::to_value(
                        context
                            .clips
                            .get(&(entity, name.into()))
                            .ok_or_else(|| format!("clip {name} is not registered for {entity}"))?,
                    )
                    .map_err(|e| e.to_string());
                }
                "animation_list" => {
                    return Ok(serde_json::json!(
                        context
                            .clips
                            .keys()
                            .filter(|(id, _)| *id == entity)
                            .map(|(_, name)| name)
                            .collect::<Vec<_>>()
                    ));
                }
                _ => return Err("unknown animation query".into()),
            }
        }
        if !op.starts_with("physics_") {
            let q: engine_core::gameplay::query::Query =
                serde_json::from_value(query).map_err(|e| e.to_string())?;
            return serde_json::to_value(q.evaluate()?).map_err(|e| e.to_string());
        }
        let args: Args = serde_json::from_value(query.clone()).map_err(|e| e.to_string())?;
        let mut context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
        let mut world = self.world.lock().map_err(|_| "world lock poisoned")?;
        match op {
            "physics_raycast" | "physics_sphere_cast" => {
                let colliders = crate::physics::PhysicsWorld::colliders(&mut world)?;
                serde_json::to_value(engine_core::gameplay::physics::cast(
                    &colliders,
                    args.origin,
                    args.direction,
                    args.distance,
                    if op == "physics_raycast" {
                        0.0
                    } else {
                        args.radius
                    },
                    &args.ignore,
                )?)
                .map_err(|e| e.to_string())
            }
            "physics_overlap" => {
                let c = crate::physics::PhysicsWorld::colliders(&mut world)?;
                serde_json::to_value(engine_core::gameplay::physics::overlap(
                    &c,
                    args.center,
                    args.radius,
                    &args.ignore,
                )?)
                .map_err(|e| e.to_string())
            }
            "physics_impulse" | "physics_force" | "physics_launch" => {
                let id = args.entity.unwrap_or(self.entity);
                if op == "physics_launch" {
                    context.physics.launch(&world, id, args.vector)?;
                } else {
                    let vector = if op == "physics_force" {
                        if !args.delta.is_finite() || args.delta < 0.0 {
                            return Err("force delta must be finite and non-negative".into());
                        }
                        let interval = context.runtime.scaled_delta(args.delta)?;
                        args.vector.map(|v| v * interval)
                    } else {
                        args.vector
                    };
                    context.physics.impulse(&world, id, vector)?;
                }
                Ok(serde_json::Value::Null)
            }
            "physics_explosion" => {
                if !args.strength.is_finite() || args.strength < 0.0 {
                    return Err("explosion strength must be non-negative".into());
                }
                let c = crate::physics::PhysicsWorld::colliders(&mut world)?;
                let ids = engine_core::gameplay::physics::overlap(
                    &c,
                    args.center,
                    args.radius,
                    &args.ignore,
                )?;
                for id in &ids {
                    if world
                        .part_attributes(*id)
                        .map_err(|e| e.to_string())?
                        .anchored
                    {
                        continue;
                    }
                    let center = c
                        .iter()
                        .find(|c| c.entity == *id)
                        .ok_or("collider disappeared")?
                        .center;
                    let d = glam::DVec3::from_array(center) - glam::DVec3::from_array(args.center);
                    let falloff = if args.radius > 0.0 {
                        (1.0 - d.length() / args.radius).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    let direction = if d.length_squared() > 1e-12 {
                        d.normalize()
                    } else {
                        glam::DVec3::Y
                    };
                    context.physics.impulse(
                        &world,
                        *id,
                        (direction * args.strength * falloff).to_array(),
                    )?;
                }
                serde_json::to_value(ids).map_err(|e| e.to_string())
            }
            _ => Err(format!("unknown physics operation: {op}")),
        }
    }
    fn gameplay_request(&mut self, mut request: Request) -> Result<(), String> {
        let mut context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
        let owner = Owner {
            entity_bound: self.entity_bound,
            scene: context.scene,
            entity: self.entity,
            script: self.owner_script,
        };
        if let Request::Start { action, .. } = &mut request {
            action.resolve_clips(&|entity, name| {
                context
                    .clips
                    .get(&(entity, name.to_owned()))
                    .cloned()
                    .ok_or_else(|| format!("clip {name} is not registered for {entity}"))
            })?;
        }
        context.runtime.request(owner, request)
    }
    fn gameplay_connections(&self) -> Vec<String> {
        self.gameplay.lock().map_or_else(
            |_| Vec::new(),
            |context| {
                context.runtime.signals.tokens(Owner {
                    entity_bound: self.entity_bound,
                    scene: context.scene,
                    entity: self.entity,
                    script: self.owner_script,
                })
            },
        )
    }
    fn gameplay_states(&self) -> Vec<OperationState> {
        self.gameplay.lock().map_or_else(
            |_| Vec::new(),
            |context| {
                context.runtime.states(Owner {
                    entity_bound: self.entity_bound,
                    scene: context.scene,
                    entity: self.entity,
                    script: self.owner_script,
                })
            },
        )
    }
    fn gameplay_callbacks(&mut self) -> Vec<Delivery> {
        self.gameplay.lock().map_or_else(
            |_| Vec::new(),
            |mut context| {
                let owner = Owner {
                    entity_bound: self.entity_bound,
                    scene: context.scene,
                    entity: self.entity,
                    script: self.owner_script,
                };
                context.runtime.drain_callbacks(owner)
            },
        )
    }
    fn cleanup_gameplay(&mut self) {
        if let Ok(mut context) = self.gameplay.lock() {
            context
                .runtime
                .cleanup(|owner| owner.script == self.owner_script);
            context
                .audio
                .cleanup(|owner| owner.script == self.owner_script);
        }
    }
    fn set_current_camera(&mut self, source: &str) -> Result<(), String> {
        let id = self
            .find_entity(source)?
            .ok_or_else(|| format!("camera `{source}` was not found"))?;
        let mut world = self
            .world
            .lock()
            .map_err(|_| "runtime world lock poisoned")?;
        world
            .camera(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("entity `{source}` is not a camera"))?;
        let mut commands = Vec::new();
        for entity in world.entity_ids() {
            if let Some(mut camera) = world.camera(entity).map_err(|e| e.to_string())? {
                let active = entity == id;
                if camera.active != active {
                    camera.active = active;
                    commands.push(WorldCommand::SetCamera {
                        entity,
                        value: Some(camera),
                    });
                }
            }
        }
        world.apply_commands(&commands).map_err(|e| e.to_string())
    }
    fn entity_id(&self) -> EntityId {
        self.entity
    }
    fn delta_time(&self) -> f64 {
        self.gameplay
            .lock()
            .map_or(0.0, |context| context.frame_delta)
    }
    fn fixed_delta_time(&self) -> f64 {
        self.gameplay
            .lock()
            .map_or(0.0, |context| context.fixed_delta)
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
        if query.starts_with("rustic.game.") {
            return self.object_entity(query).map(Some);
        }
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
                paths.insert(format!("rustic.game.{}.{path}", self.scene_name), id);
            }
        }
        paths
    }
    fn add_instance(&mut self, source: &str, parent: Option<EntityId>) -> Result<EntityId, String> {
        if self.resolve_entity(source)?.is_some() {
            return self.clone_instance(source, parent);
        }
        if let Some(snapshot) = self.asset_instance(source, parent)? {
            let engine_assets::DerivedArtifact::Model(model) = self.import_source(source)? else {
                return Err("instance asset is not a model".into());
            };
            let id = snapshot.id;
            self.world
                .lock()
                .map_err(|_| "runtime world lock poisoned".to_owned())?
                .apply_commands(&[WorldCommand::Spawn(Box::new(snapshot))])
                .map_err(|error| error.to_string())?;
            let mut context = self.gameplay.lock().map_err(|_| "gameplay lock poisoned")?;
            let mut world = self.world.lock().map_err(|_| "world lock poisoned")?;
            bind_model(&mut world, &mut context, id, &model)?;
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
    fn object_attribute(&self, source: &str, name: &str) -> Result<Option<EngineValue>, String> {
        let mut target = self.clone();
        target.entity = self.object_entity(source)?;
        target.attribute(name)
    }
    fn edit_object_attribute(
        &mut self,
        source: &str,
        name: &str,
        value: EngineValue,
    ) -> Result<(), String> {
        let mut target = self.clone();
        target.entity = self.object_entity(source)?;
        target.edit_attribute(name, value)
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
            ("position" | "size", EngineValue::Vec3(value)) => {
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
                        attributes.color[..3]
                            .copy_from_slice(&glam::DVec3::from_array(value).as_vec3().to_array());
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
    fn input_frame(&self) -> InputFrame {
        InputFrame {
            keys: self
                .input_keys
                .lock()
                .map_or_else(|_| BTreeSet::new(), |keys| keys.clone()),
            ..InputFrame::default()
        }
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
    fn enabled(&self) -> bool {
        self.enabled
            && (!self.entity_bound
                || self
                    .world
                    .lock()
                    .is_ok_and(|world| world.contains(self.entity)))
    }
}

impl RuntimeHost {
    fn object_entity(&self, source: &str) -> Result<EntityId, String> {
        if let Some(rest) = source.strip_prefix("rustic.game.") {
            let prefix = format!("{}.", self.scene_name);
            let path = rest.strip_prefix(&prefix).ok_or_else(|| {
                format!(
                    "scene in `{source}` is not loaded (loaded scene: `{}`)",
                    self.scene_name
                )
            })?;
            let world = self
                .world
                .lock()
                .map_err(|_| "runtime world lock poisoned")?;
            // Compare full hierarchy paths, rejecting duplicates instead of choosing arbitrarily.
            let matches: Vec<_> = world
                .entity_ids()
                .filter(|id| world.entity_path(*id).ok().as_deref() == Some(path))
                .collect();
            return match matches.as_slice() {
                [id] => Ok(*id),
                [] => Err(format!("scene object `{source}` was not found")),
                _ => Err(format!(
                    "scene object `{source}` is ambiguous; give siblings unique names"
                )),
            };
        }
        self.find_entity(source)?
            .ok_or_else(|| format!("scene object `{source}` was not found"))
    }

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

    fn import_source(&self, source: &str) -> Result<engine_assets::DerivedArtifact, String> {
        let relative = Path::new(source);
        if relative.is_absolute()
            || relative
                .components()
                .any(|p| !matches!(p, Component::Normal(_)))
            || relative
                .components()
                .next()
                .is_none_or(|p| p.as_os_str() != "assets")
        {
            return Err("source must be a path inside assets".into());
        }
        let path = self.snapshot_root.join(relative);
        let assets = self
            .snapshot_root
            .join("assets")
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !path
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(&assets)
        {
            return Err("source escapes the asset directory".into());
        }
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let mut request = engine_assets::ImportRequest::new(relative, bytes);
        if matches!(
            relative.extension().and_then(|s| s.to_str()),
            Some("gltf" | "glb")
        ) {
            // Resolve only validated, bounded dependencies inside the source directory.
            for uri in engine_assets::model_dependencies(&request.source_bytes)
                .map_err(|e| e.to_string())?
            {
                let dependency = path.parent().ok_or("source parent missing")?.join(&uri);
                if !dependency
                    .canonicalize()
                    .map_err(|e| e.to_string())?
                    .starts_with(&assets)
                {
                    return Err("model dependency escapes the asset directory".into());
                }
                request
                    .external_bytes
                    .insert(uri, std::fs::read(dependency).map_err(|e| e.to_string())?);
            }
        }
        engine_assets::ImporterRegistry::import(&request).map_err(|e| e.to_string())
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
        if !matches!(extension.as_str(), "obj" | "gltf" | "glb" | "fbx") {
            return Err(format!(
                "asset `{source}` is not a supported model (expected .obj, .gltf, .glb, or .fbx)"
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

fn bind_model(
    world: &mut SceneWorld,
    context: &mut GameplayContext,
    entity: EntityId,
    model: &engine_assets::ModelArtifact,
) -> Result<(), String> {
    model.validate().map_err(|e| e.to_string())?;
    if context.clips.len() + model.animations.len() > 4096 {
        return Err("clip registry limit exceeded".into());
    }
    let ids = model
        .nodes
        .iter()
        .map(|_| EntityId::new())
        .collect::<Vec<_>>();
    let mut commands = Vec::new();
    for (index, node) in model.nodes.iter().enumerate() {
        commands.push(WorldCommand::Spawn(Box::new(
            engine_world::EntitySnapshot {
                id: ids[index],
                name: Some(format!("node_{index}")),
                parent: Some(entity),
                local_transform: engine_world::LocalTransform {
                    translation: glam::DVec3::from_array(node.translation).as_vec3(),
                    rotation: glam::DQuat::from_array(node.rotation).as_quat(),
                    scale: glam::DVec3::from_array(node.scale).as_vec3(),
                },
                ..Default::default()
            },
        )));
    }
    for (index, node) in model.nodes.iter().enumerate() {
        if let Some(parent) = node.parent {
            commands.push(WorldCommand::SetParent {
                child: ids[index],
                parent: Some(*ids.get(parent).ok_or("invalid model parent index")?),
            });
        }
    }
    world.apply_commands(&commands).map_err(|e| e.to_string())?;
    for clip in &model.animations {
        context
            .clips
            .insert((entity, clip.name.clone()), clip.clone());
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "tests compare exact round trips and deterministic values"
)]
#[allow(
    clippy::field_reassign_with_default,
    reason = "fixtures start from defaults and vary only the authored values under test"
)]
mod tests {
    use super::*;
    use engine_scripting::{
        ScriptApiVersion, ScriptId, ScriptLanguage, ScriptManifest, ScriptManifestEntry,
        save_manifest_atomic,
    };
    use engine_world::{EntitySnapshot, SceneDocument, ScriptComponent};

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "keep the complete integration fixture and assertions together"
    )]
    fn model_clips_bind_before_start_and_audio_physics_follow_the_scene_clock() {
        let temp = tempfile::tempdir().unwrap();
        for folder in ["assets", "config", "scripts"] {
            std::fs::create_dir_all(temp.path().join(folder)).unwrap();
        }
        let source = temp.path().join("assets/rig.gltf");
        std::fs::write(
            &source,
            include_bytes!("../../engine-assets/tests/fixtures/skinned_animation.gltf"),
        )
        .unwrap();
        let meta = engine_assets::AssetMeta::new("rustic.gltf", 2);
        engine_assets::save_meta(&engine_assets::sidecar_path(&source), &meta).unwrap();
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36u32 + 32000).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&32000u32.to_le_bytes());
        for _ in 0..16000 {
            wav.extend_from_slice(&1000i16.to_le_bytes());
        }
        std::fs::write(temp.path().join("assets/tone.wav"), wav).unwrap();
        let script_id = ScriptId::new();
        std::fs::write(temp.path().join("scripts/actor.lua"), br"return {Start=function()
            assert(Animation.clips(nil)[1]=='Slide')
            local hit=Physics.raycast(Vector3(0,2,0),Vector3(0,-1,0),4)
            assert(hit and hit.entity==rustic.entity_id())
            Physics.launch(nil,Vector3(2,0,0))
            Animation.addMarker(nil,'Slide',0.5,'Mid')
            Animation.play(nil,'Slide').onMarker('Mid',function() rustic.log('info','imported marker') end).onFinished(function() rustic.log('info','clip complete') end)
            local voice=Audio.play('assets/tone.wav',{volume=0})
            Audio.fadeIn(voice,1,Ease.InQuad)
            Clock.timeScale(0.5)
            Timer.after(0.5,function() rustic.log('info','scaled timer') end)
        end}").unwrap();
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![ScriptManifestEntry {
                    id: script_id,
                    language: ScriptLanguage::Lua54,
                    relative_path: "scripts/actor.lua".into(),
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                }],
            },
        )
        .unwrap();
        let entity = EntitySnapshot {
            mesh: Some(Mesh {
                asset: meta.asset_id,
            }),
            primitive: Some(engine_world::Primitive::Cube { size: 1. }),
            scripts: vec![ScriptComponent::new(script_id)],
            ..Default::default()
        };
        let mut scene = SceneDocument::new("Rig");
        scene.entities.push(entity.clone());
        let mut runtime = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap()).unwrap();
        runtime.fixed_update(0.1);
        assert!(
            (runtime
                .world
                .lock()
                .unwrap()
                .local_transform(entity.id)
                .unwrap()
                .translation
                .x
                - 0.1)
                .abs()
                < 1e-5
        );
        runtime.frame_update(1.0);
        let world = runtime.world.lock().unwrap();
        let joint = world
            .entity_ids()
            .find(|id| world.snapshot(*id).unwrap().name.as_deref() == Some("node_1"))
            .unwrap();
        assert_eq!(world.local_transform(joint).unwrap().translation.x, 2.0);
        drop(world);
        let context = runtime.gameplay.lock().unwrap();
        assert_eq!(context.audio.voices.len(), 1);
        assert!((context.audio.voices.values().next().unwrap().volume - 0.25).abs() < 1e-9);
        drop(context);
        assert!(
            runtime
                .logs
                .lock()
                .unwrap()
                .iter()
                .any(|(_, message)| message == "scaled timer")
        );
        assert!(
            runtime
                .logs
                .lock()
                .unwrap()
                .iter()
                .any(|(_, message)| message == "imported marker")
        );
        runtime.stop();
        assert!(runtime.gameplay.lock().unwrap().audio.voices.is_empty());
        assert!(
            runtime
                .gameplay
                .lock()
                .unwrap()
                .runtime
                .states_for_entities()
                .is_empty()
        );
    }

    #[test]
    fn physics_runs_without_scripts_and_resets_with_new_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let mut scene = SceneDocument::new("Physics");
        let mut floor = EntitySnapshot::default();
        floor.primitive = Some(engine_world::Primitive::RectangularPrism {
            size: [20.0, 1.0, 20.0],
        });
        floor.part_attributes.anchored = true;
        let mut falling = EntitySnapshot::default();
        falling.primitive = Some(engine_world::Primitive::Cube { size: 1.0 });
        falling.local_transform.translation.y = 5.0;
        let id = falling.id;
        scene.entities = vec![floor, falling];
        let bytes = scene.to_bytes().unwrap();
        let mut runtime = RuntimeScripts::load(temp.path(), &bytes).unwrap();
        for _ in 0..180 {
            runtime.fixed_update(1.0 / 60.0);
        }
        assert!((runtime.live_entity(id).unwrap().unwrap().translation[1] - 1.0).abs() < 0.001);
        let restarted = RuntimeScripts::load(temp.path(), &bytes).unwrap();
        assert_eq!(
            restarted.live_entity(id).unwrap().unwrap().translation[1],
            5.0
        );
    }

    #[test]
    fn script_attribute_edits_affect_physics_in_same_fixed_tick() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let script_id = ScriptId::new();
        let mut entity = EntitySnapshot::default();
        entity.primitive = Some(engine_world::Primitive::Cube { size: 1.0 });
        entity.part_attributes.anchored = true;
        entity.scripts.push(ScriptComponent::new(script_id));
        let id = entity.id;
        let mut scene = SceneDocument::new("Physics");
        scene.entities.push(entity);
        std::fs::write(
            temp.path().join("scripts/release.lua"),
            b"return { FixedUpdate=function() rustic.EditAttribute('Anchored',false) end }",
        )
        .unwrap();
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![ScriptManifestEntry {
                    id: script_id,
                    language: ScriptLanguage::Lua54,
                    relative_path: "scripts/release.lua".into(),
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                }],
            },
        )
        .unwrap();
        let mut runtime = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap()).unwrap();
        runtime.fixed_update(1.0 / 60.0);
        assert!(runtime.live_entity(id).unwrap().unwrap().translation[1] < 0.0);
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "keep the complete integration fixture and assertions together"
    )]
    fn named_scene_object_calls_mutate_target_in_lua_and_javascript() {
        let root = EntitySnapshot {
            name: Some("Room".into()),
            ..Default::default()
        };
        let target = EntitySnapshot {
            name: Some("Player".into()),
            parent: Some(root.id),
            ..Default::default()
        };
        let owner = EntitySnapshot {
            name: Some("Controller".into()),
            ..Default::default()
        };
        let mut scene = SceneDocument::new("Demo");
        scene.entities = vec![root, target.clone(), owner.clone()];
        let world = Arc::new(Mutex::new(scene.create_world().unwrap()));
        let make_host = || RuntimeHost {
            gameplay: Arc::new(Mutex::new(GameplayContext::default())),
            owner_script: ScriptId::new(),
            entity_bound: true,
            scene_name: "Demo".into(),
            world: Arc::clone(&world),
            input_keys: Arc::new(Mutex::new(BTreeSet::new())),
            snapshot_root: PathBuf::new(),
            entity: owner.id,
            properties: Arc::new(Mutex::new(BTreeMap::new())),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            dropped_logs: Arc::new(Mutex::new(0)),
            enabled: true,
        };
        let mut lua = LuaBehavior::load(ScriptId::new(),
            b"return {Start=function() rustic.game.Demo.Room.Player:EditAttribute('Position',{1,2,3}); assert(rustic.game.Demo.Room.Player:GetAttribute('Position')[1]==1) end}",
            "target.lua", Box::new(make_host()), 100_000).unwrap();
        lua.on_start().unwrap();
        assert_eq!(
            world
                .lock()
                .unwrap()
                .local_transform(target.id)
                .unwrap()
                .translation
                .to_array(),
            [1., 2., 3.]
        );
        let mut js = JavaScriptBehavior::load(ScriptId::new(),
            b"globalThis.behavior={Start(){rustic.game.Demo.Room.Player.EditAttribute('Color',[1,0,0]);rustic.game['Demo']['Room']['Player'].EditAttribute('Anchored',true);}};",
            "target.js", Box::new(make_host()), 100_000).unwrap();
        js.on_start().unwrap();
        assert!(
            world
                .lock()
                .unwrap()
                .part_attributes(target.id)
                .unwrap()
                .anchored
        );
        assert_eq!(
            world
                .lock()
                .unwrap()
                .part_attributes(target.id)
                .unwrap()
                .color[..3],
            [1., 0., 0.]
        );
        assert_eq!(
            world
                .lock()
                .unwrap()
                .local_transform(owner.id)
                .unwrap()
                .translation
                .to_array(),
            [0.; 3]
        );
        let mut web = WebBehavior::load(ScriptId::new(),
            br"<!doctype html><html><body><script>globalThis.behavior={Start(){rustic.game.Demo.Room.Player.EditAttribute('Size',[2,3,4]);}};</script></body></html>",
            "ui/target.html", Box::new(make_host()),100_000).unwrap();
        web.on_start().unwrap();
        assert_eq!(
            world
                .lock()
                .unwrap()
                .local_transform(target.id)
                .unwrap()
                .scale
                .to_array(),
            [2., 3., 4.]
        );
        let mut host = make_host();
        for source in [
            "rustic.game.Other.Room.Player",
            "rustic.game.Demo.Room.Missing",
        ] {
            assert!(
                host.edit_object_attribute(source, "Anchored", EngineValue::Boolean(false))
                    .is_err()
            );
        }
        assert!(
            host.edit_object_attribute(
                "rustic.game.Demo.Room.Player",
                "Position",
                EngineValue::Boolean(true)
            )
            .is_err()
        );
        host.edit_object_attribute(
            "rustic.game.Demo.Room.Player",
            "Name",
            EngineValue::String("Renamed".into()),
        )
        .unwrap();
        assert!(
            host.object_attribute("rustic.game.Demo.Room.Player", "Name")
                .is_err()
        );
        assert_eq!(
            host.object_attribute("rustic.game.Demo.Room.Renamed", "Name")
                .unwrap(),
            Some(EngineValue::String("Renamed".into()))
        );
        let duplicate = EntitySnapshot {
            name: Some("Renamed".into()),
            parent: target.parent,
            ..Default::default()
        };
        world
            .lock()
            .unwrap()
            .apply_commands(&[WorldCommand::Spawn(Box::new(duplicate))])
            .unwrap();
        assert!(
            host.object_attribute("rustic.game.Demo.Room.Renamed", "Name")
                .unwrap_err()
                .contains("ambiguous")
        );
    }

    fn environment_host(world: &Arc<Mutex<SceneWorld>>) -> RuntimeHost {
        RuntimeHost {
            gameplay: Arc::new(Mutex::new(GameplayContext::default())),
            owner_script: ScriptId::new(),
            entity_bound: false,
            scene_name: "Environment".into(),
            world: Arc::clone(world),
            entity: EntityId::new(),
            input_keys: Arc::new(Mutex::new(BTreeSet::new())),
            snapshot_root: PathBuf::new(),
            properties: Arc::new(Mutex::new(BTreeMap::new())),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            dropped_logs: Arc::new(Mutex::new(0)),
            enabled: true,
        }
    }

    #[test]
    fn environment_patches_validate_atomically_and_preserve_other_settings() {
        let world = Arc::new(Mutex::new(SceneWorld::new()));
        let mut host = environment_host(&world);
        let settings = serde_json::json!({
            "enabled":true, "sky_image":"assets/day.hdr", "sky_color":[0.1,0.2,0.3],
            "rotation_degrees":45, "exposure":2, "ambient_color":[0.4,0.5,0.6],
            "ambient_intensity":0.5, "sun_color":[1,0.9,0.8], "sun_intensity":2,
            "sun_direction":[1,2,3], "haze_color":[0.2,0.3,0.4],
            "haze_density":0.02, "haze_start":20
        });
        let expected: engine_world::SceneEnvironment =
            serde_json::from_value(settings.clone()).unwrap();
        let expected = serde_json::to_value(expected).unwrap();
        assert_eq!(
            host.gameplay_query(serde_json::json!({"op":"scene_environment","settings":settings}))
                .unwrap(),
            expected
        );
        let before = world.lock().unwrap().environment().clone();
        for patch in [
            serde_json::json!({"enabled":false,"ambient_intensity":-1}),
            serde_json::json!({"exposure":21}),
            serde_json::json!({"sun_direction":[0,0,0]}),
            serde_json::json!({"sky_color":[1,2]}),
            serde_json::json!({"sky_image":"../escape.hdr"}),
            serde_json::json!({"sky_image":"/outside.hdr"}),
            serde_json::json!({"sky_image":"C:\\outside.hdr"}),
            serde_json::json!({"sky_image":null}),
            serde_json::json!({"enabled":1}),
            serde_json::json!({"haze_start":"20"}),
            serde_json::json!({"sun_intensity":1e100}),
            serde_json::json!({"unknown":1}),
            serde_json::json!([]),
            serde_json::json!(null),
        ] {
            assert!(
                host.gameplay_query(serde_json::json!({"op":"scene_environment","settings":patch}))
                    .is_err()
            );
            assert_eq!(world.lock().unwrap().environment(), &before);
        }
        let updated = host.gameplay_query(serde_json::json!({"op":"scene_environment","settings":{"sky_image":"assets/night.hdr"}})).unwrap();
        assert_eq!(updated["sky_image"], "assets/night.hdr");
        assert_eq!(updated["sun_intensity"], expected["sun_intensity"]);
        assert_eq!(
            host.gameplay_query(serde_json::json!({"op":"scene_environment"}))
                .unwrap(),
            updated
        );
        assert_eq!(
            host.gameplay_query(
                serde_json::json!({"op":"scene_environment_field","field":"sky_image"})
            )
            .unwrap(),
            "assets/night.hdr"
        );
        assert!(
            host.gameplay_query(
                serde_json::json!({"op":"scene_environment_field","field":"unknown"})
            )
            .is_err()
        );
        let captured = SceneDocument::from_world(
            engine_world::SceneId::new(),
            "Environment",
            &world.lock().unwrap(),
        )
        .unwrap();
        assert_eq!(captured.environment.sky_image, "assets/night.hdr");
        let cleared = host
            .gameplay_query(serde_json::json!({"op":"scene_environment",
            "settings":{"sky_image":"","enabled":false}}))
            .unwrap();
        assert_eq!(cleared["sky_image"], "");
        assert_eq!(cleared["enabled"], false);
        assert_eq!(cleared["sun_intensity"], expected["sun_intensity"]);
    }

    #[test]
    fn scene_environment_apis_work_through_all_available_script_adapters() {
        let fixtures: &[(ScriptLanguage, &str, &[u8])] = &[
            (ScriptLanguage::Lua54, "sky.lua", br#"return {Start=function()
                Game.scene.setEnvironment({enabled=true,sun_intensity=2})
                Game.scene.setSkyTexture('assets/night.hdr')
                assert(Game.scene.getEnvironment().sky_image=='assets/night.hdr')
                assert(Game.scene.setEnvironment({}).sun_intensity==2)
                assert(not pcall(function() Game.scene.setEnvironment({enabled=false,exposure=21}) end))
                assert(Game.scene.getEnvironment().enabled)
            end}"#),
            (ScriptLanguage::JavaScript, "sky.js", br#"globalThis.behavior={Start(){
                Game.scene.setEnvironment({enabled:true,sun_intensity:2});
                Game.scene.setSkyTexture('assets/night.hdr');
                if(Game.scene.getEnvironment().sky_image!=='assets/night.hdr')throw Error('stale sky');
                let rejected=false;try{Game.scene.setEnvironment({enabled:false,exposure:21});}catch(e){rejected=true;}
                if(!rejected || !Game.scene.getEnvironment().enabled)throw Error('invalid patch committed');
            }};"#),
            (ScriptLanguage::Web, "sky.html", br#"<!doctype html><html><head><title>Sky</title></head><body><script>globalThis.behavior={Start(){Game.scene.setEnvironment({enabled:true,sun_intensity:2});Game.scene.setSkyTexture('assets/night.hdr');if(Game.scene.getEnvironment().sun_intensity!==2)throw Error('stale lighting');}};</script></body></html>"#),
            (ScriptLanguage::Python, "sky.py", br#"from rustic import Game,run
def on_start():
    Game.scene.setEnvironment({"enabled":True,"sun_intensity":2})
    Game.scene.setSkyTexture("assets/night.hdr")
    assert Game.scene.getEnvironment()["sky_image"]=="assets/night.hdr"
run(globals())"#),
            (ScriptLanguage::Luau, "sky.luau", br#"return {Start=function() Game.scene.setEnvironment({enabled=true,sun_intensity=2});Game.scene.setSkyTexture('assets/night.hdr');assert(Game.scene.getEnvironment().sun_intensity==2) end}"#),
            (ScriptLanguage::Cpp, "sky.cpp", br#"#include "rustic.hpp"
void start(){Game.scene.setEnvironment({{"enabled",true},{"sun_intensity",2.0}});Game.scene.setSkyTexture("assets/night.hdr");if(Game.scene.getEnvironment().at("sky_image").string()!="assets/night.hdr")throw std::runtime_error("stale sky");}
int main(){return rustic_run(RusticBehavior{.on_start=start});}"#),
            (ScriptLanguage::C, "sky.c", br#"#include "rustic.h"
void start(void){Game.scene.setEnvironment("enabled",(RusticValue){.type=RUSTIC_BOOL,.boolean=true});Game.scene.setEnvironment("sun_intensity",(RusticValue){.type=RUSTIC_NUMBER,.number=2});Game.scene.setSkyTexture("assets/night.hdr");if(strcmp(Game.scene.getEnvironment("sky_image").string,"assets/night.hdr"))r_fail("stale sky");}
int main(void){return rustic_run((RusticBehavior){.on_start=start});}"#),
            (ScriptLanguage::CSharp, "sky.cs", br#"using static Rustic;
Run((callback,dt)=>{if(callback=="on_start"){Game.scene.setEnvironment(new {enabled=true,sun_intensity=2});Game.scene.setSkyTexture("assets/night.hdr");if(Game.scene.getEnvironment().GetProperty("sky_image").GetString()!="assets/night.hdr")throw new System.Exception("stale sky");}});"#),
            (ScriptLanguage::Java, "RusticBehavior.java", br#"class RusticBehavior extends Rustic {
public static void main(String[]args)throws Exception{run((callback,dt)->{if(callback.equals("on_start")){Game.scene.setEnvironment(java.util.Map.of("enabled",true,"sun_intensity",2));Game.scene.setSkyTexture("assets/night.hdr");if(!((java.util.Map<?,?>)Game.scene.getEnvironment()).get("sky_image").equals("assets/night.hdr"))throw new RuntimeException("stale sky");}});}}"#),
            (ScriptLanguage::Php, "sky.php", br#"<?php
require __DIR__."/rustic.php";
function on_start():void{global $Game;$Game->scene->setEnvironment(["enabled"=>true,"sun_intensity"=>2]);$Game->scene->setSkyTexture("assets/night.hdr");if($Game->scene->getEnvironment()["sky_image"]!=="assets/night.hdr")throw new Exception("stale sky");}
rustic_run(["on_start"=>"on_start"]);"#),
        ];
        for (language, name, source) in fixtures {
            if !engine_scripting::probe_language_toolchain(*language).available {
                eprintln!("skipped {}: toolchain unavailable", language.display_name());
                continue;
            }
            let world = Arc::new(Mutex::new(SceneWorld::new()));
            let mut behavior = RuntimeBehavior::load(
                *language,
                ScriptId::new(),
                source,
                name,
                Box::new(environment_host(&world)),
            )
            .unwrap();
            behavior.on_start().unwrap();
            let world = world.lock().unwrap();
            assert!(world.environment().enabled, "{}", language.display_name());
            assert_eq!(world.environment().sun_intensity, 2.0);
            assert_eq!(world.environment().sky_image, "assets/night.hdr");
            assert_eq!(world.environment().ambient_intensity, 0.12);
            eprintln!("validated environment API: {}", language.display_name());
        }
    }

    #[test]
    fn named_scene_object_edits_work_through_available_external_sdks() {
        let fixtures: &[(ScriptLanguage,&str,&[u8])] = &[
            (ScriptLanguage::Python,"target.py",br#"from rustic import rustic, run
def on_start(): rustic.game.Demo.Player.EditAttribute("Position",[1,2,3])
run(globals())"#),
            (ScriptLanguage::Luau,"target.luau",br#"return {Start=function() rustic.game.Demo.Player:EditAttribute("Position",{1,2,3}) end}"#),
            (ScriptLanguage::Cpp,"target.cpp",br#"#include "rustic.hpp"
void start(){rustic.game["Demo"]["Player"].EditAttribute("Position",RusticValue::Array{1.0,2.0,3.0});}
int main(){return rustic_run(RusticBehavior{.on_start=start});}"#),
            (ScriptLanguage::C,"target.c",br#"#include "rustic.h"
void start(void){rustic.game.EditAttribute("Demo","Player","Position",(RusticValue){.type=RUSTIC_VECTOR,.vector={1,2,3},.length=3});}
int main(void){return rustic_run((RusticBehavior){.on_start=start});}"#),
            (ScriptLanguage::CSharp,"target.cs",br#"using static Rustic;
Run((callback,dt)=>{if(callback=="on_start")rustic.game.Demo.Player.EditAttribute("Position",new double[]{1,2,3});});"#),
            (ScriptLanguage::Java,"RusticBehavior.java",br#"class RusticBehavior extends Rustic {
public static void main(String[]args)throws Exception{run((callback,dt)->{if(callback.equals("on_start"))rustic.game.scene("Demo").object("Player").EditAttribute("Position",new double[]{1,2,3});});}}"#),
            (ScriptLanguage::Php,"target.php",br#"<?php
require __DIR__."/rustic.php";
function on_start():void{global $rustic;$rustic->game->Demo->Player->EditAttribute("Position",[1,2,3]);}
rustic_run(["on_start"=>"on_start"]);"#),
        ];
        for (language, name, source) in fixtures {
            if !engine_scripting::probe_language_toolchain(*language).available {
                eprintln!("skipped {}: toolchain unavailable", language.display_name());
                continue;
            }
            let target = EntitySnapshot {
                name: Some("Player".into()),
                ..Default::default()
            };
            let owner = EntitySnapshot {
                name: Some("Controller".into()),
                ..Default::default()
            };
            let mut scene = SceneDocument::new("Demo");
            scene.entities = vec![target.clone(), owner.clone()];
            let world = Arc::new(Mutex::new(scene.create_world().unwrap()));
            let host = RuntimeHost {
                gameplay: Arc::new(Mutex::new(GameplayContext::default())),
                owner_script: ScriptId::new(),
                entity_bound: true,
                scene_name: "Demo".into(),
                world: Arc::clone(&world),
                entity: owner.id,
                input_keys: Arc::new(Mutex::new(BTreeSet::new())),
                snapshot_root: PathBuf::new(),
                properties: Arc::new(Mutex::new(BTreeMap::new())),
                logs: Arc::new(Mutex::new(VecDeque::new())),
                dropped_logs: Arc::new(Mutex::new(0)),
                enabled: true,
            };
            let mut behavior =
                ExternalBehavior::load(ScriptId::new(), *language, source, name, Box::new(host))
                    .unwrap_or_else(|e| panic!("{} load: {e}", language.display_name()));
            behavior
                .on_create()
                .unwrap_or_else(|e| panic!("{} create: {e}", language.display_name()));
            behavior
                .on_start()
                .unwrap_or_else(|e| panic!("{} start: {e}", language.display_name()));
            assert_eq!(
                world
                    .lock()
                    .unwrap()
                    .local_transform(target.id)
                    .unwrap()
                    .translation
                    .to_array(),
                [1., 2., 3.],
                "{}",
                language.display_name()
            );
            assert_eq!(
                world
                    .lock()
                    .unwrap()
                    .local_transform(owner.id)
                    .unwrap()
                    .translation
                    .to_array(),
                [0.; 3]
            );
        }
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "keep the complete integration fixture and assertions together"
    )]
    fn sdk_camera_selection_changes_render_camera_and_rejects_invalid_targets() {
        let first = EntitySnapshot {
            name: Some("First".into()),
            camera: Some(engine_world::Camera {
                order: 100,
                ..Default::default()
            }),
            ..Default::default()
        };
        let second = EntitySnapshot {
            name: Some("Second".into()),
            parent: Some(first.id),
            camera: Some(engine_world::Camera {
                active: false,
                ..Default::default()
            }),
            ..Default::default()
        };
        let part = EntitySnapshot::default();
        let mut world = SceneWorld::new();
        world
            .apply_commands(&[
                WorldCommand::Spawn(Box::new(first.clone())),
                WorldCommand::Spawn(Box::new(second.clone())),
                WorldCommand::Spawn(Box::new(part.clone())),
            ])
            .unwrap();
        let world = Arc::new(Mutex::new(world));
        let make_host = || RuntimeHost {
            gameplay: Arc::new(Mutex::new(GameplayContext::default())),
            owner_script: ScriptId::new(),
            entity_bound: true,
            scene_name: "TestScene".into(),
            world: Arc::clone(&world),
            input_keys: Arc::new(Mutex::new(BTreeSet::new())),
            snapshot_root: PathBuf::new(),
            entity: part.id,
            properties: Arc::new(Mutex::new(BTreeMap::new())),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            dropped_logs: Arc::new(Mutex::new(0)),
            enabled: true,
        };
        let mut lua = LuaBehavior::load(
            ScriptId::new(),
            b"return {on_start=function() Game.setCurrentCamera({'Game.scene.First.Second'}) end}",
            "camera.lua",
            Box::new(make_host()),
            100_000,
        )
        .unwrap();
        lua.on_start().unwrap();
        let mut buffer = engine_world::RenderWorldBuffer::new();
        {
            let world = world.lock().unwrap();
            let render = buffer.extract(&world);
            assert_eq!(render.cameras.len(), 1);
            assert_eq!(render.cameras[0].entity, second.id);
        }
        let mut host = make_host();
        for invalid in [
            "Missing".to_owned(),
            part.id.to_string(),
            EntityId::new().to_string(),
        ] {
            assert!(host.set_current_camera(&invalid).is_err());
            assert!(
                world
                    .lock()
                    .unwrap()
                    .camera(second.id)
                    .unwrap()
                    .unwrap()
                    .active
            );
        }
        let mut js = JavaScriptBehavior::load(
            ScriptId::new(),
            b"globalThis.behavior={on_start(){Game.setCurrentCamera('First');}};",
            "camera.js",
            Box::new(make_host()),
            100_000,
        )
        .unwrap();
        js.on_start().unwrap();
        assert!(
            world
                .lock()
                .unwrap()
                .camera(first.id)
                .unwrap()
                .unwrap()
                .active
        );
        assert!(
            !world
                .lock()
                .unwrap()
                .camera(second.id)
                .unwrap()
                .unwrap()
                .active
        );
        host.set_current_camera(&second.id.to_string()).unwrap();
        host.set_current_camera(&second.id.to_string()).unwrap();
        assert_eq!(
            world
                .lock()
                .unwrap()
                .camera(first.id)
                .unwrap()
                .unwrap()
                .order,
            100
        );
    }

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
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Test".into(),
            startup_scripts: Vec::new(),
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
        assert_eq!(
            runtime.live_entity(entity_id).unwrap().unwrap().translation,
            [4.0, 0.0, 0.0]
        );
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
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Mixed".into(),
            startup_scripts: Vec::new(),
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
    fn lua_behavior_receives_play_keyboard_state() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let script_id = ScriptId::new();
        let scene = SceneDocument {
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Keyboard".into(),
            startup_scripts: Vec::new(),
            entities: vec![EntitySnapshot {
                scripts: vec![ScriptComponent::new(script_id)],
                ..EntitySnapshot::default()
            }],
            instances: Vec::new(),
        };
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![ScriptManifestEntry {
                    id: script_id,
                    language: ScriptLanguage::Lua54,
                    relative_path: "scripts/controller.lua".into(),
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                }],
            },
        )
        .unwrap();
        std::fs::write(
            temp.path().join("scripts/controller.lua"),
            b"return { FixedUpdate=function(dt) if rustic.key('KeyW').held then local x,y,z=rustic.get_translation(); rustic.set_translation(x,y,z-dt) end end }",
        )
        .unwrap();
        let scene_bytes = scene.to_bytes().unwrap();
        let mut runtime = RuntimeScripts::load(temp.path(), &scene_bytes).unwrap();
        runtime.fixed_update(1.0);
        assert!(
            runtime
                .runtime_changes("base", &scene_bytes)
                .unwrap()
                .changes
                .is_empty()
        );
        runtime.set_input_keys(vec!["KeyW".into()]);
        runtime.fixed_update(1.0);
        let changes = runtime.runtime_changes("base", &scene_bytes).unwrap();
        assert_eq!(
            changes.changes[0].after,
            RuntimeValue::Vector3([0.0, 0.0, -1.0])
        );
        runtime.set_input_keys(Vec::new());
        runtime.fixed_update(1.0);
        let changes = runtime.runtime_changes("base", &scene_bytes).unwrap();
        assert_eq!(
            changes.changes[0].after,
            RuntimeValue::Vector3([0.0, 0.0, -1.0])
        );
    }

    #[test]
    fn legacy_entry_script_is_not_required_or_run() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let scene = SceneDocument {
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Entry".into(),
            startup_scripts: Vec::new(),
            entities: vec![EntitySnapshot::default()],
            instances: Vec::new(),
        };
        std::fs::write(
            temp.path().join("settings.json"),
            br#"{"entry_script":"scripts/main.lua"}"#,
        )
        .unwrap();
        std::fs::write(
            temp.path().join("scripts/main.lua"),
            b"rustic.log('info', 'entry ran')",
        )
        .unwrap();

        let runtime = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap()).unwrap();
        assert!(runtime.drain_logs().is_empty());
    }

    #[test]
    fn object_behavior_runs_on_its_attached_entity_at_start() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let script_id = ScriptId::new();
        let camera = EntitySnapshot {
            name: Some("Camera".into()),
            ..EntitySnapshot::default()
        };
        let actor = EntitySnapshot {
            name: Some("Actor".into()),
            scripts: vec![ScriptComponent::new(script_id)],
            ..EntitySnapshot::default()
        };
        let scene = SceneDocument {
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Attached entry".into(),
            startup_scripts: Vec::new(),
            entities: vec![camera.clone(), actor.clone()],
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
        GameSettings::default().save(temp.path()).unwrap();
        std::fs::write(
            temp.path().join("scripts/main.lua"),
            b"return { on_start=function() rustic.set_translation(7, 0, 0) end }",
        )
        .unwrap();

        let runtime = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap()).unwrap();
        let changes = runtime
            .runtime_changes("base", &scene.to_bytes().unwrap())
            .unwrap();

        assert_eq!(changes.changes.len(), 1);
        assert_eq!(changes.changes[0].target.entity_id, actor.id.to_string());
        assert_ne!(changes.changes[0].target.entity_id, camera.id.to_string());
        assert_eq!(
            changes.changes[0].after,
            RuntimeValue::Vector3([7.0, 0.0, 0.0])
        );
    }

    #[test]
    fn linked_web_script_executes_on_its_attached_entity() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("ui")).unwrap();
        let script_id = ScriptId::new();
        let actor = EntitySnapshot {
            scripts: vec![ScriptComponent::new(script_id)],
            ..EntitySnapshot::default()
        };
        let scene = SceneDocument {
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Linked web script".into(),
            startup_scripts: Vec::new(),
            entities: vec![actor.clone()],
            instances: Vec::new(),
        };
        save_manifest_atomic(
            &temp.path().join("config/scripts.ron"),
            &ScriptManifest {
                format_version: 2,
                scripts: vec![ScriptManifestEntry {
                    id: script_id,
                    language: ScriptLanguage::Web,
                    relative_path: "ui/index.html".into(),
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                }],
            },
        )
        .unwrap();
        GameSettings::default().save(temp.path()).unwrap();
        std::fs::write(
            temp.path().join("ui/index.html"),
            br#"<!doctype html><html><body><script src="behavior.js"></script></body></html>"#,
        )
        .unwrap();
        std::fs::write(
            temp.path().join("ui/behavior.js"),
            b"globalThis.behavior={Start(){rustic.set_translation(9,0,0);}};",
        )
        .unwrap();

        let runtime = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap()).unwrap();
        let changes = runtime
            .runtime_changes("base", &scene.to_bytes().unwrap())
            .unwrap();

        assert_eq!(changes.changes.len(), 1);
        assert_eq!(changes.changes[0].target.entity_id, actor.id.to_string());
        assert_eq!(
            changes.changes[0].after,
            RuntimeValue::Vector3([9.0, 0.0, 0.0])
        );
    }

    #[test]
    fn object_script_api_errors_are_reported_without_panicking() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let script_id = ScriptId::new();
        let scene = SceneDocument {
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Entry API error".into(),
            startup_scripts: Vec::new(),
            entities: vec![EntitySnapshot {
                scripts: vec![ScriptComponent::new(script_id)],
                ..EntitySnapshot::default()
            }],
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
        GameSettings::default().save(temp.path()).unwrap();
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
    fn unattached_manifest_scripts_do_not_run() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("config")).unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let scene = SceneDocument {
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Configured entry".into(),
            startup_scripts: Vec::new(),
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
        GameSettings::default().save(temp.path()).unwrap();

        let runtime = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap()).unwrap();

        assert!(runtime.drain_logs().is_empty());
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
            environment: engine_world::SceneEnvironment::default(),
            id: engine_core::SceneId::new(),
            name: "Invalid settings".into(),
            startup_scripts: Vec::new(),
            entities: vec![EntitySnapshot::default()],
            instances: Vec::new(),
        };

        let error = RuntimeScripts::load(temp.path(), &scene.to_bytes().unwrap())
            .err()
            .expect("invalid settings must stop runtime startup");

        assert!(error.contains("invalid"));
        assert!(error.contains("settings.json"));
    }
    #[test]
    #[allow(
        clippy::too_many_lines,
        clippy::float_cmp,
        reason = "Language fixtures verify exactly representable scene values"
    )]
    fn high_level_gameplay_bindings_advance_in_shared_scene_runtime() {
        use engine_core::gameplay::{
            Ease, Value,
            animation::{Clip, Keyframe, Marker, Track, TrackInterpolation},
        };
        let fixtures: &[(ScriptLanguage,&str,&[u8])] = &[
            (ScriptLanguage::Lua54,"actions.lua",br"return {Start=function()
                assert(Smooth.lerp(0,8,0.5,Ease.InQuad)==2)
                Animation.value({{time=0,value=0,easing=Ease.InQuad},{time=2,value=8}},function(v) rustic.log('info','keyframe') end).onFinished(function() rustic.log('info','keyframesdone') end)
                Animation.play(nil,'Grow').onMarker('Mid',function() rustic.log('info','marker') end).onFinished(function() rustic.log('info','animation') end)
                Tween.move(rustic.game.Test.Actor,Vector3(8,0,0),2,Ease.InQuad).onFinished(function() rustic.log('info','done') end)
                Timer.after(0.5,function() rustic.log('info','timer') end)
            end}"),
            (ScriptLanguage::JavaScript,"actions.js",br#"globalThis.behavior={Start(){
                if(Smooth.lerp(0,8,0.5,Ease.InQuad)!==2)throw Error("core interpolation differs");Animation.value([{time:0,value:0,easing:Ease.InQuad},{time:2,value:8}],v=>rustic.log("info","keyframe")).onFinished(()=>rustic.log("info","keyframesdone"));Animation.play(null,"Grow").onMarker("Mid",()=>rustic.log("info","marker")).onFinished(()=>rustic.log("info","animation"));Tween.move(rustic.game.Test.Actor,[8,0,0],2,Ease.InQuad).onFinished(()=>rustic.log('info','done'));
                Timer.after(0.5,()=>rustic.log('info','timer'));
            }};"#),

            (ScriptLanguage::Luau,"actions.luau",br"return {Start=function()
                assert(Smooth.lerp(0,8,0.5,Ease.InQuad)==2)
                Animation.value({{time=0,value=0,easing=Ease.InQuad},{time=2,value=8}},function(v) rustic.log('info','keyframe') end).onFinished(function() rustic.log('info','keyframesdone') end)
                Animation.play(nil,'Grow').onMarker('Mid',function() rustic.log('info','marker') end).onFinished(function() rustic.log('info','animation') end)
                Tween.move(rustic.game.Test.Actor,Vector3(8,0,0),2,Ease.InQuad).onFinished(function() rustic.log('info','done') end)
                Timer.after(0.5,function() rustic.log('info','timer') end)
            end}"),
            (ScriptLanguage::Web,"actions.html",br#"<!doctype html><html><body><script>
                globalThis.behavior={Start(){if(Smooth.lerp(0,8,0.5,Ease.InQuad)!==2)throw Error("core interpolation differs");Animation.value([{time:0,value:0,easing:Ease.InQuad},{time:2,value:8}],v=>rustic.log("info","keyframe")).onFinished(()=>rustic.log("info","keyframesdone"));Animation.play(null,"Grow").onMarker("Mid",()=>rustic.log("info","marker")).onFinished(()=>rustic.log("info","animation"));Tween.move(rustic.game.Test.Actor,[8,0,0],2,Ease.InQuad).onFinished(()=>rustic.log('info','done'));Timer.after(0.5,()=>rustic.log('info','timer'));}};
            </script></body></html>"#),
            (ScriptLanguage::Python,"actions.py",br#"from rustic import rustic,run,Tween,Timer,Ease,Smooth,Animation
def start():
    assert Smooth.lerp(0,8,0.5,Ease.InQuad)==2
    Animation.value([dict(time=0,value=0,easing=Ease.InQuad),dict(time=2,value=8)],lambda v: rustic.log("info","keyframe")).onFinished(lambda: rustic.log("info","keyframesdone"))
    Animation.play(None,"Grow").onMarker("Mid",lambda: rustic.log("info","marker")).onFinished(lambda: rustic.log("info","animation"))
    Tween.move(rustic.game.Test.Actor,[8,0,0],2,Ease.InQuad).onFinished(lambda: rustic.log('info','done'))
    Timer.after(0.5,lambda: rustic.log('info','timer'))
run({'on_start':start})
"#),
            (ScriptLanguage::C,"actions.c",br#"#include "rustic.h"
static void done(void){rustic.log("info","done");}static void timer(void){rustic.log("info","timer");}
static void marker(void){rustic.log("info","marker");}static void animation(void){rustic.log("info","animation");}
static void sample(RusticValue v){(void)v;rustic.log("info","keyframe");}static void keyframesdone(void){rustic.log("info","keyframesdone");}
static void start(void){RusticKeyframe keys[]={{0,{.type=RUSTIC_NUMBER,.number=0},"InQuad"},{2,{.type=RUSTIC_NUMBER,.number=8},NULL}};Animation.value(keys,2,sample,keyframesdone);RusticGameplayHandle a=Animation.play(NULL,"Grow",NULL,animation);Animation.onMarker(a,"Mid",marker);RusticValue zero={.type=RUSTIC_NUMBER,.number=0},eight={.type=RUSTIC_NUMBER,.number=8};if(Smooth.lerp(zero,eight,0.5,Ease.InQuad).number!=2)exit(2);Tween.move(NULL,(RusticVector3){8,0,0},2,Ease.InQuad,done);Timer.after(0.5,timer);}
int main(void){return rustic_run((RusticBehavior){.on_start=start});}"#),
            (ScriptLanguage::Cpp,"actions.cpp",br#"#include "rustic.hpp"
int main(){RusticBehavior b;b.on_start=[](){Animation::value({{0,0,Ease::InQuad},{2,8}},[](const RusticValue&){rustic.log("info","keyframe");})->onFinished([](){rustic.log("info","keyframesdone");});Animation::play("","Grow")->onMarker("Mid",[](){rustic.log("info","marker");}).onFinished([](){rustic.log("info","animation");});if(Smooth::lerp(0,8,0.5,Ease::InQuad).number()!=2)throw std::runtime_error("core interpolation differs");Tween::move("",{8,0,0},2,Ease::InQuad)->onFinished([](){rustic.log("info","done");});Timer::after(0.5,[](){rustic.log("info","timer");});};return rustic_run(b);}"#),
            (ScriptLanguage::CSharp,"actions.cs",br#"using static Rustic;
Run((callback,dt)=>{if(callback=="on_start"){Animation.value(new Animation.Key[]{new(0,0,Ease.InQuad),new(2,8)},v=>rustic.log("info","keyframe")).onFinished(()=>rustic.log("info","keyframesdone"));Animation.play(null,"Grow").onMarker("Mid",()=>rustic.log("info","marker")).onFinished(()=>rustic.log("info","animation"));if(Smooth.lerp(0.0,8.0,0.5,Ease.InQuad)!=2)throw new Exception("core interpolation differs");Tween.move(null,new double[]{8,0,0},2,Ease.InQuad).onFinished(()=>rustic.log("info","done"));Timer.after(0.5,()=>rustic.log("info","timer"));}});"#),
            (ScriptLanguage::Java,"RusticBehavior.java",br#"class RusticBehavior extends Rustic {
public static void main(String[]args)throws Exception{run((callback,dt)->{if(callback.equals("on_start")){Animation.value(java.util.List.of(new Animation.Key(0,0,Ease.InQuad),new Animation.Key(2,8)),v->rustic.log("info","keyframe")).onFinished(()->rustic.log("info","keyframesdone"));Animation.play(null,"Grow",null).onMarker("Mid",()->rustic.log("info","marker")).onFinished(()->rustic.log("info","animation"));if(((Number)Smooth.lerp(0,8,0.5,Ease.InQuad)).doubleValue()!=2)throw new Exception("core interpolation differs");Tween.move(null,new double[]{8,0,0},2,Ease.InQuad).onFinished(()->rustic.log("info","done"));Timer.after(0.5,()->rustic.log("info","timer"));}});}}"#),
            (ScriptLanguage::Php,"actions.php",br#"<?php
require __DIR__."/rustic.php";
function start():void{global $rustic;Animation::value([["time"=>0,"value"=>0,"easing"=>Ease::InQuad],["time"=>2,"value"=>8]],fn($v)=>$rustic->log("info","keyframe"))->onFinished(fn()=>$rustic->log("info","keyframesdone"));Animation::play(null,"Grow")->onMarker("Mid",fn()=>$rustic->log("info","marker"))->onFinished(fn()=>$rustic->log("info","animation"));if(Smooth::lerp(0,8,0.5,Ease::InQuad)!=2)throw new RuntimeException("core interpolation differs");Tween::move(null,[8,0,0],2,Ease::InQuad)->onFinished(fn()=>$rustic->log('info','done'));Timer::after(0.5,fn()=>$rustic->log('info','timer'));}
rustic_run(['on_start'=>'start']);"#),
        ];
        for (language, name, source) in fixtures {
            if !matches!(language, ScriptLanguage::Lua54 | ScriptLanguage::JavaScript)
                && !engine_scripting::probe_language_toolchain(*language).available
            {
                assert!(
                    std::env::var_os("RUSTIC_REQUIRE_ALL_SDKS").is_none(),
                    "{} toolchain is required",
                    language.display_name()
                );
                continue;
            }
            eprintln!("Executing gameplay facade: {}", language.display_name());
            let entity = EntitySnapshot {
                name: Some("Actor".into()),
                ..Default::default()
            };
            let mut scene = SceneDocument::new("Test");
            scene.entities.push(entity.clone());
            let world = Arc::new(Mutex::new(scene.create_world().unwrap()));
            let gameplay = Arc::new(Mutex::new(GameplayContext::default()));
            gameplay.lock().unwrap().clips.insert(
                (entity.id, "Grow".into()),
                Clip {
                    name: "Grow".into(),
                    duration: 2.0,
                    tracks: vec![Track {
                        target: "Scale".into(),
                        interpolation: TrackInterpolation::default(),
                        keys: vec![
                            Keyframe {
                                time: 0.0,
                                value: Value::Vector([1.; 3]),
                                easing: Ease::default(),
                            },
                            Keyframe {
                                time: 2.0,
                                value: Value::Vector([3.; 3]),
                                easing: Ease::default(),
                            },
                        ],
                    }],
                    markers: vec![Marker {
                        time: 1.0,
                        name: "Mid".into(),
                    }],
                },
            );
            let logs = Arc::new(Mutex::new(VecDeque::new()));
            let host = RuntimeHost {
                gameplay: Arc::clone(&gameplay),
                owner_script: ScriptId::new(),
                entity_bound: true,
                scene_name: "Test".into(),
                world: Arc::clone(&world),
                input_keys: Arc::new(Mutex::new(BTreeSet::new())),
                snapshot_root: PathBuf::new(),
                entity: entity.id,
                properties: Arc::new(Mutex::new(BTreeMap::new())),
                logs: Arc::clone(&logs),
                dropped_logs: Arc::new(Mutex::new(0)),
                enabled: true,
            };
            let mut behavior =
                RuntimeBehavior::load(*language, ScriptId::new(), source, name, Box::new(host))
                    .unwrap_or_else(|e| panic!("{}: {e}", language.display_name()));
            behavior
                .on_start()
                .unwrap_or_else(|e| panic!("{} Start: {e}", language.display_name()));
            {
                let mut context = gameplay.lock().unwrap();
                let mut scene = world.lock().unwrap();
                context
                    .runtime
                    .tick(
                        1.0,
                        &mut crate::gameplay_world::GameplayScene(
                            &mut scene,
                            &mut engine_core::gameplay::audio::Mixer::default(),
                        ),
                    )
                    .unwrap();
                assert_eq!(
                    scene.local_transform(entity.id).unwrap().translation.x,
                    2.0,
                    "{}",
                    language.display_name()
                );
            }
            behavior.update(1.0).unwrap();
            assert!(
                logs.lock().unwrap().iter().any(|(_, m)| m == "timer"),
                "{} timer missing",
                language.display_name()
            );
            {
                let mut context = gameplay.lock().unwrap();
                let mut scene = world.lock().unwrap();
                context
                    .runtime
                    .tick(
                        1.0,
                        &mut crate::gameplay_world::GameplayScene(
                            &mut scene,
                            &mut engine_core::gameplay::audio::Mixer::default(),
                        ),
                    )
                    .unwrap();
                assert_eq!(scene.local_transform(entity.id).unwrap().translation.x, 8.0);
            }
            behavior.update(1.0).unwrap();
            assert!(
                logs.lock().unwrap().iter().any(|(_, m)| m == "done"),
                "{} completion missing",
                language.display_name()
            );
            assert_eq!(
                world
                    .lock()
                    .unwrap()
                    .local_transform(entity.id)
                    .unwrap()
                    .scale
                    .x,
                3.0,
                "{} animation pose",
                language.display_name()
            );
            for message in ["marker", "animation", "keyframe", "keyframesdone"] {
                assert!(
                    logs.lock().unwrap().iter().any(|(_, m)| m == message),
                    "{} missing {} callback",
                    language.display_name(),
                    message
                );
            }
            behavior.cleanup_gameplay();
            assert!(
                gameplay
                    .lock()
                    .unwrap()
                    .runtime
                    .states_for_entities()
                    .is_empty()
            );
        }
    }
    #[test]
    fn lua_paths_values_and_cross_language_signals_use_shared_core() {
        if !engine_scripting::probe_language_toolchain(ScriptLanguage::Python).available {
            return;
        }
        let entity = EntitySnapshot::default();
        let mut scene = SceneDocument::new("Test");
        scene.entities.push(entity.clone());
        let world = Arc::new(Mutex::new(scene.create_world().unwrap()));
        let gameplay = Arc::new(Mutex::new(GameplayContext::default()));
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        let make_host = || RuntimeHost {
            gameplay: Arc::clone(&gameplay),
            owner_script: ScriptId::new(),
            entity_bound: true,
            scene_name: "Test".into(),
            world: Arc::clone(&world),
            input_keys: Arc::new(Mutex::new(BTreeSet::new())),
            snapshot_root: PathBuf::new(),
            entity: entity.id,
            properties: Arc::new(Mutex::new(BTreeMap::new())),
            logs: Arc::clone(&logs),
            dropped_logs: Arc::new(Mutex::new(0)),
            enabled: true,
        };
        let mut lua=RuntimeBehavior::load(ScriptLanguage::Lua54,ScriptId::new(),br"return {Start=function()
            Events.once('Hit',function(id,message) assert(id==rustic.entity_id());rustic.log('info',message) end)
            Path.follow(nil,Path.create({Vector3(0,0,0),Vector3(8,0,0)}),{duration=2,easing=Ease.InQuad})
            Tween.value(0,8,2,function(value) rustic.log('info','value '..value) end,Ease.InQuad)
        end}","actions.lua",Box::new(make_host())).unwrap();
        lua.on_start().unwrap();
        let mut python = RuntimeBehavior::load(
            ScriptLanguage::Python,
            ScriptId::new(),
            br"from rustic import rustic,run,Events
def start(): Events.emit('Hit',[rustic.entity_id(),'cross-language'])
run({'on_start':start})
",
            "signal.py",
            Box::new(make_host()),
        )
        .unwrap();
        python.on_start().unwrap();
        {
            let mut context = gameplay.lock().unwrap();
            let mut scene = world.lock().unwrap();
            context
                .runtime
                .tick(
                    1.0,
                    &mut crate::gameplay_world::GameplayScene(
                        &mut scene,
                        &mut engine_core::gameplay::audio::Mixer::default(),
                    ),
                )
                .unwrap();
            assert!((scene.local_transform(entity.id).unwrap().translation.x - 2.0).abs() < 1e-6);
        }
        lua.update(1.0).unwrap();
        let entries = logs.lock().unwrap();
        assert!(entries.iter().any(|(_, m)| m == "cross-language"));
        assert!(
            entries
                .iter()
                .any(|(_, m)| m == "value 2.0" || m == "value 2")
        );
    }
}
